// SPDX-License-Identifier: Apache-2.0
//! The public live gate (plan B9, ADR-0007): the one place where a
//! visitor's request becomes a run on the owner's Meta key. It implements
//! [`crate::api::LiveGate`] over the admission library of this module
//! tree, in the order ABUSE-9 fixes, stopping at the first refusal and
//! spending nothing before the ledger has reserved the run's worst case.
//! It works in two phases, with the route's live deliberation slot taken
//! between them (see [`crate::api::LiveGate`]).
//!
//! **Phase one, [`crate::api::LiveGate::verify`]: nothing held, nothing
//! charged.** It runs before the slot is taken, so a request waiting on
//! Siteverify (up to [`super::turnstile::SITEVERIFY_TIMEOUT`]; bogus tokens
//! from many prefixes included) keeps the slot from nobody.
//!
//! 1. the preset must be one of the five curated tasks (`404
//!    preset_unknown`);
//! 2. the geographic policy on the live surface: the portal-wide list,
//!    the live-only list (South Korea) and an unknown country all refuse
//!    (`451` with the geo module's code);
//! 3. the 18+ and territory acknowledgment (`422 attestation_required`);
//! 4. the evidence source ([`DeliberationFactory::status`]): one that is
//!    not ok (a sealed snapshot that failed its boot check, or whose served
//!    files changed since) is `503 live_unavailable`, before Turnstile, so
//!    the refused visitor keeps their token and nothing is reserved;
//! 5. Turnstile: a token is required (`403 turnstile_required`) and
//!    verified server-side, fail closed: an outage is `503
//!    turnstile_unavailable`, anything else `403 turnstile_failed`, the
//!    one public code for every refusal. `remoteip` is not sent (the
//!    client key is a /32 or /64 prefix, not an address); the
//!    `idempotency_key` is a fresh UUID for every call, never the
//!    request's `Idempotency-Key`, so a token is single-use at Siteverify
//!    and one solved challenge admits at most one run (the note below);
//! 6. the lane: a judge code the digest file knows enters
//!    [`Lane::Judge`]; anything else is the public lane, never an error,
//!    and nothing in the response says whether codes exist.
//!
//! **Phase two, [`crate::api::VerifiedLiveRequest::admit`]: with the slot
//! held.** A token Siteverify accepted in phase one is consumed even when
//! the slot (`429 too_many_runs`) or phase two then refuses: Siteverify
//! accepts a token once, and the web client resets its widget after every
//! submit, so a refused visitor solves a new (usually invisible) challenge.
//!
//! 7. the lane's daily quota, process-local ([`DailyCounts`]; the count goes
//!    with the reservation: taken just before the ledger is asked, so that
//!    concurrent requests of one client cannot overshoot, and given back when
//!    the ledger refuses, the run cannot be built or its start fails, so a
//!    refusal never consumes it): on the public lane at most
//!    `CONSENSUS_LIVE_CLIENT_DAILY` admissions per client key (IPv4 /32,
//!    IPv6 /64) per UTC day of the ledger's clock (`429 live_client_quota`,
//!    `Retry-After` to 00:00 UTC, a fixed sentence without figures); on the
//!    judge lane at most `CONSENSUS_LIVE_JUDGE_CODE_DAILY` per code, refused
//!    with the judge lane's own daily refusal (`429
//!    live_budget_judge_daily`), so a code's holder learns nothing new. The
//!    judge lane is exempt from the client quota. Both tables reset at a
//!    restart;
//! 8. the reservation ([`Ledger::reserve`], through [`MeteredReservation`]):
//!    a refusal is answered with a stable code ([`refusal_code`]), `429` and
//!    `Retry-After` when the ledger gives a hint, `503` otherwise, and a
//!    fixed sentence, never the refusal's own text (it names amounts, trip
//!    details and files);
//! 9. the model client for this run: the server's Meta configuration with
//!    the session pseudonym as `safety_identifier` and the live
//!    `max_completion_tokens`, metered against the reservation
//!    ([`Ledger::meter`]);
//! 10. the deliberation, built by the [`DeliberationFactory`] (the sealed
//!     snapshot in production, ADR-0008, which re-checks its served files
//!     first and refuses when they changed: the reservation is released and
//!     the answer is `503 live_unavailable`, before any model call) and
//!     wrapped so the reservation is released when the run ends, however it
//!     ends;
//! 11. the grant: the preset's canned task declared `{meta, <the
//!     factory's evidence mode>, public, <preset>}`, the deliberation, and
//!     a refund closure for a start that fails after admission (the
//!     reservation and the daily count are given back).
//!
//! Everything blocking (the ledger, Siteverify, building the HTTP client)
//! runs on the blocking pool with owned data; the gate's futures never
//! block an async worker. Requests in phase one therefore wait on
//! Siteverify concurrently, each on a blocking-pool thread: the slot does
//! not bound them; at most [`SITEVERIFY_MAX_IN_FLIGHT`] calls are in flight
//! at once (past that `503 turnstile_unavailable`, the token not sent), and
//! the per-client run rate and the edge rate limit bound who gets there.
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
//! **Reservation lifetime.** One [`ReservationHolder`] per admitted run
//! ([`MeteredReservation`], through which the operator console's metered
//! `model=meta` runs spend too), shared by the wrapped deliberation and the
//! refund closure, releases the reservation exactly once, always with the
//! bounded retries of
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

