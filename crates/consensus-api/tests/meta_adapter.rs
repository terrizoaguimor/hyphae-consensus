//! Meta adapter tests against a local fake HTTP server. The fake speaks
//! the documented Chat Completions envelope; no network or credentials
//! are involved. One live test exists but is `#[ignore]`d and doubly
//! gated (env flag + API key).

use consensus_api::config::MetaConfig;
use consensus_api::meta::MetaModelClient;
use consensus_domain::*;
use consensus_runtime::ports::{ModelClient, ModelError, RoleContext};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct FakeServer {
    base_url: String,
    hits: Arc<AtomicUsize>,
    last_body: Arc<Mutex<String>>,
}

/// Serves `responses` in order (status, body), repeating the last one.
fn serve(responses: Vec<(u16, String)>) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let last_body = Arc::new(Mutex::new(String::new()));
    let hits2 = hits.clone();
    let last2 = last_body.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let n = hits2.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0u8; 65536];
            let read = s.read(&mut buf).unwrap_or(0);
            *last2.lock().unwrap() = String::from_utf8_lossy(&buf[..read]).to_string();
            let (status, body) = &responses[n.min(responses.len() - 1)];
            let resp = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    FakeServer {
        base_url: format!("http://{addr}/v1"),
        hits,
        last_body,
    }
}

fn envelope(content: &str) -> String {
    serde_json::json!({
        "id": "chatcmpl-test-1",
        "model": "muse-wonder-1.0",
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 50 }
    })
    .to_string()
}

fn client(base_url: String) -> MetaModelClient {
    MetaModelClient::new(MetaConfig {
        base_url,
        api_key: "test-key-not-a-secret".into(),
        model: "muse-wonder-1.0".into(),
        timeout: Duration::from_secs(5),
        max_retries: 1,
    })
    .unwrap()
}

fn task() -> Task {
    Task {
        schema_version: SCHEMA_VERSION.into(),
        id: TaskId::new("task-1"),
        run_id: RunId::new("run-1"),
        project_id: ProjectId::new("demo-repo"),
        title: "Fix payment timeouts".into(),
        description: "Timeouts under load".into(),
        created_at: "2026-10-26T00:00:00Z".into(),
    }
}

const DRAFT_PROPOSAL: &str = r#"{"summary":"Raise retries","plan_steps":["edit config"],"claims":[{"statement":"Timeouts come from too few retries"}],"assumptions":["upstream is tolerant"],"uncertainties":[]}"#;

#[test]
fn propose_happy_path_assigns_ids_and_records_exact_model() {
    let server = serve(vec![(200, envelope(DRAFT_PROPOSAL))]);
    let mut c = client(server.base_url.clone());
    let (proposal, inv) = c.propose(&task()).unwrap();
    assert_eq!(proposal.run_id, RunId::new("run-1"));
    assert_eq!(proposal.claims.len(), 1);
    assert!(proposal.claims[0].id.as_str().starts_with("claim-"));
    assert_eq!(inv.provider, "meta");
    assert_eq!(inv.model, "muse-wonder-1.0");
    assert_eq!(inv.request_id.as_deref(), Some("chatcmpl-test-1"));
    assert_eq!(inv.input_tokens, Some(100));
    // The request must use the documented structured-output field.
    let body = server.last_body.lock().unwrap().clone();
    assert!(
        body.contains("\"response_format\""),
        "request body was: {body}"
    );
    assert!(body.contains("json_schema"));
    assert!(body.contains("DraftProposal"));
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
}

#[test]
fn malformed_content_is_malformed_output_error() {
    let server = serve(vec![(200, envelope("this is not json"))]);
    let mut c = client(server.base_url);
    match c.propose(&task()) {
        Err(ModelError::MalformedOutput(_)) => {}
        other => panic!("expected MalformedOutput, got {other:?}"),
    }
}

