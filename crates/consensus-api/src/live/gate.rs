// SPDX-License-Identifier: Apache-2.0
//! The public live gate (plan B9, ADR-0007): the one place where a
//! visitor's request becomes a run on the owner's Meta key. It implements
//! [`crate::api::LiveGate`] over the admission library of this module
//! tree, in the order ABUSE-9 fixes, stopping at the first refusal and
//! spending nothing before the ledger has reserved the run's worst case:
//!
//! 1. the preset must be one of the five curated tasks (`404
//!    preset_unknown`);
//! 2. the geographic policy on the live surface: the portal-wide list,
//!    the live-only list (South Korea) and an unknown country all refuse
//!    (`451` with the geo module's code);
//! 3. the 18+ and territory acknowledgment (`422 attestation_required`);
//! 4. Turnstile: a token is required (`403 turnstile_required`) and
//!    verified server-side, fail closed: an outage is `503
//!    turnstile_unavailable`, anything else `403 turnstile_failed`, the
//!    one public code for every refusal. `remoteip` is not sent (the
//!    client key is a /32 or /64 prefix, not an address); the
//!    `idempotency_key` is a fresh UUID for every call, never the
//!    request's `Idempotency-Key`, so a token is single-use at Siteverify
//!    and one solved challenge admits at most one run (the note below);
//! 5. the lane: a judge code the digest file knows enters
//!    [`Lane::Judge`]; anything else is the public lane, never an error,
//!    and nothing in the response says whether codes exist;
//! 6. the reservation ([`Ledger::reserve`]): a refusal is answered with a
//!    stable code ([`refusal_code`]), `429` and `Retry-After` when the
//!    ledger gives a hint, `503` otherwise, and a fixed sentence, never the
//!    refusal's own text (it names amounts, trip details and files);
//! 7. the model client for this run: the server's Meta configuration with
//!    the session pseudonym as `safety_identifier` and the live
//!    `max_completion_tokens`, metered against the reservation
//!    ([`Ledger::meter`]);
//! 8. the deliberation, built by the [`DeliberationFactory`] (the sealed
//!    snapshot in production, ADR-0008) and wrapped so the reservation is
//!    released when the run ends, however it ends;
//! 9. the grant: the preset's canned task declared `{meta, <the factory's
//!    evidence mode>, public, <preset>}`, the deliberation, and a refund
//!    closure for a start that fails after admission.
//!
//! Everything blocking (the ledger, Siteverify, building the HTTP client)
//! runs on the blocking pool with owned data; the gate's future never
//! blocks an async worker.
//!
//! **Siteverify's idempotency key.** Cloudflare re-validates a token under
//! the same `idempotency_key` for its whole validity window, and that
//! idempotency is global and immediate, while the route's own
//! `Idempotency-Key` replay is scoped to the owner and recorded only after
//! the run has started. Forwarding the request's header would therefore
//! let one solved challenge start a run per session, or several at once
//! from one session. The gate never reuses a key: every verification gets
//! a fresh UUID v4, so Siteverify refuses a second presentation of the
//! same token (`timeout-or-duplicate`) and a run always costs its visitor
//! one solved challenge. A retry whose first attempt succeeded is still
//! answered by the route from the stored creation before the gate is
//! asked; one whose first attempt died after Siteverify needs a new,
//! usually invisible, challenge.
//!
//! **Reservation lifetime.** One [`ReservationHolder`] per admitted run,
//! shared by the wrapped deliberation and the refund closure, releases the
//! reservation exactly once, always with the bounded retries of
//! [`RELEASE_BACKOFF`] (100/300/900 ms): when `deliberate` returns; when
//! the deliberation is dropped without having done so (a panic caught by
//! the run manager, a task creation that failed inside the start task, a
//! grant whose caller went away); or when the route reports a start that
//! was refused before the deliberation reached the start task (the server
//! draining), on a blocking task. The deliberation's `Drop` is the release
//! of a failed start in practice; the grant's refund closure then finds
//! the reservation released and is the fallback for the other refusals.
//! Every known drop happens on a blocking thread; should one happen on an
//! async worker, the release runs under `block_in_place`, so the worker's
//! other tasks are not stalled by the ledger's lock wait. A release that
//! still fails is logged with the reservation id; the ledger keeps
//! counting it at its worst case until an operator reconciles it with
//! [`Ledger::release_by_id`]. The Turnstile token spent on a failed start
//! cannot be refunded: the visitor solves a new challenge.

