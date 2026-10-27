// SPDX-License-Identifier: Apache-2.0
//! Recommendation derivation. This is a pure function of the evaluation
//! and the verification records — the single place where evidence becomes
//! a recommendation kind. Models never write this value.
//!
//! Rules, in priority order (schema 1.2, current):
//! 1. Any invalid proof anywhere in the retrieved set -> Blocked.
//! 2. Any contradicted fact among cited or contradicting evidence ->
//!    Contested, even when the Evaluator abstained: abstention must not
//!    hide a contradiction the Evaluator itself recorded.
//! 3. Evaluator abstained -> Abstained.
//! 4. Any unresolved challenge -> Contested.
//! 5. Any uncorroborated claim, any corroborated claim citing no
//!    evidence of its own, any cited supporting evidence whose integrity
//!    is not Valid (unavailable / error / unchecked), or no checked
//!    evidence at all -> InsufficientEvidence.
//! 6. Otherwise -> Supported.
//!
//! Schema 1.1 ranked abstention (3) above every contradiction (2); runs
//! recorded under 1.1 are re-derived with that order on replay, so old
//! logs keep replaying and stay tamper-checked ([`PolicyVersion`]).
//!
//! Note rule 5: a Valid proof with uncorroborated content still lands in
//! InsufficientEvidence. Integrity never upgrades factuality.

use consensus_domain::*;
use std::collections::HashSet;

/// Which rule order applies. Replay re-derives a recorded recommendation
/// with the rules it was issued under, selected by event schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyVersion {
    /// Schema 1.1: abstention outranks every contradiction.
    V1_1,
    /// Schema 1.2: a recorded contradiction outranks abstention.
    V1_2,
}

impl PolicyVersion {
    pub const CURRENT: Self = Self::V1_2;

    /// Rules for a recorded schema version. `None` for pre-1.1 records,
    /// which predate replay recomputation and are not re-derived.
    pub fn for_schema(version: &str) -> Option<Self> {
        match version {
            "1.1" => Some(Self::V1_1),
            "1.2" => Some(Self::V1_2),
            _ => None,
        }
    }
}

/// Derives the recommendation under the current rules.
pub fn derive_recommendation(
    evaluation: &Evaluation,
    verifications: &[EvidenceVerification],
) -> RecommendationKind {
    derive_recommendation_with(PolicyVersion::CURRENT, evaluation, verifications)
}

pub fn derive_recommendation_with(
    version: PolicyVersion,
    evaluation: &Evaluation,
    verifications: &[EvidenceVerification],
) -> RecommendationKind {
    let relevant: HashSet<&EvidenceId> = evaluation
        .assessments
        .iter()
        .flat_map(|a| a.relevant_evidence.iter())
        .collect();
    // Contradictions count even when the evaluator did not cite the
    // record as supporting evidence: omission must not hide them.
    let considered: HashSet<&EvidenceId> = evaluation
        .assessments
        .iter()
        .flat_map(|a| {
            a.relevant_evidence
                .iter()
                .chain(a.contradicting_evidence.iter())
        })
        .collect();

    // A failed integrity proof anywhere in the retrieved set is a
    // corruption signal about the evidence pipeline itself; it blocks
    // regardless of how the evaluator chose to cite records.
    if verifications
        .iter()
        .any(|v| v.integrity_status == IntegrityStatus::Invalid)
    {
        return RecommendationKind::Blocked;
    }
    let contradicted = verifications
        .iter()
        .filter(|v| considered.contains(&v.evidence_id))
        .any(|v| v.factual_status == FactualStatus::Contradicted);
    if contradicted && version == PolicyVersion::V1_2 {
        return RecommendationKind::Contested;
    }
    if evaluation.abstain {
        return RecommendationKind::Abstained;
    }
    if contradicted || !evaluation.unresolved_items.is_empty() {
        return RecommendationKind::Contested;
    }
    let any_uncorroborated = evaluation.assessments.iter().any(|a| !a.corroborated);
    // Corroboration is per-claim: a corroborated claim citing no evidence
    // of its own cannot ride on another claim's valid proof.
    let corroborated_without_evidence = evaluation
        .assessments
        .iter()
        .any(|a| a.corroborated && a.relevant_evidence.is_empty());
    // Every cited evidence record must actually have been retrieved and
    // integrity-checked as Valid. Cited-but-unchecked evidence (offline
    // backend, hallucinated reference) can never support a decision.
    let all_relevant_valid = relevant.iter().all(|id| {
        verifications
            .iter()
            .any(|v| &v.evidence_id == *id && v.integrity_status == IntegrityStatus::Valid)
    });
    if evaluation.assessments.is_empty()
        || any_uncorroborated
        || corroborated_without_evidence
        || !all_relevant_valid
        || relevant.is_empty()
    {
        return RecommendationKind::InsufficientEvidence;
    }
    RecommendationKind::Supported
}

