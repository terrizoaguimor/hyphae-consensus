//! Recommendation derivation. This is a pure function of the evaluation
//! and the verification records — the single place where evidence becomes
//! a recommendation kind. Models never write this value.
//!
//! Rules, in priority order:
//! 1. Any invalid proof anywhere in the retrieved set -> Blocked.
//! 2. Evaluator abstained -> Abstained.
//! 3. Any contradicted fact among cited or contradicting evidence, or
//!    any unresolved challenge -> Contested.
//! 4. Any uncorroborated claim, any corroborated claim citing no
//!    evidence of its own, any cited supporting evidence whose integrity
//!    is not Valid (unavailable / error / unchecked), or no checked
//!    evidence at all -> InsufficientEvidence.
//! 5. Otherwise -> Supported.
//!
//! Note rule 4: a Valid proof with uncorroborated content still lands in
//! InsufficientEvidence. Integrity never upgrades factuality.

use consensus_domain::*;
use std::collections::HashSet;

pub fn derive_recommendation(
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
    if evaluation.abstain {
        return RecommendationKind::Abstained;
    }
    if verifications
        .iter()
        .filter(|v| considered.contains(&v.evidence_id))
        .any(|v| v.factual_status == FactualStatus::Contradicted)
        || !evaluation.unresolved_items.is_empty()
    {
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
