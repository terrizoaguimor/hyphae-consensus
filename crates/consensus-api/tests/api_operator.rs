// SPDX-License-Identifier: Apache-2.0
//! The operator surface over HTTP: bearer token on every `/api` route,
//! Host and Origin checks, the console page, runs through the same
//! manager with `operator` authority, raw failure reasons kept.

mod common;

use axum::http::{header, StatusCode};
use common::doubles::{panicking_start, unavailable_start};
use common::{body_text, operator_server, read_sse, sequences, Req, OPERATOR_TOKEN};
use consensus_api::config::MetaConfig;
use consensus_api::server::BuildOptions;
use consensus_api::static_files::csp_hash;
use consensus_domain::RunOrigin;
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::{Owner, Retention};
use serde_json::json;

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
