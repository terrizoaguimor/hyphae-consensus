// SPDX-License-Identifier: Apache-2.0
//! Redacted, downloadable record of one completed run.
//!
//! [`RunReceipt`] is the original receipt. [`RunReceiptV2`] adds what the
//! first one leaves out: the policy trace of the final round, the
//! deliberation the human is deciding on (claims and their assessments,
//! open objections, assumptions, uncertainties, revisions), the round of
//! each verification, and the scope of the hash chain. Both are derived
//! from a run's events at read time; neither is ever stored in the log.

use crate::events::ModelInvocation;
use crate::evidence::EvidenceVerification;
use crate::ids::{ClaimId, EvidenceId, ProposalId, RunId};
use crate::proposal::{ChallengeKind, ClaimAssessment};
use crate::run::{AuthorizationState, Recommendation, RecommendationKind, RunState, Task};
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
    /// Hash of this run's last event at receipt time. It commits to every
    /// earlier record in the log file, so a later edit of any of them is
    /// detectable against this receipt; records appended after it, and the
    /// file's own head in a multi-run log, are not covered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_hash: Option<String>,
    pub generated_at: String,
    /// Plain-language limits of what this receipt proves. Always present.
    pub proof_limitations: String,
}

impl RunReceipt {
    pub const DEFAULT_LIMITATIONS: &'static str = "Integrity proofs, where valid, establish that a recalled record matches its anchored snapshot under the memory backend's documented assumptions. They do not establish that the content is true, relevant, or complete, or that a model reasoned from it.";
}

/// Policy rule order a [`PolicyTrace`] was derived under, serialized as
/// its version string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum PolicyRules {
    /// Schema 1.1: abstention outranks every contradiction.
    #[serde(rename = "1.1")]
    V1_1,
    /// Schema 1.2: a recorded contradiction outranks abstention.
    #[serde(rename = "1.2")]
    V1_2,
}

/// The policy rule that decided a recommendation kind. Listed in the 1.2
/// priority order; 1.1 ranks `abstention` above `recorded_contradiction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRule {
    /// An integrity proof failed somewhere in the retrieved set: `blocked`.
    InvalidProof,
    /// Cited or contradicting evidence is recorded as contradicted:
    /// `contested`.
    RecordedContradiction,
    /// The Evaluator abstained: `abstained`.
    Abstention,
    /// Challenge items remain unresolved: `contested`.
    UnresolvedItems,
    /// A claim, or the evidence it cites, falls short:
    /// `insufficient_evidence`.
    InsufficientEvidence,
    /// No check failed: `supported`.
    AllChecksPassed,
}

/// One policy check that keeps a run from `supported`, with the ids it
/// concerns. Ids are listed once each, in the order the evaluation and
/// verification records first name them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum TraceReason {
    /// Evidence whose integrity proof is `invalid`, cited or not.
    InvalidProofs { evidence_ids: Vec<EvidenceId> },
    /// Cited or contradicting evidence recorded as `contradicted`.
    ContradictedEvidence { evidence_ids: Vec<EvidenceId> },
    /// The Evaluator abstained, with its stated reason when it gave one.
    EvaluatorAbstained {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        abstain_reason: Option<String>,
    },
    /// Challenge item ids the evaluation lists as unresolved, as recorded.
    OpenItems { item_ids: Vec<String> },
    /// The evaluation assesses no claim at all.
    NoAssessments,
    /// Claims the evaluation does not judge corroborated.
    UncorroboratedClaims { claim_ids: Vec<ClaimId> },
    /// Claims judged corroborated that cite no evidence of their own.
    ClaimsWithoutOwnEvidence { claim_ids: Vec<ClaimId> },
    /// Cited supporting evidence with no `valid` integrity record
    /// (unavailable, error, unchecked, or never retrieved).
    CitedEvidenceNotValid { evidence_ids: Vec<EvidenceId> },
    /// No claim cites any supporting evidence, so nothing checked backs
    /// the run.
    NoCheckedEvidence,
}

