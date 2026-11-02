// SPDX-License-Identifier: Apache-2.0
//! The single-agent baseline offline: the Meta adapter's one call and the
//! operator's `baseline` subcommand against the loopback fake of the Chat
//! Completions endpoint (a scripted model double), with the scripted
//! reference records and SIMULATED proofs. No network beyond loopback, no
//! credentials, never the real Meta API.

mod support;

use consensus_api::baseline_cli::run_with;
use consensus_api::meta::baseline::{
    baseline_prompt_sha256, baseline_system_prompt, BASELINE_SCHEMA_NAME,
};
use consensus_api::meta::MetaModelClient;
use consensus_domain::*;
use consensus_runtime::baseline::{
    read_baseline, AgentVerdict, BaselineModel, ObjectionStatus, QuerySource, RUN_KIND,
};
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::orchestrator::Orchestrator;
use consensus_runtime::ports::{ModelError, RoleContext};
use consensus_runtime::scenarios::{evidence, retry_constraint_scenario};
use consensus_runtime::{EventLog, RunLimits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use support::*;

const TITLE: &str = "Fix payment client timeouts";
const DESCRIPTION: &str = "Payment requests intermittently time out under load. Propose a fix.";

/// A draft citing the fact record the task text recalls, with one open
/// objection whose record reference names nothing retrieved.
fn draft(cited: &str) -> String {
    json!({
        "summary": "Keep retries capped; add jitter",
        "plan_steps": ["keep max_attempts at 3", "add jitter"],
        "claims": [{
            "statement": "Payment client timeouts correlate with retry bursts above 5 attempts",
            "corroborated": true,
            "relevant_evidence": [cited],
            "contradicting_evidence": [],
            "note": "the fact record states it"
        }],
        "assumptions": ["upstream tolerates jitter"],
        "uncertainties": [],
        "objections": [{
            "kind": "missing_test",
            "claim_number": "1",
            "description": "No load test shows the effect of the change on timeouts",
            "evidence_refs": ["ev-unknown"],
            "status": "open"
        }],
        "verdict": "supported",
        "verdict_reason": "claim 1 is corroborated by the fact record"
    })
    .to_string()
}

fn task() -> Task {
    Task {
        schema_version: SCHEMA_VERSION.into(),
        id: TaskId::new("task-1"),
        run_id: RunId::new("baseline-1"),
        project_id: ProjectId::new("demo-repo"),
        title: TITLE.into(),
        description: DESCRIPTION.into(),
        created_at: "2026-11-01T00:00:00Z".into(),
        mode: None,
    }
}

fn context() -> RoleContext {
    let fact = evidence(
        "ev-fact",
        "demo-repo",
        "fact",
        "Fact: payment client timeouts correlate with retry bursts above 5 attempts.",
    );
    RoleContext {
        integrity: vec![(fact.id.clone(), IntegrityStatus::Valid)],
        evidence: vec![fact],
        open_items: vec![],
        carried_contradictions: vec![],
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_one_call_is_strict_structured_output_and_records_its_identity() {
    let server = serve(vec![Reply::ok(completion(
        json!({"role": "assistant", "content": draft("ev-fact")}),
        Some("stop"),
        json!({"prompt_tokens": 1200, "completion_tokens": 400}),
    ))]);
    let mut client = MetaModelClient::new(config(&server.base_url)).unwrap();
    let (doc, inv) = client.assess(&task(), &context()).unwrap();
    assert_eq!(server.hits(), 1, "one call");
    let body = server.last_body();
    assert_eq!(body["model"], "muse-wonder-1.0");
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(
        body["response_format"]["json_schema"]["name"],
        BASELINE_SCHEMA_NAME
    );
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert_eq!(body["max_completion_tokens"], json!(16000));
    assert_eq!(server.last_system(), baseline_system_prompt());
    // The prompt shows the task and the records with their integrity, in
    // the consensus prompts' untrusted blocks.
    let prompt = server.last_prompt();
    assert!(prompt.contains("<UNTRUSTED_TASK>"), "{prompt}");
    assert!(prompt.contains(r#""id": "ev-fact""#), "{prompt}");
    assert!(prompt.contains(r#""integrity": "Valid""#), "{prompt}");
    // The invocation: request id, latency, tokens, schema and prompt digests.
    assert_eq!(inv.provider, "meta");
    assert_eq!(inv.request_id.as_deref(), Some("chatcmpl-test-2"));
    assert!(inv.latency_ms.is_some());
    assert_eq!(
        (inv.input_tokens, inv.output_tokens),
        (Some(1200), Some(400))
    );
    assert_eq!(inv.schema_name.as_deref(), Some(BASELINE_SCHEMA_NAME));
    // The schema's bytes exactly as sent (a parsed Value would re-sort the
    // keys: claim_number's description comes before its anyOf, as the
    // Critic's does).
    let request = server.last_request();
    let sent = raw_schema(&request, BASELINE_SCHEMA_NAME);
    assert_eq!(
        serde_json::from_str::<Value>(sent).unwrap(),
        body["response_format"]["json_schema"]["schema"]
    );
    assert_eq!(
        inv.schema_sha256,
        Some(hex(&Sha256::digest(sent.as_bytes())))
    );
    assert_eq!(inv.prompt_sha256, Some(baseline_prompt_sha256()));
    // The document: adapter ids, the agent's verdict, unknown objection
    // references dropped.
    assert_eq!(doc.run_id.as_str(), "baseline-1");
    assert_eq!(doc.claims.len(), 1);
    assert_eq!(doc.assessments[0].claim_id, doc.claims[0].id);
    assert_eq!(doc.assessments[0].relevant_evidence[0].as_str(), "ev-fact");
    assert_eq!(doc.verdict, AgentVerdict::Supported);
    assert_eq!(doc.objections.len(), 1);
    assert_eq!(doc.objections[0].claim_id.as_ref(), Some(&doc.claims[0].id));
    assert!(doc.objections[0].evidence_refs.is_empty());
    assert_eq!(doc.objections[0].status, ObjectionStatus::Open);
}

/// The baseline is not one of the roles: it keeps `CONSENSUS_MODEL` with
/// the Proposer's and the Critic's overrides set, sent and recorded.
#[test]
fn the_baseline_keeps_consensus_model_whatever_the_role_overrides() {
    let server = serve(vec![Reply::ok(completion_without_model_id(&draft(
        "ev-fact",
    )))]);
    let mut cfg = config(&server.base_url);
    cfg.proposer_model = "muse-spark-1.3".into();
    cfg.critic_model = "muse-spark-1.3".into();
    assert_eq!(cfg.model, "muse-wonder-1.0");
    let mut client = MetaModelClient::new(cfg).unwrap();
    let (_, inv) = client.assess(&task(), &context()).unwrap();
    assert_eq!(server.last_body()["model"], "muse-wonder-1.0");
    assert_eq!(inv.model, "muse-wonder-1.0");
}

/// A chat completion answering `content` that echoes no model id (the
/// invocation then records the model the call was sent to).
fn completion_without_model_id(content: &str) -> String {
    json!({
        "id": "chatcmpl-test-6",
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 50 }
    })
    .to_string()
}

#[test]
fn a_draft_citing_an_unknown_record_is_malformed_and_keeps_the_billed_call() {
    let server = serve(vec![Reply::ok(envelope(&draft("ev-elsewhere")))]);
    let mut client = MetaModelClient::new(config(&server.base_url)).unwrap();
    match client.assess(&task(), &context()) {
        Err(e @ ModelError::MalformedOutput { .. }) => {
            assert!(e.to_string().contains("not a retrieved record"), "{e}");
            let inv = e.invocation().expect("the answered call is kept");
            assert_eq!(inv.request_id.as_deref(), Some("chatcmpl-test-1"));
        }
        other => panic!("expected malformed output, got {other:?}"),
    }
}

/// Every member key and every `type` of a schema document, recursively.
fn keys_and_types(v: &Value, keys: &mut Vec<String>, types: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                keys.push(key.clone());
                if key == "type" {
                    match value {
                        Value::String(t) => types.push(t.clone()),
                        Value::Array(ts) => {
                            types.extend(ts.iter().filter_map(|t| t.as_str()).map(str::to_string))
                        }
                        _ => {}
                    }
                }
                keys_and_types(value, keys, types);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| keys_and_types(i, keys, types)),
        _ => {}
    }
}

#[test]
fn the_request_schema_has_no_integer_format_or_minimum_and_claim_numbers_are_strings() {
    // The draft names claim "1"; an unknown position names no claim.
    let mut unknown: Value = serde_json::from_str(&draft("ev-fact")).unwrap();
    unknown["objections"][0]["claim_number"] = json!("7");
    for (content, names_the_claim) in [(draft("ev-fact"), true), (unknown.to_string(), false)] {
        let server = serve(vec![Reply::ok(envelope(&content))]);
        let mut client = MetaModelClient::new(config(&server.base_url)).unwrap();
        let (doc, _) = client.assess(&task(), &context()).unwrap();
        assert_eq!(
            doc.objections[0].claim_id.as_ref() == Some(&doc.claims[0].id),
            names_the_claim
        );
        assert_eq!(doc.objections[0].claim_id.is_some(), names_the_claim);
        let body = server.last_body();
        let schema = &body["response_format"]["json_schema"]["schema"];
        let (mut keys, mut types) = (vec![], vec![]);
        keys_and_types(schema, &mut keys, &mut types);
        for banned in ["format", "minimum", "maximum"] {
            assert!(!keys.iter().any(|k| k == banned), "{banned} sent: {schema}");
        }
        assert!(
            !types.iter().any(|t| t == "integer"),
            "integer sent: {schema}"
        );
        let claim_number =
            &schema["definitions"]["DraftBaselineObjection"]["properties"]["claim_number"];
        assert_eq!(claim_number["anyOf"][1], json!({"type": "null"}));
        assert_eq!(claim_number["anyOf"][0]["type"], "string");
        assert_eq!(claim_number["anyOf"][0]["enum"][0], "1");
        assert_eq!(claim_number["anyOf"][0]["enum"][11], "12");
    }
}

#[test]
fn a_refused_request_records_what_the_api_said_bounded_without_the_request_or_the_key() {
    let said = format!(
        "Invalid schema for response_format 'DraftBaselineAssessment': In context=('properties', 'claim_number'), 'format' is not permitted. Near: {DESCRIPTION} {TEST_KEY} {}",
        "y".repeat(2000)
    );
    let body = json!({"error": {
        "message": said,
        "type": "invalid_request_error",
        "param": "response_format",
        "code": "invalid_json_schema"
    }})
    .to_string();
    let server = serve(vec![Reply::json(400, body.clone())]);
    let mut client = MetaModelClient::new(config(&server.base_url)).unwrap();
    let err = client.assess(&task(), &context()).unwrap_err();
    assert_eq!(server.hits(), 1, "a 400 is never retried");
    assert_eq!(err.code(), Some("meta_bad_request"));
    let text = err.to_string();
    assert!(
        text.starts_with("meta_bad_request: the Meta API rejected the request (HTTP 400); the API said: Invalid schema for response_format 'DraftBaselineAssessment': In context=('properties', 'claim_number'), 'format' is not permitted. Near: [...]"),
        "{text}"
    );
    assert!(text.contains("…[truncated]"), "{text}");
    assert!(
        text.chars().count() < 600,
        "bounded: {} chars",
        text.chars().count()
    );
    // The task text is request content: elided. The key: never.
    assert!(!text.contains("intermittently"), "{text}");
    assert!(
        !text.contains(TEST_KEY) && !text.contains("test-key"),
        "{text}"
    );

    // Through the subcommand: the run log's failure reason says it too.
    let server = serve(vec![Reply::json(400, body)]);
    let out_dir = temp_dir("baseline-cli-400");
    let env = env_for(&server.base_url, &[]);
    let (code, stdout, stderr) = cli(
        &[
            "run",
            "--title",
            TITLE,
            "--description",
            DESCRIPTION,
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--memory",
            "fake",
        ],
        &env,
    );
    assert_eq!(code, 0, "{stderr}");
    let summary: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(summary["failure"]["code"], "meta_bad_request");
    let reason = summary["failure"]["reason"].as_str().unwrap();
    assert!(reason.contains("'format' is not permitted"), "{reason}");
    assert!(reason.contains("(HTTP 400)"), "{reason}");
    let persisted = std::fs::read_to_string(only_log(&out_dir)).unwrap();
    assert!(persisted.contains("'format' is not permitted"));
    for leak in [TEST_KEY, "test-key", "intermittently"] {
        let in_reasons = persisted
            .lines()
            .filter(|l| l.contains("run_failed") || l.contains("RunFailed"))
            .any(|l| l.contains(leak));
        assert!(!in_reasons, "{leak} in a failure record");
    }
    assert!(!persisted.contains(TEST_KEY), "the key is never persisted");
}

fn env_for(base_url: &str, extra: &[(&'static str, &str)]) -> impl Fn(&str) -> Option<String> {
    let mut vars: Vec<(String, String)> = vec![
        ("META_API_KEY".into(), TEST_KEY.into()),
        ("CONSENSUS_META_BASE_URL".into(), base_url.into()),
        ("CONSENSUS_MODEL".into(), "muse-wonder-1.0".into()),
    ];
    // Extras first: they override the defaults above.
    for (k, v) in extra.iter().rev() {
        vars.insert(0, (k.to_string(), v.to_string()));
    }
    move |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// `baseline <args>`: (exit code, stdout, stderr).
fn cli(args: &[&str], env: &dyn Fn(&str) -> Option<String>) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args, env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn only_log(dir: &Path) -> PathBuf {
    let logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(logs.len(), 1, "{logs:?}");
    logs[0].clone()
}

#[test]
fn baseline_run_writes_a_verifiable_log_with_one_meta_call() {
    let server = serve(vec![Reply::ok(envelope(&draft("ev-fact")))]);
    let out_dir = temp_dir("baseline-cli-run");
    let env = env_for(&server.base_url, &[]);
    let (code, stdout, stderr) = cli(
        &[
            "run",
            "--title",
            TITLE,
            "--description",
            DESCRIPTION,
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--memory",
            "fake",
        ],
        &env,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(server.hits(), 1, "exactly one model call");
    let summary: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(summary["run_kind"], RUN_KIND);
    assert_eq!(summary["outcome"], "supported");
    assert_eq!(summary["agent_verdict"], "supported");
    assert_eq!(summary["recall"]["source"]["kind"], "task_text");
    assert_eq!(summary["evidence"]["retrieved"], json!(["ev-fact"]));
    assert_eq!(summary["model_invocations"][0]["provider"], "meta");
    assert_eq!(
        summary["model_invocations"][0]["request_id"],
        "chatcmpl-test-1"
    );
    assert_eq!(summary["task"]["mode"]["evidence"], "simulated");
    assert_eq!(summary["task"]["mode"]["origin"], "cli");
    // ... and at the top of the summary, never only deep in the JSON.
    assert_eq!(summary["evidence_mode"], "simulated");
    assert!(summary["evidence_warning"]
        .as_str()
        .unwrap()
        .starts_with("SIMULATED"));
    assert!(summary["note"].as_str().unwrap().starts_with("SIMULATED"));
    // A SIMULATED proof says so; it is never presented as a real check.
    assert_eq!(summary["verifications"][0]["verifier"], "fake-verifier");
    // The open objection would have kept the consensus policy from
    // `supported`: the comparison aid says so.
    assert_eq!(summary["policy_reading"]["kind"], "contested");
    let log = only_log(&out_dir);
    assert_eq!(summary["log"], log.display().to_string());
    let run_id = summary["run_id"].as_str().unwrap();
    assert!(run_id.starts_with("baseline-"), "{run_id}");
    assert_eq!(
        log.file_name().unwrap().to_str().unwrap(),
        format!("{run_id}.jsonl")
    );
    let bytes = std::fs::read(&log).unwrap();
    assert_eq!(summary["log_sha256"], hex(&Sha256::digest(&bytes)));
    let run = read_baseline(&bytes).unwrap();
    assert_eq!(run.invocations.len(), 1);
    // The consensus log refuses it: it can never be served as a consensus run.
    assert!(EventLog::open_read_only(&log).is_err());
    let mut report = Vec::new();
    let code = consensus_api::verify_cli::run(&[log.display().to_string()], &mut report);
    let report = String::from_utf8(report).unwrap();
    assert_ne!(code, 0, "{report}");
    assert!(report.contains("consensus-api baseline verify"), "{report}");
    // `baseline verify` reads the same summary back from the log.
    let (code, verified, stderr) = cli(&["verify", log.to_str().unwrap()], &env);
    assert_eq!(code, 0, "{stderr}");
    let mut verified: Value = serde_json::from_str(&verified).unwrap();
    let mut expected = summary.clone();
    expected.as_object_mut().unwrap().remove("log");
    verified.as_object_mut().unwrap().remove("log");
    assert_eq!(verified, expected);
    // A tampered log does not verify.
    let tampered = out_dir.join("tampered.jsonl");
    let text = String::from_utf8(bytes).unwrap();
    std::fs::write(&tampered, text.replacen("Keep retries", "Drop retries", 1)).unwrap();
    let (code, _, stderr) = cli(&["verify", tampered.to_str().unwrap()], &env);
    assert_eq!(code, 1);
    assert!(stderr.contains("hash mismatch"), "{stderr}");
}

#[test]
fn with_a_consensus_log_the_baseline_recalls_with_that_runs_claims() {
    // The consensus reference run, recorded with the scripted doubles.
    let dir = temp_dir("baseline-cli-consensus");
    let consensus_path = dir.join("consensus.jsonl");
    let s = retry_constraint_scenario();
    let (title, description) = (s.input.title.clone(), s.input.description.clone());
    let mut log = EventLog::open(&consensus_path).unwrap();
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new("2026-11-01T00:00:00Z"),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let consensus = orch.run(&mut log, s.input).unwrap();
    drop(log);
    let consensus_bytes = std::fs::read(&consensus_path).unwrap();

    // Its claims recall both records, so the draft may cite the constraint.
    let mut doc: Value = serde_json::from_str(&draft("ev-fact")).unwrap();
    doc["claims"][0]["contradicting_evidence"] = json!(["ev-constraint"]);
    let server = serve(vec![Reply::ok(envelope(&doc.to_string()))]);
    let env = env_for(&server.base_url, &[]);
    let out_dir = dir.join("out");
    let base = [
        "run",
        "--title",
        title.as_str(),
        "--description",
        description.as_str(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--memory",
        "fake",
        "--queries-from",
        consensus_path.to_str().unwrap(),
    ];
    let (code, stdout, stderr) = cli(&base, &env);
    assert_eq!(code, 0, "{stderr}");
    let summary: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        summary["recall"]["source"],
        json!({
            "kind": "consensus_log",
            "log_sha256": hex(&Sha256::digest(&consensus_bytes)),
            "run_id": consensus.run_id.as_str(),
        })
    );
    // Four distinct claim statements across the proposal and its revision.
    assert_eq!(summary["recall"]["queries"], 4);
    assert_eq!(
        summary["evidence"]["retrieved"],
        json!(["ev-constraint", "ev-fact"])
    );
    assert_eq!(summary["outcome"], "supported");
    let run = read_baseline(&std::fs::read(only_log(&out_dir)).unwrap()).unwrap();
    assert!(matches!(run.plan.source, QuerySource::ConsensusLog { .. }));
    assert_eq!(
        run.verifications[0].factual_status,
        FactualStatus::Contradicted
    );
    // The consensus log was only read.
    assert_eq!(std::fs::read(&consensus_path).unwrap(), consensus_bytes);

    // Another task's consensus run is refused before any call.
    let other = temp_dir("baseline-cli-consensus-other");
    let mut mismatched = base;
    mismatched[2] = "Another title";
    mismatched[6] = other.to_str().unwrap();
    let (code, _, stderr) = cli(&mismatched, &env);
    assert_eq!(code, 1);
    assert!(stderr.contains("another task"), "{stderr}");
    assert_eq!(server.hits(), 1, "no call for the refused run");
    assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);

    // A Meta configuration with another model than the consensus run's is
    // refused before any call: the comparison holds the model equal.
    let other_model = env_for(&server.base_url, &[("CONSENSUS_MODEL", "muse-other-2.0")]);
    let other = temp_dir("baseline-cli-consensus-model");
    let mut wrong_model = base;
    wrong_model[6] = other.to_str().unwrap();
    let (code, _, stderr) = cli(&wrong_model, &other_model);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("sends model \"muse-other-2.0\""),
        "{stderr}"
    );
    assert!(stderr.contains("\"muse-wonder-1.0\""), "{stderr}");
    assert_eq!(server.hits(), 1, "no call for the refused run");
    assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
}

#[test]
fn the_baseline_is_operator_only_and_needs_a_key() {
    let server = serve(vec![Reply::ok(envelope(&draft("ev-fact")))]);
    let out_dir = temp_dir("baseline-cli-refused");
    let run = [
        "run",
        "--title",
        TITLE,
        "--description",
        DESCRIPTION,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--memory",
        "fake",
    ];
    let public = env_for(&server.base_url, &[("CONSENSUS_PUBLIC_MODE", "true")]);
    let (code, _, stderr) = cli(&run, &public);
    assert_eq!(code, 1);
    assert!(stderr.contains("operator-only"), "{stderr}");
    let no_key = |name: &str| (name == "CONSENSUS_META_BASE_URL").then(|| server.base_url.clone());
    let (code, _, stderr) = cli(&run, &no_key);
    assert_eq!(code, 1);
    assert!(stderr.contains("META_API_KEY"), "{stderr}");
    assert_eq!(server.hits(), 0, "nothing was sent");
    assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 0);
    // A usage error is exit 2.
    let (code, _, stderr) = cli(&["run", "--title", TITLE], &public);
    assert_eq!(code, 2);
    assert!(stderr.contains("usage"), "{stderr}");
    let (code, _, _) = cli(&["unknown"], &public);
    assert_eq!(code, 2);
}
