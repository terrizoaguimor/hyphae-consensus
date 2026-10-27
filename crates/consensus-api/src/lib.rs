//! Adapters and operator surface for Hyphae Consensus.
//!
//! - [`meta`]: Meta Model API client implementing the runtime's
//!   `ModelClient` port with schema-constrained structured output.
//! - [`hyphae`]: Hyphae Agent Memory MCP client (read-only profile) and
//!   offline proof verifier implementing `MemoryClient`/`ProofVerifier`.
//! - [`server`]: Axum HTTP/SSE operator console.
//! - [`config`]: environment-driven configuration. Secrets are read from
//!   the environment at process start and never logged or persisted.

pub mod config;
pub mod hyphae;
pub mod meta;
pub mod server;
