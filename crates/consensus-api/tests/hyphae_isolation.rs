// SPDX-License-Identifier: Apache-2.0
//! Hyphae adapter hardening: project isolation through the real adapter
//! (TC-4), proofs recorded for every recall including empty ones (HY-6,
//! HY-12), the typed proof-generation failure (HY-4, LC-10), what a
//! `Valid` verification requires (HY-1, HY-10), the verifier's deadline
//! (HY-2), content-keyed reuse (HY-13) and the stdio transport's
//! diagnostics (HY-9). Envelopes follow the live 4.0.0 shapes captured on
//! 2026-10-27 (see `tests/hyphae_adapter.rs`); no Hyphae runs here.

use consensus_api::config::HyphaeConfig;
use consensus_api::hyphae::{
    CommandError, CommandOutput, CommandRunner, HyphaeMemoryClient, HyphaeProofVerifier,
    InMemoryTransport, McpTransport, StdioTransport, SystemCommandRunner,
    CREDENTIAL_PATH_VARIABLES,
};
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::fakes::{FakeModelClient, FakeProofVerifier};
use consensus_runtime::orchestrator::{Orchestrator, TaskInput};
use consensus_runtime::ports::{
    MemoryClient, MemoryError, MemoryStatus, ProofFailure, ProofVerifier, Recall,
};
use consensus_runtime::scenarios::{assessment, claim, evaluation, proposal, TS};
use consensus_runtime::{EventLog, RunAggregate, RunLimits};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const ANCHOR: &str = "e9cf8c51ad12358397bcd1a387d73b0778482070b1a6133e3f5d9351bbc22621";
const PROOF_ID: &str = "recall-01a17bf3-879a-7833-94a4-361661d67761";

/// `hyphae proof verify` success report, captured live (4.0.0, 2026-10-27).
const VERIFIED_REPORT: &str = r#"{"anchor_digest":"e9cf8c51ad12358397bcd1a387d73b0778482070b1a6133e3f5d9351bbc22621","directory_count":5,"evidence_digest":"086b8215c145dd4802491d3be49276ef1a2a8a37418297e34b12c748893cab9b","file_count":5,"kind":"memory","proof_digest":"6a5f2fa6b846a707d1c6b77c9e7c8b3ce51df1e99dee2a7d3f5d52380474edbb","request_digest":"008663b572b3a1a3e79cac036fcb2f2adba29c4777aec61b28e06f57bd4a2068","result_digest":"3d6b60f10d75f3abd56d39025ca380489bca2f2d83b63d22abcc8cfcf91bac70","scope":"semantic_reexecution","semantic_reexecution_performed":true,"status":"verified","total_file_bytes":3261022,"witness_digest":"216a0a01061de84f59d9d5543b1415ed510c7e549e62d99c8ed97d68f7b7a799"}"#;

fn transport(calls: Vec<Value>) -> InMemoryTransport {
    let mut t = InMemoryTransport::default();
    t.script(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "hyphae"}}),
    );
    t.script(
        "tools/list",
        json!({"tools": [{"name": "hyphae_memory_recall"}, {"name": "hyphae_memory_status"}]}),
    );
    for call in calls {
        t.script("tools/call", call);
    }
    t
}

fn memory(id: &str, project: Option<&str>, scope: &str, kind: &str, text: &str) -> Value {
    let mut m = json!({"id": id, "kind": kind, "layer": "work", "scope": scope, "score": 0.016, "text": text});
    if let Some(project) = project {
        m["project"] = json!(project);
    }
    m
}

