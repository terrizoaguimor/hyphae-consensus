// SPDX-License-Identifier: Apache-2.0
//! Shared deterministic scenarios used by integration tests and the M1
//! demo binary. The reference scenario mirrors the planned hackathon
//! demonstration: a proposer recommends a change, the critic finds a
//! documented constraint it violates, the evaluator confirms the
//! contradiction from retrieved evidence, and the proposer revises.
//!
//! Every scripted evaluation keeps the protocol 1.3 contract
//! ([`crate::contract`]) on its own: it assesses each claim once, cites
//! only records the run retrieves, and gives an explicit verdict, with a
//! reason when resolved, on every objection under evaluation, carried
//! ones included. The fake model returns these documents verbatim.

use crate::fakes::{FakeMemoryClient, FakeModelClient, FakeProofVerifier};
use crate::orchestrator::TaskInput;
use consensus_domain::*;

pub const DEMO_PROJECT: &str = "demo-repo";
pub const TS: &str = "2026-10-26T09:00:00Z";

/// A seeded record, labeled `retrieval: simulated` (schema 1.3).
pub fn evidence(id: &str, project: &str, kind: &str, content: &str) -> EvidenceReference {
    EvidenceReference {
        schema_version: SCHEMA_VERSION.into(),
        id: EvidenceId::new(id),
        project_id: ProjectId::new(project),
        backend_ref: Some(format!("fake-memory:{id}")),
        kind: kind.into(),
        content: content.into(),
        retrieved_at: TS.into(),
        anchor: Some("fake-anchor-1".into()),
        proof_file: None,
        witness_file: None,
        proof_error: None,
        retrieval: Some(Retrieval::Simulated),
        snapshot_id: None,
        captured_at: None,
        proof_ref: None,
        proof_sha256: None,
        proof_request: None,
        backend_project: None,
        backend_scope: None,
    }
}

pub fn claim(id: &str, statement: &str) -> Claim {
    Claim {
        id: ClaimId::new(id),
        statement: statement.into(),
        evidence_refs: vec![],
    }
}

pub fn proposal(id: &str, summary: &str, claims: Vec<Claim>) -> Proposal {
    Proposal {
        schema_version: SCHEMA_VERSION.into(),
        id: ProposalId::new(id),
        run_id: RunId::new("placeholder"),
        author_role: AgentRole::Proposer,
        summary: summary.into(),
        plan_steps: vec![
            "Apply the change".into(),
            "Run the existing test suite".into(),
        ],
        claims,
        assumptions: vec!["Upstream services tolerate the proposed change".into()],
        uncertainties: vec!["No load-test data available".into()],
        created_at: TS.into(),
    }
}

pub fn assessment(
    claim_id: &str,
    corroborated: bool,
    relevant: &[&str],
    contradicting: &[&str],
) -> ClaimAssessment {
    ClaimAssessment {
        claim_id: ClaimId::new(claim_id),
        corroborated,
        relevant_evidence: relevant.iter().map(|s| EvidenceId::new(*s)).collect(),
        contradicting_evidence: contradicting.iter().map(|s| EvidenceId::new(*s)).collect(),
        note: "scripted assessment".into(),
    }
}

/// An evaluation with no item verdicts: under protocol 1.3 only valid
/// when no objection is under evaluation (and then `unresolved` is
/// empty). Use [`judged`] whenever there are items to judge.
pub fn evaluation(
    assessments: Vec<ClaimAssessment>,
    unresolved: &[&str],
    abstain: bool,
) -> Evaluation {
    Evaluation {
        schema_version: SCHEMA_VERSION.into(),
        run_id: RunId::new("placeholder"),
        target_proposal: ProposalId::new("placeholder"),
        assessments,
        unresolved_items: unresolved.iter().map(|s| s.to_string()).collect(),
        item_verdicts: vec![],
        cleared_contradictions: vec![],
        abstain,
        abstain_reason: abstain.then(|| "evidence is insufficient to judge".to_string()),
        created_at: TS.into(),
    }
}

/// One scripted verdict on one objection.
pub fn verdict(item_id: &str, status: ItemStatus, reason: &str) -> ItemVerdict {
    ItemVerdict {
        item_id: item_id.into(),
        status,
        reason: reason.into(),
    }
}

/// One scripted clearing of a carried contradiction, with its reason.
pub fn cleared(evidence_id: &str, reason: &str) -> ClearedContradiction {
    ClearedContradiction {
        evidence_id: EvidenceId::new(evidence_id),
        reason: reason.into(),
    }
}

/// An evaluation that states a verdict on each objection; its
/// `unresolved_items` are the `unresolved` verdicts, in verdict order.
/// Nothing is synthesized: every verdict comes from the script.
pub fn judged(
    assessments: Vec<ClaimAssessment>,
    verdicts: Vec<ItemVerdict>,
    abstain: bool,
) -> Evaluation {
    let mut judged = evaluation(assessments, &[], abstain);
    judged.unresolved_items = verdicts
        .iter()
        .filter(|v| v.status == ItemStatus::Unresolved)
        .map(|v| v.item_id.clone())
        .collect();
    judged.item_verdicts = verdicts;
    judged
}

