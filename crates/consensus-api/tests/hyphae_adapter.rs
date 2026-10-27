//! Hyphae adapter tests over an in-memory MCP transport and a scripted
//! command runner. These verify OUR protocol handling and status
//! mapping — they do not simulate Hyphae's cryptography. The live probe
//! against a real binary is `#[ignore]`d and env-gated.

use consensus_api::config::HyphaeConfig;
use consensus_api::hyphae::{
    connect_from_config, CommandOutput, CommandRunner, HyphaeMemoryClient, HyphaeProofVerifier,
    InMemoryTransport, SystemCommandRunner,
};
use consensus_domain::*;
use consensus_runtime::ports::{MemoryClient, ProofVerifier};
use serde_json::{json, Value};

fn transport_with(recall_result: Value) -> InMemoryTransport {
    let mut t = InMemoryTransport::default();
    t.script(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "hyphae"}}),
    );
    t.script(
        "tools/list",
        json!({"tools": [
            {"name": "hyphae_memory_recall"},
            {"name": "hyphae_memory_status"}
        ]}),
    );
    t.script("tools/call", recall_result);
    t
}

#[test]
fn handshake_and_recall_map_records_and_request_proofs() {
    let t = transport_with(json!({
        "structuredContent": {"memories": [{
            "id": "mem-42",
            "text": "Constraint: retries capped at 3 attempts",
            "kind": "constraint",
            "anchor_digest": "a1b2",
            "proof_path": "/tmp/proofs/p.hyproof",
            "witness_path": "/tmp/proofs/w.hywit"
        }]}
    }));
    let mut client = HyphaeMemoryClient::connect(t).unwrap();
    let out = client
        .recall(&ProjectId::new("demo-repo"), "retries", 5)
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id, EvidenceId::new("mem-42"));
    assert_eq!(out[0].backend_ref.as_deref(), Some("hyphae-memory:mem-42"));
    assert!(out[0].content.contains("retries capped at 3"));
    assert_eq!(out[0].kind, "constraint");
    assert_eq!(out[0].anchor.as_deref(), Some("a1b2"));
    assert_eq!(out[0].proof_file.as_deref(), Some("/tmp/proofs/p.hyproof"));

    let t = client.into_transport();
    let recall_call = t
        .requests
        .iter()
        .find(|(m, p)| m == "tools/call" && p["name"] == "hyphae_memory_recall")
        .expect("recall tools/call recorded");
    assert_eq!(recall_call.1["arguments"]["prove"], json!(true));
    assert_eq!(recall_call.1["arguments"]["project"], json!("demo-repo"));
    assert!(t.requests.iter().any(|(m, _)| m == "initialize"));
}

#[test]
fn recall_accepts_text_content_fallback() {
    let inner =
        r#"[{"id":"m1","content":"Fact: timeouts correlate with retry bursts","kind":"fact"}]"#;
    let t = transport_with(json!({"content": [{"type": "text", "text": inner}]}));
    let mut client = HyphaeMemoryClient::connect(t).unwrap();
    let out = client.recall(&ProjectId::new("p"), "retry", 3).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind, "fact");
}