/// A proven recall result in the live 4.0.0 shape: one proof for all.
fn proven(memories: Vec<Value>, proof_id: &str) -> Value {
    json!({"isError": false, "structuredContent": {
        "expired_filtered": 0,
        "memories": memories,
        "proof": {
            "anchor_hex": ANCHOR,
            "proof_blake3": "1ee5c45db81db52e0bb743f9dc2fe1bb99eb88ebe661037f0aa608ad7089df79",
            "proof_path": format!("/hyphae-home/.local/state/hyphae/proofs/{proof_id}.proof"),
            "scope": "memory-snapshot-v1",
            "witness_path": format!("/hyphae-home/.local/state/hyphae/proofs/{proof_id}.witness")
        },
        "retrieval_mode": "lexical",
        "semantic_status": "disabled"
    }})
}

fn mixed_result() -> Value {
    proven(
        vec![
            memory(
                "1",
                Some("demo-repo"),
                "project",
                "constraint",
                "Constraint: outbound retries are capped at 3 attempts.",
            ),
            memory(
                "2",
                Some("_global"),
                "global",
                "fact",
                "Fact: every service logs retries.",
            ),
            memory(
                "3",
                Some("tenant-b"),
                "project",
                "constraint",
                "Tenant B private constraint: never share.",
            ),
            memory("4", None, "project", "fact", "Unattributed fact."),
        ],
        PROOF_ID,
    )
}