/// The counts `rationale_for` reports, from the same inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TraceCounts {
    /// Claim assessments in the evaluation.
    pub claims: u64,
    /// Of those, the ones judged corroborated.
    pub corroborated: u64,
    pub proofs_valid: u64,
    pub proofs_invalid: u64,
    /// Integrity `unavailable` or `verification_error`.
    pub proofs_unavailable_or_error: u64,
    /// Entries in the evaluation's `unresolved_items`.
    pub unresolved_items: u64,
}

/// Why the policy derived the kind it did, from one evaluation and the
/// verification records of its round. `rule` is the rule that decided
/// `kind`; `reasons` lists every check that failed, in the priority
/// order of `version`, so its first entry belongs to `rule`. `reasons` is
/// empty exactly when the run is `supported`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyTrace {
    pub version: PolicyRules,
    pub kind: RecommendationKind,
    pub rule: PolicyRule,
    pub reasons: Vec<TraceReason>,
    pub counts: TraceCounts,
}

/// Receipt v2: everything a reader needs to see what a human is asked to
/// authorize, and on what grounds, derived from one run's events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunReceiptV2 {
    /// Always [`RunReceiptV2::FORMAT`].
    pub format: String,
    /// Schema version of the software that produced this receipt.
    pub schema_version: String,
    pub run_id: RunId,
    /// Schema version of the run's first record: the version it was
    /// deliberated under.
    pub run_schema_version: String,
    /// Protocol and policy rules of the run, e.g. `policy 1.2`, or
    /// `protocol 1.3 · policy rules 1.2` once carry-forward changed the
    /// policy's inputs.
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    pub final_state: RunState,
    /// Stage and reason of a failed run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReceiptFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<Recommendation>,
    /// The policy re-derived from the evaluation and verification records
    /// the recommendation was issued on. None when no recommendation was
    /// issued, or for pre-1.1 records, which predate re-derivation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_trace: Option<PolicyTrace>,
    pub deliberation: Deliberation,
    pub authorization: ReceiptAuthorization,
    /// Every model call recorded for the run, in order.
    pub model_invocations: Vec<ModelInvocation>,
    /// Every verification record, tagged with its round. The policy used
    /// the final round's records only.
    pub verifications: Vec<RoundVerification>,
    pub event_count: u64,
    pub chain: ReceiptChain,
    pub generated_at: String,
    /// Plain-language limits of what the proofs establish. Always present.
    pub proof_limitations: String,
}

impl RunReceiptV2 {
    pub const FORMAT: &'static str = "hyphae-consensus-run-receipt-v2";
}

/// Where and why a run failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptFailure {
    pub stage: String,
    pub reason: String,
}

/// The deliberation as it stood at the end of the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Deliberation {
    /// Evaluation rounds recorded. A round is one critique and its
    /// evaluation; each revision opens the next round.
    pub rounds: u32,
    /// The last proposal or revision submitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_proposal_id: Option<ProposalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_proposal_summary: Option<String>,
    /// The proposal the final evaluation judged. It differs from
    /// `final_proposal_id` only when a run ended between a revision and
    /// its evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluated_proposal_id: Option<ProposalId>,
    pub plan_steps: Vec<String>,
    /// The final proposal's claims, each with the final evaluation's
    /// assessment of it.
    pub claims: Vec<ReceiptClaim>,
    /// Final-evaluation assessments not attached to any claim above: a
    /// claim id the final proposal does not contain, a second assessment
    /// of the same claim, or every assessment when the final evaluation
    /// judged an earlier proposal. Shown, never dropped.
    pub unmatched_assessments: Vec<ClaimAssessment>,
    /// Items the final evaluation lists as unresolved.
    pub open_items: Vec<OpenItem>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
    /// The open items of kind `missing_test`.
    pub missing_tests: Vec<OpenItem>,
    pub revisions: Vec<ReceiptRevision>,
    /// Before protocol 1.3, each evaluation judged only the items of its
    /// own round's challenge. These are the items raised in earlier rounds
    /// that the final evaluation never listed: no later round judged them
    /// again. Always empty for protocol 1.3 and later, which carries every
    /// open item forward and requires a verdict on each in every round.
    pub earlier_round_objections_never_rejudged: Vec<UnrejudgedObjection>,
}

