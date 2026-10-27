// SPDX-License-Identifier: Apache-2.0
//! Protocol 1.3 (schema 1.3): the fail-closed evaluation contract and
//! cross-round carry-forward, end to end with the scripted doubles, plus
//! the schema 1.3 serialization guarantees (LC-01). Deterministic fakes
//! only: no Meta API, no Hyphae runtime.

use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::contract::ContractViolation;
use consensus_runtime::fakes::*;
use consensus_runtime::orchestrator::{
    build_receipt, Orchestrator, TaskInput, MAX_EVIDENCE_PER_RUN,
};
use consensus_runtime::policy::PolicyVersion;
use consensus_runtime::scenarios::*;
use consensus_runtime::{EventLog, EventLogError, RunAggregate, RunLimits, TransitionError};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

type TestOrchestrator =
    Orchestrator<FakeModelClient, FakeMemoryClient, FakeProofVerifier, FixedClock, SequentialIdGen>;

fn temp_log(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hyphae-consensus-p13-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn orchestrator(model: FakeModelClient, memory: FakeMemoryClient) -> TestOrchestrator {
    Orchestrator {
        model,
        memory,
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    }
}

fn input() -> TaskInput {
    TaskInput {
        project_id: ProjectId::new(DEMO_PROJECT),
        title: "Fix payment retries".into(),
        description: "Retries burst under load".into(),
        submitted_by: "mario".into(),
        mode: None,
    }
}

fn item(id: &str, kind: ChallengeKind, claim: &str, description: &str) -> ChallengeItem {
    ChallengeItem {
        id: id.into(),
        kind,
        claim_id: Some(ClaimId::new(claim)),
        description: description.into(),
        evidence_refs: vec![],
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

fn revision(id: &str, plan_steps: &[&str], claims: Vec<Claim>) -> Revision {
    let mut proposal = proposal(id, "revised", claims);
    proposal.plan_steps = plan_steps.iter().map(|s| s.to_string()).collect();
    Revision {
        schema_version: SCHEMA_VERSION.into(),
        proposal,
        supersedes: ProposalId::new("placeholder"),
        addressed_items: vec![],
    }
}

/// (stage, code, reason) of the log's last record, which must be RunFailed.
fn failure(log: &EventLog) -> (String, Option<String>, String) {
    match &log.all_events().last().expect("a record").payload {
        EventPayload::RunFailed {
            stage,
            code,
            reason,
            ..
        } => (stage.clone(), code.clone(), reason.clone()),
        other => panic!("expected RunFailed last, got {other:?}"),
    }
}

fn count(log: &EventLog, run: &RunId, kind: fn(&EventPayload) -> bool) -> usize {
    log.events_for(run).filter(|e| kind(&e.payload)).count()
}

fn is_evaluation(p: &EventPayload) -> bool {
    matches!(p, EventPayload::EvaluationSubmitted { .. })
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn every_readable_schema_version_from_1_1_maps_to_a_policy() {
    for version in READABLE_SCHEMA_VERSIONS {
        match version {
            "1.0" => assert_eq!(PolicyVersion::for_schema(version), None),
            _ => assert!(
                PolicyVersion::for_schema(version).is_some(),
                "readable schema {version} has no policy rules"
            ),
        }
    }
    assert!(READABLE_SCHEMA_VERSIONS.contains(&SCHEMA_VERSION));
    assert_eq!(
        PolicyVersion::for_schema(SCHEMA_VERSION),
        Some(PolicyVersion::CURRENT)
    );
}

#[test]
fn a_scripted_evaluation_missing_one_verdict_fails_the_run() {
    // LC-03: the scripted fake gets no exemption and synthesizes nothing.
    // Two objections, one verdict: the run fails at evaluate, the refused
    // document is never recorded, and the refused call still is.
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal(
        "p1",
        "Cap retries",
        vec![claim("c1", "Payment retries are capped at 3 attempts")],
    )));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![
            item(
                "item-1",
                ChallengeKind::MissingTest,
                "c1",
                "No test covers the cap",
            ),
            item(
                "item-2",
                ChallengeKind::RegressionRisk,
                "c1",
                "The cap may starve slow callers",
            ),
        ],
    )));
    model.evaluations.push_back(Ok(judged(
        vec![assessment("c1", true, &["ev-cap"], &[])],
        vec![verdict(
            "item-1",
            ItemStatus::Resolved,
            "ev-cap states the cap",
        )],
        false,
    )));
    let memory = FakeMemoryClient::new(vec![evidence(
        "ev-cap",
        DEMO_PROJECT,
        "constraint",
        "Constraint: payment retries are capped at 3 attempts.",
    )]);
    let mut log = EventLog::open(temp_log("missing-verdict.jsonl")).unwrap();
    let mut orch = orchestrator(model, memory);
    let agg = orch.run(&mut log, input()).unwrap();

    assert_eq!(agg.state, RunState::Failed);
    assert!(agg.recommendation.is_none());
    let (stage, code, reason) = failure(&log);
    assert_eq!(stage, "evaluate");
    assert_eq!(code.as_deref(), Some("evaluation_contract"));
    assert!(reason.contains("one_verdict_per_item"), "{reason}");
    assert!(reason.contains("\"item-2\""), "{reason}");
    assert_eq!(count(&log, &agg.run_id, is_evaluation), 0);
    // propose, critique and the refused evaluate call: all on the record.
    assert_eq!(agg.invocations.len(), 3);
    let receipt = build_receipt(&log, &agg.run_id, TS.into()).unwrap();
    assert_eq!(receipt.model_invocations.len(), 3);
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

