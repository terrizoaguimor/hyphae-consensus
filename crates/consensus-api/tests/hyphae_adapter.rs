//! Hyphae adapter tests over an in-memory MCP transport and a scripted
//! command runner. These verify OUR protocol handling and status
//! mapping — they do not simulate Hyphae's cryptography. The live probe
//! against a real binary is `#[ignore]`d and env-gated.

use consensus_api::config::HyphaeConfig;
use consensus_api::hyphae::{
    connect_from_config, CommandRunner, HyphaeMemoryClient, HyphaeProofVerifier, InMemoryTransport,
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

struct ScriptedRunner(Result<(i32, String), String>, Vec<Vec<String>>);

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, argv: &[String]) -> Result<(i32, String), String> {
        self.1.push(argv.to_vec());
        self.0.clone()
    }
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
    let mut ok = HyphaeProofVerifier::new(
        "hyphae".into(),
        ScriptedRunner(Ok((0, String::new())), vec![]),
    );
    let check = ok.verify(&evidence_with_proof());
    assert_eq!(check.status, IntegrityStatus::Valid);
    let argv = &ok.runner.1[0];
    assert_eq!(&argv[1..3], ["proof".to_string(), "verify".to_string()]);
    assert!(argv.contains(&"--anchor".to_string()));

    let mut bad = HyphaeProofVerifier::new(
        "hyphae".into(),
        ScriptedRunner(Ok((1, "digest mismatch".into())), vec![]),
    );
    let check = bad.verify(&evidence_with_proof());
    assert_eq!(check.status, IntegrityStatus::Invalid);
    assert!(check.detail.unwrap().contains("digest mismatch"));

    let mut spawn_fail = HyphaeProofVerifier::new(
        "hyphae-missing".into(),
        ScriptedRunner(Err("spawn hyphae-missing: not found".into()), vec![]),
    );
    assert_eq!(
        spawn_fail.verify(&evidence_with_proof()).status,
        IntegrityStatus::VerificationError
    );
}

#[test]
fn transport_exit_codes_are_verification_errors_not_invalid() {
    // Docker/usage failures (125-127, unknown flags) are infrastructure
    // errors; only an actual verifier rejection is Invalid.
    let mut docker_down = HyphaeProofVerifier::new(
        "wrapper".into(),
        ScriptedRunner(
            Ok((125, "docker: Cannot connect to the Docker daemon".into())),
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
    let mut v = HyphaeProofVerifier::new(
        "hyphae".into(),
        ScriptedRunner(Ok((0, String::new())), vec![]),
    );
    v.expected_anchor = Some("cd".repeat(32));
    let check = v.verify(&evidence_with_proof());
    assert_eq!(check.status, IntegrityStatus::Invalid);
    assert!(v.runner.1.is_empty(), "pinned-anchor check must run first");
}

#[test]
fn missing_proof_artifacts_are_unavailable_never_valid() {
    let mut v = HyphaeProofVerifier::new(
        "hyphae".into(),
        ScriptedRunner(Ok((0, String::new())), vec![]),
    );
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
/// Hyphae deployment (never the primary Agent Memory data):
/// `cargo test -p consensus-api --test hyphae_adapter live_ -- --ignored`
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
}