#[test]
fn recall_falls_back_to_unproven_when_proof_generation_fails() {
    // Mirrors the observed pinned-build behavior (envelope captured live
    // from the Linux 4.0.0 container on 2026-10-26): prove=true returns a
    // tool-error envelope inside structuredContent, plain recall works.
    let mut t = transport_with(json!({
        "isError": false,
        "structuredContent": {
            "schema": "hyphae-native-mcp-tool-error-v2",
            "error": {
                "category": "corruption",
                "code": "corruption",
                "message": "native durable state is invalid",
                "retry": "after-recovery"
            }
        }
    }));
    t.script(
        "tools/call",
        json!({
            "structuredContent": {"memories": [{
                "id": "77", "text": "Constraint: cap retries at 3", "kind": "constraint"
            }], "proof": null}
        }),
    );
    let mut client = HyphaeMemoryClient::connect(t).unwrap();
    let out = client
        .recall(&ProjectId::new("demo-repo"), "retries", 5)
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].content.contains("cap retries"));
    assert!(
        out[0].proof_file.is_none(),
        "unproven fallback must not fabricate artifacts"
    );
    // The failure reason travels with the record, category included: the
    // live message alone ("native durable state is invalid") never says
    // "corruption", and the runtime classifies on that word.
    let reason = out[0].proof_error.as_deref().expect("proof_error recorded");
    assert!(reason.contains("corruption"), "{reason}");
    assert!(
        reason.contains("native durable state is invalid"),
        "{reason}"
    );
    let calls: Vec<Value> = client
        .into_transport()
        .requests
        .iter()
        .filter(|(m, _)| m == "tools/call")
        .map(|(_, p)| p.clone())
        .collect();
    assert_eq!(calls.len(), 2, "expected proven attempt + unproven retry");
    assert_eq!(calls[0]["arguments"]["prove"], json!(true));
    assert_eq!(calls[1]["arguments"]["prove"], json!(false));
}

#[test]
fn proven_recall_attaches_the_shared_proof_to_every_record() {
    // Envelope captured live (Linux 4.0.0 container, 2026-10-27) after the
    // daemon moved to its canonical socket: one proof for the result.
    let t = transport_with(json!({
        "isError": false,
        "structuredContent": {
            "expired_filtered": 0,
            "memories": [
                {"id": "186670067796945807561772405393844769735", "kind": "constraint",
                 "layer": "work", "project": "demo-repo", "scope": "project", "score": 0.016,
                 "text": "Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage."},
                {"id": "8864874238917673930644756936982232317", "kind": "fact",
                 "layer": "work", "project": "demo-repo", "scope": "project", "score": 0.016,
                 "text": "Fact: payment client timeouts correlate with retry bursts above 5 attempts."}
            ],
            "proof": {
                "anchor_hex": "e9cf8c51ad12358397bcd1a387d73b0778482070b1a6133e3f5d9351bbc22621",
                "proof_blake3": "1ee5c45db81db52e0bb743f9dc2fe1bb99eb88ebe661037f0aa608ad7089df79",
                "proof_path": "/hyphae-home/.local/state/hyphae/proofs/recall-01a17bf3-879a-7833-94a4-361661d67761.proof",
                "scope": "memory-snapshot-v1",
                "witness_path": "/hyphae-home/.local/state/hyphae/proofs/recall-01a17bf3-879a-7833-94a4-361661d67761.witness"
            },
            "retrieval_mode": "lexical",
            "semantic_status": "disabled",
            "snapshot": {"logical_time_micros": 1793080919911004_i64,
                         "root_digest": "2cf23a5b0239cec20d63ac2d244aec932a617700107c0ace3b322e99de023693",
                         "visible_csn": 26}
        }
    }));
    let mut client = HyphaeMemoryClient::connect(t).unwrap();
    let out = client
        .recall(&ProjectId::new("demo-repo"), "retries attempts", 5)
        .unwrap();
    assert_eq!(out.len(), 2);
    for rec in &out {
        assert!(rec.proof_error.is_none(), "{:?}", rec.proof_error);
        assert_eq!(
            rec.anchor.as_deref(),
            Some("e9cf8c51ad12358397bcd1a387d73b0778482070b1a6133e3f5d9351bbc22621")
        );
        assert!(rec.proof_file.as_deref().unwrap().ends_with(".proof"));
        assert!(rec.witness_file.as_deref().unwrap().ends_with(".witness"));
    }
    // Exactly one recall: the proven call succeeded, no unproven retry.
    let calls = client
        .into_transport()
        .requests
        .iter()
        .filter(|(m, _)| m == "tools/call")
        .count();
    assert_eq!(calls, 1);
}

