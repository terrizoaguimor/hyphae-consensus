// SPDX-License-Identifier: Apache-2.0
//! Admission controls for the public live mode: the USD budget ledger,
//! Turnstile verification, curated presets and the geographic policy, and
//! the gate ([`gate`]) that applies them to a request. The library modules
//! call no model: they decide whether a visitor's request may spend the
//! operator's key at all; the gate is what the public route
//! (`POST /api/v1/live-runs`) hands the request to.
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
//! - [`gate`]: the admission itself ([`gate::LiveGate`]), the evidence
//!   factory seam for the sealed snapshot ([`gate::DeliberationFactory`])
//!   and the reservation's lifetime ([`gate::MeteredDeliberation`]).
//! - [`snapshot_factory`]: the production evidence factory
//!   ([`snapshot_factory::SnapshotFactory`]): the sealed snapshot client
//!   (ADR-0008) boot-verified from `CONSENSUS_SNAPSHOT_DIR` with live mode
//!   enabled, its served files re-checked at every admission and (cached)
//!   for `/ready`, a copy per run with a fresh verifier bound to the boot
//!   check; [`snapshot_factory::UnavailableEvidence`] when the snapshot
//!   does not boot (the portal stays up, live mode says why).
//!
//! How the pieces are wired (implemented; see each item's code):
//! - Check order per ABUSE-9 ([`gate::LiveGate`]): the route's
//!   owner-scoped idempotency lookup first, then the preset, geo
//!   ([`geo::GeoPolicy::decide`] with [`geo::Surface::Live`]), the 18+
//!   and territory acknowledgment, the evidence source's status (`503
//!   live_unavailable` while it is not ok, before any token is spent),
//!   Turnstile, the lane, then [`budget::Ledger::reserve`], and only then
//!   the model client, the deliberation (whose factory re-checks the
//!   evidence in full) and `create_task`. Nothing is spent before the
//!   reservation exists.
//! - The `idempotency_key` given to [`turnstile::Siteverify::verify`] is a
//!   fresh UUID on every call, never the request's `Idempotency-Key`:
//!   Cloudflare's idempotency is global and immediate, the route's replay
//!   is owner-scoped and recorded only after the start, so forwarding the
//!   header would let one solved challenge start a run per session or
//!   several at once. A token is single-use at Siteverify; a retry whose
//!   first attempt succeeded is answered by the route from the stored
//!   creation (no `verify`, no `reserve`), one whose first attempt died
//!   after Siteverify solves a new challenge.
//! - The [`budget::Reservation`] lives in a [`gate::ReservationHolder`]
//!   shared by the run's deliberation and its refund closure, and is
//!   released exactly once, always with three bounded retries (since
//!   [`budget::Ledger::release`] keeps the handle and its slot on `Err`):
//!   when `deliberate` returns, when the deliberation is dropped (the
//!   manager catches a panic and drops it afterwards; a task creation that
//!   fails drops it inside the start task), or by the route's refund for a
//!   start refused before the start task. A release that still fails is
//!   logged with its id; the reservation then keeps counting at its worst
//!   case until an operator reconciles it with
//!   [`budget::Ledger::release_by_id`] (operator console only).
//! - The Meta adapter gets [`budget::Ledger::meter`]
//!   ([`crate::meta::MetaModelClient::with_meter`]): one draw before every
//!   HTTP attempt, one settlement after it, a 2xx settled with
//!   [`budget::Usage::from_chat_usage`] (`completion_tokens` as reported,
//!   reasoning included) once the body is parsed. A draw refusal fails the
//!   run at its current stage with the code `budget_refused`; a settlement
//!   error leaves the attempt counted at its estimate and fails the call
//!   closed (`budget_unsettled`).
//! - `/capabilities` reads the ledger with
//!   [`budget::Ledger::snapshot_cached`] (five seconds of `max_age`, off
//!   the async workers) and publishes [`budget::Snapshot::public`] without
//!   the judge figures; when the ledger cannot be read the budget is
//!   absent (and `/ready` says `ledger_ok: false`), never a made-up
//!   figure. The kill-switch file is reported as a trip by both routes as
//!   soon as it exists (a metadata read), before the next admission
//!   records it. [`budget::Ledger::snapshot`] waits for the file lock and
//!   may write a trip record: operator-only. No response carries a
//!   [`budget::Snapshot`] wholesale or a [`budget::Refusal`]'s or
//!   [`budget::LedgerError`]'s `Display` text (upstream statuses, amounts,
//!   paths): the wire gets the fixed codes of
//!   [`crate::api::error::ErrorCode`] ([`gate::refusal_code`], the same
//!   strings as [`budget::Refusal::code`]) and
//!   [`budget::TripReason::as_str`], with fixed sentences.
//! - [`budget::BudgetConfig::max_concurrent`] slots are per process, and
//!   the run manager's live semaphore bounds the deliberations; a
//!   server-side limit keyed by client (/64) is still a follow-up.
//! - The ledger and the HTTP Siteverify client block on I/O: the gate runs
//!   the whole admission on a blocking task with owned data.
//! - The judge lane is entered only after [`judge::JudgeCodes::verify`]
//!   accepted the presented code; an unknown code falls back to the
//!   public lane, it never errors in a way that reveals whether codes
//!   exist. Judge reservations are bounded by the judge slice and by
//!   `CONSENSUS_LIVE_JUDGE_DAILY_USD` (default: the slice), so one day's
//!   exposure on the key is `daily + judge daily`; the owner can lower the
//!   judge daily cap without touching the public lane.
//! - [`budget::Ledger::reset`], [`budget::Ledger::release_by_id`] and
//!   [`budget::Ledger::snapshot`] are operator-only and are not reachable
//!   from the public listener.
//! - The portal-wide geographic block is applied to every public route by
//!   the API router (`CONSENSUS_GEO_*` are read whether or not live mode
//!   is enabled); the operator's own live runs do not go through the
//!   ledger yet (ADR-0007 follow-up).

pub mod budget;
pub mod config;
pub mod gate;
pub mod geo;
pub mod judge;
pub mod money;
pub mod presets;
pub mod snapshot_factory;
pub mod turnstile;
