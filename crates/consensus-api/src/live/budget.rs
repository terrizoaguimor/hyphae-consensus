// SPDX-License-Identifier: Apache-2.0
//! The USD ledger (ABUSE-1): every dollar the owner's key can spend on
//! public live runs is reserved here before a request is built, drawn per
//! HTTP attempt, settled on the attempt's outcome and never refunded when
//! the outcome is unknown.
//!
//! **Files.** Under the ledger directory: one append-only JSONL file per
//! UTC day (`YYYY-MM-DD.jsonl`), a lifetime summary (`lifetime.json`,
//! written atomically after every change) and `ledger.lock`. Every record
//! is written with one `write_all` and fsync'd before the operation
//! returns. Day files are the source of truth; the summary is derived from
//! them and read back at open as a floor: lifetime spend and per-lane
//! commitments the summary remembers but the day files no longer show, or
//! a trip they no longer show, stay counted (as offsets fixed at open and
//! carried into the next summary), the snapshot reports the
//! inconsistency, and a floor that reaches the lifetime cap trips at open.
//! The summary is honored only when it is at least as new as the newest
//! record in the files; a record newer than the summary means the summary
//! is the stale side (a crash between an append and its rewrite) and the
//! files win. Today's commitments cannot be rebuilt from the summary: the
//! lifetime figures are the protection when today's file is lost.
//!
//! **Sharing.** Every process that holds the key (`serve-public`, the
//! operator console, staging scripts) opens its own [`Ledger`] on the same
//! directory. Handles *serialize through the lock per operation*: each
//! operation takes the exclusive std file lock on `ledger.lock`, reads
//! whatever other handles appended since its last look, applies its own
//! change, appends, fsyncs, rewrites the summary and releases the lock. A
//! second handle is therefore never refused; it just sees the first one's
//! records. The lock wait is bounded ([`LedgerOptions::lock_wait`]); when
//! it runs out the operation fails closed ([`LedgerError::Locked`]). The
//! one read path that never waits for the lock is
//! [`Ledger::snapshot_cached`], meant for the public `/capabilities`.
//!
//! **Records.** `reservation {id, lane, worst_case_usd, at}`,
//! `draw {reservation, attempt, est_usd, input_bound_tokens,
//! max_completion_tokens, at}`, `settlement {reservation, attempt,
//! outcome: usage{input,completion,reasoning_included}|zero|unknown,
//! charged_usd, at}`, `release {reservation, at}`, `trip {code, reason,
//! at}` and `reset {by, at}`. Money is a decimal string with six places,
//! parsed exactly; arithmetic is integer micro-dollars ([`super::money`]).
//!
//! **Admission** ([`Ledger::reserve`]). The run's worst case is
//! `(run token cap + max_completion_tokens + per-call input cap)` priced
//! at the *output* rate ([`BudgetConfig::run_worst_case`]). A reservation
//! is refused, with a retry hint where one exists, when the lane's budget
//! cannot hold that whole amount, so exhaustion elsewhere never kills an
//! admitted run: the reservation is the run's hard ceiling. Per lane:
//! - `public`: the daily budget refills evenly over the UTC day with a
//!   burst of `burst_fraction × daily cap` available at 00:00; at most
//!   `hourly_admissions` reservations per rolling hour; at most
//!   `max_concurrent` active reservations per process; the lifetime cap
//!   minus the judge slice bounds it with the operator lane.
//! - `judge`: its own slice of the lifetime cap, its own daily cap
//!   ([`BudgetConfig::judge_daily_cap`], by default the whole slice), its
//!   own rolling-hour window and one slot; not paced by the public daily
//!   budget, so judges can run while the public lane is exhausted, and
//!   never beyond the slice or the judge daily cap. One UTC day's exposure
//!   on the key is therefore bounded by `daily_cap + judge_daily_cap`.
//! - `operator`: the daily and lifetime caps without pacing or slots.
//!
//! **Exposure.** An active reservation counts at its full worst case. A
//! released one counts at what it settled plus any attempt it left
//! unsettled (at that attempt's estimate). A reservation that is never
//! released, because the process crashed, therefore stays at worst case
//! forever: startup replays the files and counts it so.
//!
//! **Attempts** ([`Ledger::draw`], [`Ledger::settle`], [`AttemptMeter`]).
//! One draw per HTTP attempt, retries included, taken before the request
//! is sent. The input bound is `request bytes + 1024` tokens; a request
//! over the per-call input cap, or asking for more than the configured
//! `max_completion_tokens`, is refused before sending. The attempt's
//! estimate is `input bound × input rate + max_completion_tokens × output
//! rate` and must fit the reservation's remaining ceiling. Settlement:
//! a success with usage charges `prompt_tokens × input rate +
//! completion_tokens × output rate`, where `completion_tokens` is the
//! API's figure, reasoning included ([`Usage`]); HTTP
//! 400/401/402/403/404/413/429 charge zero, and so does an attempt whose
//! request never left this process (a connection that could not be
//! opened, a request that could not be built: [`Outcome::NotSent`]); a
//! timeout, a transport error after the request was sent, any other
//! status (5xx included), a success without usage or one whose usage is
//! inconsistent charge the attempt's full estimate, never refunded.
//!
//! **Trips.** The ledger trips, persistently, when a settlement reports
//! more completion tokens than the attempt's `max_completion_tokens` or a
//! charge above the attempt's estimate (the bound did not hold), on any
//! 401/402/403 from the provider (the key is in trouble), when the lifetime
//! spend reaches the lifetime cap, or when the kill-switch file exists.
//! Each trip has a coarse public code ([`TripReason`]) and an
//! operator-only detail ([`Trip`]). Tripped, it refuses every reservation
//! and every draw, so in-flight runs stop at their next call. The state
//! survives reopen; only an explicit operator [`Ledger::reset`] clears it,
//! and a kill-switch file still present trips it again at the next
//! admission.

use super::money::{Ppm, Usd};
use chrono::{DateTime, NaiveDate, SecondsFormat, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Exclusive lock file; holds no data.
pub const LOCK_FILE: &str = "ledger.lock";
/// Derived lifetime summary, rewritten atomically after every change.
pub const SUMMARY_FILE: &str = "lifetime.json";
/// Record format this version writes and reads.
pub const RECORD_VERSION: u32 = 1;
/// Tokens added to the request byte length to bound the input tokens.
pub const INPUT_MARGIN_TOKENS: u64 = 1024;
/// Statuses settled at zero: the provider did no work for them.
pub const ZERO_CHARGE_STATUSES: [u16; 7] = [400, 401, 402, 403, 404, 413, 429];
/// Statuses that trip the ledger: the key itself is refused.
pub const TRIPPING_STATUSES: [u16; 3] = [401, 402, 403];
/// Retry hint when every slot is busy.
pub const CONCURRENCY_RETRY_AFTER: Duration = Duration::from_secs(30);
const SECONDS_PER_DAY: u64 = 86_400;
const SECONDS_PER_HOUR: i64 = 3_600;

/// Clock the ledger reads; tests inject a controllable one.
pub type LedgerClock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Caps, limits and prices the ledger enforces (see [`super::config`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetConfig {
    /// Public and operator reservations per UTC day.
    pub daily_cap: Usd,
    pub lifetime_cap: Usd,
    /// Reserved for the judge lane inside `lifetime_cap`.
    pub judge_slice: Usd,
    /// Judge reservations per UTC day, at most `judge_slice` (the default
    /// equals it, so judges stay admissible while the public day is
    /// exhausted; one day's exposure is then `daily_cap + judge_daily_cap`).
    pub judge_daily_cap: Usd,
    /// Active public reservations per process.
    pub max_concurrent: u32,
    /// Reservations per rolling hour, per lane (public and judge).
    pub hourly_admissions: u32,
    /// Share of the daily cap available at 00:00 UTC.
    pub burst_fraction: Ppm,
    pub run_token_cap: u64,
    pub max_completion_tokens: u64,
    pub input_cap_tokens: u64,
    pub price_input_per_m: Usd,
    pub price_output_per_m: Usd,
    /// Its presence trips the ledger.
    pub kill_switch_file: Option<PathBuf>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct BudgetConfigError(String);

impl BudgetConfig {
    /// One run's worst case: every token the run may consume, priced at
    /// the output rate.
    pub fn run_worst_case(&self) -> Usd {
        let tokens = self
            .run_token_cap
            .saturating_add(self.max_completion_tokens)
            .saturating_add(self.input_cap_tokens);
        Usd::price(tokens, self.price_output_per_m)
    }

    /// One attempt's worst case for a bounded input and output.
    pub fn attempt_worst_case(&self, input_bound_tokens: u64, max_completion_tokens: u64) -> Usd {
        Usd::price(input_bound_tokens, self.price_input_per_m)
            .saturating_add(Usd::price(max_completion_tokens, self.price_output_per_m))
    }

    /// Upper bound on the input tokens of a request of `request_bytes`
    /// bytes: tokens never outnumber UTF-8 bytes, plus a template margin.
    pub fn input_bound_tokens(request_bytes: u64) -> u64 {
        request_bytes.saturating_add(INPUT_MARGIN_TOKENS)
    }

    /// The daily budget available at 00:00 UTC.
    pub fn burst(&self) -> Usd {
        self.daily_cap.mul_ppm(self.burst_fraction)
    }

    /// The price of reported usage: input at the input rate, output and
    /// reasoning at the output rate.
    pub fn charge(&self, usage: &Usage) -> Usd {
        Usd::price(usage.input, self.price_input_per_m).saturating_add(Usd::price(
            usage.completion_tokens(),
            self.price_output_per_m,
        ))
    }

    pub fn validate(&self) -> Result<(), BudgetConfigError> {
        let err = |m: String| Err(BudgetConfigError(m));
        if self.daily_cap.is_zero() || self.lifetime_cap.is_zero() {
            return err("daily and lifetime caps must be greater than zero".into());
        }
        if self.judge_slice > self.lifetime_cap {
            return err(format!(
                "judge slice {} exceeds the lifetime cap {}",
                self.judge_slice, self.lifetime_cap
            ));
        }
        if self.judge_daily_cap > self.judge_slice {
            return err(format!(
                "judge daily cap {} exceeds the judge slice {}",
                self.judge_daily_cap, self.judge_slice
            ));
        }
        if self.max_concurrent == 0 || self.hourly_admissions == 0 {
            return err("max_concurrent and hourly_admissions must be at least 1".into());
        }
        if self.run_token_cap == 0 || self.max_completion_tokens == 0 || self.input_cap_tokens == 0
        {
            return err("token caps must be greater than zero".into());
        }
        if self.price_input_per_m.is_zero() || self.price_output_per_m.is_zero() {
            return err("prices must be greater than zero".into());
        }
        if self.run_worst_case() > self.daily_cap {
            return err(format!(
                "one run's worst case {} exceeds the daily cap {}",
                self.run_worst_case(),
                self.daily_cap
            ));
        }
        if self.run_worst_case() > self.public_share() {
            return err(format!(
                "the public share of the lifetime cap ({} − {} judge slice = {}) cannot hold one run's worst case {}",
                self.lifetime_cap,
                self.judge_slice,
                self.public_share(),
                self.run_worst_case()
            ));
        }
        Ok(())
    }

    /// The lifetime cap left to the public and operator lanes once the
    /// judge slice is set aside.
    pub fn public_share(&self) -> Usd {
        self.lifetime_cap.saturating_sub(self.judge_slice)
    }
}

/// Who a reservation is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Public,
    Judge,
    Operator,
}

impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Lane::Public => "public",
            Lane::Judge => "judge",
            Lane::Operator => "operator",
        }
    }
}

