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

pub mod budget;
pub mod config;
pub mod geo;
pub mod judge;
pub mod money;
pub mod presets;
pub mod turnstile;
