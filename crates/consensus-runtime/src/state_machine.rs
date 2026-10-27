//! Deterministic run state machine.
//!
//! `apply` is a pure transition function over the aggregate: the same
//! event sequence always yields the same aggregate, which is what makes
//! log replay a faithful rebuild. Authorization transitions additionally
//! require a human actor, so no agent or runtime component can approve
//! its own run.

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
    #[error("abstained runs may only receive an abstained or blocked recommendation")]
    AbstainKindMismatch,
    #[error("stored recommendation {stored:?} does not match derived {derived:?}")]
    RecommendationMismatch {
        stored: RecommendationKind,
        derived: RecommendationKind,
    },
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
        }
    }

    pub fn apply(&mut self, event: &ConsensusEvent) -> Result<(), TransitionError> {
        if event.run_id != self.run_id {
            return Err(TransitionError::RunMismatch);
        }
        if self.state.is_terminal() {
            return Err(TransitionError::Terminal { state: self.state });
        }
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
                self.current_proposal = Some(proposal.id.clone());
                self.invocations.push(invocation.clone());
                self.state = RunState::Proposed;
            }
            EventPayload::EvidenceRetrieved { .. } | EventPayload::EvidenceUnavailable { .. } => {
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
                self.verifications.push(verification.clone());
                self.round_verifications.push(verification.clone());
            }
            EventPayload::ChallengeSubmitted { invocation, .. } => {
                self.require(
                    &[
                        RunState::Proposed,
                        RunState::EvidenceReview,
                        RunState::Revised,
                    ],
                    "challenge_submitted",
                )?;
                self.invocations.push(invocation.clone());
                self.state = RunState::EvidenceReview;
            }
            EventPayload::EvaluationSubmitted {
                evaluation,
                invocation,
            } => {
                self.require(&[RunState::EvidenceReview], "evaluation_submitted")?;
                self.invocations.push(invocation.clone());
                self.last_evaluation = Some(evaluation.clone());
                self.round_verifications.clear();
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
                self.require(&[RunState::Evaluated], "revision_submitted")?;
                if self.revision_round >= MAX_REVISION_ROUNDS {
                    return Err(TransitionError::RoundLimit);
                }
                self.revision_round += 1;
                self.current_proposal = Some(revision.proposal.id.clone());
                self.invocations.push(invocation.clone());
                self.state = RunState::Revised;
            }
            EventPayload::RecommendationIssued { recommendation } => {
                self.require(
                    &[RunState::Evaluated, RunState::Abstained],
                    "recommendation_issued",
                )?;
                // An invalid proof outranks abstention in the policy, so an
                // abstained run may still be Blocked — but never upgraded.
                if self.state == RunState::Abstained
                    && !matches!(
                        recommendation.kind,
                        RecommendationKind::Abstained | RecommendationKind::Blocked
                    )
                {
                    return Err(TransitionError::AbstainKindMismatch);
                }
                // Schema 1.1+: never trust the stored kind — recompute it
                // from the tracked evaluation + this round's verification
                // records, exactly as the orchestrator derived it. A log
                // edited to upgrade its own outcome fails replay. Legacy
                // records predate the current policy and skip recompute.
                if event.schema_version == SCHEMA_VERSION {
                    if let Some(eval) = &self.last_evaluation {
                        let derived =
                            crate::policy::derive_recommendation(eval, &self.round_verifications);
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
            EventPayload::RunFailed { reason, .. } => {
                self.failure_reason = Some(reason.clone());
                self.state = RunState::Failed;
            }
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

    fn proposal(run: &RunId) -> Proposal {
        Proposal {
            schema_version: SCHEMA_VERSION.into(),
            id: ProposalId::new("prop-1"),
            run_id: run.clone(),
            author_role: AgentRole::Proposer,
            summary: "s".into(),
            plan_steps: vec![],
            claims: vec![],
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