use super::budget::{
    until_midnight, Lane, Ledger, Refusal, Reservation, ReservationId, ReservationMeter, Snapshot,
    TripReason,
};
use super::config::LiveConfig;
use super::geo::{GeoDecision, GeoPolicy, Surface as GeoSurface};
use super::judge::{JudgeCodeId, JudgeCodes};
use super::presets::{PresetId as LivePresetId, PRESETS};
use super::turnstile::Siteverify;
use crate::api::error::{ApiError, ErrorCode};
use crate::api::types::{
    EvidenceProvenance, LiveCapabilitiesV1, LiveReadyV1, ModelProvenance, PresetV1, PublicBudgetV1,
};
use crate::api::{
    geo_message, AdmitFuture, LiveAdmissionRequest, LiveGrant, VerifiedLiveRequest, VerifyFuture,
};
use crate::config::MetaConfig;
use crate::limits::{ClientKey, DailyCounts};
use crate::meta::MetaModelClient;
use crate::runs::Deliberation;
use chrono::NaiveDate;
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
/// Client keys the per-client daily quota remembers at once. A key stays in
/// the table only once an admission is counted for it, and the ledger's
/// rolling-hour cap bounds those to 24 × `CONSENSUS_LIVE_HOURLY_ADMISSIONS`
/// a day (144 with the defaults), so the bound is not reached in practice;
/// past it the least recently counted key is forgotten first (and could be
/// admitted again that day).
pub const CLIENT_QUOTA_TABLE_MAX: usize = 10_000;

/// Siteverify calls in flight at once. Phase one no longer waits behind
/// the live slot, so without this bound every request carrying a token,
/// a bogus one included, could hold a blocking-pool thread for up to
/// [`super::turnstile::SITEVERIFY_TIMEOUT`]; past it a request is refused
/// at once with `503 turnstile_unavailable` and a short Retry-After, before
/// Siteverify sees its token (so a genuine token stays usable).
pub const SITEVERIFY_MAX_IN_FLIGHT: usize = 8;

// ---------------------------------------------------------------------
// The deliberation factory (the seam for the sealed snapshot client)
// ---------------------------------------------------------------------

/// Why a factory could not build a run's deliberation. Operator-only
/// text: the visitor gets `503 live_unavailable`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct FactoryError(pub String);

