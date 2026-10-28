// SPDX-License-Identifier: Apache-2.0
//! Admission controls for the public live mode: the USD budget ledger,
//! Turnstile verification, curated presets and the geographic policy.
//! Nothing here calls a model: these modules decide whether a visitor's
//! request may spend the operator's key at all. Filled in by lane S-b2;
//! the public API (lane S-b1) wires them in afterwards.
//!
//! - [`config`]: [`config::LiveConfig::from_env`], every variable with its
//!   default, validated (an invalid value is an error naming the
//!   variable). Secrets come from files and are never logged.
//! - [`money`]: exact money, integer micro-dollars, no floats.
//! - [`budget`]: the shared, fsynced, lock-serialized USD ledger
//!   ([`budget::Ledger`]): reserve a run's worst case, draw per HTTP
//!   attempt, settle on the outcome, never refund the unknown, trip and
//!   stay tripped; [`budget::AttemptMeter`] for the Meta adapter.
//! - [`judge`]: invite codes for the judge lane, stored only as SHA-256
//!   digests.
//! - [`turnstile`]: fail-closed Siteverify with action, hostname and
//!   freshness checks; test secrets refused in public mode.
//! - [`presets`]: the only tasks live mode can run, verbatim from the
//!   recorded batteries; no type here can carry visitor free text.
//! - [`geo`]: portal-wide and live-only country blocks, HTTP 451, fixed
//!   public codes.
//!
//! Suggested HTTP mapping for lane S-b1: a [`budget::Refusal`] answers
//! 429 with `Retry-After` when [`budget::Refusal::retry_after`] is set and
//! 503 otherwise, body `{code: refusal.code()}`; a
//! [`turnstile::TurnstileError`] answers 403 `turnstile_failed`; a
//! [`geo::GeoDecision::Block`] answers 451 with its code.
//!
//! Integration notes for the server and the Meta adapter:
//! - Check order per ABUSE-9: owner-scoped idempotency lookup, geo
//!   ([`geo::GeoPolicy::decide`] with [`geo::Surface::Live`]), the 18+
//!   and territory acknowledgment, Turnstile, then
//!   [`budget::Ledger::reserve`], and only then `create_task`. Nothing is
//!   spent before the reservation exists.
//! - Hold the [`budget::Reservation`] for the run's whole life and call
//!   [`budget::Ledger::release`] on every terminal state (finished,
//!   failed, cancelled, panicked). It takes the handle by reference and is
//!   retry-safe: on `Err` nothing was released and the handle keeps its
//!   slot, so retry (with backoff) before giving up; log the id of a
//!   reservation you could not release. A reservation that is only
//!   dropped is treated as a crash and keeps counting at its worst case
//!   until an operator reconciles it with [`budget::Ledger::release_by_id`]
//!   (operator console only).
//! - Give the adapter [`budget::Ledger::meter`]; it draws before each
//!   HTTP attempt and settles right after with
//!   [`budget::Usage::from_chat_usage`] (`completion_tokens` as reported,
//!   reasoning included). A draw refusal fails the run at stage `budget`;
//!   a settlement error leaves the attempt counted at its estimate, so the
//!   run should fail closed too.
//! - `/capabilities` reads the ledger with
//!   [`budget::Ledger::snapshot_cached`] (a few seconds of `max_age`) and
//!   publishes [`budget::Snapshot::public`] only. [`budget::Ledger::snapshot`]
//!   waits for the file lock and may write a trip record: operator-only.
//!   Never serialize a [`budget::Snapshot`] wholesale or put a
//!   [`budget::Refusal`]'s or [`budget::LedgerError`]'s `Display` text in
//!   a response: they carry upstream statuses, amounts and paths. The wire
//!   gets [`budget::Refusal::code`] and [`budget::TripReason::as_str`].
//! - [`budget::BudgetConfig::max_concurrent`] slots are per process; a
//!   server-side semaphore keyed by client (/64) is still needed.
//! - The ledger and the HTTP Siteverify client block on I/O: call them
//!   from a blocking task, never on the async executor.
//! - The judge lane is entered only after [`judge::JudgeCodes::verify`]
//!   accepted the presented code; an unknown code falls back to the
//!   public lane, it never errors in a way that reveals whether codes
//!   exist.
//! - [`budget::Ledger::reset`] is operator-only and must never be
//!   reachable from the public listener.

pub mod budget;
pub mod config;
pub mod geo;
pub mod judge;
pub mod money;
pub mod presets;
pub mod turnstile;