pub struct Scenario {
    pub model: FakeModelClient,
    pub memory: FakeMemoryClient,
    pub verifier: FakeProofVerifier,
    pub input: TaskInput,
}

fn retry_input() -> TaskInput {
    TaskInput {
        project_id: ProjectId::new(DEMO_PROJECT),
        title: "Fix payment client timeouts".into(),
        description: "Payment requests intermittently time out under load. Propose a fix.".into(),
        submitted_by: "mario".into(),
        mode: None,
    }
}

/// The reference task's first proposal: it raises retries past the cap.
fn retry_first_proposal() -> Proposal {
    proposal(
        "prop-1",
        "Raise the payment client retry limit to fix timeouts",
        vec![
            claim(
                "claim-1",
                "Payment timeouts are caused by insufficient retries",
            ),
            claim("claim-2", "Raising the retry limit to 10 attempts is safe"),
        ],
    )
}

/// The reference task's revision: it keeps the documented cap.
fn retry_revision() -> Revision {
    Revision {
        schema_version: SCHEMA_VERSION.into(),
        proposal: proposal(
            "prop-2",
            "Cap retries at 3 attempts with jitter per the documented constraint",
            vec![
                claim(
                    "claim-1",
                    "Payment timeouts correlate with retry bursts above 5 attempts",
                ),
                claim(
                    "claim-2",
                    "Capping retries at 3 attempts with jitter matches the documented constraint",
                ),
            ],
        ),
        supersedes: ProposalId::new("placeholder"),
        addressed_items: vec!["item-1".into(), "item-2".into()],
    }
}

fn challenge(id: &str, items: Vec<ChallengeItem>) -> Challenge {
    Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new(id),
        run_id: RunId::new("placeholder"),
        target_proposal: ProposalId::new("placeholder"),
        items,
        created_at: TS.into(),
    }
}

/// Proposer violates a stored constraint; critic catches it; revision
/// complies; the second evaluation resolves both carried objections and
/// clears the carried contradiction, each with its reason, and
/// corroborates. Expected: Supported.
pub fn retry_constraint_scenario() -> Scenario {
    let constraint = evidence(
        "ev-constraint", DEMO_PROJECT, "constraint",
        "Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage.",
    );
    let fact = evidence(
        "ev-fact",
        DEMO_PROJECT,
        "fact",
        "Fact: payment client timeouts correlate with retry bursts above 5 attempts.",
    );

    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(retry_first_proposal()));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![
            ChallengeItem {
                id: "item-1".into(),
                kind: ChallengeKind::ConstraintConflict,
                claim_id: Some(ClaimId::new("claim-2")),
                description: "Documented constraint caps retries at 3 attempts".into(),
                evidence_refs: vec![EvidenceId::new("ev-constraint")],
            },
            ChallengeItem {
                id: "item-2".into(),
                kind: ChallengeKind::UnsupportedClaim,
                claim_id: Some(ClaimId::new("claim-1")),
                description: "No evidence ties timeouts to insufficient retries".into(),
                evidence_refs: vec![],
            },
        ],
    )));
    model.evaluations.push_back(Ok(judged(
        vec![
            assessment("claim-1", false, &["ev-fact"], &[]),
            assessment("claim-2", false, &["ev-constraint"], &["ev-constraint"]),
        ],
        vec![
            verdict(
                "item-1",
                ItemStatus::Unresolved,
                "ev-constraint caps retries at 3 attempts; the proposal raises them to 10",
            ),
            verdict(
                "item-2",
                ItemStatus::Unresolved,
                "ev-fact ties timeouts to retry bursts, not to too few retries",
            ),
        ],
        false,
    )));
    model.revisions.push_back(Ok(retry_revision()));
    model.critiques.push_back(Ok(challenge("chal-2", vec![])));
    model.evaluations.push_back(Ok(Evaluation {
        cleared_contradictions: vec![cleared(
            "ev-constraint",
            "the revision caps retries at 3 attempts with jitter, as the constraint requires",
        )],
        ..judged(
            vec![
                assessment("claim-1", true, &["ev-fact"], &[]),
                assessment("claim-2", true, &["ev-constraint"], &[]),
            ],
            vec![
                verdict(
                    "item-1",
                    ItemStatus::Resolved,
                    "claim-2 now caps retries at 3 attempts with jitter",
                ),
                verdict(
                    "item-2",
                    ItemStatus::Resolved,
                    "claim-1 now states the correlation ev-fact records",
                ),
            ],
            false,
        )
    }));

    Scenario {
        model,
        memory: FakeMemoryClient::new(vec![constraint, fact]),
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        input: retry_input(),
    }
}

