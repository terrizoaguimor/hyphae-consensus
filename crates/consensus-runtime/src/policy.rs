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
//!
//! [`explain`] is the one implementation of these rules. It returns a
//! [`PolicyTrace`]: the kind, the rule that decided it, every check that
//! failed with the ids involved, and the counts [`rationale_for`]
//! reports. [`derive_recommendation_with`] is its `kind`.

use consensus_domain::*;
use std::collections::HashSet;

pub use consensus_domain::{PolicyRule, PolicyRules, PolicyTrace, TraceCounts, TraceReason};

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

/// The kind [`explain`] derives under the rules of `version`.
pub fn derive_recommendation_with(
    version: PolicyVersion,
    evaluation: &Evaluation,
    verifications: &[EvidenceVerification],
) -> RecommendationKind {
    explain(version, evaluation, verifications).kind
}

impl From<PolicyVersion> for PolicyRules {
    fn from(version: PolicyVersion) -> Self {
        match version {
            PolicyVersion::V1_1 => PolicyRules::V1_1,
            PolicyVersion::V1_2 => PolicyRules::V1_2,
        }
    }
}

/// Derives the recommendation under the rules of `version` and explains
/// it: the deciding rule, every failed check with the ids involved (in
/// the priority order of `version`, so the first one decided `kind`), and
/// the counts [`rationale_for`] reports. A pure function of its inputs,
/// like the rest of the policy.
pub fn explain(
    version: PolicyVersion,
    evaluation: &Evaluation,
    verifications: &[EvidenceVerification],
) -> PolicyTrace {
    let assessments = &evaluation.assessments;
    // Cited supporting evidence, once each, in citation order.
    let relevant: Vec<EvidenceId> =
        distinct(assessments.iter().flat_map(|a| a.relevant_evidence.iter()));
    // Contradictions count even when the evaluator did not cite the
    // record as supporting evidence: omission must not hide them.
    let considered: HashSet<&EvidenceId> = assessments
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
    let invalid = distinct(
        verifications
            .iter()
            .filter(|v| v.integrity_status == IntegrityStatus::Invalid)
            .map(|v| &v.evidence_id),
    );
    let contradicted = distinct(
        verifications
            .iter()
            .filter(|v| considered.contains(&v.evidence_id))
            .filter(|v| v.factual_status == FactualStatus::Contradicted)
            .map(|v| &v.evidence_id),
    );
    let invalid = (!invalid.is_empty()).then_some(TraceReason::InvalidProofs {
        evidence_ids: invalid,
    });
    let contradiction = (!contradicted.is_empty()).then_some(TraceReason::ContradictedEvidence {
        evidence_ids: contradicted,
    });
    let abstention = evaluation.abstain.then(|| TraceReason::EvaluatorAbstained {
        abstain_reason: evaluation.abstain_reason.clone(),
    });
    let open_items = (!evaluation.unresolved_items.is_empty()).then(|| TraceReason::OpenItems {
        item_ids: evaluation.unresolved_items.clone(),
    });
    // Schema 1.1 ranked abstention above every contradiction; 1.2 ranks
    // a recorded contradiction first, so abstaining cannot hide it.
    let ranked = match version {
        PolicyVersion::V1_1 => [invalid, abstention, contradiction, open_items],
        PolicyVersion::V1_2 => [invalid, contradiction, abstention, open_items],
    };
    let mut reasons: Vec<TraceReason> = ranked.into_iter().flatten().collect();

    if assessments.is_empty() {
        reasons.push(TraceReason::NoAssessments);
    }
    let uncorroborated = distinct(
        assessments
            .iter()
            .filter(|a| !a.corroborated)
            .map(|a| &a.claim_id),
    );
    if !uncorroborated.is_empty() {
        reasons.push(TraceReason::UncorroboratedClaims {
            claim_ids: uncorroborated,
        });
    }
    // Corroboration is per-claim: a corroborated claim citing no evidence
    // of its own cannot ride on another claim's valid proof.
    let without_evidence = distinct(
        assessments
            .iter()
            .filter(|a| a.corroborated && a.relevant_evidence.is_empty())
            .map(|a| &a.claim_id),
    );
    if !without_evidence.is_empty() {
        reasons.push(TraceReason::ClaimsWithoutOwnEvidence {
            claim_ids: without_evidence,
        });
    }
    // Every cited evidence record must actually have been retrieved and
    // integrity-checked as Valid. Cited-but-unchecked evidence (offline
    // backend, hallucinated reference) can never support a decision.
    let not_valid: Vec<EvidenceId> = relevant
        .iter()
        .filter(|id| {
            !verifications
                .iter()
                .any(|v| &v.evidence_id == *id && v.integrity_status == IntegrityStatus::Valid)
        })
        .cloned()
        .collect();
    if !not_valid.is_empty() {
        reasons.push(TraceReason::CitedEvidenceNotValid {
            evidence_ids: not_valid,
        });
    }
    if relevant.is_empty() {
        reasons.push(TraceReason::NoCheckedEvidence);
    }

    let rule = reasons
        .first()
        .map_or(PolicyRule::AllChecksPassed, rule_for);
    PolicyTrace {
        version: version.into(),
        kind: kind_for(rule),
        rule,
        reasons,
        counts: counts(evaluation, verifications),
    }
}

