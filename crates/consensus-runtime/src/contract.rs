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
//! On replay the records the runtime itself derives in a 1.3 round are
//! held to the run's record as well, so a forged verification fails
//! replay too. Only the integrity status comes from outside the record
//! (the proof verifier), so only it is taken as written:
//! - `verification_derived` ([`validate_verification`]): an
//!   `EvidenceVerified` recorded after the round's evaluation verifies a
//!   record the run retrieved, once in the round, is tagged with the
//!   round, and states the factual and relevance status that evaluation
//!   gives the record ([`verification_statuses`]);
//! - `round_fully_verified` ([`validate_round_verified`]): the
//!   recommendation follows a verification of every retrieved record, so
//!   none can be left out of the policy's inputs.
//!
//! Records before schema 1.3 predate the contract and are never checked
//! against it ([`binds`]): they replay exactly as they always did. A
//! run's records never go back to an earlier schema version
//! ([`schema_rank`]): the state machine refuses a downgrade, so no single
//! record of a 1.3 run can be relabeled out of these checks.
//!
//! Every value a violation message quotes is cut at [`MAX_QUOTED_CHARS`]
//! characters ([`quote`]): the message is persisted as `RunFailed.reason`
//! in the hash-chained log, which is exported verbatim, so its length
//! never follows the length of a model's output.

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
/// starts with the rule's name ([`rule`](Self::rule)) and quotes each
/// value through [`quote`], so it stays bounded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContractViolation {
    #[error("one_assessment_per_claim: claim {} has no assessment", quote(.0.as_str()))]
    UnassessedClaim(ClaimId),
    #[error(
        "one_assessment_per_claim: claim {} is assessed more than once",
        quote(.0.as_str())
    )]
    DuplicateAssessment(ClaimId),
    #[error(
        "one_assessment_per_claim: {} is not a claim of the proposal under evaluation",
        quote(.0.as_str())
    )]
    UnknownClaim(ClaimId),
    #[error(
        "cited_evidence_retrieved: {field} of claim {} names {}, a record this run never retrieved",
        quote(.claim.as_str()),
        quote(.evidence.as_str())
    )]
    UnretrievedEvidence {
        claim: ClaimId,
        /// `relevant_evidence` or `contradicting_evidence`.
        field: &'static str,
        evidence: EvidenceId,
    },
    #[error("one_verdict_per_item: item {} has no verdict", quote(.0))]
    MissingVerdict(String),
    #[error("one_verdict_per_item: item {} has more than one verdict", quote(.0))]
    DuplicateVerdict(String),
    #[error(
        "one_verdict_per_item: {} is not an item under evaluation (open from an earlier round or raised in this one)",
        quote(.0)
    )]
    UnknownItem(String),
    #[error(
        "resolved_needs_reason: item {} is resolved without a reason",
        quote(.0)
    )]
    ResolvedWithoutReason(String),
    #[error(
        "unresolved_matches_verdicts: unresolved_items lists {}, which has no unresolved verdict",
        quote(.0)
    )]
    UnresolvedWithoutVerdict(String),
    #[error(
        "unresolved_matches_verdicts: unresolved_items lists {} more than once",
        quote(.0)
    )]
    UnresolvedListedTwice(String),
    #[error(
        "unresolved_matches_verdicts: item {} has an unresolved verdict but is missing from unresolved_items",
        quote(.0)
    )]
    UnresolvedNotListed(String),
    #[error(
        "carried_contradiction_judged: {}, contradicted in an earlier round, is neither listed as contradicting again nor cleared",
        quote(.0.as_str())
    )]
    SilentContradiction(EvidenceId),
    #[error(
        "carried_contradiction_judged: {} is cleared without a reason",
        quote(.0.as_str())
    )]
    ClearedWithoutReason(EvidenceId),
    #[error(
        "cleared_is_carried: {} is not a contradiction carried from an earlier round",
        quote(.0.as_str())
    )]
    ClearedNotCarried(EvidenceId),
    #[error("cleared_is_carried: {} is cleared more than once", quote(.0.as_str()))]
    ClearedTwice(EvidenceId),
    #[error(
        "cleared_is_carried: {} is cleared and listed as contradicting in the same evaluation",
        quote(.0.as_str())
    )]
    ClearedAndContradicting(EvidenceId),
    #[error("item_ids_unique: challenge item {index} has an empty id")]
    EmptyItemId {
        /// 1-based position of the item in its challenge.
        index: usize,
    },
    #[error("item_ids_unique: item id {} is already used in this run", quote(.0))]
    ReusedItemId(String),
    #[error(
        "claim_ids_unique: claim id {} appears more than once in the proposal",
        quote(.0.as_str())
    )]
    DuplicateClaimId(ClaimId),
    #[error(
        "verification_derived: {} is verified, but this run never retrieved it",
        quote(.0.as_str())
    )]
    VerifiedUnretrieved(EvidenceId),
    #[error(
        "verification_derived: {} is verified more than once in the round",
        quote(.0.as_str())
    )]
    VerifiedTwice(EvidenceId),
    #[error(
        "verification_derived: the verification of {} is tagged round {stored:?}, but it is recorded in round {expected}",
        quote(.evidence.as_str())
    )]
    VerificationRound {
        evidence: EvidenceId,
        stored: Option<u32>,
        expected: u32,
    },
    #[error(
        "verification_derived: the verification of {} records {stored:?}, but the round's evaluation gives it {derived:?}",
        quote(.evidence.as_str())
    )]
    VerificationStatus {
        evidence: EvidenceId,
        /// (factual, relevance) as recorded.
        stored: (FactualStatus, RelevanceStatus),
        /// (factual, relevance) as [`verification_statuses`] derives them.
        derived: (FactualStatus, RelevanceStatus),
    },
    #[error(
        "round_fully_verified: {}, retrieved in this run, has no verification in the round the recommendation closes",
        quote(.0.as_str())
    )]
    UnverifiedRecord(EvidenceId),
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
            Self::VerifiedUnretrieved(_)
            | Self::VerifiedTwice(_)
            | Self::VerificationRound { .. }
            | Self::VerificationStatus { .. } => "verification_derived",
            Self::UnverifiedRecord(_) => "round_fully_verified",
        }
    }
}

