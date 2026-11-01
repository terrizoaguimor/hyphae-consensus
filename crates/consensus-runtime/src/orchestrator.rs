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
//!
//! Protocol 1.3 (new runs) carries the deliberation across rounds: each
//! round's context holds every record retrieved so far in the run, the
//! objections still open and the contradictions not yet cleared, all read
//! from the run's aggregate. Each model document is checked against the
//! [`contract`] before it is recorded; a violation fails the run at that
//! call's stage, with the refused call kept in the record. A call the
//! adapter refused after the provider answered is kept the same way
//! ([`ModelError::invocation`]). The recommendation is derived from the
//! recorded evaluation and verifications, exactly the inputs replay
//! re-derives it from.

use crate::clock::{Clock, IdGen};
use crate::contract::{self, ContractViolation};
use crate::control::{RunControl, Stop};
use crate::event_log::{EventLog, EventLogError};
use crate::policy::{derive_recommendation, rationale_for};
use crate::ports::*;
use crate::sink::EventSink;
use crate::state_machine::{RunAggregate, TransitionError, MAX_REVISION_ROUNDS};
use consensus_domain::*;
use thiserror::Error;

/// Stable machine codes the orchestrator records next to an English
/// reason (schema 1.3): `RunFailed.code` and `EvidenceUnavailable.code`.
/// A reader localizes from these, never from the hashed reason text. A
/// stop requested through [`RunControl`] records its stage (`cancelled`,
/// `shutdown` or `deadline`) as its code. A model client may classify a
/// failure under its own code ([`ModelError::code`], e.g. the Meta
/// adapter's `meta_unauthorized`); that code is recorded instead of the
/// generic one.
pub mod codes {
    /// An evaluation broke the protocol 1.3 evaluation contract (stage
    /// `evaluate`).
    pub const EVALUATION_CONTRACT: &str = "evaluation_contract";
    /// A challenge left an item id empty or reused one (stage `critique`).
    pub const CHALLENGE_CONTRACT: &str = "challenge_contract";
    /// A proposal or revision repeated a claim id (stage `propose` or
    /// `revise`).
    pub const PROPOSAL_CONTRACT: &str = "proposal_contract";
    /// A model call timed out.
    pub const MODEL_TIMEOUT: &str = "model_timeout";
    /// A model call returned malformed structured output.
    pub const MODEL_MALFORMED_OUTPUT: &str = "model_malformed_output";
    /// The model's output hit its token bound (`finish_reason` `length`)
    /// before the document was complete.
    pub const MODEL_OUTPUT_TRUNCATED: &str = "model_output_truncated";
    /// The model declined to produce the document.
    pub const MODEL_REFUSAL: &str = "model_refusal";
    /// The model service was unavailable.
    pub const MODEL_UNAVAILABLE: &str = "model_unavailable";
    /// The per-run token cap was crossed (stage `token_budget`).
    pub const TOKEN_BUDGET_EXCEEDED: &str = "token_budget_exceeded";
    /// A call reported no token usage (stage `token_budget`).
    pub const TOKEN_USAGE_MISSING: &str = "token_usage_missing";
    /// The process restarted mid-run (stage `interrupted`).
    pub const INTERRUPTED: &str = "interrupted";
    /// Evidence: the memory backend was unreachable.
    pub const MEMORY_UNAVAILABLE: &str = "memory_unavailable";
    /// Evidence: the memory backend answered with an error.
    pub const MEMORY_ERROR: &str = "memory_error";
    /// Evidence: records past the per-run cap were not processed.
    pub const EVIDENCE_CAP: &str = "evidence_cap";
    /// Evidence: the memory backend returned a record of a project other
    /// than the run's or `_global` (audit finding TC-4); it was dropped.
    pub const FOREIGN_RECORD: &str = "foreign_record";
}

/// The code recorded for a failed model call: the client's own when it
/// gave one, else the generic code of the error's kind.
fn model_error_code(e: &ModelError) -> &'static str {
    e.code().unwrap_or(match e {
        ModelError::Timeout => codes::MODEL_TIMEOUT,
        ModelError::MalformedOutput { .. } => codes::MODEL_MALFORMED_OUTPUT,
        ModelError::Refused { .. } => codes::MODEL_REFUSAL,
        ModelError::Unavailable(_) | ModelError::Provider { .. } => codes::MODEL_UNAVAILABLE,
    })
}

/// Hyphae answers a failed proof generation with one generic `corruption`
/// category for capacity, configuration and concurrency failures as well
/// as for real corruption (audit finding HY-4), so the record's detail says
/// so next to the fail-closed `Invalid`.
const GENERIC_CORRUPTION_NOTE: &str = "No verifier ran. Hyphae reports capacity and configuration failures (a store past its proof size limit, a socket inside the data directory, a store that changed while its witness was bundled) under the same generic corruption category as tampering, so this alone does not show tampering; the record still counts as Invalid (fail-closed).";

/// The integrity check of a record whose proof generation failed
/// upstream, from its recorded `proof_error` and, when the recall gave
/// it, the backend's typed failure. `Invalid` when the typed category is
/// `corruption` OR the text names corruption (the keyword rule earlier
/// runs were recorded under): the typed category may only widen that
/// rule, never narrow it (LC-10). Anything else is `Unavailable`.
fn upstream_proof_check(
    ev: &EvidenceReference,
    reason: &str,
    failure: Option<&ProofFailure>,
) -> ProofCheck {
    let typed_corruption = failure
        .and_then(|f| f.category.as_deref())
        .is_some_and(|c| c.eq_ignore_ascii_case("corruption"));
    let corruption = typed_corruption || reason.to_lowercase().contains("corrupt");
    let (status, detail) = if corruption {
        (
            IntegrityStatus::Invalid,
            format!("proof generation failed upstream: {reason}. {GENERIC_CORRUPTION_NOTE}"),
        )
    } else {
        (
            IntegrityStatus::Unavailable,
            format!("proof generation failed upstream: {reason}. No verifier ran; the record's integrity is unavailable."),
        )
    };
    ProofCheck {
        status,
        verifier: Some("recall(prove) upstream".into()),
        verified_anchor: ev.anchor.clone(),
        detail: Some(detail),
        report: None,
    }
}