/// Token usage the provider reported for one successful attempt, with the
/// OpenAI-compatible semantics Meta's API uses: `completion` is the API's
/// `completion_tokens` **as reported, inclusive of reasoning tokens**, and
/// `reasoning_included` is `completion_tokens_details.reasoning_tokens`, a
/// breakdown of that same total. Only `input` (at the input rate) and
/// `completion` (at the output rate) are billed; `reasoning_included` is
/// recorded for the operator and never priced a second time. Build it with
/// [`Usage::from_chat_usage`] from the adapter's parsed `usage` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// `usage.prompt_tokens`.
    pub input: u64,
    /// `usage.completion_tokens`, reasoning included.
    pub completion: u64,
    /// `usage.completion_tokens_details.reasoning_tokens`, a subset of
    /// `completion`; zero when the API does not report it.
    pub reasoning_included: u64,
}

impl Usage {
    /// From the fields of an OpenAI-style `usage` block, passed exactly as
    /// reported: `completion_tokens` already includes the reasoning tokens.
    pub fn from_chat_usage(
        prompt_tokens: u64,
        completion_tokens: u64,
        reasoning_tokens: Option<u64>,
    ) -> Self {
        Self {
            input: prompt_tokens,
            completion: completion_tokens,
            reasoning_included: reasoning_tokens.unwrap_or(0),
        }
    }

    /// Everything billed at the output rate: the reported completion total.
    pub fn completion_tokens(&self) -> u64 {
        self.completion
    }

    /// False when the reasoning breakdown exceeds the total it is part of.
    /// Such a report cannot be trusted and is settled like
    /// [`Outcome::SuccessWithoutUsage`]: the attempt's full estimate.
    pub fn is_consistent(&self) -> bool {
        self.reasoning_included <= self.completion
    }
}

/// How one HTTP attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 2xx with a usage block.
    Success(Usage),
    /// 2xx without usage: charged at the attempt's full estimate.
    SuccessWithoutUsage,
    /// A non-2xx status. The listed 4xx codes charge zero (401/402/403
    /// also trip); anything else charges the full estimate.
    Status(u16),
    /// The request may have been processed: full estimate.
    Timeout,
    /// The connection failed after the request was sent: full estimate.
    TransportAfterSend,
    /// Nothing was sent: the connection never opened or the request could
    /// not be built. The provider did no work, so the attempt charges
    /// zero; recorded like [`RecordedOutcome::Zero`] with the detail
    /// `not_sent`.
    NotSent,
}

/// What a settlement record stores about its outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordedOutcome {
    /// The reported usage, with [`Usage`]'s inclusive semantics.
    Usage {
        input: u64,
        completion: u64,
        reasoning_included: u64,
    },
    Zero,
    Unknown,
}

/// Why the ledger tripped: a coarse code that is safe to publish. The
/// detail string that accompanies it ([`Trip::detail`]) is operator-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TripReason {
    /// The kill-switch file exists.
    KillSwitch,
    /// The lifetime spend reached the lifetime cap.
    LifetimeCap,
    /// A settlement reported more completion tokens than the attempt's
    /// `max_completion_tokens`: the output bound did not hold.
    OutputBound,
    /// A settlement charged more than the attempt's estimate.
    ChargeBound,
    /// The provider answered 401/402/403: the key itself is refused.
    ProviderRefusedKey,
    /// The lifetime summary remembers a trip the day files do not show.
    Inconsistent,
}

impl TripReason {
    /// Stable public code.
    pub fn as_str(self) -> &'static str {
        match self {
            TripReason::KillSwitch => "kill_switch",
            TripReason::LifetimeCap => "lifetime_cap",
            TripReason::OutputBound => "output_bound",
            TripReason::ChargeBound => "charge_bound",
            TripReason::ProviderRefusedKey => "provider_refused_key",
            TripReason::Inconsistent => "inconsistent",
        }
    }
}

/// A trip the ledger is in: the public code and the operator-only detail
/// (which may name an upstream HTTP status or ledger amounts; never send
/// it to a visitor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trip {
    pub code: TripReason,
    pub detail: String,
}

impl Trip {
    fn new(code: TripReason, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

/// A reservation's id: a UUID v4 the ledger generates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReservationId(String);

impl ReservationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReservationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Record {
    Reservation {
        v: u32,
        id: ReservationId,
        lane: Lane,
        worst_case_usd: Usd,
        at: String,
    },
    Draw {
        v: u32,
        reservation: ReservationId,
        attempt: u32,
        est_usd: Usd,
        input_bound_tokens: u64,
        max_completion_tokens: u64,
        at: String,
    },
    Settlement {
        v: u32,
        reservation: ReservationId,
        attempt: u32,
        outcome: RecordedOutcome,
        charged_usd: Usd,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        at: String,
    },
    Release {
        v: u32,
        reservation: ReservationId,
        /// Set when an operator released it by id after a crash.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
        at: String,
    },
    Trip {
        v: u32,
        /// Absent in records written before the code existed; inferred
        /// from `reason` then.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<TripReason>,
        reason: String,
        at: String,
    },
    Reset {
        v: u32,
        by: String,
        at: String,
    },
}

/// The code of a trip record written without one: the detail strings have
/// always started with a stable prefix.
fn infer_trip_code(detail: &str) -> TripReason {
    if detail.starts_with("kill_switch") {
        TripReason::KillSwitch
    } else if detail.starts_with("lifetime_cap") {
        TripReason::LifetimeCap
    } else if detail.starts_with("completion_tokens_exceeded_max") {
        TripReason::OutputBound
    } else if detail.starts_with("charge_exceeded_attempt_draw") {
        TripReason::ChargeBound
    } else if detail.starts_with("provider_refused_key") {
        TripReason::ProviderRefusedKey
    } else {
        TripReason::Inconsistent
    }
}

impl Record {
    fn version(&self) -> u32 {
        match self {
            Record::Reservation { v, .. }
            | Record::Draw { v, .. }
            | Record::Settlement { v, .. }
            | Record::Release { v, .. }
            | Record::Trip { v, .. }
            | Record::Reset { v, .. } => *v,
        }
    }

    fn at(&self) -> &str {
        match self {
            Record::Reservation { at, .. }
            | Record::Draw { at, .. }
            | Record::Settlement { at, .. }
            | Record::Release { at, .. }
            | Record::Trip { at, .. }
            | Record::Reset { at, .. } => at,
        }
    }
}

/// Refuses a stored record this version cannot trust: another format
/// version, or a timestamp it cannot read (a reservation with an unreadable
/// time could otherwise slip out of today's budget).
fn validate_record(record: Record, path: &Path, offset: u64) -> Result<Record, LedgerError> {
    if record.version() != RECORD_VERSION {
        return Err(LedgerError::Corrupt {
            file: path.to_path_buf(),
            offset,
            reason: format!("unsupported record version {}", record.version()),
        });
    }
    if let Err(reason) = parse_at(record.at()) {
        return Err(LedgerError::Corrupt {
            file: path.to_path_buf(),
            offset,
            reason,
        });
    }
    Ok(record)
}

/// The lifetime summary file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LifetimeSummary {
    pub v: u32,
    pub updated_at: String,
    /// Settled charges plus the estimates of attempts left unsettled by
    /// released reservations: money that is spent for sure.
    pub lifetime_spent_usd: Usd,
    /// Spent plus the worst case of every active reservation.
    pub lifetime_committed_usd: Usd,
    /// The judge lane's share of `lifetime_committed_usd`.
    pub judge_committed_usd: Usd,
    /// The public and operator lanes' share of `lifetime_committed_usd`
    /// (derived as `lifetime − judge` when a summary predates the field).
    #[serde(default)]
    pub nonjudge_committed_usd: Usd,
    pub reservations: u64,
    pub unsettled_reservations: u64,
    pub tripped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip_code: Option<TripReason>,
    /// Operator-only detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip_reason: Option<String>,
}

/// Errors are for the operator log: their `Display` names files and
/// paths. The wire answer is `live_unavailable`
/// (`crate::api::error::ErrorCode::LiveUnavailable`, see
/// [`Refusal::code`]).
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("ledger configuration: {0}")]
    Config(#[from] BudgetConfigError),
    #[error("ledger I/O on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("ledger lock {lock_file} held by another writer for more than {waited:?}")]
    Locked {
        lock_file: PathBuf,
        waited: Duration,
    },
    #[error("ledger file {file} is corrupt at byte {offset}: {reason}")]
    Corrupt {
        file: PathBuf,
        offset: u64,
        reason: String,
    },
    #[error("unknown reservation {0}")]
    UnknownReservation(ReservationId),
    #[error("reservation {0} was already released")]
    Released(ReservationId),
    #[error("reservation {reservation} has no open attempt {attempt}")]
    UnknownAttempt {
        reservation: ReservationId,
        attempt: u32,
    },
}

