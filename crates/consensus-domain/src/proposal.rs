// SPDX-License-Identifier: Apache-2.0
//! Model-produced documents: proposal, challenge, evaluation, revision.
//!
//! None of these types carries an outcome, vote, or authorization field.
//! Models supply claims and concise justifications only — never hidden
//! reasoning traces, never the run's authoritative result.

use crate::ids::{ChallengeId, ClaimId, EvidenceId, ProposalId, RunId};
use crate::AgentRole;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One explicit, checkable assertion inside a proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Claim {
    pub id: ClaimId,
    pub statement: String,
    /// Evidence the author believes supports this claim (may be empty,
    /// which the evaluator treats as unsupported until retrieved).
    pub evidence_refs: Vec<EvidenceId>,
}

/// Proposer output: a plan plus the claims, assumptions, and
/// uncertainties it rests on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Proposal {
    pub schema_version: String,
    pub id: ProposalId,
    pub run_id: RunId,
    pub author_role: AgentRole,
    pub summary: String,
    pub plan_steps: Vec<String>,
    pub claims: Vec<Claim>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
    pub created_at: String,
}

/// A revision is a new proposal document that supersedes an earlier one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Revision {
    pub schema_version: String,
    pub proposal: Proposal,
    pub supersedes: ProposalId,
    /// Which challenge ITEMS this revision claims to address, by item id.
    #[serde(default, alias = "addressed_challenges")]
    pub addressed_items: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeKind {
    ContradictoryEvidence,
    MissingTest,
    UnsupportedClaim,
    RegressionRisk,
    SecurityRisk,
    ConstraintConflict,
}

/// One specific objection raised by the Critic against a claim or plan.
/// Items carry stable ids so resolution is tracked per objection, not
/// per challenge document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChallengeItem {
    #[serde(default)]
    pub id: String,
    pub kind: ChallengeKind,
    /// Claim under challenge, when the objection targets one claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<ClaimId>,
    pub description: String,
    /// Evidence the critic believes contradicts or constrains the claim.
    pub evidence_refs: Vec<EvidenceId>,
}

/// Critic output. The critic's job is to find problems, not to agree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Challenge {
    pub schema_version: String,
    pub id: ChallengeId,
    pub run_id: RunId,
    pub target_proposal: ProposalId,
    pub items: Vec<ChallengeItem>,
    pub created_at: String,
}

/// Evaluator output: per-claim sufficiency judgment. Factual and relevance
/// judgments live here; cryptographic integrity is reported separately in
/// `EvidenceVerification` and is never inferred from this document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimAssessment {
    pub claim_id: ClaimId,
    pub corroborated: bool,
    pub relevant_evidence: Vec<EvidenceId>,
    /// Evidence the evaluator judges to contradict this claim.
    #[serde(default)]
    pub contradicting_evidence: Vec<EvidenceId>,
    pub note: String,
}

// The Meta adapter's strict draft schema reuses this enum, and schemars
// copies doc comments into it: keep the doc model-facing, and keep the
// variants undocumented (variant docs turn the plain string `enum` into a
// `oneOf`).
/// Evaluator judgment on one Critic objection (schema 1.3): `resolved`
/// when the proposal as written no longer has the problem or the
/// retrieved evidence shows the objection is mistaken; `unresolved` when
/// it still applies, so it stays open into the next round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Resolved,
    Unresolved,
}

/// One explicit verdict on one objection, by item id (schema 1.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ItemVerdict {
    pub item_id: String,
    pub status: ItemStatus,
    /// Why. Required (non-empty) for a resolved verdict: an objection is
    /// never closed without saying what settled it.
    pub reason: String,
}

/// A contradiction recorded in an earlier round that this evaluation
/// judges no longer applies, with the reason (schema 1.3). Silence never
/// clears a contradiction; only an entry here does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClearedContradiction {
    pub evidence_id: EvidenceId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evaluation {
    pub schema_version: String,
    pub run_id: RunId,
    pub target_proposal: ProposalId,
    pub assessments: Vec<ClaimAssessment>,
    /// Challenge ITEM ids the evaluator judges still unresolved. From
    /// schema 1.3 exactly the ids of the `unresolved` verdicts.
    #[serde(default, alias = "unresolved_challenges")]
    pub unresolved_items: Vec<String>,
    /// Schema 1.3: one verdict for every objection under evaluation, the
    /// ones still open from earlier rounds and this round's. Empty on
    /// earlier records, which judged this round's items only, implicitly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_verdicts: Vec<ItemVerdict>,
    /// Schema 1.3: contradictions carried from earlier rounds that this
    /// evaluation clears, each with its reason.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleared_contradictions: Vec<ClearedContradiction>,
    /// True when the evaluator cannot reach a supported judgment and the
    /// correct behavior is to abstain rather than guess.
    pub abstain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abstain_reason: Option<String>,
    pub created_at: String,
}
