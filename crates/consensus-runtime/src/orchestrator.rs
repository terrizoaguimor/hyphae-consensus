// SPDX-License-Identifier: Apache-2.0
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
use crate::control::RunControl;
use crate::event_log::{EventLog, EventLogError};
use crate::policy::{derive_recommendation, rationale_for};
use crate::ports::*;
use crate::sink::EventSink;
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
    /// Creates and deliberates one run in a single call, with a run id
    /// from `ids` and a control that is never cancelled and has no
    /// deadline: [`create_task`](Self::create_task) followed by
    /// [`deliberate`](Self::deliberate).
    pub fn run<S: EventSink>(
        &mut self,
        sink: &mut S,
        input: TaskInput,
    ) -> Result<RunAggregate, OrchestratorError> {
        let run_id = RunId::new(self.ids.next("run"));
        let (agg, task) = self.create_task(sink, run_id, input)?;
        self.deliberate(sink, agg, &task, &RunControl::new())
    }

    /// Records `TaskCreated` for a new run and returns the run's
    /// aggregate and task, ready for [`deliberate`](Self::deliberate).
    /// Makes no model or memory call.
    ///
    /// `run_id` must not already have events in `sink`. The event is
    /// validated against a fresh aggregate, and [`EventLog`] does not
    /// replay a run's stored events before appending, so a reused id can
    /// record a second task for that run and make it fail replay.
    pub fn create_task<S: EventSink>(
        &mut self,
        sink: &mut S,
        run_id: RunId,
        input: TaskInput,
    ) -> Result<(RunAggregate, Task), OrchestratorError> {
        let mut agg = RunAggregate::new(run_id.clone());
        let task = Task {
            schema_version: SCHEMA_VERSION.into(),
            id: TaskId::new(self.ids.next("task")),
            run_id,
            project_id: input.project_id.clone(),
            title: input.title,
            description: input.description,
            created_at: self.clock.now(),
        };
        self.emit(
            sink,
            &mut agg,
            Actor::Human {
                id: input.submitted_by.clone(),
            },
            EventPayload::TaskCreated { task: task.clone() },
        )?;
        Ok((agg, task))
    }

    /// Deliberates a run made by [`create_task`](Self::create_task):
    /// proposal, evidence retrieval and integrity checks, critique,
    /// evaluation, bounded revisions, then the runtime-issued
    /// recommendation, which leaves the run awaiting a human decision.
    ///
    /// `control` is checked before every model call, before every memory
    /// recall and before the recommendation. Once it is tripped, the next
    /// checkpoint ends the run with `RunFailed` (stage `cancelled`,
    /// `shutdown` or `deadline`). A call already in progress is not
    /// interrupted: its outcome is handled as usual first (its document
    /// recorded and charged, or the run failed at that call's stage).
    ///
    /// `agg` must be the run of `task`, still in state `Created`.
    /// Anything else is refused before any model call is made.
    pub fn deliberate<S: EventSink>(
        &mut self,
        sink: &mut S,
        mut agg: RunAggregate,
        task: &Task,
        control: &RunControl,
    ) -> Result<RunAggregate, OrchestratorError> {
        // A model call spends tokens even if its document is then refused,
        // so a wrong starting point is refused here, before the first call.
        if task.run_id != agg.run_id {
            return Err(TransitionError::RunMismatch.into());
        }
        if agg.state.is_terminal() {
            return Err(TransitionError::Terminal { state: agg.state }.into());
        }
        if agg.state != RunState::Created || agg.task.is_none() {
            return Err(TransitionError::InvalidTransition {
                state: agg.state,
                event: "proposal_submitted",
            }
            .into());
        }

        // Every model call is charged against the run's token cap. The
        // call's document is recorded first (its tokens were spent and
        // belong in the receipt); then an exhausted budget fails the run.
        let mut budget = TokenBudget {
            cap: self.limits.max_run_tokens,
            used: 0,
            calls: 0,
        };

        // Stage 1: proposal.
        if let Some(stop) = control.check() {
            return self.fail(sink, agg, stop.stage(), &stop.reason("propose call"));
        }
        let (mut proposal, inv) = match self.model.propose(task) {
            Ok(v) => v,
            Err(e) => return self.fail(sink, agg, "propose", &e.to_string()),
        };
        let over_budget = budget.charge("propose", &inv);
        self.emit(
            sink,
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
            return self.fail(sink, agg, "token_budget", &reason);
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
                if let Some(stop) = control.check() {
                    return self.fail(sink, agg, stop.stage(), &stop.reason("memory recall"));
                }
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
                                    sink,
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
                            sink,
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
                    sink,
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

            if let Some(stop) = control.check() {
                return self.fail(sink, agg, stop.stage(), &stop.reason("critique call"));
            }
            let (challenge, inv) = match self.model.critique(&proposal, &ctx) {
                Ok(v) => v,
                Err(e) => return self.fail(sink, agg, "critique", &e.to_string()),
            };
            let over_budget = budget.charge("critique", &inv);
            self.emit(
                sink,
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
                return self.fail(sink, agg, "token_budget", &reason);
            }

            if let Some(stop) = control.check() {
                return self.fail(sink, agg, stop.stage(), &stop.reason("evaluate call"));
            }
            let (evaluation, inv) = match self.model.evaluate(&proposal, &challenge, &ctx) {
                Ok(v) => v,
                Err(e) => return self.fail(sink, agg, "evaluate", &e.to_string()),
            };
            let over_budget = budget.charge("evaluate", &inv);
            self.emit(
                sink,
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
                return self.fail(sink, agg, "token_budget", &reason);
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
                    sink,
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
            if let Some(stop) = control.check() {
                return self.fail(sink, agg, stop.stage(), &stop.reason("revise call"));
            }
            let (revision, inv) = match self.model.revise(&proposal, &challenge, &evaluation, &ctx)
            {
                Ok(v) => v,
                Err(e) => return self.fail(sink, agg, "revise", &e.to_string()),
            };
            let over_budget = budget.charge("revise", &inv);
            self.emit(
                sink,
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
                return self.fail(sink, agg, "token_budget", &reason);
            }
            proposal = revision.proposal;
        };

        if let Some(stop) = control.check() {
            return self.fail(sink, agg, stop.stage(), &stop.reason("recommendation"));
        }
        let recommendation = Recommendation {
            schema_version: SCHEMA_VERSION.into(),
            kind: final_kind,
            rationale: rationale_for(final_kind, &evaluation, &all_verifications),
            unresolved_assumptions: proposal.assumptions.clone(),
            issued_at: self.clock.now(),
        };
        self.emit(
            sink,
            &mut agg,
            Actor::Runtime,
            EventPayload::RecommendationIssued { recommendation },
        )?;
        Ok(agg)
    }

    fn emit<S: EventSink>(
        &mut self,
        sink: &mut S,
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
        // Apply the record as the sink stored it (sequence, chain hash;
        // the original record when the event id was already stored).
        let stored = sink.append(new)?;
        agg.apply(&stored)?;
        Ok(())
    }

    fn fail<S: EventSink>(
        &mut self,
        sink: &mut S,
        mut agg: RunAggregate,
        stage: &str,
        reason: &str,
    ) -> Result<RunAggregate, OrchestratorError> {
        self.emit(
            sink,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, SequentialIdGen};
    use crate::control::CancelReason;
    use crate::fakes::{FakeMemoryClient, FakeModelClient, FakeProofVerifier};
    use crate::scenarios::{retry_constraint_scenario, TS};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    type TestOrchestrator<M, Mem> =
        Orchestrator<M, Mem, FakeProofVerifier, FixedClock, SequentialIdGen>;

    fn temp_log(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyphae-consensus-orch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    fn orchestrator<M: ModelClient, Mem: MemoryClient>(
        model: M,
        memory: Mem,
        verifier: FakeProofVerifier,
    ) -> TestOrchestrator<M, Mem> {
        Orchestrator {
            model,
            memory,
            verifier,
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        }
    }

    /// `create_task` with a run id drawn first from the orchestrator's
    /// own generator, exactly as `run` does.
    fn create<M: ModelClient, Mem: MemoryClient>(
        orch: &mut TestOrchestrator<M, Mem>,
        log: &mut EventLog,
        input: TaskInput,
    ) -> (RunAggregate, Task) {
        let run_id = RunId::new(orch.ids.next("run"));
        orch.create_task(log, run_id, input).unwrap()
    }

    /// Creates the reference run, then deliberates it under a `control`
    /// that is already tripped, so no model call may be made.
    fn deliberate_tripped(name: &str, control: &RunControl) -> (RunAggregate, EventLog) {
        let s = retry_constraint_scenario();
        let mut log = EventLog::open(temp_log(name)).unwrap();
        let mut orch = orchestrator(s.model, s.memory, s.verifier);
        let (agg, task) = create(&mut orch, &mut log, s.input);
        let agg = orch.deliberate(&mut log, agg, &task, control).unwrap();
        assert!(orch.model.calls.is_empty(), "{:?}", orch.model.calls);
        (agg, log)
    }

    /// (stage, reason) of the log's last record, which must be RunFailed.
    fn failure(log: &EventLog) -> (&str, &str) {
        match &log.all_events().last().expect("a record").payload {
            EventPayload::RunFailed { stage, reason } => (stage.as_str(), reason.as_str()),
            other => panic!("expected RunFailed last, got {other:?}"),
        }
    }

    /// Exactly TaskCreated, then RunFailed with `stage` and `reason`.
    fn assert_stopped_before_propose(
        agg: &RunAggregate,
        log: &EventLog,
        stage: &str,
        reason: &str,
    ) {
        let events = log.all_events();
        assert_eq!(events.len(), 2, "{events:#?}");
        assert!(matches!(
            events[0].payload,
            EventPayload::TaskCreated { .. }
        ));
        assert_eq!(failure(log), (stage, reason));
        assert_eq!(agg.state, RunState::Failed);
        assert_eq!(agg.failure_reason.as_deref(), Some(reason));
        assert_eq!(&log.rebuild(&agg.run_id).unwrap(), agg, "replays");
    }

    #[test]
    fn cancel_before_propose_records_only_task_and_failure() {
        let control = RunControl::new();
        assert!(control.cancel(CancelReason::Owner));
        let (agg, log) = deliberate_tripped("cancel-before-propose.jsonl", &control);
        assert_stopped_before_propose(
            &agg,
            &log,
            "cancelled",
            "cancelled by the run's owner before the propose call",
        );
    }

    #[test]
    fn expired_deadline_fails_the_run_with_stage_deadline() {
        let control = RunControl::with_deadline(Instant::now());
        let (agg, log) = deliberate_tripped("deadline-expired.jsonl", &control);
        assert_stopped_before_propose(
            &agg,
            &log,
            "deadline",
            "run deadline passed before the propose call",
        );
    }

    #[test]
    fn shutdown_is_recorded_with_stage_shutdown() {
        let control = RunControl::new();
        control.cancel(CancelReason::Shutdown);
        let (agg, log) = deliberate_tripped("shutdown.jsonl", &control);
        assert_stopped_before_propose(
            &agg,
            &log,
            "shutdown",
            "stopped by shutdown before the propose call",
        );
    }

    /// The scripted fake, plus an owner's cancel that arrives while its
    /// `cancel_during`-th call (1-based) is in flight.
    struct CancellingModel {
        inner: FakeModelClient,
        control: RunControl,
        cancel_during: usize,
    }

    impl CancellingModel {
        fn after_call(&self) {
            if self.inner.calls.len() == self.cancel_during {
                self.control.cancel(CancelReason::Owner);
            }
        }
    }

    impl ModelClient for CancellingModel {
        fn propose(&mut self, task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
            let out = self.inner.propose(task);
            self.after_call();
            out
        }
        fn critique(
            &mut self,
            proposal: &Proposal,
            ctx: &RoleContext,
        ) -> Result<(Challenge, ModelInvocation), ModelError> {
            let out = self.inner.critique(proposal, ctx);
            self.after_call();
            out
        }
        fn evaluate(
            &mut self,
            proposal: &Proposal,
            challenge: &Challenge,
            ctx: &RoleContext,
        ) -> Result<(Evaluation, ModelInvocation), ModelError> {
            let out = self.inner.evaluate(proposal, challenge, ctx);
            self.after_call();
            out
        }
        fn revise(
            &mut self,
            proposal: &Proposal,
            challenge: &Challenge,
            evaluation: &Evaluation,
            ctx: &RoleContext,
        ) -> Result<(Revision, ModelInvocation), ModelError> {
            let out = self.inner.revise(proposal, challenge, evaluation, ctx);
            self.after_call();
            out
        }
    }

    #[test]
    fn cancel_during_a_model_call_is_honored_at_the_next_checkpoint() {
        // Reference calls: propose, critique, evaluate (two items open),
        // revise, critique, evaluate (nothing open).
        for (cancel_during, next_checkpoint) in [
            (1, "memory recall"),
            (2, "evaluate call"),
            (3, "revise call"),
            (4, "memory recall"),
            (5, "evaluate call"),
            (6, "recommendation"),
        ] {
            let s = retry_constraint_scenario();
            let control = RunControl::new();
            let model = CancellingModel {
                inner: s.model,
                control: control.clone(),
                cancel_during,
            };
            let name = format!("cancel-during-call-{cancel_during}.jsonl");
            let mut log = EventLog::open(temp_log(&name)).unwrap();
            let mut orch = orchestrator(model, s.memory, s.verifier);
            let (agg, task) = create(&mut orch, &mut log, s.input);
            let agg = orch.deliberate(&mut log, agg, &task, &control).unwrap();

            // The call in flight completed and its document was recorded;
            // no later call was made.
            assert_eq!(orch.model.inner.calls.len(), cancel_during);
            assert_eq!(agg.invocations.len(), cancel_during, "{name}");
            let reason = format!("cancelled by the run's owner before the {next_checkpoint}");
            assert_eq!(failure(&log), ("cancelled", reason.as_str()), "{name}");
            assert_eq!(agg.state, RunState::Failed);
            assert!(agg.recommendation.is_none());
            assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "{name} replays");
        }
    }

    /// The seeded fake memory, plus an owner's cancel that arrives while
    /// the first recall is in flight.
    struct CancellingMemory {
        inner: FakeMemoryClient,
        control: RunControl,
        recalls: usize,
    }

    impl MemoryClient for CancellingMemory {
        fn recall(
            &mut self,
            project: &ProjectId,
            query: &str,
            limit: usize,
        ) -> Result<Vec<EvidenceReference>, MemoryError> {
            self.recalls += 1;
            let out = self.inner.recall(project, query, limit);
            self.control.cancel(CancelReason::Owner);
            out
        }
        fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
            self.inner.status()
        }
    }

    #[test]
    fn cancel_during_a_memory_recall_is_honored_before_the_next_recall() {
        let s = retry_constraint_scenario();
        let control = RunControl::new();
        let memory = CancellingMemory {
            inner: s.memory,
            control: control.clone(),
            recalls: 0,
        };
        let mut log = EventLog::open(temp_log("cancel-during-recall.jsonl")).unwrap();
        let mut orch = orchestrator(s.model, memory, s.verifier);
        let (agg, task) = create(&mut orch, &mut log, s.input);
        let agg = orch.deliberate(&mut log, agg, &task, &control).unwrap();

        // The proposal has two claims; the second recall never ran.
        assert_eq!(orch.memory.recalls, 1);
        assert_eq!(orch.model.calls, ["propose"]);
        assert_eq!(
            failure(&log),
            (
                "cancelled",
                "cancelled by the run's owner before the memory recall"
            )
        );
        assert_eq!(agg.state, RunState::Failed);
        assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "replays");
    }

    #[test]
    fn create_task_then_deliberate_matches_run_for_the_reference_scenario() {
        let reference = |name: &str, split: Option<RunControl>| {
            let s = retry_constraint_scenario();
            let path = temp_log(name);
            let mut log = EventLog::open(&path).unwrap();
            let mut orch = orchestrator(s.model, s.memory, s.verifier);
            let agg = match split {
                None => orch.run(&mut log, s.input).unwrap(),
                Some(control) => {
                    let (agg, task) = create(&mut orch, &mut log, s.input);
                    orch.deliberate(&mut log, agg, &task, &control).unwrap()
                }
            };
            let events = log.all_events().to_vec();
            drop(log);
            (agg, events, orch.model.calls, std::fs::read(&path).unwrap())
        };
        let via_run = reference("reference-run.jsonl", None);
        assert_eq!(via_run.0.state, RunState::AwaitingHumanApproval);
        assert_eq!(
            via_run.0.recommendation.as_ref().map(|r| r.kind),
            Some(RecommendationKind::Supported)
        );
        assert_eq!(
            via_run.2,
            ["propose", "critique", "evaluate", "revise", "critique", "evaluate"]
        );

        // Same aggregate, same records (ids, timestamps, chain hashes) and
        // the same bytes on disk. A deadline not yet reached changes nothing.
        let split = reference("reference-split.jsonl", Some(RunControl::new()));
        assert_eq!(split, via_run);
        let later = RunControl::with_deadline(Instant::now() + Duration::from_secs(3600));
        let split = reference("reference-split-deadline.jsonl", Some(later));
        assert_eq!(split, via_run);
    }

    #[test]
    fn deliberate_refuses_a_run_that_is_not_freshly_created() {
        let s = retry_constraint_scenario();
        let mut log = EventLog::open(temp_log("refuse.jsonl")).unwrap();
        let mut orch = orchestrator(s.model, s.memory, s.verifier);
        let (created, task) = create(&mut orch, &mut log, s.input.clone());
        let awaiting = orch
            .deliberate(&mut log, created.clone(), &task, &RunControl::new())
            .unwrap();
        assert_eq!(awaiting.state, RunState::AwaitingHumanApproval);
        let calls = orch.model.calls.len();
        let cancelled = RunControl::new();
        cancelled.cancel(CancelReason::Owner);

        // A second deliberation, even under a tripped control, must leave
        // a run that awaits its human decision untouched.
        let len = log.len();
        let err = orch
            .deliberate(&mut log, awaiting.clone(), &task, &cancelled)
            .unwrap_err();
        assert!(
            matches!(
                err,
                OrchestratorError::Transition(TransitionError::InvalidTransition {
                    state: RunState::AwaitingHumanApproval,
                    ..
                })
            ),
            "{err}"
        );
        assert_eq!(log.len(), len);
        assert_eq!(log.rebuild(&awaiting.run_id).unwrap(), awaiting);

        // A task that belongs to another run.
        let mut other = task.clone();
        other.run_id = RunId::new("run-other");
        let err = orch
            .deliberate(&mut log, created, &other, &RunControl::new())
            .unwrap_err();
        assert!(
            matches!(
                err,
                OrchestratorError::Transition(TransitionError::RunMismatch)
            ),
            "{err}"
        );
        assert_eq!(log.len(), len);

        // A run that already ended.
        let (fresh, task) = create(&mut orch, &mut log, s.input);
        let failed = orch.deliberate(&mut log, fresh, &task, &cancelled).unwrap();
        assert_eq!(failed.state, RunState::Failed);
        let len = log.len();
        let err = orch
            .deliberate(&mut log, failed, &task, &RunControl::new())
            .unwrap_err();
        assert!(
            matches!(
                err,
                OrchestratorError::Transition(TransitionError::Terminal {
                    state: RunState::Failed
                })
            ),
            "{err}"
        );
        assert_eq!(log.len(), len);
        assert_eq!(orch.model.calls.len(), calls, "no model call was made");
    }
}