#[test]
fn proven_recall_without_artifacts_is_recorded_never_silent() {
    let t = transport_with(json!({
        "structuredContent": {"memories": [{
            "id": "7", "text": "Fact: retries capped at 3", "kind": "fact"
        }], "proof": null}
    }));
    let mut client = HyphaeMemoryClient::connect(t).unwrap();
    let out = client
        .recall(&ProjectId::new("demo-repo"), "retries", 5)
        .unwrap();
    let reason = out[0].proof_error.as_deref().expect("proof_error recorded");
    assert!(reason.contains("no proof artifacts"), "{reason}");
    assert!(
        !reason.contains("corrupt"),
        "a missing proof is unavailable, not tampering"
    );
}

#[test]
fn connect_rejects_surface_without_memory_recall() {
    let mut t = InMemoryTransport::default();
    t.script("initialize", json!({}));
    t.script(
        "tools/list",
        json!({"tools": [{"name": "hyphae_native_search_lexical"}]}),
    );
    match HyphaeMemoryClient::connect(t) {
        Ok(_) => panic!("connect must refuse a non-memory surface"),
        Err(e) => assert!(e.to_string().contains("hyphae_memory_recall"), "{e}"),
    }
}

struct ScriptedRunner(Result<CommandOutput, String>, Vec<Vec<String>>);

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, argv: &[String]) -> Result<CommandOutput, String> {
        self.1.push(argv.to_vec());
        self.0.clone()
    }
}

/// `hyphae proof verify` success report, captured live from the Linux
/// 4.0.0 container on 2026-10-27 (recall proof over `demo-repo`).
const VERIFIED_REPORT: &str = r#"{
  "anchor_digest": "e9cf8c51ad12358397bcd1a387d73b0778482070b1a6133e3f5d9351bbc22621",
  "directory_count": 5,
  "evidence_digest": "086b8215c145dd4802491d3be49276ef1a2a8a37418297e34b12c748893cab9b",
  "file_count": 5,
  "kind": "memory",
  "proof_digest": "6a5f2fa6b846a707d1c6b77c9e7c8b3ce51df1e99dee2a7d3f5d52380474edbb",
  "request_digest": "008663b572b3a1a3e79cac036fcb2f2adba29c4777aec61b28e06f57bd4a2068",
  "result_digest": "3d6b60f10d75f3abd56d39025ca380489bca2f2d83b63d22abcc8cfcf91bac70",
  "scope": "semantic_reexecution",
  "semantic_reexecution_performed": true,
  "status": "verified",
  "total_file_bytes": 3261022,
  "witness_digest": "216a0a01061de84f59d9d5543b1415ed510c7e549e62d99c8ed97d68f7b7a799"
}"#;

fn exited(code: i32, stdout: &str, stderr: &str) -> Result<CommandOutput, String> {
    Ok(CommandOutput {
        code,
        stdout: stdout.into(),
        stderr: stderr.into(),
    })
}

fn verifier(outcome: Result<CommandOutput, String>) -> HyphaeProofVerifier<ScriptedRunner> {
    HyphaeProofVerifier::new("hyphae".into(), ScriptedRunner(outcome, vec![]))
}

fn evidence_with_proof() -> EvidenceReference {
    EvidenceReference {
        schema_version: SCHEMA_VERSION.into(),
        id: EvidenceId::new("mem-1"),
        project_id: ProjectId::new("p"),
        backend_ref: Some("hyphae-memory:mem-1".into()),
        kind: "constraint".into(),
        content: "x".into(),
        retrieved_at: "2026-10-26T00:00:00Z".into(),
        anchor: Some("ab".repeat(32)),
        proof_file: Some("/tmp/p.hyproof".into()),
        witness_file: Some("/tmp/w.hywit".into()),
        proof_error: None,
    }
}