/// The reference task with the memory backend offline. Nothing can be
/// retrieved, so nothing is cited: protocol 1.3 refuses a citation of a
/// record the run never retrieved. The Critic's two objections are about
/// the plan; the revision answers them and the second evaluation resolves
/// both, with reasons, but no claim can be corroborated. Expected:
/// InsufficientEvidence after one revision (6 model calls).
pub fn offline_scenario() -> Scenario {
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(retry_first_proposal()));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![
            ChallengeItem {
                id: "item-1".into(),
                kind: ChallengeKind::UnsupportedClaim,
                claim_id: Some(ClaimId::new("claim-2")),
                description: "No retrieved record shows that 10 retry attempts are safe".into(),
                evidence_refs: vec![],
            },
            ChallengeItem {
                id: "item-2".into(),
                kind: ChallengeKind::UnsupportedClaim,
                claim_id: Some(ClaimId::new("claim-1")),
                description: "No retrieved record ties the timeouts to too few retries".into(),
                evidence_refs: vec![],
            },
        ],
    )));
    model.evaluations.push_back(Ok(judged(
        vec![
            assessment("claim-1", false, &[], &[]),
            assessment("claim-2", false, &[], &[]),
        ],
        vec![
            verdict(
                "item-1",
                ItemStatus::Unresolved,
                "memory is offline: no record could be retrieved to settle it",
            ),
            verdict(
                "item-2",
                ItemStatus::Unresolved,
                "memory is offline: no record could be retrieved to settle it",
            ),
        ],
        false,
    )));
    model.revisions.push_back(Ok(retry_revision()));
    model.critiques.push_back(Ok(challenge("chal-2", vec![])));
    model.evaluations.push_back(Ok(judged(
        vec![
            assessment("claim-1", false, &[], &[]),
            assessment("claim-2", false, &[], &[]),
        ],
        vec![
            verdict(
                "item-1",
                ItemStatus::Resolved,
                "the revision no longer raises the attempt limit",
            ),
            verdict(
                "item-2",
                ItemStatus::Resolved,
                "the revision no longer claims a cause; claim-1 states a correlation",
            ),
        ],
        false,
    )));

    Scenario {
        model,
        memory: FakeMemoryClient::offline(),
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        input: retry_input(),
    }
}

/// The reference task for the single-agent baseline: the consensus
/// scenario's memory and verifier, and one scripted document in which the
/// agent proposes raising retries to 10, sees the stored constraint that
/// contradicts it, raises the conflict as an open objection, and still
/// calls its proposal supported: the case the baseline comparison counts
/// as "approved what the consensus caught".
pub struct BaselineScenario {
    pub model: crate::fakes::ScriptedBaselineModel,
    pub memory: FakeMemoryClient,
    pub verifier: FakeProofVerifier,
    pub input: TaskInput,
}

/// The scripted single-agent document of [`baseline_scenario`].
pub fn baseline_document() -> crate::baseline::BaselineAssessment {
    use crate::baseline::{AgentVerdict, BaselineAssessment, BaselineObjection, ObjectionStatus};
    BaselineAssessment {
        schema_version: crate::baseline::BASELINE_SCHEMA_VERSION.into(),
        id: "assess-1".into(),
        run_id: RunId::new("placeholder"),
        summary: "Raise the payment client retry limit to 10 attempts".into(),
        plan_steps: vec!["Set max_attempts to 10".into()],
        claims: vec![
            claim(
                "claim-1",
                "Payment timeouts correlate with retry bursts above 5 attempts",
            ),
            claim("claim-2", "Raising the retry limit to 10 attempts is safe"),
        ],
        assumptions: vec!["Upstream services tolerate more retries".into()],
        uncertainties: vec![],
        assessments: vec![
            assessment("claim-1", true, &["ev-fact"], &[]),
            assessment("claim-2", false, &[], &["ev-constraint"]),
        ],
        objections: vec![BaselineObjection {
            id: "obj-1".into(),
            kind: ChallengeKind::ConstraintConflict,
            claim_id: Some(ClaimId::new("claim-2")),
            description: "The stored constraint caps retries at 3 attempts".into(),
            evidence_refs: vec![EvidenceId::new("ev-constraint")],
            status: ObjectionStatus::Open,
        }],
        verdict: AgentVerdict::Supported,
        verdict_reason: "More retries ride out transient upstream failures".into(),
        created_at: TS.into(),
    }
}

/// [`retry_constraint_scenario`]'s task, memory and verifier with the
/// scripted single agent of [`baseline_document`].
pub fn baseline_scenario() -> BaselineScenario {
    let s = retry_constraint_scenario();
    let mut model = crate::fakes::ScriptedBaselineModel::new();
    model.assessments.push_back(Ok(baseline_document()));
    BaselineScenario {
        model,
        memory: s.memory,
        verifier: s.verifier,
        input: s.input,
    }
}
