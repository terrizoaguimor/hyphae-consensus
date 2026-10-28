// SPDX-License-Identifier: Apache-2.0
//! Adapters and operator surface for Hyphae Consensus.
//!
//! - [`meta`]: Meta Model API client implementing the runtime's
//!   `ModelClient` port with schema-constrained structured output.
//! - [`hyphae`]: Hyphae Agent Memory MCP client (read-only profile) and
//!   offline proof verifier implementing `MemoryClient`/`ProofVerifier`.
//! - [`server`]: Axum HTTP/SSE operator console.
//! - [`live`]: admission controls for the public live mode (budget ledger,
//!   Turnstile verification, presets, geographic policy).
//! - [`config`]: environment-driven configuration. Secrets are read from
//!   the environment at process start and never logged or persisted.

pub mod config;
pub mod hyphae;
pub mod live;
pub mod meta;
pub mod server;
pub mod snapshot;
