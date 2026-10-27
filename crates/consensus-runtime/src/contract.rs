// SPDX-License-Identifier: Apache-2.0
//! Protocol 1.3 contract: what a model document must say, given what the
//! run has recorded so far.
//!
//! Up to protocol 1.2 the runtime trusted the Evaluator to judge every
//! claim and objection, and each round judged only what that round showed
//! it: an objection or a contradiction from round 1 never came back in
//! round 2 (audit finding RT-1), and the Meta adapter dropped ids it could
//! not match (MA-1). Silence, or a misspelled id, could make an objection
//! vanish on its way to the policy. From protocol 1.3 every evaluation is
//! checked against the run's own record by one function,
//! [`validate_evaluation`], in two places:
//! - the orchestrator, before `EvaluationSubmitted` is recorded: a
//!   violation fails the run at stage `evaluate` with code
//!   `evaluation_contract`, and the refused call stays on the record;
//! - the state machine, when it applies an `EvaluationSubmitted` record of
//!   schema 1.3 or later, with the [`EvaluationContext`] it derived from
//!   the events before it, so a forged 1.3 log fails replay.
//!
//! It binds every `ModelClient`, the scripted fakes included. Nothing here
//! completes, repairs or reorders a document: it is accepted as written or
//! refused.
//!
//! The evaluation rules, as named by [`ContractViolation::rule`]:
//! - `one_assessment_per_claim`: exactly one assessment per claim of the
//!   proposal under evaluation, and none for any other id;
//! - `cited_evidence_retrieved`: every relevant or contradicting id names
//!   a record this run retrieved;
//! - `one_verdict_per_item`: exactly one verdict per expected item (the
//!   objections still open from earlier rounds, then this round's), and
//!   none for any other id;
//! - `resolved_needs_reason`: a `resolved` verdict says why;
//! - `unresolved_matches_verdicts`: `unresolved_items` lists the items
//!   with an `unresolved` verdict, each once, and nothing else;
//! - `carried_contradiction_judged`: a contradiction recorded in an
//!   earlier round and not cleared since is recorded again (in some
//!   assessment's `contradicting_evidence`) or cleared with a non-empty
//!   reason; silence never clears it;
//! - `cleared_is_carried`: only a carried contradiction can be cleared,
//!   once, and not while it is also listed as contradicting.
//!
//! Two id rules keep those well defined, checked on the documents that
//! introduce the ids: `item_ids_unique` ([`validate_challenge`]: a
//! challenge item id is non-empty and never reused within a run, so one
//! verdict cannot settle two objections) and `claim_ids_unique`
//! ([`validate_claims`]: claim ids are unique within a proposal, so one
//! assessment cannot cover two claims).
//!
//! Records before schema 1.3 predate the contract and are never checked
//! against it ([`binds`]): they replay exactly as they always did.

use consensus_domain::*;
use thiserror::Error;

/// One Critic objection still open for judgment, with the round (1 =
/// first) in which the Critic raised it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenItem {
    pub item: ChallengeItem,
    pub raised_in_round: u32,
}

/// What a protocol 1.3 evaluation is checked against. The state machine
/// derives it from the run's events
/// ([`RunAggregate::evaluation_context`](crate::state_machine::RunAggregate::evaluation_context)),
/// and the orchestrator checks against that same derivation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationContext {
    /// Claim ids of the proposal under evaluation, in proposal order.
    pub claims: Vec<ClaimId>,
    /// The items the evaluation must judge: those the previous evaluation
    /// left open, in its verdict order, then this round's challenge items.
    pub expected_items: Vec<String>,
    /// Every evidence id with an `EvidenceRetrieved` event in the run.
    pub retrieved: Vec<EvidenceId>,
    /// Records an earlier evaluation listed as contradicting a claim and
    /// no later evaluation cleared.
    pub carried_contradictions: Vec<EvidenceId>,
}