/// Deterministic, human-readable rationale for receipts and the console.
pub fn rationale_for(
    kind: RecommendationKind,
    evaluation: &Evaluation,
    verifications: &[EvidenceVerification],
) -> String {
    let corroborated = evaluation
        .assessments
        .iter()
        .filter(|a| a.corroborated)
        .count();
    let total = evaluation.assessments.len();
    let valid = verifications
        .iter()
        .filter(|v| v.integrity_status == IntegrityStatus::Valid)
        .count();
    let invalid = verifications
        .iter()
        .filter(|v| v.integrity_status == IntegrityStatus::Invalid)
        .count();
    let unavailable = verifications
        .iter()
        .filter(|v| {
            matches!(
                v.integrity_status,
                IntegrityStatus::Unavailable | IntegrityStatus::VerificationError
            )
        })
        .count();
    format!(
        "{kind:?}: {corroborated}/{total} claims corroborated; proofs: {valid} valid, {invalid} invalid, {unavailable} unavailable/error; {} unresolved challenges.",
        evaluation.unresolved_items.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::{assessment, evaluation};

    fn verification(
        id: &str,
        integrity: IntegrityStatus,
        factual: FactualStatus,
    ) -> EvidenceVerification {
        EvidenceVerification {
            schema_version: SCHEMA_VERSION.into(),
            evidence_id: EvidenceId::new(id),
            integrity_status: integrity,
            factual_status: factual,
            relevance_status: RelevanceStatus::Relevant,
            verifier: Some("test".into()),
            verified_anchor: Some("a1".into()),
            detail: None,
            checked_at: "2026-10-26T00:00:00Z".into(),
        }
    }

    #[test]
    fn supported_requires_corroborated_claims_and_valid_proofs() {
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &[])], &[], false);
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Corroborated,
        )];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Supported);
    }

    #[test]
    fn valid_proof_does_not_imply_corroboration() {
        // The record is bit-for-bit intact (proof valid) but its content
        // does not corroborate the claim: never Supported.
        let e = evaluation(vec![assessment("c1", false, &["ev1"], &[])], &[], false);
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Uncorroborated,
        )];
        assert_eq!(
            derive_recommendation(&e, &v),
            RecommendationKind::InsufficientEvidence
        );
    }

    #[test]
    fn invalid_proof_blocks_even_corroborated_claims() {
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &[])], &[], false);
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Invalid,
            FactualStatus::Corroborated,
        )];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Blocked);
    }

    #[test]
    fn unavailable_proof_caps_at_insufficient_evidence() {
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &[])], &[], false);
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Unavailable,
            FactualStatus::Corroborated,
        )];
        assert_eq!(
            derive_recommendation(&e, &v),
            RecommendationKind::InsufficientEvidence
        );
    }

    #[test]
    fn contradicted_or_unresolved_is_contested() {
        let e = evaluation(
            vec![assessment("c1", false, &["ev1"], &["ev1"])],
            &[],
            false,
        );
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Contradicted,
        )];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Contested);

        let e2 = evaluation(
            vec![assessment("c1", true, &["ev1"], &[])],
            &["chal-1"],
            false,
        );
        let v2 = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Corroborated,
        )];
        assert_eq!(
            derive_recommendation(&e2, &v2),
            RecommendationKind::Contested
        );
    }

    #[test]
    fn abstention_wins_over_partial_support() {
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &[])], &[], true);
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Corroborated,
        )];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Abstained);
    }

    #[test]
    fn recorded_contradiction_outranks_abstention() {
        // Live S02 (2026-10-26): the Evaluator listed the stored cap as
        // contradicting a 10-attempt claim, then abstained, and the 1.1
        // order reported `abstained`, hiding the contradiction.
        let e = evaluation(vec![assessment("c1", false, &[], &["ev1"])], &[], true);
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Contradicted,
        )];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Contested);
        // Runs recorded under 1.1 keep their original derivation.
        assert_eq!(
            derive_recommendation_with(PolicyVersion::V1_1, &e, &v),
            RecommendationKind::Abstained
        );
        // An invalid proof still outranks both.
        let mut blocked = v.clone();
        blocked.push(verification(
            "ev2",
            IntegrityStatus::Invalid,
            FactualStatus::NotAssessed,
        ));
        assert_eq!(
            derive_recommendation(&e, &blocked),
            RecommendationKind::Blocked
        );
    }

    #[test]
    fn abstention_with_open_items_but_no_contradiction_stays_abstained() {
        // Unresolved items alone do not outrank abstention (unchanged);
        // the orchestrator may answer them with a revision instead.
        let e = evaluation(vec![assessment("c1", false, &[], &[])], &["i1"], true);
        assert_eq!(
            derive_recommendation(&e, &[]),
            RecommendationKind::Abstained
        );
    }

    #[test]
    fn policy_version_follows_recorded_schema() {
        assert_eq!(PolicyVersion::for_schema("1.1"), Some(PolicyVersion::V1_1));
        assert_eq!(
            PolicyVersion::for_schema(SCHEMA_VERSION),
            Some(PolicyVersion::CURRENT)
        );
        assert_eq!(PolicyVersion::for_schema("1.0"), None);
    }

    #[test]
    fn contradicting_evidence_prevents_supported_even_if_not_relevant() {
        // Audit finding 1: a contradiction recorded outside
        // `relevant_evidence` must still block Supported.
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &["ev2"])], &[], false);
        let v = vec![
            verification("ev1", IntegrityStatus::Valid, FactualStatus::Corroborated),
            verification("ev2", IntegrityStatus::Valid, FactualStatus::Contradicted),
        ];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Contested);
    }

    #[test]
    fn invalid_proof_anywhere_in_retrieved_evidence_blocks() {
        // Audit finding 1b: corruption signals must not be ignorable by
        // simply not citing the corrupted record as relevant.
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &[])], &[], false);
        let v = vec![
            verification("ev1", IntegrityStatus::Valid, FactualStatus::Corroborated),
            verification("evX", IntegrityStatus::Invalid, FactualStatus::NotAssessed),
        ];
        assert_eq!(derive_recommendation(&e, &v), RecommendationKind::Blocked);
    }

    #[test]
    fn corroborated_claim_without_own_evidence_is_not_supported() {
        // Audit finding 2: corroboration is per-claim; a claim citing no
        // evidence cannot ride on another claim's valid proof.
        let e = evaluation(
            vec![
                assessment("c1", true, &["ev1"], &[]),
                assessment("c2", true, &[], &[]),
            ],
            &[],
            false,
        );
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Corroborated,
        )];
        assert_eq!(
            derive_recommendation(&e, &v),
            RecommendationKind::InsufficientEvidence
        );
    }

    #[test]
    fn no_checked_evidence_is_never_supported() {
        let e = evaluation(vec![assessment("c1", true, &["ev1"], &[])], &[], false);
        assert_eq!(
            derive_recommendation(&e, &[]),
            RecommendationKind::InsufficientEvidence
        );
    }
}
