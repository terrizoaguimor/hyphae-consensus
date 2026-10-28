// SPDX-License-Identifier: Apache-2.0
//! The public surface over HTTP: sessions and request security, guided
//! runs end to end (202, SSE replay and follow, resume, decisions,
//! receipts, ndjson), the run lifecycle under cancel, disconnect, panic
//! and shutdown, ownership, and every limit.

mod common;

use axum::http::{header, StatusCode};
use common::doubles::{gated_start, panicking_start, unavailable_start, GateMode, TestGate};
use common::{
    body_bytes, body_text, cookie_value, public_server, read_sse, sequences, Req, SseReader,
    INDEX_SCRIPT,
};
use consensus_api::runs::RunKind;
use consensus_api::server::BuildOptions;
use consensus_api::static_files::csp_hash;
use consensus_domain::*;
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::{IndexKind, Owner, Retention};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

fn visitor_retention() -> Retention {
    Retention::Expiring {
        access_for: Duration::from_secs(3600),
        delete_after: Duration::from_secs(7200),
    }
}

/// The owner of a session cookie, as the store records it.
fn owner_of(server: &common::TestServer, cookie: &str) -> Owner {
    server.state.sessions.lookup(cookie).unwrap().owner()
}

fn expected_of(detail: &Value) -> Value {
    json!({
        "recommendation": detail["summary"]["recommendation"],
        "last_sequence": detail["summary"]["last_sequence"]
    })
}

