//! Typed, versioned contracts for Hyphae Consensus.
//!
//! This crate contains data definitions only. It performs no I/O, holds no
//! state, and makes no decisions. The runtime crate owns all state
//! transitions; model-produced payloads in this crate deliberately contain
//! no outcome or authorization fields, so a model cannot set the
//! authoritative result of a run.

pub mod events;
pub mod evidence;
pub mod ids;
pub mod proposal;
pub mod receipt;
pub mod run;

pub use events::*;
pub use evidence::*;
pub use ids::*;
pub use proposal::*;
pub use receipt::*;
pub use run::*;

/// Schema version stamped on every serialized contract in this crate.
/// 1.1: per-item challenge ids, event hash chain, proof_error field.
pub const SCHEMA_VERSION: &str = "1.1";

/// Versions the event log can still read (additive/aliased changes only).
pub const READABLE_SCHEMA_VERSIONS: [&str; 2] = ["1.0", "1.1"];

/// The three logical agent roles. Roles may share one underlying model;
/// they differ in instructions, context, and tool permissions.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Proposer,
    Critic,
    EvidenceEvaluator,
}

/// Who performed an action recorded in the event log. `Human` is the only
/// actor kind allowed to authorize a run.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    Agent { role: AgentRole },
    Runtime,
    Human { id: String },
}