/// Audit RT-1 with the scripted doubles. Round 1: c1 raises attempts past
/// the stored cap (ev-cap contradicts it), c2 adds backoff; the Critic
/// objects to both. The revision drops c1 but keeps the plan step that
/// sets 10 attempts; round 2's recall no longer finds ev-cap and its
/// Critic is silent. `second` is the round-2 evaluation.
fn rt1_model(second: Evaluation) -> FakeModelClient {
    let steps = ["Set max attempts to 10", "Use exponential backoff"];
    let mut model = FakeModelClient::new();
    let mut first = proposal(
        "p1",
        "Retry harder",
        vec![
            claim("c1", "Raise max attempts to 10"),
            claim("c2", "Use exponential backoff with a 50 ms base"),
        ],
    );
    first.plan_steps = steps.iter().map(|s| s.to_string()).collect();
    model.proposals.push_back(Ok(first));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![
            item(
                "item-1",
                ChallengeKind::ConstraintConflict,
                "c1",
                "Ten attempts break the documented 3-attempt cap",
            ),
            item(
                "item-2",
                ChallengeKind::MissingTest,
                "c2",
                "No test covers the backoff schedule",
            ),
        ],
    )));
    model.evaluations.push_back(Ok(judged(
        vec![
            assessment("c1", false, &["ev-cap"], &["ev-cap"]),
            assessment("c2", true, &["ev-backoff"], &[]),
        ],
        vec![
            verdict("item-1", ItemStatus::Unresolved, "ev-cap caps retries at 3"),
            verdict("item-2", ItemStatus::Unresolved, "no test yet"),
        ],
        false,
    )));
    model.revisions.push_back(Ok(revision(
        "p2",
        &steps,
        vec![claim("c2", "Use exponential backoff with a 50 ms base")],
    )));
    model.critiques.push_back(Ok(challenge("chal-2", vec![])));
    model.evaluations.push_back(Ok(second));
    model
}

fn rt1_memory() -> FakeMemoryClient {
    FakeMemoryClient::new(vec![
        evidence(
            "ev-cap",
            DEMO_PROJECT,
            "constraint",
            "Constraint: outbound retries are capped at 3 attempts.",
        ),
        evidence(
            "ev-backoff",
            DEMO_PROJECT,
            "decision",
            "Decision: payment retries use exponential backoff with a 50 ms base.",
        ),
    ])
}

fn rt1_run(name: &str, second: Evaluation) -> (RunAggregate, EventLog, TestOrchestrator) {
    let mut log = EventLog::open(temp_log(name)).unwrap();
    let mut orch = orchestrator(rt1_model(second), rt1_memory());
    let agg = orch.run(&mut log, input()).unwrap();
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "{name} replays");
    (agg, log, orch)
}

#[test]
fn rt1_round_two_silence_cannot_reach_supported() {
    let backoff = || vec![assessment("c2", true, &["ev-backoff"], &[])];

    // Protocol 1.2 derived `supported` from exactly this silent evaluation.
    let (agg, log, _) = rt1_run("rt1-silent.jsonl", evaluation(backoff(), &[], false));
    assert!(agg.recommendation.is_none());
    let (stage, code, reason) = failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("evaluate", Some("evaluation_contract"))
    );
    assert!(
        reason.contains("item \"item-1\" has no verdict"),
        "{reason}"
    );

    // Verdicts given, but the round-1 contradiction is passed over.
    let verdicts_only = judged(
        backoff(),
        vec![
            verdict("item-1", ItemStatus::Resolved, "c1 was dropped"),
            verdict("item-2", ItemStatus::Resolved, "backoff is documented"),
        ],
        false,
    );
    let (agg, log, _) = rt1_run("rt1-contradiction.jsonl", verdicts_only);
    assert!(agg.recommendation.is_none());
    let (stage, code, reason) = failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("evaluate", Some("evaluation_contract"))
    );
    assert!(reason.contains("carried_contradiction_judged"), "{reason}");
    assert!(reason.contains("\"ev-cap\""), "{reason}");

    // An explicit judgment keeps the plan-step conflict open: contested.
    let explicit = Evaluation {
        cleared_contradictions: vec![cleared(
            "ev-cap",
            "c1 was dropped; item-1 keeps the plan-step conflict open",
        )],
        ..judged(
            backoff(),
            vec![
                verdict(
                    "item-1",
                    ItemStatus::Unresolved,
                    "the plan still sets 10 attempts",
                ),
                verdict("item-2", ItemStatus::Unresolved, "still no test"),
            ],
            false,
        )
    };
    let (agg, _, orch) = rt1_run("rt1-explicit.jsonl", explicit);
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Contested)
    );
    // Nothing that was open got resolved: no progress, no second revision.
    assert_eq!(
        orch.model.calls,
        ["propose", "critique", "evaluate", "revise", "critique", "evaluate"]
    );
}