fn ids(records: &[EvidenceReference]) -> Vec<&str> {
    records.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn records_keep_their_backend_attribution_and_foreign_ones_are_dropped() {
    let mut client = HyphaeMemoryClient::connect(transport(vec![mixed_result()])).unwrap();
    let recall = client
        .recall_with_proof(&ProjectId::new("demo-repo"), "retries attempts", 5)
        .unwrap();
    assert_eq!(ids(&recall.records), ["1", "2"]);
    let own = &recall.records[0];
    assert_eq!(own.project_id, ProjectId::new("demo-repo"));
    assert_eq!(own.backend_project.as_deref(), Some("demo-repo"));
    assert_eq!(own.backend_scope.as_deref(), Some("project"));
    // A `_global` record stays labeled `_global`, never relabeled.
    let global = &recall.records[1];
    assert_eq!(global.project_id, ProjectId::new("_global"));
    assert_eq!(global.backend_project.as_deref(), Some("_global"));
    assert_eq!(global.backend_scope.as_deref(), Some("global"));
    let request = ProofRequest {
        operation: "recall".into(),
        project: "demo-repo".into(),
        query: Some("retries attempts".into()),
        limit: Some(5),
    };
    for rec in &recall.records {
        assert_eq!(rec.retrieval, Some(Retrieval::LiveRecall));
        assert_eq!(
            rec.proof_ref.as_deref(),
            Some(PROOF_ID),
            "the artifact id, no path"
        );
        assert_eq!(rec.proof_request.as_ref(), Some(&request));
        assert_eq!(rec.anchor.as_deref(), Some(ANCHOR));
        assert!(rec.proof_file.as_deref().unwrap().ends_with(".proof"));
        assert_eq!(
            rec.proof_sha256, None,
            "a container path is not readable here"
        );
        assert!(rec.proof_error.is_none());
    }
    // Another tenant's record and an unattributed one are dropped.
    let foreign: Vec<(&str, Option<&str>)> = recall
        .foreign
        .iter()
        .map(|f| (f.evidence_id.as_str(), f.backend_project.as_deref()))
        .collect();
    assert_eq!(foreign, [("3", Some("tenant-b")), ("4", None)]);
    // The proof covers the whole sealed result, foreign records included.
    let proof = recall.proof.expect("a proven recall returns its proof");
    assert_eq!(proof.request, request);
    assert_eq!(proof.proof_ref, PROOF_ID);
    assert_eq!(proof.anchor, ANCHOR);
    let sealed: Vec<&str> = proof.evidence_ids.iter().map(|i| i.as_str()).collect();
    assert_eq!(sealed, ["1", "2", "3", "4"]);
    // The plain recall serves only the attributed records.
    let plain = client
        .recall(&ProjectId::new("demo-repo"), "retries attempts", 5)
        .unwrap();
    assert_eq!(ids(&plain), ["1", "2"]);
}

#[test]
fn an_empty_proven_recall_still_returns_its_proof() {
    let mut client =
        HyphaeMemoryClient::connect(transport(vec![proven(vec![], "recall-empty")])).unwrap();
    let recall = client
        .recall_with_proof(&ProjectId::new("other-repo"), "retry cap", 5)
        .unwrap();
    assert!(recall.records.is_empty() && recall.foreign.is_empty());
    let proof = recall.proof.expect("absence is proven too");
    assert!(proof.evidence_ids.is_empty());
    assert_eq!(proof.request.project, "other-repo");
    assert_eq!(proof.proof_ref, "recall-empty");
}

#[test]
fn a_failed_proof_travels_typed_next_to_its_text() {
    let tool_error = json!({"isError": false, "structuredContent": {
        "schema": "hyphae-native-mcp-tool-error-v2",
        "error": {"category": "corruption", "code": "corruption",
                  "message": "native durable state is invalid", "retry": "after-recovery"}
    }});
    let unproven = json!({"structuredContent": {"memories": [
        memory("77", Some("demo-repo"), "project", "constraint", "Constraint: cap retries at 3")
    ], "proof": null}});
    let mut client = HyphaeMemoryClient::connect(transport(vec![tool_error, unproven])).unwrap();
    let recall = client
        .recall_with_proof(&ProjectId::new("demo-repo"), "retries", 5)
        .unwrap();
    assert_eq!(
        recall.proof_failure,
        Some(ProofFailure {
            category: Some("corruption".into()),
            message: "native durable state is invalid".into(),
        })
    );
    assert!(recall.proof.is_none());
    let rec = &recall.records[0];
    let reason = rec
        .proof_error
        .as_deref()
        .expect("the text is kept on the record");
    assert!(reason.contains("(corruption)") && reason.contains("native durable state is invalid"));
    assert_eq!(rec.retrieval, Some(Retrieval::LiveRecall));
    assert_eq!(rec.proof_ref, None, "no proof covers it");
}

#[test]
fn an_answer_without_a_record_list_is_an_error_never_an_empty_recall() {
    let odd = json!({"structuredContent": {"hits": [{"id": "1"}]}});
    let mut client = HyphaeMemoryClient::connect(transport(vec![odd])).unwrap();
    let err = client
        .recall_with_proof(&ProjectId::new("demo-repo"), "retries", 5)
        .unwrap_err();
    assert!(err.to_string().contains("no record list"), "{err}");
}

#[test]
fn a_record_without_an_id_is_named_the_same_way_every_time() {
    let result = json!({"structuredContent": {"memories": [
        {"project": "demo-repo", "kind": "fact", "text": "Fact: no id here."}
    ], "proof": null}});
    let mut client = HyphaeMemoryClient::connect(transport(vec![result])).unwrap();
    let first = client.recall(&ProjectId::new("demo-repo"), "a", 5).unwrap();
    let second = client.recall(&ProjectId::new("demo-repo"), "b", 5).unwrap();
    assert!(first[0].id.as_str().starts_with("hye-"), "{}", first[0].id);
    assert_eq!(first[0].id, second[0].id);
}

fn temp_log(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("hyphae-consensus-isolation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// A two-claim run whose Critic finds nothing and whose Evaluator cites
/// record `cited` for the first claim.
fn two_claim_model(cited: &[&str], corroborated: bool) -> FakeModelClient {
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal(
        "p1",
        "Cap retries",
        vec![
            claim("c1", "Outbound retries are capped at 3 attempts"),
            claim("c2", "Every service logs retries"),
        ],
    )));
    model.critiques.push_back(Ok(Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new("chal-1"),
        run_id: RunId::new("placeholder"),
        target_proposal: ProposalId::new("placeholder"),
        items: vec![],
        created_at: TS.into(),
    }));
    model.evaluations.push_back(Ok(evaluation(
        vec![
            assessment("c1", corroborated, cited, &[]),
            assessment("c2", false, &[], &[]),
        ],
        &[],
        false,
    )));
    model
}

