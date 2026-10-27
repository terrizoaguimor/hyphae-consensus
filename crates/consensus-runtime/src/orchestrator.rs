//! Run orchestration: Proposer -> evidence retrieval/verification ->
//! Critic -> Evidence Evaluator -> optional bounded revisions ->
//! runtime-issued recommendation -> awaiting human authorization.
//!
//! Model or memory failures never panic and never fabricate progress:
//! model errors fail the run with a recorded reason; memory unavailability
//! degrades evidence (see README offline statement); a run whose model
//! calls cross the per-run token cap ([`RunLimits`]) fails with stage
//! `token_budget`. Human approval is recorded by [`record_human_decision`]
//! and triggers no execution in M1-M3 — the `ExecutorPort` is never
//! invoked here.

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

/// Hard bound on distinct evidence records processed per run, so a
/// model emitting many claims cannot cause unbounded proof-verification
/// processes or prompt growth. Records past the cap are dropped and the
/// drop is recorded. Recall calls are bounded separately: one per claim
/// per round, with claims capped by the model adapter.
pub const MAX_EVIDENCE_PER_RUN: usize = 20;

/// Default per-run token cap (input + output, all model calls). The
/// 2026-10-26 live calls averaged about 4k tokens each, so a full 9-call
/// run at that rate needs roughly a quarter of this.
pub const DEFAULT_MAX_RUN_TOKENS: u64 = 150_000;

/// Per-run resource limits, enforced by the runtime for any `ModelClient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLimits {
    /// Cap on reported input + output tokens across one run's model calls.
    /// Checked after every call: the run fails at the first call that
    /// crosses it, so it can overshoot by at most that one call.
    pub max_run_tokens: u64,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_run_tokens: DEFAULT_MAX_RUN_TOKENS,
        }
    }
}

/// Running token account for one run.
struct TokenBudget {
    cap: u64,
    used: u64,
    calls: u32,
}

impl TokenBudget {
    /// Charges one call. Returns the failure reason when the run must
    /// stop: the cap is crossed, or the call reported no usage, which
    /// would make the cap unenforceable (never assumed to be zero).
    fn charge(&mut self, stage: &str, invocation: &ModelInvocation) -> Option<String> {
        self.calls += 1;
        let (Some(input), Some(output)) = (invocation.input_tokens, invocation.output_tokens)
        else {
            return Some(format!(
                "token budget cannot be enforced: the {stage} call reported no token usage"
            ));
        };
        self.used = self.used.saturating_add(input).saturating_add(output);
        (self.used > self.cap).then(|| {
            format!(
                "token budget exceeded after the {stage} call: {} tokens used across {} model calls, cap {} (CONSENSUS_MAX_RUN_TOKENS)",
                self.used, self.calls, self.cap
            )
        })
    }
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
    pub limits: RunLimits,
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

        // Every model call is charged against the run's token cap. The
        // call's document is recorded first (its tokens were spent and
        // belong in the receipt); then an exhausted budget fails the run.
        let mut budget = TokenBudget {
            cap: self.limits.max_run_tokens,
            used: 0,
            calls: 0,
        };

        // Stage 1: proposal.
        let (mut proposal, inv) = match self.model.propose(&task) {
            Ok(v) => v,
            Err(e) => return self.fail(log, agg, "propose", &e.to_string()),
        };
        let over_budget = budget.charge("propose", &inv);
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
        if let Some(reason) = over_budget {
            return self.fail(log, agg, "token_budget", &reason);
        }

        let mut seen_evidence: HashSet<EvidenceId> = HashSet::new();
        let mut prev_unresolved: Option<usize> = None;