#[test]
fn proof_verifier_maps_exit_codes_to_integrity_only() {
    let mut ok = verifier(exited(0, VERIFIED_REPORT, ""));
    let check = ok.verify(&evidence_with_proof());
    assert_eq!(check.status, IntegrityStatus::Valid);
    let detail = check.detail.unwrap();
    assert!(detail.contains("semantic_reexecution"), "{detail}");
    assert!(
        detail.contains("3d6b60f10d75"),
        "result digest kept: {detail}"
    );
    let argv = &ok.runner.1[0];
    assert_eq!(&argv[1..3], ["proof".to_string(), "verify".to_string()]);
    assert!(argv.contains(&"--anchor".to_string()));

    let mut bad = verifier(exited(1, "", "digest mismatch"));
    let check = bad.verify(&evidence_with_proof());
    assert_eq!(check.status, IntegrityStatus::Invalid);
    assert!(check.detail.unwrap().contains("digest mismatch"));

    let mut spawn_fail = verifier(Err("spawn hyphae-missing: not found".into()));
    assert_eq!(
        spawn_fail.verify(&evidence_with_proof()).status,
        IntegrityStatus::VerificationError
    );
}

#[test]
fn exit_zero_without_a_verified_reexecution_report_is_not_valid() {
    // `Valid` needs the verifier's own report, not just an exit code.
    for stdout in [
        "",
        "ok",
        r#"{"status":"verified","scope":"artifact_integrity","semantic_reexecution_performed":false}"#,
    ] {
        let mut v = verifier(exited(0, stdout, ""));
        assert_eq!(
            v.verify(&evidence_with_proof()).status,
            IntegrityStatus::VerificationError,
            "{stdout:?}"
        );
    }
}

#[test]
fn verifier_failure_categories_captured_live_map_to_integrity() {
    // Exact stderr from `hyphae proof verify` in the container, 2026-10-27.
    let tampered_witness = r#"{"error":{"category":"corruption","code":"corruption","message":"native durable state is invalid","object_id":null,"request_id":null,"retry":"after-recovery","trace_id":null,"transaction_id":null,"transaction_state":"none"},"exit_class":9}"#;
    let wrong_anchor = r#"{"error":{"category":"invalid-request","code":"invalid_request","message":"native product request is invalid","object_id":null,"request_id":null,"retry":"never","trace_id":null,"transaction_id":null,"transaction_state":"none"},"exit_class":2}"#;
    let missing_file = r#"{"error":{"category":"io","code":"io","message":"native filesystem operation failed","object_id":null,"request_id":null,"retry":"after-recovery","trace_id":null,"transaction_id":null,"transaction_state":"none"},"exit_class":11}"#;
    for (code, stderr, expected) in [
        (9, tampered_witness, IntegrityStatus::Invalid),
        (2, wrong_anchor, IntegrityStatus::Invalid),
        (11, missing_file, IntegrityStatus::VerificationError),
    ] {
        let mut v = verifier(exited(code, "", stderr));
        let check = v.verify(&evidence_with_proof());
        assert_eq!(check.status, expected, "exit {code}");
        assert!(check.detail.unwrap().contains("category"), "exit {code}");
    }
}

#[test]
fn one_proof_covering_several_records_is_verified_once() {
    let mut v = verifier(exited(0, VERIFIED_REPORT, ""));
    let first = evidence_with_proof();
    let mut second = evidence_with_proof();
    second.id = EvidenceId::new("mem-2");
    assert_eq!(v.verify(&first).status, IntegrityStatus::Valid);
    assert_eq!(v.verify(&second).status, IntegrityStatus::Valid);
    assert_eq!(v.runner.1.len(), 1, "same proof, one verifier process");
    let mut other_proof = evidence_with_proof();
    other_proof.proof_file = Some("/tmp/other.proof".into());
    v.verify(&other_proof);
    assert_eq!(v.runner.1.len(), 2);
}

#[test]
fn transport_exit_codes_are_verification_errors_not_invalid() {
    // Docker/usage failures (125-127, unknown flags) are infrastructure
    // errors; only an actual verifier rejection is Invalid.
    let mut docker_down = HyphaeProofVerifier::new(
        "wrapper".into(),
        ScriptedRunner(
            exited(125, "", "docker: Cannot connect to the Docker daemon"),
            vec![],
        ),
    );
    assert_eq!(
        docker_down.verify(&evidence_with_proof()).status,
        IntegrityStatus::VerificationError
    );
}

