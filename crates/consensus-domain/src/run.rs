// SPDX-License-Identifier: Apache-2.0
//! Run lifecycle states, recommendation, and authorization.
//!
//! Recommendation and authorization are separate axes by design:
//! the runtime derives a `Recommendation` from evaluated evidence, and only
//! a human can move `AuthorizationState` away from `Pending`.

use crate::ids::{ProjectId, RunId, TaskId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A user-defined problem submitted for multi-agent deliberation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Task {
    pub schema_version: String,
    pub id: TaskId,
    pub run_id: RunId,
    pub project_id: ProjectId,
    pub title: String,
    pub description: String,
    /// RFC 3339 timestamp supplied by the runtime clock.
    pub created_at: String,
    /// Schema 1.3: the mode the run was declared with when it was
    /// created. A declaration, not proof: what actually happened is read
    /// from the recorded invocations and verifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<TaskMode>,
}

/// How a run was declared at creation (schema 1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TaskMode {
    pub model: ModelMode,
    pub evidence: EvidenceMode,
    pub origin: RunOrigin,
    /// Curated preset the task came from, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelMode {
    /// Scripted test doubles; no model is called.
    Scripted,
    /// The Meta Model API.
    Meta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceMode {
    /// Seeded in-memory records with scripted proof outcomes.
    Simulated,
    /// No memory backend: retrieval fails and the run degrades.
    Offline,
    /// A sealed, pre-captured Hyphae snapshot.
    SealedSnapshot,
    /// A running Hyphae Agent Memory deployment.
    LiveHyphae,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunOrigin {
    /// The command-line demo.
    Cli,
    /// The operator console.
    Operator,
    /// The public portal.
    Public,
}

/// Deterministic lifecycle of one run. Transitions are owned and validated
/// by the runtime state machine; see `docs/adr/0001-m1-architecture.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Created,
    Proposed,
    EvidenceReview,
    Evaluated,
    Revised,
    Abstained,
    AwaitingHumanApproval,
    Approved,
    Rejected,
    Failed,
}

impl RunState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunState::Approved | RunState::Rejected | RunState::Failed
        )
    }
}

/// Runtime-derived assessment of a run. Never written by a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationKind {
    Supported,
    Contested,
    InsufficientEvidence,
    Abstained,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Recommendation {
    pub schema_version: String,
    pub kind: RecommendationKind,
    /// Human-readable, concise justification citing claim/evidence ids.
    pub rationale: String,
    /// Assumptions that remain unresolved at decision time.
    pub unresolved_assumptions: Vec<String>,
    /// Schema 1.3: the final proposal's own uncertainties, carried to
    /// the human next to its assumptions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_uncertainties: Vec<String>,
    pub issued_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationState {
    Pending,
    Approved,
    Rejected,
}

/// A human decision recorded against a run. Recording an approval performs
/// no external action in M1-M3; the executor is deferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HumanAuthorization {
    pub schema_version: String,
    pub run_id: RunId,
    pub authorized_by: String,
    pub approved: bool,
    pub decided_at: String,
    /// Caller-supplied key making retries of the same decision no-ops.
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Schema 1.3: the authority the decision was taken under. Absent on
    /// earlier records, which read as unspecified, never as operator
    /// authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<Authority>,
}

/// Under whose authority a human decision was recorded (schema 1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    /// The operator of this deployment.
    Operator,
    /// A public visitor deciding on their own simulated run: the record
    /// carries no operational authority.
    PublicDemo,
}