fn run<Mem: MemoryClient>(
    name: &str,
    model: FakeModelClient,
    memory: Mem,
) -> (RunAggregate, EventLog, String) {
    let path = temp_log(name);
    let mut log = EventLog::open(&path).unwrap();
    let mut orch = Orchestrator {
        model,
        memory,
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let input = TaskInput {
        project_id: ProjectId::new("demo-repo"),
        title: "Cap payment retries".into(),
        description: "Retries burst under load.".into(),
        submitted_by: "test".into(),
        mode: None,
    };
    let agg = orch.run(&mut log, input).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    (agg, log, text)
}

#[test]
fn the_orchestrator_records_every_proof_and_every_dropped_record() {
    // Claim 1's recall returns the mixed result; claim 2's is a proven
    // empty result (isolation evidence).
    let memory = HyphaeMemoryClient::connect(transport(vec![
        mixed_result(),
        proven(vec![], "recall-empty"),
    ]))
    .unwrap();
    let (agg, log, text) = run(
        "orchestrated-recall.jsonl",
        two_claim_model(&["1"], false),
        memory,
    );
    let retrieved: Vec<&str> = agg.retrieved.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        retrieved,
        ["1", "2"],
        "foreign records never become evidence"
    );
    let events = log.all_events();
    let dropped: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::EvidenceUnavailable { code, reason, .. }
                if code.as_deref() == Some("foreign_record") =>
            {
                Some(reason.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(dropped.len(), 2, "{dropped:?}");
    assert!(
        dropped[0].contains("\"3\"") && dropped[0].contains("\"tenant-b\""),
        "{}",
        dropped[0]
    );
    assert!(
        dropped[1].contains("\"4\"") && dropped[1].contains("without naming its project"),
        "{}",
        dropped[1]
    );
    assert!(
        !text.contains("Tenant B private constraint"),
        "a foreign record's content is never persisted"
    );
    let proofs: Vec<(Vec<&str>, &str, bool)> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::RecallProven {
                evidence_ids,
                proof_ref,
                empty,
                anchor,
                request,
            } => {
                assert_eq!(anchor, ANCHOR);
                assert_eq!(request.operation, "recall");
                Some((
                    evidence_ids.iter().map(|i| i.as_str()).collect(),
                    proof_ref.as_str(),
                    *empty,
                ))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        proofs,
        [
            (vec!["1", "2", "3", "4"], PROOF_ID, false),
            (vec![], "recall-empty", true)
        ]
    );
    // Verified records name their proof; nothing foreign is verified.
    for v in &agg.verifications {
        assert!(["1", "2"].contains(&v.evidence_id.as_str()));
        assert_eq!(v.proof_ref.as_deref(), Some(PROOF_ID));
    }
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "replays");
}

/// A memory backend that answers every recall with one scripted result.
struct ScriptedRecall(Recall);

impl MemoryClient for ScriptedRecall {
    fn recall(
        &mut self,
        _: &ProjectId,
        _: &str,
        _: usize,
    ) -> Result<Vec<EvidenceReference>, MemoryError> {
        Ok(self.0.records.clone())
    }
    fn recall_with_proof(
        &mut self,
        _: &ProjectId,
        _: &str,
        _: usize,
    ) -> Result<Recall, MemoryError> {
        Ok(self.0.clone())
    }
    fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
        Ok(MemoryStatus {
            healthy: true,
            detail: "scripted".into(),
        })
    }
}

fn failed_proof(reason: &str, category: Option<&str>) -> ScriptedRecall {
    let mut rec = consensus_runtime::scenarios::evidence(
        "1",
        "demo-repo",
        "constraint",
        "Constraint: outbound retries are capped at 3 attempts.",
    );
    rec.proof_error = Some(reason.into());
    ScriptedRecall(Recall {
        records: vec![rec],
        proof: None,
        proof_failure: Some(ProofFailure {
            category: category.map(str::to_string),
            message: "native durable state is invalid".into(),
        }),
        foreign: vec![],
    })
}

