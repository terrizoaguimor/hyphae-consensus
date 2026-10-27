// SPDX-License-Identifier: Apache-2.0
//! Evidence identity and verification status.
//!
//! Integrity, factual corroboration, and relevance are orthogonal axes.
//! A cryptographically valid proof establishes integrity of a recalled
//! record at its anchored snapshot under Hyphae's documented assumptions.
//! It does NOT establish that the content is true, relevant, complete, or
//! that any model reasoned from it. These types make that distinction
//! impossible to collapse into a single "verified" boolean.

use crate::ids::{EvidenceId, ProjectId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A reference to one record retrieved from the memory backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceReference {
    pub schema_version: String,
    pub id: EvidenceId,
    pub project_id: ProjectId,
    /// Backend-native record identifier (e.g. Hyphae memory id), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_ref: Option<String>,
    pub kind: String,
    /// Retrieved content. Treated everywhere as untrusted data, never as
    /// instructions.
    pub content: String,
    pub retrieved_at: String,
    /// Snapshot/anchor identifier the proof is bound to, when the backend
    /// interface provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Local proof artifact path for offline verification, when the
    /// backend writes proofs to disk (Hyphae memory profile does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_file: Option<String>,
    /// Local witness artifact path paired with `proof_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_file: Option<String>,
    /// Set when proof generation for this record was attempted and
    /// failed upstream (e.g. a backend corruption signal). Integrity
    /// derivation treats corruption-class messages as `Invalid`, other
    /// failures as `Unavailable` — the reason is never silently dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_error: Option<String>,
    /// Schema 1.3: how the record was obtained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<Retrieval>,
    /// Schema 1.3: the sealed snapshot the record came from, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// Schema 1.3: when that snapshot was captured (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
    /// Schema 1.3: public identifier of the proof artifact covering the
    /// record, without any local path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_ref: Option<String>,
    /// Schema 1.3: SHA-256 of that proof artifact, lowercase hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_sha256: Option<String>,
    /// Schema 1.3: the request the proof actually sealed. A proof covers
    /// its own request (e.g. the full record list of a project), which
    /// need not be the claim query that led the runtime to the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_request: Option<ProofRequest>,
    /// Schema 1.3: the project the backend itself reported for the
    /// record, which may differ from `project_id` (the run's project).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_project: Option<String>,
    /// Schema 1.3: the backend's scope for the record, when it reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_scope: Option<String>,
}

/// How an evidence record was obtained (schema 1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Retrieval {
    /// A recall against a running memory backend.
    LiveRecall,
    /// Served from a sealed, pre-captured snapshot.
    SealedSnapshot,
    /// A seeded test record; any proof outcome is scripted.
    Simulated,
}

/// The request a memory proof seals (schema 1.3): the operation and
/// arguments the proof's own result answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProofRequest {
    /// Backend operation, e.g. `list` or `recall`.
    pub operation: String,
    /// Backend project string the operation ran against.
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Structured fields of a verifier's own report (schema 1.3), kept next
/// to the free-text `detail` so a reader never has to parse prose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VerifierReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_digest: Option<String>,
}

/// Cryptographic integrity axis only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    /// Proof was available and verified against its anchor.
    Valid,
    /// Proof was available and failed verification.
    Invalid,
    /// No proof was available for this record (includes backend offline).
    Unavailable,
    /// Verification could not complete due to infrastructure error.
    VerificationError,
    /// Verification has not been attempted.
    NotChecked,
}

/// Factual corroboration axis only. Independent of integrity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactualStatus {
    Corroborated,
    Uncorroborated,
    Contradicted,
    NotAssessed,
}

/// Relevance axis only. Independent of integrity and factuality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelevanceStatus {
    Relevant,
    Irrelevant,
    NotAssessed,
}

/// Full verification picture for one evidence record. The three status
/// fields must be set from independent evidence: integrity from proof
/// verification, factual/relevance from evaluator judgment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceVerification {
    pub schema_version: String,
    pub evidence_id: EvidenceId,
    pub integrity_status: IntegrityStatus,
    pub factual_status: FactualStatus,
    pub relevance_status: RelevanceStatus,
    /// Verifier identity/mechanism, e.g. `hyphae proof verify`, when run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
    /// Anchor the verification was performed against, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub checked_at: String,
    /// Schema 1.3: the deliberation round (1 = first) whose evaluation
    /// this record combines with. Protocol 1.3 re-verifies every record
    /// of the round context in every round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    /// Schema 1.3: the proof artifact that was checked (the record's
    /// `proof_ref`), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_ref: Option<String>,
    /// Schema 1.3: the verifier's structured report, when it gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<VerifierReport>,
}