use super::budget::{Lane, Ledger, Refusal, Reservation, ReservationId, Snapshot, TripReason};
use super::config::LiveConfig;
use super::geo::{GeoDecision, GeoPolicy, Surface as GeoSurface};
use super::judge::JudgeCodes;
use super::presets::{PresetId as LivePresetId, PRESETS};
use super::turnstile::Siteverify;
use crate::api::error::{ApiError, ErrorCode};
use crate::api::types::{
    EvidenceProvenance, LiveCapabilitiesV1, LiveReadyV1, ModelProvenance, PresetV1, PublicBudgetV1,
};
use crate::api::{geo_message, AdmitFuture, LiveAdmissionRequest, LiveGrant};
use crate::config::MetaConfig;
use crate::meta::MetaModelClient;
use crate::runs::Deliberation;
use consensus_domain::{EvidenceMode, ModelMode, RunId, RunOrigin, Task, TaskMode};
use consensus_runtime::orchestrator::OrchestratorError;
use consensus_runtime::{RunAggregate, RunControl, RunLimits, SharedRunLog, TaskInput};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

/// The variable the sealed-snapshot client is configured by
/// ([`crate::config::SnapshotConfig`],
/// [`super::snapshot_factory::SnapshotFactory::from_env`]); named by the
/// startup error when live mode is enabled without it.
pub const SNAPSHOT_DIR_VAR: &str = "CONSENSUS_SNAPSHOT_DIR";
/// How old the ledger view behind `/capabilities` and `/ready` may be
/// ([`Ledger::snapshot_cached`]).
pub const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(5);
/// Waits between the retries of a release that failed (the lock wait ran
/// out, an I/O error): three retries, then the id is logged.
pub const RELEASE_BACKOFF: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(300),
    Duration::from_millis(900),
];

// ---------------------------------------------------------------------
// The deliberation factory (the seam for the sealed snapshot client)
// ---------------------------------------------------------------------

/// Why a factory could not build a run's deliberation. Operator-only
/// text: the visitor gets `503 live_unavailable`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct FactoryError(pub String);

/// The evidence source as `/ready` sees it: plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryStatus {
    /// False when the source failed its check (a snapshot that did not
    /// verify at boot, say): `/ready` then says not ready.
    pub ok: bool,
    /// The evidence mode the factory declares.
    pub kind: EvidenceMode,
    /// A short stable code describing the state (`ok`, `boot_check_failed`,
    /// ...), never a path.
    pub detail: Option<String>,
}

/// Builds the deliberation of one admitted live run around the metered
/// model client the gate made for it.
///
/// The production factory is the sealed Hyphae snapshot (ADR-0008,
/// [`super::snapshot_factory::SnapshotFactory`]): a `SnapshotMemoryClient`
/// serving the proven `demo-repo` record list with the pinned anchor, and
/// the hardened proof verifier, declaring [`EvidenceMode::SealedSnapshot`].
/// It exists only when `CONSENSUS_SNAPSHOT_DIR` is set and the snapshot
/// verified at boot; without one, `serve-public` refuses to start with
/// live mode enabled (see [`crate::server::build_live_gate`]). Tests plug
/// in a scripted factory over the runtime's fakes. There is no fallback to
/// simulated evidence in production: the mode a factory declares is the
/// one every run it builds is labelled with.
pub trait DeliberationFactory: Send + Sync {
    /// The evidence mode every deliberation this factory builds runs in;
    /// recorded as `Task.mode.evidence` and published by `/capabilities`.
    fn evidence_mode(&self) -> EvidenceMode;

    /// One run's deliberation over `model` (already metered) with the
    /// live run limits.
    fn build(
        &self,
        model: MetaModelClient,
        limits: RunLimits,
    ) -> Result<Box<dyn Deliberation>, FactoryError>;

    /// The source's state, for `/ready`.
    fn status(&self) -> FactoryStatus;
}

/// The API's name for an evidence mode.
pub fn evidence_provenance(mode: EvidenceMode) -> EvidenceProvenance {
    match mode {
        EvidenceMode::Simulated => EvidenceProvenance::Simulated,
        EvidenceMode::SealedSnapshot => EvidenceProvenance::HyphaeSnapshot,
        EvidenceMode::LiveHyphae => EvidenceProvenance::HyphaeLive,
        EvidenceMode::Offline => EvidenceProvenance::None,
    }
}

// ---------------------------------------------------------------------
// Refusal mapping
// ---------------------------------------------------------------------

/// The stable public code of a ledger refusal at admission. Every variant
/// maps; the draw-time refusals (an exhausted reservation, the per-call
/// caps) cannot happen at admission and read as `live_unavailable`, like a
/// ledger that could not be consulted.
pub fn refusal_code(refusal: &Refusal) -> ErrorCode {
    match refusal {
        Refusal::Tripped { .. } => ErrorCode::LiveTripped,
        Refusal::DailyBudget { .. } => ErrorCode::LiveBudgetDaily,
        Refusal::LifetimeBudget => ErrorCode::LiveBudgetLifetime,
        Refusal::JudgeSlice => ErrorCode::LiveBudgetJudgeSlice,
        Refusal::JudgeDaily { .. } => ErrorCode::LiveBudgetJudgeDaily,
        Refusal::HourlyAdmissions { .. } => ErrorCode::LiveHourlyAdmissions,
        Refusal::Concurrency { .. } => ErrorCode::LiveConcurrency,
        Refusal::ReservationExhausted { .. }
        | Refusal::InputCap { .. }
        | Refusal::OutputCap { .. }
        | Refusal::Ledger(_) => ErrorCode::LiveUnavailable,
    }
}