#[test]
fn the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it() {
    // Typed `corruption`, no keyword in the text: Invalid, blocked, and
    // the detail says what Hyphae's generic code can mean.
    let memory = failed_proof(
        "tool hyphae_memory_recall failed (state): native durable state is invalid",
        Some("corruption"),
    );
    let (agg, _, _) = run(
        "typed-corruption.jsonl",
        two_claim_model(&["1"], true),
        memory,
    );
    let v = &agg.verifications[0];
    assert_eq!(v.integrity_status, IntegrityStatus::Invalid);
    assert_eq!(v.verifier.as_deref(), Some("recall(prove) upstream"));
    let detail = v.detail.as_deref().unwrap();
    assert!(detail.contains("generic corruption category"), "{detail}");
    assert!(
        detail.contains("capacity and configuration failures"),
        "{detail}"
    );
    assert!(detail.contains("fail-closed"), "{detail}");
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Blocked)
    );

    // Another category, but the text names corruption: still Invalid (the
    // keyword rule is never narrowed).
    let memory = failed_proof(
        "memory backend error: tool failed (io): corrupt witness",
        Some("io"),
    );
    let (agg, _, _) = run(
        "keyword-corruption.jsonl",
        two_claim_model(&["1"], true),
        memory,
    );
    assert_eq!(
        agg.verifications[0].integrity_status,
        IntegrityStatus::Invalid
    );

    // Neither: Unavailable, never supported.
    let memory = failed_proof(
        "memory backend error: tool failed (io): native filesystem operation failed",
        Some("io"),
    );
    let (agg, _, _) = run("io-failure.jsonl", two_claim_model(&["1"], true), memory);
    let v = &agg.verifications[0];
    assert_eq!(v.integrity_status, IntegrityStatus::Unavailable);
    assert!(v.detail.as_deref().unwrap().contains("No verifier ran"));
    assert_ne!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
}

struct ScriptedRunner(Result<CommandOutput, CommandError>, Vec<Vec<String>>);

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, argv: &[String], _timeout: Duration) -> Result<CommandOutput, CommandError> {
        self.1.push(argv.to_vec());
        self.0.clone()
    }
}

fn exited(code: i32, stdout: &str, stderr: &str) -> Result<CommandOutput, CommandError> {
    Ok(CommandOutput {
        code,
        stdout: stdout.into(),
        stderr: stderr.into(),
    })
}

fn verifier(outcome: Result<CommandOutput, CommandError>) -> HyphaeProofVerifier<ScriptedRunner> {
    HyphaeProofVerifier::new("hyphae".into(), ScriptedRunner(outcome, vec![]))
}

fn proven_record(proof: &str, witness: &str) -> EvidenceReference {
    let mut rec = consensus_runtime::scenarios::evidence("mem-1", "demo-repo", "constraint", "x");
    rec.retrieval = Some(Retrieval::LiveRecall);
    rec.anchor = Some(ANCHOR.into());
    rec.proof_file = Some(proof.into());
    rec.witness_file = Some(witness.into());
    rec
}

fn report_with(key: &str, value: Value) -> String {
    let mut report: Value = serde_json::from_str(VERIFIED_REPORT).unwrap();
    match value {
        Value::Null => {
            report.as_object_mut().unwrap().remove(key);
        }
        value => report[key] = value,
    }
    report.to_string()
}