impl LedgerError {
    fn io(path: &Path, source: io::Error) -> Self {
        LedgerError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// Why an admission or a draw was refused. Every variant has a stable
/// public code ([`Refusal::code`], the wire string of the
/// `crate::api::error::ErrorCode` the gate answers with) and, where
/// retrying helps, a hint for `Retry-After`. Everything else, the fields
/// and the `Display` text (which can carry a trip detail, ledger amounts
/// or file paths), is operator-only: never put `to_string()` of a refusal
/// in a response body.
#[derive(Debug, Error)]
pub enum Refusal {
    #[error("ledger tripped ({}): {reason}", code.as_str())]
    Tripped { code: TripReason, reason: String },
    #[error("daily live budget exhausted; retry after {retry_after:?}")]
    DailyBudget { retry_after: Duration },
    #[error("lifetime live budget exhausted")]
    LifetimeBudget,
    #[error("judge budget slice exhausted")]
    JudgeSlice,
    #[error("judge daily budget exhausted; retry after {retry_after:?}")]
    JudgeDaily { retry_after: Duration },
    #[error("hourly admission cap reached; retry after {retry_after:?}")]
    HourlyAdmissions { retry_after: Duration },
    #[error("every live slot is busy; retry after {retry_after:?}")]
    Concurrency { retry_after: Duration },
    #[error("reservation ceiling exhausted: {remaining} left, {needed} needed")]
    ReservationExhausted { remaining: Usd, needed: Usd },
    #[error("request input bound {bound_tokens} tokens exceeds the per-call cap {cap_tokens}")]
    InputCap { bound_tokens: u64, cap_tokens: u64 },
    #[error("requested max_completion_tokens {requested} exceeds the cap {cap}")]
    OutputCap { requested: u64, cap: u64 },
    #[error("ledger unavailable: {0}")]
    Ledger(#[from] LedgerError),
}

impl Refusal {
    /// The stable public code of this refusal at admission: exactly the
    /// snake_case string of the `crate::api::error::ErrorCode` that
    /// `crate::live::gate::refusal_code` answers with (a unit test there
    /// keeps the two in step), so the UI and operator tooling localize one
    /// set of codes. The draw-time refusals (an exhausted reservation, the
    /// per-call caps) never reach the wire as such: the run fails with the
    /// adapter's `budget_refused`; at admission they cannot happen and
    /// read as `live_unavailable`, like a ledger that could not be
    /// consulted.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::Tripped { .. } => "live_tripped",
            Refusal::DailyBudget { .. } => "live_budget_daily",
            Refusal::LifetimeBudget => "live_budget_lifetime",
            Refusal::JudgeSlice => "live_budget_judge_slice",
            Refusal::JudgeDaily { .. } => "live_budget_judge_daily",
            Refusal::HourlyAdmissions { .. } => "live_hourly_admissions",
            Refusal::Concurrency { .. } => "live_concurrency",
            Refusal::ReservationExhausted { .. }
            | Refusal::InputCap { .. }
            | Refusal::OutputCap { .. }
            | Refusal::Ledger(_) => "live_unavailable",
        }
    }

    /// A hint for `Retry-After`, when retrying can help.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Refusal::DailyBudget { retry_after }
            | Refusal::JudgeDaily { retry_after }
            | Refusal::HourlyAdmissions { retry_after }
            | Refusal::Concurrency { retry_after } => Some(*retry_after),
            _ => None,
        }
    }
}

/// Per-process tuning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerOptions {
    /// How long one operation waits for the file lock before failing.
    pub lock_wait: Duration,
    /// Poll interval while waiting.
    pub lock_poll: Duration,
}

impl Default for LedgerOptions {
    fn default() -> Self {
        Self {
            lock_wait: Duration::from_secs(10),
            lock_poll: Duration::from_millis(20),
        }
    }
}

/// A run's admitted budget. Not `Clone`: one handle per reservation.
/// Dropping it without a successful [`Ledger::release`] is treated as a
/// crash: the reservation keeps counting at its worst case (only an
/// operator's [`Ledger::release_by_id`] can end it then). The handle holds
/// its concurrency slot until the release record is durable, so a failed
/// release can be retried with the same handle.
pub struct Reservation {
    id: ReservationId,
    lane: Lane,
    worst_case: Usd,
    slot: Mutex<Option<SlotGuard>>,
}

impl fmt::Debug for Reservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reservation")
            .field("id", &self.id)
            .field("lane", &self.lane)
            .field("worst_case", &self.worst_case)
            .finish()
    }
}

impl Reservation {
    pub fn id(&self) -> &ReservationId {
        &self.id
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// The run's hard ceiling.
    pub fn worst_case(&self) -> Usd {
        self.worst_case
    }

    /// Frees the in-process slot; called once the release is durable. Must
    /// not run while the ledger's state mutex is held (the guard takes it).
    fn free_slot(&self) {
        let guard = self.slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(guard);
    }
}

/// An in-process slot, freed when the reservation is released or its
/// handle drops.
struct SlotGuard {
    inner: Arc<Inner>,
    lane: Lane,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let mut st = self.inner.state();
        match self.lane {
            Lane::Public => st.active_public = st.active_public.saturating_sub(1),
            Lane::Judge => st.active_judge = st.active_judge.saturating_sub(1),
            Lane::Operator => {}
        }
    }
}

/// What the adapter knows about an attempt before sending it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptEstimate {
    /// UTF-8 length of the request body.
    pub request_bytes: u64,
    /// The `max_completion_tokens` the request will carry.
    pub max_completion_tokens: u64,
}

/// One admitted attempt. Not `Clone`: settle it exactly once.
#[derive(Debug, PartialEq, Eq)]
pub struct Draw {
    reservation: ReservationId,
    attempt: u32,
    est: Usd,
    input_bound_tokens: u64,
    max_completion_tokens: u64,
}

impl Draw {
    pub fn reservation(&self) -> &ReservationId {
        &self.reservation
    }

    /// 1-based attempt number within the reservation.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The attempt's worst case, charged in full on an unknown outcome.
    pub fn estimate(&self) -> Usd {
        self.est
    }

    pub fn input_bound_tokens(&self) -> u64 {
        self.input_bound_tokens
    }

    pub fn max_completion_tokens(&self) -> u64 {
        self.max_completion_tokens
    }
}

/// The result of settling one attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub charged: Usd,
    pub outcome: RecordedOutcome,
    /// The ledger's trip after this settlement, if tripped.
    pub tripped: Option<Trip>,
}

/// The result of releasing a reservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Released {
    /// What the run cost for sure: settled charges plus unsettled draws.
    pub spent: Usd,
}

/// Per-attempt metering the Meta adapter calls around each HTTP attempt:
/// [`AttemptMeter::draw`] before the request is sent, [`AttemptMeter::settle`]
/// right after it ends. A success is settled with
/// [`Outcome::Success`]`(`[`Usage::from_chat_usage`]`(prompt_tokens,
/// completion_tokens, reasoning_tokens))`, passing `completion_tokens`
/// exactly as the API reported it (it already includes reasoning).
pub trait AttemptMeter: Send + Sync {
    fn draw(&self, est: AttemptEstimate) -> Result<Draw, Refusal>;
    fn settle(&self, draw: Draw, outcome: Outcome) -> Result<Settlement, LedgerError>;
}

/// An [`AttemptMeter`] bound to one reservation.
#[derive(Clone)]
pub struct ReservationMeter {
    ledger: Ledger,
    reservation: ReservationId,
}

impl fmt::Debug for ReservationMeter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReservationMeter")
            .field("reservation", &self.reservation)
            .finish_non_exhaustive()
    }
}

