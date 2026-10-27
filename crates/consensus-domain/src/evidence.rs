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
}
