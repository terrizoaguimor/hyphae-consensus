// SPDX-License-Identifier: Apache-2.0
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
/// 1.2: policy order (a recorded contradiction outranks abstention) and a
/// revision may follow an abstention that lists unresolved items. Record
/// shapes are unchanged; replay re-derives each run with its own rules.
///
/// 1.3 (protocol 1.3; the policy rules stay those of 1.2): the
/// fail-closed evaluation contract and cross-round carry-forward. The
/// runtime checks every 1.3 evaluation against the run's own record of
/// open objections, retrieved records and earlier contradictions, both
/// before recording it and on replay (`consensus_runtime::contract`);
/// replay also re-derives each 1.3 verification record from the recorded
/// evaluation, and refuses a run whose records go back to an earlier
/// schema version. Carry-forward changes the inputs of the 1.2 rules, so
/// the same deliberation can now derive `contested` where 1.2 derived
/// `supported`.
///
/// Every 1.3 change is additive. Each field added to an existing type
/// reachable from [`ConsensusEvent`] is an `Option` or `Vec` with
/// `#[serde(default, skip_serializing_if = ...)]`, so a 1.1-1.2 (chained)
/// record parses and re-serializes byte for byte and its chain hash still
/// verifies (1.0 records, which predate the chain, still parse); no
/// `HashMap`/`HashSet` appears in a serialized type. The 1.3 inventory:
/// - [`Evaluation::item_verdicts`] ([`ItemVerdict`], [`ItemStatus`]) and
///   [`Evaluation::cleared_contradictions`] ([`ClearedContradiction`]);
/// - [`Recommendation::unresolved_uncertainties`];
/// - [`HumanAuthorization::authority`] ([`Authority`]);
/// - [`Task::mode`] ([`TaskMode`]: [`ModelMode`], [`EvidenceMode`],
///   [`RunOrigin`], preset);
/// - [`EvidenceReference`]: `retrieval` ([`Retrieval`]), `snapshot_id`,
///   `captured_at`, `proof_ref`, `proof_sha256`, `proof_request`
///   ([`ProofRequest`]), `backend_project`, `backend_scope`;
/// - [`EvidenceVerification`]: `round`, `proof_ref`, `report`
///   ([`VerifierReport`]);
/// - [`ModelInvocation`]: `finish_reason`, `reasoning_tokens`,
///   `max_completion_tokens`, `schema_name`, `schema_sha256`,
///   `prompt_sha256`, `reasoning_effort`, `safety_identifier_sent`,
///   `attempts`;
/// - [`EventPayload::RunFailed`]: `code` and `invocation` (a call the
///   provider answered is still recorded when its output was refused, by
///   the adapter or by the runtime's contract);
///   [`EventPayload::EvidenceUnavailable`]: `code`;
/// - a new event, [`EventPayload::RecallProven`]: the request a proof
///   actually sealed, the ids it covers (none for an empty recall), the
///   proof reference and anchor. Accepted wherever the other evidence
///   events are; nothing emits it yet.
pub const SCHEMA_VERSION: &str = "1.3";

/// Versions the event log can still read (additive/aliased changes only).
pub const READABLE_SCHEMA_VERSIONS: [&str; 4] = ["1.0", "1.1", "1.2", "1.3"];

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