#[test]
fn persistent_500_retries_once_then_unavailable() {
    let server = serve(vec![(500, "{\"error\":\"boom\"}".into())]);
    let mut c = client(server.base_url.clone());
    match c.propose(&task()) {
        Err(ModelError::Unavailable(msg)) => assert!(msg.contains("500"), "{msg}"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
    assert_eq!(server.hits.load(Ordering::SeqCst), 2, "1 initial + 1 retry");
}

#[test]
fn evaluate_filters_unknown_ids_and_completes_omitted_claims() {
    let draft_eval = r#"{"assessments":[{"claim_id":"claim-1","corroborated":true,"relevant_evidence":["ev-real","ev-hallucinated"],"contradicting_evidence":[],"note":"ok"}],"unresolved_challenges":["chal-unknown"],"abstain":false,"abstain_reason":null}"#;
    let server = serve(vec![(200, envelope(draft_eval))]);
    let mut c = client(server.base_url);
    let proposal = Proposal {
        schema_version: SCHEMA_VERSION.into(),
        id: ProposalId::new("prop-1"),
        run_id: RunId::new("run-1"),
        author_role: AgentRole::Proposer,
        summary: "s".into(),
        plan_steps: vec![],
        claims: vec![
            Claim {
                id: ClaimId::new("claim-1"),
                statement: "a".into(),
                evidence_refs: vec![],
            },
            Claim {
                id: ClaimId::new("claim-2"),
                statement: "b".into(),
                evidence_refs: vec![],
            },
        ],
        assumptions: vec![],
        uncertainties: vec![],
        created_at: "2026-10-26T00:00:00Z".into(),
    };
    let challenge = Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new("chal-1"),
        run_id: RunId::new("run-1"),
        target_proposal: ProposalId::new("prop-1"),
        items: vec![],
        created_at: "2026-10-26T00:00:00Z".into(),
    };
    let ctx = RoleContext {
        evidence: vec![EvidenceReference {
            schema_version: SCHEMA_VERSION.into(),
            id: EvidenceId::new("ev-real"),
            project_id: ProjectId::new("demo-repo"),
            backend_ref: None,
            kind: "fact".into(),
            content: "x".into(),
            retrieved_at: "2026-10-26T00:00:00Z".into(),
            anchor: None,
            proof_file: None,
            witness_file: None,
        }],
        integrity: vec![],
    };
    let (evaluation, _) = c.evaluate(&proposal, &challenge, &ctx).unwrap();
    assert_eq!(evaluation.assessments.len(), 2);
    assert!(evaluation.assessments[0].corroborated);
    assert_eq!(
        evaluation.assessments[0].relevant_evidence,
        vec![EvidenceId::new("ev-real")]
    );
    assert!(
        !evaluation.assessments[1].corroborated,
        "omitted claim must not be corroborated"
    );
    assert!(
        evaluation.unresolved_challenges.is_empty(),
        "unknown challenge ids are dropped"
    );
}

/// Live Meta API smoke test. Runs ONLY with `CONSENSUS_LIVE_TESTS=1` and
/// a real key (`META_API_KEY` or `MODEL_API_KEY`):
/// `cargo test -p consensus-api --test meta_adapter live_ -- --ignored`
#[test]
#[ignore = "live Meta API test; requires CONSENSUS_LIVE_TESTS=1 and an API key"]
fn live_meta_propose() {
    assert_eq!(std::env::var("CONSENSUS_LIVE_TESTS").as_deref(), Ok("1"));
    let cfg = MetaConfig::from_env().expect("META_API_KEY or MODEL_API_KEY must be set");
    let mut c = MetaModelClient::new(cfg).unwrap();
    let (proposal, inv) = c.propose(&task()).unwrap();
    assert!(!proposal.claims.is_empty());
    assert_eq!(inv.provider, "meta");
    println!(
        "live model identity: {} request: {:?}",
        inv.model, inv.request_id
    );
}