/// The fixed sentence answered for a live-mode refusal code: never the
/// refusal's or the ledger's own text.
pub fn refusal_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::LiveBudgetDaily => {
            "the daily live budget cannot hold another run right now; retry after the given delay"
        }
        ErrorCode::LiveBudgetLifetime => "the lifetime live budget is exhausted",
        ErrorCode::LiveBudgetJudgeDaily => "the judge lane's daily budget is exhausted",
        ErrorCode::LiveBudgetJudgeSlice => "the judge lane's budget is exhausted",
        ErrorCode::LiveConcurrency => "every live slot is busy; retry shortly",
        ErrorCode::LiveHourlyAdmissions => {
            "the hourly cap on live runs was reached; retry after the given delay"
        }
        ErrorCode::LiveTripped => "live mode is paused",
        ErrorCode::TurnstileRequired => {
            "a Turnstile token is required (cf-turnstile-response header)"
        }
        ErrorCode::TurnstileFailed => "the Turnstile verification failed",
        ErrorCode::TurnstileUnavailable => {
            "the Turnstile verification service could not be reached; retry shortly"
        }
        ErrorCode::AttestationRequired => {
            "live mode requires the 18+ and territory acknowledgment (attest_adult: true)"
        }
        ErrorCode::PresetUnknown => "unknown preset",
        _ => "live mode is unavailable right now",
    }
}

/// The API error of a ledger refusal: its code, its fixed message, and
/// `Retry-After` when the ledger gives a hint (`429`; `503` otherwise).
pub fn refusal_error(refusal: &Refusal) -> ApiError {
    let code = refusal_code(refusal);
    let error = ApiError::new(code, refusal_message(code));
    match refusal.retry_after() {
        Some(wait) => error.with_retry_after(wait.as_secs()),
        None => error,
    }
}

fn unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::LiveUnavailable,
        refusal_message(ErrorCode::LiveUnavailable),
    )
    .with_retry_after(30)
}

// ---------------------------------------------------------------------
// The reservation holder and the metered deliberation
// ---------------------------------------------------------------------

/// What became of a release request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// The release record is durable.
    Released,
    /// Another holder of the same reservation released it first.
    AlreadyReleased,
    /// Every attempt failed; the id was logged for the operator and the
    /// ledger keeps the worst case.
    Failed,
}

/// One admitted run's reservation, shared by the deliberation that spends
/// it and the refund closure, so that exactly one of them releases it.
pub struct ReservationHolder {
    ledger: Ledger,
    id: ReservationId,
    slot: Mutex<Option<Reservation>>,
}

impl std::fmt::Debug for ReservationHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReservationHolder")
            .field("id", &self.id)
            .field("held", &self.is_held())
            .finish()
    }
}

impl ReservationHolder {
    pub fn new(ledger: Ledger, reservation: Reservation) -> Self {
        Self {
            ledger,
            id: reservation.id().clone(),
            slot: Mutex::new(Some(reservation)),
        }
    }

    pub fn id(&self) -> &ReservationId {
        &self.id
    }

    /// True until a release was attempted (successfully or not).
    pub fn is_held(&self) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Releases the reservation once: the first attempt at once, then one
    /// per entry of `backoff` after that wait. Never panics (it runs from
    /// `Drop`). On a final failure the reservation id is logged; the
    /// handle is then dropped, which frees the in-process slot while the
    /// ledger keeps the worst case for the operator to reconcile.
    pub fn release(&self, backoff: &[Duration], why: &str) -> ReleaseOutcome {
        let taken = self.slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(reservation) = taken else {
            return ReleaseOutcome::AlreadyReleased;
        };
        let mut waits = backoff.iter();
        loop {
            match self.ledger.release(&reservation) {
                Ok(_) => return ReleaseOutcome::Released,
                Err(e) => match waits.next() {
                    Some(wait) => {
                        eprintln!(
                            "[live-gate] releasing reservation {} ({why}) failed, retrying in {} ms: {e}",
                            self.id,
                            wait.as_millis()
                        );
                        std::thread::sleep(*wait);
                    }
                    None => {
                        eprintln!(
                            "[live-gate] reservation {} could not be released ({why}); it keeps counting at its worst case until an operator releases it by id: {e}",
                            self.id
                        );
                        return ReleaseOutcome::Failed;
                    }
                },
            }
        }
    }
}