// ---------------------------------------------------------------------
// Sessions and request security
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_are_minted_by_post_only_and_carry_the_right_attributes() {
    let server = public_server(&[], BuildOptions::default());
    // No session: the run routes refuse before anything else.
    let (status, body, _) = server.json(Req::get("/api/v1/runs")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "session_required");
    let (status, body, _) = server.json(Req::get("/api/v1/session")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    // A GET never mints one.
    let response = server.send(Req::get("/")).await;
    assert!(cookie_value(response.headers()).is_none());

    let response = server.send(Req::post("/api/v1/session")).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let set_cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    // cookie_secure=false in this test: the __Host- prefix requires Secure
    // (a browser would drop the cookie), so the plain name is used.
    for attr in ["hc_session=", "HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(set_cookie.contains(attr), "{set_cookie}");
    }
    assert!(
        !set_cookie.contains("Secure") && !set_cookie.contains("__Host-"),
        "{set_cookie}"
    );
    let cookie = cookie_value(response.headers()).unwrap();
    assert_eq!(cookie.len(), 64);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    assert_eq!(body["created"], true);
    assert!(body["pseudonym"].as_str().unwrap().starts_with("visitor:"));

    // The secure default sets Secure, with the host-locked name.
    let secure = public_server(
        &[("CONSENSUS_COOKIE_SECURE", "true")],
        BuildOptions::default(),
    );
    let response = secure.send(Req::post("/api/v1/session")).await;
    let set_cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.starts_with("__Host-hc_session="), "{set_cookie}");
    assert!(set_cookie.contains("; Secure"), "{set_cookie}");
    let secure_cookie = cookie_value(response.headers()).unwrap();
    let (status, _, _) = secure
        .json(Req::get("/api/v1/session").cookie(&secure_cookie))
        .await;
    assert_eq!(status, StatusCode::OK);

    // A second POST with a live cookie keeps the session.
    let (status, body, headers) = server
        .json(Req::post("/api/v1/session").cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["created"], false);
    assert!(headers.get(header::SET_COOKIE).is_none());
    let (status, _, _) = server
        .json(Req::get("/api/v1/session").cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    assert_eq!(status, StatusCode::OK);

    // DELETE forgets it and clears the cookie.
    let response = server
        .send(Req::delete("/api/v1/session").cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let (status, _, _) = server
        .json(Req::get("/api/v1/session").cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn csrf_content_type_unknown_fields_and_size_are_enforced() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let body = json!({"scenario": "retry"});
    let cases: Vec<(Req, StatusCode, &str)> = vec![
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&body)
                .without("origin"),
            StatusCode::FORBIDDEN,
            "csrf_rejected",
        ),
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&body)
                .header("origin", "https://evil.example"),
            StatusCode::FORBIDDEN,
            "csrf_rejected",
        ),
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&body)
                .header("sec-fetch-site", "cross-site"),
            StatusCode::FORBIDDEN,
            "csrf_rejected",
        ),
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .raw_body(body.to_string().into_bytes())
                .header("content-type", "text/plain"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&json!({"scenario": "retry", "title": "free text"})),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ),
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&json!({"scenario": "invalid-proof"})),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ),
        (
            Req::post("/api/v1/runs").cookie(&cookie).json(&json!({})),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ),
        (
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&json!({"scenario": "retry", "pad": "x".repeat(17 * 1024)})),
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
        ),
        (
            Req::post("/api/v1/session").without("origin"),
            StatusCode::FORBIDDEN,
            "csrf_rejected",
        ),
        (
            Req::get("/api/v1/health").header("host", "evil.example"),
            StatusCode::MISDIRECTED_REQUEST,
            "host_not_allowed",
        ),
        (
            Req::get("/api/v1/health").without("host"),
            StatusCode::MISDIRECTED_REQUEST,
            "host_not_allowed",
        ),
        (
            Req::get("/api/v1/nope").cookie(&cookie),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            Req::delete("/api/v1/runs").cookie(&cookie),
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ),
    ];
    for (req, status, code) in cases {
        let (got, body, _) = server.json(req).await;
        assert_eq!(got, status, "{code}: {body}");
        assert_eq!(body["code"], code, "{body}");
        assert!(body["message"].is_string());
    }
    // A text/plain POST with the JSON body never reaches the handler, and
    // nothing was created meanwhile.
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn security_headers_and_per_page_csp_are_present() {
    let server = public_server(&[], BuildOptions::default());
    let response = server.send(Req::get("/api/v1/health")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let h = response.headers();
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(h[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(h[header::X_FRAME_OPTIONS], "DENY");
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
    assert_eq!(h["cross-origin-resource-policy"], "same-origin");
    // Health is static: no environment probing, the exact field set.
    let health: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let keys: Vec<&str> = health
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut keys = keys;
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "contract_sha256",
            "executor",
            "policy_version",
            "schema_version",
            "status",
            "version"
        ]
    );
    assert_eq!(health["executor"], "disabled");
    assert_eq!(health["contract_sha256"].as_str().unwrap().len(), 64);

    let response = server.send(Req::get("/")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        csp.contains(&format!("'{}'", csp_hash(INDEX_SCRIPT))),
        "{csp}"
    );
    assert!(csp.contains("default-src 'none'") && csp.contains("frame-ancestors 'none'"));
    assert!(csp.contains("https://challenges.cloudflare.com"));
    assert!(!csp.contains("unsafe-eval") && !csp.contains("script-src 'self' 'unsafe-inline'"));
    assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
    // HTTP/2 style: the authority travels in the request target, with no
    // Host header; it is checked the same way.
    let response = server
        .send(Req::get("http://localhost:8788/api/v1/health").without("host"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = server
        .send(Req::get("http://evil.example/api/v1/health").without("host"))
        .await;
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    // Every limit is reported, the request timeout included.
    let (_, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["limits"]["request_timeout_secs"], 30);
    let response = server.send(Req::get("/en/")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        !csp.contains(&csp_hash(INDEX_SCRIPT)),
        "another page's hash must not leak: {csp}"
    );
    let response = server.send(Req::get("/nope/")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_text(response).await.contains("missing"));
    // The old console and its routes do not exist on this listener.
    let response = server.send(Req::get("/api/runs")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = server.send(Req::get("/api/health")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capabilities_scenarios_ready_and_the_disabled_live_route() {
    let server = public_server(&[], BuildOptions::default());
    let (status, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(caps["surface"], "public");
    assert_eq!(caps["live_enabled"], false);
    assert_eq!(caps["guided_enabled"], true);
    assert_eq!(caps["showcase_enabled"], true);
    assert_eq!(caps["decision_authority"], "public_demo");
    assert_eq!(caps["limits"]["body_limit_bytes"], 16384);
    assert_eq!(caps["limits"]["sse_heartbeat_secs"], 1);
    assert_eq!(caps["schema_version"], SCHEMA_VERSION);
    assert_eq!(caps["protocol"], "protocol 1.3 · policy rules 1.2");

    let (status, scenarios, _) = server.json(Req::get("/api/v1/scenarios")).await;
    assert_eq!(status, StatusCode::OK);
    let list = scenarios["scenarios"].as_array().unwrap();
    assert_eq!(list.len(), 5);
    assert!(list
        .iter()
        .all(|s| s["simulated"] == true && s["model"] == "scripted"));
    assert_eq!(list[2]["id"], "invalid_proof");
    assert_eq!(list[2]["expected_outcome"], "blocked");

    let (status, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["run_store"]["ok"], true);
    assert_eq!(ready["showcase"]["ok"], true);
    assert_eq!(ready["active_runs"], 0);

    let cookie = server.new_session().await;
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/live-runs")
                .cookie(&cookie)
                .json(&json!({"preset_id": "retry-cap-3"})),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_disabled");
    // Even disabled, the route accepts no free text.
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/live-runs")
                .cookie(&cookie)
                .json(&json!({"preset_id": "retry-cap-3", "description": "do this"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

// ---------------------------------------------------------------------
// Guided runs end to end
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guided_run_is_accepted_streamed_decided_and_receipted() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let (status, created, headers) = server
        .json(
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .json(&json!({"scenario": "retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    assert!(run_id.starts_with("run-"));
    assert_eq!(headers[header::LOCATION], format!("/api/v1/runs/{run_id}"));
    assert_eq!(
        created["run"]["event_count"], 1,
        "202 arrives once the task is durable"
    );
    assert_eq!(created["run"]["provenance"]["model"], "undetermined");
    assert_eq!(created["run"]["provenance"]["declared"]["origin"], "public");
    assert_eq!(
        created["run"]["provenance"]["declared"]["preset"],
        "guided:retry"
    );
    assert_eq!(
        created["links"]["events"],
        format!("/api/v1/runs/{run_id}/events")
    );

    // The stream replays what is stored and ends at the decision point.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let frames = read_sse(response.into_body(), 200).await;
    assert_eq!(frames[0].retry, Some(3000), "retry hint first");
    let end = frames.last().unwrap();
    assert_eq!(end.event, "stream_end");
    assert_eq!(end.json()["reason"], "awaiting_human_approval");
    let seqs = sequences(&frames);
    let n = seqs.len() as u64;
    assert_eq!(seqs, (1..=n).collect::<Vec<_>>(), "gap-free, no duplicates");
    assert_eq!(end.json()["last_sequence"], n);
    for frame in frames.iter().filter(|f| f.event == "consensus") {
        let data = frame.json();
        assert_eq!(
            frame.id.as_deref(),
            Some(data["event"]["sequence"].to_string().as_str())
        );
        assert!(matches!(
            data["delivery"].as_str(),
            Some("replay") | Some("live")
        ));
        assert_eq!(data["summary"]["run_id"], run_id);
    }

    let detail = server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "awaiting_human_approval");
    assert_eq!(detail["summary"]["recommendation"], "supported");
    assert_eq!(detail["summary"]["provenance"]["model"], "scripted");
    assert_eq!(detail["summary"]["provenance"]["evidence"], "simulated");
    assert_eq!(detail["summary"]["provenance"]["source"], "this_server");
    assert!(
        detail["summary"]["provenance"]["flags"]
            .as_array()
            .is_none_or(|flags| flags.is_empty()),
        "no provenance flags on a consistent run"
    );
    assert_eq!(detail["summary"]["active"], false);
    assert_eq!(detail["chain"]["scope"], "run");
    assert_eq!(detail["rounds"].as_array().unwrap().len(), 2);
    assert_eq!(
        detail["recommendation"]["rationale_code"],
        "supported.all_checks_passed"
    );
    assert_eq!(
        detail["recommendation"]["trace"]["rule"],
        "all_checks_passed"
    );
    let items = detail["challenge_items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["status"] == "resolved"));
    // Public views carry no backend internals.
    let text = detail.to_string();
    assert!(!text.contains("fake-memory:"), "backend_ref leaked: {text}");
    assert!(!text.contains("proof_file"));

    // The decision is bound to what the human saw.
    let expected = expected_of(&detail);
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .json(&json!({"approved": true, "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "idempotency_key_required");
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "k1")
                .json(&json!({"approved": true, "expected": {"recommendation": "contested", "last_sequence": expected["last_sequence"]}})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "decision_conflict");
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "k1")
                .json(&json!({"approved": true, "expected": {"recommendation": "supported", "last_sequence": 1}})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "decision_conflict");

    let decision = json!({"approved": true, "note": "looks right", "expected": expected});
    let (status, first, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "k1")
                .json(&decision),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["run"]["state"], "approved");
    assert_eq!(first["run"]["authorization"], "approved");
    assert_eq!(first["run"]["override"], false);
    assert_eq!(first["decision"]["authority"], "public_demo");
    assert!(first["decision"]["authorized_by"]
        .as_str()
        .unwrap()
        .starts_with("visitor:"));
    assert!(first["decision"]["notice"]
        .as_str()
        .unwrap()
        .contains("no operational authority"));
    assert!(
        first["decision"].get("idempotency_key").is_none(),
        "public view"
    );

    // Same key, same body: the same answer, nothing appended.
    let (status, again, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "k1")
                .json(&decision),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["run"]["event_count"], first["run"]["event_count"]);
    // Same key, different body: refused through the log's conflict check.
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "k1")
                .json(&json!({"approved": false, "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "idempotency_key_reused");
    // A new key on a decided run: it is no longer awaiting a decision.
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "k2")
                .json(&json!({"approved": false, "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "run_not_awaiting_decision");
    // Nor cancellable.
    let (status, body, _) = server
        .json(Req::post(&format!("/api/v1/runs/{run_id}/cancel")).cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "run_not_cancellable");

    // The stream of a decided run replays and ends as terminal, and no
    // replayed frame of a resting run claims the deliberation is active.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    let frames = read_sse(response.into_body(), 200).await;
    let end = frames.last().unwrap().json();
    assert_eq!(end["reason"], "terminal");
    assert_eq!(end["state"], "approved");
    assert_eq!(sequences(&frames).len() as u64, n + 1);
    for frame in frames.iter().filter(|f| f.event == "consensus") {
        let data = frame.json();
        assert_eq!(data["delivery"], "replay");
        assert_eq!(
            data["summary"]["active"], false,
            "replay of a finished run at sequence {}",
            data["event"]["sequence"]
        );
    }

    // Receipt v2 and the verbatim ndjson agree with the detail.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/receipt")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains("attachment"));
    let receipt: RunReceiptV2 = serde_json::from_str(&body_text(response).await).unwrap();
    assert_eq!(receipt.event_count, n + 1);
    assert_eq!(
        receipt.authorization.authority.as_deref(),
        Some("public_demo")
    );
    assert_eq!(receipt.authorization.state, AuthorizationState::Approved);
    assert_eq!(
        receipt.recommendation.as_ref().unwrap().kind,
        RecommendationKind::Supported
    );
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/x-ndjson"
    );
    let ndjson = body_bytes(response).await;
    let events = verify_bytes(&ndjson).expect("the ndjson re-verifies as a chain");
    assert_eq!(events.len() as u64, n + 1);
    assert_eq!(
        events.last().unwrap().hash,
        receipt.chain.head_hash.unwrap()
    );
    let (_, after, _) = server
        .json(Req::get(&format!("/api/v1/runs/{run_id}")).cookie(&cookie))
        .await;
    assert_eq!(after["chain"]["head_hash"], events.last().unwrap().hash);
    assert_eq!(after["decision"]["authority"], "public_demo");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_runs_need_an_override_note_and_every_scenario_reaches_its_outcome() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let expected = [
        ("retry", "supported"),
        ("offline", "insufficient_evidence"),
        ("invalid_proof", "blocked"),
        ("injection", "insufficient_evidence"),
        ("abstain", "abstained"),
    ];
    let mut blocked = None;
    for (scenario, outcome) in expected {
        let (status, created) = server.create_guided(&cookie, scenario).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{scenario}: {created}");
        let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
        let detail = server.wait_resting(&cookie, &run_id).await;
        assert_eq!(detail["summary"]["recommendation"], outcome, "{scenario}");
        assert_eq!(detail["summary"]["authorization"], "pending");
        for v in detail["rounds"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["verifications"].as_array().unwrap())
        {
            assert_eq!(v["simulated"], true, "{scenario}");
        }
        if scenario == "invalid_proof" {
            blocked = Some((run_id, detail));
        }
    }
    let (run_id, detail) = blocked.unwrap();
    let expected = expected_of(&detail);
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "approve-1")
                .json(&json!({"approved": true, "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "override_note_required");
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "approve-1")
                .json(&json!({"approved": true, "note": "   ", "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "approve-1")
                .json(&json!({"approved": true, "note": "override: demo", "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run"]["override"], true);
    // A note too long is refused before anything is read.
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "approve-2")
                .json(&json!({"approved": false, "note": "x".repeat(2001), "expected": expected})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // The list is newest first and paginates by cursor.
    let (status, page, _) = server
        .json(Req::get("/api/v1/runs?limit=2").cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["runs"].as_array().unwrap().len(), 2);
    assert_eq!(
        page["runs"][0]["provenance"]["declared"]["preset"],
        "guided:abstain"
    );
    let cursor = page["next_cursor"].as_str().unwrap().to_string();
    let (status, page2, _) = server
        .json(Req::get(&format!("/api/v1/runs?limit=2&cursor={cursor}")).cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::OK, "{page2}");
    assert_eq!(page2["runs"].as_array().unwrap().len(), 2);
    let (status, page3, _) = server
        .json(
            Req::get(&format!(
                "/api/v1/runs?limit=2&cursor={}",
                page2["next_cursor"].as_str().unwrap()
            ))
            .cookie(&cookie),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page3}");
    assert_eq!(page3["runs"].as_array().unwrap().len(), 1);
    assert!(page3["next_cursor"].is_null());
    let (status, body, _) = server
        .json(Req::get("/api/v1/runs?limit=0").cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body, _) = server
        .json(Req::get("/api/v1/runs?bogus=1").cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_guided_runs_stay_isolated_and_ordered() {
    // Four of the six starts wait for one of the two slots. This test is
    // about isolation and order, not admission timing (the Busy answer has
    // its own test), so the wait is generous: under a loaded machine the
    // default 2 s expired and a start was refused (seen 2026-10-28).
    let server = Arc::new(public_server(
        &[("CONSENSUS_GUIDED_CONCURRENCY", "2")],
        BuildOptions {
            admission_wait: Duration::from_secs(30),
            ..BuildOptions::default()
        },
    ));
    let cookie = server.new_session().await;
    let scenarios = [
        "retry",
        "offline",
        "invalid_proof",
        "injection",
        "abstain",
        "retry",
    ];
    let mut handles = Vec::new();
    for scenario in scenarios {
        let server = server.clone();
        let cookie = cookie.clone();
        handles.push(tokio::spawn(async move {
            let (status, created) = server.create_guided(&cookie, scenario).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{scenario}: {created}");
            (
                scenario,
                created["run"]["run_id"].as_str().unwrap().to_string(),
            )
        }));
    }
    let mut ids = Vec::new();
    for handle in handles {
        ids.push(handle.await.unwrap());
    }
    let distinct: std::collections::HashSet<&String> = ids.iter().map(|(_, id)| id).collect();
    assert_eq!(distinct.len(), ids.len());
    for (scenario, run_id) in &ids {
        let detail = server.wait_resting(&cookie, run_id).await;
        let events = detail["events"].as_array().unwrap();
        assert!(
            events.iter().all(|e| e["run_id"] == *run_id),
            "{scenario}: foreign event"
        );
        let seqs: Vec<u64> = events
            .iter()
            .map(|e| e["sequence"].as_u64().unwrap())
            .collect();
        assert_eq!(
            seqs,
            (1..=seqs.len() as u64).collect::<Vec<_>>(),
            "{scenario}"
        );
        assert_eq!(
            detail["summary"]["provenance"]["declared"]["preset"],
            format!("guided:{scenario}")
        );
        assert_eq!(detail["chain"]["scope"], "run");
    }
    let (_, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["active_runs"], 0);
}

// ---------------------------------------------------------------------
// SSE resume, lag, disconnect
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_resumes_from_every_sequence_without_duplicates_or_gaps() {
    // Every resume point is tried twice: well over the default 30
    // streams per minute per client.
    let server = public_server(
        &[("CONSENSUS_RATE_SSE_PER_MIN", "1000")],
        BuildOptions::default(),
    );
    let cookie = server.new_session().await;
    let (_, created) = server.create_guided(&cookie, "retry").await;
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    let detail = server.wait_resting(&cookie, &run_id).await;
    let n = detail["summary"]["last_sequence"].as_u64().unwrap();
    assert!(n > 5);
    for k in 0..=n {
        for (name, req) in [
            (
                "Last-Event-ID",
                Req::get(&format!("/api/v1/runs/{run_id}/events"))
                    .cookie(&cookie)
                    .header("last-event-id", &k.to_string()),
            ),
            (
                "?after",
                Req::get(&format!("/api/v1/runs/{run_id}/events?after={k}")).cookie(&cookie),
            ),
        ] {
            let response = server.send(req).await;
            assert_eq!(response.status(), StatusCode::OK, "{name} resume from {k}");
            let frames = read_sse(response.into_body(), 200).await;
            let seqs = sequences(&frames);
            assert_eq!(
                seqs,
                (k + 1..=n).collect::<Vec<_>>(),
                "{name} resume from {k}"
            );
            let end = frames.last().unwrap();
            assert_eq!(end.event, "stream_end", "{name} from {k}");
            assert_eq!(end.json()["last_sequence"], n);
            assert_eq!(end.json()["reason"], "awaiting_human_approval");
        }
    }
    // The header wins over the query.
    let response = server
        .send(
            Req::get(&format!("/api/v1/runs/{run_id}/events?after=2"))
                .cookie(&cookie)
                .header("last-event-id", &(n - 1).to_string()),
        )
        .await;
    let frames = read_sse(response.into_body(), 200).await;
    assert_eq!(sequences(&frames), vec![n]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lagged_subscriber_reconciles_from_the_log() {
    let server = public_server(
        &[],
        BuildOptions {
            broadcast_capacity: 2,
            ..BuildOptions::default()
        },
    );
    let cookie = server.new_session().await;
    let (start, gate) = gated_start(owner_of(&server, &cookie), visitor_retention(), "propose");
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    gate.wait_entered();

    // Subscribe and consume the stored task record; the stream now
    // follows the channel, and this client stops reading.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut reader = SseReader::new(response.into_body());
    let first = reader.next_event("consensus").await.expect("task frame");
    assert_eq!(first.json()["event"]["sequence"], 1);
    assert_eq!(first.json()["delivery"], "replay");

    // More than two records land in a two-slot channel meanwhile.
    gate.open();
    server.wait_resting(&cookie, &run_id).await;

    let rest = reader.rest(200).await;
    assert!(
        rest.iter()
            .any(|f| f.event == "resync" && f.json()["reason"] == "lagged"),
        "a lagged subscriber is told it was resynced: {rest:?}"
    );
    let mut seqs = vec![1];
    seqs.extend(sequences(&rest));
    let n = *seqs.last().unwrap();
    assert_eq!(
        seqs,
        (1..=n).collect::<Vec<_>>(),
        "gap-free, no duplicates after the resync"
    );
    let end = rest.last().unwrap();
    assert_eq!(end.event, "stream_end");
    assert_eq!(end.json()["reason"], "awaiting_human_approval");
    assert_eq!(end.json()["last_sequence"], n);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_disconnected_client_never_stops_a_run() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let (start, gate) = gated_start(owner_of(&server, &cookie), visitor_retention(), "critique");
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    gate.wait_entered();

    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(server.state.stream_caps.open_streams(), 1);
    let mut reader = SseReader::new(response.into_body());
    let frame = reader.next_event("consensus").await.expect("first frame");
    assert_eq!(frame.json()["event"]["sequence"], 1);
    // The browser goes away mid-stream.
    drop(reader);
    assert!(server.state.manager.is_active(&RunId::new(&run_id)));
    assert_eq!(
        server.state.stream_caps.open_streams(),
        0,
        "the stream slot is released"
    );

    gate.open();
    let detail = server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "awaiting_human_approval");
    assert_eq!(detail["summary"]["recommendation"], "supported");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_decision_during_deliberation_is_refused_and_live_frames_say_live() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let (start, gate) = gated_start(owner_of(&server, &cookie), visitor_retention(), "critique");
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    gate.wait_entered();

    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "early")
                .json(&json!({"approved": true, "expected": {"recommendation": "supported", "last_sequence": 1}})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "run_not_awaiting_decision");
    let (_, detail) = server.detail(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["active"], true);
    assert!(detail["summary"]["event_count"].as_u64().unwrap() >= 2);

    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    let mut reader = SseReader::new(response.into_body());
    let first = reader.next_event("consensus").await.unwrap();
    assert_eq!(first.json()["delivery"], "replay");
    gate.open();
    let rest = reader.rest(200).await;
    let deliveries: Vec<String> = rest
        .iter()
        .filter(|f| f.event == "consensus")
        .map(|f| f.json()["delivery"].as_str().unwrap().to_string())
        .collect();
    assert!(deliveries.iter().any(|d| d == "live"), "{deliveries:?}");
    let first_live = deliveries.iter().position(|d| d == "live").unwrap();
    assert!(
        deliveries[first_live..].iter().all(|d| d == "live"),
        "replay never follows live: {deliveries:?}"
    );
    let mut seqs = vec![1];
    seqs.extend(sequences(&rest));
    assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());
    // Every frame's summary says what the records prove: a scripted
    // model, never live_meta, whatever the delivery.
    for frame in rest.iter().filter(|f| f.event == "consensus") {
        assert_ne!(frame.json()["summary"]["provenance"]["model"], "live_meta");
    }
    assert_eq!(
        rest.last().unwrap().json()["reason"],
        "awaiting_human_approval"
    );
    // Live frames of a deliberating run say active; the frame whose record
    // brings the run to its decision point does not.
    let live: Vec<Value> = rest
        .iter()
        .filter(|f| f.event == "consensus" && f.json()["delivery"] == "live")
        .map(|f| f.json())
        .collect();
    let (last, before) = live.split_last().unwrap();
    assert!(
        before.iter().all(|f| f["summary"]["active"] == true),
        "{before:?}"
    );
    assert_eq!(last["summary"]["state"], "awaiting_human_approval");
    assert_eq!(last["summary"]["active"], false);

    // What subscribers saw is exactly what is stored: a record is
    // published only after its fsync, so every frame's sequence and hash
    // match the durable log.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")).cookie(&cookie))
        .await;
    let stored = verify_bytes(&body_bytes(response).await).expect("stored chain verifies");
    let mut published = 0;
    for frame in rest.iter().filter(|f| f.event == "consensus") {
        let data = frame.json();
        let seq = data["event"]["sequence"].as_u64().unwrap();
        let record = stored
            .iter()
            .find(|e| e.sequence == seq)
            .expect("every published record is stored");
        assert_eq!(
            data["event"]["hash"], record.hash,
            "frame {seq} is the stored record"
        );
        published += 1;
    }
    assert_eq!(published + 1, stored.len(), "one frame per stored record");

    let detail = server.wait_resting(&cookie, &run_id).await;
    let (status, body, _) = server
        .json(
            Req::post(&format!("/api/v1/runs/{run_id}/decision"))
                .cookie(&cookie)
                .header("idempotency-key", "late")
                .json(&json!({"approved": true, "expected": expected_of(&detail)})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_at_every_stage_records_a_cancelled_failure() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    for stage in ["propose", "critique", "evaluate", "revise"] {
        let (start, gate) = gated_start(owner_of(&server, &cookie), visitor_retention(), stage);
        let snapshot = server.state.manager.start(start).await.unwrap();
        let run_id = snapshot.aggregate.run_id.to_string();
        assert_eq!(gate.wait_entered(), stage);
        let (status, body, _) = server
            .json(Req::post(&format!("/api/v1/runs/{run_id}/cancel")).cookie(&cookie))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{stage}: {body}");
        assert_eq!(body["cancelling"], true);
        gate.open();
        let detail = server.wait_resting(&cookie, &run_id).await;
        assert_eq!(detail["summary"]["state"], "failed", "{stage}: {detail}");
        assert_eq!(detail["summary"]["failure"]["code"], "cancelled", "{stage}");
        assert_eq!(
            detail["summary"]["failure"]["stage"], "cancelled",
            "{stage}"
        );
        assert!(
            detail["summary"]["failure"]["reason"].is_null(),
            "public view keeps no reason"
        );
        assert_eq!(
            detail["summary"]["failure"]["message"],
            "The run was cancelled by its owner."
        );
        // The call in progress completed and was recorded before the stop.
        let types: Vec<&str> = detail["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["payload"]["type"].as_str().unwrap())
            .collect();
        let recorded = match stage {
            "propose" => "proposal_submitted",
            "critique" => "challenge_submitted",
            "evaluate" => "evaluation_submitted",
            _ => "revision_submitted",
        };
        assert!(types.contains(&recorded), "{stage}: {types:?}");
        assert_eq!(*types.last().unwrap(), "run_failed");
        // The history replays and the receipt is sanitized.
        let response = server
            .send(Req::get(&format!("/api/v1/runs/{run_id}/receipt")).cookie(&cookie))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{stage}");
        let receipt: RunReceiptV2 = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(receipt.final_state, RunState::Failed);
        let failure = receipt.failure.unwrap();
        assert_eq!(failure.code.as_deref(), Some("cancelled"));
        assert_eq!(failure.reason, "The run was cancelled by its owner.");
        // A resting run cannot be cancelled.
        let (status, body, _) = server
            .json(Req::post(&format!("/api/v1/runs/{run_id}/cancel")).cookie(&cookie))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{stage}: {body}");
        assert_eq!(body["code"], "run_not_cancellable");
    }
    let (_, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["active_runs"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_decisions_record_exactly_one() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let (_, created) = server.create_guided(&cookie, "retry").await;
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    let detail = server.wait_resting(&cookie, &run_id).await;
    let expected = expected_of(&detail);
    let approve = server.json(
        Req::post(&format!("/api/v1/runs/{run_id}/decision"))
            .cookie(&cookie)
            .header("idempotency-key", "race-approve")
            .json(&json!({"approved": true, "expected": expected})),
    );
    let reject = server.json(
        Req::post(&format!("/api/v1/runs/{run_id}/decision"))
            .cookie(&cookie)
            .header("idempotency-key", "race-reject")
            .json(&json!({"approved": false, "expected": expected})),
    );
    let ((s1, b1, _), (s2, b2, _)) = tokio::join!(approve, reject);
    let statuses = [s1, s2];
    assert_eq!(
        statuses.iter().filter(|s| **s == StatusCode::OK).count(),
        1,
        "{b1} / {b2}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1,
        "{b1} / {b2}"
    );
    let (_, after) = server.detail(&cookie, &run_id).await;
    let decisions = after["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["payload"]["type"] == "human_decision_recorded")
        .count();
    assert_eq!(decisions, 1);
    let winner = if s1 == StatusCode::OK { &b1 } else { &b2 };
    assert_eq!(after["summary"]["state"], winner["run"]["state"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_session_sees_nothing_of_a_run() {
    let server = public_server(&[], BuildOptions::default());
    let a = server.new_session().await;
    let b = server.new_session().await;
    let (_, created) = server.create_guided(&a, "retry").await;
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    let detail = server.wait_resting(&a, &run_id).await;
    let expected = expected_of(&detail);

    let reads = [
        Req::get(&format!("/api/v1/runs/{run_id}")),
        Req::get(&format!("/api/v1/runs/{run_id}/events")),
        Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")),
        Req::get(&format!("/api/v1/runs/{run_id}/receipt")),
        Req::post(&format!("/api/v1/runs/{run_id}/decision"))
            .header("idempotency-key", "b1")
            .json(&json!({"approved": true, "expected": expected})),
        Req::post(&format!("/api/v1/runs/{run_id}/cancel")),
    ];
    for req in reads {
        let path = req.path.clone();
        let (status, body, headers) = server.json(req.cookie(&b)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert_eq!(body["code"], "not_found", "{path}");
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json"),
            "{path}: a 404 JSON, never a stream"
        );
    }
    let (status, list, _) = server.json(Req::get("/api/v1/runs").cookie(&b)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["runs"].as_array().unwrap().len(),
        0,
        "no listing of others' runs"
    );
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&a)).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 1);

    // Malformed ids are 404 before any I/O, even for the owner.
    for bad in [
        "run-../x",
        "..%2F..%2Fetc%2Fpasswd",
        "run-00000000-0000-4000-8000-00000000000",
        "RUN-00000000-0000-4000-8000-000000000000",
        "run-00000000-0000-1000-8000-000000000000",
        "run-00000000-0000-4000-8000-000000000000",
    ] {
        let (status, body, _) = server
            .json(Req::get(&format!("/api/v1/runs/{bad}")).cookie(&a))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{bad}: {body}");
    }
    // Nothing was created by any of the probes.
    let runs_dir = server.dir.path().join("store").join("runs");
    let files = std::fs::read_dir(&runs_dir).unwrap().count();
    assert_eq!(
        files, 5,
        "index, index lock, and the one run's log, sidecar and lock"
    );

    // The stream of the owner still works after all that.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&a))
        .await;
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_ndjson_is_published_only_for_scripted_chains() {
    const UPSTREAM: &str = "UPSTREAM-BODY-7f3e9c2a-never-public";
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    // A run declared live whose model call fails as the Meta adapter does:
    // the recorded reason quotes the upstream response.
    let start = unavailable_start(
        owner_of(&server, &cookie),
        visitor_retention(),
        RunOrigin::Public,
        &format!("meta api http 500: {UPSTREAM}"),
    );
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    let detail = server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    assert_eq!(detail["summary"]["failure"]["code"], "model_unavailable");
    assert_eq!(detail["summary"]["provenance"]["declared"]["model"], "meta");
    assert!(
        !detail.to_string().contains(UPSTREAM),
        "the detail strips the reason"
    );
    // The stored chain commits to the text, so it cannot be sanitized:
    // the public surface does not publish such a chain whole.
    let stored = std::fs::read_to_string(
        server
            .dir
            .path()
            .join("store")
            .join("runs")
            .join(format!("{run_id}.jsonl")),
    )
    .unwrap();
    assert!(stored.contains(UPSTREAM), "the record itself holds it");
    let (status, body, _) = server
        .json(Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")).cookie(&cookie))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "ndjson_unavailable");
    assert!(!body.to_string().contains(UPSTREAM));
    // The sanitized views stay available and clean.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/receipt")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!body_text(response).await.contains(UPSTREAM));
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let frames = read_sse(response.into_body(), 50).await;
    assert_eq!(frames.last().unwrap().json()["reason"], "terminal");
    for frame in &frames {
        assert!(!frame.data.contains(UPSTREAM), "{frame:?}");
    }
    // A stranger still gets the plain 404 before any of this.
    let other = server.new_session().await;
    let (status, body, _) = server
        .json(Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")).cookie(&other))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    // A guided run's chain, scripted end to end, is published verbatim.
    let (_, created) = server.create_guided(&cookie, "retry").await;
    let guided_id = created["run"]["run_id"].as_str().unwrap().to_string();
    server.wait_resting(&cookie, &guided_id).await;
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{guided_id}/events.ndjson")).cookie(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    verify_bytes(&body_bytes(response).await).expect("the published chain verifies");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_live_gate_is_asked_after_the_slot_and_refunded_when_the_start_fails() {
    let gate = TestGate::new(GateMode::Refuse);
    let server = public_server(
        &[("CONSENSUS_LIVE_CONCURRENCY", "1")],
        BuildOptions {
            live_gate: Some(gate.clone()),
            admission_wait: Duration::from_millis(100),
            ..BuildOptions::default()
        },
    );
    let cookie = server.new_session().await;
    let (_, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live_enabled"], true);
    let submit = |token: Option<&str>| {
        let req = Req::post("/api/v1/live-runs")
            .cookie(&cookie)
            .json(&json!({"preset_id": "retry-cap-3"}));
        match token {
            Some(token) => req.header("cf-turnstile-response", token),
            None => req,
        }
    };

    // Refused by the gate: its error is answered as is, nothing was
    // created, and the gate saw only the preset, the pseudonym and the
    // token.
    let (status, body, _) = server.json(submit(Some("tok-1"))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "too_many_runs");
    assert_eq!(gate.admitted(), 1);
    let (preset, pseudonym, token) = gate.last_request().unwrap();
    assert_eq!(preset, "retry-cap-3");
    assert!(pseudonym.starts_with("visitor:"), "{pseudonym}");
    assert!(!pseudonym.contains(&cookie), "never the session id");
    assert_eq!(token.as_deref(), Some("tok-1"));
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0);

    // The slot comes before the gate: with the one live slot taken, the
    // request is refused without the gate being asked (nothing charged).
    gate.set_mode(GateMode::Grant);
    let reservation = server.state.manager.reserve(RunKind::Live).await.unwrap();
    assert_eq!(reservation.kind(), RunKind::Live);
    let (status, body, _) = server.json(submit(None)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "too_many_runs");
    assert_eq!(gate.admitted(), 1, "the gate was not asked");
    drop(reservation);

    // Granted, but the start fails: the grant is refunded and no run is
    // left behind, not even an empty indexed one.
    gate.set_mode(GateMode::GrantFailingStart);
    let (status, body, _) = server.json(submit(None)).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(gate.admitted(), 2);
    assert_eq!(gate.refunded(), 1);
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0, "no orphan run");
    assert_eq!(server.state.manager.session_run_count().unwrap(), 0);
    let records = server.state.manager.store().index_records().unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|r| r.kind == IndexKind::Created)
            .count(),
        1,
        "{records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.kind == IndexKind::Purged)
            .count(),
        1,
        "{records:?}"
    );
    assert_eq!(server.state.manager.broadcasts().active_channels(), 0);
    let runs_dir = server.dir.path().join("store").join("runs");
    assert_eq!(
        std::fs::read_dir(&runs_dir).unwrap().count(),
        2,
        "the index and its lock only"
    );

    // Granted and started: 202, and the run deliberates like any other.
    gate.set_mode(GateMode::Grant);
    let (status, created, _) = server.json(submit(Some("tok-2"))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    assert_eq!(
        created["run"]["provenance"]["declared"]["preset"],
        "live:retry-cap-3"
    );
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    let detail = server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["recommendation"], "supported");
    assert_eq!(gate.admitted(), 3);
    assert_eq!(gate.refunded(), 1, "a started run is never refunded");
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panicking_model_fails_only_its_run() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let start = panicking_start(
        owner_of(&server, &cookie),
        visitor_retention(),
        RunOrigin::Public,
    );
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    let detail = server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    assert_eq!(detail["summary"]["failure"]["code"], "panic");
    assert_eq!(detail["summary"]["failure"]["stage"], "panic");
    assert!(detail["summary"]["failure"]["reason"].is_null());
    assert_eq!(
        detail["summary"]["failure"]["message"],
        "The run stopped because of an internal error."
    );
    assert!(
        !detail.to_string().contains("boom"),
        "panic text never reaches a view"
    );
    // The stream of the failed run replays and ends as terminal.
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")).cookie(&cookie))
        .await;
    let frames = read_sse(response.into_body(), 50).await;
    assert_eq!(frames.last().unwrap().json()["reason"], "terminal");
    // Everything else keeps working.
    let (status, _, _) = server.json(Req::get("/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, created) = server.create_guided(&cookie, "retry").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let other = server
        .wait_resting(&cookie, created["run"]["run_id"].as_str().unwrap())
        .await;
    assert_eq!(other["summary"]["recommendation"], "supported");
    let (_, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["active_runs"], 0);
    assert_eq!(ready["ready"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_cancels_active_runs_and_refuses_new_ones() {
    let server = public_server(
        &[],
        BuildOptions {
            shutdown_grace: Duration::from_secs(20),
            ..BuildOptions::default()
        },
    );
    let cookie = server.new_session().await;
    let (start, gate) = gated_start(owner_of(&server, &cookie), visitor_retention(), "critique");
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    gate.wait_entered();

    let manager = server.state.manager.clone();
    let shutdown = tokio::spawn(async move { manager.shutdown().await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server.state.manager.is_shutting_down());
    // The call in progress finishes; the run stops at its next checkpoint.
    gate.open();
    tokio::time::timeout(Duration::from_secs(15), shutdown)
        .await
        .expect("shutdown completes once the run reaches a checkpoint")
        .unwrap();
    let detail = server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    assert_eq!(detail["summary"]["failure"]["code"], "shutdown");
    assert_eq!(detail["summary"]["failure"]["stage"], "shutdown");

    let (status, body) = server.create_guided(&cookie, "retry").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "shutting_down");
    let (_, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], false);
    assert_eq!(ready["shutting_down"], true);
    // Reads still work during the drain.
    let (status, _) = server.detail(&cookie, &run_id).await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rates_streams_and_client_keys_are_enforced_per_slash_64() {
    let server = public_server(
        &[
            ("CONSENSUS_TRUST_PROXY_HEADERS", "true"),
            ("CONSENSUS_RATE_RUNS_PER_MIN", "2"),
            ("CONSENSUS_RATE_SESSION_PER_MIN", "3"),
            ("CONSENSUS_SSE_MAX_PER_CLIENT", "2"),
            ("CONSENSUS_RATE_TABLE_MAX", "16"),
            ("CONSENSUS_SESSION_MAX", "16"),
        ],
        BuildOptions::default(),
    );
    // The listener trusts the proxy: a request without its header is refused.
    let (status, body, _) = server.json(Req::post("/api/v1/session")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "client_unidentified");

    let name = "cf-connecting-ip";
    let value = "203.0.113.5";
    let (status, cookie) = server.new_session_from_header(value).await;
    assert_eq!(status, StatusCode::CREATED);
    let cookie = cookie.unwrap();

    // Two runs per minute per client.
    for _ in 0..2 {
        let (status, body, _) = server
            .json(
                Req::post("/api/v1/runs")
                    .cookie(&cookie)
                    .header(name, value)
                    .json(&json!({"scenario": "retry"})),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let (status, body, headers) = server
        .json(
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .header(name, value)
                .json(&json!({"scenario": "retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "rate_limited");
    assert!(headers.get(header::RETRY_AFTER).is_some());
    assert!(body["retry_after_secs"].as_u64().unwrap() >= 1);
    // Another client is unaffected.
    let (_, other) = server.new_session_from_header("198.51.100.7").await;
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/runs")
                .cookie(other.as_deref().unwrap())
                .header(name, "198.51.100.7")
                .json(&json!({"scenario": "retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");

    // One IPv6 /64 is one client: three sessions, then 429 from a sibling
    // address, while another /64 is fine.
    for host in ["2001:db8:1:2::1", "2001:db8:1:2::2", "2001:db8:1:2:aaaa::3"] {
        let (status, _) = server.new_session_from_header(host).await;
        assert_eq!(status, StatusCode::CREATED, "{host}");
    }
    let (status, _) = server
        .new_session_from_header("2001:db8:1:2:ffff:ffff:ffff:ffff")
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let (status, _) = server.new_session_from_header("2001:db8:1:3::1").await;
    assert_eq!(status, StatusCode::CREATED);

    // Streams per client: two held open, the third is refused, a closed
    // one frees its slot.
    let (_, list, _) = server
        .json(Req::get("/api/v1/runs").cookie(&cookie).header(name, value))
        .await;
    let run_id = list["runs"][0]["run_id"].as_str().unwrap().to_string();
    let stream = || {
        Req::get(&format!("/api/v1/runs/{run_id}/events"))
            .cookie(&cookie)
            .header(name, value)
    };
    let first = server.send(stream()).await;
    assert_eq!(first.status(), StatusCode::OK);
    let second = server.send(stream()).await;
    assert_eq!(second.status(), StatusCode::OK);
    let (status, body, headers) = server.json(stream()).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "too_many_streams");
    assert!(headers.get(header::RETRY_AFTER).is_some());
    drop(first);
    let third = server.send(stream()).await;
    assert_eq!(third.status(), StatusCode::OK);
    drop(second);
    drop(third);
    assert_eq!(server.state.stream_caps.open_streams(), 0);

    // Tables stay bounded under churn from many clients.
    for i in 0..64u8 {
        let host = format!("192.0.2.{i}");
        let (status, _) = server.new_session_from_header(&host).await;
        assert_eq!(status, StatusCode::CREATED, "{host}");
    }
    assert!(
        server.state.sessions.len() <= 16,
        "{}",
        server.state.sessions.len()
    );
    assert!(
        server.state.rates.session.len() <= 16,
        "{}",
        server.state.rates.session.len()
    );
    assert!(server.state.sessions.evictions() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_guided_ring_frees_the_oldest_resting_run_and_refuses_only_when_nothing_can_be_freed() {
    let server = public_server(&[("CONSENSUS_GUIDED_RING", "2")], BuildOptions::default());
    let cookie = server.new_session().await;
    let mut ids = Vec::new();
    for _ in 0..2 {
        let (status, body) = server.create_guided(&cookie, "abstain").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let id = body["run"]["run_id"].as_str().unwrap().to_string();
        server.wait_resting(&cookie, &id).await;
        server.wait_released(&id).await;
        ids.push(id);
    }
    assert_eq!(server.state.manager.session_run_count().unwrap(), 2);

    // Full: the next creation is admitted and the oldest run makes room.
    let (status, body) = server.create_guided(&cookie, "abstain").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let third = body["run"]["run_id"].as_str().unwrap().to_string();
    server.wait_resting(&cookie, &third).await;
    let (status, body) = server.detail(&cookie, &ids[0]).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the oldest run is gone: {body}"
    );
    assert_eq!(body["code"], "not_found");
    let (status, _) = server.detail(&cookie, &ids[1]).await;
    assert_eq!(status, StatusCode::OK);
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    let listed: Vec<&str> = list["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, [third.as_str(), ids[1].as_str()]);
    assert_eq!(server.state.manager.session_run_count().unwrap(), 2);
    // The purge is on the record, like retention's.
    let records = server.state.manager.store().index_records().unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.kind == IndexKind::Purged && r.run_id.as_str() == ids[0]),
        "{records:?}"
    );
    let (_, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["limits"]["guided_ring"], 2);

    // Only when nothing can be freed is a creation refused: a ring of one
    // whose single run is still deliberating.
    let server = public_server(&[("CONSENSUS_GUIDED_RING", "1")], BuildOptions::default());
    let cookie = server.new_session().await;
    let (start, gate) = gated_start(owner_of(&server, &cookie), visitor_retention(), "critique");
    let snapshot = server.state.manager.start(start).await.unwrap();
    let run_id = snapshot.aggregate.run_id.to_string();
    gate.wait_entered();
    let (status, body) = server.create_guided(&cookie, "abstain").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "storage_full");
    assert!(body["retry_after_secs"].as_u64().unwrap() >= 1);
    let (status, _) = server.detail(&cookie, &run_id).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the deliberating run was not touched"
    );
    // Once it rests, the ring turns.
    gate.open();
    server.wait_resting(&cookie, &run_id).await;
    server.wait_released(&run_id).await;
    let (status, body) = server.create_guided(&cookie, "abstain").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, _) = server.detail(&cookie, &run_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_creation_with_the_same_key_returns_the_same_run() {
    // One run per minute: the retry must be answered before admission, or
    // it would be rate limited (and counted twice).
    let server = public_server(
        &[("CONSENSUS_RATE_RUNS_PER_MIN", "1")],
        BuildOptions::default(),
    );
    let cookie = server.new_session().await;
    let submit = || {
        Req::post("/api/v1/runs")
            .cookie(&cookie)
            .header("idempotency-key", "submit-1")
            .json(&json!({"scenario": "retry"}))
    };
    let (status, first, _) = server.json(submit()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let run_id = first["run"]["run_id"].as_str().unwrap().to_string();
    let (status, again, headers) = server.json(submit()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["run"]["run_id"], run_id);
    assert_eq!(headers[header::LOCATION], format!("/api/v1/runs/{run_id}"));
    let (_, list, _) = server.json(Req::get("/api/v1/runs").cookie(&cookie)).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 1, "one run");
    // The same key with another body is refused.
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .header("idempotency-key", "submit-1")
                .json(&json!({"scenario": "abstain"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "idempotency_key_reused");
    // A malformed key is refused before anything else.
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/runs")
                .cookie(&cookie)
                .header("idempotency-key", "bad key!")
                .json(&json!({"scenario": "retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "idempotency_key_required");
    // Keys are scoped to the owner: another session's same key is its own.
    let (status, other) = server.new_session_from("203.0.113.9:4000").await;
    assert_eq!(status, StatusCode::CREATED);
    let other = other.unwrap();
    let (status, theirs, _) = server
        .json(
            Req::post("/api/v1/runs")
                .cookie(&other)
                .peer("203.0.113.9:4000")
                .header("idempotency-key", "submit-1")
                .json(&json!({"scenario": "retry"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{theirs}");
    assert_ne!(theirs["run"]["run_id"], run_id);
    assert_eq!(server.state.creation_keys.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_damaged_stored_run_is_reported_never_served_as_if_whole() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let (_, created) = server.create_guided(&cookie, "retry").await;
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    server.wait_resting(&cookie, &run_id).await;

    // Flip one byte inside a middle record of the stored log: its hash no
    // longer matches, so the chain fails verification.
    let path = server
        .dir
        .path()
        .join("store")
        .join("runs")
        .join(format!("{run_id}.jsonl"));
    let bytes = std::fs::read(&path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let target = text
        .find("Raise the payment client retry limit")
        .expect("proposal content");
    let mut tampered = bytes;
    tampered[target] = b'r';
    std::fs::write(&path, &tampered).unwrap();

    for route in [
        format!("/api/v1/runs/{run_id}"),
        format!("/api/v1/runs/{run_id}/events"),
        format!("/api/v1/runs/{run_id}/events.ndjson"),
        format!("/api/v1/runs/{run_id}/receipt"),
        "/api/v1/runs".to_string(),
    ] {
        let (status, body, headers) = server.json(Req::get(&route).cookie(&cookie)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{route}: {body}");
        assert_eq!(body["code"], "internal", "{route}");
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json"),
            "{route}: an error, never a stream of the damaged history"
        );
        assert!(
            !body["message"].as_str().unwrap().contains(".jsonl"),
            "no paths"
        );
    }
    // The file was reported, never repaired.
    assert_eq!(std::fs::read(&path).unwrap(), tampered);

    // A stranger's /events never reads the log: the damage is not even
    // noticed, the answer is the 404 of any run not theirs, no stream slot
    // is taken, and the probe counted against the stranger's client key.
    let streams_before = server.state.rates.sse.len();
    let (status, other) = server.new_session_from("198.51.100.9:5000").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body, headers) = server
        .json(
            Req::get(&format!("/api/v1/runs/{run_id}/events"))
                .cookie(other.as_deref().unwrap())
                .peer("198.51.100.9:5000"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "not_found");
    assert!(headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("application/json"));
    assert_eq!(server.state.stream_caps.open_streams(), 0);
    assert_eq!(server.state.rates.sse.len(), streams_before + 1);
}