impl AttemptMeter for ReservationMeter {
    fn draw(&self, est: AttemptEstimate) -> Result<Draw, Refusal> {
        self.ledger.draw_for(&self.reservation, est)
    }

    fn settle(&self, draw: Draw, outcome: Outcome) -> Result<Settlement, LedgerError> {
        self.ledger.settle(draw, outcome)
    }
}

/// The ledger's full state, for the operator. It carries internal detail
/// (`trip_reason`, `inconsistent`) that must not reach a visitor: the
/// `/capabilities` payload is [`Snapshot::public`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    /// The UTC day the daily figures refer to.
    pub day: String,
    pub daily_cap: Usd,
    /// Worst-case exposure of today's non-judge reservations.
    pub daily_committed: Usd,
    pub daily_remaining: Usd,
    /// What the public lane could reserve right now, pacing included.
    pub daily_available_now: Usd,
    pub lifetime_cap: Usd,
    pub lifetime_spent: Usd,
    pub lifetime_committed: Usd,
    pub lifetime_remaining: Usd,
    pub judge_slice: Usd,
    pub judge_committed: Usd,
    pub judge_remaining: Usd,
    pub judge_daily_cap: Usd,
    /// Worst-case exposure of today's judge reservations.
    pub judge_daily_committed: Usd,
    pub judge_daily_remaining: Usd,
    pub run_worst_case: Usd,
    pub hourly_cap: u32,
    pub hourly_public: u32,
    pub hourly_judge: u32,
    pub max_concurrent: u32,
    pub active_public: u32,
    pub active_judge: u32,
    pub reservations: u64,
    pub unsettled_reservations: u64,
    pub tripped: bool,
    /// The coarse, publishable reason.
    pub trip_code: Option<TripReason>,
    /// Operator-only detail (may name an upstream HTTP status or amounts).
    pub trip_reason: Option<String>,
    /// Operator-only: set when the summary and the day files disagree.
    pub inconsistent: Option<String>,
}

impl Snapshot {
    /// The visitor-safe projection for `/capabilities`: remaining budgets,
    /// whether live mode is tripped and the coarse code, nothing else.
    pub fn public(&self) -> PublicSnapshot {
        PublicSnapshot {
            daily_remaining: self.daily_remaining,
            daily_available_now: self.daily_available_now,
            lifetime_remaining: self.lifetime_remaining,
            judge_remaining: self.judge_remaining,
            tripped: self.tripped,
            trip_code: self.trip_code,
        }
    }
}

/// What `/capabilities` may publish about the ledger; see
/// [`Snapshot::public`]. No detail strings, statuses, paths or amounts
/// from trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicSnapshot {
    pub daily_remaining: Usd,
    /// What the public lane could reserve right now, pacing included.
    pub daily_available_now: Usd,
    pub lifetime_remaining: Usd,
    pub judge_remaining: Usd,
    pub tripped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trip_code: Option<TripReason>,
}

struct ResState {
    lane: Lane,
    worst_case: Usd,
    at: DateTime<Utc>,
    charged: Usd,
    open: BTreeMap<u32, Usd>,
    attempts: u32,
    released: bool,
}

impl ResState {
    fn open_total(&self) -> Usd {
        self.open
            .values()
            .fold(Usd::ZERO, |acc, est| acc.saturating_add(*est))
    }

    /// Spent for sure: settled charges plus unsettled draws.
    fn spent(&self) -> Usd {
        self.charged.saturating_add(self.open_total())
    }

    /// What the caps must hold for this reservation.
    fn exposure(&self) -> Usd {
        if self.released {
            self.spent()
        } else {
            self.worst_case
        }
    }

    fn remaining_ceiling(&self) -> Usd {
        self.worst_case.saturating_sub(self.spent())
    }
}

#[derive(Default)]
struct Cursor {
    /// Bytes of the file already applied.
    offset: u64,
    /// The last applied record has no trailing newline yet.
    needs_newline: bool,
}

/// What the lifetime summary remembered at open that the day files no
/// longer show, fixed once at open and added to every total from then on
/// (and so carried into the next summary): lost history stays counted.
#[derive(Default, Clone, Copy)]
struct Offsets {
    spent: Usd,
    judge_committed: Usd,
    nonjudge_committed: Usd,
}

impl Offsets {
    fn is_zero(&self) -> bool {
        self.spent.is_zero() && self.judge_committed.is_zero() && self.nonjudge_committed.is_zero()
    }
}

#[derive(Default)]
struct State {
    reservations: BTreeMap<ReservationId, ResState>,
    tripped: Option<Trip>,
    /// When the files were last read under the lock.
    last_catch_up: Option<Instant>,
    /// The newest `at` among the records read from the day files.
    last_record_at: Option<DateTime<Utc>>,
    cursors: BTreeMap<String, Cursor>,
    offsets: Offsets,
    inconsistent: Option<String>,
    active_public: u32,
    active_judge: u32,
}

struct Totals {
    lifetime_spent: Usd,
    lifetime_committed: Usd,
    nonjudge_committed: Usd,
    judge_committed: Usd,
    daily_committed: Usd,
    judge_daily_committed: Usd,
    reservations: u64,
    unsettled: u64,
    hourly_public: u32,
    hourly_judge: u32,
    oldest_public_in_window: Option<DateTime<Utc>>,
    oldest_judge_in_window: Option<DateTime<Utc>>,
}

struct Inner {
    dir: PathBuf,
    cfg: BudgetConfig,
    clock: LedgerClock,
    options: LedgerOptions,
    state: Mutex<State>,
}

/// The ledger handle; clones share one process-local state and the same
/// files. See the module docs.
#[derive(Clone)]
pub struct Ledger {
    inner: Arc<Inner>,
}

impl fmt::Debug for Ledger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ledger")
            .field("dir", &self.inner.dir)
            .finish_non_exhaustive()
    }
}

struct FileLock(File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_at(at: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(at)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| format!("bad timestamp {at:?}: {e}"))
}

/// Makes `dir`'s entries durable after a file was created or renamed into
/// it: fsync on Unix; nothing on Windows, where std cannot open a
/// directory for syncing (same limitation as the event log).
fn sync_dir(dir: &Path) -> Result<(), LedgerError> {
    if cfg!(unix) {
        File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| LedgerError::io(dir, e))?;
    }
    Ok(())
}

fn day_file_name(day: NaiveDate) -> String {
    format!("{}.jsonl", day.format("%Y-%m-%d"))
}