/// The recorded reason for a dropped foreign record: its id and the
/// project the backend named for it, each quoted and bounded
/// ([`contract::quote`]), never its content.
fn foreign_record_reason(foreign: &ForeignRecord, project: &ProjectId) -> String {
    let owner = match &foreign.backend_project {
        Some(p) => format!("for project {}", contract::quote(p)),
        None => "without naming its project".to_string(),
    };
    format!(
        "record {} was returned {owner}, neither {} nor _global: dropped, never used as evidence",
        contract::quote(foreign.evidence_id.as_str()),
        contract::quote(project.as_str())
    )
}

fn memory_error_code(e: &MemoryError) -> &'static str {
    match e {
        MemoryError::Unavailable(_) => codes::MEMORY_UNAVAILABLE,
        MemoryError::Backend(_) => codes::MEMORY_ERROR,
    }
}

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
/// per round, with claims capped by the model adapter. Since protocol 1.3
/// every round's context is the run's retrieved records, so this also
/// bounds each round's context.
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
    /// Charges one call. Returns the failure code and reason when the run
    /// must stop: the cap is crossed, or the call reported no usage, which
    /// would make the cap unenforceable (never assumed to be zero).
    fn charge(
        &mut self,
        stage: &str,
        invocation: &ModelInvocation,
    ) -> Option<(&'static str, String)> {
        self.calls += 1;
        let (Some(input), Some(output)) = (invocation.input_tokens, invocation.output_tokens)
        else {
            return Some((
                codes::TOKEN_USAGE_MISSING,
                format!(
                    "token budget cannot be enforced: the {stage} call reported no token usage"
                ),
            ));
        };
        self.used = self.used.saturating_add(input).saturating_add(output);
        (self.used > self.cap).then(|| {
            (
                codes::TOKEN_BUDGET_EXCEEDED,
                format!(
                    "token budget exceeded after the {stage} call: {} tokens used across {} model calls, cap {} (CONSENSUS_MAX_RUN_TOKENS)",
                    self.used, self.calls, self.cap
                ),
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
    /// Declared mode, recorded as `Task.mode` (schema 1.3).
    pub mode: Option<TaskMode>,
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
            mode: input.mode,
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
    /// Protocol 1.3: each round's context is every record retrieved so far
    /// in the run (at most [`MAX_EVIDENCE_PER_RUN`]); every record of it is
    /// integrity-checked and verified for that round; and the objections
    /// and contradictions the previous evaluation left open are shown
    /// again and must be judged again. A proposal, challenge or evaluation
    /// that breaks the [`contract`] is refused before it is recorded: the
    /// run fails at that call's stage (code `proposal_contract`,
    /// `challenge_contract` or `evaluation_contract`) and the refused call
    /// is kept in `RunFailed.invocation`, as is an answered call whose
    /// output the adapter refused (code `model_malformed_output`, or the
    /// adapter's own code); either is charged to the token budget. A
    /// revision follows only while objections stay open, the result is not
    /// blocked, the round made progress (it resolved at least one objection
    /// the previous evaluation had left open) and the round bound allows.
    ///
    /// Each recall goes through [`MemoryClient::recall_with_proof`]. A
    /// record the backend returned for another project is dropped and
    /// recorded as `EvidenceUnavailable` (code `foreign_record`); every
    /// proof the backend sealed, an empty result's included, is recorded as
    /// `RecallProven` after the recall's records.
    ///
    /// `control` is checked before every model call, before every memory
    /// recall, before every proof verification and before the
    /// recommendation. Once it is tripped, the next checkpoint ends the
    /// run with `RunFailed` (stage `cancelled`, `shutdown` or `deadline`).
    /// A call already in progress is not interrupted: its outcome is
    /// handled as usual first (its document recorded and charged, or the
    /// run failed at that call's stage).
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
            return self.stop(sink, agg, stop, "propose call");
        }
        let (mut proposal, inv) = match self.model.propose(task) {
            Ok(v) => v,
            Err(e) => return self.model_failed(sink, agg, "propose", &e, &mut budget),
        };
        let over_budget = budget.charge("propose", &inv);
        if let Err(violation) = contract::validate_claims(&proposal) {
            return self.refuse(
                sink,
                agg,
                "propose",
                codes::PROPOSAL_CONTRACT,
                &violation,
                inv,
            );
        }
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
        if let Some((code, reason)) = over_budget {
            return self.fail(sink, agg, "token_budget", code, &reason);
        }

        // Why proof generation failed upstream, as the backend typed it,
        // for each record retrieved in this run (the first retrieval of a
        // record is the one the log keeps, so it is the one kept here).
        let mut proof_failures: Vec<(EvidenceId, ProofFailure)> = Vec::new();

        // Stages 2-5, bounded by revision rounds and a no-progress exit.
        let final_kind = loop {
            let round = agg.round();
            // Retrieve evidence per claim (read-only memory access).
            // Retrieval is performed by the runtime, never by a model,
            // so these events are attributed to Actor::Runtime.
            let mut capped = 0usize;
            for claim in &proposal.claims {
                if let Some(stop) = control.check() {
                    return self.stop(sink, agg, stop, "memory recall");
                }
                match self
                    .memory
                    .recall_with_proof(&task.project_id, &claim.statement, 5)
                {
                    Ok(recall) => {
                        for rec in recall.records {
                            // A record retrieved earlier in the run is
                            // already in every later round's context; only
                            // NEW records count against the cap.
                            if agg.retrieved.iter().any(|e| e.id == rec.id) {
                                continue;
                            }
                            if agg.retrieved.len() >= MAX_EVIDENCE_PER_RUN {
                                capped += 1;
                                continue;
                            }
                            if let Some(failure) = &recall.proof_failure {
                                proof_failures.push((rec.id.clone(), failure.clone()));
                            }
                            self.emit(
                                sink,
                                &mut agg,
                                Actor::Runtime,
                                EventPayload::EvidenceRetrieved { evidence: rec },
                            )?;
                        }
                        // TC-4: a record of another project never becomes
                        // evidence, and its drop is on the record.
                        for foreign in &recall.foreign {
                            self.emit(
                                sink,
                                &mut agg,
                                Actor::Runtime,
                                EventPayload::EvidenceUnavailable {
                                    query: claim.statement.clone(),
                                    reason: foreign_record_reason(foreign, &task.project_id),
                                    code: Some(codes::FOREIGN_RECORD.into()),
                                },
                            )?;
                        }
                        // HY-6/HY-12: every proof the backend sealed is
                        // recorded, an empty result's included (it proves
                        // absence, e.g. project isolation).
                        if let Some(proof) = recall.proof {
                            let empty = proof.evidence_ids.is_empty();
                            self.emit(
                                sink,
                                &mut agg,
                                Actor::Runtime,
                                EventPayload::RecallProven {
                                    request: proof.request,
                                    evidence_ids: proof.evidence_ids,
                                    proof_ref: proof.proof_ref,
                                    anchor: proof.anchor,
                                    empty,
                                },
                            )?;
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
                                code: Some(memory_error_code(&e).into()),
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
                        code: Some(codes::EVIDENCE_CAP.into()),
                    },
                )?;
            }
            // The round context: every record retrieved in the run so far,
            // as the log recorded it (protocol 1.3 carry-forward).
            let evidence = agg.retrieved.clone();
            // Integrity pre-checks (events emitted after evaluation so the
            // three orthogonal axes land in one record). Records whose
            // proof GENERATION failed upstream carry `proof_error`:
            // corruption-class signals count as Invalid (never silently
            // downgraded), other failures as Unavailable. Every record is
            // checked every round; a verifier may reuse its own results
            // (the Hyphae verifier reuses a result for the same artifact
            // bytes and anchor).
            let mut checks: Vec<ProofCheck> = Vec::with_capacity(evidence.len());
            for ev in &evidence {
                let check = match &ev.proof_error {
                    Some(reason) => {
                        let failure = proof_failures
                            .iter()
                            .find(|(id, _)| id == &ev.id)
                            .map(|(_, f)| f);
                        upstream_proof_check(ev, reason, failure)
                    }
                    None => {
                        if let Some(stop) = control.check() {
                            return self.stop(sink, agg, stop, "proof verification");
                        }
                        self.verifier.verify(ev)
                    }
                };
                checks.push(check);
            }
            let ctx = RoleContext {
                integrity: evidence
                    .iter()
                    .zip(&checks)
                    .map(|(ev, check)| (ev.id.clone(), check.status))
                    .collect(),
                evidence,
                open_items: agg.open_items.clone(),
                carried_contradictions: agg.contradicted.clone(),
            };

            if let Some(stop) = control.check() {
                return self.stop(sink, agg, stop, "critique call");
            }
            let (challenge, inv) = match self.model.critique(&proposal, &ctx) {
                Ok(v) => v,
                Err(e) => return self.model_failed(sink, agg, "critique", &e, &mut budget),
            };
            let over_budget = budget.charge("critique", &inv);
            if let Err(violation) = contract::validate_challenge(&agg.items_seen, &challenge) {
                return self.refuse(
                    sink,
                    agg,
                    "critique",
                    codes::CHALLENGE_CONTRACT,
                    &violation,
                    inv,
                );
            }
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
            if let Some((code, reason)) = over_budget {
                return self.fail(sink, agg, "token_budget", code, &reason);
            }

            if let Some(stop) = control.check() {
                return self.stop(sink, agg, stop, "evaluate call");
            }
            let (evaluation, inv) = match self.model.evaluate(&proposal, &challenge, &ctx) {
                Ok(v) => v,
                Err(e) => return self.model_failed(sink, agg, "evaluate", &e, &mut budget),
            };
            let over_budget = budget.charge("evaluate", &inv);
            // The contract replay enforces, against what the log holds so
            // far: a violation is the model's failure, never repaired here.
            if let Err(violation) =
                contract::validate_evaluation(&agg.evaluation_context(), &evaluation)
            {
                return self.refuse(
                    sink,
                    agg,
                    "evaluate",
                    codes::EVALUATION_CONTRACT,
                    &violation,
                    inv,
                );
            }
            self.emit(
                sink,
                &mut agg,
                Actor::Agent {
                    role: AgentRole::EvidenceEvaluator,
                },
                EventPayload::EvaluationSubmitted {
                    evaluation,
                    invocation: inv,
                },
            )?;
            if let Some((code, reason)) = over_budget {
                return self.fail(sink, agg, "token_budget", code, &reason);
            }
            // From here on the evaluation is read as the log recorded it.
            let evaluation = recorded_evaluation(&agg)?;

            // Combined verification records: integrity from the verifier,
            // factual/relevance from the evaluation. Never cross-derived.
            // One per record of the round context, tagged with the round;
            // replay re-derives all but the integrity status with the same
            // function.
            for (ev, check) in ctx.evidence.iter().zip(checks) {
                let (factual_status, relevance_status) =
                    contract::verification_statuses(&evaluation, &ev.id);
                let verification = EvidenceVerification {
                    schema_version: SCHEMA_VERSION.into(),
                    evidence_id: ev.id.clone(),
                    integrity_status: check.status,
                    factual_status,
                    relevance_status,
                    verifier: check.verifier,
                    verified_anchor: check.verified_anchor,
                    detail: check.detail,
                    checked_at: self.clock.now(),
                    round: Some(round),
                    proof_ref: ev.proof_ref.clone(),
                    report: check.report,
                };
                self.emit(
                    sink,
                    &mut agg,
                    Actor::Runtime,
                    EventPayload::EvidenceVerified { verification },
                )?;
            }
            // The policy reads the recorded evaluation and this round's
            // recorded verifications, exactly the inputs replay uses.
            let final_kind = derive_recommendation(&evaluation, &agg.round_verifications);

            // Progress is set-based: this round made some only if its
            // evaluation resolved at least one objection the previous
            // evaluation had left open. The first evaluation has no
            // earlier objections, so the rule starts with the second.
            let progress = round == 1
                || evaluation.item_verdicts.iter().any(|v| {
                    v.status == ItemStatus::Resolved
                        && ctx.open_items.iter().any(|o| o.item.id == v.item_id)
                });
            // Open objections drive revision. Since schema 1.2 that holds
            // after an abstention too (bounded by the same round limit and
            // no-progress exit); nothing open, or a blocked result, ends
            // deliberation.
            let should_revise = final_kind != RecommendationKind::Blocked
                && !agg.open_items.is_empty()
                && progress
                && agg.revision_round < MAX_REVISION_ROUNDS;
            if !should_revise {
                break final_kind;
            }
            if let Some(stop) = control.check() {
                return self.stop(sink, agg, stop, "revise call");
            }
            let (revision, inv) = match self.model.revise(&proposal, &challenge, &evaluation, &ctx)
            {
                Ok(v) => v,
                Err(e) => return self.model_failed(sink, agg, "revise", &e, &mut budget),
            };
            let over_budget = budget.charge("revise", &inv);
            if let Err(violation) = contract::validate_claims(&revision.proposal) {
                return self.refuse(
                    sink,
                    agg,
                    "revise",
                    codes::PROPOSAL_CONTRACT,
                    &violation,
                    inv,
                );
            }
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
            if let Some((code, reason)) = over_budget {
                return self.fail(sink, agg, "token_budget", code, &reason);
            }
            proposal = revision.proposal;
        };

        if let Some(stop) = control.check() {
            return self.stop(sink, agg, stop, "recommendation");
        }
        let evaluation = recorded_evaluation(&agg)?;
        let recommendation = Recommendation {
            schema_version: SCHEMA_VERSION.into(),
            kind: final_kind,
            rationale: rationale_for(final_kind, &evaluation, &agg.round_verifications),
            unresolved_assumptions: proposal.assumptions.clone(),
            unresolved_uncertainties: proposal.uncertainties.clone(),
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

    /// Ends the run with `RunFailed` at `stage`, with its code and reason.
    fn fail<S: EventSink>(
        &mut self,
        sink: &mut S,
        agg: RunAggregate,
        stage: &str,
        code: &str,
        reason: &str,
    ) -> Result<RunAggregate, OrchestratorError> {
        self.fail_with(sink, agg, stage, code, reason, None)
    }

    fn fail_with<S: EventSink>(
        &mut self,
        sink: &mut S,
        mut agg: RunAggregate,
        stage: &str,
        code: &str,
        reason: &str,
        invocation: Option<ModelInvocation>,
    ) -> Result<RunAggregate, OrchestratorError> {
        self.emit(
            sink,
            &mut agg,
            Actor::Runtime,
            EventPayload::RunFailed {
                reason: reason.to_string(),
                stage: stage.to_string(),
                code: Some(code.to_string()),
                invocation,
            },
        )?;
        Ok(agg)
    }

    /// A tripped [`RunControl`]: stage and code are the stop's, and the
    /// reason names the checkpoint that saw it.
    fn stop<S: EventSink>(
        &mut self,
        sink: &mut S,
        agg: RunAggregate,
        stop: Stop,
        checkpoint: &str,
    ) -> Result<RunAggregate, OrchestratorError> {
        self.fail(
            sink,
            agg,
            stop.stage(),
            stop.stage(),
            &stop.reason(checkpoint),
        )
    }

    /// A model call that returned an error instead of a document. When the
    /// provider had answered (the adapter refused its output, or a body
    /// read failed after the answer), the call stays on the record
    /// (`RunFailed.invocation`) with its request id and tokens, and it is
    /// charged to the run's token budget like any other call (TC-7). The
    /// call comes from the error itself or, when the error could not carry
    /// it, from [`ModelClient::take_failed_invocation`]; it is recorded
    /// once either way. The run fails with the model's code: a budget the
    /// call exhausted changes nothing more, as for a refused document.
    fn model_failed<S: EventSink>(
        &mut self,
        sink: &mut S,
        agg: RunAggregate,
        stage: &str,
        error: &ModelError,
        budget: &mut TokenBudget,
    ) -> Result<RunAggregate, OrchestratorError> {
        // Always taken, so a kept call can never surface on a later error.
        let kept = self.model.take_failed_invocation();
        let invocation = error.invocation().cloned().or(kept);
        if let Some(call) = &invocation {
            let _ = budget.charge(stage, call);
        }
        self.fail_with(
            sink,
            agg,
            stage,
            model_error_code(error),
            &error.to_string(),
            invocation,
        )
    }

    /// A model document the contract refuses. The run fails at the call's
    /// stage and the call stays on the record (`RunFailed.invocation`), so
    /// its request id and tokens still reach the receipt.
    fn refuse<S: EventSink>(
        &mut self,
        sink: &mut S,
        agg: RunAggregate,
        stage: &str,
        code: &str,
        violation: &ContractViolation,
        invocation: ModelInvocation,
    ) -> Result<RunAggregate, OrchestratorError> {
        let reason = format!("{stage} output refused by the protocol 1.3 contract: {violation}");
        self.fail_with(sink, agg, stage, code, &reason, Some(invocation))
    }
}

/// The latest evaluation as the log recorded it: what the policy and the
/// next role calls read, never a local copy.
fn recorded_evaluation(agg: &RunAggregate) -> Result<Evaluation, OrchestratorError> {
    agg.last_evaluation.clone().ok_or_else(|| {
        TransitionError::InvalidTransition {
            state: agg.state,
            event: "recommendation_issued",
        }
        .into()
    })
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
                    code: Some(codes::INTERRUPTED.into()),
                    invocation: None,
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
            EventPayload::RunFailed { stage, reason, .. } => (stage.as_str(), reason.as_str()),
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

    /// The scripted verifier, plus an owner's cancel that arrives while the
    /// first proof verification is in flight.
    struct CancellingVerifier {
        inner: FakeProofVerifier,
        control: RunControl,
        verifies: usize,
    }

    impl ProofVerifier for CancellingVerifier {
        fn verify(&mut self, evidence: &EvidenceReference) -> ProofCheck {
            self.verifies += 1;
            let out = self.inner.verify(evidence);
            self.control.cancel(CancelReason::Owner);
            out
        }
    }

    #[test]
    fn cancel_during_a_proof_verification_is_honored_before_the_next_one() {
        let s = retry_constraint_scenario();
        let control = RunControl::new();
        let mut orch = Orchestrator {
            model: s.model,
            memory: s.memory,
            verifier: CancellingVerifier {
                inner: s.verifier,
                control: control.clone(),
                verifies: 0,
            },
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        let mut log = EventLog::open(temp_log("cancel-during-verify.jsonl")).unwrap();
        let run_id = RunId::new(orch.ids.next("run"));
        let (agg, task) = orch.create_task(&mut log, run_id, s.input).unwrap();
        let agg = orch.deliberate(&mut log, agg, &task, &control).unwrap();

        // The round context holds two records: the second was never
        // verified, and the Critic was never called.
        assert_eq!(orch.verifier.verifies, 1);
        assert_eq!(orch.model.calls, ["propose"]);
        assert_eq!(
            failure(&log),
            (
                "cancelled",
                "cancelled by the run's owner before the proof verification"
            )
        );
        assert_eq!(agg.state, RunState::Failed);
        assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "replays");
    }

    #[test]
    fn every_failure_records_a_stable_code_next_to_its_reason() {
        let code = |log: &EventLog| match &log.all_events().last().unwrap().payload {
            EventPayload::RunFailed { code, .. } => code.clone(),
            other => panic!("expected RunFailed last, got {other:?}"),
        };
        let control = RunControl::new();
        control.cancel(CancelReason::Shutdown);
        let (_, log) = deliberate_tripped("code-shutdown.jsonl", &control);
        assert_eq!(code(&log).as_deref(), Some("shutdown"));

        let mut model = FakeModelClient::new();
        model.proposals.push_back(Err(ModelError::Timeout));
        let s = retry_constraint_scenario();
        let mut log = EventLog::open(temp_log("code-timeout.jsonl")).unwrap();
        let mut orch = orchestrator(model, s.memory, s.verifier);
        orch.run(&mut log, s.input).unwrap();
        assert_eq!(code(&log).as_deref(), Some(codes::MODEL_TIMEOUT));

        // Every other code, each from the smallest run that records it:
        // RunFailed.code for a failure, EvidenceUnavailable.code for a
        // degraded retrieval, always next to its English reason.
        let mut recorded: Vec<&str> = vec!["shutdown", codes::MODEL_TIMEOUT];
        let mut note = |code: &'static str, log: &EventLog| {
            let found = log.all_events().iter().any(|e| match &e.payload {
                EventPayload::RunFailed {
                    code: c, reason, ..
                }
                | EventPayload::EvidenceUnavailable {
                    code: c, reason, ..
                } => c.as_deref() == Some(code) && !reason.is_empty(),
                _ => false,
            });
            assert!(found, "{code} is not recorded next to a reason");
            recorded.push(code);
        };
        let control = RunControl::new();
        control.cancel(CancelReason::Owner);
        note(
            "cancelled",
            &deliberate_tripped("code-cancelled.jsonl", &control).1,
        );
        let control = RunControl::with_deadline(Instant::now());
        note(
            "deadline",
            &deliberate_tripped("code-deadline.jsonl", &control).1,
        );

        let reference = || retry_constraint_scenario().model;
        let memory = || retry_constraint_scenario().memory;
        let mut model = reference();
        model.critiques.clear();
        model
            .critiques
            .push_back(Err(ModelError::malformed("missing field `items`")));
        note(
            codes::MODEL_MALFORMED_OUTPUT,
            &run_log("code-malformed.jsonl", model, memory(), None),
        );
        // A client's own codes: truncated output and a refusal.
        let mut model = reference();
        model.critiques.clear();
        model
            .critiques
            .push_back(Err(ModelError::malformed_with_code(
                codes::MODEL_OUTPUT_TRUNCATED,
                "the output reached its token bound",
            )));
        note(
            codes::MODEL_OUTPUT_TRUNCATED,
            &run_log("code-truncated.jsonl", model, memory(), None),
        );
        let mut model = reference();
        model.critiques.clear();
        model
            .critiques
            .push_back(Err(ModelError::Refused { invocation: None }));
        note(
            codes::MODEL_REFUSAL,
            &run_log("code-refusal.jsonl", model, memory(), None),
        );
        note(
            codes::MODEL_UNAVAILABLE,
            &run_log(
                "code-unavailable.jsonl",
                FakeModelClient::new(),
                memory(),
                None,
            ),
        );
        let mut model = reference();
        model.tokens_per_call = (400, 200);
        note(
            codes::TOKEN_BUDGET_EXCEEDED,
            &run_log("code-budget.jsonl", model, memory(), Some(1_000)),
        );
        let mut model = reference();
        model.omit_usage = true;
        note(
            codes::TOKEN_USAGE_MISSING,
            &run_log("code-usage.jsonl", model, memory(), None),
        );

        // The contract codes: a repeated claim id, reused item ids and a
        // missing verdict, each refused before it is recorded.
        let mut model = reference();
        model.proposals[0].as_mut().unwrap().claims[1].id = ClaimId::new("claim-1");
        note(
            codes::PROPOSAL_CONTRACT,
            &run_log("code-proposal.jsonl", model, memory(), None),
        );
        let mut model = reference();
        let reused = model.critiques[0].as_ref().unwrap().items.clone();
        model.critiques[1].as_mut().unwrap().items = reused;
        note(
            codes::CHALLENGE_CONTRACT,
            &run_log("code-challenge.jsonl", model, memory(), None),
        );
        let mut model = reference();
        model.evaluations[0].as_mut().unwrap().item_verdicts.pop();
        note(
            codes::EVALUATION_CONTRACT,
            &run_log("code-evaluation.jsonl", model, memory(), None),
        );

        // Retrieval codes: offline, a backend error, and the per-run cap
        // (five claims recalling five records each: 25 for a cap of 20).
        note(
            codes::MEMORY_UNAVAILABLE,
            &run_log(
                "code-offline.jsonl",
                reference(),
                FakeMemoryClient::offline(),
                None,
            ),
        );
        note(
            codes::MEMORY_ERROR,
            &run_log("code-memory-error.jsonl", reference(), FailingMemory, None),
        );
        let topics = ["alpha", "bravo", "charlie", "delta", "echo"];
        let records = topics
            .iter()
            .flat_map(|t| {
                (0..5).map(move |j| {
                    let content = format!("{t}topic record {j}");
                    crate::scenarios::evidence(
                        &format!("ev-{t}-{j}"),
                        "demo-repo",
                        "fact",
                        &content,
                    )
                })
            })
            .collect();
        let mut model = FakeModelClient::new();
        model.proposals.push_back(Ok(crate::scenarios::proposal(
            "p1",
            "s",
            topics
                .iter()
                .map(|t| crate::scenarios::claim(&format!("c-{t}"), &format!("{t}topic")))
                .collect(),
        )));
        note(
            codes::EVIDENCE_CAP,
            &run_log(
                "code-cap.jsonl",
                model,
                FakeMemoryClient::new(records),
                None,
            ),
        );
        // A record the backend attributed to another project (TC-4).
        let foreign = ScriptedRecall(Recall {
            records: memory().records,
            foreign: vec![ForeignRecord {
                evidence_id: EvidenceId::new("ev-other"),
                backend_project: Some("other-repo".into()),
            }],
            ..Recall::default()
        });
        note(
            codes::FOREIGN_RECORD,
            &run_log("code-foreign.jsonl", reference(), foreign, None),
        );

        // A restart closes a run left mid-deliberation.
        let s = retry_constraint_scenario();
        let mut log = EventLog::open(temp_log("code-interrupted.jsonl")).unwrap();
        let mut orch = orchestrator(s.model, s.memory, s.verifier);
        create(&mut orch, &mut log, s.input);
        let recovery = mark_interrupted_runs(&mut log, &mut orch.ids, TS.into()).unwrap();
        assert_eq!(recovery.interrupted.len(), 1);
        note(codes::INTERRUPTED, &log);

        for code in [
            "cancelled",
            "shutdown",
            "deadline",
            codes::EVALUATION_CONTRACT,
            codes::CHALLENGE_CONTRACT,
            codes::PROPOSAL_CONTRACT,
            codes::MODEL_TIMEOUT,
            codes::MODEL_MALFORMED_OUTPUT,
            codes::MODEL_OUTPUT_TRUNCATED,
            codes::MODEL_REFUSAL,
            codes::MODEL_UNAVAILABLE,
            codes::TOKEN_BUDGET_EXCEEDED,
            codes::TOKEN_USAGE_MISSING,
            codes::INTERRUPTED,
            codes::MEMORY_UNAVAILABLE,
            codes::MEMORY_ERROR,
            codes::EVIDENCE_CAP,
            codes::FOREIGN_RECORD,
        ] {
            assert!(recorded.contains(&code), "{code} is never exercised");
        }
    }

    /// One run of `model` against `memory` (every proof Valid) with the
    /// reference task and an optional token cap; returns its log.
    fn run_log<Mem: MemoryClient>(
        name: &str,
        model: FakeModelClient,
        memory: Mem,
        max_run_tokens: Option<u64>,
    ) -> EventLog {
        let mut log = EventLog::open(temp_log(name)).unwrap();
        let mut limits = RunLimits::default();
        if let Some(cap) = max_run_tokens {
            limits.max_run_tokens = cap;
        }
        let mut orch = Orchestrator {
            model,
            memory,
            verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits,
        };
        orch.run(&mut log, retry_constraint_scenario().input)
            .unwrap();
        log
    }

    /// A memory backend whose every recall fails with a backend error.
    struct FailingMemory;

    impl MemoryClient for FailingMemory {
        fn recall(
            &mut self,
            _project: &ProjectId,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<EvidenceReference>, MemoryError> {
            Err(MemoryError::Backend("index unreadable".into()))
        }
        fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
            Err(MemoryError::Backend("index unreadable".into()))
        }
    }

    #[test]
    fn an_answered_call_whose_output_was_refused_stays_on_the_record() {
        // TC-7: a client that refuses a document after the provider
        // answered hands the call back on its error; the run records it.
        let s = retry_constraint_scenario();
        let answered = ModelInvocation {
            provider: "fake".into(),
            model: "muse-wonder-1.0".into(),
            request_id: Some("fake-refused-critique".into()),
            latency_ms: Some(1),
            input_tokens: Some(7),
            output_tokens: Some(3),
            finish_reason: Some("stop".into()),
            reasoning_tokens: None,
            max_completion_tokens: None,
            schema_name: Some("DraftChallenge".into()),
            schema_sha256: None,
            prompt_sha256: None,
            reasoning_effort: None,
            safety_identifier_sent: None,
            attempts: Some(1),
        };
        let mut model = s.model;
        model.critiques.clear();
        model.critiques.push_back(Err(ModelError::malformed(
            "challenge item 1 of 1 states no objection",
        )
        .after_call(answered.clone())));
        let mut log = EventLog::open(temp_log("refused-answer.jsonl")).unwrap();
        let mut orch = orchestrator(model, s.memory, s.verifier);
        let agg = orch.run(&mut log, s.input).unwrap();
        match &log.all_events().last().expect("a record").payload {
            EventPayload::RunFailed {
                stage,
                code,
                invocation,
                ..
            } => {
                assert_eq!(stage, "critique");
                assert_eq!(code.as_deref(), Some(codes::MODEL_MALFORMED_OUTPUT));
                assert_eq!(invocation.as_ref(), Some(&answered));
            }
            other => panic!("expected RunFailed last, got {other:?}"),
        }
        assert_eq!(agg.invocations.len(), 2, "propose and the refused critique");
        assert_eq!(agg.invocations[1], answered);
        let receipt = build_receipt(&log, &agg.run_id, TS.into()).unwrap();
        assert_eq!(receipt.model_invocations, agg.invocations);
        assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "replays");
        // An error with no answer behind it records no call.
        assert!(ModelError::Timeout
            .after_call(answered)
            .invocation()
            .is_none());
    }

    /// A client whose every call fails with `error` and that keeps `kept`
    /// for [`ModelClient::take_failed_invocation`].
    struct FailingAfterAnswer {
        error: ModelError,
        kept: Option<ModelInvocation>,
        taken: usize,
    }

    impl ModelClient for FailingAfterAnswer {
        fn propose(&mut self, _: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
            Err(self.error.clone())
        }
        fn critique(
            &mut self,
            _: &Proposal,
            _: &RoleContext,
        ) -> Result<(Challenge, ModelInvocation), ModelError> {
            Err(self.error.clone())
        }
        fn evaluate(
            &mut self,
            _: &Proposal,
            _: &Challenge,
            _: &RoleContext,
        ) -> Result<(Evaluation, ModelInvocation), ModelError> {
            Err(self.error.clone())
        }
        fn revise(
            &mut self,
            _: &Proposal,
            _: &Challenge,
            _: &Evaluation,
            _: &RoleContext,
        ) -> Result<(Revision, ModelInvocation), ModelError> {
            Err(self.error.clone())
        }
        fn take_failed_invocation(&mut self) -> Option<ModelInvocation> {
            self.taken += 1;
            self.kept.take()
        }
    }

    fn answered_call(request_id: &str) -> ModelInvocation {
        ModelInvocation {
            provider: "meta".into(),
            model: "muse-wonder-1.0".into(),
            request_id: Some(request_id.into()),
            latency_ms: Some(5),
            input_tokens: Some(70),
            output_tokens: Some(30),
            finish_reason: None,
            reasoning_tokens: None,
            max_completion_tokens: Some(16_000),
            schema_name: Some("DraftProposal".into()),
            schema_sha256: None,
            prompt_sha256: None,
            reasoning_effort: None,
            safety_identifier_sent: Some(false),
            attempts: Some(1),
        }
    }

    /// Runs `model` against the reference memory; returns the run, its
    /// log and the client.
    fn run_failing(
        name: &str,
        model: FailingAfterAnswer,
    ) -> (RunAggregate, EventLog, FailingAfterAnswer) {
        let s = retry_constraint_scenario();
        let mut log = EventLog::open(temp_log(name)).unwrap();
        let mut orch = orchestrator(model, s.memory, s.verifier);
        let agg = orch.run(&mut log, s.input).unwrap();
        (agg, log, orch.model)
    }

    fn last_failure(log: &EventLog) -> (String, Option<String>, Option<ModelInvocation>) {
        match &log.all_events().last().expect("a record").payload {
            EventPayload::RunFailed {
                stage,
                code,
                invocation,
                ..
            } => (stage.clone(), code.clone(), invocation.clone()),
            other => panic!("expected RunFailed last, got {other:?}"),
        }
    }

    #[test]
    fn a_call_kept_by_the_client_is_recorded_once_with_the_clients_code() {
        // TC-7: the error cannot carry the call (a body read that timed
        // out after the answer); the client keeps it, the run records it.
        let kept = answered_call("chatcmpl-kept");
        let (agg, log, model) = run_failing(
            "kept-call.jsonl",
            FailingAfterAnswer {
                error: ModelError::Provider {
                    code: "meta_timeout",
                    message: "the Meta API answered but its response did not arrive".into(),
                },
                kept: Some(kept.clone()),
                taken: 0,
            },
        );
        assert_eq!(
            last_failure(&log),
            (
                "propose".into(),
                Some("meta_timeout".into()),
                Some(kept.clone())
            )
        );
        assert_eq!(agg.invocations, vec![kept.clone()]);
        assert_eq!(model.taken, 1);
        assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "replays");

        // The same call on the error and kept by the client: once.
        let (agg, log, _) = run_failing(
            "kept-and-carried.jsonl",
            FailingAfterAnswer {
                error: ModelError::malformed("draft parse: expected value")
                    .after_call(kept.clone()),
                kept: Some(kept.clone()),
                taken: 0,
            },
        );
        assert_eq!(
            last_failure(&log),
            (
                "propose".into(),
                Some(codes::MODEL_MALFORMED_OUTPUT.into()),
                Some(kept.clone())
            )
        );
        assert_eq!(agg.invocations, vec![kept]);

        // No answer behind the error, nothing kept: no call recorded.
        let (agg, log, model) = run_failing(
            "nothing-kept.jsonl",
            FailingAfterAnswer {
                error: ModelError::Timeout,
                kept: None,
                taken: 0,
            },
        );
        assert_eq!(
            last_failure(&log),
            ("propose".into(), Some(codes::MODEL_TIMEOUT.into()), None)
        );
        assert!(agg.invocations.is_empty());
        assert_eq!(model.taken, 1, "always taken, so nothing stale survives");
    }

    #[test]
    fn truncated_and_refused_outputs_record_their_own_codes() {
        for (error, code) in [
            (
                ModelError::malformed_with_code(codes::MODEL_OUTPUT_TRUNCATED, "cut at the bound"),
                codes::MODEL_OUTPUT_TRUNCATED,
            ),
            (
                ModelError::Refused { invocation: None },
                codes::MODEL_REFUSAL,
            ),
            (
                ModelError::Provider {
                    code: "meta_unauthorized",
                    message: "the Meta API rejected the API key (HTTP 401)".into(),
                },
                "meta_unauthorized",
            ),
        ] {
            let call = answered_call("chatcmpl-coded");
            let (agg, log, _) = run_failing(
                &format!("coded-{code}.jsonl"),
                FailingAfterAnswer {
                    error: error.clone().after_call(call.clone()),
                    kept: None,
                    taken: 0,
                },
            );
            let (stage, recorded, invocation) = last_failure(&log);
            assert_eq!(
                (stage.as_str(), recorded.as_deref()),
                ("propose", Some(code))
            );
            // A refusal and malformed output carry the answered call.
            let answered = !matches!(error, ModelError::Provider { .. });
            assert_eq!(invocation.is_some(), answered, "{code}");
            assert_eq!(agg.invocations.len(), usize::from(answered), "{code}");
        }
    }

    /// One scripted recall result for every query.
    struct ScriptedRecall(Recall);

    impl MemoryClient for ScriptedRecall {
        fn recall(
            &mut self,
            _: &ProjectId,
            _: &str,
            _: usize,
        ) -> Result<Vec<EvidenceReference>, MemoryError> {
            Ok(self.0.records.clone())
        }
        fn recall_with_proof(
            &mut self,
            _: &ProjectId,
            _: &str,
            _: usize,
        ) -> Result<Recall, MemoryError> {
            Ok(self.0.clone())
        }
        fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
            Ok(MemoryStatus {
                healthy: true,
                detail: "scripted".into(),
            })
        }
    }

    #[test]
    fn foreign_records_are_dropped_and_proofs_recorded_even_when_empty() {
        let s = retry_constraint_scenario();
        let own = s.memory.records.clone();
        let request = |project: &str| ProofRequest {
            operation: "recall".into(),
            project: project.into(),
            query: None,
            limit: Some(5),
        };
        let memory = ScriptedRecall(Recall {
            records: own.clone(),
            proof: Some(RecallProof {
                request: request("demo-repo"),
                evidence_ids: own
                    .iter()
                    .map(|r| r.id.clone())
                    .chain([EvidenceId::new("ev-foreign")])
                    .collect(),
                proof_ref: "proof-1".into(),
                anchor: "ab".repeat(32),
            }),
            proof_failure: None,
            foreign: vec![ForeignRecord {
                evidence_id: EvidenceId::new("ev-foreign"),
                backend_project: Some("tenant-b".into()),
            }],
        });
        let mut log = EventLog::open(temp_log("foreign-and-proven.jsonl")).unwrap();
        let mut orch = orchestrator(s.model, memory, s.verifier);
        let agg = orch.run(&mut log, s.input).unwrap();
        // The reference run still ends as documented: the foreign record
        // never entered it.
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
        assert!(agg.retrieved.iter().all(|e| e.id.as_str() != "ev-foreign"));
        let events = log.all_events();
        let dropped = events
            .iter()
            .filter(|e| {
                matches!(&e.payload, EventPayload::EvidenceUnavailable { code, reason, .. }
                    if code.as_deref() == Some(codes::FOREIGN_RECORD)
                        && reason.contains("\"ev-foreign\"")
                        && reason.contains("\"tenant-b\""))
            })
            .count();
        let proven = events
            .iter()
            .filter(|e| matches!(&e.payload, EventPayload::RecallProven { proof_ref, .. } if proof_ref == "proof-1"))
            .count();
        // One drop and one proof per recall: two claims, two rounds.
        assert_eq!((dropped, proven), (4, 4));
        assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "replays");

        // A proven empty recall is recorded too.
        let s = retry_constraint_scenario();
        let empty = ScriptedRecall(Recall {
            proof: Some(RecallProof {
                request: request("demo-repo"),
                evidence_ids: vec![],
                proof_ref: "proof-empty".into(),
                anchor: "ab".repeat(32),
            }),
            ..Recall::default()
        });
        let mut log = EventLog::open(temp_log("proven-empty.jsonl")).unwrap();
        let mut orch = orchestrator(s.model, empty, s.verifier);
        let agg = orch.run(&mut log, s.input).unwrap();
        assert!(log.all_events().iter().any(|e| matches!(
            &e.payload,
            EventPayload::RecallProven { empty: true, evidence_ids, .. } if evidence_ids.is_empty()
        )));
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

    /// Live run M01 (2026-10-30, `research/2026-10-30/.../M01.json`): the
    /// seven answered calls' (input, output) tokens, then two p90 round-3
    /// calls (critique 2.5k in + 5.8k out, evaluate 2.5k in + 6.4k out)
    /// in place of the critique that timed out: a full three-round run.
    const M01_THREE_ROUNDS: [(&str, u64, u64); 9] = [
        ("propose", 281, 2_936),
        ("critique", 1_159, 4_458),
        ("evaluate", 1_977, 6_003),
        ("revise", 2_542, 2_995),
        ("critique", 2_004, 6_780),
        ("evaluate", 2_563, 7_086),
        ("revise", 2_108, 3_050),
        ("critique", 2_500, 5_800),
        ("evaluate", 2_500, 6_400),
    ];

    /// The first call after which a run capped at `cap` fails its token
    /// budget, if any.
    fn first_over_budget(cap: u64) -> Option<(usize, &'static str)> {
        let mut budget = TokenBudget {
            cap,
            used: 0,
            calls: 0,
        };
        M01_THREE_ROUNDS
            .iter()
            .enumerate()
            .find_map(|(i, (stage, input, output))| {
                let call = ModelInvocation {
                    provider: "meta".into(),
                    model: "muse-wonder-1.0".into(),
                    request_id: None,
                    latency_ms: None,
                    input_tokens: Some(*input),
                    output_tokens: Some(*output),
                    finish_reason: Some("stop".into()),
                    reasoning_tokens: None,
                    max_completion_tokens: Some(16_000),
                    schema_name: None,
                    schema_sha256: None,
                    prompt_sha256: None,
                    reasoning_effort: None,
                    safety_identifier_sent: Some(true),
                    attempts: Some(1),
                };
                budget.charge(stage, &call).map(|(code, _)| {
                    assert_eq!(code, codes::TOKEN_BUDGET_EXCEEDED);
                    (i, *stage)
                })
            })
    }

    #[test]
    fn a_three_round_m01_run_fits_the_90k_cap_with_room_and_the_70k_cap_barely() {
        let total: u64 = M01_THREE_ROUNDS.iter().map(|(_, i, o)| i + o).sum();
        // M01's seven calls used 45,942 tokens (docs/evaluation.md §6.3).
        let m01: u64 = M01_THREE_ROUNDS[..7].iter().map(|(_, i, o)| i + o).sum();
        assert_eq!(m01, 45_942);
        assert_eq!(total, 63_142);
        // 90,000 (round 8, public.env): no token_budget_exceeded, with
        // 26,858 tokens to spare.
        assert_eq!(first_over_budget(90_000), None);
        // 70,000, the cap deployed on 2026-11-01: these p90 figures still
        // fit, but with 6,858 tokens (under 10 %) to spare...
        assert_eq!(first_over_budget(70_000), None);
        assert!((70_000 - total) * 10 < 70_000);
        // ...and the budget is checked after each call: a cap one token
        // below the total fails the run after its last, already billed,
        // evaluation (M01's own round-2 evaluation was 9,649 tokens, 749
        // above the p90 used here).
        assert_eq!(first_over_budget(total - 1), Some((8, "evaluate")));
        assert_eq!(first_over_budget(total), None, "the cap is inclusive");
    }
}