#[test]
fn pinned_anchor_mismatch_is_invalid_without_running_verifier() {
    let mut v = verifier(exited(0, VERIFIED_REPORT, ""));
    v.expected_anchor = Some("cd".repeat(32));
    let check = v.verify(&evidence_with_proof());
    assert_eq!(check.status, IntegrityStatus::Invalid);
    assert!(v.runner.1.is_empty(), "pinned-anchor check must run first");
}

#[test]
fn missing_proof_artifacts_are_unavailable_never_valid() {
    let mut v = verifier(exited(0, VERIFIED_REPORT, ""));
    let mut ev = evidence_with_proof();
    ev.proof_file = None;
    let check = v.verify(&ev);
    assert_eq!(check.status, IntegrityStatus::Unavailable);
    assert!(
        v.runner.1.is_empty(),
        "verifier must not run without artifacts"
    );
}

/// Live compatibility probe against an installed Hyphae runtime.
/// Requires `CONSENSUS_LIVE_TESTS=1`, `HYPHAE_NATIVE_API_KEY_FILE`, and
/// `HYPHAE_NATIVE_ENDPOINT` or `HYPHAE_BASE_URL` pointing at an ISOLATED
/// Hyphae deployment (never the primary Agent Memory data), plus
/// `CONSENSUS_HYPHAE_BIN` when `hyphae` is not on PATH. Optional:
/// `CONSENSUS_LIVE_PROJECT` (default `demo-repo`, must hold records).
/// `cargo test -p consensus-api --test hyphae_adapter live_ -- --ignored --nocapture`
///
/// Beyond the handshake it performs a proven recall and verifies every
/// returned record's proof end-to-end through this adapter, then checks
/// that the same artifacts are rejected against a different anchor.
#[test]
#[ignore = "live Hyphae probe; requires isolated runtime + env config"]
fn live_hyphae_probe() {
    assert_eq!(std::env::var("CONSENSUS_LIVE_TESTS").as_deref(), Ok("1"));
    let cfg = HyphaeConfig::from_env().expect("hyphae env config required");
    let mut client = connect_from_config(&cfg).unwrap();
    println!("advertised tools: {:?}", client.tools);
    let status = client.status().unwrap();
    assert!(status.healthy);
    println!("status: {}", status.detail);

    let project = std::env::var("CONSENSUS_LIVE_PROJECT").unwrap_or_else(|_| "demo-repo".into());
    let records = client
        .recall(&ProjectId::new(&project), "retries attempts", 5)
        .unwrap();
    assert!(
        !records.is_empty(),
        "project {project} holds no matching records"
    );
    let mut verifier = HyphaeProofVerifier::new(cfg.binary.clone(), SystemCommandRunner);
    verifier.expected_anchor = cfg.expected_anchor.clone();
    for rec in &records {
        assert!(rec.proof_error.is_none(), "{:?}", rec.proof_error);
        let check = verifier.verify(rec);
        println!(
            "record {} kind={} anchor={:?}\n  proof={:?}\n  -> {:?} via {:?}: {}",
            rec.id,
            rec.kind,
            rec.anchor,
            rec.proof_file,
            check.status,
            check.verifier,
            check.detail.as_deref().unwrap_or("")
        );
        assert_eq!(check.status, IntegrityStatus::Valid);
    }

    // Negative control through the real verifier: one anchor digit off.
    let mut forged = records[0].clone();
    let anchor = forged.anchor.clone().unwrap();
    let flipped = if anchor.starts_with('0') { "1" } else { "0" };
    forged.anchor = Some(format!("{flipped}{}", &anchor[1..]));
    let check = HyphaeProofVerifier::new(cfg.binary.clone(), SystemCommandRunner).verify(&forged);
    println!(
        "wrong anchor -> {:?}: {}",
        check.status,
        check.detail.as_deref().unwrap_or("")
    );
    assert_eq!(check.status, IntegrityStatus::Invalid);
}
