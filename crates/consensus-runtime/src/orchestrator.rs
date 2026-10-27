//! Run orchestration: Proposer -> evidence retrieval/verification ->
//! Critic -> Evidence Evaluator -> optional bounded revisions ->
//! runtime-issued recommendation -> awaiting human authorization.
//!
//! Model or memory failures never panic and never fabricate progress:
//! model errors fail the run with a recorded reason; memory unavailability
//! degrades evidence (see README offline statement). Human approval is
//! recorded by [`record_human_decision`] and triggers no execution in
//! M1-M3 — the `ExecutorPort` is never invoked here.

use crate::clock::{Clock, IdGen};
use crate::event_log::{EventLog, EventLogError};
use crate::policy::{derive_recommendation, rationale_for};
use crate::ports::*;
use crate::state_machine::{RunAggregate, TransitionError, MAX_REVISION_ROUNDS};
use consensus_domain::*;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("event log error: {0}")]
    Log(#[from] EventLogError),
    #[error("state transition error: {0}")]
    Transition(#[from] TransitionError),
}

#[derive(Debug, Clone)]
pub struct TaskInput {
    pub project_id: ProjectId,
    pub title: String,
    pub description: String,
    pub submitted_by: String,
}

pub struct Orchestrator<M, Mem, V, C, I> {
    pub model: M,
    pub memory: Mem,
    pub verifier: V,
    pub clock: C,
    pub ids: I,
}

impl<M: ModelClient, Mem: MemoryClient, V: ProofVerifier, C: Clock, I: IdGen>
    Orchestrator<M, Mem, V, C, I>
{
    pub fn run(
        &mut self,
        log: &mut EventLog,
        input: TaskInput,
    ) -> Result<RunAggregate, OrchestratorError> {
        let run_id = RunId::new(self.ids.next("run"));
        let mut agg = RunAggregate::new(run_id.clone());
        let task = Task {
            schema_version: SCHEMA_VERSION.into(),
            id: TaskId::new(self.ids.next("task")),
            run_id: run_id.clone(),
            project_id: input.project_id.clone(),
            title: input.title,
            description: input.description,
            created_at: self.clock.now(),
        };
        self.emit(
            log,
            &mut agg,
            Actor::Human {
                id: input.submitted_by.clone(),
            },
            EventPayload::TaskCreated { task: task.clone() },
        )?;

        // Stage 1: proposal.
        let (mut proposal, inv) = match self.model.propose(&task) {
            Ok(v) => v,
            Err(e) => return self.fail(log, agg, "propose", &e.to_string()),
        };
        self.emit(
            log,
            &mut agg,
            Actor::Agent {
                role: AgentRole::Proposer,
            },
            EventPayload::ProposalSubmitted {
                proposal: proposal.clone(),
                invocation: inv,
            },
        )?;

        let mut seen_evidence: HashSet<EvidenceId> = HashSet::new();
        let mut prev_unresolved: Option<usize> = None;

        // Stages 2-5, bounded by revision rounds and a no-progress exit.
        let (final_kind, evaluation, all_verifications) = loop {
            // Retrieve evidence per claim (read-only memory access).
            let mut evidence: Vec<EvidenceReference> = Vec::new();
            for claim in &proposal.claims {
                match self.memory.recall(&task.project_id, &claim.statement, 5) {
                    Ok(records) => {
                        for rec in records {
                            if seen_evidence.insert(rec.id.clone()) {
                                self.emit(
                                    log,
                                    &mut agg,
                                    Actor::Agent {
                                        role: AgentRole::EvidenceEvaluator,
                                    },
                                    EventPayload::EvidenceRetrieved {
                                        evidence: rec.clone(),
                                    },
                                )?;
                            }
                            if !evidence.iter().any(|e| e.id == rec.id) {
                                evidence.push(rec);
                            }
                        }
                    }
                    Err(e) => {
                        self.emit(
                            log,
                            &mut agg,
                            Actor::Agent {
                                role: AgentRole::EvidenceEvaluator,
                            },
                            EventPayload::EvidenceUnavailable {
                                query: claim.statement.clone(),
                                reason: e.to_string(),
                            },
                        )?;
                    }
                }
            }
            // Integrity pre-checks (events emitted after evaluation so the
            // three orthogonal axes land in one record).
            let mut checks: HashMap<EvidenceId, ProofCheck> = HashMap::new();
            for ev in &evidence {
                checks.insert(ev.id.clone(), self.verifier.verify(ev));
            }
            let ctx = RoleContext {
                evidence: evidence.clone(),
                integrity: checks.iter().map(|(k, v)| (k.clone(), v.status)).collect(),
            };

            let (challenge, inv) = match self.model.critique(&proposal, &ctx) {
                Ok(v) => v,
                Err(e) => return self.fail(log, agg, "critique", &e.to_string()),
            };
            self.emit(
                log,
                &mut agg,
                Actor::Agent {
                    role: AgentRole::Critic,
                },
                EventPayload::ChallengeSubmitted {
                    challenge: challenge.clone(),
                    invocation: inv,
                },
            )?;

            let (evaluation, inv) = match self.model.evaluate(&proposal, &challenge, &ctx) {
                Ok(v) => v,
                Err(e) => return self.fail(log, agg, "evaluate", &e.to_string()),
            };
            self.emit(
                log,
                &mut agg,
                Actor::Agent {
                    role: AgentRole::EvidenceEvaluator,
                },
                EventPayload::EvaluationSubmitted {
                    evaluation: evaluation.clone(),
                    invocation: inv,
                },
            )?;

            // Combined verification records: integrity from the verifier,
            // factual/relevance from the evaluation. Never cross-derived.
            let mut round_verifications = Vec::new();
            for ev in &evidence {
                let check = checks.get(&ev.id).cloned().unwrap_or(ProofCheck {
                    status: IntegrityStatus::NotChecked,
                    verifier: None,
                    verified_anchor: None,
                    detail: None,
                });
                let relevant = evaluation
                    .assessments
                    .iter()
                    .any(|a| a.relevant_evidence.contains(&ev.id));
                let contradicted = evaluation
                    .assessments
                    .iter()
                    .any(|a| a.contradicting_evidence.contains(&ev.id));
                let corroborating = evaluation
                    .assessments
                    .iter()
                    .any(|a| a.corroborated && a.relevant_evidence.contains(&ev.id));
                let verification = EvidenceVerification {
                    schema_version: SCHEMA_VERSION.into(),
                    evidence_id: ev.id.clone(),
                    integrity_status: check.status,
                    factual_status: if contradicted {
                        FactualStatus::Contradicted
                    } else if corroborating {
                        FactualStatus::Corroborated
                    } else if relevant {
                        FactualStatus::Uncorroborated
                    } else {
                        FactualStatus::NotAssessed
                    },
                    relevance_status: if relevant || contradicted {
                        RelevanceStatus::Relevant
                    } else {
                        RelevanceStatus::Irrelevant
                    },
                    verifier: check.verifier,
                    verified_anchor: check.verified_anchor,
                    detail: check.detail,
                    checked_at: self.clock.now(),
                };
                self.emit(
                    log,
                    &mut agg,
                    Actor::Runtime,
                    EventPayload::EvidenceVerified {
                        verification: verification.clone(),
                    },
                )?;
                round_verifications.push(verification);
            }
            let all_verifications = round_verifications;
            let final_kind = derive_recommendation(&evaluation, &all_verifications);
            let unresolved = evaluation.unresolved_challenges.len();

            let no_progress = prev_unresolved.map(|p| unresolved >= p).unwrap_or(false);
            prev_unresolved = Some(unresolved);
            let should_revise = !evaluation.abstain
                && final_kind != RecommendationKind::Blocked
                && unresolved > 0
                && !no_progress
                && agg.revision_round < MAX_REVISION_ROUNDS;
            if !should_revise {
                break (final_kind, evaluation, all_verifications);
            }
            let (revision, inv) = match self.model.revise(&proposal, &challenge, &evaluation) {
                Ok(v) => v,
                Err(e) => return self.fail(log, agg, "revise", &e.to_string()),
            };
            self.emit(
                log,
                &mut agg,
                Actor::Agent {
                    role: AgentRole::Proposer,
                },
                EventPayload::RevisionSubmitted {
                    revision: revision.clone(),
                    invocation: inv,
                },
            )?;
            proposal = revision.proposal;
        };

        let recommendation = Recommendation {
            schema_version: SCHEMA_VERSION.into(),
            kind: final_kind,
            rationale: rationale_for(final_kind, &evaluation, &all_verifications),
            unresolved_assumptions: proposal.assumptions.clone(),
            issued_at: self.clock.now(),
        };
        self.emit(
            log,
            &mut agg,
            Actor::Runtime,
            EventPayload::RecommendationIssued { recommendation },
        )?;
        Ok(agg)
    }

    fn emit(
        &mut self,
        log: &mut EventLog,
        agg: &mut RunAggregate,
        actor: Actor,
        payload: EventPayload,
    ) -> Result<(), OrchestratorError> {
        let new = NewEvent {
            event_id: EventId::new(self.ids.next("evt")),
            run_id: agg.run_id.clone(),
            actor,
            occurred_at: self.clock.now(),
            payload,
        };
        let seq = log.append(new)?;
        let stored = &log.all_events()[(seq - 1) as usize];
        agg.apply(stored)?;
        Ok(())
    }

    fn fail(
        &mut self,
        log: &mut EventLog,
        mut agg: RunAggregate,
        stage: &str,
        reason: &str,
    ) -> Result<RunAggregate, OrchestratorError> {
        self.emit(
            log,
            &mut agg,
            Actor::Runtime,
            EventPayload::RunFailed {
                reason: reason.to_string(),
                stage: stage.to_string(),
            },
        )?;
        Ok(agg)
    }
}

/// Record a human approval/rejection. Idempotent on
/// `authorization.idempotency_key`: a retry returns the recorded outcome
/// without appending. Performs NO external action (executor deferred).
pub fn record_human_decision(
    log: &mut EventLog,
    authorization: HumanAuthorization,
    occurred_at: String,
) -> Result<RunAggregate, OrchestratorError> {
    let event_id = EventId::new(format!("human-decision:{}", authorization.idempotency_key));
    if log.contains(&event_id) {
        return Ok(log.rebuild(&authorization.run_id)?);
    }
    let new = NewEvent {
        event_id,
        run_id: authorization.run_id.clone(),
        actor: Actor::Human {
            id: authorization.authorized_by.clone(),
        },
        occurred_at,
        payload: EventPayload::HumanDecisionRecorded { authorization },
    };
    // Validate against a rebuilt aggregate before persisting.
    let mut probe = log.rebuild(&new.run_id)?;
    probe.apply(&new.clone().into_sequenced(0))?;
    let run_id = new.run_id.clone();
    log.append(new)?;
    Ok(log.rebuild(&run_id)?)
}

/// Build the redacted run receipt from replayed state.
pub fn build_receipt(
    log: &EventLog,
    run_id: &RunId,
    generated_at: String,
) -> Result<RunReceipt, OrchestratorError> {
    let agg = log.rebuild(run_id)?;
    Ok(RunReceipt {
        schema_version: SCHEMA_VERSION.into(),
        run_id: run_id.clone(),
        final_state: agg.state,
        recommendation: agg.recommendation,
        authorization: agg.authorization,
        model_invocations: agg.invocations,
        verifications: agg.verifications,
        event_count: agg.event_count,
        generated_at,
        proof_limitations: RunReceipt::DEFAULT_LIMITATIONS.into(),
    })
}