/// Clones of `ids`, each once, in the order first seen.
fn distinct<'a, T: Clone + PartialEq + 'a>(ids: impl Iterator<Item = &'a T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for id in ids {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// The rule a failed check belongs to.
fn rule_for(reason: &TraceReason) -> PolicyRule {
    match reason {
        TraceReason::InvalidProofs { .. } => PolicyRule::InvalidProof,
        TraceReason::ContradictedEvidence { .. } => PolicyRule::RecordedContradiction,
        TraceReason::EvaluatorAbstained { .. } => PolicyRule::Abstention,
        TraceReason::OpenItems { .. } => PolicyRule::UnresolvedItems,
        TraceReason::NoAssessments
        | TraceReason::UncorroboratedClaims { .. }
        | TraceReason::ClaimsWithoutOwnEvidence { .. }
        | TraceReason::CitedEvidenceNotValid { .. }
        | TraceReason::NoCheckedEvidence => PolicyRule::InsufficientEvidence,
    }
}

/// The kind each rule derives.
fn kind_for(rule: PolicyRule) -> RecommendationKind {
    match rule {
        PolicyRule::InvalidProof => RecommendationKind::Blocked,
        PolicyRule::RecordedContradiction | PolicyRule::UnresolvedItems => {
            RecommendationKind::Contested
        }
        PolicyRule::Abstention => RecommendationKind::Abstained,
        PolicyRule::InsufficientEvidence => RecommendationKind::InsufficientEvidence,
        PolicyRule::AllChecksPassed => RecommendationKind::Supported,
    }
}

fn counts(evaluation: &Evaluation, verifications: &[EvidenceVerification]) -> TraceCounts {
    let integrity = |pred: fn(IntegrityStatus) -> bool| {
        verifications
            .iter()
            .filter(|v| pred(v.integrity_status))
            .count() as u64
    };
    TraceCounts {
        claims: evaluation.assessments.len() as u64,
        corroborated: evaluation
            .assessments
            .iter()
            .filter(|a| a.corroborated)
            .count() as u64,
        proofs_valid: integrity(|s| s == IntegrityStatus::Valid),
        proofs_invalid: integrity(|s| s == IntegrityStatus::Invalid),
        proofs_unavailable_or_error: integrity(|s| {
            matches!(
                s,
                IntegrityStatus::Unavailable | IntegrityStatus::VerificationError
            )
        }),
        unresolved_items: evaluation.unresolved_items.len() as u64,
    }
}

/// Stable machine-readable code for the rule that decided a
/// recommendation, `<kind>.<rule>` in their serialized names (for example
/// `contested.recorded_contradiction`). It goes next to the English
/// sentence of [`rationale_for`], so a reader can localize the rationale
/// without parsing it. A code never changes meaning; a new rule gets a
/// new code.
pub fn rationale_code(rule: PolicyRule) -> &'static str {
    match rule {
        PolicyRule::InvalidProof => "blocked.invalid_proof",
        PolicyRule::RecordedContradiction => "contested.recorded_contradiction",
        PolicyRule::Abstention => "abstained.abstention",
        PolicyRule::UnresolvedItems => "contested.unresolved_items",
        PolicyRule::InsufficientEvidence => "insufficient_evidence.insufficient_evidence",
        PolicyRule::AllChecksPassed => "supported.all_checks_passed",
    }
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
    use std::collections::HashMap;

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

    /// The derivation as it stood before `explain` (the schema 1.2 code of
    /// `derive_recommendation_with`, copied verbatim and only renamed).
    /// Frozen here so the differential test can hold the one
    /// implementation to it on every input.
    fn frozen_derivation(
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

    /// xorshift64 (Marsaglia's 13/7/17 triple): a tiny deterministic
    /// generator, so the property tests need no dependency and every run
    /// checks the same cases.
    struct XorShift(u64);

    impl XorShift {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// In `0..n` (the modulo bias is irrelevant here).
        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }

        /// True with probability `percent` / 100.
        fn percent(&mut self, percent: usize) -> bool {
            self.below(100) < percent
        }

        fn pick<T: Copy>(&mut self, items: &[T]) -> T {
            items[self.below(items.len())]
        }
    }

    /// Cases per property test, each checked under both rule orders.
    const CASES: usize = 20_000;
    const VERSIONS: [PolicyVersion; 2] = [PolicyVersion::V1_1, PolicyVersion::V1_2];
    const RULES: [PolicyRule; 6] = [
        PolicyRule::InvalidProof,
        PolicyRule::RecordedContradiction,
        PolicyRule::Abstention,
        PolicyRule::UnresolvedItems,
        PolicyRule::InsufficientEvidence,
        PolicyRule::AllChecksPassed,
    ];
    const EVIDENCE: [&str; 5] = ["ev-a", "ev-b", "ev-c", "ev-d", "ev-e"];
    const CLAIMS: [&str; 4] = ["c1", "c2", "c3", "c4"];
    const ITEMS: [&str; 3] = ["item-1", "item-2", "item-3"];
    const INTEGRITY: [IntegrityStatus; 5] = [
        IntegrityStatus::Valid,
        IntegrityStatus::Invalid,
        IntegrityStatus::Unavailable,
        IntegrityStatus::VerificationError,
        IntegrityStatus::NotChecked,
    ];
    const FACTUAL: [FactualStatus; 4] = [
        FactualStatus::Corroborated,
        FactualStatus::Uncorroborated,
        FactualStatus::Contradicted,
        FactualStatus::NotAssessed,
    ];

    /// One pseudo-random evaluation and verification set. Most cases look
    /// like what the orchestrator records (each cited record verified
    /// once, contradicting records marked contradicted), and every part is
    /// sometimes perturbed, so all six rules and both rule orders are
    /// reached, along with inputs no real run produces: no assessments,
    /// repeated claims, cited records never verified, repeated or uncited
    /// verification records.
    fn random_case(rng: &mut XorShift) -> (Evaluation, Vec<EvidenceVerification>) {
        let claims = if rng.percent(8) { 0 } else { 1 + rng.below(3) };
        let mut assessments = Vec::new();
        for &own in CLAIMS.iter().take(claims) {
            let claim = if rng.percent(5) {
                rng.pick(&CLAIMS)
            } else {
                own
            };
            let mut relevant = Vec::new();
            for _ in 0..rng.below(3) {
                relevant.push(rng.pick(&EVIDENCE));
            }
            let mut contradicting = Vec::new();
            if rng.percent(15) {
                contradicting.push(rng.pick(&EVIDENCE));
            }
            let corroborated = rng.percent(80);
            assessments.push(assessment(claim, corroborated, &relevant, &contradicting));
        }
        let mut unresolved = Vec::new();
        if rng.percent(15) {
            for _ in 0..1 + rng.below(2) {
                unresolved.push(rng.pick(&ITEMS));
            }
        }
        let abstain = rng.percent(12);
        let mut e = evaluation(assessments, &unresolved, abstain);
        if abstain && rng.percent(30) {
            e.abstain_reason = None;
        }

        let mut cited: Vec<EvidenceId> = Vec::new();
        let mut contradicting: Vec<EvidenceId> = Vec::new();
        for a in &e.assessments {
            for id in a.relevant_evidence.iter().chain(&a.contradicting_evidence) {
                if !cited.contains(id) {
                    cited.push(id.clone());
                }
            }
            contradicting.extend(a.contradicting_evidence.iter().cloned());
        }
        let mut v = Vec::new();
        for id in &cited {
            if rng.percent(8) {
                continue; // cited, never retrieved or verified
            }
            let integrity = if rng.percent(85) {
                IntegrityStatus::Valid
            } else {
                rng.pick(&INTEGRITY)
            };
            let factual = if !rng.percent(85) {
                rng.pick(&FACTUAL)
            } else if contradicting.contains(id) {
                FactualStatus::Contradicted
            } else {
                FactualStatus::Corroborated
            };
            v.push(verification(id.as_str(), integrity, factual));
            if rng.percent(5) {
                let (integrity, factual) = (rng.pick(&INTEGRITY), rng.pick(&FACTUAL));
                v.push(verification(id.as_str(), integrity, factual));
            }
        }
        for _ in 0..rng.below(3) {
            let id = rng.pick(&EVIDENCE);
            let integrity = if rng.percent(80) {
                IntegrityStatus::Valid
            } else {
                rng.pick(&INTEGRITY)
            };
            let factual = rng.pick(&FACTUAL);
            v.push(verification(id, integrity, factual));
        }
        (e, v)
    }

    /// `rationale_for`'s sentence, rebuilt from a trace's counts alone.
    fn rationale_from(trace: &PolicyTrace) -> String {
        let c = trace.counts;
        format!(
            "{:?}: {}/{} claims corroborated; proofs: {} valid, {} invalid, {} unavailable/error; {} unresolved challenges.",
            trace.kind,
            c.corroborated,
            c.claims,
            c.proofs_valid,
            c.proofs_invalid,
            c.proofs_unavailable_or_error,
            c.unresolved_items
        )
    }

    #[test]
    fn explain_agrees_with_the_frozen_derivation_on_random_cases() {
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        let mut hits: HashMap<(PolicyRules, PolicyRule), usize> = HashMap::new();
        let mut orders_differ = 0;
        for case in 0..CASES {
            let (e, v) = random_case(&mut rng);
            for version in VERSIONS {
                let expected = frozen_derivation(version, &e, &v);
                let trace = explain(version, &e, &v);
                assert_eq!(trace.kind, expected, "case {case} {version:?}: {e:?} {v:?}");
                assert_eq!(derive_recommendation_with(version, &e, &v), expected);
                assert_eq!(trace.version, PolicyRules::from(version));
                // The trace is consistent with itself: the first failed
                // check decided the rule, and no failed check means
                // supported.
                assert_eq!(trace.kind, kind_for(trace.rule));
                assert_eq!(
                    trace
                        .reasons
                        .first()
                        .map_or(PolicyRule::AllChecksPassed, rule_for),
                    trace.rule
                );
                assert_eq!(
                    trace.reasons.is_empty(),
                    trace.kind == RecommendationKind::Supported
                );
                // Its counts are exactly the numbers rationale_for prints.
                assert_eq!(rationale_from(&trace), rationale_for(trace.kind, &e, &v));
                *hits.entry((trace.version, trace.rule)).or_default() += 1;
            }
            // The two orders disagree only where 1.1 let an abstention
            // hide a recorded contradiction.
            let old = derive_recommendation_with(PolicyVersion::V1_1, &e, &v);
            let new = derive_recommendation_with(PolicyVersion::V1_2, &e, &v);
            if old != new {
                assert_eq!(
                    (old, new),
                    (RecommendationKind::Abstained, RecommendationKind::Contested),
                    "case {case}"
                );
                orders_differ += 1;
            }
        }
        // Every rule fired often under both orders, so no branch went
        // untested.
        for version in VERSIONS {
            for rule in RULES {
                let n = hits
                    .get(&(version.into(), rule))
                    .copied()
                    .unwrap_or_default();
                assert!(n >= 400, "{version:?} {rule:?} fired only {n} times");
            }
        }
        assert!(
            orders_differ >= 200,
            "orders differ in {orders_differ} cases"
        );
    }

    #[test]
    fn policy_properties_hold_on_random_cases() {
        let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
        let mut supported = 0;
        for case in 0..CASES {
            let (e, v) = random_case(&mut rng);
            let corrupt = rng.pick(&EVIDENCE);
            for version in VERSIONS {
                // An invalid proof anywhere, cited or not, blocks, and the
                // trace names it.
                let mut corrupted = v.clone();
                corrupted.push(verification(
                    corrupt,
                    IntegrityStatus::Invalid,
                    FactualStatus::NotAssessed,
                ));
                let blocked = explain(version, &e, &corrupted);
                assert_eq!(blocked.rule, PolicyRule::InvalidProof, "case {case}");
                assert!(
                    matches!(
                        &blocked.reasons[0],
                        TraceReason::InvalidProofs { evidence_ids }
                            if evidence_ids.contains(&EvidenceId::new(corrupt))
                    ),
                    "case {case}: {:?}",
                    blocked.reasons
                );

                if explain(version, &e, &v).kind != RecommendationKind::Supported {
                    continue;
                }
                supported += 1;
                // Each single weakening of a supported case loses support,
                // and the trace names exactly that weakening.
                let mut open = e.clone();
                open.unresolved_items.push("item-x".into());
                assert_eq!(
                    explain(version, &open, &v).reasons,
                    [TraceReason::OpenItems {
                        item_ids: vec!["item-x".into()]
                    }]
                );
                let mut abstained = e.clone();
                abstained.abstain = true;
                assert_eq!(
                    explain(version, &abstained, &v).rule,
                    PolicyRule::Abstention
                );
                // A valid proof never stands in for corroboration.
                let mut doubted = e.clone();
                doubted.assessments[0].corroborated = false;
                assert_eq!(
                    explain(version, &doubted, &v).reasons,
                    [TraceReason::UncorroboratedClaims {
                        claim_ids: vec![doubted.assessments[0].claim_id.clone()]
                    }]
                );
                // Losing the valid proof of one cited record loses support.
                let cited = e.assessments[0].relevant_evidence[0].clone();
                let unproven: Vec<EvidenceVerification> = v
                    .iter()
                    .cloned()
                    .map(|mut x| {
                        if x.evidence_id == cited && x.integrity_status == IntegrityStatus::Valid {
                            x.integrity_status = IntegrityStatus::Unavailable;
                        }
                        x
                    })
                    .collect();
                assert_eq!(
                    explain(version, &e, &unproven).reasons,
                    [TraceReason::CitedEvidenceNotValid {
                        evidence_ids: vec![cited.clone()]
                    }]
                );
                // A recorded contradiction plus an abstention: 1.2 reports
                // the contradiction, 1.1 the abstention.
                let contradicted: Vec<EvidenceVerification> = v
                    .iter()
                    .cloned()
                    .map(|mut x| {
                        if x.evidence_id == cited {
                            x.factual_status = FactualStatus::Contradicted;
                        }
                        x
                    })
                    .collect();
                let expected = match version {
                    PolicyVersion::V1_1 => PolicyRule::Abstention,
                    PolicyVersion::V1_2 => PolicyRule::RecordedContradiction,
                };
                assert_eq!(explain(version, &abstained, &contradicted).rule, expected);
            }
        }
        assert!(supported >= 1_000, "only {supported} supported cases");
    }

    fn ids(list: &[&str]) -> Vec<EvidenceId> {
        list.iter().map(|s| EvidenceId::new(*s)).collect()
    }

    fn claims(list: &[&str]) -> Vec<ClaimId> {
        list.iter().map(|s| ClaimId::new(*s)).collect()
    }

    fn abstained_with_reason() -> TraceReason {
        TraceReason::EvaluatorAbstained {
            abstain_reason: Some("evidence is insufficient to judge".into()),
        }
    }

    #[test]
    fn invalid_proof_trace_names_every_invalid_record_cited_or_not() {
        let e = evaluation(
            vec![assessment("c1", true, &["ev1"], &[])],
            &["item-1"],
            true,
        );
        let v = vec![
            verification("ev1", IntegrityStatus::Valid, FactualStatus::Corroborated),
            verification("evX", IntegrityStatus::Invalid, FactualStatus::NotAssessed),
            verification("evX", IntegrityStatus::Invalid, FactualStatus::NotAssessed),
        ];
        let trace = explain(PolicyVersion::V1_2, &e, &v);
        assert_eq!(trace.kind, RecommendationKind::Blocked);
        assert_eq!(trace.rule, PolicyRule::InvalidProof);
        assert_eq!(
            trace.reasons,
            [
                TraceReason::InvalidProofs {
                    evidence_ids: ids(&["evX"])
                },
                abstained_with_reason(),
                TraceReason::OpenItems {
                    item_ids: vec!["item-1".into()]
                },
            ]
        );
        assert_eq!(
            trace.counts,
            TraceCounts {
                claims: 1,
                corroborated: 1,
                proofs_valid: 1,
                proofs_invalid: 2,
                proofs_unavailable_or_error: 0,
                unresolved_items: 1,
            }
        );
    }

    #[test]
    fn contradiction_trace_follows_each_versions_rule_order() {
        // Live S02 shape: the stored cap contradicts the claim, and the
        // Evaluator abstains. A contradicted record that no assessment
        // cites or contradicts is not considered.
        let e = evaluation(vec![assessment("c1", false, &[], &["ev1"])], &[], true);
        let v = vec![
            verification("ev1", IntegrityStatus::Valid, FactualStatus::Contradicted),
            verification("ev9", IntegrityStatus::Valid, FactualStatus::Contradicted),
        ];
        let contradiction = TraceReason::ContradictedEvidence {
            evidence_ids: ids(&["ev1"]),
        };
        let uncorroborated = TraceReason::UncorroboratedClaims {
            claim_ids: claims(&["c1"]),
        };
        let current = explain(PolicyVersion::V1_2, &e, &v);
        assert_eq!(current.version, PolicyRules::V1_2);
        assert_eq!(
            (current.kind, current.rule),
            (
                RecommendationKind::Contested,
                PolicyRule::RecordedContradiction
            )
        );
        assert_eq!(
            current.reasons,
            [
                contradiction.clone(),
                abstained_with_reason(),
                uncorroborated.clone(),
                TraceReason::NoCheckedEvidence,
            ]
        );
        let old = explain(PolicyVersion::V1_1, &e, &v);
        assert_eq!(old.version, PolicyRules::V1_1);
        assert_eq!(
            (old.kind, old.rule),
            (RecommendationKind::Abstained, PolicyRule::Abstention)
        );
        assert_eq!(
            old.reasons,
            [
                abstained_with_reason(),
                contradiction,
                uncorroborated,
                TraceReason::NoCheckedEvidence,
            ]
        );
    }

    #[test]
    fn abstention_trace_carries_the_reason_and_the_open_items() {
        let mut e = evaluation(vec![assessment("c1", false, &[], &[])], &["i1", "i2"], true);
        let trace = explain(PolicyVersion::V1_2, &e, &[]);
        assert_eq!(
            (trace.kind, trace.rule),
            (RecommendationKind::Abstained, PolicyRule::Abstention)
        );
        let rest = [
            TraceReason::OpenItems {
                item_ids: vec!["i1".into(), "i2".into()],
            },
            TraceReason::UncorroboratedClaims {
                claim_ids: claims(&["c1"]),
            },
            TraceReason::NoCheckedEvidence,
        ];
        let mut expected = vec![abstained_with_reason()];
        expected.extend(rest.iter().cloned());
        assert_eq!(trace.reasons, expected);
        // No stated reason is reported as none, not invented.
        e.abstain_reason = None;
        let mut expected = vec![TraceReason::EvaluatorAbstained {
            abstain_reason: None,
        }];
        expected.extend(rest);
        assert_eq!(explain(PolicyVersion::V1_2, &e, &[]).reasons, expected);
    }

    #[test]
    fn unresolved_items_trace_lists_item_ids_as_recorded() {
        let e = evaluation(
            vec![assessment("c1", true, &["ev1"], &[])],
            &["item-2", "item-1", "item-2"],
            false,
        );
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Corroborated,
        )];
        let trace = explain(PolicyVersion::V1_2, &e, &v);
        assert_eq!(
            (trace.kind, trace.rule),
            (RecommendationKind::Contested, PolicyRule::UnresolvedItems)
        );
        assert_eq!(
            trace.reasons,
            [TraceReason::OpenItems {
                item_ids: vec!["item-2".into(), "item-1".into(), "item-2".into()]
            }]
        );
        assert_eq!(trace.counts.unresolved_items, 3);
    }

    #[test]
    fn insufficient_evidence_trace_names_each_failed_check() {
        let insufficient = |e: &Evaluation, v: &[EvidenceVerification]| {
            let trace = explain(PolicyVersion::V1_2, e, v);
            assert_eq!(
                (trace.kind, trace.rule),
                (
                    RecommendationKind::InsufficientEvidence,
                    PolicyRule::InsufficientEvidence
                )
            );
            trace.reasons
        };
        let valid = |id| verification(id, IntegrityStatus::Valid, FactualStatus::Corroborated);

        // Nothing assessed, so nothing cited either.
        assert_eq!(
            insufficient(&evaluation(vec![], &[], false), &[]),
            [TraceReason::NoAssessments, TraceReason::NoCheckedEvidence]
        );
        // Uncorroborated claims, each once.
        let e = evaluation(
            vec![
                assessment("c1", false, &["ev1"], &[]),
                assessment("c2", true, &["ev2"], &[]),
                assessment("c1", false, &["ev1"], &[]),
            ],
            &[],
            false,
        );
        assert_eq!(
            insufficient(&e, &[valid("ev1"), valid("ev2")]),
            [TraceReason::UncorroboratedClaims {
                claim_ids: claims(&["c1"])
            }]
        );
        // A corroborated claim citing nothing cannot ride on c1's proof.
        let e = evaluation(
            vec![
                assessment("c1", true, &["ev1"], &[]),
                assessment("c2", true, &[], &[]),
            ],
            &[],
            false,
        );
        assert_eq!(
            insufficient(&e, &[valid("ev1")]),
            [TraceReason::ClaimsWithoutOwnEvidence {
                claim_ids: claims(&["c2"])
            }]
        );
        // Cited records without a valid proof: unavailable, error,
        // unchecked, never verified. A valid record beside an unavailable
        // one for the same id counts as valid.
        let e = evaluation(
            vec![
                assessment("c1", true, &["ev1", "ev2", "ev3"], &[]),
                assessment("c2", true, &["ev4", "ev5", "ev1"], &[]),
            ],
            &[],
            false,
        );
        let mut v = vec![
            valid("ev1"),
            verification(
                "ev2",
                IntegrityStatus::Unavailable,
                FactualStatus::Corroborated,
            ),
            verification(
                "ev3",
                IntegrityStatus::VerificationError,
                FactualStatus::Corroborated,
            ),
            verification(
                "ev4",
                IntegrityStatus::NotChecked,
                FactualStatus::Corroborated,
            ),
        ];
        assert_eq!(
            insufficient(&e, &v),
            [TraceReason::CitedEvidenceNotValid {
                evidence_ids: ids(&["ev2", "ev3", "ev4", "ev5"])
            }]
        );
        v.push(valid("ev2"));
        assert_eq!(
            insufficient(&e, &v),
            [TraceReason::CitedEvidenceNotValid {
                evidence_ids: ids(&["ev3", "ev4", "ev5"])
            }]
        );
        // No claim cites anything.
        let e = evaluation(vec![assessment("c1", true, &[], &[])], &[], false);
        assert_eq!(
            insufficient(&e, &[valid("ev1")]),
            [
                TraceReason::ClaimsWithoutOwnEvidence {
                    claim_ids: claims(&["c1"])
                },
                TraceReason::NoCheckedEvidence,
            ]
        );
    }

    #[test]
    fn supported_trace_has_no_reasons_and_rationale_counts() {
        let e = evaluation(
            vec![
                assessment("c1", true, &["ev1"], &[]),
                assessment("c2", true, &["ev2"], &[]),
            ],
            &[],
            false,
        );
        let v = vec![
            verification("ev1", IntegrityStatus::Valid, FactualStatus::Corroborated),
            verification("ev2", IntegrityStatus::Valid, FactualStatus::Corroborated),
            // Retrieved, never cited: it cannot help, and does not hurt.
            verification(
                "ev3",
                IntegrityStatus::Unavailable,
                FactualStatus::NotAssessed,
            ),
        ];
        let trace = explain(PolicyVersion::V1_2, &e, &v);
        assert_eq!(
            (trace.kind, trace.rule),
            (RecommendationKind::Supported, PolicyRule::AllChecksPassed)
        );
        assert!(trace.reasons.is_empty());
        assert_eq!(
            rationale_for(trace.kind, &e, &v),
            "Supported: 2/2 claims corroborated; proofs: 2 valid, 0 invalid, 1 unavailable/error; 0 unresolved challenges."
        );
        assert_eq!(rationale_from(&trace), rationale_for(trace.kind, &e, &v));
    }

    #[test]
    fn trace_serializes_to_its_documented_shape() {
        let e = evaluation(
            vec![assessment("c1", false, &[], &["ev1"])],
            &["item-1"],
            true,
        );
        let v = vec![verification(
            "ev1",
            IntegrityStatus::Valid,
            FactualStatus::Contradicted,
        )];
        let trace = explain(PolicyVersion::V1_2, &e, &v);
        let json = serde_json::to_value(&trace).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "version": "1.2",
                "kind": "contested",
                "rule": "recorded_contradiction",
                "reasons": [
                    {"reason": "contradicted_evidence", "evidence_ids": ["ev1"]},
                    {"reason": "evaluator_abstained", "abstain_reason": "evidence is insufficient to judge"},
                    {"reason": "open_items", "item_ids": ["item-1"]},
                    {"reason": "uncorroborated_claims", "claim_ids": ["c1"]},
                    {"reason": "no_checked_evidence"}
                ],
                "counts": {
                    "claims": 1,
                    "corroborated": 0,
                    "proofs_valid": 1,
                    "proofs_invalid": 0,
                    "proofs_unavailable_or_error": 0,
                    "unresolved_items": 1
                }
            })
        );
        let back: PolicyTrace = serde_json::from_value(json).unwrap();
        assert_eq!(back, trace);
    }

    #[test]
    fn rationale_codes_are_stable_and_name_kind_and_rule() {
        let codes: Vec<&str> = RULES.iter().map(|r| rationale_code(*r)).collect();
        assert_eq!(
            codes,
            [
                "blocked.invalid_proof",
                "contested.recorded_contradiction",
                "abstained.abstention",
                "contested.unresolved_items",
                "insufficient_evidence.insufficient_evidence",
                "supported.all_checks_passed",
            ]
        );
        let name = |value: serde_json::Value| value.as_str().unwrap().to_string();
        for rule in RULES {
            let kind = name(serde_json::to_value(kind_for(rule)).unwrap());
            let rule_name = name(serde_json::to_value(rule).unwrap());
            assert_eq!(rationale_code(rule), format!("{kind}.{rule_name}"));
        }
    }
}