/// A deliberation that releases its run's reservation when it ends,
/// however it ends (see the module doc).
pub struct MeteredDeliberation {
    inner: Box<dyn Deliberation>,
    holder: Arc<ReservationHolder>,
}

impl MeteredDeliberation {
    pub fn new(inner: Box<dyn Deliberation>, holder: Arc<ReservationHolder>) -> Self {
        Self { inner, holder }
    }

    pub fn holder(&self) -> &Arc<ReservationHolder> {
        &self.holder
    }
}

impl Deliberation for MeteredDeliberation {
    fn create_task(
        &mut self,
        sink: &mut SharedRunLog,
        run_id: RunId,
        input: TaskInput,
    ) -> Result<(RunAggregate, Task), OrchestratorError> {
        self.inner.create_task(sink, run_id, input)
    }

    fn deliberate(
        &mut self,
        sink: &mut SharedRunLog,
        agg: RunAggregate,
        task: &Task,
        control: &RunControl,
    ) -> Result<RunAggregate, OrchestratorError> {
        let result = self.inner.deliberate(sink, agg, task, control);
        // Finished, failed or cancelled: the run spends nothing more.
        self.holder.release(&RELEASE_BACKOFF, "the run ended");
        result
    }
}

impl Drop for MeteredDeliberation {
    fn drop(&mut self) {
        // Catching nothing and panicking never: a run whose deliberation
        // panicked (the manager catches it and drops this afterwards), a
        // task creation that failed inside the start task, or a grant that
        // was never started still ends with its reservation released (with
        // the usual retries) or logged as unreleased with its id. Nothing
        // to do when `deliberate` already released it.
        if !self.holder.is_held() {
            return;
        }
        blocking_section(|| {
            self.holder
                .release(&RELEASE_BACKOFF, "the deliberation was dropped")
        });
    }
}

/// Runs the blocking `f` where it will not stall other tasks: on an async
/// worker of a multi-thread runtime it is wrapped in
/// [`tokio::task::block_in_place`] (the worker hands its tasks to another
/// thread first); on a blocking-pool thread, a current-thread runtime or
/// outside any runtime it simply runs. The ledger's lock wait and fsyncs
/// are what `f` does.
fn blocking_section<R>(f: impl FnOnce() -> R) -> R {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

// ---------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------

/// Everything [`LiveGate::new`] needs. `geo` and `judge_codes` are given
/// next to `config` so a test can script them; the server passes the
/// configuration's own.
pub struct GateParts {
    pub config: LiveConfig,
    pub ledger: Ledger,
    /// [`super::turnstile::TurnstileVerifier::from_config`] over the HTTP
    /// transport in production; a verifier over
    /// [`super::turnstile::FakeSiteverify`] in tests.
    pub siteverify: Arc<dyn Siteverify>,
    pub geo: GeoPolicy,
    pub judge_codes: JudgeCodes,
    /// The server's key and model settings; cloned per run with the
    /// session pseudonym and the live output bound.
    pub meta: MetaConfig,
    /// The live runs' limits (`max_run_tokens` =
    /// `CONSENSUS_LIVE_RUN_TOKEN_CAP`).
    pub run_limits: RunLimits,
    pub factory: Arc<dyn DeliberationFactory>,
    /// How old the ledger view behind `/capabilities` and `/ready` may be:
    /// [`SNAPSHOT_MAX_AGE`] in production; a test lowers it to read the
    /// files on every call.
    pub snapshot_max_age: Duration,
}

struct Inner {
    max_completion_tokens: u64,
    ledger: Ledger,
    /// The kill-switch file of the configuration (`None` when the ledger
    /// has none): its presence is reported by `/ready` and
    /// `/capabilities` as soon as it exists, before the next admission
    /// records the trip.
    kill_switch_file: Option<std::path::PathBuf>,
    snapshot_max_age: Duration,
    siteverify: Arc<dyn Siteverify>,
    geo: GeoPolicy,
    judge_codes: JudgeCodes,
    meta: MetaConfig,
    run_limits: RunLimits,
    factory: Arc<dyn DeliberationFactory>,
}

/// The production [`crate::api::LiveGate`]; see the module doc.
#[derive(Clone)]
pub struct LiveGate {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for LiveGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveGate")
            .field("ledger", &self.inner.ledger)
            .field("evidence", &self.inner.factory.evidence_mode())
            .finish_non_exhaustive()
    }
}

impl LiveGate {
    pub fn new(parts: GateParts) -> Self {
        Self {
            inner: Arc::new(Inner {
                max_completion_tokens: parts.config.budget.max_completion_tokens,
                kill_switch_file: parts.config.budget.kill_switch_file.clone(),
                snapshot_max_age: parts.snapshot_max_age,
                ledger: parts.ledger,
                siteverify: parts.siteverify,
                geo: parts.geo,
                judge_codes: parts.judge_codes,
                meta: parts.meta,
                run_limits: parts.run_limits,
                factory: parts.factory,
            }),
        }
    }

