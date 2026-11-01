// SPDX-License-Identifier: Apache-2.0
//! Meta adapter requests, retries, errors and accounting against the
//! loopback fake (audit findings MA-3, MA-4, MA-5, MA-8, MA-9, TC-7,
//! SEC-9): the documented request parameters, the retry policy, one time
//! budget per call, fixed error messages per HTTP class, truncation and
//! refusal, and every answered call kept on the record.

mod support;

use consensus_api::config::{ApiKey, MetaConfig};
use consensus_api::meta::{MetaModelClient, Role};
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::fakes::{FakeMemoryClient, FakeProofVerifier};
use consensus_runtime::orchestrator::{build_receipt, Orchestrator, TaskInput};
use consensus_runtime::ports::{ModelClient, ModelError};
use consensus_runtime::{EventLog, RunAggregate, RunLimits};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use support::*;

fn task() -> Task {
    Task {
        schema_version: SCHEMA_VERSION.into(),
        id: TaskId::new("task-1"),
        run_id: RunId::new("run-1"),
        project_id: ProjectId::new("demo-repo"),
        title: "Fix payment timeouts".into(),
        description: "Timeouts under load".into(),
        created_at: "2026-10-26T00:00:00Z".into(),
        mode: None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn usage(prompt: u64, completion: u64) -> serde_json::Value {
    json!({"prompt_tokens": prompt, "completion_tokens": completion})
}

/// One run through the real adapter and orchestrator against `server`
/// (scripted memory, every proof Valid). Returns the run, its log and the
/// log's bytes as text.
fn run_through_adapter(name: &str, cfg: MetaConfig) -> (RunAggregate, EventLog, String) {
    let path = temp_dir(&format!("meta-requests-{name}")).join("events.jsonl");
    let mut orch = Orchestrator {
        model: MetaModelClient::new(cfg).unwrap(),
        memory: FakeMemoryClient::new(vec![consensus_runtime::scenarios::evidence(
            "ev-1",
            "demo-repo",
            "constraint",
            "Constraint: outbound retries must be capped at 3 attempts.",
        )]),
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        clock: FixedClock::new("2026-10-27T09:00:00Z"),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let mut log = EventLog::open(&path).unwrap();
    let input = TaskInput {
        project_id: ProjectId::new("demo-repo"),
        title: "Fix payment retries".into(),
        description: "Retries burst under load.".into(),
        submitted_by: "test".into(),
        mode: None,
    };
    let agg = orch.run(&mut log, input).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    (agg, log, text)
}

/// (stage, code, reason, invocation) of the log's last record, which must
/// be RunFailed.
fn run_failure(log: &EventLog) -> (String, Option<String>, String, Option<ModelInvocation>) {
    match &log.all_events().last().expect("a record").payload {
        EventPayload::RunFailed {
            stage,
            code,
            reason,
            invocation,
        } => (
            stage.clone(),
            code.clone(),
            reason.clone(),
            invocation.clone(),
        ),
        other => panic!("expected RunFailed last, got {other:?}"),
    }
}

#[test]
fn requests_carry_the_documented_parameters_and_invocations_record_them() {
    let answer = completion(
        json!({"role": "assistant", "content": DRAFT_PROPOSAL}),
        Some("stop"),
        json!({"prompt_tokens": 120, "completion_tokens": 900,
               "completion_tokens_details": {"reasoning_tokens": 700}}),
    );
    let server = serve(vec![Reply::ok(answer)]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let (_, inv) = c.propose(&task()).unwrap();
    let body = server.last_body();
    // The output bound is always sent; nothing optional is sent unasked.
    assert_eq!(body["max_completion_tokens"], json!(16000));
    assert!(body.get("reasoning_effort").is_none(), "{body}");
    assert!(body.get("safety_identifier").is_none(), "{body}");
    assert!(
        body.get("max_tokens").is_none(),
        "deprecated alias never sent"
    );
    assert_eq!(inv.max_completion_tokens, Some(16000));
    assert_eq!(inv.reasoning_tokens, Some(700));
    assert_eq!(inv.finish_reason.as_deref(), Some("stop"));
    assert_eq!(inv.reasoning_effort, None);
    assert_eq!(inv.safety_identifier_sent, Some(false));
    assert_eq!(inv.attempts, Some(1));
    assert_eq!(inv.schema_name.as_deref(), Some("DraftProposal"));
    // schema_sha256 is the digest of the strict schema exactly as sent.
    let sent = serde_json::to_string(&body["response_format"]["json_schema"]["schema"]).unwrap();
    assert_eq!(
        inv.schema_sha256,
        Some(hex(&Sha256::digest(sent.as_bytes())))
    );
    assert_eq!(inv.prompt_sha256, Some(Role::Propose.prompt_sha256()));
    // The key travels in the Authorization header and nowhere else.
    let raw = server.last_request();
    let lower = raw.to_ascii_lowercase();
    assert!(
        lower.contains(&format!(
            "authorization: bearer {}",
            TEST_KEY.to_ascii_lowercase()
        )),
        "{raw}"
    );
    assert_eq!(raw.matches(TEST_KEY).count(), 1, "only in the header");
    assert!(!serde_json::to_string(&inv).unwrap().contains(TEST_KEY));

    // Configured: a reasoning effort, a per-run safety identifier and a
    // smaller output bound are sent and recorded.
    let server = serve(vec![Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let mut cfg = config(&server.base_url);
    cfg.reasoning_effort = Some("high".into());
    cfg.max_completion_tokens = 32;
    cfg.safety_identifier = Some("5f1d7c0e9a".into());
    let mut c = MetaModelClient::new(cfg).unwrap();
    let (_, inv) = c.propose(&task()).unwrap();
    let body = server.last_body();
    assert_eq!(body["max_completion_tokens"], json!(32));
    assert_eq!(body["reasoning_effort"], json!("high"));
    assert_eq!(body["safety_identifier"], json!("5f1d7c0e9a"));
    assert_eq!(inv.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(inv.safety_identifier_sent, Some(true));
    assert_eq!(inv.max_completion_tokens, Some(32));
    // The caller changes it per run; an unusable one is refused.
    assert!(c.set_safety_identifier(Some("has space".into())).is_err());
    c.set_safety_identifier(None).unwrap();
    let (_, inv) = c.propose(&task()).unwrap();
    assert!(server.last_body().get("safety_identifier").is_none());
    assert_eq!(inv.safety_identifier_sent, Some(false));
}

#[test]
fn prompt_hashes_identify_the_template_never_the_input() {
    let server = serve(vec![Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let (_, first) = c.propose(&task()).unwrap();
    let mut other = task();
    other.title = "Something else entirely".into();
    other.description = "A different description with </UNTRUSTED_TASK> in it".into();
    let (_, second) = c.propose(&other).unwrap();
    assert_eq!(first.prompt_sha256, second.prompt_sha256);
    let hashes: Vec<String> = Role::ALL.iter().map(|r| r.prompt_sha256()).collect();
    assert_eq!(first.prompt_sha256.as_deref(), Some(hashes[0].as_str()));
    for (n, h) in hashes.iter().enumerate() {
        assert!(!hashes[..n].contains(h), "each role has its own template");
    }
}

#[test]
fn truncated_output_has_its_own_code_and_keeps_the_billed_call() {
    let truncated = completion(
        json!({"role": "assistant", "content": r#"{"summary":"Raise retr"#}),
        Some("length"),
        usage(10, 16000),
    );
    let server = serve(vec![Reply::ok(truncated)]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    match c.propose(&task()) {
        Err(e @ ModelError::MalformedOutput { .. }) => {
            assert_eq!(e.code(), Some("model_output_truncated"));
            assert!(
                e.to_string().contains("max_completion_tokens (16000)"),
                "{e}"
            );
            let inv = e.invocation().expect("the answered call is kept");
            assert_eq!(inv.finish_reason.as_deref(), Some("length"));
            assert_eq!(inv.output_tokens, Some(16000));
        }
        other => panic!("expected truncated MalformedOutput, got {other:?}"),
    }

    // Through the orchestrator: the run fails at propose with that code,
    // and the call is in the record and the receipt.
    let (agg, log, _) = run_through_adapter("truncated", config(&server.base_url));
    let (stage, code, _, invocation) = run_failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("propose", Some("model_output_truncated"))
    );
    assert_eq!(
        invocation.and_then(|i| i.finish_reason),
        Some("length".into())
    );
    assert_eq!(agg.invocations.len(), 1);
    let receipt = build_receipt(&log, &agg.run_id, "2026-10-27T09:00:00Z".into()).unwrap();
    assert_eq!(receipt.model_invocations, agg.invocations);
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

#[test]
fn a_refusal_has_its_own_code_keeps_the_call_and_never_its_text() {
    let refusal = completion(
        json!({"role": "assistant", "content": null,
               "refusal": "I cannot help with the payroll records you included."}),
        Some("stop"),
        usage(80, 12),
    );
    let server = serve(vec![Reply::ok(refusal)]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    match c.propose(&task()) {
        Err(e @ ModelError::Refused { .. }) => {
            assert_eq!(e.code(), Some("model_refusal"));
            assert!(!e.to_string().contains("payroll"), "{e}");
            let inv = e.invocation().expect("the answered call is kept");
            assert_eq!(inv.request_id.as_deref(), Some("chatcmpl-test-2"));
            assert_eq!(inv.output_tokens, Some(12));
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    let (agg, log, text) = run_through_adapter("refusal", config(&server.base_url));
    let (stage, code, reason, invocation) = run_failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("propose", Some("model_refusal"))
    );
    assert!(reason.starts_with("model_refusal:"), "{reason}");
    assert!(invocation.is_some());
    assert!(
        !text.contains("payroll"),
        "the refusal text is never persisted"
    );
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

#[test]
fn retry_after_in_seconds_is_honored_on_429() {
    let server = serve(vec![
        Reply::json(429, "{}").header("retry-after", "1"),
        Reply::ok(envelope(DRAFT_PROPOSAL)),
    ]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let started = Instant::now();
    let (_, inv) = c.propose(&task()).unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(inv.attempts, Some(2));
    assert_eq!(server.hits(), 2);
}

#[test]
fn retry_after_as_an_http_date_is_honored_on_503() {
    let at = chrono::Utc::now() + chrono::Duration::seconds(2);
    let date = at.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    let server = serve(vec![
        Reply::json(503, "{}").header("retry-after", &date),
        Reply::ok(envelope(DRAFT_PROPOSAL)),
    ]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let started = Instant::now();
    let (_, inv) = c.propose(&task()).unwrap();
    // The date has whole seconds: the wait is at least one of them.
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(inv.attempts, Some(2));
}

#[test]
fn a_retry_after_beyond_the_budget_fails_at_once() {
    let server = serve(vec![Reply::json(429, "{}").header("retry-after", "120")]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let started = Instant::now();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_rate_limited"));
    assert!(err.to_string().contains("retry after 120 s"), "{err}");
    assert_eq!(server.hits(), 1, "never retried sooner than asked");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_retry_after_that_leaves_no_time_for_the_retry_fails_at_once() {
    // Retry-After fits under 30 s but would consume the whole call budget:
    // the rate limit is reported, not a timeout after a wait that could
    // not pay off.
    let server = serve(vec![
        Reply::json(429, "{}").header("retry-after", "2"),
        Reply::ok(envelope(DRAFT_PROPOSAL)),
    ]);
    let mut cfg = config(&server.base_url);
    cfg.timeout = Duration::from_secs(2);
    let mut c = MetaModelClient::new(cfg).unwrap();
    let started = Instant::now();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_rate_limited"), "{err}");
    assert!(err.to_string().contains("retry after 2 s"), "{err}");
    assert_eq!(server.hits(), 1, "never retried into a timeout");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert!(c.take_failed_invocation().is_none(), "no 2xx, no call kept");
}

#[test]
fn only_429_and_503_are_retried_to_the_budget_and_500_and_502_once() {
    for (status, code) in [
        (400, "meta_bad_request"),
        (401, "meta_unauthorized"),
        (402, "meta_billing"),
        (403, "meta_forbidden"),
        (404, "meta_bad_request"),
        (413, "meta_bad_request"),
        (504, "meta_unavailable"),
    ] {
        let server = serve(vec![Reply::json(status, "{}")]);
        let mut cfg = config(&server.base_url);
        cfg.max_attempts = 3;
        let mut c = MetaModelClient::new(cfg).unwrap();
        let err = c.propose(&task()).unwrap_err();
        assert_eq!(err.code(), Some(code), "{status}");
        assert!(err.to_string().contains(&status.to_string()), "{err}");
        assert!(err.invocation().is_none(), "no 2xx, no invocation");
        assert_eq!(server.hits(), 1, "HTTP {status} is never retried");
    }
    for status in [429, 503] {
        let server = serve(vec![Reply::json(status, "{}")]);
        let mut cfg = config(&server.base_url);
        cfg.max_attempts = 3;
        let mut c = MetaModelClient::new(cfg).unwrap();
        let err = c.propose(&task()).unwrap_err();
        assert!(err.to_string().contains("after 3 attempt(s)"), "{err}");
        assert_eq!(
            server.hits(),
            3,
            "HTTP {status} is retried up to the attempt budget"
        );
    }
    // Possibly billed: one retry per call, whatever the attempt budget.
    for status in [500, 502] {
        let server = serve(vec![Reply::json(status, "{}")]);
        let mut cfg = config(&server.base_url);
        cfg.max_attempts = 3;
        let mut c = MetaModelClient::new(cfg).unwrap();
        let err = c.propose(&task()).unwrap_err();
        assert_eq!(err.code(), Some("meta_unavailable"), "{status}");
        assert!(err.to_string().contains(&status.to_string()), "{err}");
        assert!(err.to_string().contains("after 2 attempt(s)"), "{err}");
        assert!(err.invocation().is_none(), "no 2xx, no invocation");
        assert_eq!(server.hits(), 2, "HTTP {status} is retried exactly once");
    }
}

#[test]
fn a_refused_connection_is_retried_and_a_dropped_one_is_not_with_the_transport_retry_off() {
    // Nothing listens on this port: the connection never opens, so no
    // request was sent and retrying cannot bill twice.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut cfg = config(&format!("http://127.0.0.1:{port}/v1"));
    cfg.timeout = Duration::from_secs(30);
    let mut c = MetaModelClient::new(cfg).unwrap();
    let err = c.propose(&task()).unwrap_err();
    let text = err.to_string();
    assert_eq!(err.code(), Some("meta_transport"), "{text}");
    assert!(text.contains("after 2 attempt(s)"), "{text}");
    // reqwest's text without its URL (SEC-9).
    assert!(
        !text.contains("127.0.0.1") && !text.contains("http"),
        "{text}"
    );

    // The server read the request and hung up: it may have been billed,
    // so with the one post-send retry switched off it is never sent again.
    let server = serve(vec![Reply::hang_up()]);
    let mut cfg = config(&server.base_url);
    cfg.max_attempts = 3;
    cfg.retry_transport_once = false;
    let mut c = MetaModelClient::new(cfg).unwrap();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_transport"), "{err}");
    assert!(err.to_string().contains("after it was sent"), "{err}");
    assert_eq!(server.hits(), 1);
}

#[test]
fn a_connection_dropped_after_the_request_is_retried_once_by_default() {
    // Live R06 (2026-10-27): one transient "error sending request" after
    // the request was sent failed the whole run. The connection is retried
    // once, within the attempt budget, and the invocation records it.
    let server = serve(vec![Reply::hang_up(), Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let (_, inv) = c.propose(&task()).unwrap();
    assert_eq!(inv.attempts, Some(2));
    assert_eq!(server.hits(), 2);

    // Through the orchestrator: one proposal, one invocation of 2 attempts.
    let server = serve(vec![Reply::hang_up(), Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let (agg, log, _) = run_through_adapter("transport-retry", config(&server.base_url));
    let proposals = log
        .all_events()
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::ProposalSubmitted { .. }))
        .count();
    assert_eq!(proposals, 1);
    assert_eq!(agg.invocations[0].attempts, Some(2));
    assert_eq!(
        agg.invocations[0].schema_name.as_deref(),
        Some("DraftProposal")
    );
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);

    // Two drops: the retry is one, never more, whatever the attempt budget.
    let server = serve(vec![
        Reply::hang_up(),
        Reply::hang_up(),
        Reply::ok(envelope(DRAFT_PROPOSAL)),
    ]);
    let mut cfg = config(&server.base_url);
    cfg.max_attempts = 5;
    let mut c = MetaModelClient::new(cfg).unwrap();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_transport"), "{err}");
    assert!(err.to_string().contains("after it was sent"), "{err}");
    assert!(err.to_string().contains("2 attempt(s)"), "{err}");
    assert_eq!(server.hits(), 2);
    assert!(err.invocation().is_none(), "never answered");
    assert!(c.take_failed_invocation().is_none());

    // It shares the attempt budget with the status retries: a 429, then a
    // drop, then the answer fit in three attempts ...
    let server = serve(vec![
        Reply::json(429, "{}"),
        Reply::hang_up(),
        Reply::ok(envelope(DRAFT_PROPOSAL)),
    ]);
    let mut cfg = config(&server.base_url);
    cfg.max_attempts = 3;
    let mut c = MetaModelClient::new(cfg).unwrap();
    let (_, inv) = c.propose(&task()).unwrap();
    assert_eq!(inv.attempts, Some(3));
    assert_eq!(server.hits(), 3);
    // ... and a single attempt allows no retry at all.
    let server = serve(vec![Reply::hang_up(), Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let mut cfg = config(&server.base_url);
    cfg.max_attempts = 1;
    let mut c = MetaModelClient::new(cfg).unwrap();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_transport"), "{err}");
    assert_eq!(server.hits(), 1);

    // Switched off (CONSENSUS_META_RETRY_TRANSPORT_ONCE=false): the first
    // drop fails the call.
    let server = serve(vec![Reply::hang_up(), Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let mut cfg = config(&server.base_url);
    cfg.retry_transport_once = false;
    let mut c = MetaModelClient::new(cfg).unwrap();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_transport"), "{err}");
    assert!(err.to_string().contains("1 attempt(s)"), "{err}");
    assert_eq!(server.hits(), 1);
    // reqwest's text without its URL (SEC-9), as for every transport error.
    assert!(
        !err.to_string().contains("127.0.0.1") && !err.to_string().contains("http"),
        "{err}"
    );
}

#[test]
fn one_budget_bounds_the_whole_call_including_the_body_read() {
    // Headers at once, then the body stalls: the old client restarted
    // its timeout for the body read (MA-3); the budget is shared now.
    let server = serve(vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)).stalled(Duration::from_secs(8))
    ]);
    let mut cfg = config(&server.base_url);
    cfg.timeout = Duration::from_secs(1);
    let mut c = MetaModelClient::new(cfg).unwrap();
    let started = Instant::now();
    let err = c.propose(&task()).unwrap_err();
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(err.code(), Some("meta_timeout"), "{err}");
    // The API had answered: the call is kept for the orchestrator.
    let kept = c
        .take_failed_invocation()
        .expect("the answered call is kept");
    assert_eq!(kept.attempts, Some(1));
    assert_eq!(kept.request_id, None, "its body never arrived");
    assert!(c.take_failed_invocation().is_none(), "taken once");

    // Headers that never come: one timeout, never retried.
    let server = serve(vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)).delayed(Duration::from_secs(4))
    ]);
    let mut cfg = config(&server.base_url);
    cfg.timeout = Duration::from_secs(1);
    cfg.max_attempts = 3;
    let mut c = MetaModelClient::new(cfg).unwrap();
    let started = Instant::now();
    let err = c.propose(&task()).unwrap_err();
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(err.code(), Some("meta_timeout"), "{err}");
    assert_eq!(server.hits(), 1);
    assert!(c.take_failed_invocation().is_none(), "never answered");
}

#[test]
fn an_answered_call_whose_body_timed_out_is_recorded_and_charged() {
    let server = serve(vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)).stalled(Duration::from_secs(8))
    ]);
    let mut cfg = config(&server.base_url);
    cfg.timeout = Duration::from_secs(1);
    let (agg, log, _) = run_through_adapter("body-timeout", cfg);
    let (stage, code, reason, invocation) = run_failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("propose", Some("meta_timeout"))
    );
    assert!(reason.starts_with("meta_timeout:"), "{reason}");
    let invocation = invocation.expect("kept through take_failed_invocation");
    assert_eq!(invocation.provider, "meta");
    assert_eq!(agg.invocations, vec![invocation]);
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

#[test]
fn upstream_bodies_and_the_key_never_reach_errors_or_the_log() {
    let echo = format!(
        r#"{{"error":{{"message":"Incorrect API key provided: {TEST_KEY}","type":"invalid_request_error","code":"invalid_api_key"}}}}"#
    );
    let server = serve(vec![Reply::json(401, echo.clone())]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let err = c.propose(&task()).unwrap_err();
    let text = err.to_string();
    assert_eq!(err.code(), Some("meta_unauthorized"));
    assert!(text.starts_with("meta_unauthorized: "), "{text}");
    assert!(text.contains("401"), "{text}");
    for leak in [TEST_KEY, "Incorrect API key", "invalid_api_key"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    let (agg, log, persisted) = run_through_adapter("unauthorized", config(&server.base_url));
    let (stage, code, reason, invocation) = run_failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("propose", Some("meta_unauthorized"))
    );
    assert_eq!(reason, text, "the fixed message is the persisted reason");
    assert!(invocation.is_none());
    for leak in [TEST_KEY, "Incorrect API key", "invalid_api_key", "test-key"] {
        assert!(!persisted.contains(leak), "{leak} persisted");
    }
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);

    // A 400 that echoes the prompt back is reduced to its class too.
    let server = serve(vec![Reply::json(
        400,
        r#"{"error":{"message":"schema invalid near: Timeouts under load"}}"#,
    )]);
    let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
    let err = c.propose(&task()).unwrap_err();
    assert_eq!(err.code(), Some("meta_bad_request"));
    assert!(!err.to_string().contains("Timeouts under load"), "{err}");
}

#[test]
fn an_unusable_config_is_refused_before_any_call() {
    let server = serve(vec![Reply::ok(envelope(DRAFT_PROPOSAL))]);
    let mut cfg = config(&server.base_url);
    cfg.api_key = ApiKey::new("sk-not-a-meta-key");
    let err = match MetaModelClient::new(cfg) {
        Err(e) => e,
        Ok(_) => panic!("a malformed key must be refused"),
    };
    assert_eq!(err.code(), Some("meta_config"));
    assert!(
        err.to_string().contains("LLM_<numeric id>_<secret>"),
        "{err}"
    );
    assert!(!err.to_string().contains("sk-not-a-meta-key"), "{err}");
    assert_eq!(server.hits(), 0);

    let mut cfg = config(&server.base_url);
    cfg.base_url = "http://api.meta.ai/v1".into();
    assert!(
        MetaModelClient::new(cfg).is_err(),
        "plain http off loopback"
    );
}

#[test]
fn the_debug_dir_keeps_sanitized_files_without_the_key_and_never_in_public_mode() {
    let dir = temp_dir("meta-debug-dir");
    let hostile = json!({
        "id": "..\\..\\..\\AppData\\Roaming\\Code\\User\\settings/../../x",
        "model": "muse-wonder-1.0",
        "choices": [{"message": {"content": DRAFT_PROPOSAL}}],
        "usage": usage(10, 5)
    })
    .to_string();
    let server = serve(vec![
        Reply::ok(hostile),
        Reply::json(
            400,
            format!(r#"{{"error":{{"message":"bad key {TEST_KEY}"}}}}"#),
        ),
    ]);
    let mut cfg = config(&server.base_url);
    cfg.debug_dir = Some(dir.clone());
    let mut c = MetaModelClient::new(cfg.clone()).unwrap();
    let (_, inv) = c.propose(&task()).unwrap();
    assert_eq!(
        inv.request_id, None,
        "an id that is not a plain token is not recorded"
    );
    c.propose(&task()).unwrap_err();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 2, "{names:?}");
    for name in &names {
        let stem = name.strip_suffix(".json").expect("a .json file");
        assert!(!stem.is_empty() && stem.len() <= 64, "{name}");
        assert!(
            stem.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{name}"
        );
    }
    let error_file = names
        .iter()
        .find(|n| n.starts_with("error-400"))
        .expect("the failed call's body");
    let kept = std::fs::read_to_string(dir.join(error_file)).unwrap();
    assert!(!kept.contains(TEST_KEY), "{kept}");
    assert!(kept.contains("<redacted>"), "{kept}");

    // Public mode refuses the directory, however the config was built.
    cfg.public_mode = true;
    let err = match MetaModelClient::new(cfg) {
        Err(e) => e,
        Ok(_) => panic!("the debug dir must be refused in public mode"),
    };
    assert_eq!(err.code(), Some("meta_config"));
    assert!(err.to_string().contains("public mode"), "{err}");
}

// ---------------------------------------------------------------------
// Round 8, lane provider-budget: 5xx retries, strict timeouts, the run
// deadline and fresh connections (live runs M01, M02, K02).
// ---------------------------------------------------------------------

mod r8_provider_budget {
    use super::*;
    use consensus_api::live::budget::{
        AttemptEstimate, AttemptMeter, BudgetConfig, Draw, Lane, Ledger, LedgerError, Outcome,
        Refusal, Reservation, Settlement,
    };
    use consensus_api::live::money::{Ppm, Usd};
    use consensus_runtime::RunControl;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// One settlement as the meter saw it: the outcome, what it charged and
    /// the attempt's estimate.
    type Settled = (Outcome, Usd, Usd);

    /// A ledger meter that also records every draw and settlement.
    struct RecordingMeter {
        inner: Arc<dyn AttemptMeter>,
        draws: AtomicUsize,
        settled: Mutex<Vec<Settled>>,
    }

    impl AttemptMeter for RecordingMeter {
        fn draw(&self, est: AttemptEstimate) -> Result<Draw, Refusal> {
            self.draws.fetch_add(1, Ordering::SeqCst);
            self.inner.draw(est)
        }

        fn settle(&self, draw: Draw, outcome: Outcome) -> Result<Settlement, LedgerError> {
            let estimate = draw.estimate();
            let settlement = self.inner.settle(draw, outcome.clone())?;
            self.settled
                .lock()
                .unwrap()
                .push((outcome, settlement.charged, estimate));
            Ok(settlement)
        }
    }

    /// A metered client on a fresh ledger: the ledger, the reservation
    /// (kept alive by the caller) and the recording meter.
    struct Metered {
        ledger: Ledger,
        _reservation: Reservation,
        meter: Arc<RecordingMeter>,
    }

    impl Metered {
        fn new(name: &str) -> Self {
            let dir = temp_dir(&format!("r8-ledger-{name}"));
            let cfg = BudgetConfig {
                daily_cap: Usd::parse("10.00").unwrap(),
                lifetime_cap: Usd::parse("60.00").unwrap(),
                judge_slice: Usd::parse("15.00").unwrap(),
                judge_daily_cap: Usd::parse("15.00").unwrap(),
                max_concurrent: 100,
                hourly_admissions: 1_000,
                burst_fraction: Ppm::ONE,
                run_token_cap: 70_000,
                max_completion_tokens: 16_000,
                input_cap_tokens: 32_000,
                price_input_per_m: Usd::parse("4.00").unwrap(),
                price_output_per_m: Usd::parse("20.00").unwrap(),
                kill_switch_file: None,
            };
            let ledger = Ledger::open(&dir, cfg).unwrap();
            let reservation = ledger.reserve(Lane::Public).unwrap();
            let meter = Arc::new(RecordingMeter {
                inner: Arc::new(ledger.meter(&reservation)),
                draws: AtomicUsize::new(0),
                settled: Mutex::new(Vec::new()),
            });
            Self {
                ledger,
                _reservation: reservation,
                meter,
            }
        }

        fn client(&self, cfg: MetaConfig) -> MetaModelClient {
            MetaModelClient::new(cfg)
                .unwrap()
                .with_meter(self.meter.clone())
        }

        fn draws(&self) -> usize {
            self.meter.draws.load(Ordering::SeqCst)
        }

        fn settled(&self) -> Vec<Settled> {
            self.meter.settled.lock().unwrap().clone()
        }
    }

    fn answer() -> Reply {
        Reply::ok(envelope(DRAFT_PROPOSAL))
    }

    // ----- B1: HTTP 500 and 502 are retried once -----

    #[test]
    fn a_500_then_a_200_succeeds_on_the_second_attempt_with_the_first_settled_at_its_estimate() {
        let server = serve(vec![Reply::json(500, "{}"), answer()]);
        let metered = Metered::new("500-200");
        let mut cfg = config(&server.base_url);
        cfg.max_attempts = 3;
        let mut c = metered.client(cfg);
        let (proposal, inv) = c.propose(&task()).unwrap();
        assert_eq!(proposal.claims.len(), 1);
        assert_eq!(inv.attempts, Some(2));
        assert_eq!(server.hits(), 2);
        let settled = metered.settled();
        assert_eq!(settled.len(), 2, "{settled:?}");
        let (first, charged, estimate) = &settled[0];
        assert_eq!(*first, Outcome::Status(500));
        assert_eq!(charged, estimate, "the 500 may have been billed: whole");
        assert!(!charged.is_zero());
        assert!(
            matches!(settled[1].0, Outcome::Success(_)),
            "{:?}",
            settled[1]
        );
        assert_eq!(metered.draws(), 2);
    }

    #[test]
    fn two_500s_fail_as_unavailable_after_two_attempts_whatever_the_budget() {
        let server = serve(vec![Reply::json(500, "{}")]);
        let metered = Metered::new("500-500");
        let mut cfg = config(&server.base_url);
        cfg.max_attempts = 5;
        let mut c = metered.client(cfg);
        let err = c.propose(&task()).unwrap_err();
        assert_eq!(err.code(), Some("meta_unavailable"));
        assert!(err.to_string().contains("after 2 attempt(s)"), "{err}");
        assert_eq!(server.hits(), 2, "one retry, never a second");
        let outcomes: Vec<Outcome> = metered.settled().into_iter().map(|s| s.0).collect();
        assert_eq!(outcomes, vec![Outcome::Status(500), Outcome::Status(500)]);
    }

    #[test]
    fn a_502_then_a_200_succeeds() {
        let server = serve(vec![Reply::json(502, "{}"), answer()]);
        let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
        let (_, inv) = c.propose(&task()).unwrap();
        assert_eq!(inv.attempts, Some(2));
        assert_eq!(server.hits(), 2);
    }

    #[test]
    fn a_504_is_never_retried_since_the_generation_may_have_completed() {
        let server = serve(vec![Reply::json(504, "{}"), answer()]);
        let metered = Metered::new("504");
        let mut cfg = config(&server.base_url);
        cfg.max_attempts = 3;
        let mut c = metered.client(cfg);
        let err = c.propose(&task()).unwrap_err();
        assert_eq!(err.code(), Some("meta_unavailable"));
        assert!(err.to_string().contains("504"), "{err}");
        assert_eq!(server.hits(), 1);
        let settled = metered.settled();
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].0, Outcome::Status(504));
        assert_eq!(settled[0].1, settled[0].2, "charged at its estimate");
    }

    #[test]
    fn a_502_asking_for_more_than_the_budget_left_fails_at_once() {
        let server = serve(vec![
            Reply::json(502, "{}").header("retry-after", "40"),
            answer(),
        ]);
        let mut cfg = config(&server.base_url);
        cfg.timeout = Duration::from_secs(30);
        cfg.max_attempts = 3;
        let mut c = MetaModelClient::new(cfg).unwrap();
        let started = Instant::now();
        let err = c.propose(&task()).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5), "no wait");
        assert_eq!(err.code(), Some("meta_unavailable"));
        assert!(
            err.to_string()
                .contains("beyond the remaining retry budget"),
            "{err}"
        );
        assert!(err.to_string().contains("retry after 40 s"), "{err}");
        assert_eq!(server.hits(), 1);
    }

    #[test]
    fn a_502_with_a_retry_after_that_fits_is_retried_after_it() {
        let server = serve(vec![
            Reply::json(502, "{}").header("retry-after", "1"),
            answer(),
        ]);
        let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
        let started = Instant::now();
        let (_, inv) = c.propose(&task()).unwrap();
        assert!(started.elapsed() >= Duration::from_secs(1), "never sooner");
        assert_eq!(inv.attempts, Some(2));
    }

    #[test]
    fn a_500_with_less_than_the_retry_headroom_left_fails_without_waiting() {
        // The 500 arrives 1.3 s into a 2 s budget: 0.7 s left, below the
        // 1 s a retry needs to be answered.
        let server = serve(vec![
            Reply::json(500, "{}").delayed(Duration::from_millis(1_300)),
            answer(),
        ]);
        let mut cfg = config(&server.base_url);
        cfg.timeout = Duration::from_secs(2);
        cfg.max_attempts = 3;
        cfg.retry_base_delay = Duration::from_millis(500);
        let mut c = MetaModelClient::new(cfg).unwrap();
        let started = Instant::now();
        let err = c.propose(&task()).unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(1_900), "no wait");
        assert_eq!(err.code(), Some("meta_unavailable"));
        assert!(err.to_string().contains("after 1 attempt(s)"), "{err}");
        assert_eq!(server.hits(), 1);
    }

    #[test]
    fn a_400_and_a_401_are_one_attempt_and_the_401_still_trips_the_ledger() {
        for (status, code, trips) in [
            (400, "meta_bad_request", false),
            (401, "meta_unauthorized", true),
        ] {
            let server = serve(vec![Reply::json(status, "{}"), answer()]);
            let metered = Metered::new(&format!("status-{status}"));
            let mut cfg = config(&server.base_url);
            cfg.max_attempts = 3;
            let mut c = metered.client(cfg);
            let err = c.propose(&task()).unwrap_err();
            assert_eq!(err.code(), Some(code), "{status}");
            assert_eq!(server.hits(), 1, "{status}");
            assert_eq!(metered.draws(), 1, "{status}");
            assert_eq!(metered.ledger.is_tripped().unwrap(), trips, "{status}");
        }
    }

    // ----- B2: the per-call timeout -----

    #[test]
    fn a_slow_answer_inside_the_budget_succeeds_and_one_past_it_times_out_once() {
        let slow = || answer().delayed(Duration::from_millis(1_500));
        let server = serve(vec![slow()]);
        let mut cfg = config(&server.base_url);
        cfg.timeout = Duration::from_secs(2);
        let mut c = MetaModelClient::new(cfg).unwrap();
        let (_, inv) = c.propose(&task()).unwrap();
        assert_eq!(inv.attempts, Some(1));

        let server = serve(vec![slow()]);
        let metered = Metered::new("timeout-1s");
        let mut cfg = config(&server.base_url);
        cfg.timeout = Duration::from_secs(1);
        cfg.max_attempts = 3;
        let mut c = metered.client(cfg);
        let err = c.propose(&task()).unwrap_err();
        assert_eq!(err.code(), Some("meta_timeout"));
        assert!(err.to_string().contains("within its 1 s budget"), "{err}");
        assert!(err.to_string().contains("(attempt 1)"), "{err}");
        assert_eq!(server.hits(), 1, "a timeout is never retried");
        let outcomes: Vec<Outcome> = metered.settled().into_iter().map(|s| s.0).collect();
        assert_eq!(outcomes, vec![Outcome::Timeout]);
    }

    // ----- B3: the run deadline -----

    #[test]
    fn a_call_with_too_little_run_left_is_refused_before_any_draw_or_request() {
        let server = serve(vec![answer()]);
        let metered = Metered::new("deadline-5s");
        let mut cfg = config(&server.base_url);
        cfg.timeout = Duration::from_secs(240);
        let mut c = metered
            .client(cfg)
            .with_run_deadline(Some(Instant::now() + Duration::from_secs(5)));
        let err = c.propose(&task()).unwrap_err();
        assert_eq!(err.code(), Some("run_deadline_reached"));
        assert!(err.to_string().contains("not made"), "{err}");
        assert!(err.invocation().is_none());
        assert_eq!(server.hits(), 0, "nothing sent");
        assert_eq!(metered.draws(), 0, "nothing drawn");
        assert!(metered.settled().is_empty());
        // The same through a bound run control.
        let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
        c.bind_run_control(&RunControl::with_deadline(
            Instant::now() + Duration::from_secs(29),
        ));
        assert_eq!(
            c.propose(&task()).unwrap_err().code(),
            Some("run_deadline_reached")
        );
        assert_eq!(server.hits(), 0);
        // With enough run left, the call is made.
        let mut c = MetaModelClient::new(config(&server.base_url)).unwrap();
        c.bind_run_control(&RunControl::with_deadline(
            Instant::now() + Duration::from_secs(600),
        ));
        assert!(c.propose(&task()).is_ok());
        assert_eq!(server.hits(), 1);
    }

    #[test]
    fn a_call_is_cut_to_the_run_deadline_and_settled_like_a_timeout() {
        // 240 s of call budget, 40 s of run left, an answer after 45 s.
        let server = serve(vec![answer().delayed(Duration::from_secs(45))]);
        let metered = Metered::new("deadline-40s");
        let mut cfg = config(&server.base_url);
        cfg.timeout = Duration::from_secs(240);
        let mut c = metered.client(cfg);
        c.bind_run_control(&RunControl::with_deadline(
            Instant::now() + Duration::from_secs(40),
        ));
        let started = Instant::now();
        let err = c.propose(&task()).unwrap_err();
        let took = started.elapsed();
        assert!(
            took >= Duration::from_secs(39) && took < Duration::from_secs(44),
            "{took:?}"
        );
        assert_eq!(err.code(), Some("run_deadline_reached"));
        assert!(err.to_string().contains("the run deadline"), "{err}");
        assert_eq!(server.hits(), 1);
        let outcomes: Vec<Outcome> = metered.settled().into_iter().map(|s| s.0).collect();
        assert_eq!(outcomes, vec![Outcome::Timeout]);
    }

    #[test]
    fn a_run_whose_deadline_leaves_no_call_window_fails_with_the_deadline_code() {
        // The real adapter in the orchestrator, bound as the run manager
        // binds it: the propose call is refused before anything is sent.
        let server = serve(vec![answer()]);
        let path = temp_dir("r8-deadline-run").join("events.jsonl");
        let mut orch = Orchestrator {
            model: MetaModelClient::new(config(&server.base_url)).unwrap(),
            memory: FakeMemoryClient::new(vec![]),
            verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
            clock: FixedClock::new("2026-11-01T09:00:00Z"),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        let mut log = EventLog::open(&path).unwrap();
        let input = TaskInput {
            project_id: ProjectId::new("demo-repo"),
            title: "Fix payment retries".into(),
            description: "Retries burst under load.".into(),
            submitted_by: "test".into(),
            mode: None,
        };
        let (agg, task) = orch
            .create_task(&mut log, RunId::new("run-r8-deadline"), input)
            .unwrap();
        let control = RunControl::with_deadline(Instant::now() + Duration::from_secs(10));
        orch.model.bind_run_control(&control);
        let agg = orch.deliberate(&mut log, agg, &task, &control).unwrap();
        assert_eq!(agg.state, RunState::Failed);
        let (stage, code, reason, invocation) = run_failure(&log);
        assert_eq!(stage, "propose");
        assert_eq!(code.as_deref(), Some("run_deadline_reached"));
        assert!(reason.contains("run deadline"), "{reason}");
        assert!(invocation.is_none(), "no call was made");
        assert_eq!(server.hits(), 0);

        // A scripted model failing that way is recorded the same way.
        let mut model = consensus_runtime::fakes::FakeModelClient::new();
        model.proposals.push_back(Err(ModelError::Provider {
            code: "run_deadline_reached",
            message: "the run deadline leaves 5 s".into(),
        }));
        let mut orch = Orchestrator {
            model,
            memory: FakeMemoryClient::new(vec![]),
            verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
            clock: FixedClock::new("2026-11-01T09:00:00Z"),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        let path = temp_dir("r8-deadline-scripted").join("events.jsonl");
        let mut log = EventLog::open(&path).unwrap();
        let input = TaskInput {
            project_id: ProjectId::new("demo-repo"),
            title: "Fix payment retries".into(),
            description: "Retries burst under load.".into(),
            submitted_by: "test".into(),
            mode: None,
        };
        let agg = orch.run(&mut log, input).unwrap();
        assert_eq!(agg.state, RunState::Failed);
        let (stage, code, _, _) = run_failure(&log);
        assert_eq!(stage, "propose");
        assert_eq!(code.as_deref(), Some("run_deadline_reached"));
    }

    #[test]
    fn a_bound_call_is_abandoned_once_the_run_aborts_it_and_settled_at_its_estimate() {
        let server = serve(vec![answer().delayed(Duration::from_secs(20))]);
        let metered = Metered::new("abort");
        let mut c = metered.client(config(&server.base_url));
        let control = RunControl::new();
        c.bind_run_control(&control);
        let handle = control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            handle.abort_in_flight();
        });
        let started = Instant::now();
        let err = c.propose(&task()).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "abandoned at once"
        );
        assert_eq!(err.code(), Some("shutdown"));
        let settled = metered.settled();
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].0, Outcome::Timeout);
        assert_eq!(settled[0].1, settled[0].2, "charged at its estimate");
        let open = metered.ledger.open_reservations().unwrap();
        assert_eq!(open.len(), 1, "the reservation is the run's to release");
        assert_eq!(open[0].unsettled_attempts, 0, "nothing left unsettled");
    }

    // ----- B5: a fresh connection per call -----

    /// How a keep-alive double treats a connection after its first answer.
    #[derive(Clone, Copy)]
    enum AfterFirst {
        /// Close it after this long (a FIN the client can see).
        CloseAfter(Duration),
        /// Keep it open and never read from it again (a middlebox that
        /// dropped the flow without telling anyone).
        BlackHole,
    }

    /// An HTTP/1.1 double that answers with keep-alive: (base URL,
    /// connections accepted, requests answered).
    fn keep_alive_server(after: AfterFirst) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let answered = Arc::new(AtomicUsize::new(0));
        let (conns, done) = (connections.clone(), answered.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                conns.fetch_add(1, Ordering::SeqCst);
                let done = done.clone();
                std::thread::spawn(move || serve_keep_alive(stream, after, &done));
            }
        });
        (format!("http://{addr}/v1"), connections, answered)
    }

    fn serve_keep_alive(mut s: TcpStream, after: AfterFirst, answered: &AtomicUsize) {
        if !read_one_request(&mut s) {
            return;
        }
        let body = envelope(DRAFT_PROPOSAL);
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{body}",
            body.len()
        );
        let _ = s.write_all(reply.as_bytes());
        let _ = s.flush();
        answered.fetch_add(1, Ordering::SeqCst);
        match after {
            AfterFirst::CloseAfter(wait) => {
                std::thread::sleep(wait);
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            AfterFirst::BlackHole => {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
    }

    /// Reads one request (headers and a content-length body); false when
    /// the client closed first.
    fn read_one_request(s: &mut TcpStream) -> bool {
        let mut data = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => return false,
                Ok(n) => data.extend_from_slice(&buf[..n]),
            }
            let text = String::from_utf8_lossy(&data);
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if data.len() >= end + 4 + length {
                    return true;
                }
            }
        }
    }

    #[test]
    fn a_connection_the_server_closed_is_not_reused() {
        let (base, connections, answered) =
            keep_alive_server(AfterFirst::CloseAfter(Duration::from_secs(1)));
        let metered = Metered::new("keep-alive-closed");
        let mut c = metered.client(config(&base));
        c.propose(&task()).unwrap();
        std::thread::sleep(Duration::from_secs(2));
        let (_, inv) = c.propose(&task()).unwrap();
        assert_eq!(inv.attempts, Some(1));
        assert_eq!(answered.load(Ordering::SeqCst), 2);
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert!(
            !metered
                .settled()
                .iter()
                .any(|s| s.0 == Outcome::TransportAfterSend),
            "{:?}",
            metered.settled()
        );
    }

    #[test]
    fn a_connection_idle_past_the_pool_timeout_is_never_reused() {
        // The first connection stays open but is never answered again: a
        // client that reused it would wait out its budget (smoke run K02's
        // failure mode). Past the 5 s idle timeout a fresh one is opened.
        let (base, connections, answered) = keep_alive_server(AfterFirst::BlackHole);
        let metered = Metered::new("keep-alive-black-hole");
        let mut cfg = config(&base);
        cfg.timeout = Duration::from_secs(3);
        cfg.max_attempts = 1;
        let mut c = metered.client(cfg);
        c.propose(&task()).unwrap();
        std::thread::sleep(consensus_api::meta::POOL_IDLE_TIMEOUT + Duration::from_secs(1));
        let (_, inv) = c.propose(&task()).unwrap();
        assert_eq!(inv.attempts, Some(1));
        assert_eq!(answered.load(Ordering::SeqCst), 2);
        assert_eq!(connections.load(Ordering::SeqCst), 2, "a fresh connection");
        assert!(
            metered
                .settled()
                .iter()
                .all(|s| matches!(s.0, Outcome::Success(_))),
            "{:?}",
            metered.settled()
        );
    }
}
