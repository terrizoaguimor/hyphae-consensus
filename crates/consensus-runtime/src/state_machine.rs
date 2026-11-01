// SPDX-License-Identifier: Apache-2.0
//! Deterministic run state machine.
//!
//! `apply` is a pure transition function over the aggregate: the same
//! event sequence always yields the same aggregate, which is what makes
//! log replay a faithful rebuild. Authorization transitions additionally
//! require a human actor, so no agent or runtime component can approve
//! its own run.
//!
//! Protocol 1.3 (records of schema 1.3 and later): the aggregate derives
//! from the events what each evaluation must judge, the run's open
//! objections, retrieved records and uncleared contradictions, and
//! `apply` checks every `EvaluationSubmitted`, challenge and proposal
//! against the [`contract`], exactly as the orchestrator did before
//! recording them. It also re-derives the runtime's own verification
//! records from the recorded evaluation and requires one per retrieved
//! record before a recommendation. Earlier records are tracked the same
//! way but never checked: they replay exactly as before.
//!
//! Because each record is checked by its own schema version, a run's
//! records never go back to an earlier version
//! ([`TransitionError::SchemaDowngrade`]): one record of a 1.3 run
//! relabeled 1.2 would otherwise skip the very check that catches its
//! forgery. Honest runs only move up, e.g. a newer binary recording a
//! decision on an archived run.

use crate::contract::{self, ContractViolation, EvaluationContext, OpenItem};
use consensus_domain::*;
use thiserror::Error;

/// Maximum proposer revisions per run. Bounds debate; no endless loops.
pub const MAX_REVISION_ROUNDS: u32 = 2;

