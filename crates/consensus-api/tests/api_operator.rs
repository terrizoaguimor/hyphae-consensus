// SPDX-License-Identifier: Apache-2.0
//! The operator surface over HTTP: bearer token on every `/api` route,
//! Host and Origin checks, the console page, runs through the same
//! manager with `operator` authority, raw failure reasons kept, and the
//! metering of `model=meta` runs on the live ledger (against the fake Chat
//! Completions server; never the real API).

mod common;
mod support;

use axum::http::{header, StatusCode};
use chrono::Utc;
use common::doubles::{panicking_start, unavailable_start};
use common::live::{ledger_records, live_config};
use common::{body_text, operator_server, read_sse, sequences, Req, TempDir, OPERATOR_TOKEN};
use consensus_api::api::MetaSource;
use consensus_api::config::MetaConfig;
use consensus_api::live::budget::{Ledger, LedgerOptions};
use consensus_api::live::config::vars;
use consensus_api::runs::RunKind;
use consensus_api::server::{operator_ledger, operator_metering_line, BuildOptions};
use consensus_api::static_files::csp_hash;
use consensus_domain::RunOrigin;
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::{Owner, Retention};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use support::{config as meta_config, envelope, serve, Reply, TEST_KEY};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_api_route_needs_the_bearer_token() {
    let server = operator_server(&[], BuildOptions::default());
    let routes = [
        ("GET", "/api/v1/health"),
        ("GET", "/api/v1/capabilities"),
        ("GET", "/api/v1/ready"),
        ("GET", "/api/v1/runs"),
        (
            "GET",
            "/api/v1/runs/run-00000000-0000-4000-8000-000000000001",
        ),
        (
            "GET",
            "/api/v1/runs/run-00000000-0000-4000-8000-000000000001/events",
        ),
        (
            "GET",
            "/api/v1/runs/run-00000000-0000-4000-8000-000000000001/receipt",
        ),
        ("GET", "/api/v1/showcase"),
        ("POST", "/api/v1/operator/runs"),
        (
            "POST",
            "/api/v1/runs/run-00000000-0000-4000-8000-000000000001/cancel",
        ),
        ("GET", "/api/v1/nope"),
    ];
    for (method, path) in routes {
        let req = match method {
            "GET" => Req::get(path),
            _ => Req::post(path).json(&json!({})),
        };
        // No token.
        let (status, body, headers) = server.json(req.without("authorization")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}: {body}");
        assert_eq!(body["code"], "unauthorized");
        assert!(headers.get(header::WWW_AUTHENTICATE).is_some());
        // Wrong token (same length, one character off).
        let wrong = format!("{}X", &OPERATOR_TOKEN[..OPERATOR_TOKEN.len() - 1]);
        let req = match method {
            "GET" => Req::get(path),
            _ => Req::post(path).json(&json!({})),
        };
        let (status, body, _) = server
            .json(req.header("authorization", &format!("Bearer {wrong}")))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}: {body}");
    }
    // A wrong method on a known route is no route-existence oracle: 401,
    // not 405, without the token.
    for req in [
        Req::delete("/api/v1/runs"),
        Req::new(
            axum::http::Method::PUT,
            "/api/v1/runs/run-00000000-0000-4000-8000-000000000001/decision",
        )
        .json(&json!({})),
        Req::delete("/api/v1/health"),
    ] {
        let path = req.path.clone();
        let (status, body, _) = server.json(req.without("authorization")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}: {body}");
        assert_eq!(body["code"], "unauthorized");
    }
    // With the token the method is what is wrong.
    let (status, body, _) = server.json(Req::delete("/api/v1/runs")).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{body}");
    assert_eq!(body["code"], "method_not_allowed");
    // The operator surface has its own generous limits.
    let (_, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["limits"]["rate_runs_per_min"], 120);
    assert_eq!(caps["limits"]["rate_sse_per_min"], 600);
    assert_eq!(
        caps["limits"]["sse_max_per_client"],
        caps["limits"]["sse_max_global"]
    );
    // The console itself needs no token: it holds no data.
    let response = server.send(Req::get("/").without("authorization")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_and_origin_are_checked() {
    let server = operator_server(&[], BuildOptions::default());
    let (status, body, _) = server
        .json(Req::get("/api/v1/health").header("host", "evil.example"))
        .await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST, "{body}");
    assert_eq!(body["code"], "host_not_allowed");
    let (status, _, _) = server
        .json(Req::get("/api/v1/health").header("host", "localhost:8787"))
        .await;
    assert_eq!(status, StatusCode::OK);
    // A foreign Origin is rejected; a request without one (a script) is
    // not, on this loopback-only surface.
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/operator/runs")
                .header("origin", "http://evil.example")
                .json(&json!({"model":"fake","memory":"fake","scenario":"retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "csrf_rejected");
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/operator/runs")
                .without("origin")
                .without("sec-fetch-site")
                .json(&json!({"model":"fake","memory":"fake","scenario":"retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_console_is_served_with_a_hash_csp_and_no_store() {
    let server = operator_server(&[], BuildOptions::default());
    let response = server.send(Req::get("/")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
    let html = body_text(response).await;
    let script = html
        .split("<script>")
        .nth(1)
        .and_then(|rest| rest.split("</script>").next())
        .expect("inline script");
    assert!(
        csp.contains(&csp_hash(script)),
        "console script hash in CSP: {csp}"
    );
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(!csp.contains("script-src 'unsafe-inline'"), "{csp}");
    assert!(csp.contains("style-src-attr 'unsafe-inline'"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(
        html.contains("/api/v1/operator/runs"),
        "console targets the async routes"
    );
    assert!(
        !html.contains("/api/runs'") && !html.contains("'/api/runs"),
        "old routes are gone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operator_runs_go_through_the_same_manager_with_operator_authority() {
    let server = operator_server(&[], BuildOptions::default());
    let (status, created, headers) = server
        .json(
            Req::post("/api/v1/operator/runs")
                .json(&json!({"model":"fake","memory":"fake","scenario":"invalid_proof"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    assert_eq!(headers[header::LOCATION], format!("/api/v1/runs/{run_id}"));
    assert_eq!(
        created["run"]["provenance"]["declared"]["origin"],
        "operator"
    );

    // The stream replays and ends at the decision point.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let frames = read_sse(response.into_body(), 100).await;
    let last = frames.last().unwrap();
    assert_eq!(last.event, "stream_end");
    assert_eq!(last.json()["reason"], "awaiting_human_approval");
    let seqs = sequences(&frames);
    assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());

    let (status, detail, _) = server
        .json(Req::get(&format!("/api/v1/runs/{run_id}")))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["summary"]["recommendation"], "blocked");
    assert_eq!(detail["summary"]["provenance"]["model"], "scripted");
    assert_eq!(detail["summary"]["provenance"]["evidence"], "simulated");
    // The operator view keeps the simulated verifier's detail.
    let verification = &detail["rounds"][0]["verifications"][0];
    assert_eq!(verification["simulated"], true);
    assert!(verification["detail"]
        .as_str()
        .unwrap()
        .contains("SIMULATED"));

    // Approving a blocked run needs a note; the body cannot name the
    // decider: identity comes from the authenticated channel.
    let expected = json!({
        "recommendation": "blocked",
        "last_sequence": detail["summary"]["last_sequence"]
    });
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .header("idempotency-key", "op-1")
                .json(&json!({"approved": true, "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "override_note_required");
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .header("idempotency-key", "op-1")
                .json(&json!({"approved": true, "authorized_by": "someone", "note": "x", "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .header("idempotency-key", "op-1")
                .json(&json!({"approved": true, "note": "override: accepting the known-corrupt proof risk", "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["decision"]["authority"], "operator");
    assert_eq!(body["decision"]["authorized_by"], "operator");
    assert!(body["decision"]["notice"].is_null());
    assert_eq!(body["run"]["override"], true);
    assert_eq!(body["run"]["state"], "approved");

    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/receipt")))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
    assert_eq!(receipt["authorization"]["authority"], "operator");
    assert_eq!(receipt["authorization"]["state"], "approved");

    let (status, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(caps["surface"], "operator");
    assert_eq!(caps["decision_authority"], "operator");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn modes_are_explicit_and_credentials_are_required() {
    let server = operator_server(&[], BuildOptions::default());
    // model=fake with a live memory is refused, never substituted.
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/operator/runs")
                .json(&json!({"model":"fake","memory":"hyphae","scenario":"retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("silently ignored"));
    // Missing scenario, unknown field, misspelled enum.
    for bad in [
        json!({"model":"fake","memory":"fake"}),
        json!({"model":"fake","memory":"fake","scenario":"retry","scenaro":"x"}),
        json!({"model":"meta","memroy":"hyphae"}),
        json!({"model":"meta","memory":"hyphae","project_id":"../other"}),
    ] {
        let (status, body, _) = server
            .json(Req::post("/api/v1/operator/runs").json(&bad))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
        assert_eq!(body["code"], "invalid_request");
    }
    // Live mode without credentials is an explicit error (only
    // meaningful when the test environment has no key).
    if MetaConfig::from_env().is_none() {
        let (status, body, _) = server
            .json(Req::post("/api/v1/operator/runs").json(&json!({"model":"meta","memory":"fake"})))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "model_not_configured");
        assert!(body["message"].as_str().unwrap().contains("META_API_KEY"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operator_views_keep_the_raw_failure_reason() {
    let server = operator_server(&[], BuildOptions::default());
    let start = panicking_start(Owner::Operator, Retention::Keep, RunOrigin::Operator);
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    let mut detail = serde_json::Value::Null;
    for _ in 0..300 {
        let (status, body, _) = server
            .json(Req::get(&format!("/api/v1/runs/{run_id}")))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["summary"]["state"] == "failed" {
            detail = body;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(detail["summary"]["failure"]["code"], "panic", "{detail}");
    assert_eq!(detail["summary"]["failure"]["stage"], "panic");
    assert_eq!(
        detail["summary"]["failure"]["reason"], "the deliberation thread panicked",
        "the operator sees the recorded reason"
    );
    let failed = detail["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["payload"]["type"] == "run_failed")
        .unwrap();
    assert!(failed["payload"]["reason"].is_string());
    assert!(
        !detail.to_string().contains("boom:"),
        "panic text never reaches a record"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_operator_gets_the_whole_chain_of_a_live_run() {
    const UPSTREAM: &str = "UPSTREAM-BODY-0b1d-operator-only";
    let server = operator_server(&[], BuildOptions::default());
    let start = unavailable_start(
        Owner::Operator,
        Retention::Keep,
        RunOrigin::Operator,
        &format!("meta api http 500: {UPSTREAM}"),
    );
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    let detail = server.wait_resting("", &run_id).await;
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    assert_eq!(detail["summary"]["failure"]["code"], "model_unavailable");
    // The operator surface keeps the raw reason and the verbatim chain,
    // which the public surface withholds for such a run.
    assert!(detail["summary"]["failure"]["reason"]
        .as_str()
        .unwrap()
        .contains(UPSTREAM));
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(text.contains(UPSTREAM));
    assert!(verify_bytes(text.as_bytes()).is_ok(), "the chain verifies");
}

/// The operator route claims its key before the start too: concurrent
/// duplicates of one console submit start one run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_duplicates_of_one_operator_submit_start_one_run() {
    let server = operator_server(
        &[("CONSENSUS_GUIDED_CONCURRENCY", "1")],
        BuildOptions {
            admission_wait: Duration::from_secs(5),
            ..BuildOptions::default()
        },
    );
    let slot = server.state.manager.reserve(RunKind::Guided).await.unwrap();
    let duplicate = || {
        Req::post("/api/v1/operator/runs")
            .header("idempotency-key", "dup-operator-1")
            .json(&json!({"model":"fake","memory":"fake","scenario":"abstain"}))
    };
    let release = async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(slot);
    };
    let (a, b, c, ()) = tokio::join!(
        server.json(duplicate()),
        server.json(duplicate()),
        server.json(duplicate()),
        release
    );
    let outcomes = [a, b, c];
    for (status, body, _) in &outcomes {
        assert_eq!(*status, StatusCode::ACCEPTED, "{outcomes:?}: {body}");
    }
    let first = &outcomes[0].1["run"]["run_id"];
    assert!(
        outcomes
            .iter()
            .all(|(_, body, _)| &body["run"]["run_id"] == first),
        "{outcomes:?}"
    );
    let (_, list, _) = server.json(Req::get("/api/v1/runs")).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 1, "{list}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_operator_ready_keeps_the_index_error_text_next_to_its_code() {
    let server = operator_server(&[], BuildOptions::default());
    let (status, created, _) = server
        .json(
            Req::post("/api/v1/operator/runs")
                .json(&json!({"model":"fake","memory":"fake","scenario":"retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    server.wait_resting("", &run_id).await;
    server.wait_released(&run_id).await;
    server.tamper_index();
    let (status, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    let store = &ready["run_store"];
    assert_eq!(store["index_error_code"], "index_tampered", "{store}");
    assert!(
        store["index_error"]
            .as_str()
            .is_some_and(|text| text.contains("fails verification")),
        "{store}"
    );
}

// ---------------------------------------------------------------------
// model=meta on the live ledger (R3)
// ---------------------------------------------------------------------

/// The drafts the fake Chat Completions server answers for a supported run
/// over `memory=fake` (the reference records): one claim the records
/// corroborate, an empty critique, an evaluation keyed by the id the
/// adapter mints for the claim. Three calls.
const DRAFT_PROPOSAL: &str = r#"{"summary":"Keep retries capped at 3 attempts with jitter","plan_steps":["add jitter to the payment client backoff"],"primary_claim":{"statement":"Payment timeouts correlate with retry bursts above 5 attempts"},"further_claims":[],"assumptions":[],"uncertainties":[]}"#;
const DRAFT_CHALLENGE: &str = r#"{"items":[]}"#;
const DRAFT_EVALUATION: &str = r#"{"assessments":{"claim-2":{"corroborated":true,"relevant_evidence":["ev-fact"],"contradicting_evidence":[],"note":"ev-fact states the correlation"}},"item_verdicts":{},"cleared_contradictions":[],"abstain":false,"abstain_reason":null}"#;

fn one_run() -> Vec<Reply> {
    vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)),
        Reply::ok(envelope(DRAFT_CHALLENGE)),
        Reply::ok(envelope(DRAFT_EVALUATION)),
    ]
}

/// The operator's Meta configuration, pointing at the fake server: never
/// the process environment, so no real key and no real API.
fn fixed_meta(cfg: MetaConfig) -> MetaSource {
    Arc::new(move || Ok(Some(cfg.clone())))
}

/// The live ledger the public server writes, in `dir`: the harness's caps
/// with the production output bound (16,000), so a run's worst case is
/// (70,000 + 16,000 + 32,000) × $20/M = $2.36.
fn server_ledger(dir: &Path, overrides: &[(&'static str, &str)]) -> Ledger {
    let mut env: Vec<(&'static str, &str)> = vec![(vars::MAX_COMPLETION_TOKENS, "16000")];
    env.extend_from_slice(overrides);
    let live = live_config(dir, &env);
    Ledger::open_with(
        live.ledger_dir.as_ref().unwrap(),
        live.budget.clone(),
        Arc::new(Utc::now),
        LedgerOptions {
            lock_wait: Duration::from_secs(5),
            lock_poll: Duration::from_millis(5),
        },
    )
    .unwrap()
}

async fn wait_for_records(dir: &Path, kind: &str, n: usize) -> Vec<Value> {
    for _ in 0..300 {
        let records = ledger_records(dir, kind);
        if records.len() >= n {
            return records;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "expected {n} {kind} record(s): {:?}",
        ledger_records(dir, kind)
    );
}

fn meta_run() -> Req {
    Req::post("/api/v1/operator/runs").json(&json!({"model":"meta","memory":"fake"}))
}

/// Every file's text under `dir`, concatenated (for canaries).
fn files_text(dir: &Path) -> String {
    let mut out = String::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            out.push_str(&text);
        }
    }
    out
}

/// With the live ledger configured, a console `model=meta` run reserves its
/// worst case on the operator lane, draws and settles every HTTP attempt,
/// and releases the reservation when it ends; the kill switch and the trip
/// it records then refuse the next one with the ledger's code, before any
/// call. Before, `serve-operator` never opened the ledger: operator runs
/// spent outside the caps and the kill switch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_metered_operator_run_debits_the_live_ledger_and_is_refused_while_it_is_tripped() {
    let meta = serve(one_run());
    let scratch = TempDir::new("operator-ledger");
    let ledger = server_ledger(scratch.path(), &[]);
    let ledger_dir = ledger.dir().to_path_buf();
    let server = operator_server(
        &[],
        BuildOptions {
            operator_ledger: Some(ledger.clone()),
            operator_meta: Some(fixed_meta(meta_config(&meta.base_url))),
            ..BuildOptions::default()
        },
    );
    let (status, created, _) = server.json(meta_run()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    let detail = server.wait_resting("", &run_id).await;
    assert_eq!(detail["summary"]["recommendation"], "supported", "{detail}");
    assert_eq!(detail["summary"]["provenance"]["model"], "live_meta");
    assert_eq!(meta.hits(), 3);

    let releases = wait_for_records(&ledger_dir, "release", 1).await;
    let reservations = ledger_records(&ledger_dir, "reservation");
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0]["lane"], "operator");
    assert_eq!(reservations[0]["worst_case_usd"], "2.360000");
    assert_eq!(releases[0]["reservation"], reservations[0]["id"]);
    assert_eq!(ledger_records(&ledger_dir, "draw").len(), 3, "one per call");
    let settlements = ledger_records(&ledger_dir, "settlement");
    assert_eq!(settlements.len(), 3);
    for s in &settlements {
        assert_eq!(s["outcome"]["kind"], "usage", "{s}");
        // 100 × $4/M + 50 × $20/M.
        assert_eq!(s["charged_usd"], "0.001400");
    }
    let snapshot = ledger.snapshot().unwrap();
    assert_eq!(snapshot.lifetime_spent.to_fixed(), "0.004200");
    assert_eq!(snapshot.unsettled_reservations, 0);
    // The operator lane spends from the day the public lane shares.
    assert_eq!(snapshot.daily_committed.to_fixed(), "0.004200");

    // The kill switch stops operator runs: the trip is recorded, the run is
    // refused with the ledger's code and fixed sentence, nothing is sent.
    std::fs::write(ledger_dir.join("kill-switch"), b"stop\n").unwrap();
    let (status, body, _) = server.json(meta_run()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_tripped");
    assert_eq!(body["message"], "live mode is paused");
    assert_eq!(ledger_records(&ledger_dir, "trip").len(), 1);
    // Removing the file reopens nothing: the trip holds until a reset.
    std::fs::remove_file(ledger_dir.join("kill-switch")).unwrap();
    let (status, body, _) = server.json(meta_run()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_tripped");
    assert_eq!(meta.hits(), 3, "no call after the trip");
    assert_eq!(ledger_records(&ledger_dir, "reservation").len(), 1);
    let (_, list, _) = server.json(Req::get("/api/v1/runs")).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 1, "no orphan run");
    // A reset (the operator's ledger subcommand) lets the console run again.
    ledger.reset("operator-test").unwrap();
    assert!(!ledger.is_tripped().unwrap());
    assert!(
        !files_text(&ledger_dir).contains(TEST_KEY),
        "the key is nowhere"
    );
}

/// A metered run's calls must fit the ledger's per-call output bound: one
/// that does not is refused before anything is reserved or sent, naming
/// both variables.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_metered_operator_run_must_fit_the_ledgers_output_bound() {
    let meta = serve(one_run());
    let scratch = TempDir::new("operator-bound");
    let ledger = server_ledger(scratch.path(), &[(vars::MAX_COMPLETION_TOKENS, "12000")]);
    let ledger_dir = ledger.dir().to_path_buf();
    let server = operator_server(
        &[],
        BuildOptions {
            operator_ledger: Some(ledger),
            // 16,000: the Meta default.
            operator_meta: Some(fixed_meta(meta_config(&meta.base_url))),
            ..BuildOptions::default()
        },
    );
    let (status, body, _) = server.json(meta_run()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "model_not_configured");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("CONSENSUS_META_MAX_COMPLETION_TOKENS")
            && message.contains("CONSENSUS_LIVE_MAX_COMPLETION_TOKENS"),
        "{message}"
    );
    assert!(ledger_records(&ledger_dir, "reservation").is_empty());
    assert_eq!(meta.hits(), 0);
}

/// Without a ledger directory (a development machine running the recorded
/// batteries) a console `model=meta` run is unmetered, and the startup
/// line says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_ledger_directory_operator_runs_are_unmetered_and_the_startup_line_says_so() {
    let meta = serve(one_run());
    let server = operator_server(
        &[],
        BuildOptions {
            operator_meta: Some(fixed_meta(meta_config(&meta.base_url))),
            ..BuildOptions::default()
        },
    );
    assert!(server.state.operator_ledger.is_none());
    let (status, created, _) = server.json(meta_run()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    let detail = server.wait_resting("", &run_id).await;
    assert_eq!(detail["summary"]["recommendation"], "supported", "{detail}");
    assert_eq!(meta.hits(), 3);
    // No ledger file was written anywhere under the server's directory.
    fn ledger_files(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                ledger_files(&path, found);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "ledger.lock" || name == "lifetime.json" {
                    found.push(path.display().to_string());
                }
            }
        }
    }
    let mut found = Vec::new();
    ledger_files(server.dir.path(), &mut found);
    assert!(found.is_empty(), "{found:?}");
    let line = operator_metering_line(None);
    assert!(
        line.contains("UNMETERED") && line.contains(vars::LEDGER_DIR),
        "{line}"
    );
}

/// What `serve-operator` makes of the server's environment: no ledger
/// without `CONSENSUS_LIVE_LEDGER_DIR`; an error naming the variable for a
/// directory that does not exist (never a fresh, empty ledger) or for an
/// unusable budget variable; otherwise the ledger with the server's caps,
/// read without any credential (live mode on, the Turnstile secret and the
/// judge codes absent, as they are outside the public unit).
#[test]
fn serve_operator_opens_an_existing_live_ledger_with_the_server_caps_and_no_credential() {
    assert!(operator_ledger(&BTreeMap::new()).unwrap().is_none());
    let scratch = TempDir::new("operator-ledger-env");
    let dir = scratch.path().join("ledger").join("public");
    let mut env = BTreeMap::from([(vars::LEDGER_DIR, dir.display().to_string())]);
    let err = operator_ledger(&env).unwrap_err();
    assert!(err.contains(vars::LEDGER_DIR), "{err}");
    assert!(!dir.exists(), "nothing is created");

    std::fs::create_dir_all(&dir).unwrap();
    env.insert(vars::LIVE_ENABLED, "true".into());
    env.insert(
        vars::TURNSTILE_SECRET_FILE,
        scratch.path().join("absent.secret").display().to_string(),
    );
    env.insert(
        vars::JUDGE_CODES_FILE,
        scratch.path().join("absent-codes").display().to_string(),
    );
    // public.env's other Turnstile settings, as deployed: the hostname is
    // required by serve-public and must not trip the credential-free load.
    env.insert(vars::TURNSTILE_HOSTNAME, "portal.example".into());
    env.insert(vars::TURNSTILE_ACTION_LIVE, "live-run".into());
    env.insert(vars::TURNSTILE_ACTION_SESSION, "session".into());
    env.insert(vars::DAILY_USD, "5.00".into());
    let ledger = operator_ledger(&env).unwrap().expect("the server's ledger");
    assert_eq!(ledger.dir(), dir.as_path());
    assert_eq!(ledger.config().daily_cap.to_fixed(), "5.000000");
    let line = operator_metering_line(Some(&ledger));
    assert!(line.contains("metered on the live ledger"), "{line}");
    assert!(line.contains("daily cap 5.00"), "{line}");

    env.insert(vars::DAILY_USD, "ten".into());
    let err = operator_ledger(&env).unwrap_err();
    assert!(err.contains(vars::DAILY_USD), "{err}");
}