#[test]
fn each_round_context_carries_every_record_open_item_and_contradiction() {
    let explicit = Evaluation {
        cleared_contradictions: vec![cleared("ev-cap", "c1 was dropped")],
        ..judged(
            vec![assessment("c2", true, &["ev-backoff"], &[])],
            vec![
                verdict(
                    "item-1",
                    ItemStatus::Unresolved,
                    "the plan still sets 10 attempts",
                ),
                verdict("item-2", ItemStatus::Unresolved, "still no test"),
            ],
            false,
        )
    };
    let (agg, log, orch) = rt1_run("rt1-contexts.jsonl", explicit);
    let ids = |ctx: &consensus_runtime::ports::RoleContext| -> Vec<String> {
        ctx.evidence.iter().map(|e| e.id.to_string()).collect()
    };
    let contexts = &orch.model.contexts;
    let stages: Vec<&str> = contexts.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        stages,
        ["critique", "evaluate", "revise", "critique", "evaluate"]
    );
    // Round 1 starts with nothing carried.
    assert_eq!(ids(&contexts[0].1), ["ev-cap", "ev-backoff"]);
    assert!(contexts[0].1.open_items.is_empty());
    assert!(contexts[0].1.carried_contradictions.is_empty());
    // Round 2: the revised claim no longer recalls ev-cap, but the record
    // stays in the context, with both open objections and the contradiction.
    for (stage, ctx) in &contexts[3..] {
        assert_eq!(ids(ctx), ["ev-cap", "ev-backoff"], "{stage}");
        let open: Vec<(&str, u32)> = ctx
            .open_items
            .iter()
            .map(|o| (o.item.id.as_str(), o.raised_in_round))
            .collect();
        assert_eq!(open, [("item-1", 1), ("item-2", 1)], "{stage}");
        assert_eq!(
            ctx.carried_contradictions,
            [EvidenceId::new("ev-cap")],
            "{stage}"
        );
    }
    // Every record of the round context is verified in each round.
    let rounds: Vec<(String, Option<u32>)> = log
        .events_for(&agg.run_id)
        .filter_map(|e| match &e.payload {
            EventPayload::EvidenceVerified { verification } => {
                Some((verification.evidence_id.to_string(), verification.round))
            }
            _ => None,
        })
        .collect();
    let expected: Vec<(String, Option<u32>)> = [
        ("ev-cap", Some(1)),
        ("ev-backoff", Some(1)),
        ("ev-cap", Some(2)),
        ("ev-backoff", Some(2)),
    ]
    .iter()
    .map(|(id, r)| (id.to_string(), *r))
    .collect();
    assert_eq!(rounds, expected);
}

#[test]
fn progress_is_set_based_not_count_based() {
    // RT-7: round 2 resolves `a` but the Critic adds `c` and `d`, so more
    // objections are open than before (3 > 2). The 1.2 count rule would
    // stop here; one open objection got resolved, so the run revises again.
    let c1 = || vec![claim("c1", "Payment retries are capped at 3 attempts")];
    let judged_c1 = |verdicts| judged(vec![assessment("c1", false, &[], &[])], verdicts, false);
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal("p1", "s", c1())));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![
            item(
                "a",
                ChallengeKind::MissingTest,
                "c1",
                "No test covers the cap",
            ),
            item(
                "b",
                ChallengeKind::UnsupportedClaim,
                "c1",
                "No record states the cap",
            ),
        ],
    )));
    model.evaluations.push_back(Ok(judged_c1(vec![
        verdict("a", ItemStatus::Unresolved, "no test"),
        verdict("b", ItemStatus::Unresolved, "no record"),
    ])));
    model
        .revisions
        .push_back(Ok(revision("p2", &["Add a cap test"], c1())));
    model.critiques.push_back(Ok(challenge(
        "chal-2",
        vec![
            item(
                "c",
                ChallengeKind::RegressionRisk,
                "c1",
                "The cap may starve slow callers",
            ),
            item(
                "d",
                ChallengeKind::SecurityRisk,
                "c1",
                "Retries could amplify an outage",
            ),
        ],
    )));
    model.evaluations.push_back(Ok(judged_c1(vec![
        verdict("a", ItemStatus::Resolved, "the revision adds a cap test"),
        verdict("b", ItemStatus::Unresolved, "no record"),
        verdict("c", ItemStatus::Unresolved, "unmeasured"),
        verdict("d", ItemStatus::Unresolved, "unmeasured"),
    ])));
    model
        .revisions
        .push_back(Ok(revision("p3", &["Add a cap test"], c1())));
    model.critiques.push_back(Ok(challenge("chal-3", vec![])));
    model.evaluations.push_back(Ok(judged_c1(vec![
        verdict("b", ItemStatus::Unresolved, "no record"),
        verdict("c", ItemStatus::Unresolved, "unmeasured"),
        verdict("d", ItemStatus::Unresolved, "unmeasured"),
    ])));
    let mut log = EventLog::open(temp_log("set-progress.jsonl")).unwrap();
    let mut orch = orchestrator(model, FakeMemoryClient::new(vec![]));
    let agg = orch.run(&mut log, input()).unwrap();
    assert_eq!(agg.revision_round, 2);
    assert_eq!(
        orch.model.calls.iter().filter(|c| *c == "revise").count(),
        2
    );
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Contested)
    );
    let open: Vec<&str> = agg.open_items.iter().map(|o| o.item.id.as_str()).collect();
    assert_eq!(open, ["b", "c", "d"]);
    let raised: Vec<u32> = agg.open_items.iter().map(|o| o.raised_in_round).collect();
    assert_eq!(raised, [1, 2, 2]);
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