    /// The ledger this gate reserves on (tests read its records).
    pub fn ledger(&self) -> &Ledger {
        &self.inner.ledger
    }

    pub fn evidence_mode(&self) -> EvidenceMode {
        self.inner.factory.evidence_mode()
    }
}

impl Inner {
    /// The whole admission, on a blocking thread, in ABUSE-9 order.
    fn admit_blocking(&self, request: LiveAdmissionRequest) -> Result<LiveGrant, ApiError> {
        // (a) One of the five curated tasks, or nothing.
        let preset_id = LivePresetId::parse(request.preset.as_str()).map_err(|_| {
            ApiError::new(
                ErrorCode::PresetUnknown,
                refusal_message(ErrorCode::PresetUnknown),
            )
        })?;
        let preset = preset_id.preset();

        // (b) The live surface: the portal list, the live-only list, and
        // an unknown country all refuse.
        if let GeoDecision::Block { code, reason } = self
            .geo
            .decide(request.country.as_deref(), GeoSurface::Live)
        {
            eprintln!("[live-gate] refused by the geographic policy: {reason}");
            let code = ErrorCode::from(code);
            return Err(ApiError::new(code, geo_message(code)));
        }

        // (c) The acknowledgment.
        if !request.attest_adult {
            return Err(ApiError::new(
                ErrorCode::AttestationRequired,
                refusal_message(ErrorCode::AttestationRequired),
            ));
        }

        // (d) Turnstile, fail closed. A fresh key for every verification:
        // the token is single-use at Siteverify, whatever the request's
        // own Idempotency-Key was (see the module doc).
        let Some(token) = request.turnstile_token.as_deref() else {
            return Err(ApiError::new(
                ErrorCode::TurnstileRequired,
                refusal_message(ErrorCode::TurnstileRequired),
            ));
        };
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        if let Err(e) = self.siteverify.verify(token, None, &idempotency_key) {
            // The variant is for the operator; the token is never logged.
            eprintln!("[live-gate] turnstile refused a live run: {e}");
            return Err(if e.is_outage() {
                ApiError::new(
                    ErrorCode::TurnstileUnavailable,
                    refusal_message(ErrorCode::TurnstileUnavailable),
                )
                .with_retry_after(5)
            } else {
                ApiError::new(
                    ErrorCode::TurnstileFailed,
                    refusal_message(ErrorCode::TurnstileFailed),
                )
            });
        }

        // (e) The lane. An unknown code is the public lane, silently.
        let lane = match request.judge_code.as_deref() {
            Some(code) if self.judge_codes.verify(code) => Lane::Judge,
            _ => Lane::Public,
        };

        // (f) The reservation: the first and only thing that commits money.
        let reservation = self.ledger.reserve(lane).map_err(|refusal| {
            eprintln!(
                "[live-gate] the ledger refused a {} run: {refusal}",
                lane.as_str()
            );
            refusal_error(&refusal)
        })?;
        let meter = Arc::new(self.ledger.meter(&reservation));
        let holder = Arc::new(ReservationHolder::new(self.ledger.clone(), reservation));

        // (g) This run's model client: the session pseudonym identifies
        // the visitor to Meta, the live output bound caps every call, and
        // every HTTP attempt is drawn on the reservation.
        let mut cfg = self.meta.clone();
        cfg.safety_identifier = Some(request.session_pseudonym.clone());
        cfg.max_completion_tokens = self.max_completion_tokens;
        let model = match MetaModelClient::new(cfg) {
            Ok(model) => model.with_meter(meter),
            Err(e) => {
                // A configuration error names its setting, never the key.
                eprintln!("[live-gate] the model client could not be built: {e}");
                holder.release(&RELEASE_BACKOFF, "the model client could not be built");
                return Err(unavailable());
            }
        };

        // (h) The deliberation over the factory's evidence, wrapped so the
        // reservation is released when the run ends.
        let inner = match self.factory.build(model, self.run_limits) {
            Ok(deliberation) => deliberation,
            Err(e) => {
                eprintln!("[live-gate] the deliberation could not be built: {e}");
                holder.release(&RELEASE_BACKOFF, "the deliberation could not be built");
                return Err(unavailable());
            }
        };
        let deliberation = MeteredDeliberation::new(inner, holder.clone());

        // (i) The grant: the canned task declared with what will actually
        // run, and the refund for a start that fails after this point.
        let mut input = preset.task_input();
        input.mode = Some(TaskMode {
            model: ModelMode::Meta,
            evidence: self.factory.evidence_mode(),
            origin: RunOrigin::Public,
            preset: Some(preset_id.as_str().to_string()),
        });
        let refund = holder.clone();
        Ok(LiveGrant {
            input,
            deliberation: Box::new(deliberation),
            on_start_failed: Some(Box::new(move || {
                // Off the async worker: the ledger does file I/O. A start
                // whose task creation failed has already released the
                // reservation when the start task dropped the deliberation
                // (this then finds nothing to do); a start refused before
                // that, the server draining, is released here. The
                // Turnstile token was spent at Siteverify and cannot be
                // given back; the visitor solves a new challenge.
                let release = move || {
                    refund.release(&RELEASE_BACKOFF, "the run could not be started");
                };
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        handle.spawn_blocking(release);
                    }
                    Err(_) => release(),
                }
            })),
        })
    }

    /// The visitor-safe budget for `/capabilities`: the public projection
    /// of a cached snapshot, with the kill switch overlaid
    /// ([`Inner::trip_state`]). `None` when the ledger cannot be read:
    /// nothing can be reserved then, `/ready` says `ledger_ok: false`, and
    /// the error goes to the operator log; no figure is invented.
    fn public_budget(&self) -> Option<PublicBudgetV1> {
        let snapshot = self.snapshot("/capabilities")?;
        let (tripped, trip_code) = self.trip_state(&snapshot);
        let public = snapshot.public();
        Some(PublicBudgetV1 {
            daily_remaining: public.daily_remaining.to_fixed(),
            daily_available_now: public.daily_available_now.to_fixed(),
            lifetime_remaining: public.lifetime_remaining.to_fixed(),
            tripped,
            trip_code,
        })
    }

    /// Whether live mode is paused, and why: the ledger's recorded trip,
    /// or the kill-switch file when it exists and no trip is recorded yet.
    /// The file is the operator's order and takes effect at the next
    /// admission ([`Ledger::reserve`] trips on it); reporting it here
    /// makes `/ready` and `/capabilities` say so at once instead of
    /// staying green until a visitor submits. A read of the file's
    /// metadata only: these routes never write to the ledger.
    fn trip_state(&self, snapshot: &Snapshot) -> (bool, Option<String>) {
        if snapshot.tripped {
            return (true, snapshot.trip_code.map(|c| c.as_str().to_string()));
        }
        let switched_off = self
            .kill_switch_file
            .as_deref()
            .is_some_and(|path| path.exists());
        if switched_off {
            return (true, Some(TripReason::KillSwitch.as_str().to_string()));
        }
        (false, None)
    }

    fn snapshot(&self, route: &str) -> Option<Snapshot> {
        match self.ledger.snapshot_cached(self.snapshot_max_age) {
            Ok(snapshot) => Some(snapshot),
            Err(e) => {
                eprintln!("[live-gate] the ledger could not be read for {route}: {e}");
                None
            }
        }
    }
}