/// A broken contract rule and the value that broke it. The message
/// starts with the rule's name ([`rule`](Self::rule)).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContractViolation {
    #[error("one_assessment_per_claim: claim {:?} has no assessment", .0.as_str())]
    UnassessedClaim(ClaimId),
    #[error("one_assessment_per_claim: claim {:?} is assessed more than once", .0.as_str())]
    DuplicateAssessment(ClaimId),
    #[error(
        "one_assessment_per_claim: {:?} is not a claim of the proposal under evaluation",
        .0.as_str()
    )]
    UnknownClaim(ClaimId),
    #[error(
        "cited_evidence_retrieved: {field} of claim {:?} names {:?}, a record this run never retrieved",
        .claim.as_str(),
        .evidence.as_str()
    )]
    UnretrievedEvidence {
        claim: ClaimId,
        /// `relevant_evidence` or `contradicting_evidence`.
        field: &'static str,
        evidence: EvidenceId,
    },
    #[error("one_verdict_per_item: item {0:?} has no verdict")]
    MissingVerdict(String),
    #[error("one_verdict_per_item: item {0:?} has more than one verdict")]
    DuplicateVerdict(String),
    #[error(
        "one_verdict_per_item: {0:?} is not an item under evaluation (open from an earlier round or raised in this one)"
    )]
    UnknownItem(String),
    #[error("resolved_needs_reason: item {0:?} is resolved without a reason")]
    ResolvedWithoutReason(String),
    #[error(
        "unresolved_matches_verdicts: unresolved_items lists {0:?}, which has no unresolved verdict"
    )]
    UnresolvedWithoutVerdict(String),
    #[error("unresolved_matches_verdicts: unresolved_items lists {0:?} more than once")]
    UnresolvedListedTwice(String),
    #[error(
        "unresolved_matches_verdicts: item {0:?} has an unresolved verdict but is missing from unresolved_items"
    )]
    UnresolvedNotListed(String),
    #[error(
        "carried_contradiction_judged: {:?}, contradicted in an earlier round, is neither listed as contradicting again nor cleared",
        .0.as_str()
    )]
    SilentContradiction(EvidenceId),
    #[error("carried_contradiction_judged: {:?} is cleared without a reason", .0.as_str())]
    ClearedWithoutReason(EvidenceId),
    #[error(
        "cleared_is_carried: {:?} is not a contradiction carried from an earlier round",
        .0.as_str()
    )]
    ClearedNotCarried(EvidenceId),
    #[error("cleared_is_carried: {:?} is cleared more than once", .0.as_str())]
    ClearedTwice(EvidenceId),
    #[error(
        "cleared_is_carried: {:?} is cleared and listed as contradicting in the same evaluation",
        .0.as_str()
    )]
    ClearedAndContradicting(EvidenceId),
    #[error("item_ids_unique: challenge item {index} has an empty id")]
    EmptyItemId {
        /// 1-based position of the item in its challenge.
        index: usize,
    },
    #[error("item_ids_unique: item id {0:?} is already used in this run")]
    ReusedItemId(String),
    #[error(
        "claim_ids_unique: claim id {:?} appears more than once in the proposal",
        .0.as_str()
    )]
    DuplicateClaimId(ClaimId),
}

impl ContractViolation {
    /// Stable name of the broken rule (see the module documentation).
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnassessedClaim(_) | Self::DuplicateAssessment(_) | Self::UnknownClaim(_) => {
                "one_assessment_per_claim"
            }
            Self::UnretrievedEvidence { .. } => "cited_evidence_retrieved",
            Self::MissingVerdict(_) | Self::DuplicateVerdict(_) | Self::UnknownItem(_) => {
                "one_verdict_per_item"
            }
            Self::ResolvedWithoutReason(_) => "resolved_needs_reason",
            Self::UnresolvedWithoutVerdict(_)
            | Self::UnresolvedListedTwice(_)
            | Self::UnresolvedNotListed(_) => "unresolved_matches_verdicts",
            Self::SilentContradiction(_) | Self::ClearedWithoutReason(_) => {
                "carried_contradiction_judged"
            }
            Self::ClearedNotCarried(_)
            | Self::ClearedTwice(_)
            | Self::ClearedAndContradicting(_) => "cleared_is_carried",
            Self::EmptyItemId { .. } | Self::ReusedItemId(_) => "item_ids_unique",
            Self::DuplicateClaimId(_) => "claim_ids_unique",
        }
    }
}

