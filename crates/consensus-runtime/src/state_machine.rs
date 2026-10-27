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
    #[error("abstained runs may only receive an abstained recommendation")]
    AbstainKindMismatch,
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
                if self.state == RunState::Abstained
                    && recommendation.kind != RecommendationKind::Abstained
                {
                    return Err(TransitionError::AbstainKindMismatch);
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
                    unresolved_challenges: vec![],
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