#[test]
fn valid_requires_a_reexecuted_memory_proof_bound_to_the_anchor_given() {
    let record = proven_record("/hyphae/p.proof", "/hyphae/w.witness");
    // The captured report: Valid, with its fields kept.
    let check = verifier(exited(0, VERIFIED_REPORT, "")).verify(&record);
    assert_eq!(check.status, IntegrityStatus::Valid, "{:?}", check.detail);
    assert_eq!(
        check.report,
        Some(VerifierReport {
            status: Some("verified".into()),
            scope: Some("semantic_reexecution".into()),
            kind: Some("memory".into()),
            anchor_digest: Some(ANCHOR.into()),
            result_digest: Some(
                "3d6b60f10d75f3abd56d39025ca380489bca2f2d83b63d22abcc8cfcf91bac70".into()
            ),
            proof_digest: Some(
                "6a5f2fa6b846a707d1c6b77c9e7c8b3ce51df1e99dee2a7d3f5d52380474edbb".into()
            ),
        })
    );
    let detail = check.detail.as_deref().unwrap();
    assert!(detail.contains("not byte-compared"), "{detail}");
    assert!(
        detail.contains("none pinned") && detail.contains(ANCHOR),
        "an unpinned anchor is named as the backend's: {detail}"
    );

    for (label, report, expected) in [
        (
            "another proof kind",
            report_with("kind", json!("catalog")),
            IntegrityStatus::Invalid,
        ),
        (
            "another anchor",
            report_with("anchor_digest", json!("ab".repeat(32))),
            IntegrityStatus::Invalid,
        ),
        (
            "a lesser scope",
            report_with("scope", json!("artifact_integrity")),
            IntegrityStatus::VerificationError,
        ),
        (
            "no kind at all",
            report_with("kind", Value::Null),
            IntegrityStatus::VerificationError,
        ),
        (
            "not re-executed",
            report_with("semantic_reexecution_performed", json!(false)),
            IntegrityStatus::VerificationError,
        ),
        // Valid requires anchor_digest == the anchor given; a report that
        // names none cannot meet it, and nothing is inferred from an
        // absent field.
        (
            "no anchor digest",
            report_with("anchor_digest", Value::Null),
            IntegrityStatus::VerificationError,
        ),
    ] {
        let check = verifier(exited(0, &report, "")).verify(&record);
        assert_eq!(check.status, expected, "{label}: {:?}", check.detail);
        assert!(check.report.is_some(), "{label}: the report is kept");
    }
}

#[test]
fn the_pinned_anchor_is_normalized_and_handed_to_the_verifier() {
    let mut record = proven_record("/hyphae/p.proof", "/hyphae/w.witness");
    record.anchor = Some(ANCHOR.to_ascii_uppercase());
    let mut v = verifier(exited(0, VERIFIED_REPORT, ""));
    v.expected_anchor = Some(format!("  {}\r\n", ANCHOR.to_ascii_uppercase()));
    let check = v.verify(&record);
    assert_eq!(check.status, IntegrityStatus::Valid);
    assert!(
        check.detail.as_deref().unwrap().contains("operator-pinned"),
        "{:?}",
        check.detail
    );
    let argv = &v.runner.1[0];
    let at = argv.iter().position(|a| a == "--anchor").unwrap();
    assert_eq!(argv[at + 1], ANCHOR, "lowercase, as Hyphae's CLI requires");

    // from_config takes the pin and the deadline from the configuration.
    let vars = [
        (
            "HYPHAE_NATIVE_API_KEY_FILE",
            "/run/keys/hyphae.key".to_string(),
        ),
        ("HYPHAE_BASE_URL", "http://127.0.0.1:7700".to_string()),
        (
            "CONSENSUS_HYPHAE_EXPECTED_ANCHOR",
            format!("{}\n", ANCHOR.to_ascii_uppercase()),
        ),
        ("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS", "7".to_string()),
    ];
    let cfg = HyphaeConfig::from_vars(|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    })
    .unwrap()
    .unwrap();
    let v = HyphaeProofVerifier::from_config(&cfg, SystemCommandRunner);
    assert_eq!(v.expected_anchor.as_deref(), Some(ANCHOR));
    assert_eq!(v.timeout, Duration::from_secs(7));
}