fn is_day_file(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 16
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && &name[10..] == ".jsonl"
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    fn open_lock_file(&self) -> Result<(PathBuf, File), LedgerError> {
        let path = self.dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| LedgerError::io(&path, e))?;
        Ok((path, file))
    }

    fn acquire_lock(&self) -> Result<FileLock, LedgerError> {
        let (path, file) = self.open_lock_file()?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(FileLock(file)),
                Err(TryLockError::WouldBlock) => {
                    let waited = started.elapsed();
                    if waited >= self.options.lock_wait {
                        return Err(LedgerError::Locked {
                            lock_file: path,
                            waited,
                        });
                    }
                    std::thread::sleep(self.options.lock_poll);
                }
                Err(TryLockError::Error(e)) => return Err(LedgerError::io(&path, e)),
            }
        }
    }

    /// One attempt at the lock, never waiting: `None` when another writer
    /// holds it.
    fn try_acquire_lock(&self) -> Result<Option<FileLock>, LedgerError> {
        let (path, file) = self.open_lock_file()?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FileLock(file))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(LedgerError::io(&path, e)),
        }
    }

    /// Reads every record other handles appended since the last look.
    fn catch_up(&self, st: &mut State) -> Result<(), LedgerError> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .map_err(|e| LedgerError::io(&self.dir, e))?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_day_file(name))
            .collect();
        names.sort();
        let mut pending: Vec<(PathBuf, u64, Record)> = Vec::new();
        for name in names {
            let path = self.dir.join(&name);
            let cursor = st.cursors.entry(name.clone()).or_default();
            let mut file = File::open(&path).map_err(|e| LedgerError::io(&path, e))?;
            let len = file
                .metadata()
                .map_err(|e| LedgerError::io(&path, e))?
                .len();
            if len < cursor.offset {
                return Err(LedgerError::Corrupt {
                    file: path,
                    offset: len,
                    reason: format!(
                        "file shrank below the {} bytes already applied",
                        cursor.offset
                    ),
                });
            }
            if len == cursor.offset {
                continue;
            }
            file.seek(SeekFrom::Start(cursor.offset))
                .map_err(|e| LedgerError::io(&path, e))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| LedgerError::io(&path, e))?;
            let start = cursor.offset;
            let mut pos = 0usize;
            while let Some(nl) = buf[pos..].iter().position(|b| *b == b'\n') {
                let line = &buf[pos..pos + nl];
                let line_offset = start + pos as u64;
                if !line.is_empty() {
                    let record = parse_record(&path, line_offset, line)?;
                    if let Err(record) = apply(st, record) {
                        pending.push((path.clone(), line_offset, record));
                    }
                }
                pos += nl + 1;
            }
            let tail = &buf[pos..];
            let line_end = start + pos as u64;
            let (new_offset, needs_newline) = if tail.is_empty() {
                (line_end, false)
            } else if let Ok(record) = serde_json::from_slice::<Record>(tail) {
                // A complete record whose newline never made it to disk.
                let record = validate_record(record, &path, line_end)?;
                if let Err(record) = apply(st, record) {
                    pending.push((path.clone(), line_end, record));
                }
                (len, true)
            } else {
                // A torn tail from a crash mid-write: the appender truncates it.
                (line_end, false)
            };
            let cursor = st.cursors.get_mut(&name).expect("cursor inserted above");
            cursor.offset = new_offset;
            cursor.needs_newline = needs_newline;
        }
        // Records that referred to a reservation read later (clock skew
        // across a day boundary) apply once everything is in.
        let mut progress = true;
        while progress && !pending.is_empty() {
            progress = false;
            let mut still = Vec::new();
            for (path, offset, record) in pending {
                match apply(st, record) {
                    Ok(()) => progress = true,
                    Err(record) => still.push((path, offset, record)),
                }
            }
            pending = still;
        }
        if let Some((file, offset, _)) = pending.into_iter().next() {
            return Err(LedgerError::Corrupt {
                file,
                offset,
                reason: "record refers to a reservation no file holds".into(),
            });
        }
        Ok(())
    }

    /// Appends one record to today's file, fsyncs it and applies it.
    fn append(&self, st: &mut State, record: Record) -> Result<(), LedgerError> {
        let now = self.now();
        let name = day_file_name(now.date_naive());
        let path = self.dir.join(&name);
        let cursor = st.cursors.entry(name.clone()).or_default();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| LedgerError::io(&path, e))?;
        let len = file
            .metadata()
            .map_err(|e| LedgerError::io(&path, e))?
            .len();
        if len > cursor.offset {
            // Torn tail left by a crash: cut it before writing.
            file.set_len(cursor.offset)
                .map_err(|e| LedgerError::io(&path, e))?;
        }
        let mut line = serde_json::to_vec(&record).expect("record serializes");
        line.push(b'\n');
        if cursor.needs_newline {
            line.insert(0, b'\n');
        }
        file.seek(SeekFrom::Start(cursor.offset))
            .map_err(|e| LedgerError::io(&path, e))?;
        file.write_all(&line)
            .map_err(|e| LedgerError::io(&path, e))?;
        file.sync_all().map_err(|e| LedgerError::io(&path, e))?;
        if len == 0 {
            // A new day file: its directory entry must be durable too.
            sync_dir(&self.dir)?;
        }
        cursor.offset += line.len() as u64;
        cursor.needs_newline = false;
        let new_offset = cursor.offset;
        if apply(st, record).is_err() {
            return Err(LedgerError::Corrupt {
                file: path,
                offset: new_offset,
                reason: "appended record refers to an unknown reservation".into(),
            });
        }
        Ok(())
    }

    fn write_summary(&self, st: &State) -> Result<(), LedgerError> {
        let totals = self.totals(st);
        let summary = LifetimeSummary {
            v: RECORD_VERSION,
            updated_at: rfc3339(self.now()),
            lifetime_spent_usd: totals.lifetime_spent,
            lifetime_committed_usd: totals.lifetime_committed,
            judge_committed_usd: totals.judge_committed,
            nonjudge_committed_usd: totals.nonjudge_committed,
            reservations: totals.reservations,
            unsettled_reservations: totals.unsettled,
            tripped: st.tripped.is_some(),
            trip_code: st.tripped.as_ref().map(|t| t.code),
            trip_reason: st.tripped.as_ref().map(|t| t.detail.clone()),
        };
        let path = self.dir.join(SUMMARY_FILE);
        let tmp = self.dir.join(format!("{SUMMARY_FILE}.tmp"));
        let bytes = serde_json::to_vec_pretty(&summary).expect("summary serializes");
        {
            let mut file = File::create(&tmp).map_err(|e| LedgerError::io(&tmp, e))?;
            file.write_all(&bytes)
                .map_err(|e| LedgerError::io(&tmp, e))?;
            file.sync_all().map_err(|e| LedgerError::io(&tmp, e))?;
        }
        std::fs::rename(&tmp, &path).map_err(|e| LedgerError::io(&path, e))?;
        sync_dir(&self.dir)
    }

    /// Reconciles the replayed day files with the summary at open. The
    /// summary is written right after every record, so it is normally at
    /// least as new as the newest record; then anything it remembers that
    /// the files do not show (a lost or truncated day file) becomes an
    /// offset that stays counted, and a trip it remembers is honored. When
    /// the files hold a record newer than the summary, the summary is the
    /// stale side (a crash between an append and the summary rewrite) and
    /// the files are authoritative: no offset, no re-trip. Finally the
    /// lifetime cap is checked, so a floor that reaches it trips at once.
    fn apply_summary(&self, st: &mut State, summary: &LifetimeSummary) -> Result<(), LedgerError> {
        let summary_at = parse_at(&summary.updated_at).ok();
        let files_newer = match (summary_at, st.last_record_at) {
            (Some(summary_at), Some(last)) => last > summary_at,
            // No readable summary time, or no records: the summary stands.
            _ => false,
        };
        if !files_newer {
            let replayed = self.totals(st);
            let nonjudge_in_summary = summary.nonjudge_committed_usd.max(
                summary
                    .lifetime_committed_usd
                    .saturating_sub(summary.judge_committed_usd),
            );
            let offsets = Offsets {
                spent: summary
                    .lifetime_spent_usd
                    .saturating_sub(replayed.lifetime_spent),
                judge_committed: summary
                    .judge_committed_usd
                    .saturating_sub(replayed.judge_committed),
                nonjudge_committed: nonjudge_in_summary.saturating_sub(replayed.nonjudge_committed),
            };
            if !offsets.is_zero() {
                st.offsets = offsets;
                st.inconsistent = Some(format!(
                    "lifetime summary records {} spent and {} committed ({} judge), day files {} spent and {} committed ({} judge): the larger figures are used",
                    summary.lifetime_spent_usd,
                    summary.lifetime_committed_usd,
                    summary.judge_committed_usd,
                    replayed.lifetime_spent,
                    replayed.lifetime_committed,
                    replayed.judge_committed
                ));
            }
            if summary.tripped && st.tripped.is_none() {
                let reason = summary
                    .trip_reason
                    .clone()
                    .unwrap_or_else(|| "unrecorded".into());
                let code = summary.trip_code.unwrap_or(TripReason::Inconsistent);
                st.tripped = Some(Trip::new(
                    code,
                    format!("summary_trip_not_in_day_files: {reason}"),
                ));
                st.inconsistent = Some(format!(
                    "lifetime summary records a trip ({reason}) the day files do not"
                ));
            }
        }
        self.check_lifetime(st)
    }

    fn read_summary(&self) -> Result<Option<LifetimeSummary>, LedgerError> {
        let path = self.dir.join(SUMMARY_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|e| LedgerError::Corrupt {
                        file: path,
                        offset: 0,
                        reason: format!("lifetime summary unreadable: {e}"),
                    })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(LedgerError::io(&path, e)),
        }
    }

    fn totals(&self, st: &State) -> Totals {
        let now = self.now();
        let today = now.date_naive();
        let window_start = now - TimeDelta::seconds(SECONDS_PER_HOUR);
        let mut t = Totals {
            lifetime_spent: Usd::ZERO,
            lifetime_committed: Usd::ZERO,
            nonjudge_committed: Usd::ZERO,
            judge_committed: Usd::ZERO,
            daily_committed: Usd::ZERO,
            judge_daily_committed: Usd::ZERO,
            reservations: 0,
            unsettled: 0,
            hourly_public: 0,
            hourly_judge: 0,
            oldest_public_in_window: None,
            oldest_judge_in_window: None,
        };
        for res in st.reservations.values() {
            let exposure = res.exposure();
            t.reservations += 1;
            t.lifetime_spent = t.lifetime_spent.saturating_add(res.spent());
            t.lifetime_committed = t.lifetime_committed.saturating_add(exposure);
            if !res.released {
                t.unsettled += 1;
            }
            let in_window = res.at > window_start && res.at <= now;
            match res.lane {
                Lane::Judge => {
                    t.judge_committed = t.judge_committed.saturating_add(exposure);
                    if res.at.date_naive() == today {
                        t.judge_daily_committed = t.judge_daily_committed.saturating_add(exposure);
                    }
                    if in_window {
                        t.hourly_judge += 1;
                        t.oldest_judge_in_window =
                            Some(t.oldest_judge_in_window.map_or(res.at, |o| o.min(res.at)));
                    }
                }
                Lane::Public | Lane::Operator => {
                    t.nonjudge_committed = t.nonjudge_committed.saturating_add(exposure);
                    if res.at.date_naive() == today {
                        t.daily_committed = t.daily_committed.saturating_add(exposure);
                    }
                    if res.lane == Lane::Public && in_window {
                        t.hourly_public += 1;
                        t.oldest_public_in_window =
                            Some(t.oldest_public_in_window.map_or(res.at, |o| o.min(res.at)));
                    }
                }
            }
        }
        // History the summary remembers and the files lost, per lane.
        let o = st.offsets;
        t.lifetime_spent = t.lifetime_spent.saturating_add(o.spent);
        t.judge_committed = t.judge_committed.saturating_add(o.judge_committed);
        t.nonjudge_committed = t.nonjudge_committed.saturating_add(o.nonjudge_committed);
        t.lifetime_committed = t
            .lifetime_committed
            .saturating_add(o.judge_committed)
            .saturating_add(o.nonjudge_committed)
            .max(t.lifetime_spent);
        t
    }

    /// Daily budget the public lane may reserve from right now.
    fn released_today(&self, now: DateTime<Utc>) -> Usd {
        let elapsed = u64::from(now.num_seconds_from_midnight());
        let paced = self
            .cfg
            .burst()
            .saturating_add(self.cfg.daily_cap.mul_div(elapsed, SECONDS_PER_DAY));
        paced.min(self.cfg.daily_cap)
    }

    /// Seconds until `need` of today's budget has been released, or until
    /// midnight when today cannot hold it.
    fn retry_after_daily(&self, now: DateTime<Utc>, need: Usd) -> Duration {
        let elapsed = u64::from(now.num_seconds_from_midnight());
        let to_midnight = SECONDS_PER_DAY.saturating_sub(elapsed).max(1);
        if need > self.cfg.daily_cap {
            return Duration::from_secs(to_midnight);
        }
        let burst = self.cfg.burst();
        if need <= burst {
            return Duration::from_secs(1);
        }
        let numerator =
            u128::from(need.saturating_sub(burst).micros()) * u128::from(SECONDS_PER_DAY);
        let target = numerator.div_ceil(u128::from(self.cfg.daily_cap.micros()));
        let target = u64::try_from(target).unwrap_or(SECONDS_PER_DAY);
        Duration::from_secs(target.saturating_sub(elapsed).clamp(1, to_midnight))
    }

    fn check_kill_switch(&self, st: &mut State) -> Result<(), LedgerError> {
        if let Some(path) = &self.cfg.kill_switch_file {
            if st.tripped.is_none() && path.exists() {
                self.trip(
                    st,
                    Trip::new(TripReason::KillSwitch, "kill_switch_file_present"),
                )?;
            }
        }
        Ok(())
    }

    /// Trips once (a second reason is not recorded over the first) and
    /// rewrites the summary, so the trip is visible there too.
    fn trip(&self, st: &mut State, trip: Trip) -> Result<(), LedgerError> {
        if st.tripped.is_some() {
            return Ok(());
        }
        self.append(
            st,
            Record::Trip {
                v: RECORD_VERSION,
                code: Some(trip.code),
                reason: trip.detail,
                at: rfc3339(self.now()),
            },
        )?;
        self.write_summary(st)
    }

    /// Trips when the lifetime spend has reached the lifetime cap.
    fn check_lifetime(&self, st: &mut State) -> Result<(), LedgerError> {
        if st.tripped.is_none() && self.totals(st).lifetime_spent >= self.cfg.lifetime_cap {
            self.trip(
                st,
                Trip::new(TripReason::LifetimeCap, "lifetime_cap_reached"),
            )?;
        }
        Ok(())
    }

    /// The refusal a tripped ledger answers with.
    fn tripped_refusal(st: &State) -> Option<Refusal> {
        st.tripped.as_ref().map(|t| Refusal::Tripped {
            code: t.code,
            reason: t.detail.clone(),
        })
    }

    fn snapshot(&self, st: &State) -> Snapshot {
        let now = self.now();
        let t = self.totals(st);
        let cfg = &self.cfg;
        Snapshot {
            day: now.date_naive().format("%Y-%m-%d").to_string(),
            daily_cap: cfg.daily_cap,
            daily_committed: t.daily_committed,
            daily_remaining: cfg.daily_cap.saturating_sub(t.daily_committed),
            daily_available_now: self.released_today(now).saturating_sub(t.daily_committed),
            lifetime_cap: cfg.lifetime_cap,
            lifetime_spent: t.lifetime_spent,
            lifetime_committed: t.lifetime_committed,
            lifetime_remaining: cfg.lifetime_cap.saturating_sub(t.lifetime_committed),
            judge_slice: cfg.judge_slice,
            judge_committed: t.judge_committed,
            judge_remaining: cfg.judge_slice.saturating_sub(t.judge_committed),
            judge_daily_cap: cfg.judge_daily_cap,
            judge_daily_committed: t.judge_daily_committed,
            judge_daily_remaining: cfg.judge_daily_cap.saturating_sub(t.judge_daily_committed),
            run_worst_case: cfg.run_worst_case(),
            hourly_cap: cfg.hourly_admissions,
            hourly_public: t.hourly_public,
            hourly_judge: t.hourly_judge,
            max_concurrent: cfg.max_concurrent,
            active_public: st.active_public,
            active_judge: st.active_judge,
            reservations: t.reservations,
            unsettled_reservations: t.unsettled,
            tripped: st.tripped.is_some(),
            trip_code: st.tripped.as_ref().map(|t| t.code),
            trip_reason: st.tripped.as_ref().map(|t| t.detail.clone()),
            inconsistent: st.inconsistent.clone(),
        }
    }
}