/// True when a record of `schema_version` is bound by the protocol 1.3
/// contract: schema 1.3 or later. A version that does not parse as
/// `major.minor` counts as later, so an unknown record is checked rather
/// than trusted (the event log refuses such records anyway).
pub fn binds(schema_version: &str) -> bool {
    let mut parts = schema_version.split('.').map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten(), parts.next()) {
        (Some(major), Some(minor), None) => (major, minor) >= (1, 3),
        _ => true,
    }
}

/// Checks `evaluation` against every evaluation rule. Rules are checked
/// in the order the module documentation lists them, each over the
/// document in its own order; the first violation is returned.
pub fn validate_evaluation(
    ctx: &EvaluationContext,
    evaluation: &Evaluation,
) -> Result<(), ContractViolation> {
    // Claims: exactly one assessment each, citing only this run's records.
    let mut assessed: Vec<&ClaimId> = Vec::new();
    for a in &evaluation.assessments {
        if !ctx.claims.contains(&a.claim_id) {
            return Err(ContractViolation::UnknownClaim(a.claim_id.clone()));
        }
        if assessed.contains(&&a.claim_id) {
            return Err(ContractViolation::DuplicateAssessment(a.claim_id.clone()));
        }
        assessed.push(&a.claim_id);
        for (field, cited) in [
            ("relevant_evidence", &a.relevant_evidence),
            ("contradicting_evidence", &a.contradicting_evidence),
        ] {
            if let Some(id) = cited.iter().find(|id| !ctx.retrieved.contains(id)) {
                return Err(ContractViolation::UnretrievedEvidence {
                    claim: a.claim_id.clone(),
                    field,
                    evidence: id.clone(),
                });
            }
        }
    }
    if let Some(claim) = ctx.claims.iter().find(|c| !assessed.contains(c)) {
        return Err(ContractViolation::UnassessedClaim(claim.clone()));
    }

    // Objections: one verdict each; a resolved one says why.
    let mut judged: Vec<&str> = Vec::new();
    for v in &evaluation.item_verdicts {
        if !ctx.expected_items.contains(&v.item_id) {
            return Err(ContractViolation::UnknownItem(v.item_id.clone()));
        }
        if judged.contains(&v.item_id.as_str()) {
            return Err(ContractViolation::DuplicateVerdict(v.item_id.clone()));
        }
        judged.push(&v.item_id);
        if v.status == ItemStatus::Resolved && v.reason.trim().is_empty() {
            return Err(ContractViolation::ResolvedWithoutReason(v.item_id.clone()));
        }
    }
    if let Some(item) = ctx
        .expected_items
        .iter()
        .find(|i| !judged.contains(&i.as_str()))
    {
        return Err(ContractViolation::MissingVerdict(item.clone()));
    }

    // `unresolved_items` is exactly the unresolved verdicts.
    let unresolved_verdict = |id: &str| {
        evaluation
            .item_verdicts
            .iter()
            .any(|v| v.item_id == id && v.status == ItemStatus::Unresolved)
    };
    let mut listed: Vec<&str> = Vec::new();
    for id in &evaluation.unresolved_items {
        if listed.contains(&id.as_str()) {
            return Err(ContractViolation::UnresolvedListedTwice(id.clone()));
        }
        if !unresolved_verdict(id) {
            return Err(ContractViolation::UnresolvedWithoutVerdict(id.clone()));
        }
        listed.push(id);
    }
    if let Some(v) = evaluation
        .item_verdicts
        .iter()
        .find(|v| v.status == ItemStatus::Unresolved && !listed.contains(&v.item_id.as_str()))
    {
        return Err(ContractViolation::UnresolvedNotListed(v.item_id.clone()));
    }

    // Carried contradictions: recorded again or cleared, never dropped.
    let contradicting = |id: &EvidenceId| {
        evaluation
            .assessments
            .iter()
            .any(|a| a.contradicting_evidence.contains(id))
    };
    let mut cleared: Vec<&EvidenceId> = Vec::new();
    for c in &evaluation.cleared_contradictions {
        if !ctx.carried_contradictions.contains(&c.evidence_id) {
            return Err(ContractViolation::ClearedNotCarried(c.evidence_id.clone()));
        }
        if cleared.contains(&&c.evidence_id) {
            return Err(ContractViolation::ClearedTwice(c.evidence_id.clone()));
        }
        if contradicting(&c.evidence_id) {
            return Err(ContractViolation::ClearedAndContradicting(
                c.evidence_id.clone(),
            ));
        }
        if c.reason.trim().is_empty() {
            return Err(ContractViolation::ClearedWithoutReason(
                c.evidence_id.clone(),
            ));
        }
        cleared.push(&c.evidence_id);
    }
    if let Some(id) = ctx
        .carried_contradictions
        .iter()
        .find(|id| !contradicting(id) && !cleared.contains(id))
    {
        return Err(ContractViolation::SilentContradiction(id.clone()));
    }
    Ok(())
}