#[test]
fn a_reused_item_id_fails_the_run_at_critique() {
    // One verdict must never settle two objections: a round-2 item that
    // reuses a round-1 id is refused before it is recorded.
    let c1 = || vec![claim("c1", "Payment retries are capped at 3 attempts")];
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal("p1", "s", c1())));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![item(
            "item-1",
            ChallengeKind::MissingTest,
            "c1",
            "No test covers the cap",
        )],
    )));
    model.evaluations.push_back(Ok(judged(
        vec![assessment("c1", false, &[], &[])],
        vec![verdict("item-1", ItemStatus::Unresolved, "no test")],
        false,
    )));
    model
        .revisions
        .push_back(Ok(revision("p2", &["Add a cap test"], c1())));
    model.critiques.push_back(Ok(challenge(
        "chal-2",
        vec![item(
            "item-1",
            ChallengeKind::RegressionRisk,
            "c1",
            "The cap may starve slow callers",
        )],
    )));
    let mut log = EventLog::open(temp_log("reused-item.jsonl")).unwrap();
    let mut orch = orchestrator(model, FakeMemoryClient::new(vec![]));
    let agg = orch.run(&mut log, input()).unwrap();
    let (stage, code, reason) = failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("critique", Some("challenge_contract"))
    );
    assert!(reason.contains("item_ids_unique"), "{reason}");
    assert!(reason.contains("\"item-1\""), "{reason}");
    let challenges = count(&log, &agg.run_id, |p| {
        matches!(p, EventPayload::ChallengeSubmitted { .. })
    });
    assert_eq!(challenges, 1, "the refused challenge is not recorded");
    assert_eq!(agg.invocations.len(), 5, "the refused critique call is");
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