impl crate::api::LiveGate for LiveGate {
    fn admit(&self, request: LiveAdmissionRequest) -> AdmitFuture<'_> {
        let inner = self.inner.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || inner.admit_blocking(request))
                .await
                .map_err(|e| ApiError::internal(format!("live admission task: {e}")))?
        })
    }

    fn capabilities(&self) -> Option<LiveCapabilitiesV1> {
        let evidence = evidence_provenance(self.inner.factory.evidence_mode());
        Some(LiveCapabilitiesV1 {
            presets: PRESETS
                .iter()
                .map(|p| PresetV1 {
                    id: p.id().as_str().to_string(),
                    title: p.title().to_string(),
                    description: p.description().to_string(),
                    project: p.project().to_string(),
                    model: ModelProvenance::LiveMeta,
                    evidence,
                })
                .collect(),
            budget: self.inner.public_budget(),
            requires_turnstile: true,
            requires_adult_attestation: true,
        })
    }

    fn readiness(&self) -> Option<LiveReadyV1> {
        let evidence_ok = self.inner.factory.status().ok;
        Some(match self.inner.snapshot("/ready") {
            Some(snapshot) => {
                let (tripped, trip_code) = self.inner.trip_state(&snapshot);
                LiveReadyV1 {
                    ledger_ok: true,
                    tripped,
                    trip_code,
                    evidence_ok,
                }
            }
            None => LiveReadyV1 {
                ledger_ok: false,
                tripped: false,
                trip_code: None,
                evidence_ok,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::budget::{BudgetConfig, LedgerError, LedgerOptions, TripReason};
    use crate::live::money::{Ppm, Usd};
    use axum::http::StatusCode;
    use consensus_domain::{EvidenceMode, Task};
    use consensus_runtime::orchestrator::OrchestratorError;
    use consensus_runtime::{EventLogError, SharedRunLog};

    fn usd(text: &str) -> Usd {
        Usd::parse(text).unwrap()
    }

    fn cfg() -> BudgetConfig {
        BudgetConfig {
            daily_cap: usd("10.00"),
            lifetime_cap: usd("60.00"),
            judge_slice: usd("15.00"),
            judge_daily_cap: usd("15.00"),
            max_concurrent: 1,
            hourly_admissions: 6,
            burst_fraction: Ppm::ONE,
            run_token_cap: 70_000,
            max_completion_tokens: 16_000,
            input_cap_tokens: 8_000,
            price_input_per_m: usd("4.00"),
            price_output_per_m: usd("20.00"),
            kill_switch_file: None,
        }
    }

    fn every_refusal() -> Vec<Refusal> {
        let id: ReservationId = serde_json::from_str("\"res-x\"").unwrap();
        let mut all = vec![
            Refusal::DailyBudget {
                retry_after: Duration::from_secs(7),
            },
            Refusal::LifetimeBudget,
            Refusal::JudgeSlice,
            Refusal::JudgeDaily {
                retry_after: Duration::from_secs(3600),
            },
            Refusal::HourlyAdmissions {
                retry_after: Duration::from_secs(60),
            },
            Refusal::Concurrency {
                retry_after: Duration::from_secs(30),
            },
            Refusal::ReservationExhausted {
                remaining: Usd::ZERO,
                needed: usd("0.01"),
            },
            Refusal::InputCap {
                bound_tokens: 9_000,
                cap_tokens: 8_000,
            },
            Refusal::OutputCap {
                requested: 20_000,
                cap: 16_000,
            },
            Refusal::Ledger(LedgerError::UnknownReservation(id)),
        ];
        for code in [
            TripReason::KillSwitch,
            TripReason::LifetimeCap,
            TripReason::OutputBound,
            TripReason::ChargeBound,
            TripReason::ProviderRefusedKey,
            TripReason::Inconsistent,
        ] {
            all.push(Refusal::Tripped {
                code,
                reason: "operator detail with /a/path and http 402".into(),
            });
        }
        all
    }

    #[test]
    fn every_refusal_and_trip_reason_maps_to_a_code_status_and_fixed_message() {
        for refusal in every_refusal() {
            let code = refusal_code(&refusal);
            let error = refusal_error(&refusal);
            assert_eq!(error.code, code, "{refusal}");
            // 429 with a hint exactly when the ledger gives one, else 503.
            match refusal.retry_after() {
                Some(_) => {
                    assert_eq!(error.status(), StatusCode::TOO_MANY_REQUESTS, "{refusal}");
                    assert!(error.retry_after_secs.unwrap() >= 1, "{refusal}");
                }
                None => {
                    assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
                    assert_eq!(error.retry_after_secs, None, "{refusal}");
                }
            }
            // The body is the fixed sentence, never the refusal's text.
            assert_eq!(error.message, refusal_message(code), "{refusal}");
            // One set of public codes: the ledger's own `Refusal::code` is
            // the wire string of the ErrorCode answered here.
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                refusal.code(),
                "{refusal}"
            );
            let json = serde_json::to_string(&error).unwrap();
            for leak in ["path", "http", "402", "res-x", "9000", "0.01"] {
                assert!(!json.contains(leak), "{refusal}: {json}");
            }
            if let Refusal::Tripped { .. } = refusal {
                assert_eq!(code, ErrorCode::LiveTripped);
            }
        }
        assert_eq!(
            refusal_code(&Refusal::DailyBudget {
                retry_after: Duration::from_millis(500)
            }),
            ErrorCode::LiveBudgetDaily
        );
        assert_eq!(
            refusal_error(&Refusal::DailyBudget {
                retry_after: Duration::from_millis(500)
            })
            .retry_after_secs,
            Some(1),
            "a sub-second hint still asks for a one second wait"
        );
        for mode in [
            EvidenceMode::Simulated,
            EvidenceMode::SealedSnapshot,
            EvidenceMode::LiveHyphae,
            EvidenceMode::Offline,
        ] {
            let _ = evidence_provenance(mode);
        }
        assert_eq!(
            evidence_provenance(EvidenceMode::SealedSnapshot),
            EvidenceProvenance::HyphaeSnapshot
        );
    }

    /// A deliberation that never runs: the holder's behaviour is what is
    /// under test.
    struct Inert;

    impl Deliberation for Inert {
        fn create_task(
            &mut self,
            _: &mut SharedRunLog,
            _: RunId,
            _: TaskInput,
        ) -> Result<(RunAggregate, Task), OrchestratorError> {
            Err(OrchestratorError::Log(EventLogError::Io(
                std::io::Error::other("inert"),
            )))
        }

        fn deliberate(
            &mut self,
            _: &mut SharedRunLog,
            _: RunAggregate,
            _: &Task,
            _: &RunControl,
        ) -> Result<RunAggregate, OrchestratorError> {
            unreachable!("never deliberates")
        }
    }

    fn temp_ledger(name: &str) -> (std::path::PathBuf, Ledger) {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-live-gate-unit-{}-{name}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let ledger = Ledger::open_with(
            &dir,
            cfg(),
            Arc::new(chrono::Utc::now),
            LedgerOptions {
                lock_wait: Duration::from_millis(200),
                lock_poll: Duration::from_millis(5),
            },
        )
        .unwrap();
        (dir, ledger)
    }

    #[test]
    fn dropping_a_metered_deliberation_releases_its_reservation_once() {
        let (dir, ledger) = temp_ledger("drop");
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let holder = Arc::new(ReservationHolder::new(ledger.clone(), reservation));
        assert!(holder.is_held());
        assert_eq!(ledger.snapshot().unwrap().unsettled_reservations, 1);
        assert!(
            matches!(
                ledger.reserve(Lane::Public),
                Err(Refusal::Concurrency { .. })
            ),
            "the one slot is held"
        );
        let deliberation = MeteredDeliberation::new(Box::new(Inert), holder.clone());
        drop(deliberation);
        assert!(!holder.is_held());
        let snap = ledger.snapshot().unwrap();
        assert_eq!(snap.unsettled_reservations, 0);
        assert_eq!(snap.active_public, 0);
        // A second holder (the refund closure) finds nothing to do.
        assert_eq!(
            holder.release(&RELEASE_BACKOFF, "again"),
            ReleaseOutcome::AlreadyReleased
        );
        let releases = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .flat_map(|text| {
                text.lines()
                    .filter(|l| l.contains("\"type\":\"release\""))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .count();
        assert_eq!(releases, 1, "released exactly once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dropped_deliberation_retries_its_release_until_the_ledger_is_free() {
        let (dir, ledger) = temp_ledger("drop-retry");
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let holder = Arc::new(ReservationHolder::new(ledger.clone(), reservation));
        let deliberation = MeteredDeliberation::new(Box::new(Inert), holder.clone());
        // Another writer holds the lock past the first two attempts (the
        // lock wait is 200 ms, the retries wait 100 and 300 ms), then lets
        // go: a start that failed or a caught panic must not leave the
        // reservation counting because of one transient failure.
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join(crate::live::budget::LOCK_FILE))
            .unwrap();
        lock.try_lock().unwrap();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(650));
            lock.unlock().unwrap();
        });
        let started = std::time::Instant::now();
        drop(deliberation);
        let elapsed = started.elapsed();
        writer.join().unwrap();
        assert!(!holder.is_held());
        assert!(
            elapsed >= Duration::from_millis(650),
            "the release waited for the ledger: {elapsed:?}"
        );
        let snap = ledger.snapshot().unwrap();
        assert_eq!(snap.unsettled_reservations, 0, "released after retrying");
        assert_eq!(snap.active_public, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_deliberation_dropped_on_an_async_worker_still_releases() {
        let (dir, ledger) = temp_ledger("drop-worker");
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let holder = Arc::new(ReservationHolder::new(ledger.clone(), reservation));
        let deliberation = MeteredDeliberation::new(Box::new(Inert), holder.clone());
        // What a start refused while the server drains does: the request,
        // deliberation included, is dropped on the worker polling the
        // route. The release runs under `block_in_place` there.
        tokio::spawn(async move {
            drop(deliberation);
        })
        .await
        .unwrap();
        assert!(!holder.is_held());
        let snap = ledger.snapshot().unwrap();
        assert_eq!(snap.unsettled_reservations, 0);
        assert_eq!(snap.active_public, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_release_that_cannot_reach_the_ledger_is_logged_and_frees_the_slot() {
        let (dir, ledger) = temp_ledger("locked");
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let id = reservation.id().clone();
        let holder = ReservationHolder::new(ledger.clone(), reservation);
        // Another writer holds the lock for the whole attempt sequence.
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join(crate::live::budget::LOCK_FILE))
            .unwrap();
        lock.try_lock().unwrap();
        let started = std::time::Instant::now();
        let outcome = holder.release(&[Duration::from_millis(50)], "test");
        assert_eq!(outcome, ReleaseOutcome::Failed);
        assert!(
            started.elapsed() >= Duration::from_millis(450),
            "two attempts"
        );
        assert!(!holder.is_held(), "the handle is gone; the slot is free");
        lock.unlock().unwrap();
        // The ledger still counts the worst case until an operator
        // reconciles it by id.
        let snap = ledger.snapshot().unwrap();
        assert_eq!(snap.unsettled_reservations, 1);
        assert_eq!(snap.active_public, 0);
        ledger.release_by_id(&id, "operator").unwrap();
        assert_eq!(ledger.snapshot().unwrap().unsettled_reservations, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