/// The evidence source as `/ready`, `/capabilities` and the gate see it:
/// plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryStatus {
    /// False when the source failed its check (a snapshot that did not
    /// verify at boot, or whose served files changed since): `/ready` then
    /// says not ready, `/capabilities` says live mode is not enabled, and
    /// the gate refuses every admission with `503 live_unavailable`.
    pub ok: bool,
    /// The evidence mode the factory declares.
    pub kind: EvidenceMode,
    /// A short stable code describing the state (`ok`, or the snapshot
    /// error code: `digest_mismatch`, `file_unreadable`,
    /// `boot_verification_failed`, ...), never a path. For the operator
    /// (the factory logs it); no public response carries it.
    pub detail: Option<String>,
}

/// Builds the deliberation of one admitted live run around the metered
/// model client the gate made for it.
///
/// The production factory is the sealed Hyphae snapshot (ADR-0008,
/// [`super::snapshot_factory::SnapshotFactory`]): a `SnapshotMemoryClient`
/// serving the proven `demo-repo` record list with the pinned anchor, and
/// the hardened proof verifier bound to the boot check, declaring
/// [`EvidenceMode::SealedSnapshot`]. `serve-public` builds it with live
/// mode enabled only; without `CONSENSUS_SNAPSHOT_DIR` (or with an unusable
/// snapshot setting) it refuses to start (see
/// [`crate::server::build_live_gate`]), and a snapshot that does not boot
/// is [`super::snapshot_factory::UnavailableEvidence`], whose status says
/// why and which builds nothing. Tests plug in a scripted factory over the
/// runtime's fakes. There is no fallback to simulated evidence in
/// production: the mode a factory declares is the one every run it builds
/// is labelled with.
pub trait DeliberationFactory: Send + Sync {
    /// The evidence mode every deliberation this factory builds runs in;
    /// recorded as `Task.mode.evidence` and published by `/capabilities`.
    fn evidence_mode(&self) -> EvidenceMode;

    /// One run's deliberation over `model` (already metered) with the
    /// live run limits. An `Err` (the evidence is not usable) makes the
    /// gate release the reservation and answer `503 live_unavailable`
    /// before any model call.
    fn build(
        &self,
        model: MetaModelClient,
        limits: RunLimits,
    ) -> Result<Box<dyn Deliberation>, FactoryError>;

    /// The source's state, for `/ready`, `/capabilities` and the gate's
    /// check before Turnstile. May be cached briefly (the snapshot factory
    /// re-hashes at most once per [`crate::api::READY_CACHE_TTL`]);
    /// [`build`](Self::build) never relies on it.
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
        ErrorCode::LiveClientQuota => {
            "this network has started its share of live runs for today; retry after the given delay"
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

/// One metered run's reservation: the run's worst case reserved on a
/// lane, the meter its Meta client draws every HTTP attempt on, and the
/// [`ReservationHolder`] that releases it exactly once. The public gate and
/// the operator console's `model=meta` runs both spend through it, so a
/// run on either listener is reserved, drawn, settled and released the same
/// way, and every trip (the kill switch included) stops both.
pub struct MeteredReservation {
    holder: Arc<ReservationHolder>,
    meter: Arc<ReservationMeter>,
}

impl std::fmt::Debug for MeteredReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeteredReservation")
            .field("holder", &self.holder)
            .finish_non_exhaustive()
    }
}

impl MeteredReservation {
    /// Reserves one run's worst case on `lane` ([`Ledger::reserve`]): the
    /// first thing that commits money. A refusal is the ledger's, for
    /// [`refusal_error`].
    pub fn reserve(ledger: &Ledger, lane: Lane) -> Result<Self, Refusal> {
        let reservation = ledger.reserve(lane)?;
        let meter = Arc::new(ledger.meter(&reservation));
        let holder = Arc::new(ReservationHolder::new(ledger.clone(), reservation));
        Ok(Self { holder, meter })
    }

    /// `model`, drawing every HTTP attempt on this reservation.
    pub fn meter_model(&self, model: MetaModelClient) -> MetaModelClient {
        model.with_meter(self.meter.clone())
    }