#[test]
fn evidence_cap_reached_in_round_one_keeps_round_two_at_the_carried_records() {
    // LC-09: 6 claims x 5 matching records = 30 candidates. Round 1 takes
    // the first 20; round 2 recalls the same 30, adds nothing, and records
    // the cap again. Both contexts hold exactly the 20 carried records.
    let topics = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    let claims = || -> Vec<Claim> {
        topics
            .iter()
            .enumerate()
            .map(|(i, t)| claim(&format!("c{i}"), &format!("{t}topic")))
            .collect()
    };
    let records: Vec<EvidenceReference> = topics
        .iter()
        .flat_map(|t| {
            (0..5).map(move |j| {
                let content = format!("{t}topic record {j}");
                evidence(&format!("ev-{t}-{j}"), DEMO_PROJECT, "fact", &content)
            })
        })
        .collect();
    let assessments = || {
        (0..topics.len())
            .map(|i| assessment(&format!("c{i}"), false, &[], &[]))
            .collect()
    };
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal("p1", "s", claims())));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![item(
            "item-1",
            ChallengeKind::MissingTest,
            "c0",
            "No test covers alpha",
        )],
    )));
    model.evaluations.push_back(Ok(judged(
        assessments(),
        vec![verdict("item-1", ItemStatus::Unresolved, "no test")],
        false,
    )));
    model
        .revisions
        .push_back(Ok(revision("p2", &["Add tests"], claims())));
    model.critiques.push_back(Ok(challenge("chal-2", vec![])));
    model.evaluations.push_back(Ok(judged(
        assessments(),
        vec![verdict("item-1", ItemStatus::Unresolved, "still no test")],
        false,
    )));
    let mut log = EventLog::open(temp_log("cap-two-rounds.jsonl")).unwrap();
    let mut orch = orchestrator(model, FakeMemoryClient::new(records));
    let agg = orch.run(&mut log, input()).unwrap();
    assert_eq!(agg.revision_round, 1);

    let retrieved = count(&log, &agg.run_id, |p| {
        matches!(p, EventPayload::EvidenceRetrieved { .. })
    });
    assert_eq!(retrieved, MAX_EVIDENCE_PER_RUN);
    let capped: Vec<Option<String>> = log
        .events_for(&agg.run_id)
        .filter_map(|e| match &e.payload {
            EventPayload::EvidenceUnavailable { reason, code, .. }
                if reason.contains("per-run cap") =>
            {
                Some(code.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        capped,
        [Some("evidence_cap".into()), Some("evidence_cap".into())]
    );
    for (stage, ctx) in &orch.model.contexts {
        assert_eq!(ctx.evidence.len(), MAX_EVIDENCE_PER_RUN, "{stage}");
    }
    for round in [1, 2] {
        let verified = log
            .events_for(&agg.run_id)
            .filter(|e| match &e.payload {
                EventPayload::EvidenceVerified { verification } => {
                    verification.round == Some(round)
                }
                _ => false,
            })
            .count();
        assert_eq!(verified, MAX_EVIDENCE_PER_RUN, "round {round}");
    }
}

#[test]
fn the_recommendation_carries_the_final_proposals_uncertainties() {
    let s = retry_constraint_scenario();
    let mut log = EventLog::open(temp_log("uncertainties.jsonl")).unwrap();
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let agg = orch.run(&mut log, s.input).unwrap();
    let rec = agg.recommendation.expect("recommendation issued");
    assert_eq!(rec.kind, RecommendationKind::Supported);
    assert_eq!(
        rec.unresolved_uncertainties,
        ["No load-test data available"]
    );
    assert_eq!(
        rec.unresolved_assumptions,
        ["Upstream services tolerate the proposed change"]
    );
}

// --- Replay: a forged 1.3 log fails, and 1.3 records hash stably. ---

/// Writes `events` as a validly chained log: what a forger who recomputes
/// the unsigned SHA-256 chain would produce (the recipe research/README.md
/// gives: `hash` is sha256 of `prev_hash` followed by the record with
/// `"hash":""`).
fn write_rechained(path: &Path, events: &[ConsensusEvent]) {
    let mut prev = "genesis".to_string();
    let mut out = String::new();
    for ev in events {
        let mut e = ev.clone();
        e.prev_hash = prev.clone();
        e.hash = String::new();
        let blanked = serde_json::to_string(&e).unwrap();
        e.hash = hex(Sha256::digest(format!("{prev}{blanked}")));
        out.push_str(&serde_json::to_string(&e).unwrap());
        out.push('\n');
        prev = e.hash;
    }
    std::fs::write(path, out).unwrap();
}

/// The reference run's events: two evaluations, the second resolving both
/// carried items and clearing the carried contradiction.
fn reference_events() -> (RunId, Vec<ConsensusEvent>) {
    let s = retry_constraint_scenario();
    let mut log = EventLog::open(temp_log("forge-source.jsonl")).unwrap();
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let agg = orch.run(&mut log, s.input).unwrap();
    assert_eq!(
        agg.recommendation.map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
    (agg.run_id, log.all_events().to_vec())
}

/// Applies `edit` to the `nth` evaluation (0-based) of the events.
fn forge(
    events: &[ConsensusEvent],
    nth: usize,
    edit: impl Fn(&mut Evaluation),
) -> Vec<ConsensusEvent> {
    let mut forged = events.to_vec();
    let target = forged
        .iter_mut()
        .filter_map(|e| match &mut e.payload {
            EventPayload::EvaluationSubmitted { evaluation, .. } => Some(evaluation),
            _ => None,
        })
        .nth(nth)
        .expect("evaluation to forge");
    edit(target);
    forged
}

#[test]
fn a_forged_1_3_log_fails_replay() {
    let (run, events) = reference_events();
    let cases: Vec<(&str, Vec<ConsensusEvent>, ContractViolation)> = vec![
        (
            "missing verdict",
            forge(&events, 1, |e| {
                e.item_verdicts.retain(|v| v.item_id != "item-2")
            }),
            ContractViolation::MissingVerdict("item-2".into()),
        ),
        (
            "unknown item",
            forge(&events, 1, |e| e.item_verdicts[0].item_id = "item-9".into()),
            ContractViolation::UnknownItem("item-9".into()),
        ),
        (
            "unresolved_items mismatch",
            forge(&events, 0, |e| {
                e.unresolved_items.retain(|id| id != "item-2")
            }),
            ContractViolation::UnresolvedNotListed("item-2".into()),
        ),
        (
            "resolved without reason",
            forge(&events, 1, |e| e.item_verdicts[0].reason = " ".into()),
            ContractViolation::ResolvedWithoutReason("item-1".into()),
        ),
        (
            "cleared without reason",
            forge(&events, 1, |e| e.cleared_contradictions[0].reason.clear()),
            ContractViolation::ClearedWithoutReason(EvidenceId::new("ev-constraint")),
        ),
    ];
    for (label, forged, violation) in cases {
        let path = temp_log(&format!("forged-{}.jsonl", label.replace(' ', "-")));
        write_rechained(&path, &forged);
        // The chain itself verifies: only the contract can catch this.
        let log = EventLog::open(&path).unwrap_or_else(|e| panic!("{label}: {e}"));
        match log.rebuild(&run) {
            Err(EventLogError::Replay(TransitionError::Contract(found))) => {
                assert_eq!(found, violation, "{label}")
            }
            other => panic!("{label}: expected a contract violation, got {other:?}"),
        }
    }
    // The unforged events, re-chained the same way, replay to the same end.
    let path = temp_log("forged-none.jsonl");
    write_rechained(&path, &events);
    let agg = EventLog::open(&path).unwrap().rebuild(&run).unwrap();
    assert_eq!(agg.state, RunState::AwaitingHumanApproval);
}

#[test]
fn pre_1_3_records_are_never_held_to_the_contract() {
    // The same silent evaluation, recorded as a 1.2 run: replayed under
    // its own rules exactly as before protocol 1.3 (every archived run in
    // research/ is such a record).
    let (run, events) = reference_events();
    let mut forged = forge(&events, 1, |e| {
        e.item_verdicts.clear();
        e.cleared_contradictions.clear();
    });
    for e in &mut forged {
        e.schema_version = "1.2".into();
    }
    let path = temp_log("relabeled-1-2.jsonl");
    write_rechained(&path, &forged);
    let agg = EventLog::open(&path).unwrap().rebuild(&run).unwrap();
    assert_eq!(
        agg.recommendation.map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
}

/// A run whose single evaluation judges three objections.
fn three_verdict_log(name: &str) -> (PathBuf, RunId) {
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal(
        "p1",
        "Cap retries",
        vec![claim("c1", "Payment retries are capped at 3 attempts")],
    )));
    model.critiques.push_back(Ok(challenge(
        "chal-1",
        vec![
            item(
                "item-1",
                ChallengeKind::MissingTest,
                "c1",
                "No test covers the cap",
            ),
            item(
                "item-2",
                ChallengeKind::RegressionRisk,
                "c1",
                "The cap may starve slow callers",
            ),
            item(
                "item-3",
                ChallengeKind::SecurityRisk,
                "c1",
                "Retries could amplify an outage",
            ),
        ],
    )));
    model.evaluations.push_back(Ok(judged(
        vec![assessment("c1", true, &["ev-cap"], &[])],
        vec![
            verdict("item-1", ItemStatus::Resolved, "the plan adds a cap test"),
            verdict(
                "item-2",
                ItemStatus::Resolved,
                "ev-cap: slow callers back off",
            ),
            verdict(
                "item-3",
                ItemStatus::Resolved,
                "ev-cap: three attempts bound the load",
            ),
        ],
        false,
    )));
    let memory = FakeMemoryClient::new(vec![evidence(
        "ev-cap",
        DEMO_PROJECT,
        "constraint",
        "Constraint: payment retries are capped at 3 attempts.",
    )]);
    let path = temp_log(name);
    let mut log = EventLog::open(&path).unwrap();
    let mut orch = orchestrator(model, memory);
    let agg = orch.run(&mut log, input()).unwrap();
    assert_eq!(
        agg.recommendation.map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
    (path, agg.run_id)
}

#[test]
fn a_1_3_record_with_three_verdicts_round_trips_and_rehashes_identically() {
    let (path, run) = three_verdict_log("three-verdicts.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let evaluation_line = lines
        .iter()
        .find(|l| l.contains("\"type\":\"evaluation_submitted\""))
        .expect("an evaluation record");
    assert_eq!(evaluation_line.matches("\"item_id\"").count(), 3);
    let mut prev = "genesis".to_string();
    for line in &lines {
        let stored: ConsensusEvent = serde_json::from_str(line).unwrap();
        assert_eq!(stored.schema_version, SCHEMA_VERSION);
        for _ in 0..100 {
            // A fresh parse writes back the same bytes...
            let parsed: ConsensusEvent = serde_json::from_str(line).unwrap();
            assert!(serde_json::to_string(&parsed).unwrap() == *line);
            // ...and hashes to the stored hash (no map-order drift).
            let mut blanked = parsed;
            blanked.hash.clear();
            let canonical = serde_json::to_string(&blanked).unwrap();
            assert_eq!(
                hex(Sha256::digest(format!("{prev}{canonical}"))),
                stored.hash
            );
        }
        // The third-party recipe, on the raw line, agrees.
        let field = format!(r#""hash":"{}""#, stored.hash);
        let raw_blanked = line.replacen(&field, r#""hash":"""#, 1);
        assert_eq!(
            hex(Sha256::digest(format!("{prev}{raw_blanked}"))),
            stored.hash
        );
        prev = stored.hash;
    }
    // Reopening re-verifies every link and replays the same run.
    for _ in 0..3 {
        let log = EventLog::open(&path).unwrap();
        assert_eq!(
            log.rebuild(&run).unwrap().state,
            RunState::AwaitingHumanApproval
        );
    }
}

/// One record of every type schema 1.3 extends, with every new field set.
fn fully_populated_1_3_events(run: &RunId) -> Vec<NewEvent> {
    let invocation = ModelInvocation {
        provider: "meta".into(),
        model: "muse-wonder-1.0".into(),
        request_id: Some("chatcmpl-1".into()),
        latency_ms: Some(1200),
        input_tokens: Some(900),
        output_tokens: Some(300),
        finish_reason: Some("stop".into()),
        reasoning_tokens: Some(120),
        max_completion_tokens: Some(16_000),
        schema_name: Some("DraftEvaluation".into()),
        schema_sha256: Some("ab".repeat(32)),
        prompt_sha256: Some("cd".repeat(32)),
        reasoning_effort: Some("medium".into()),
        safety_identifier_sent: Some(true),
        attempts: Some(2),
    };
    let request = ProofRequest {
        operation: "list".into(),
        project: "demo-repo".into(),
        query: Some("retry cap".into()),
        limit: Some(7),
    };
    let mut record = evidence(
        "ev-1",
        DEMO_PROJECT,
        "constraint",
        "Retries are capped at 3.",
    );
    record.retrieval = Some(Retrieval::SealedSnapshot);
    record.snapshot_id = Some("snap-2026-10-29".into());
    record.captured_at = Some("2026-10-29T10:00:00Z".into());
    record.proof_ref = Some("snap-2026-10-29/list-demo-repo".into());
    record.proof_sha256 = Some("ef".repeat(32));
    record.proof_request = Some(request.clone());
    record.backend_project = Some("demo-repo".into());
    record.backend_scope = Some("work".into());
    let mut evaluation = judged(
        vec![assessment("c1", true, &["ev-1"], &[])],
        vec![
            verdict("item-1", ItemStatus::Resolved, "fixed"),
            verdict("item-2", ItemStatus::Unresolved, "open"),
            verdict("item-3", ItemStatus::Resolved, "fixed too"),
        ],
        false,
    );
    evaluation.cleared_contradictions = vec![cleared("ev-0", "the claim was dropped")];
    let payloads = vec![
        EventPayload::TaskCreated {
            task: Task {
                schema_version: SCHEMA_VERSION.into(),
                id: TaskId::new("task-1"),
                run_id: run.clone(),
                project_id: ProjectId::new(DEMO_PROJECT),
                title: "t".into(),
                description: "d".into(),
                created_at: TS.into(),
                mode: Some(TaskMode {
                    model: ModelMode::Meta,
                    evidence: EvidenceMode::SealedSnapshot,
                    origin: RunOrigin::Public,
                    preset: Some("raise-to-10".into()),
                }),
            },
        },
        EventPayload::EvidenceRetrieved { evidence: record },
        EventPayload::RecallProven {
            request: request.clone(),
            evidence_ids: vec![EvidenceId::new("ev-1")],
            proof_ref: "snap-2026-10-29/list-demo-repo".into(),
            anchor: "12".repeat(32),
            empty: false,
        },
        EventPayload::RecallProven {
            request: ProofRequest {
                operation: "recall".into(),
                project: "other-repo".into(),
                query: Some("retry cap".into()),
                limit: None,
            },
            evidence_ids: vec![],
            proof_ref: "snap-2026-10-29/recall-other-repo".into(),
            anchor: "12".repeat(32),
            empty: true,
        },
        EventPayload::EvidenceUnavailable {
            query: "(per-run evidence cap)".into(),
            reason: "1 additional record(s) not processed".into(),
            code: Some("evidence_cap".into()),
        },
        EventPayload::EvaluationSubmitted {
            evaluation,
            invocation: invocation.clone(),
        },
        EventPayload::EvidenceVerified {
            verification: EvidenceVerification {
                schema_version: SCHEMA_VERSION.into(),
                evidence_id: EvidenceId::new("ev-1"),
                integrity_status: IntegrityStatus::Valid,
                factual_status: FactualStatus::Corroborated,
                relevance_status: RelevanceStatus::Relevant,
                verifier: Some("hyphae proof verify".into()),
                verified_anchor: Some("12".repeat(32)),
                detail: Some("verified".into()),
                checked_at: TS.into(),
                round: Some(2),
                proof_ref: Some("snap-2026-10-29/list-demo-repo".into()),
                report: Some(VerifierReport {
                    status: Some("verified".into()),
                    scope: Some("semantic_reexecution".into()),
                    kind: Some("memory".into()),
                    anchor_digest: Some("12".repeat(32)),
                    result_digest: Some("34".repeat(32)),
                    proof_digest: Some("56".repeat(32)),
                }),
            },
        },
        EventPayload::RecommendationIssued {
            recommendation: Recommendation {
                schema_version: SCHEMA_VERSION.into(),
                kind: RecommendationKind::Contested,
                rationale: "r".into(),
                unresolved_assumptions: vec!["a".into()],
                unresolved_uncertainties: vec!["No load-test data".into()],
                issued_at: TS.into(),
            },
        },
        EventPayload::HumanDecisionRecorded {
            authorization: HumanAuthorization {
                schema_version: SCHEMA_VERSION.into(),
                run_id: run.clone(),
                authorized_by: "visitor".into(),
                approved: false,
                decided_at: TS.into(),
                idempotency_key: "k".into(),
                note: None,
                authority: Some(Authority::PublicDemo),
            },
        },
        EventPayload::RunFailed {
            reason: "evaluate output refused".into(),
            stage: "evaluate".into(),
            code: Some("evaluation_contract".into()),
            invocation: Some(invocation),
        },
    ];
    payloads
        .into_iter()
        .enumerate()
        .map(|(n, payload)| NewEvent {
            event_id: EventId::new(format!("e{n}")),
            run_id: run.clone(),
            actor: Actor::Runtime,
            occurred_at: TS.into(),
            payload,
        })
        .collect()
}

#[test]
fn every_new_1_3_field_round_trips_byte_for_byte() {
    // EventLog::append chains records without replaying them, so this log
    // is a serialization fixture, not a valid run.
    let run = RunId::new("run-fields");
    let path = temp_log("all-1-3-fields.jsonl");
    let mut log = EventLog::open(&path).unwrap();
    for new in fully_populated_1_3_events(&run) {
        log.append(new).unwrap();
    }
    drop(log);
    // Reopening recomputes every hash from the parsed records.
    let log = EventLog::open(&path).unwrap();
    assert_eq!(log.len(), 10);
    let text = std::fs::read_to_string(&path).unwrap();
    for line in text.lines() {
        let parsed: ConsensusEvent = serde_json::from_str(line).unwrap();
        assert!(serde_json::to_string(&parsed).unwrap() == line, "{line}");
    }
    // The documented wire names.
    for name in [
        r#""type":"recall_proven""#,
        r#""empty":true"#,
        r#""retrieval":"sealed_snapshot""#,
        r#""authority":"public_demo""#,
        r#""origin":"public""#,
        r#""evidence":"sealed_snapshot""#,
        r#""status":"resolved""#,
        r#""status":"unresolved""#,
        r#""safety_identifier_sent":true"#,
        r#""round":2"#,
        r#""code":"evaluation_contract""#,
    ] {
        assert!(text.contains(name), "{name} missing");
    }
}

#[test]
fn absent_1_3_fields_are_never_written() {
    // LC-01: a record that sets none of the 1.3 fields serializes exactly
    // as a 1.2 record did, so archived lines keep their bytes and hashes.
    let run = RunId::new("run-empty");
    let mut events = fully_populated_1_3_events(&run);
    for new in &mut events {
        match &mut new.payload {
            EventPayload::TaskCreated { task } => task.mode = None,
            EventPayload::EvidenceRetrieved { evidence } => {
                *evidence = consensus_runtime::scenarios::evidence(
                    "ev-1",
                    DEMO_PROJECT,
                    "constraint",
                    "Retries are capped at 3.",
                );
                evidence.retrieval = None;
            }
            EventPayload::EvidenceUnavailable { code, .. } => *code = None,
            EventPayload::EvaluationSubmitted {
                evaluation,
                invocation,
            } => {
                evaluation.item_verdicts.clear();
                evaluation.cleared_contradictions.clear();
                invocation.finish_reason = None;
                invocation.reasoning_tokens = None;
                invocation.max_completion_tokens = None;
                invocation.schema_name = None;
                invocation.schema_sha256 = None;
                invocation.prompt_sha256 = None;
                invocation.reasoning_effort = None;
                invocation.safety_identifier_sent = None;
                invocation.attempts = None;
            }
            EventPayload::EvidenceVerified { verification } => {
                verification.round = None;
                verification.proof_ref = None;
                verification.report = None;
            }
            EventPayload::RecommendationIssued { recommendation } => {
                recommendation.unresolved_uncertainties.clear()
            }
            EventPayload::HumanDecisionRecorded { authorization } => authorization.authority = None,
            EventPayload::RunFailed {
                code, invocation, ..
            } => {
                *code = None;
                *invocation = None;
            }
            _ => {}
        }
    }
    for new in events {
        if matches!(new.payload, EventPayload::RecallProven { .. }) {
            continue; // a 1.3 event type: it has no 1.2 form
        }
        // Model-call events always carried an invocation; RunFailed never did.
        let failed = matches!(new.payload, EventPayload::RunFailed { .. });
        let json = serde_json::to_string(&new.into_sequenced(1)).unwrap();
        if failed {
            assert!(!json.contains("\"invocation\""), "{json}");
        }
        for key in [
            "\"mode\"",
            "\"retrieval\"",
            "\"snapshot_id\"",
            "\"captured_at\"",
            "\"proof_ref\"",
            "\"proof_sha256\"",
            "\"proof_request\"",
            "\"backend_project\"",
            "\"backend_scope\"",
            "\"code\"",
            "\"item_verdicts\"",
            "\"cleared_contradictions\"",
            "\"finish_reason\"",
            "\"reasoning_tokens\"",
            "\"max_completion_tokens\"",
            "\"schema_name\"",
            "\"schema_sha256\"",
            "\"prompt_sha256\"",
            "\"reasoning_effort\"",
            "\"safety_identifier_sent\"",
            "\"attempts\"",
            "\"round\"",
            "\"report\"",
            "\"unresolved_uncertainties\"",
            "\"authority\"",
        ] {
            assert!(!json.contains(key), "{key} written in {json}");
        }
    }
}