fn parse_record(path: &Path, offset: u64, line: &[u8]) -> Result<Record, LedgerError> {
    let record: Record = serde_json::from_slice(line).map_err(|e| LedgerError::Corrupt {
        file: path.to_path_buf(),
        offset,
        reason: e.to_string(),
    })?;
    validate_record(record, path, offset)
}

/// Applies one record; `Err` hands back a record whose reservation is not
/// known yet.
fn apply(st: &mut State, record: Record) -> Result<(), Record> {
    // Stored records were validated by `validate_record`, and the ledger's
    // own records come from `rfc3339`; the fallback is unreachable.
    let at = parse_at(record.at()).unwrap_or_default();
    st.last_record_at = Some(st.last_record_at.map_or(at, |last| last.max(at)));
    match record {
        Record::Reservation {
            id,
            lane,
            worst_case_usd,
            at,
            ..
        } => {
            let at = parse_at(&at).unwrap_or_default();
            st.reservations.entry(id).or_insert(ResState {
                lane,
                worst_case: worst_case_usd,
                at,
                charged: Usd::ZERO,
                open: BTreeMap::new(),
                attempts: 0,
                released: false,
            });
            Ok(())
        }
        Record::Draw {
            ref reservation,
            attempt,
            est_usd,
            ..
        } => match st.reservations.get_mut(reservation) {
            Some(res) => {
                res.open.insert(attempt, est_usd);
                res.attempts = res.attempts.max(attempt);
                Ok(())
            }
            None => Err(record),
        },
        Record::Settlement {
            ref reservation,
            attempt,
            charged_usd,
            ..
        } => match st.reservations.get_mut(reservation) {
            Some(res) => {
                res.open.remove(&attempt);
                res.charged = res.charged.saturating_add(charged_usd);
                Ok(())
            }
            None => Err(record),
        },
        Record::Release {
            ref reservation, ..
        } => match st.reservations.get_mut(reservation) {
            Some(res) => {
                res.released = true;
                Ok(())
            }
            None => Err(record),
        },
        Record::Trip { code, reason, .. } => {
            let code = code.unwrap_or_else(|| infer_trip_code(&reason));
            st.tripped = Some(Trip::new(code, reason));
            Ok(())
        }
        Record::Reset { .. } => {
            st.tripped = None;
            Ok(())
        }
    }
}

impl Ledger {
    /// Opens (creating when missing) the ledger in `dir` with the system
    /// clock and default options, replaying every day file.
    pub fn open(dir: impl AsRef<Path>, cfg: BudgetConfig) -> Result<Self, LedgerError> {
        Self::open_with(dir, cfg, Arc::new(Utc::now), LedgerOptions::default())
    }

    pub fn open_with(
        dir: impl AsRef<Path>,
        cfg: BudgetConfig,
        clock: LedgerClock,
        options: LedgerOptions,
    ) -> Result<Self, LedgerError> {
        cfg.validate()?;
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| LedgerError::io(&dir, e))?;
        let ledger = Ledger {
            inner: Arc::new(Inner {
                dir,
                cfg,
                clock,
                options,
                state: Mutex::new(State::default()),
            }),
        };
        ledger.locked(|inner, st| {
            if let Some(summary) = inner.read_summary()? {
                inner.apply_summary(st, &summary)?;
            }
            Ok::<(), LedgerError>(())
        })?;
        Ok(ledger)
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    pub fn config(&self) -> &BudgetConfig {
        &self.inner.cfg
    }

    /// Runs `f` under the file lock and the state mutex, after reading what
    /// other handles appended. Lock order: file lock first, then the state
    /// mutex ([`Ledger::snapshot_cached`] takes them the other way round
    /// but never waits for the file lock, so the two cannot deadlock).
    fn locked<T, E: From<LedgerError>>(
        &self,
        f: impl FnOnce(&Inner, &mut State) -> Result<T, E>,
    ) -> Result<T, E> {
        let _lock = self.inner.acquire_lock()?;
        let mut st = self.inner.state();
        self.inner.catch_up(&mut st)?;
        st.last_catch_up = Some(Instant::now());
        f(&self.inner, &mut st)
    }