#[test]
fn a_verifier_past_its_deadline_is_a_verification_error_with_its_code() {
    let check = verifier(Err(CommandError::TimedOut(Duration::from_secs(60))))
        .verify(&proven_record("/hyphae/p.proof", "/hyphae/w.witness"));
    assert_eq!(check.status, IntegrityStatus::VerificationError);
    let detail = check.detail.unwrap();
    assert!(detail.starts_with("verifier_timeout:"), "{detail}");
    assert!(
        detail.contains("60 s") && detail.contains("killed"),
        "{detail}"
    );
}

fn artifacts(dir: &std::path::Path, name: &str, proof: &str, witness: &str) -> (String, String) {
    std::fs::create_dir_all(dir).unwrap();
    let (p, w) = (
        dir.join(format!("{name}.proof")),
        dir.join(format!("{name}.witness")),
    );
    std::fs::write(&p, proof).unwrap();
    std::fs::write(&w, witness).unwrap();
    (
        p.to_string_lossy().into_owned(),
        w.to_string_lossy().into_owned(),
    )
}

#[test]
fn results_are_reused_by_artifact_bytes_never_by_path() {
    let dir = std::env::temp_dir().join(format!("hyphae-consensus-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Same bytes under two names: verified once.
    let (p1, w1) = artifacts(&dir, "a", "proof-1", "witness-1");
    let (p2, w2) = artifacts(&dir, "b", "proof-1", "witness-1");
    let mut v = verifier(exited(0, VERIFIED_REPORT, ""));
    assert_eq!(
        v.verify(&proven_record(&p1, &w1)).status,
        IntegrityStatus::Valid
    );
    assert_eq!(
        v.verify(&proven_record(&p2, &w2)).status,
        IntegrityStatus::Valid
    );
    assert_eq!(
        v.runner.1.len(),
        1,
        "identical artifacts, one verifier process"
    );
    // The same path with other bytes: verified again (HY-13).
    std::fs::write(&p1, "proof-1, replaced").unwrap();
    v.verify(&proven_record(&p1, &w1));
    assert_eq!(
        v.runner.1.len(),
        2,
        "a replaced artifact is never served from cache"
    );
    // Artifacts this process cannot read are never reused.
    let unreadable = proven_record("/no/such/dir/p.proof", "/no/such/dir/w.witness");
    v.verify(&unreadable);
    v.verify(&unreadable);
    assert_eq!(v.runner.1.len(), 4);
    // A verification error is never reused either.
    let io_error = r#"{"error":{"category":"io","code":"io","message":"native filesystem operation failed"},"exit_class":11}"#;
    let mut v = verifier(exited(11, "", io_error));
    let (p3, w3) = artifacts(&dir, "c", "proof-3", "witness-3");
    assert_eq!(
        v.verify(&proven_record(&p3, &w3)).status,
        IntegrityStatus::VerificationError
    );
    v.verify(&proven_record(&p3, &w3));
    assert_eq!(v.runner.1.len(), 2);
}

/// A command that runs for seconds and prints nothing (Windows: no
/// `cmd` wrapper, so it is the direct child).
fn silent_sleeper() -> Vec<String> {
    if cfg!(windows) {
        [
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 8",
        ]
        .map(String::from)
        .to_vec()
    } else {
        ["sh", "-c", "sleep 8"].map(String::from).to_vec()
    }
}

#[test]
fn the_system_runner_kills_a_command_past_its_deadline() {
    let started = Instant::now();
    let result = SystemCommandRunner.run(&silent_sleeper(), Duration::from_millis(300));
    assert_eq!(
        result,
        Err(CommandError::TimedOut(Duration::from_millis(300)))
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    // A command that finishes in time returns its exit code and output.
    let quick: Vec<String> = if cfg!(windows) {
        [
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Write-Output ok; exit 3",
        ]
        .map(String::from)
        .to_vec()
    } else {
        ["sh", "-c", "echo ok; exit 3"].map(String::from).to_vec()
    };
    let out = SystemCommandRunner
        .run(&quick, Duration::from_secs(60))
        .unwrap();
    assert_eq!(out.code, 3);
    assert_eq!(out.stdout.trim(), "ok");

    // A missing program is named by its file name only (SEC-9).
    let missing = ["/no/such/dir/hyphae-missing".to_string()];
    match SystemCommandRunner.run(&missing, Duration::from_secs(5)) {
        Err(CommandError::Spawn(what)) => {
            assert!(
                what.contains("hyphae-missing") && !what.contains("/no/such/dir"),
                "{what}"
            )
        }
        other => panic!("expected a spawn error, got {other:?}"),
    }
}

#[test]
fn hyphae_children_never_inherit_the_meta_key() {
    // MA-4: the verifier (and `hyphae mcp`, built the same way) starts
    // without any Meta key variable, whatever this process holds, and
    // without the variables that say where the server's credential files
    // are (systemd's directory and the file settings pointing into it).
    let keys = consensus_api::config::META_KEY_VARIABLES;
    let paths = CREDENTIAL_PATH_VARIABLES;
    for name in keys {
        std::env::set_var(name, "LLM_1_canary-secret");
    }
    for name in paths {
        std::env::set_var(name, "/run/credentials/canary-unit/canary-file");
    }
    let names: Vec<&str> = keys.iter().chain(paths.iter()).copied().collect();
    let echo: Vec<String> = if cfg!(windows) {
        let fields: Vec<String> = names.iter().map(|n| format!("$env:{n}")).collect();
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            format!("Write-Output \"[{}]\"", fields.join("|")),
        ]
    } else {
        let fields: Vec<String> = names.iter().map(|n| format!("${n}")).collect();
        vec![
            "sh".into(),
            "-c".into(),
            format!("echo \"[{}]\"", fields.join("|")),
        ]
    };
    let out = SystemCommandRunner.run(&echo, Duration::from_secs(60));
    for name in &names {
        std::env::remove_var(name);
    }
    let out = out.unwrap();
    let empty = format!("[{}]", "|".repeat(names.len() - 1));
    assert_eq!(out.stdout.trim(), empty, "{out:?}");
    assert!(!out.stdout.contains("canary"));
}

#[test]
fn the_stdio_transport_reports_a_dead_child_and_keeps_its_stderr_tail() {
    let dying: Vec<String> = if cfg!(windows) {
        ["cmd", "/C", "echo boom: No such container 1>&2 & exit 3"]
            .map(String::from)
            .to_vec()
    } else {
        ["sh", "-c", "echo 'boom: No such container' >&2; exit 3"]
            .map(String::from)
            .to_vec()
    };
    let mut transport = StdioTransport::spawn(&dying).unwrap();
    let err = transport.request("initialize", json!({})).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, MemoryError::Unavailable(_)), "{text}");
    assert!(
        text.contains("hyphae mcp exited") || text.contains("hyphae mcp closed its output"),
        "{text}"
    );
    assert!(!text.contains("boom"), "stderr stays out of errors: {text}");
    // The tail is kept for the operator (the reader may still be draining).
    let deadline = Instant::now() + Duration::from_secs(5);
    while !transport.stderr_tail().contains("boom") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        transport.stderr_tail().contains("boom: No such container"),
        "{}",
        transport.stderr_tail()
    );

    // A child that never answers: the request has one deadline.
    let mut silent = StdioTransport::spawn(&silent_sleeper())
        .unwrap()
        .with_timeout(Duration::from_millis(300));
    let started = Instant::now();
    let err = silent.request("initialize", json!({})).unwrap_err();
    assert!(
        err.to_string().contains("did not answer initialize"),
        "{err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );

    // A missing binary is named by its file name only.
    let err = match StdioTransport::spawn(&["/no/such/dir/hyphae-missing".to_string()]) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("spawn must fail"),
    };
    assert!(
        err.contains("hyphae-missing") && !err.contains("/no/such/dir"),
        "{err}"
    );
}