        // Stages 2-5, bounded by revision rounds and a no-progress exit.
        let (final_kind, evaluation, all_verifications) = loop {
            // Retrieve evidence per claim (read-only memory access).
            // Retrieval is performed by the runtime, never by a model,
            // so these events are attributed to Actor::Runtime.
            let mut evidence: Vec<EvidenceReference> = Vec::new();
            let mut capped = 0usize;
            for claim in &proposal.claims {
                match self.memory.recall(&task.project_id, &claim.statement, 5) {
                    Ok(records) => {
                        for rec in records {
                            // Already-seen records stay usable in later
                            // rounds; only NEW records count against the cap.
                            if !seen_evidence.contains(&rec.id)
                                && seen_evidence.len() >= MAX_EVIDENCE_PER_RUN
                            {
                                capped += 1;
                                continue;
                            }
                            if seen_evidence.insert(rec.id.clone()) {
                                self.emit(
                                    log,
                                    &mut agg,
                                    Actor::Runtime,
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
                            Actor::Runtime,
                            EventPayload::EvidenceUnavailable {
                                query: claim.statement.clone(),
                                reason: e.to_string(),
                            },
                        )?;
                    }
                }
            }
            if capped > 0 {
                // The cap degrades evidence; it must never do so silently.
                self.emit(
                    log,
                    &mut agg,
                    Actor::Runtime,
                    EventPayload::EvidenceUnavailable {
                        query: "(per-run evidence cap)".into(),
                        reason: format!(
                            "{capped} additional record(s) not processed: per-run cap of {MAX_EVIDENCE_PER_RUN} distinct records reached"
                        ),
                    },
                )?;
            }
            // Integrity pre-checks (events emitted after evaluation so the
            // three orthogonal axes land in one record). Records whose
            // proof GENERATION failed upstream carry `proof_error`:
            // corruption-class signals count as Invalid (never silently
            // downgraded), other failures as Unavailable.
            let mut checks: HashMap<EvidenceId, ProofCheck> = HashMap::new();
            for ev in &evidence {
                let check = match &ev.proof_error {
                    Some(reason) => ProofCheck {
                        status: if reason.to_lowercase().contains("corrupt") {
                            IntegrityStatus::Invalid
                        } else {
                            IntegrityStatus::Unavailable
                        },
                        verifier: Some("recall(prove) upstream".into()),
                        verified_anchor: ev.anchor.clone(),
                        detail: Some(reason.clone()),
                    },
                    None => self.verifier.verify(ev),
                };
                checks.insert(ev.id.clone(), check);
            }
            let ctx = RoleContext {
                evidence: evidence.clone(),
                integrity: checks.iter().map(|(k, v)| (k.clone(), v.status)).collect(),
            };

            let (challenge, inv) = match self.model.critique(&proposal, &ctx) {
                Ok(v) => v,
                Err(e) => return self.fail(log, agg, "critique", &e.to_string()),
            };
            let over_budget = budget.charge("critique", &inv);
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
            if let Some(reason) = over_budget {
                return self.fail(log, agg, "token_budget", &reason);
            }

            let (evaluation, inv) = match self.model.evaluate(&proposal, &challenge, &ctx) {
                Ok(v) => v,
                Err(e) => return self.fail(log, agg, "evaluate", &e.to_string()),
            };
            let over_budget = budget.charge("evaluate", &inv);
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
            if let Some(reason) = over_budget {
                return self.fail(log, agg, "token_budget", &reason);
            }

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
            let unresolved = evaluation.unresolved_items.len();

            let no_progress = prev_unresolved.map(|p| unresolved >= p).unwrap_or(false);
            prev_unresolved = Some(unresolved);
            // Unresolved items drive revision. Since schema 1.2 that holds
            // after an abstention too (bounded by the same round limit and
            // no-progress exit); an abstention with nothing unresolved, or
            // a blocked result, ends deliberation.
            let should_revise = final_kind != RecommendationKind::Blocked
                && unresolved > 0
                && !no_progress
                && agg.revision_round < MAX_REVISION_ROUNDS;
            if !should_revise {
                break (final_kind, evaluation, all_verifications);
            }
            let (revision, inv) = match self.model.revise(&proposal, &challenge, &evaluation, &ctx)
            {
                Ok(v) => v,
                Err(e) => return self.fail(log, agg, "revise", &e.to_string()),
            };
            let over_budget = budget.charge("revise", &inv);
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
            if let Some(reason) = over_budget {
                return self.fail(log, agg, "token_budget", &reason);
            }
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
        // Validate before persisting: the log must never hold a record
        // that replay would reject (that would poison the whole run).
        agg.clone().apply(&new.clone().into_sequenced(0))?;
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

/// Record a human approval/rejection. Idempotent per run on
/// `authorization.idempotency_key` (scoped by run id, so reusing a key
/// across runs cannot silently swallow a decision): a retry returns the
/// recorded outcome without appending. Performs NO external action
/// (executor deferred).
pub fn record_human_decision(
    log: &mut EventLog,
    authorization: HumanAuthorization,
    occurred_at: String,
) -> Result<RunAggregate, OrchestratorError> {
    let event_id = EventId::new(format!(
        "human-decision:{}:{}",
        authorization.run_id, authorization.idempotency_key
    ));
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

/// Outcome of [`mark_interrupted_runs`].
#[derive(Debug, Default)]
pub struct Recovery {
    /// Runs closed as `Failed{stage:"interrupted"}`.
    pub interrupted: Vec<RunId>,
    /// Runs whose recorded history fails replay. Left untouched (a broken
    /// history is never rewritten) and reported, so one bad run cannot
    /// stop the console from starting; the console shows them as failed.
    pub unreplayable: Vec<(RunId, String)>,
}

/// Startup recovery: runs left mid-deliberation by a dead process are
/// closed as `Failed{stage:"interrupted"}` instead of lingering forever.
/// Runs awaiting a human are legitimate resting states and are kept.
pub fn mark_interrupted_runs(
    log: &mut EventLog,
    ids: &mut impl IdGen,
    now: String,
) -> Result<Recovery, OrchestratorError> {
    let mut run_ids: Vec<RunId> = Vec::new();
    for ev in log.all_events() {
        if !run_ids.contains(&ev.run_id) {
            run_ids.push(ev.run_id.clone());
        }
    }
    let mut recovery = Recovery::default();
    for run_id in run_ids {
        let mut agg = match log.rebuild(&run_id) {
            Ok(agg) => agg,
            Err(e) => {
                recovery.unreplayable.push((run_id, e.to_string()));
                continue;
            }
        };
        if agg.task.is_some()
            && !agg.state.is_terminal()
            && agg.state != RunState::AwaitingHumanApproval
        {
            let new = NewEvent {
                event_id: EventId::new(ids.next("evt")),
                run_id: run_id.clone(),
                actor: Actor::Runtime,
                occurred_at: now.clone(),
                payload: EventPayload::RunFailed {
                    reason: "process restarted before the run completed".into(),
                    stage: "interrupted".into(),
                },
            };
            let seq = log.append(new)?;
            agg.apply(&log.all_events()[(seq - 1) as usize])?;
            recovery.interrupted.push(run_id);
        }
    }
    Ok(recovery)
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
        head_hash: log
            .events_for(run_id)
            .last()
            .map(|e| e.hash.clone())
            .filter(|h| !h.is_empty()),
        generated_at,
        proof_limitations: RunReceipt::DEFAULT_LIMITATIONS.into(),
    })
}