    /// Admits one run on `lane`, reserving its worst case; see the module
    /// docs for what each lane is checked against.
    pub fn reserve(&self, lane: Lane) -> Result<Reservation, Refusal> {
        let worst_case = self.inner.cfg.run_worst_case();
        let id = ReservationId(uuid::Uuid::new_v4().to_string());
        let inner = self.inner.clone();
        self.locked(move |ledger, st| {
            ledger.check_kill_switch(st)?;
            if let Some(refusal) = Inner::tripped_refusal(st) {
                return Err(refusal);
            }
            let now = ledger.now();
            let cfg = &ledger.cfg;
            let t = ledger.totals(st);
            let public_share = cfg.lifetime_cap.saturating_sub(cfg.judge_slice);
            match lane {
                Lane::Public => {
                    if st.active_public >= cfg.max_concurrent {
                        return Err(Refusal::Concurrency {
                            retry_after: CONCURRENCY_RETRY_AFTER,
                        });
                    }
                    if t.hourly_public >= cfg.hourly_admissions {
                        return Err(Refusal::HourlyAdmissions {
                            retry_after: window_retry(now, t.oldest_public_in_window),
                        });
                    }
                    if public_share.saturating_sub(t.nonjudge_committed) < worst_case {
                        return Err(Refusal::LifetimeBudget);
                    }
                    let available = ledger.released_today(now).saturating_sub(t.daily_committed);
                    if available < worst_case {
                        let need = t.daily_committed.saturating_add(worst_case);
                        return Err(Refusal::DailyBudget {
                            retry_after: ledger.retry_after_daily(now, need),
                        });
                    }
                }
                Lane::Judge => {
                    if st.active_judge >= 1 {
                        return Err(Refusal::Concurrency {
                            retry_after: CONCURRENCY_RETRY_AFTER,
                        });
                    }
                    if t.hourly_judge >= cfg.hourly_admissions {
                        return Err(Refusal::HourlyAdmissions {
                            retry_after: window_retry(now, t.oldest_judge_in_window),
                        });
                    }
                    if cfg.judge_slice.saturating_sub(t.judge_committed) < worst_case {
                        return Err(Refusal::JudgeSlice);
                    }
                    if cfg.judge_daily_cap.saturating_sub(t.judge_daily_committed) < worst_case {
                        return Err(Refusal::JudgeDaily {
                            retry_after: until_midnight(now),
                        });
                    }
                }
                Lane::Operator => {
                    if public_share.saturating_sub(t.nonjudge_committed) < worst_case {
                        return Err(Refusal::LifetimeBudget);
                    }
                    if cfg.daily_cap.saturating_sub(t.daily_committed) < worst_case {
                        let need = t.daily_committed.saturating_add(worst_case);
                        return Err(Refusal::DailyBudget {
                            retry_after: ledger.retry_after_daily(now, need),
                        });
                    }
                }
            }
            ledger.append(
                st,
                Record::Reservation {
                    v: RECORD_VERSION,
                    id: id.clone(),
                    lane,
                    worst_case_usd: worst_case,
                    at: rfc3339(now),
                },
            )?;
            ledger.write_summary(st)?;
            let slot = match lane {
                Lane::Public => {
                    st.active_public += 1;
                    Some(SlotGuard { inner, lane })
                }
                Lane::Judge => {
                    st.active_judge += 1;
                    Some(SlotGuard { inner, lane })
                }
                Lane::Operator => None,
            };
            Ok(Reservation {
                id,
                lane,
                worst_case,
                slot: Mutex::new(slot),
            })
        })
    }

    /// Draws one attempt on `reservation`; see the module docs.
    pub fn draw(&self, reservation: &Reservation, est: AttemptEstimate) -> Result<Draw, Refusal> {
        self.draw_for(&reservation.id, est)
    }

    /// An [`AttemptMeter`] for `reservation`, for the Meta adapter.
    pub fn meter(&self, reservation: &Reservation) -> ReservationMeter {
        ReservationMeter {
            ledger: self.clone(),
            reservation: reservation.id.clone(),
        }
    }

    fn draw_for(&self, reservation: &ReservationId, est: AttemptEstimate) -> Result<Draw, Refusal> {
        let cfg = &self.inner.cfg;
        let input_bound_tokens = BudgetConfig::input_bound_tokens(est.request_bytes);
        if input_bound_tokens > cfg.input_cap_tokens {
            return Err(Refusal::InputCap {
                bound_tokens: input_bound_tokens,
                cap_tokens: cfg.input_cap_tokens,
            });
        }
        if est.max_completion_tokens > cfg.max_completion_tokens {
            return Err(Refusal::OutputCap {
                requested: est.max_completion_tokens,
                cap: cfg.max_completion_tokens,
            });
        }
        let est_usd = cfg.attempt_worst_case(input_bound_tokens, est.max_completion_tokens);
        let id = reservation.clone();
        self.locked(move |ledger, st| {
            ledger.check_kill_switch(st)?;
            if let Some(refusal) = Inner::tripped_refusal(st) {
                return Err(refusal);
            }
            let res = st
                .reservations
                .get(&id)
                .ok_or_else(|| LedgerError::UnknownReservation(id.clone()))?;
            if res.released {
                return Err(LedgerError::Released(id.clone()).into());
            }
            let remaining = res.remaining_ceiling();
            if remaining < est_usd {
                return Err(Refusal::ReservationExhausted {
                    remaining,
                    needed: est_usd,
                });
            }
            let attempt = res.attempts + 1;
            ledger.append(
                st,
                Record::Draw {
                    v: RECORD_VERSION,
                    reservation: id.clone(),
                    attempt,
                    est_usd,
                    input_bound_tokens,
                    max_completion_tokens: est.max_completion_tokens,
                    at: rfc3339(ledger.now()),
                },
            )?;
            ledger.write_summary(st)?;
            Ok(Draw {
                reservation: id,
                attempt,
                est: est_usd,
                input_bound_tokens,
                max_completion_tokens: est.max_completion_tokens,
            })
        })
    }

    /// Settles one attempt; see the module docs for the charges and trips.
    pub fn settle(&self, draw: Draw, outcome: Outcome) -> Result<Settlement, LedgerError> {
        self.locked(move |ledger, st| {
            let res = st
                .reservations
                .get(&draw.reservation)
                .ok_or_else(|| LedgerError::UnknownReservation(draw.reservation.clone()))?;
            if !res.open.contains_key(&draw.attempt) {
                return Err(LedgerError::UnknownAttempt {
                    reservation: draw.reservation.clone(),
                    attempt: draw.attempt,
                });
            }
            let (recorded, charged, detail, trip) = match outcome {
                Outcome::Success(usage) if !usage.is_consistent() => (
                    RecordedOutcome::Unknown,
                    draw.est,
                    Some(format!(
                        "2xx with inconsistent usage: reasoning_included {} > completion {}",
                        usage.reasoning_included, usage.completion
                    )),
                    None,
                ),
                Outcome::Success(usage) => {
                    let charged = ledger.cfg.charge(&usage);
                    let completion = usage.completion_tokens();
                    let trip = if completion > draw.max_completion_tokens {
                        Some(Trip::new(
                            TripReason::OutputBound,
                            format!(
                                "completion_tokens_exceeded_max: {completion} > {}",
                                draw.max_completion_tokens
                            ),
                        ))
                    } else if charged > draw.est {
                        Some(Trip::new(
                            TripReason::ChargeBound,
                            format!("charge_exceeded_attempt_draw: {charged} > {}", draw.est),
                        ))
                    } else {
                        None
                    };
                    (
                        RecordedOutcome::Usage {
                            input: usage.input,
                            completion: usage.completion,
                            reasoning_included: usage.reasoning_included,
                        },
                        charged,
                        None,
                        trip,
                    )
                }
                Outcome::SuccessWithoutUsage => (
                    RecordedOutcome::Unknown,
                    draw.est,
                    Some("2xx without usage".to_string()),
                    None,
                ),
                Outcome::Status(status) if ZERO_CHARGE_STATUSES.contains(&status) => (
                    RecordedOutcome::Zero,
                    Usd::ZERO,
                    Some(format!("http {status}")),
                    TRIPPING_STATUSES.contains(&status).then(|| {
                        Trip::new(
                            TripReason::ProviderRefusedKey,
                            format!("provider_refused_key: http {status}"),
                        )
                    }),
                ),
                Outcome::Status(status) => (
                    RecordedOutcome::Unknown,
                    draw.est,
                    Some(format!("http {status}")),
                    None,
                ),
                Outcome::Timeout => (
                    RecordedOutcome::Unknown,
                    draw.est,
                    Some("timeout".to_string()),
                    None,
                ),
                Outcome::TransportAfterSend => (
                    RecordedOutcome::Unknown,
                    draw.est,
                    Some("transport error after send".to_string()),
                    None,
                ),
                Outcome::NotSent => (
                    RecordedOutcome::Zero,
                    Usd::ZERO,
                    Some("not_sent".to_string()),
                    None,
                ),
            };
            ledger.append(
                st,
                Record::Settlement {
                    v: RECORD_VERSION,
                    reservation: draw.reservation.clone(),
                    attempt: draw.attempt,
                    outcome: recorded,
                    charged_usd: charged,
                    detail,
                    at: rfc3339(ledger.now()),
                },
            )?;
            if let Some(trip) = trip {
                ledger.trip(st, trip)?;
            }
            ledger.check_lifetime(st)?;
            ledger.write_summary(st)?;
            Ok(Settlement {
                charged,
                outcome: recorded,
                tripped: st.tripped.clone(),
            })
        })
    }

    /// Ends a run's reservation: from now on it counts at what it spent.
    /// Idempotent, and retry-safe: on an error (the lock wait ran out, an
    /// I/O failure) nothing was released, the caller keeps the handle and
    /// its slot, and may call again. The slot is freed only once the
    /// release record is durable.
    pub fn release(&self, reservation: &Reservation) -> Result<Released, LedgerError> {
        let released = self.release_record(&reservation.id, None)?;
        reservation.free_slot();
        Ok(released)
    }