/// Longest value, in characters, that a violation message quotes. The
/// values are model-supplied (or backend-supplied) and the message is
/// persisted in the hash-chained log.
pub const MAX_QUOTED_CHARS: usize = 80;

/// `value` quoted the way `{:?}` quotes it, with anything past
/// [`MAX_QUOTED_CHARS`] characters cut and marked `…[truncated]` after
/// the closing quote. Short values quote exactly as `{:?}` would.
pub fn quote(value: &str) -> String {
    if value.chars().count() <= MAX_QUOTED_CHARS {
        format!("{value:?}")
    } else {
        let cut: String = value.chars().take(MAX_QUOTED_CHARS).collect();
        format!("{cut:?}…[truncated]")
    }
}

/// Order of schema versions: `(major, minor)` for a `major.minor`
/// version; any other string ranks after every such version (as
/// [`binds`] reads it), so an unknown record is checked rather than
/// trusted. The state machine refuses a record that ranks below one the
/// run already holds.
pub fn schema_rank(schema_version: &str) -> (u32, u32) {
    let mut parts = schema_version.split('.').map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten(), parts.next()) {
        (Some(major), Some(minor), None) => (major, minor),
        _ => (u32::MAX, u32::MAX),
    }
}

/// True when a record of `schema_version` is bound by the protocol 1.3
/// contract: schema 1.3 or later. A version that does not parse as
/// `major.minor` counts as later, so an unknown record is checked rather
/// than trusted (the event log refuses such records anyway).
pub fn binds(schema_version: &str) -> bool {
    schema_rank(schema_version) >= (1, 3)
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

/// The factual and relevance status `evaluation` gives the record
/// `evidence`, as a protocol 1.3 `EvidenceVerified` records them next to
/// the proof check's integrity status. Factual: contradicted when some
/// assessment lists the record as contradicting; otherwise corroborated
/// when a corroborated claim cites it; otherwise uncorroborated when any
/// claim cites it; otherwise not assessed. Relevance: relevant when it is
/// cited or contradicting at all, irrelevant otherwise. The orchestrator
/// derives each verification with this function and replay re-derives it
/// with the same one.
pub fn verification_statuses(
    evaluation: &Evaluation,
    evidence: &EvidenceId,
) -> (FactualStatus, RelevanceStatus) {
    let relevant = evaluation
        .assessments
        .iter()
        .any(|a| a.relevant_evidence.contains(evidence));
    let contradicted = evaluation
        .assessments
        .iter()
        .any(|a| a.contradicting_evidence.contains(evidence));
    let corroborating = evaluation
        .assessments
        .iter()
        .any(|a| a.corroborated && a.relevant_evidence.contains(evidence));
    let factual = if contradicted {
        FactualStatus::Contradicted
    } else if corroborating {
        FactualStatus::Corroborated
    } else if relevant {
        FactualStatus::Uncorroborated
    } else {
        FactualStatus::NotAssessed
    };
    let relevance = if relevant || contradicted {
        RelevanceStatus::Relevant
    } else {
        RelevanceStatus::Irrelevant
    };
    (factual, relevance)
}

/// `verification_derived`: `verification`, recorded in `round` after the
/// round's `evaluation`, verifies a record of `retrieved` (every record
/// the run retrieved) that is not yet in `verified` (the verifications
/// recorded since that evaluation), carries the round's number, and
/// states the statuses [`verification_statuses`] derives. The integrity
/// status is the proof verifier's and is taken as written.
pub fn validate_verification(
    round: u32,
    retrieved: &[EvidenceReference],
    verified: &[EvidenceVerification],
    evaluation: &Evaluation,
    verification: &EvidenceVerification,
) -> Result<(), ContractViolation> {
    let id = &verification.evidence_id;
    if !retrieved.iter().any(|e| &e.id == id) {
        return Err(ContractViolation::VerifiedUnretrieved(id.clone()));
    }
    if verified.iter().any(|v| &v.evidence_id == id) {
        return Err(ContractViolation::VerifiedTwice(id.clone()));
    }
    if verification.round != Some(round) {
        return Err(ContractViolation::VerificationRound {
            evidence: id.clone(),
            stored: verification.round,
            expected: round,
        });
    }
    let stored = (verification.factual_status, verification.relevance_status);
    let derived = verification_statuses(evaluation, id);
    if stored != derived {
        return Err(ContractViolation::VerificationStatus {
            evidence: id.clone(),
            stored,
            derived,
        });
    }
    Ok(())
}

/// `round_fully_verified`: every record of `retrieved` has a verification
/// in `verified` (those recorded since the round's evaluation), so the
/// policy reads the whole round context. Together with
/// [`validate_verification`] this makes `verified` exactly one
/// verification per retrieved record.
pub fn validate_round_verified(
    retrieved: &[EvidenceReference],
    verified: &[EvidenceVerification],
) -> Result<(), ContractViolation> {
    match retrieved
        .iter()
        .find(|e| !verified.iter().any(|v| v.evidence_id == e.id))
    {
        Some(e) => Err(ContractViolation::UnverifiedRecord(e.id.clone())),
        None => Ok(()),
    }
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

    /// One violation of every kind, each quoting `value` wherever it
    /// quotes a model- or backend-supplied value.
    fn every_violation(value: &str) -> Vec<ContractViolation> {
        let claim = || ClaimId::new(value);
        let record = || EvidenceId::new(value);
        vec![
            ContractViolation::UnassessedClaim(claim()),
            ContractViolation::DuplicateAssessment(claim()),
            ContractViolation::UnknownClaim(claim()),
            ContractViolation::UnretrievedEvidence {
                claim: claim(),
                field: "relevant_evidence",
                evidence: record(),
            },
            ContractViolation::MissingVerdict(value.into()),
            ContractViolation::DuplicateVerdict(value.into()),
            ContractViolation::UnknownItem(value.into()),
            ContractViolation::ResolvedWithoutReason(value.into()),
            ContractViolation::UnresolvedWithoutVerdict(value.into()),
            ContractViolation::UnresolvedListedTwice(value.into()),
            ContractViolation::UnresolvedNotListed(value.into()),
            ContractViolation::SilentContradiction(record()),
            ContractViolation::ClearedWithoutReason(record()),
            ContractViolation::ClearedNotCarried(record()),
            ContractViolation::ClearedTwice(record()),
            ContractViolation::ClearedAndContradicting(record()),
            ContractViolation::EmptyItemId { index: 9 },
            ContractViolation::ReusedItemId(value.into()),
            ContractViolation::DuplicateClaimId(claim()),
            ContractViolation::VerifiedUnretrieved(record()),
            ContractViolation::VerifiedTwice(record()),
            ContractViolation::VerificationRound {
                evidence: record(),
                stored: Some(9),
                expected: 2,
            },
            ContractViolation::VerificationStatus {
                evidence: record(),
                stored: (FactualStatus::Corroborated, RelevanceStatus::Relevant),
                derived: (FactualStatus::Contradicted, RelevanceStatus::Relevant),
            },
            ContractViolation::UnverifiedRecord(record()),
        ]
    }

    #[test]
    fn every_violation_message_names_its_rule_and_value() {
        for v in every_violation("v9") {
            let message = v.to_string();
            assert!(message.starts_with(&format!("{}: ", v.rule())), "{message}");
            assert!(message.contains('9'), "{message}");
        }
    }

    #[test]
    fn quoted_values_are_bounded_whatever_the_model_sends() {
        // Short values quote exactly as `{:?}` does.
        assert_eq!(quote("item-5"), "\"item-5\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        let at_cap = "x".repeat(MAX_QUOTED_CHARS);
        assert_eq!(quote(&at_cap), format!("{at_cap:?}"));
        // A 10 KB id is cut after the cap, and says so.
        let long = "x".repeat(10 * 1024);
        assert_eq!(
            quote(&long),
            format!("{:?}…[truncated]", "x".repeat(MAX_QUOTED_CHARS))
        );
        for v in every_violation(&long) {
            let message = v.to_string();
            assert!(message.len() < 400, "{} bytes: {message}", message.len());
            assert!(message.starts_with(&format!("{}: ", v.rule())), "{message}");
        }
    }

    #[test]
    fn schema_versions_rank_by_major_then_minor_with_unknown_last() {
        let ranked = ["1.0", "1.1", "1.2", "1.3", "1.10", "2.0", "x"];
        for pair in ranked.windows(2) {
            assert!(schema_rank(pair[0]) < schema_rank(pair[1]), "{pair:?}");
        }
        assert_eq!(schema_rank(SCHEMA_VERSION), (1, 3));
        for unknown in ["", "1.3.1", "1", "1.x", "v1.3"] {
            assert_eq!(schema_rank(unknown), schema_rank("x"), "{unknown:?}");
        }
    }

    fn verification_of(
        id: &str,
        round: Option<u32>,
        factual: FactualStatus,
        relevance: RelevanceStatus,
    ) -> EvidenceVerification {
        EvidenceVerification {
            schema_version: SCHEMA_VERSION.into(),
            evidence_id: EvidenceId::new(id),
            integrity_status: IntegrityStatus::Valid,
            factual_status: factual,
            relevance_status: relevance,
            verifier: Some("test".into()),
            verified_anchor: None,
            detail: None,
            checked_at: TS.into(),
            round,
            proof_ref: None,
            report: None,
        }
    }

    #[test]
    fn a_verification_is_exactly_what_the_rounds_evaluation_implies() {
        use crate::scenarios::evidence;
        // c1 is corroborated by ev-a; c2 cites ev-b and is contradicted by
        // ev-c; nothing mentions ev-d.
        let e = evaluation(
            vec![
                assessment("c1", true, &["ev-a"], &[]),
                assessment("c2", false, &["ev-b"], &["ev-c"]),
            ],
            &[],
            false,
        );
        use FactualStatus as F;
        use RelevanceStatus as R;
        for (id, expected) in [
            ("ev-a", (F::Corroborated, R::Relevant)),
            ("ev-b", (F::Uncorroborated, R::Relevant)),
            ("ev-c", (F::Contradicted, R::Relevant)),
            ("ev-d", (F::NotAssessed, R::Irrelevant)),
        ] {
            assert_eq!(
                verification_statuses(&e, &EvidenceId::new(id)),
                expected,
                "{id}"
            );
        }

        let retrieved: Vec<EvidenceReference> = ["ev-a", "ev-c"]
            .iter()
            .map(|id| evidence(id, "p", "fact", "Retries are capped"))
            .collect();
        let ok_a = verification_of("ev-a", Some(2), F::Corroborated, R::Relevant);
        let ok_c = verification_of("ev-c", Some(2), F::Contradicted, R::Relevant);
        validate_verification(2, &retrieved, &[], &e, &ok_a).unwrap();
        validate_verification(2, &retrieved, std::slice::from_ref(&ok_a), &e, &ok_c).unwrap();
        let check = |verified: &[EvidenceVerification], v: &EvidenceVerification| {
            validate_verification(2, &retrieved, verified, &e, v)
        };
        assert_eq!(
            check(
                &[],
                &verification_of("ev-d", Some(2), F::NotAssessed, R::Irrelevant)
            ),
            Err(ContractViolation::VerifiedUnretrieved(EvidenceId::new(
                "ev-d"
            )))
        );
        assert_eq!(
            check(std::slice::from_ref(&ok_a), &ok_a),
            Err(ContractViolation::VerifiedTwice(EvidenceId::new("ev-a")))
        );
        for round in [None, Some(1), Some(3)] {
            assert_eq!(
                check(
                    &[],
                    &verification_of("ev-a", round, F::Corroborated, R::Relevant)
                ),
                Err(ContractViolation::VerificationRound {
                    evidence: EvidenceId::new("ev-a"),
                    stored: round,
                    expected: 2,
                })
            );
        }
        // A contradiction recorded as corroboration, or as irrelevant.
        for stored in [
            (F::Corroborated, R::Relevant),
            (F::Contradicted, R::Irrelevant),
        ] {
            assert_eq!(
                check(&[], &verification_of("ev-c", Some(2), stored.0, stored.1)),
                Err(ContractViolation::VerificationStatus {
                    evidence: EvidenceId::new("ev-c"),
                    stored,
                    derived: (F::Contradicted, R::Relevant),
                })
            );
        }

        // The recommendation needs every retrieved record verified.
        validate_round_verified(&retrieved, &[ok_a.clone(), ok_c]).unwrap();
        validate_round_verified(&[], &[]).unwrap();
        assert_eq!(
            validate_round_verified(&retrieved, &[ok_a]),
            Err(ContractViolation::UnverifiedRecord(EvidenceId::new("ev-c")))
        );
    }
}