/// One claim of the final proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptClaim {
    pub id: ClaimId,
    pub statement: String,
    /// Evidence the proposer cited for the claim.
    pub evidence_refs: Vec<EvidenceId>,
    /// The final evaluation's assessment. None when it did not assess the
    /// claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<ReceiptAssessment>,
}

/// The Evaluator's judgment of one claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptAssessment {
    pub corroborated: bool,
    pub relevant_evidence: Vec<EvidenceId>,
    pub contradicting_evidence: Vec<EvidenceId>,
    pub note: String,
}

/// A challenge item the final evaluation lists as unresolved. Details are
/// None when no recorded challenge contains an item with this id (pre-1.1
/// evaluations listed challenge ids, not item ids).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpenItem {
    pub id: String,
    /// Round whose challenge raised the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raised_in_round: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ChallengeKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<ClaimId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A challenge item from an earlier round that no later round judged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnrejudgedObjection {
    /// Item id; empty on pre-1.1 records, which predate item ids.
    pub id: String,
    pub raised_in_round: u32,
    pub kind: ChallengeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<ClaimId>,
    pub description: String,
    /// Whether the evaluation of the round that raised it listed it as
    /// unresolved (pre-1.1: listed its challenge's id). False means that
    /// evaluation left it unlisted, which before protocol 1.3 counted as
    /// resolved without any stated reason.
    pub unresolved_in_its_round: bool,
}

/// One revision, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptRevision {
    /// The evaluation round this revision answers. The revised proposal is
    /// judged in the next round.
    pub after_round: u32,
    pub proposal_id: ProposalId,
    pub supersedes: ProposalId,
    pub summary: String,
    /// Item ids the revision says it addresses (a claim, not a verdict).
    pub addressed_items: Vec<String>,
}

/// The human decision, if one was recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptAuthorization {
    pub state: AuthorizationState,
    /// Authority the decision was recorded under: `unspecified` for every
    /// record before schema 1.3, which carried no authority and must never
    /// be read as operator authority. None while no decision is recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A verification record and the evaluation round it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoundVerification {
    /// 1-based evaluation round: the records after the k-th evaluation are
    /// round k. 0 for a record made before any evaluation, which no policy
    /// derivation reads.
    pub round: u32,
    #[serde(flatten)]
    pub verification: EvidenceVerification,
}

/// What a reader needs to verify the run's events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChainScope {
    /// The run's events are a complete chain from genesis on their own.
    Run,
    /// The run is a segment of a multi-run log: its records link to other
    /// runs' records, so verifying them needs the whole file.
    Log,
    /// Pre-1.1 records carry no hash chain.
    Unchained,
}

/// The hash chain behind a receipt. The chain is unsigned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptChain {
    pub scope: ChainScope,
    /// Hash of the run's last event (not the head of a multi-run file).
    /// None when that record is unchained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_hash: Option<String>,
    /// Sequences of the run's first and last records in their log.
    pub first_sequence: u64,
    pub last_sequence: u64,
    /// Plain-language statement of what the chain does and does not show.
    pub note: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON Schema of `T`, as `write_schemas` would emit it.
    fn schema<T: JsonSchema>() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(T)).unwrap()
    }

    #[test]
    fn receipt_v2_and_policy_trace_have_json_schemas() {
        let receipt = schema::<RunReceiptV2>();
        for field in ["policy_trace", "deliberation", "chain", "verifications"] {
            assert!(receipt["properties"][field].is_object(), "{field}");
        }
        // A round-tagged verification is one flat object.
        let round = &receipt["definitions"]["RoundVerification"]["properties"];
        assert!(round["round"].is_object() && round["evidence_id"].is_object());

        // Each trace reason is tagged by `reason`.
        let trace = schema::<PolicyTrace>();
        let reasons = trace["definitions"]["TraceReason"]["oneOf"]
            .as_array()
            .expect("one schema per reason");
        assert_eq!(reasons.len(), 9);
        let tag = serde_json::Value::from("reason");
        assert!(reasons
            .iter()
            .all(|r| r["required"].as_array().unwrap().contains(&tag)));
    }
}