    /// Operator-only: releases a reservation by id, for reconciliation
    /// after a crash left it counting at its worst case (check the
    /// provider dashboard first; `by` is recorded with the release). It
    /// frees no in-process slot: a handle still alive in some process keeps
    /// its slot until it drops. Idempotent. Never reachable from the public
    /// listener.
    pub fn release_by_id(&self, id: &ReservationId, by: &str) -> Result<Released, LedgerError> {
        self.release_record(id, Some(by.to_string()))
    }

    fn release_record(
        &self,
        id: &ReservationId,
        by: Option<String>,
    ) -> Result<Released, LedgerError> {
        let id = id.clone();
        self.locked(move |ledger, st| {
            let res = st
                .reservations
                .get(&id)
                .ok_or_else(|| LedgerError::UnknownReservation(id.clone()))?;
            if !res.released {
                ledger.append(
                    st,
                    Record::Release {
                        v: RECORD_VERSION,
                        reservation: id.clone(),
                        by,
                        at: rfc3339(ledger.now()),
                    },
                )?;
            }
            let spent = st.reservations[&id].spent();
            ledger.check_lifetime(st)?;
            ledger.write_summary(st)?;
            Ok::<Released, LedgerError>(Released { spent })
        })
    }

    /// Operator-only: clears a trip. Never reachable from the public
    /// listener. A kill-switch file still present trips again at the next
    /// admission; a lifetime cap already reached refuses every reservation
    /// anyway.
    pub fn reset(&self, by: &str) -> Result<(), LedgerError> {
        let by = by.to_string();
        self.locked(move |ledger, st| {
            if st.tripped.is_some() {
                ledger.append(
                    st,
                    Record::Reset {
                        v: RECORD_VERSION,
                        by,
                        at: rfc3339(ledger.now()),
                    },
                )?;
                ledger.write_summary(st)?;
            }
            Ok::<(), LedgerError>(())
        })
    }

    /// Operator-only: the current state after waiting for the file lock and
    /// reading what other handles appended. It also notices a kill-switch
    /// file and trips (a write). Never serve it from an unauthenticated
    /// route: a client hammering it would compete with admissions for the
    /// lock. `/capabilities` uses [`Ledger::snapshot_cached`].
    pub fn snapshot(&self) -> Result<Snapshot, LedgerError> {
        self.locked(|ledger, st| {
            ledger.check_kill_switch(st)?;
            Ok::<Snapshot, LedgerError>(ledger.snapshot(st))
        })
    }

    /// The read path for `/capabilities`: never waits for the file lock,
    /// never writes. It serves this process's in-process state, which every
    /// operation of this handle group refreshes; when that state is older
    /// than `max_age` *and* the file lock happens to be free, it reads what
    /// other processes appended first (one non-blocking `try_lock`, at most
    /// once per `max_age`). Time-dependent figures (`day`,
    /// `daily_available_now`, the hourly windows) are always computed for
    /// now; another process's records may be up to `max_age` late. It does
    /// not check the kill-switch file: that is a write, done at the next
    /// admission.
    pub fn snapshot_cached(&self, max_age: Duration) -> Result<Snapshot, LedgerError> {
        let mut st = self.inner.state();
        let stale = st.last_catch_up.is_none_or(|at| at.elapsed() >= max_age);
        if stale {
            if let Some(_lock) = self.inner.try_acquire_lock()? {
                self.inner.catch_up(&mut st)?;
                st.last_catch_up = Some(Instant::now());
            }
        }
        Ok(self.inner.snapshot(&st))
    }

    /// True when the ledger refuses every reservation (operator-only, see
    /// [`Ledger::snapshot`]).
    pub fn is_tripped(&self) -> Result<bool, LedgerError> {
        self.snapshot().map(|s| s.tripped)
    }
}

/// Seconds until the next UTC day (at least one).
fn until_midnight(now: DateTime<Utc>) -> Duration {
    let elapsed = u64::from(now.num_seconds_from_midnight());
    Duration::from_secs(SECONDS_PER_DAY.saturating_sub(elapsed).max(1))
}

fn window_retry(now: DateTime<Utc>, oldest: Option<DateTime<Utc>>) -> Duration {
    let secs = oldest
        .map(|o| (o + TimeDelta::seconds(SECONDS_PER_HOUR) - now).num_seconds())
        .unwrap_or(SECONDS_PER_HOUR);
    Duration::from_secs(u64::try_from(secs).unwrap_or(0).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_file_names_are_strict() {
        assert!(is_day_file("2026-10-27.jsonl"));
        assert!(!is_day_file("2026-10-27.jsonl.tmp"));
        assert!(!is_day_file("lifetime.json"));
        assert!(!is_day_file("ledger.lock"));
        assert!(!is_day_file("20261027.jsonl"));
        assert_eq!(
            day_file_name(NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()),
            "2026-10-27.jsonl"
        );
    }

    #[test]
    fn worst_cases_follow_the_owner_prices() {
        let cfg = BudgetConfig {
            daily_cap: Usd::parse("10").unwrap(),
            lifetime_cap: Usd::parse("60").unwrap(),
            judge_slice: Usd::parse("15").unwrap(),
            judge_daily_cap: Usd::parse("15").unwrap(),
            max_concurrent: 1,
            hourly_admissions: 6,
            burst_fraction: Ppm::parse("0.25").unwrap(),
            run_token_cap: 70_000,
            max_completion_tokens: 16_000,
            input_cap_tokens: 8_000,
            price_input_per_m: Usd::parse("4").unwrap(),
            price_output_per_m: Usd::parse("20").unwrap(),
            kill_switch_file: None,
        };
        cfg.validate().unwrap();
        assert_eq!(cfg.run_worst_case(), Usd::parse("1.88").unwrap());
        assert_eq!(cfg.public_share(), Usd::parse("45").unwrap());
        // The public share must hold one run, and the judge daily cap must
        // fit the slice.
        let dead_public = BudgetConfig {
            judge_slice: Usd::parse("58.50").unwrap(),
            judge_daily_cap: Usd::parse("15").unwrap(),
            ..cfg.clone()
        };
        let err = dead_public.validate().unwrap_err().to_string();
        assert!(err.contains("public share"), "{err}");
        assert!(BudgetConfig {
            judge_slice: Usd::parse("58").unwrap(),
            ..cfg.clone()
        }
        .validate()
        .is_ok());
        let err = BudgetConfig {
            judge_daily_cap: Usd::parse("15.01").unwrap(),
            ..cfg.clone()
        }
        .validate()
        .unwrap_err()
        .to_string();
        assert!(err.contains("judge daily cap"), "{err}");
        assert_eq!(cfg.burst(), Usd::parse("2.50").unwrap());
        assert_eq!(BudgetConfig::input_bound_tokens(5_000), 6_024);
        assert_eq!(
            cfg.attempt_worst_case(6_024, 16_000),
            Usd::parse("0.344096").unwrap()
        );
        assert_eq!(
            cfg.charge(&Usage {
                input: 2_688,
                completion: 10_692,
                reasoning_included: 0
            }),
            Usd::parse("0.224592").unwrap()
        );
        // Reasoning is a breakdown of the completion total, never added.
        assert_eq!(
            cfg.charge(&Usage::from_chat_usage(2_688, 10_692, Some(6_000))),
            Usd::parse("0.224592").unwrap()
        );
        assert!(Usage::from_chat_usage(1, 10, Some(10)).is_consistent());
        assert!(!Usage::from_chat_usage(1, 10, Some(11)).is_consistent());
        assert_eq!(
            Usage::from_chat_usage(5, 7, None),
            Usage {
                input: 5,
                completion: 7,
                reasoning_included: 0
            }
        );
    }

    #[test]
    fn trip_codes_are_stable_and_inferred_for_old_records() {
        for (code, text) in [
            (TripReason::KillSwitch, "kill_switch"),
            (TripReason::LifetimeCap, "lifetime_cap"),
            (TripReason::OutputBound, "output_bound"),
            (TripReason::ChargeBound, "charge_bound"),
            (TripReason::ProviderRefusedKey, "provider_refused_key"),
            (TripReason::Inconsistent, "inconsistent"),
        ] {
            assert_eq!(code.as_str(), text);
            assert_eq!(serde_json::to_string(&code).unwrap(), format!("{text:?}"));
        }
        assert_eq!(
            infer_trip_code("kill_switch_file_present"),
            TripReason::KillSwitch
        );
        assert_eq!(
            infer_trip_code("provider_refused_key: http 402"),
            TripReason::ProviderRefusedKey
        );
        assert_eq!(infer_trip_code("something else"), TripReason::Inconsistent);
        let old = r#"{"type":"trip","v":1,"reason":"lifetime_cap_reached","at":"2026-10-27T12:00:00.000Z"}"#;
        match serde_json::from_str::<Record>(old).unwrap() {
            Record::Trip { code, reason, .. } => {
                assert_eq!(code, None);
                assert_eq!(infer_trip_code(&reason), TripReason::LifetimeCap);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn records_round_trip_as_jsonl() {
        let rec = Record::Settlement {
            v: 1,
            reservation: ReservationId("r".into()),
            attempt: 2,
            outcome: RecordedOutcome::Usage {
                input: 1,
                completion: 3,
                reasoning_included: 2,
            },
            charged_usd: Usd::from_micros(64),
            detail: None,
            at: "2026-10-27T12:00:00.000Z".into(),
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"type\":\"settlement\""));
        assert!(json.contains("\"charged_usd\":\"0.000064\""));
        assert!(json.contains("\"kind\":\"usage\""));
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }
}