/// `item_ids_unique`: every item of `challenge` has a non-empty id that
/// no earlier item of the run (`items_seen`) or of this challenge uses.
pub fn validate_challenge(
    items_seen: &[String],
    challenge: &Challenge,
) -> Result<(), ContractViolation> {
    for (n, item) in challenge.items.iter().enumerate() {
        if item.id.trim().is_empty() {
            return Err(ContractViolation::EmptyItemId { index: n + 1 });
        }
        if items_seen.contains(&item.id) || challenge.items[..n].iter().any(|i| i.id == item.id) {
            return Err(ContractViolation::ReusedItemId(item.id.clone()));
        }
    }
    Ok(())
}

/// `claim_ids_unique`: no claim id repeats within `proposal`.
pub fn validate_claims(proposal: &Proposal) -> Result<(), ContractViolation> {
    for (n, claim) in proposal.claims.iter().enumerate() {
        if proposal.claims[..n].iter().any(|c| c.id == claim.id) {
            return Err(ContractViolation::DuplicateClaimId(claim.id.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::{assessment, claim, cleared, evaluation, judged, proposal, verdict, TS};

    /// Round 2 of a run: claims c1 and c2, item-1 carried open from round
    /// 1, item-2 raised now, ev-a and ev-b retrieved, ev-b contradicted in
    /// round 1.
    fn context() -> EvaluationContext {
        EvaluationContext {
            claims: vec![ClaimId::new("c1"), ClaimId::new("c2")],
            expected_items: vec!["item-1".into(), "item-2".into()],
            retrieved: vec![EvidenceId::new("ev-a"), EvidenceId::new("ev-b")],
            carried_contradictions: vec![EvidenceId::new("ev-b")],
        }
    }

    /// A round-2 evaluation that keeps every rule.
    fn valid() -> Evaluation {
        Evaluation {
            cleared_contradictions: vec![cleared("ev-b", "c2 no longer raises the cap")],
            ..judged(
                vec![
                    assessment("c1", true, &["ev-a"], &[]),
                    assessment("c2", false, &["ev-b"], &[]),
                ],
                vec![
                    verdict("item-1", ItemStatus::Resolved, "c2 now keeps the cap"),
                    verdict("item-2", ItemStatus::Unresolved, "no test yet"),
                ],
                false,
            )
        }
    }

    fn check(e: &Evaluation) -> Result<(), ContractViolation> {
        validate_evaluation(&context(), e)
    }

    #[test]
    fn an_evaluation_that_judges_everything_explicitly_passes() {
        check(&valid()).unwrap();
        // A carried contradiction may also be recorded again instead.
        let mut again = valid();
        again.cleared_contradictions.clear();
        again.assessments[1].contradicting_evidence = vec![EvidenceId::new("ev-b")];
        check(&again).unwrap();
        // Round 1 of a run with no claims and no items: nothing to judge.
        validate_evaluation(
            &EvaluationContext::default(),
            &evaluation(vec![], &[], true),
        )
        .unwrap();
    }

    #[test]
    fn claims_are_assessed_exactly_once_and_cite_only_retrieved_records() {
        let mut e = valid();
        e.assessments.pop();
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnassessedClaim(ClaimId::new("c2")))
        );
        let mut e = valid();
        e.assessments.push(assessment("c1", false, &[], &["ev-a"]));
        assert_eq!(
            check(&e),
            Err(ContractViolation::DuplicateAssessment(ClaimId::new("c1")))
        );
        let mut e = valid();
        e.assessments[0].claim_id = ClaimId::new("item-1");
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnknownClaim(ClaimId::new("item-1")))
        );
        let mut e = valid();
        e.assessments[0].relevant_evidence =
            vec![EvidenceId::new("ev-a"), EvidenceId::new("ev-hallucinated")];
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnretrievedEvidence {
                claim: ClaimId::new("c1"),
                field: "relevant_evidence",
                evidence: EvidenceId::new("ev-hallucinated"),
            })
        );
        let mut e = valid();
        e.assessments[0].contradicting_evidence = vec![EvidenceId::new("EV-A")];
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnretrievedEvidence {
                claim: ClaimId::new("c1"),
                field: "contradicting_evidence",
                evidence: EvidenceId::new("EV-A"),
            })
        );
    }

    #[test]
    fn every_expected_item_gets_exactly_one_verdict() {
        for variant in ["Item-1", "item 1", "chal-6/item-1", "c1"] {
            let mut e = valid();
            e.item_verdicts[0].item_id = variant.into();
            assert_eq!(
                check(&e),
                Err(ContractViolation::UnknownItem(variant.into())),
                "{variant}"
            );
        }
        let mut e = valid();
        e.item_verdicts.remove(0);
        assert_eq!(
            check(&e),
            Err(ContractViolation::MissingVerdict("item-1".into()))
        );
        let mut e = valid();
        e.item_verdicts.push(verdict(
            "item-1",
            ItemStatus::Unresolved,
            "on second thought",
        ));
        assert_eq!(
            check(&e),
            Err(ContractViolation::DuplicateVerdict("item-1".into()))
        );
        for reason in ["", "  \n"] {
            let mut e = valid();
            e.item_verdicts[0].reason = reason.into();
            assert_eq!(
                check(&e),
                Err(ContractViolation::ResolvedWithoutReason("item-1".into()))
            );
        }
        // An unresolved verdict needs no reason: it closes nothing.
        let mut e = valid();
        e.item_verdicts[1].reason.clear();
        check(&e).unwrap();
    }

    #[test]
    fn unresolved_items_mirror_the_unresolved_verdicts() {
        let mut e = valid();
        e.unresolved_items.clear();
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnresolvedNotListed("item-2".into()))
        );
        let mut e = valid();
        e.unresolved_items.push("item-1".into());
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnresolvedWithoutVerdict("item-1".into()))
        );
        let mut e = valid();
        e.unresolved_items.push("item-2".into());
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnresolvedListedTwice("item-2".into()))
        );
        let mut e = valid();
        e.unresolved_items.push("Item-2".into());
        assert_eq!(
            check(&e),
            Err(ContractViolation::UnresolvedWithoutVerdict("Item-2".into()))
        );
    }

    #[test]
    fn a_carried_contradiction_is_never_cleared_by_silence() {
        let mut e = valid();
        e.cleared_contradictions.clear();
        assert_eq!(
            check(&e),
            Err(ContractViolation::SilentContradiction(EvidenceId::new(
                "ev-b"
            )))
        );
        let mut e = valid();
        e.cleared_contradictions[0].reason = " ".into();
        assert_eq!(
            check(&e),
            Err(ContractViolation::ClearedWithoutReason(EvidenceId::new(
                "ev-b"
            )))
        );
        let mut e = valid();
        e.cleared_contradictions
            .push(cleared("ev-a", "never contradicted"));
        assert_eq!(
            check(&e),
            Err(ContractViolation::ClearedNotCarried(EvidenceId::new(
                "ev-a"
            )))
        );
        let mut e = valid();
        e.cleared_contradictions.push(cleared("ev-b", "twice"));
        assert_eq!(
            check(&e),
            Err(ContractViolation::ClearedTwice(EvidenceId::new("ev-b")))
        );
        let mut e = valid();
        e.assessments[1].contradicting_evidence = vec![EvidenceId::new("ev-b")];
        assert_eq!(
            check(&e),
            Err(ContractViolation::ClearedAndContradicting(EvidenceId::new(
                "ev-b"
            )))
        );
    }

    #[test]
    fn item_ids_are_non_empty_and_run_unique_and_claim_ids_unique() {
        let item = |id: &str| ChallengeItem {
            id: id.into(),
            kind: ChallengeKind::MissingTest,
            claim_id: None,
            description: "No test covers the jitter requirement".into(),
            evidence_refs: vec![],
        };
        let challenge = |items| Challenge {
            schema_version: SCHEMA_VERSION.into(),
            id: ChallengeId::new("chal-2"),
            run_id: RunId::new("run-1"),
            target_proposal: ProposalId::new("p2"),
            items,
            created_at: TS.into(),
        };
        let seen = vec!["item-1".to_string()];
        validate_challenge(&seen, &challenge(vec![item("item-2"), item("item-3")])).unwrap();
        assert_eq!(
            validate_challenge(&seen, &challenge(vec![item("item-2"), item(" ")])),
            Err(ContractViolation::EmptyItemId { index: 2 })
        );
        assert_eq!(
            validate_challenge(&seen, &challenge(vec![item("item-1")])),
            Err(ContractViolation::ReusedItemId("item-1".into()))
        );
        assert_eq!(
            validate_challenge(&seen, &challenge(vec![item("item-2"), item("item-2")])),
            Err(ContractViolation::ReusedItemId("item-2".into()))
        );

        let p = |ids: &[&str]| {
            proposal(
                "p1",
                "s",
                ids.iter()
                    .map(|id| claim(id, "Retries are capped"))
                    .collect(),
            )
        };
        validate_claims(&p(&["c1", "c2"])).unwrap();
        assert_eq!(
            validate_claims(&p(&["c1", "c2", "c1"])),
            Err(ContractViolation::DuplicateClaimId(ClaimId::new("c1")))
        );
    }

    #[test]
    fn the_contract_binds_schema_1_3_and_later_only() {
        for old in ["1.0", "1.1", "1.2"] {
            assert!(!binds(old), "{old}");
        }
        for bound in [
            SCHEMA_VERSION,
            "1.3",
            "1.4",
            "1.10",
            "2.0",
            "1.3.1",
            "x",
            "",
        ] {
            assert!(binds(bound), "{bound:?}");
        }
    }

    #[test]
    fn every_violation_message_names_its_rule_and_value() {
        let all = [
            ContractViolation::UnassessedClaim(ClaimId::new("c9")),
            ContractViolation::DuplicateAssessment(ClaimId::new("c9")),
            ContractViolation::UnknownClaim(ClaimId::new("c9")),
            ContractViolation::UnretrievedEvidence {
                claim: ClaimId::new("c9"),
                field: "relevant_evidence",
                evidence: EvidenceId::new("v9"),
            },
            ContractViolation::MissingVerdict("v9".into()),
            ContractViolation::DuplicateVerdict("v9".into()),
            ContractViolation::UnknownItem("v9".into()),
            ContractViolation::ResolvedWithoutReason("v9".into()),
            ContractViolation::UnresolvedWithoutVerdict("v9".into()),
            ContractViolation::UnresolvedListedTwice("v9".into()),
            ContractViolation::UnresolvedNotListed("v9".into()),
            ContractViolation::SilentContradiction(EvidenceId::new("v9")),
            ContractViolation::ClearedWithoutReason(EvidenceId::new("v9")),
            ContractViolation::ClearedNotCarried(EvidenceId::new("v9")),
            ContractViolation::ClearedTwice(EvidenceId::new("v9")),
            ContractViolation::ClearedAndContradicting(EvidenceId::new("v9")),
            ContractViolation::EmptyItemId { index: 9 },
            ContractViolation::ReusedItemId("v9".into()),
            ContractViolation::DuplicateClaimId(ClaimId::new("c9")),
        ];
        for v in all {
            let message = v.to_string();
            assert!(message.starts_with(&format!("{}: ", v.rule())), "{message}");
            assert!(message.contains('9'), "{message}");
        }
    }
}