#[derive(Debug, Error, PartialEq)]
pub enum TransitionError {
    #[error("event {event} is not allowed in state {state:?}")]
    InvalidTransition {
        state: RunState,
        event: &'static str,
    },
    #[error("run is terminal ({state:?}); no further events accepted")]
    Terminal { state: RunState },
    #[error("only a human actor may record an authorization decision")]
    NotHuman,
    #[error("revision round limit ({MAX_REVISION_ROUNDS}) reached")]
    RoundLimit,
    #[error("event run_id does not match aggregate run_id")]
    RunMismatch,
    #[error("duplicate task creation for run")]
    DuplicateTask,
    #[error("abstained runs may only receive an abstained, blocked, or contested recommendation")]
    AbstainKindMismatch,
    #[error("a revision may follow an abstention only while challenge items remain unresolved")]
    AbstainedWithoutOpenItems,
    #[error("stored recommendation {stored:?} does not match derived {derived:?}")]
    RecommendationMismatch {
        stored: RecommendationKind,
        derived: RecommendationKind,
    },
    /// A schema 1.3+ record breaks the protocol 1.3 contract.
    #[error("protocol 1.3 contract: {0}")]
    Contract(#[from] ContractViolation),
    /// A record ranks below a schema version the run already recorded
    /// ([`contract::schema_rank`]).
    #[error(
        "a schema {to:?} record follows a schema {from:?} record of the same run; a run's schema version never goes down"
    )]
    SchemaDowngrade { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunAggregate {
    pub run_id: RunId,
    pub state: RunState,
    pub task: Option<Task>,
    pub current_proposal: Option<ProposalId>,
    pub revision_round: u32,
    pub recommendation: Option<Recommendation>,
    pub authorization: AuthorizationState,
    pub failure_reason: Option<String>,
    pub verifications: Vec<EvidenceVerification>,
    pub invocations: Vec<ModelInvocation>,
    pub event_count: u64,
    /// Latest evaluation and the verifications recorded after it —
    /// exactly the inputs the orchestrator's policy derivation used.
    /// Replay recomputes the recommendation from these (schema 1.1+).
    pub last_evaluation: Option<Evaluation>,
    pub round_verifications: Vec<EvidenceVerification>,
    /// Claim ids of the current proposal (the submitted one, then each
    /// revision's), in proposal order.
    pub claims: Vec<ClaimId>,
    /// Items raised since the latest evaluation, with their round; the
    /// next evaluation judges them after `open_items`.
    pub round_items: Vec<OpenItem>,
    /// Every challenge item id recorded in the run, in order. From schema
    /// 1.3 item ids are non-empty and never reused.
    pub items_seen: Vec<String>,
    /// Items the latest evaluation left open (its `unresolved` verdicts,
    /// in verdict order), each with the round it was raised in. Always
    /// empty after a pre-1.3 evaluation, which carries no verdicts (its
    /// open items are its own `unresolved_items`).
    pub open_items: Vec<OpenItem>,
    /// Every record with an `EvidenceRetrieved` event in the run, in
    /// retrieval order (the first record per id). Protocol 1.3's round
    /// context.
    pub retrieved: Vec<EvidenceReference>,
    /// Records an evaluation listed as contradicting a claim, minus those
    /// a later evaluation cleared, in order of first contradiction.
    pub contradicted: Vec<EvidenceId>,
    /// The highest schema version among the run's records so far, in
    /// [`contract::schema_rank`] order; no later record may rank below it.
    pub highest_schema_version: Option<String>,
}

impl RunAggregate {
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            state: RunState::Created,
            task: None,
            current_proposal: None,
            revision_round: 0,
            recommendation: None,
            authorization: AuthorizationState::Pending,
            failure_reason: None,
            verifications: Vec::new(),
            invocations: Vec::new(),
            event_count: 0,
            last_evaluation: None,
            round_verifications: Vec::new(),
            claims: Vec::new(),
            round_items: Vec::new(),
            items_seen: Vec::new(),
            open_items: Vec::new(),
            retrieved: Vec::new(),
            contradicted: Vec::new(),
            highest_schema_version: None,
        }
    }

    /// The deliberation round in progress (1 = first): one more than the
    /// revisions recorded so far.
    pub fn round(&self) -> u32 {
        self.revision_round + 1
    }

    /// What an evaluation recorded now must satisfy (protocol 1.3): the
    /// current claims, the open items followed by this round's items, the
    /// retrieved records and the uncleared contradictions. `apply` checks
    /// a 1.3 `EvaluationSubmitted` against exactly this, and the
    /// orchestrator checks before recording against the same.
    pub fn evaluation_context(&self) -> EvaluationContext {
        EvaluationContext {
            claims: self.claims.clone(),
            expected_items: self
                .open_items
                .iter()
                .chain(&self.round_items)
                .map(|o| o.item.id.clone())
                .collect(),
            retrieved: self.retrieved.iter().map(|e| e.id.clone()).collect(),
            carried_contradictions: self.contradicted.clone(),
        }
    }

    pub fn apply(&mut self, event: &ConsensusEvent) -> Result<(), TransitionError> {
        if event.run_id != self.run_id {
            return Err(TransitionError::RunMismatch);
        }
        if self.state.is_terminal() {
            return Err(TransitionError::Terminal { state: self.state });
        }
        // The checks below follow each record's own schema version, so a
        // run's records may keep or raise it but never lower it.
        let rank = contract::schema_rank(&event.schema_version);
        if let Some(highest) = &self.highest_schema_version {
            if rank < contract::schema_rank(highest) {
                return Err(TransitionError::SchemaDowngrade {
                    from: highest.clone(),
                    to: event.schema_version.clone(),
                });
            }
        }
        // Protocol 1.3 checks apply to records of schema 1.3 and later.
        let contract = contract::binds(&event.schema_version);
        match &event.payload {
            EventPayload::TaskCreated { task } => {
                if self.task.is_some() {
                    return Err(TransitionError::DuplicateTask);
                }
                self.task = Some(task.clone());
            }
            EventPayload::ProposalSubmitted {
                proposal,
                invocation,
            } => {
                self.require(&[RunState::Created], "proposal_submitted")?;
                if self.task.is_none() {
                    return Err(TransitionError::InvalidTransition {
                        state: self.state,
                        event: "proposal_submitted",
                    });
                }
                if contract {
                    contract::validate_claims(proposal)?;
                }
                self.current_proposal = Some(proposal.id.clone());
                self.claims = proposal.claims.iter().map(|c| c.id.clone()).collect();
                self.invocations.push(invocation.clone());
                self.state = RunState::Proposed;
            }
            EventPayload::EvidenceRetrieved { evidence } => {
                self.require(
                    &[
                        RunState::Proposed,
                        RunState::EvidenceReview,
                        RunState::Revised,
                    ],
                    "evidence_retrieved",
                )?;
                if !self.retrieved.iter().any(|e| e.id == evidence.id) {
                    self.retrieved.push(evidence.clone());
                }
                self.state = RunState::EvidenceReview;
            }
            EventPayload::EvidenceUnavailable { .. } | EventPayload::RecallProven { .. } => {
                self.require(
                    &[
                        RunState::Proposed,
                        RunState::EvidenceReview,
                        RunState::Revised,
                    ],
                    "evidence_retrieved",
                )?;
                self.state = RunState::EvidenceReview;
            }
            EventPayload::EvidenceVerified { verification } => {
                self.require(
                    &[
                        RunState::Proposed,
                        RunState::EvidenceReview,
                        RunState::Evaluated,
                        RunState::Revised,
                        RunState::Abstained,
                    ],
                    "evidence_verified",
                )?;
                // Protocol 1.3: the runtime derives each verification from
                // the round's recorded evaluation, after it, one per record
                // retrieved so far, so replay re-derives it (all but the
                // integrity status, which only the verifier can give).
                if contract {
                    self.require(
                        &[RunState::Evaluated, RunState::Abstained],
                        "evidence_verified",
                    )?;
                    let evaluation = self.last_evaluation.as_ref().ok_or(
                        TransitionError::InvalidTransition {
                            state: self.state,
                            event: "evidence_verified",
                        },
                    )?;
                    contract::validate_verification(
                        self.round(),
                        &self.retrieved,
                        &self.round_verifications,
                        evaluation,
                        verification,
                    )?;
                }
                self.verifications.push(verification.clone());
                self.round_verifications.push(verification.clone());
            }
            EventPayload::ChallengeSubmitted {
                challenge,
                invocation,
            } => {
                self.require(
                    &[
                        RunState::Proposed,
                        RunState::EvidenceReview,
                        RunState::Revised,
                    ],
                    "challenge_submitted",
                )?;
                if contract {
                    contract::validate_challenge(&self.items_seen, challenge)?;
                }
                let round = self.round();
                for item in &challenge.items {
                    self.items_seen.push(item.id.clone());
                    self.round_items.push(OpenItem {
                        item: item.clone(),
                        raised_in_round: round,
                    });
                }
                self.invocations.push(invocation.clone());
                self.state = RunState::EvidenceReview;
            }
            EventPayload::EvaluationSubmitted {
                evaluation,
                invocation,
            } => {
                self.require(&[RunState::EvidenceReview], "evaluation_submitted")?;
                // Protocol 1.3: the evaluation must judge what the events
                // so far say is open, so a forged 1.3 log fails replay.
                if contract {
                    contract::validate_evaluation(&self.evaluation_context(), evaluation)?;
                }
                self.invocations.push(invocation.clone());
                self.last_evaluation = Some(evaluation.clone());
                self.round_verifications.clear();
                // What stays open is what this evaluation left unresolved,
                // carried items and this round's alike.
                let judged: Vec<OpenItem> = self
                    .open_items
                    .drain(..)
                    .chain(self.round_items.drain(..))
                    .collect();
                self.open_items = evaluation
                    .item_verdicts
                    .iter()
                    .filter(|v| v.status == ItemStatus::Unresolved)
                    .filter_map(|v| judged.iter().find(|o| o.item.id == v.item_id).cloned())
                    .collect();
                self.contradicted.retain(|id| {
                    !evaluation
                        .cleared_contradictions
                        .iter()
                        .any(|c| &c.evidence_id == id)
                });
                for id in evaluation
                    .assessments
                    .iter()
                    .flat_map(|a| &a.contradicting_evidence)
                {
                    if !self.contradicted.contains(id) {
                        self.contradicted.push(id.clone());
                    }
                }
                self.state = if evaluation.abstain {
                    RunState::Abstained
                } else {
                    RunState::Evaluated
                };
            }
            EventPayload::RevisionSubmitted {
                revision,
                invocation,
            } => {
                self.require(
                    &[RunState::Evaluated, RunState::Abstained],
                    "revision_submitted",
                )?;
                // Schema 1.2: an abstention that still lists unresolved
                // items may be answered by a revision; one with nothing
                // unresolved ends deliberation.
                if self.state == RunState::Abstained
                    && self
                        .last_evaluation
                        .as_ref()
                        .is_none_or(|e| e.unresolved_items.is_empty())
                {
                    return Err(TransitionError::AbstainedWithoutOpenItems);
                }
                if self.revision_round >= MAX_REVISION_ROUNDS {
                    return Err(TransitionError::RoundLimit);
                }
                if contract {
                    contract::validate_claims(&revision.proposal)?;
                }
                self.revision_round += 1;
                self.current_proposal = Some(revision.proposal.id.clone());
                self.claims = revision
                    .proposal
                    .claims
                    .iter()
                    .map(|c| c.id.clone())
                    .collect();
                self.invocations.push(invocation.clone());
                self.state = RunState::Revised;
            }
            EventPayload::RecommendationIssued { recommendation } => {
                self.require(
                    &[RunState::Evaluated, RunState::Abstained],
                    "recommendation_issued",
                )?;
                // An invalid proof and (since 1.2) a recorded contradiction
                // outrank abstention in the policy, so an abstained run may
                // still be Blocked or Contested — but never upgraded.
                if self.state == RunState::Abstained
                    && !matches!(
                        recommendation.kind,
                        RecommendationKind::Abstained
                            | RecommendationKind::Blocked
                            | RecommendationKind::Contested
                    )
                {
                    return Err(TransitionError::AbstainKindMismatch);
                }
                // Protocol 1.3: the policy reads one verification of every
                // retrieved record for this round; one left out (an invalid
                // proof, say) would silently change its inputs.
                if contract {
                    contract::validate_round_verified(&self.retrieved, &self.round_verifications)?;
                }
                // Schema 1.1+: never trust the stored kind — recompute it
                // from the tracked evaluation + this round's verification
                // records with the rules of the record's own schema
                // version, exactly as the orchestrator derived it. A log
                // edited to upgrade its own outcome fails replay. Pre-1.1
                // records predate recomputation and skip it.
                if let Some(policy) =
                    crate::policy::PolicyVersion::for_schema(&event.schema_version)
                {
                    if let Some(eval) = &self.last_evaluation {
                        let derived = crate::policy::derive_recommendation_with(
                            policy,
                            eval,
                            &self.round_verifications,
                        );
                        if derived != recommendation.kind {
                            return Err(TransitionError::RecommendationMismatch {
                                stored: recommendation.kind,
                                derived,
                            });
                        }
                    }
                }
                self.recommendation = Some(recommendation.clone());
                self.state = RunState::AwaitingHumanApproval;
            }
            EventPayload::HumanDecisionRecorded { authorization } => {
                if !matches!(event.actor, Actor::Human { .. }) {
                    return Err(TransitionError::NotHuman);
                }
                self.require(&[RunState::AwaitingHumanApproval], "human_decision")?;
                if authorization.approved {
                    self.authorization = AuthorizationState::Approved;
                    self.state = RunState::Approved;
                } else {
                    self.authorization = AuthorizationState::Rejected;
                    self.state = RunState::Rejected;
                }
            }
            EventPayload::RunFailed {
                reason, invocation, ..
            } => {
                // A call whose document was refused still happened.
                if let Some(invocation) = invocation {
                    self.invocations.push(invocation.clone());
                }
                self.failure_reason = Some(reason.clone());
                self.state = RunState::Failed;
            }
        }
        if self
            .highest_schema_version
            .as_deref()
            .is_none_or(|highest| rank > contract::schema_rank(highest))
        {
            self.highest_schema_version = Some(event.schema_version.clone());
        }
        self.event_count += 1;
        Ok(())
    }

    fn require(&self, allowed: &[RunState], event: &'static str) -> Result<(), TransitionError> {
        if allowed.contains(&self.state) {
            Ok(())
        } else {
            Err(TransitionError::InvalidTransition {
                state: self.state,
                event,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(run: &RunId) -> Task {
        Task {
            schema_version: SCHEMA_VERSION.into(),
            id: TaskId::new("task-1"),
            run_id: run.clone(),
            project_id: ProjectId::new("p"),
            title: "t".into(),
            description: "d".into(),
            created_at: "2026-10-26T00:00:00Z".into(),
            mode: None,
        }
    }

    fn ev(run: &RunId, seq: u64, actor: Actor, payload: EventPayload) -> ConsensusEvent {
        ConsensusEvent {
            schema_version: SCHEMA_VERSION.into(),
            sequence: seq,
            event_id: EventId::new(format!("evt-{seq}")),
            run_id: run.clone(),
            actor,
            occurred_at: "2026-10-26T00:00:00Z".into(),
            prev_hash: String::new(),
            hash: String::new(),
            payload,
        }
    }

    /// One claim, `c1`: the claim the tests' evaluations assess (protocol
    /// 1.3 refuses an assessment of a claim the proposal does not make).
    fn proposal(run: &RunId) -> Proposal {
        Proposal {
            schema_version: SCHEMA_VERSION.into(),
            id: ProposalId::new("prop-1"),
            run_id: run.clone(),
            author_role: AgentRole::Proposer,
            summary: "s".into(),
            plan_steps: vec![],
            claims: vec![Claim {
                id: ClaimId::new("c1"),
                statement: "Retries are capped at 3 attempts".into(),
                evidence_refs: vec![],
            }],
            assumptions: vec![],
            uncertainties: vec![],
            created_at: "2026-10-26T00:00:00Z".into(),
        }
    }

    fn invocation() -> ModelInvocation {
        ModelInvocation {
            provider: "fake".into(),
            model: "muse-wonder-1.0".into(),
            request_id: None,
            latency_ms: None,
            input_tokens: None,
            output_tokens: None,
            finish_reason: None,
            reasoning_tokens: None,
            max_completion_tokens: None,
            schema_name: None,
            schema_sha256: None,
            prompt_sha256: None,
            reasoning_effort: None,
            safety_identifier_sent: None,
            attempts: None,
            rejected_calls: None,
            corrective_sha256: None,
        }
    }

    fn auth(run: &RunId, approved: bool) -> HumanAuthorization {
        HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: run.clone(),
            authorized_by: "mario".into(),
            approved,
            decided_at: "2026-10-26T00:00:00Z".into(),
            idempotency_key: "k1".into(),
            note: None,
            authority: None,
        }
    }

    fn to_awaiting() -> (RunAggregate, RunId) {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        agg.apply(&ev(
            &run,
            1,
            Actor::Human { id: "mario".into() },
            EventPayload::TaskCreated { task: task(&run) },
        ))
        .unwrap();
        agg.apply(&ev(
            &run,
            2,
            Actor::Agent {
                role: AgentRole::Proposer,
            },
            EventPayload::ProposalSubmitted {
                proposal: proposal(&run),
                invocation: invocation(),
            },
        ))
        .unwrap();
        agg.apply(&ev(
            &run,
            3,
            Actor::Agent {
                role: AgentRole::EvidenceEvaluator,
            },
            EventPayload::EvaluationSubmitted {
                evaluation: Evaluation {
                    schema_version: SCHEMA_VERSION.into(),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    assessments: vec![],
                    unresolved_items: vec![],
                    item_verdicts: vec![],
                    cleared_contradictions: vec![],
                    abstain: false,
                    abstain_reason: None,
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            },
        ))
        .unwrap_err(); // evaluation requires EvidenceReview first
        (agg, run)
    }

    #[test]
    fn replay_recomputes_recommendation_and_rejects_forged_kind() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        let agent = |role| Actor::Agent { role };
        agg.apply(&ev(
            &run,
            1,
            Actor::Human { id: "mario".into() },
            EventPayload::TaskCreated { task: task(&run) },
        ))
        .unwrap();
        agg.apply(&ev(
            &run,
            2,
            agent(AgentRole::Proposer),
            EventPayload::ProposalSubmitted {
                proposal: proposal(&run),
                invocation: invocation(),
            },
        ))
        .unwrap();
        agg.apply(&ev(
            &run,
            3,
            agent(AgentRole::Critic),
            EventPayload::ChallengeSubmitted {
                challenge: Challenge {
                    schema_version: SCHEMA_VERSION.into(),
                    id: ChallengeId::new("chal-1"),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    items: vec![],
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            },
        ))
        .unwrap();
        // Uncorroborated claim, no evidence: policy derives
        // InsufficientEvidence. A stored Supported must fail replay.
        agg.apply(&ev(
            &run,
            4,
            agent(AgentRole::EvidenceEvaluator),
            EventPayload::EvaluationSubmitted {
                evaluation: Evaluation {
                    schema_version: SCHEMA_VERSION.into(),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    assessments: vec![ClaimAssessment {
                        claim_id: ClaimId::new("c1"),
                        corroborated: false,
                        relevant_evidence: vec![],
                        contradicting_evidence: vec![],
                        note: "no evidence".into(),
                    }],
                    unresolved_items: vec![],
                    item_verdicts: vec![],
                    cleared_contradictions: vec![],
                    abstain: false,
                    abstain_reason: None,
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            },
        ))
        .unwrap();
        let rec = |kind| EventPayload::RecommendationIssued {
            recommendation: Recommendation {
                schema_version: SCHEMA_VERSION.into(),
                kind,
                rationale: "r".into(),
                unresolved_assumptions: vec![],
                unresolved_uncertainties: vec![],
                issued_at: "2026-10-26T00:00:00Z".into(),
            },
        };
        let err = agg
            .apply(&ev(
                &run,
                5,
                Actor::Runtime,
                rec(RecommendationKind::Supported),
            ))
            .unwrap_err();
        assert_eq!(
            err,
            TransitionError::RecommendationMismatch {
                stored: RecommendationKind::Supported,
                derived: RecommendationKind::InsufficientEvidence,
            }
        );
        agg.apply(&ev(
            &run,
            5,
            Actor::Runtime,
            rec(RecommendationKind::InsufficientEvidence),
        ))
        .unwrap();
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
    }

    fn item(id: &str) -> ChallengeItem {
        ChallengeItem {
            id: id.into(),
            kind: ChallengeKind::ConstraintConflict,
            claim_id: Some(ClaimId::new("c1")),
            description: "Ten attempts break the documented cap".into(),
            evidence_refs: vec![EvidenceId::new("ev1")],
        }
    }

    /// Task, proposal, the retrieved record ev1, a challenge raising the
    /// `unresolved` items, and an abstaining evaluation that keeps them
    /// unresolved (one explicit verdict each); the aggregate ends in
    /// `Abstained`.
    fn to_abstained(unresolved: &[&str], schema: &str) -> (RunAggregate, RunId) {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        let agent = |role| Actor::Agent { role };
        let at = |seq, actor, payload| {
            let mut e = ev(&run, seq, actor, payload);
            e.schema_version = schema.into();
            e
        };
        agg.apply(&at(
            1,
            Actor::Human { id: "mario".into() },
            EventPayload::TaskCreated { task: task(&run) },
        ))
        .unwrap();
        agg.apply(&at(
            2,
            agent(AgentRole::Proposer),
            EventPayload::ProposalSubmitted {
                proposal: proposal(&run),
                invocation: invocation(),
            },
        ))
        .unwrap();
        agg.apply(&at(
            3,
            Actor::Runtime,
            EventPayload::EvidenceRetrieved {
                evidence: record("ev1"),
            },
        ))
        .unwrap();
        agg.apply(&at(
            4,
            agent(AgentRole::Critic),
            EventPayload::ChallengeSubmitted {
                challenge: Challenge {
                    schema_version: schema.into(),
                    id: ChallengeId::new("chal-1"),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    items: unresolved.iter().map(|id| item(id)).collect(),
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            },
        ))
        .unwrap();
        agg.apply(&at(
            5,
            agent(AgentRole::EvidenceEvaluator),
            EventPayload::EvaluationSubmitted {
                evaluation: Evaluation {
                    schema_version: schema.into(),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    assessments: vec![ClaimAssessment {
                        claim_id: ClaimId::new("c1"),
                        corroborated: false,
                        relevant_evidence: vec![],
                        contradicting_evidence: vec![EvidenceId::new("ev1")],
                        note: "conflicts with the stored cap".into(),
                    }],
                    unresolved_items: unresolved.iter().map(|s| s.to_string()).collect(),
                    item_verdicts: unresolved
                        .iter()
                        .map(|id| ItemVerdict {
                            item_id: id.to_string(),
                            status: ItemStatus::Unresolved,
                            reason: "cannot judge".into(),
                        })
                        .collect(),
                    cleared_contradictions: vec![],
                    abstain: true,
                    abstain_reason: Some("insufficient".into()),
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            },
        ))
        .unwrap();
        assert_eq!(agg.state, RunState::Abstained);
        (agg, run)
    }

    fn record(id: &str) -> EvidenceReference {
        crate::scenarios::evidence(id, "p", "constraint", "Retries are capped at 3 attempts")
    }

    fn revision_event(run: &RunId) -> ConsensusEvent {
        ev(
            run,
            6,
            Actor::Agent {
                role: AgentRole::Proposer,
            },
            EventPayload::RevisionSubmitted {
                revision: Revision {
                    schema_version: SCHEMA_VERSION.into(),
                    proposal: proposal(run),
                    supersedes: ProposalId::new("prop-1"),
                    addressed_items: vec!["i1".into()],
                },
                invocation: invocation(),
            },
        )
    }

    #[test]
    fn revision_after_abstention_requires_open_items() {
        let (mut agg, run) = to_abstained(&["i1"], SCHEMA_VERSION);
        agg.apply(&revision_event(&run)).unwrap();
        assert_eq!(agg.state, RunState::Revised);
        assert_eq!(agg.revision_round, 1);

        let (mut agg, run) = to_abstained(&[], SCHEMA_VERSION);
        assert_eq!(
            agg.apply(&revision_event(&run)).unwrap_err(),
            TransitionError::AbstainedWithoutOpenItems
        );
    }

    /// The round-1 verification of ev1 that `to_abstained`'s evaluation
    /// implies; tagged with its round from schema 1.3, as the runtime
    /// records it.
    fn contradicted_verification(schema: &str) -> EventPayload {
        EventPayload::EvidenceVerified {
            verification: EvidenceVerification {
                schema_version: schema.into(),
                evidence_id: EvidenceId::new("ev1"),
                integrity_status: IntegrityStatus::Valid,
                factual_status: FactualStatus::Contradicted,
                relevance_status: RelevanceStatus::Relevant,
                verifier: Some("test".into()),
                verified_anchor: None,
                detail: None,
                checked_at: "2026-10-26T00:00:00Z".into(),
                round: contract::binds(schema).then_some(1),
                proof_ref: None,
                report: None,
            },
        }
    }

    fn recommendation(kind: RecommendationKind, schema: &str) -> EventPayload {
        EventPayload::RecommendationIssued {
            recommendation: Recommendation {
                schema_version: schema.into(),
                kind,
                rationale: "r".into(),
                unresolved_assumptions: vec![],
                unresolved_uncertainties: vec![],
                issued_at: "2026-10-26T00:00:00Z".into(),
            },
        }
    }

    #[test]
    fn replay_rederives_each_run_with_its_recorded_rules() {
        // A 1.1 run (abstained over a recorded contradiction) must keep
        // replaying under the 1.1 order, and stays tamper-checked. 1.2
        // and 1.3 records both follow the 1.2 order.
        for (schema, stored, derived) in [
            (
                "1.1",
                RecommendationKind::Abstained,
                RecommendationKind::Abstained,
            ),
            (
                "1.2",
                RecommendationKind::Contested,
                RecommendationKind::Contested,
            ),
            (
                SCHEMA_VERSION,
                RecommendationKind::Contested,
                RecommendationKind::Contested,
            ),
            (
                "1.1",
                RecommendationKind::Contested,
                RecommendationKind::Abstained,
            ),
            (
                "1.2",
                RecommendationKind::Abstained,
                RecommendationKind::Contested,
            ),
            (
                SCHEMA_VERSION,
                RecommendationKind::Abstained,
                RecommendationKind::Contested,
            ),
        ] {
            let (mut agg, run) = to_abstained(&[], schema);
            let mut verified = ev(&run, 6, Actor::Runtime, contradicted_verification(schema));
            verified.schema_version = schema.into();
            agg.apply(&verified).unwrap();
            let mut issued = ev(&run, 7, Actor::Runtime, recommendation(stored, schema));
            issued.schema_version = schema.into();
            let result = agg.apply(&issued);
            if stored == derived {
                result.unwrap();
                assert_eq!(agg.state, RunState::AwaitingHumanApproval);
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    TransitionError::RecommendationMismatch { stored, derived },
                    "schema {schema}"
                );
            }
        }
    }

    /// `ev` with an explicit schema version.
    fn ev_at(run: &RunId, schema: &str, actor: Actor, payload: EventPayload) -> ConsensusEvent {
        let mut e = ev(run, 1, actor, payload);
        e.schema_version = schema.into();
        e
    }

    fn challenge_of(run: &RunId, items: Vec<ChallengeItem>) -> EventPayload {
        EventPayload::ChallengeSubmitted {
            challenge: Challenge {
                schema_version: SCHEMA_VERSION.into(),
                id: ChallengeId::new("chal-2"),
                run_id: run.clone(),
                target_proposal: ProposalId::new("prop-1"),
                items,
                created_at: "2026-10-26T00:00:00Z".into(),
            },
            invocation: invocation(),
        }
    }

    #[test]
    fn item_ids_are_held_unique_from_schema_1_3_only() {
        for schema in ["1.2", SCHEMA_VERSION] {
            // Abstained over i1, then revised: a new challenge may follow.
            let (mut agg, run) = to_abstained(&["i1"], schema);
            let mut revised = revision_event(&run);
            revised.schema_version = schema.into();
            agg.apply(&revised).unwrap();
            let critic = Actor::Agent {
                role: AgentRole::Critic,
            };
            let reused = agg.clone().apply(&ev_at(
                &run,
                schema,
                critic.clone(),
                challenge_of(&run, vec![item("i1")]),
            ));
            let empty = agg.clone().apply(&ev_at(
                &run,
                schema,
                critic.clone(),
                challenge_of(&run, vec![item(" ")]),
            ));
            if schema == "1.2" {
                reused.unwrap();
                empty.unwrap();
            } else {
                assert_eq!(
                    reused.unwrap_err(),
                    TransitionError::Contract(ContractViolation::ReusedItemId("i1".into()))
                );
                assert_eq!(
                    empty.unwrap_err(),
                    TransitionError::Contract(ContractViolation::EmptyItemId { index: 1 })
                );
            }
        }
    }

    #[test]
    fn evaluations_are_checked_against_the_events_from_schema_1_3_only() {
        for schema in ["1.2", SCHEMA_VERSION] {
            let (agg, run) = to_abstained(&[], schema);
            // A second evaluation in the round (as a forged log might hold):
            // silent on the carried contradiction ev1 and citing a record
            // the run never retrieved.
            let mut agg = agg;
            agg.state = RunState::EvidenceReview;
            let forged = EventPayload::EvaluationSubmitted {
                evaluation: Evaluation {
                    schema_version: schema.into(),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    assessments: vec![ClaimAssessment {
                        claim_id: ClaimId::new("c1"),
                        corroborated: true,
                        relevant_evidence: vec![EvidenceId::new("ev9")],
                        contradicting_evidence: vec![],
                        note: "forged".into(),
                    }],
                    unresolved_items: vec![],
                    item_verdicts: vec![],
                    cleared_contradictions: vec![],
                    abstain: false,
                    abstain_reason: None,
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            };
            let evaluator = Actor::Agent {
                role: AgentRole::EvidenceEvaluator,
            };
            let result = agg.apply(&ev_at(&run, schema, evaluator, forged));
            if schema == "1.2" {
                result.unwrap();
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    TransitionError::Contract(ContractViolation::UnretrievedEvidence {
                        claim: ClaimId::new("c1"),
                        field: "relevant_evidence",
                        evidence: EvidenceId::new("ev9"),
                    })
                );
            }
        }
    }

    #[test]
    fn the_aggregate_carries_open_items_and_contradictions_across_rounds() {
        let (mut agg, run) = to_abstained(&["i1", "i2"], SCHEMA_VERSION);
        let open = |agg: &RunAggregate| -> Vec<(String, u32)> {
            agg.open_items
                .iter()
                .map(|o| (o.item.id.clone(), o.raised_in_round))
                .collect()
        };
        assert_eq!(open(&agg), [("i1".into(), 1), ("i2".into(), 1)]);
        assert_eq!(agg.contradicted, [EvidenceId::new("ev1")]);
        assert_eq!(agg.claims, [ClaimId::new("c1")]);

        // Round 2: a revision, then a challenge raising i3.
        agg.apply(&revision_event(&run)).unwrap();
        assert_eq!(agg.round(), 2);
        let critic = Actor::Agent {
            role: AgentRole::Critic,
        };
        agg.apply(&ev(&run, 7, critic, challenge_of(&run, vec![item("i3")])))
            .unwrap();
        let ctx = agg.evaluation_context();
        assert_eq!(ctx.claims, [ClaimId::new("c1")]);
        assert_eq!(ctx.expected_items, ["i1", "i2", "i3"]);
        assert_eq!(ctx.retrieved, [EvidenceId::new("ev1")]);
        assert_eq!(ctx.carried_contradictions, [EvidenceId::new("ev1")]);

        // i1 resolved, i2 and i3 open; ev1 cleared.
        let verdict = |id: &str, status| ItemVerdict {
            item_id: id.into(),
            status,
            reason: "judged".into(),
        };
        agg.apply(&ev(
            &run,
            8,
            Actor::Agent {
                role: AgentRole::EvidenceEvaluator,
            },
            EventPayload::EvaluationSubmitted {
                evaluation: Evaluation {
                    schema_version: SCHEMA_VERSION.into(),
                    run_id: run.clone(),
                    target_proposal: ProposalId::new("prop-1"),
                    assessments: vec![ClaimAssessment {
                        claim_id: ClaimId::new("c1"),
                        corroborated: true,
                        relevant_evidence: vec![EvidenceId::new("ev1")],
                        contradicting_evidence: vec![],
                        note: "the revision keeps the cap".into(),
                    }],
                    unresolved_items: vec!["i2".into(), "i3".into()],
                    item_verdicts: vec![
                        verdict("i1", ItemStatus::Resolved),
                        verdict("i2", ItemStatus::Unresolved),
                        verdict("i3", ItemStatus::Unresolved),
                    ],
                    cleared_contradictions: vec![ClearedContradiction {
                        evidence_id: EvidenceId::new("ev1"),
                        reason: "the revision keeps the cap".into(),
                    }],
                    abstain: false,
                    abstain_reason: None,
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
                invocation: invocation(),
            },
        ))
        .unwrap();
        assert_eq!(open(&agg), [("i2".into(), 1), ("i3".into(), 2)]);
        assert!(agg.contradicted.is_empty());
        assert!(agg.round_items.is_empty());
        assert_eq!(agg.items_seen, ["i1", "i2", "i3"]);
    }

    #[test]
    fn recall_proven_is_accepted_where_evidence_events_are() {
        let run = RunId::new("run-1");
        let proven = || EventPayload::RecallProven {
            request: ProofRequest {
                operation: "recall".into(),
                project: "other-repo".into(),
                query: Some("retry cap".into()),
                limit: Some(5),
            },
            evidence_ids: vec![],
            proof_ref: "snapshot-1/recall-other-repo".into(),
            anchor: "ab".repeat(32),
            empty: true,
        };
        let mut agg = RunAggregate::new(run.clone());
        agg.apply(&ev(
            &run,
            1,
            Actor::Human { id: "mario".into() },
            EventPayload::TaskCreated { task: task(&run) },
        ))
        .unwrap();
        // Not before a proposal, exactly like EvidenceRetrieved.
        assert!(matches!(
            agg.clone()
                .apply(&ev(&run, 2, Actor::Runtime, proven()))
                .unwrap_err(),
            TransitionError::InvalidTransition { .. }
        ));
        agg.apply(&ev(
            &run,
            2,
            Actor::Agent {
                role: AgentRole::Proposer,
            },
            EventPayload::ProposalSubmitted {
                proposal: proposal(&run),
                invocation: invocation(),
            },
        ))
        .unwrap();
        agg.apply(&ev(&run, 3, Actor::Runtime, proven())).unwrap();
        assert_eq!(agg.state, RunState::EvidenceReview);
        assert!(
            agg.retrieved.is_empty(),
            "a proof is not a retrieved record"
        );
    }

    #[test]
    fn a_refused_call_on_a_failure_record_counts_as_a_model_call() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        agg.apply(&ev(
            &run,
            1,
            Actor::Runtime,
            EventPayload::RunFailed {
                reason: "evaluate output refused by the protocol 1.3 contract".into(),
                stage: "evaluate".into(),
                code: Some("evaluation_contract".into()),
                invocation: Some(invocation()),
            },
        ))
        .unwrap();
        assert_eq!(agg.state, RunState::Failed);
        assert_eq!(agg.invocations, [invocation()]);
    }

    #[test]
    fn duplicate_claim_ids_are_refused_from_schema_1_3_only() {
        for schema in ["1.2", SCHEMA_VERSION] {
            let run = RunId::new("run-1");
            let mut agg = RunAggregate::new(run.clone());
            agg.apply(&ev_at(
                &run,
                schema,
                Actor::Human { id: "mario".into() },
                EventPayload::TaskCreated { task: task(&run) },
            ))
            .unwrap();
            let mut twice = proposal(&run);
            twice.claims.push(twice.claims[0].clone());
            let result = agg.apply(&ev_at(
                &run,
                schema,
                Actor::Agent {
                    role: AgentRole::Proposer,
                },
                EventPayload::ProposalSubmitted {
                    proposal: twice,
                    invocation: invocation(),
                },
            ));
            if schema == "1.2" {
                result.unwrap();
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    TransitionError::Contract(ContractViolation::DuplicateClaimId(ClaimId::new(
                        "c1"
                    )))
                );
            }
        }
    }

    #[test]
    fn a_runs_schema_version_never_goes_down() {
        let run = RunId::new("run-1");
        let created = |schema: &str| {
            let mut agg = RunAggregate::new(run.clone());
            agg.apply(&ev_at(
                &run,
                schema,
                Actor::Human { id: "mario".into() },
                EventPayload::TaskCreated { task: task(&run) },
            ))
            .unwrap();
            agg
        };
        let proposed = |schema: &str| {
            ev_at(
                &run,
                schema,
                Actor::Agent {
                    role: AgentRole::Proposer,
                },
                EventPayload::ProposalSubmitted {
                    proposal: proposal(&run),
                    invocation: invocation(),
                },
            )
        };

        // Down, by one minor version or to the unchained 1.0: refused, and
        // the aggregate is left exactly as it was.
        for lower in ["1.2", "1.0"] {
            let mut agg = created(SCHEMA_VERSION);
            let before = agg.clone();
            assert_eq!(
                agg.apply(&proposed(lower)).unwrap_err(),
                TransitionError::SchemaDowngrade {
                    from: SCHEMA_VERSION.into(),
                    to: lower.into(),
                }
            );
            assert_eq!(agg, before);
        }
        // Level or up is how honest runs look (a newer binary may append
        // to an older run); the highest version so far is what counts.
        for (first, then) in [("1.2", "1.2"), ("1.0", "1.1"), ("1.2", SCHEMA_VERSION)] {
            let mut agg = created(first);
            agg.apply(&proposed(then)).unwrap();
            assert_eq!(agg.highest_schema_version.as_deref(), Some(then));
        }
        // A version that does not parse ranks last, so nothing parseable
        // may follow it.
        let mut agg = created("x");
        assert_eq!(
            agg.apply(&proposed(SCHEMA_VERSION)).unwrap_err(),
            TransitionError::SchemaDowngrade {
                from: "x".into(),
                to: SCHEMA_VERSION.into(),
            }
        );
    }

    #[test]
    fn verifications_are_rederived_from_schema_1_3_only() {
        for schema in ["1.2", SCHEMA_VERSION] {
            let checked = contract::binds(schema);
            let (agg, run) = to_abstained(&[], schema);
            let at = |seq, payload| {
                let mut e = ev(&run, seq, Actor::Runtime, payload);
                e.schema_version = schema.into();
                e
            };
            // `to_abstained`'s evaluation lists ev1 as contradicting c1.
            let mut flipped = contradicted_verification(schema);
            if let EventPayload::EvidenceVerified { verification } = &mut flipped {
                verification.factual_status = FactualStatus::Corroborated;
            }
            let flipped = agg.clone().apply(&at(6, flipped));
            // The verification of ev1 left out: the policy would read no
            // contradiction and derive Abstained instead of Contested.
            let unverified = agg.clone().apply(&at(
                6,
                recommendation(RecommendationKind::Abstained, schema),
            ));
            if checked {
                assert_eq!(
                    flipped.unwrap_err(),
                    TransitionError::Contract(ContractViolation::VerificationStatus {
                        evidence: EvidenceId::new("ev1"),
                        stored: (FactualStatus::Corroborated, RelevanceStatus::Relevant),
                        derived: (FactualStatus::Contradicted, RelevanceStatus::Relevant),
                    })
                );
                assert_eq!(
                    unverified.unwrap_err(),
                    TransitionError::Contract(ContractViolation::UnverifiedRecord(
                        EvidenceId::new("ev1")
                    ))
                );
            } else {
                flipped.unwrap();
                unverified.unwrap();
            }

            // A verification recorded before the round's evaluation.
            let mut early = RunAggregate::new(run.clone());
            for (seq, actor, payload) in [
                (
                    1,
                    Actor::Human { id: "mario".into() },
                    EventPayload::TaskCreated { task: task(&run) },
                ),
                (
                    2,
                    Actor::Agent {
                        role: AgentRole::Proposer,
                    },
                    EventPayload::ProposalSubmitted {
                        proposal: proposal(&run),
                        invocation: invocation(),
                    },
                ),
                (
                    3,
                    Actor::Runtime,
                    EventPayload::EvidenceRetrieved {
                        evidence: record("ev1"),
                    },
                ),
            ] {
                let mut e = ev(&run, seq, actor, payload);
                e.schema_version = schema.into();
                early.apply(&e).unwrap();
            }
            let result = early.apply(&at(4, contradicted_verification(schema)));
            if checked {
                assert_eq!(
                    result.unwrap_err(),
                    TransitionError::InvalidTransition {
                        state: RunState::EvidenceReview,
                        event: "evidence_verified",
                    }
                );
            } else {
                result.unwrap();
            }
        }
    }

    #[test]
    fn evaluation_before_evidence_review_is_rejected() {
        let (agg, _) = to_awaiting();
        assert_eq!(agg.state, RunState::Proposed);
    }

    #[test]
    fn agent_cannot_self_approve() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        agg.state = RunState::AwaitingHumanApproval;
        let err = agg
            .apply(&ev(
                &run,
                1,
                Actor::Agent {
                    role: AgentRole::Proposer,
                },
                EventPayload::HumanDecisionRecorded {
                    authorization: auth(&run, true),
                },
            ))
            .unwrap_err();
        assert_eq!(err, TransitionError::NotHuman);
        assert_eq!(agg.authorization, AuthorizationState::Pending);
    }

    #[test]
    fn approval_before_awaiting_is_rejected() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        let err = agg
            .apply(&ev(
                &run,
                1,
                Actor::Human { id: "mario".into() },
                EventPayload::HumanDecisionRecorded {
                    authorization: auth(&run, true),
                },
            ))
            .unwrap_err();
        assert!(matches!(err, TransitionError::InvalidTransition { .. }));
    }

    #[test]
    fn terminal_state_rejects_further_events() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        agg.state = RunState::AwaitingHumanApproval;
        agg.apply(&ev(
            &run,
            1,
            Actor::Human { id: "mario".into() },
            EventPayload::HumanDecisionRecorded {
                authorization: auth(&run, false),
            },
        ))
        .unwrap();
        assert_eq!(agg.state, RunState::Rejected);
        let err = agg
            .apply(&ev(
                &run,
                2,
                Actor::Runtime,
                EventPayload::RunFailed {
                    reason: "late".into(),
                    stage: "x".into(),
                    code: None,
                    invocation: None,
                },
            ))
            .unwrap_err();
        assert!(matches!(err, TransitionError::Terminal { .. }));
    }

    #[test]
    fn revision_beyond_round_limit_is_rejected() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        agg.state = RunState::Evaluated;
        agg.revision_round = MAX_REVISION_ROUNDS;
        let revision = Revision {
            schema_version: SCHEMA_VERSION.into(),
            proposal: proposal(&run),
            supersedes: ProposalId::new("prop-1"),
            addressed_items: vec![],
        };
        let err = agg
            .apply(&ev(
                &run,
                1,
                Actor::Agent {
                    role: AgentRole::Proposer,
                },
                EventPayload::RevisionSubmitted {
                    revision,
                    invocation: invocation(),
                },
            ))
            .unwrap_err();
        assert_eq!(err, TransitionError::RoundLimit);
    }

    #[test]
    fn abstained_run_rejects_non_abstained_recommendation() {
        let run = RunId::new("run-1");
        let mut agg = RunAggregate::new(run.clone());
        agg.state = RunState::Abstained;
        let rec = Recommendation {
            schema_version: SCHEMA_VERSION.into(),
            kind: RecommendationKind::Supported,
            rationale: "r".into(),
            unresolved_assumptions: vec![],
            unresolved_uncertainties: vec![],
            issued_at: "2026-10-26T00:00:00Z".into(),
        };
        let err = agg
            .apply(&ev(
                &run,
                1,
                Actor::Runtime,
                EventPayload::RecommendationIssued {
                    recommendation: rec,
                },
            ))
            .unwrap_err();
        assert_eq!(err, TransitionError::AbstainKindMismatch);
    }
}