    /// `inner`, releasing this reservation when the run ends, however it
    /// ends ([`MeteredDeliberation`]).
    pub fn wrap(&self, inner: Box<dyn Deliberation>) -> MeteredDeliberation {
        MeteredDeliberation::new(inner, self.holder.clone())
    }

    /// Releases the reservation now, for a run that will not be built.
    pub fn release(&self, why: &str) -> ReleaseOutcome {
        self.holder.release(&RELEASE_BACKOFF, why)
    }

    pub fn holder(&self) -> &Arc<ReservationHolder> {
        &self.holder
    }
}

// ---------------------------------------------------------------------
// The daily quotas
// ---------------------------------------------------------------------

/// What one admission is counted against: its client key on the public
/// lane, its judge code on the judge lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaKey {
    Client(ClientKey),
    JudgeCode(JudgeCodeId),
}

/// The gate's process-local daily quotas (see the module doc, step 7).
struct Quotas {
    clients: DailyCounts<ClientKey>,
    judge_codes: DailyCounts<JudgeCodeId>,
}

impl Quotas {
    fn new(config: &LiveConfig, judge_codes: &JudgeCodes) -> Self {
        Self {
            clients: DailyCounts::new(config.client_daily, CLIENT_QUOTA_TABLE_MAX),
            // One entry per code at most: only a code the file knows is
            // ever counted.
            judge_codes: DailyCounts::new(config.judge_code_daily, judge_codes.len().max(1)),
        }
    }

    fn take(&self, key: QuotaKey, day: NaiveDate) -> bool {
        match key {
            QuotaKey::Client(client) => self.clients.take(&client, day),
            QuotaKey::JudgeCode(code) => self.judge_codes.take(&code, day),
        }
    }

    fn give_back(&self, key: QuotaKey, day: NaiveDate) {
        match key {
            QuotaKey::Client(client) => self.clients.give_back(&client, day),
            QuotaKey::JudgeCode(code) => self.judge_codes.give_back(&code, day),
        }
    }
}

/// One admission's count, taken just before the reservation and given back
/// when it is dropped unkept: the ledger refused, or the run could not be
/// built. [`CountedAdmission::keep`] hands the count to the grant's refund.
struct CountedAdmission {
    inner: Arc<Inner>,
    key: QuotaKey,
    day: NaiveDate,
    kept: bool,
}

impl CountedAdmission {
    /// The count stays; the returned closure gives it back (the refund of
    /// a start that fails after the grant).
    fn keep(mut self) -> impl FnOnce() + Send {
        self.kept = true;
        let (inner, key, day) = (self.inner.clone(), self.key, self.day);
        move || inner.quotas.give_back(key, day)
    }
}

impl Drop for CountedAdmission {
    fn drop(&mut self) {
        if !self.kept {
            self.inner.quotas.give_back(self.key, self.day);
        }
    }
}

// ---------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------

/// Everything [`LiveGate::new`] needs. `geo` and `judge_codes` are given
/// next to `config` so a test can script them; the server passes the
/// configuration's own. The daily quotas come from `config`
/// (`client_daily`, `judge_code_daily`).
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
    quotas: Quotas,
    /// [`SITEVERIFY_MAX_IN_FLIGHT`] permits, one per Siteverify call.
    siteverify_in_flight: tokio::sync::Semaphore,
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
                quotas: Quotas::new(&parts.config, &parts.judge_codes),
                ledger: parts.ledger,
                siteverify: parts.siteverify,
                geo: parts.geo,
                judge_codes: parts.judge_codes,
                meta: parts.meta,
                run_limits: parts.run_limits,
                factory: parts.factory,
                siteverify_in_flight: tokio::sync::Semaphore::new(SITEVERIFY_MAX_IN_FLIGHT),
            }),
        }
    }

    /// Public-lane admissions counted for `client` on the ledger's current
    /// UTC day (tests and diagnostics).
    pub fn client_admissions_today(&self, client: ClientKey) -> u32 {
        let day = self.inner.ledger.now().date_naive();
        self.inner.quotas.clients.count(&client, day)
    }

    /// The ledger this gate reserves on (tests read its records).
    pub fn ledger(&self) -> &Ledger {
        &self.inner.ledger
    }

    pub fn evidence_mode(&self) -> EvidenceMode {
        self.inner.factory.evidence_mode()
    }
}

