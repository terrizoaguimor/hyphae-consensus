// SPDX-License-Identifier: Apache-2.0
//! Adapters, public API and operator surface for Hyphae Consensus.
//!
//! - [`meta`]: Meta Model API client implementing the runtime's
//!   `ModelClient` port with schema-constrained structured output.
//! - [`hyphae`]: Hyphae Agent Memory MCP client (read-only profile) and
//!   offline proof verifier implementing `MemoryClient`/`ProofVerifier`.
//! - [`api`]: the `/api/v1` contract (types, errors, routers, handlers,
//!   frozen schemas and fixtures).
//! - [`runs`]: the run manager (admission, durable creation, background
//!   deliberation, cancel, decisions, shutdown).
//! - [`stream`]: per-run broadcast channels and the SSE stream.
//! - [`public_view`]: provenance axes and the views derived from a run's
//!   chained events.
//! - [`session`], [`limits`]: visitor sessions and per-client limits.
//! - [`showcase`], [`static_files`]: the research archive and the static
//!   web export with per-page CSP.
//! - [`server`]: `serve-public` and `serve-operator`.
//! - [`verify_cli`]: the read-only `verify` subcommand.
//! - [`ledger_cli`]: the operator's `ledger` subcommand (status, reset,
//!   release) on the live-mode USD ledger.
//! - [`guided`]: the five guided scenarios.
//! - [`live`]: admission controls for the public live mode (budget ledger,
//!   Turnstile verification, presets, geographic policy).
//! - [`config`], [`public_config`]: environment-driven configuration.
//!   Secrets are read from the environment at process start and never
//!   logged or persisted.

pub mod api;
pub mod config;
pub mod guided;
pub mod hyphae;
pub mod ledger_cli;
pub mod limits;
pub mod live;
pub mod meta;
pub mod public_config;
pub mod public_view;
pub mod runs;
pub mod server;
pub mod session;
pub mod showcase;
pub mod snapshot;
pub mod static_files;
pub mod stream;
pub mod verify_cli;
