//! Redacted, downloadable record of one completed run.

use crate::events::ModelInvocation;
use crate::evidence::EvidenceVerification;
use crate::ids::RunId;
use crate::run::{AuthorizationState, Recommendation, RunState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunReceipt {
    pub schema_version: String,
    pub run_id: RunId,
    pub final_state: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<Recommendation>,
    pub authorization: AuthorizationState,
    /// Every model call made during the run, with exact model identity.
    pub model_invocations: Vec<ModelInvocation>,
    /// Development-model disclaimer is documentation, not data; receipts
    /// record application inference identity only.
    pub verifications: Vec<EvidenceVerification>,
    pub event_count: u64,
    /// Hash-chain head (last event's hash) at receipt time, so any later
    /// edit of the log is detectable against this receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_hash: Option<String>,
    pub generated_at: String,
    /// Plain-language limits of what this receipt proves. Always present.
    pub proof_limitations: String,
}

impl RunReceipt {
    pub const DEFAULT_LIMITATIONS: &'static str = "Integrity proofs, where valid, establish that a recalled record matches its anchored snapshot under the memory backend's documented assumptions. They do not establish that the content is true, relevant, or complete, or that a model reasoned from it.";
}
