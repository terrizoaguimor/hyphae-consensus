// SPDX-License-Identifier: Apache-2.0
//! Shared deterministic scenarios used by integration tests and the M1
//! demo binary. The reference scenario mirrors the planned hackathon
//! demonstration: a proposer recommends a change, the critic finds a
//! documented constraint it violates, the evaluator confirms the
//! contradiction from retrieved evidence, and the proposer revises.

use crate::fakes::{FakeMemoryClient, FakeModelClient, FakeProofVerifier};
use crate::orchestrator::TaskInput;
use consensus_domain::*;

pub const DEMO_PROJECT: &str = "demo-repo";
pub const TS: &str = "2026-10-26T09:00:00Z";

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
        abstain,
        abstain_reason: abstain.then(|| "evidence is insufficient to judge".to_string()),
        created_at: TS.into(),
    }
}

pub struct Scenario {
    pub model: FakeModelClient,
    pub memory: FakeMemoryClient,
    pub verifier: FakeProofVerifier,
    pub input: TaskInput,
}

/// Proposer violates a stored constraint; critic catches it; revision
/// complies; second evaluation corroborates. Expected: Supported.
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
    model.proposals.push_back(Ok(proposal(
        "prop-1",
        "Raise the payment client retry limit to fix timeouts",
        vec![
            claim(
                "claim-1",
                "Payment timeouts are caused by insufficient retries",
            ),
            claim("claim-2", "Raising the retry limit to 10 attempts is safe"),
        ],
    )));
    model.critiques.push_back(Ok(Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new("chal-1"),
        run_id: RunId::new("placeholder"),
        target_proposal: ProposalId::new("placeholder"),
        items: vec![
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
        created_at: TS.into(),
    }));
    model.evaluations.push_back(Ok(evaluation(
        vec![
            assessment("claim-1", false, &["ev-fact"], &[]),
            assessment("claim-2", false, &["ev-constraint"], &["ev-constraint"]),
        ],
        &["item-1", "item-2"],
        false,
    )));
    model.revisions.push_back(Ok(Revision {
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
    }));
    model.critiques.push_back(Ok(Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new("chal-2"),
        run_id: RunId::new("placeholder"),
        target_proposal: ProposalId::new("placeholder"),
        items: vec![],
        created_at: TS.into(),
    }));
    model.evaluations.push_back(Ok(evaluation(
        vec![
            assessment("claim-1", true, &["ev-fact"], &[]),
            assessment("claim-2", true, &["ev-constraint"], &[]),
        ],
        &[],
        false,
    )));

    Scenario {
        model,
        memory: FakeMemoryClient::new(vec![constraint, fact]),
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        input: TaskInput {
            project_id: ProjectId::new(DEMO_PROJECT),
            title: "Fix payment client timeouts".into(),
            description: "Payment requests intermittently time out under load. Propose a fix."
                .into(),
            submitted_by: "mario".into(),
        },
    }
}