/// A request phase one accepted: what phase two needs, and nothing held.
struct Verified {
    inner: Arc<Inner>,
    preset_id: LivePresetId,
    lane: Lane,
    /// What the admission is counted against: the client key on the public
    /// lane, the code on the judge lane.
    quota: QuotaKey,
    session_pseudonym: String,
}

impl VerifiedLiveRequest for Verified {
    fn admit(self: Box<Self>) -> AdmitFuture<'static> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let inner = self.inner.clone();
                inner.admit_blocking(*self)
            })
            .await
            .map_err(|e| ApiError::internal(format!("live admission task: {e}")))?
        })
    }
}

impl Inner {
    /// Phase one, on a blocking thread, in ABUSE-9 order: everything that
    /// holds nothing and charges nothing, Turnstile last.
    fn verify_blocking(
        self: &Arc<Self>,
        request: LiveAdmissionRequest,
    ) -> Result<Verified, ApiError> {
        // (a) One of the five curated tasks, or nothing.
        let preset_id = LivePresetId::parse(request.preset.as_str()).map_err(|_| {
            ApiError::new(
                ErrorCode::PresetUnknown,
                refusal_message(ErrorCode::PresetUnknown),
            )
        })?;

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

        // (c') The evidence, before Turnstile: while the source is not ok
        // nothing could be admitted, so the visitor's token is not spent
        // and nothing is reserved. The factory logs the code once per
        // change between ok and not ok; this line does not repeat it.
        if !self.factory.status().ok {
            eprintln!(
                "[live-gate] refused a live run: the evidence source is not ok (the [snapshot] line says why)"
            );
            return Err(unavailable());
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
        // At most SITEVERIFY_MAX_IN_FLIGHT calls wait on Cloudflare at once;
        // past that the token is not sent, so a genuine one stays usable.
        let Ok(_in_flight) = self.siteverify_in_flight.try_acquire() else {
            eprintln!(
                "[live-gate] refused a live run: {SITEVERIFY_MAX_IN_FLIGHT} Siteverify calls are already in flight"
            );
            return Err(ApiError::new(
                ErrorCode::TurnstileUnavailable,
                refusal_message(ErrorCode::TurnstileUnavailable),
            )
            .with_retry_after(2));
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

        // (e) The lane. An unknown code is the public lane, silently; the
        // admission is counted against the code or the client key.
        let (lane, quota) = match request
            .judge_code
            .as_deref()
            .and_then(|code| self.judge_codes.lookup(code))
        {
            Some(code) => (Lane::Judge, QuotaKey::JudgeCode(code)),
            None => (Lane::Public, QuotaKey::Client(request.client)),
        };
        Ok(Verified {
            inner: self.clone(),
            preset_id,
            lane,
            quota,
            session_pseudonym: request.session_pseudonym,
        })
    }

    /// Takes the admission's daily count (step 7 of the module doc), or
    /// refuses with the lane's fixed answer: `429 live_client_quota` on the
    /// public lane, the judge lane's own daily refusal for a code.
    fn count(self: &Arc<Self>, verified: &Verified) -> Result<CountedAdmission, ApiError> {
        let now = self.ledger.now();
        let day = now.date_naive();
        if self.quotas.take(verified.quota, day) {
            return Ok(CountedAdmission {
                inner: self.clone(),
                key: verified.quota,
                day,
                kept: false,
            });
        }
        let retry_after = until_midnight(now);
        Err(match verified.quota {
            QuotaKey::Client(_) => {
                // Never the client's address: the line says what happened.
                eprintln!(
                    "[live-gate] refused a public run: the client started its {} live admissions of the UTC day",
                    self.quotas.clients.limit()
                );
                ApiError::new(
                    ErrorCode::LiveClientQuota,
                    refusal_message(ErrorCode::LiveClientQuota),
                )
                .with_retry_after(retry_after.as_secs())
            }
            QuotaKey::JudgeCode(_) => {
                // Never the code. The answer is the judge lane's daily
                // refusal, so a code's holder learns nothing new from it.
                eprintln!(
                    "[live-gate] refused a judge run: the code started its {} live admissions of the UTC day",
                    self.quotas.judge_codes.limit()
                );
                refusal_error(&Refusal::JudgeDaily { retry_after })
            }
        })
    }

    /// Phase two, on a blocking thread, with the route's live slot held:
    /// the daily count, the reservation, the metered model client, the
    /// deliberation, the grant.
    fn admit_blocking(self: &Arc<Self>, verified: Verified) -> Result<LiveGrant, ApiError> {
        let preset = verified.preset_id.preset();
        let lane = verified.lane;

        // (f) The lane's daily quota, counted now and given back by every
        // refusal below (the guard's drop).
        let counted = self.count(&verified)?;

        // (g) The reservation: the first and only thing that commits money.
        let metered = MeteredReservation::reserve(&self.ledger, lane).map_err(|refusal| {
            eprintln!(
                "[live-gate] the ledger refused a {} run: {refusal}",
                lane.as_str()
            );
            refusal_error(&refusal)
        })?;

        // (h) This run's model client: the session pseudonym identifies
        // the visitor to Meta, the live output bound caps every call, and
        // every HTTP attempt is drawn on the reservation.
        let mut cfg = self.meta.clone();
        cfg.safety_identifier = Some(verified.session_pseudonym.clone());
        cfg.max_completion_tokens = self.max_completion_tokens;
        let model = match MetaModelClient::new(cfg) {
            Ok(model) => metered.meter_model(model),
            Err(e) => {
                // A configuration error names its setting, never the key.
                eprintln!("[live-gate] the model client could not be built: {e}");
                metered.release("the model client could not be built");
                return Err(unavailable());
            }
        };

        // (i) The deliberation over the factory's evidence, wrapped so the
        // reservation is released when the run ends. The factory re-checks
        // its evidence here in full (the status above may be a cached
        // read); a refusal releases the reservation before any model call.
        let inner = match self.factory.build(model, self.run_limits) {
            Ok(deliberation) => deliberation,
            Err(e) => {
                eprintln!("[live-gate] the deliberation could not be built: {e}");
                metered.release("the deliberation could not be built");
                return Err(unavailable());
            }
        };
        let deliberation = metered.wrap(inner);

        // (j) The grant: the canned task declared with what will actually
        // run, and the refund for a start that fails after this point.
        let mut input = preset.task_input();
        input.mode = Some(TaskMode {
            model: ModelMode::Meta,
            evidence: self.factory.evidence_mode(),
            origin: RunOrigin::Public,
            preset: Some(verified.preset_id.as_str().to_string()),
        });
        let refund = metered.holder().clone();
        let give_back = counted.keep();
        Ok(LiveGrant {
            input,
            deliberation: Box::new(deliberation),
            on_start_failed: Some(Box::new(move || {
                // The run never started: its daily count is given back.
                give_back();
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
    fn verify(&self, request: LiveAdmissionRequest) -> VerifyFuture<'_> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let verified = tokio::task::spawn_blocking(move || inner.verify_blocking(request))
                .await
                .map_err(|e| ApiError::internal(format!("live verification task: {e}")))??;
            Ok(Box::new(verified) as Box<dyn VerifiedLiveRequest>)
        })
    }

    /// Not while the evidence source is not ok: `/capabilities` then
    /// publishes live mode as disabled (the gate refuses every admission
    /// anyway), and `/ready` says why.
    fn available(&self) -> bool {
        self.inner.factory.status().ok
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
