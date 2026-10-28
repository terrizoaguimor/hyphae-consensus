// SPDX-License-Identifier: Apache-2.0
//! The public live mode over HTTP (plan B9, ADR-0007): the portal-wide
//! geographic block, every admission refusal in ABUSE-9 order, the judge
//! lane, the ledger's refusals and, end to end against the fake Chat
//! Completions server, a metered live run: one reservation, one draw and
//! one settlement per HTTP attempt, a release, the budget published by
//! `/capabilities`, and the key appearing nowhere. Then one solved
//! Turnstile challenge admitting one run however the token is replayed
//! (another session, concurrent duplicates), and what the operator sees
//! before anyone submits: the kill switch at once, no budget at all for a
//! ledger that cannot be read.

mod common;
mod support;

use axum::http::{header, StatusCode};
use chrono::{DateTime, Utc};
use common::live::{
    live_config, live_server, FactoryMode, HeldSiteverify, LiveHarness, LiveOptions,
    ScriptedEvidenceFactory, SiteverifyMode, HELD_TOKEN_PREFIX, JUDGE_CODE,
    LIVE_MAX_COMPLETION_TOKENS, SECOND_JUDGE_CODE, VISITOR_IP,
};
use common::{body_text, public_server, read_sse, Req, COUNTRY_HEADER};
use consensus_api::api::error::ErrorCode;
use consensus_api::limits::ClientKey;
use consensus_api::live::budget::{Lane, LedgerClock};
use consensus_api::live::config::{defaults as live_defaults, vars};
use consensus_api::live::gate::{refusal_message, SITEVERIFY_MAX_IN_FLIGHT};
use consensus_api::live::money::Usd;
use consensus_api::live::snapshot_factory::SnapshotFactory;
use consensus_api::runs::RunKind;
use consensus_api::server::{build_live_gate, BuildOptions};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{config as meta_config, envelope, serve, Reply, TEST_KEY};

/// The reference preset every test runs.
const PRESET: &str = "R01-reference";
/// A Turnstile token; the scripted Siteverify accepts any.
const TOKEN: &str = "tok-live-1";

/// The drafts the fake Chat Completions server answers, in call order:
/// one claim the reference records corroborate, an empty critique, and
/// an evaluation keyed by the id the adapter mints for that claim
/// (`prop-1`, then `claim-2`). Supported in one round: three calls.
const DRAFT_PROPOSAL: &str = r#"{"summary":"Keep retries capped at 3 attempts with jitter","plan_steps":["add jitter to the payment client backoff"],"claims":[{"statement":"Payment timeouts correlate with retry bursts above 5 attempts"}],"assumptions":[],"uncertainties":[]}"#;
const DRAFT_CHALLENGE: &str = r#"{"items":[]}"#;
const DRAFT_EVALUATION: &str = r#"{"assessments":{"claim-2":{"corroborated":true,"relevant_evidence":["ev-fact"],"contradicting_evidence":[],"note":"ev-fact states the correlation"}},"item_verdicts":{},"cleared_contradictions":[],"abstain":false,"abstain_reason":null}"#;

/// The three answers of one supported run, the first delayed so a stream
/// opened right after the 202 sees live frames.
fn one_run(first_delay: Duration) -> Vec<Reply> {
    vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)).delayed(first_delay),
        Reply::ok(envelope(DRAFT_CHALLENGE)),
        Reply::ok(envelope(DRAFT_EVALUATION)),
    ]
}

fn live_body(attest: bool, judge_code: Option<&str>) -> Value {
    let mut body = json!({"preset_id": PRESET, "attest_adult": attest});
    if let Some(code) = judge_code {
        body["judge_code"] = json!(code);
    }
    body
}

/// A visitor's request on a listener that trusts the proxy headers.
fn visitor(req: Req, cookie: &str) -> Req {
    req.cookie(cookie).header("cf-connecting-ip", VISITOR_IP)
}

fn submit(cookie: &str, body: &Value, token: Option<&str>) -> Req {
    let req = visitor(Req::post("/api/v1/live-runs"), cookie).json(body);
    match token {
        Some(token) => req.header("cf-turnstile-response", token),
        None => req,
    }
}

fn run_id_of(created: &Value) -> String {
    created["run"]["run_id"].as_str().unwrap().to_string()
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------
// Disabled, and the portal-wide geographic block
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_gate_live_mode_is_disabled_and_nothing_live_is_published() {
    let server = public_server(&[], BuildOptions::default());
    let cookie = server.new_session().await;
    let (status, body, _) = server
        .json(
            Req::post("/api/v1/live-runs")
                .cookie(&cookie)
                .header("cf-turnstile-response", TOKEN)
                .json(&live_body(true, Some(JUDGE_CODE))),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_disabled");
    let (_, caps, _) = server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live_enabled"], false);
    assert!(caps.get("live").is_none(), "{caps}");
    let (_, ready, _) = server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], true);
    assert!(ready.get("live").is_none(), "{ready}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_portal_block_covers_every_route_and_fails_closed_without_a_country() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    for path in [
        "/",
        "/api/v1/capabilities",
        "/research/MANIFEST.json",
        "/en/",
    ] {
        let (status, body, _) = h
            .server
            .json(Req::get(path).header(COUNTRY_HEADER, "CU"))
            .await;
        assert_eq!(
            status,
            StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
            "{path}: {body}"
        );
        assert_eq!(body["code"], "geo_blocked_portal", "{path}");
        assert!(
            !body.to_string().contains("CU"),
            "the country is never echoed"
        );
        // Behind cloudflared every request carries the header: one without
        // it did not come through the edge, and is refused.
        let (status, body, _) = h.server.json(Req::get(path).without(COUNTRY_HEADER)).await;
        assert_eq!(
            status,
            StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
            "{path}: {body}"
        );
        assert_eq!(body["code"], "geo_unknown", "{path}");
        // Ukraine is on the list whole; the live-only country and an
        // unknown placeholder are fine on the portal.
        let (status, _, _) = h
            .server
            .json(Req::get(path).header(COUNTRY_HEADER, "ua"))
            .await;
        assert_eq!(status, StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS, "{path}");
        for ok in ["KR", "XX", "US"] {
            let response = h
                .server
                .send(Req::get(path).header(COUNTRY_HEADER, ok))
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{path} from {ok}");
        }
    }
    // A POST is blocked before any of its own checks run.
    let (status, body, _) = h
        .server
        .json(Req::post("/api/v1/session").header(COUNTRY_HEADER, "IR"))
        .await;
    assert_eq!(status, StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS, "{body}");

    // A development bind trusts no proxy header: nothing is blocked, and
    // a missing header means nothing.
    let dev = public_server(&[], BuildOptions::default());
    for req in [
        Req::get("/").header(COUNTRY_HEADER, "CU"),
        Req::get("/api/v1/capabilities").header(COUNTRY_HEADER, "CU"),
        Req::get("/api/v1/capabilities").without(COUNTRY_HEADER),
    ] {
        let response = dev.send(req).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn korea_blocks_live_mode_only_and_an_unknown_country_blocks_it_too() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    // Guided runs stay open from South Korea...
    let (status, created, _) = h
        .server
        .json(
            visitor(Req::post("/api/v1/runs"), &cookie)
                .header(COUNTRY_HEADER, "KR")
                .json(&json!({"scenario": "abstain"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    // ...live runs do not, and the geo check comes before the attestation
    // and Turnstile (nothing else is asked, nothing reserved).
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(false, None), None).header(COUNTRY_HEADER, "KR"))
        .await;
    assert_eq!(status, StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS, "{body}");
    assert_eq!(body["code"], "geo_blocked_live");
    assert!(h.siteverify.calls().is_empty());
    assert!(h.ledger_records("reservation").is_empty());
    // An unknown placeholder passes the portal but never the live surface.
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)).header(COUNTRY_HEADER, "XX"))
        .await;
    assert_eq!(status, StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS, "{body}");
    assert_eq!(body["code"], "geo_blocked_live");

    // On a development bind the country is never known (the header is not
    // trusted), so the live surface refuses: an owner key is at stake.
    let meta = serve(one_run(Duration::ZERO));
    let dev = live_server(
        LiveOptions {
            trust_proxy: false,
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let cookie = dev.server.new_session().await;
    let (status, body, _) = dev
        .server
        .json(
            Req::post("/api/v1/live-runs")
                .cookie(&cookie)
                .header(COUNTRY_HEADER, "US")
                .header("cf-turnstile-response", TOKEN)
                .json(&live_body(true, None)),
        )
        .await;
    assert_eq!(status, StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS, "{body}");
    assert_eq!(body["code"], "geo_blocked_live");
    assert!(
        dev.siteverify.calls().is_empty(),
        "refused before Turnstile"
    );
}

// ---------------------------------------------------------------------
// Admission refusals, in order
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_refuses_in_order_and_charges_nothing() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;

    // (a) An unknown preset, before anything else is looked at.
    let (status, body, _) = h
        .server
        .json(submit(
            &cookie,
            &json!({"preset_id": "retry-cap-3", "attest_adult": false}),
            None,
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "preset_unknown");
    // (c) The attestation, before Turnstile is asked.
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(false, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "attestation_required");
    assert!(h.siteverify.calls().is_empty(), "Turnstile was not asked");
    // (d) No token, then a token Cloudflare refuses, then an outage.
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), None))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "turnstile_required");
    h.set_siteverify(SiteverifyMode::Fail);
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "turnstile_failed");
    assert!(!body.to_string().contains("invalid-input-response"));
    h.set_siteverify(SiteverifyMode::Outage);
    let (status, body, headers) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "turnstile_unavailable");
    assert!(headers.get(header::RETRY_AFTER).is_some());
    let calls = h.siteverify.calls();
    assert_eq!(calls.len(), 2, "one Siteverify call per token presented");
    assert!(calls
        .iter()
        .all(|c| c.token == TOKEN && c.remote_ip.is_none()));
    assert_ne!(
        calls[0].idempotency_key, calls[1].idempotency_key,
        "without an Idempotency-Key every request gets a fresh one"
    );
    // A judge code outside its bounds is refused by the route itself.
    let (status, body, _) = h
        .server
        .json(submit(
            &cookie,
            &live_body(true, Some(&"x".repeat(257))),
            Some(TOKEN),
        ))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "invalid_request");
    // Nothing was reserved, no run exists, and the live slot was given
    // back every time: the next admitted request is not "too many runs".
    assert!(h.ledger_records("reservation").is_empty());
    let (_, list, _) = h
        .server
        .json(visitor(Req::get("/api/v1/runs"), &cookie))
        .await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0);
    assert_eq!(meta.hits(), 0, "the model was never called");
    h.set_siteverify(SiteverifyMode::Pass);
    let (status, created, _) = h
        .server
        .json(
            submit(&cookie, &live_body(true, None), Some(TOKEN))
                .header("idempotency-key", "submit-live-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let calls = h.siteverify.calls();
    // Siteverify never sees the request's key: Cloudflare's idempotency is
    // global, so the header would let one token start several runs. Every
    // call gets its own UUID instead.
    let siteverify_key = &calls.last().unwrap().idempotency_key;
    assert_ne!(siteverify_key, "submit-live-1");
    assert!(
        uuid::Uuid::parse_str(siteverify_key).is_ok(),
        "a fresh UUID per verification: {siteverify_key}"
    );
    let mut keys: Vec<&str> = calls.iter().map(|c| c.idempotency_key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), calls.len(), "no key is ever reused: {calls:?}");
    // A retry with the same key is answered from the stored run: Siteverify
    // is not asked again and nothing more is reserved.
    let (status, again, _) = h
        .server
        .json(
            submit(&cookie, &live_body(true, None), Some(TOKEN))
                .header("idempotency-key", "submit-live-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["run"]["run_id"], created["run"]["run_id"]);
    assert_eq!(h.siteverify.calls().len(), calls.len());
    assert_eq!(h.ledger_records("reservation").len(), 1);
    // The same key with another body: refused before Siteverify.
    let (status, body, _) = h
        .server
        .json(
            submit(&cookie, &live_body(true, Some("other")), Some(TOKEN))
                .header("idempotency-key", "submit-live-1"),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "idempotency_key_reused");
    assert_eq!(h.siteverify.calls().len(), calls.len());
    h.server.wait_resting(&cookie, &run_id_of(&created)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_known_judge_code_takes_the_judge_lane_and_an_unknown_one_the_public_lane() {
    let mut replies = one_run(Duration::ZERO);
    replies.extend(one_run(Duration::ZERO));
    let meta = serve(replies);
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    let (status, judge, _) = h
        .server
        .json(submit(
            &cookie,
            &live_body(true, Some(JUDGE_CODE)),
            Some(TOKEN),
        ))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{judge}");
    let judge_id = run_id_of(&judge);
    h.server.wait_resting(&cookie, &judge_id).await;
    h.server.wait_released(&judge_id).await;
    let (status, public, _) = h
        .server
        .json(submit(
            &cookie,
            &live_body(true, Some("not-a-code-anyone-knows")),
            Some(TOKEN),
        ))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{public}");
    let public_id = run_id_of(&public);
    h.server.wait_resting(&cookie, &public_id).await;
    // The ledger knows the lanes; the responses have the same shape and
    // say nothing about codes or lanes.
    let reservations = h.ledger_records("reservation");
    assert_eq!(reservations.len(), 2);
    assert_eq!(reservations[0]["lane"], "judge");
    assert_eq!(reservations[1]["lane"], "public");
    assert_eq!(sorted_keys(&judge), sorted_keys(&public));
    assert_eq!(sorted_keys(&judge["run"]), sorted_keys(&public["run"]));
    for body in [&judge, &public] {
        let text = body.to_string().to_ascii_lowercase();
        assert!(!text.contains("judge") && !text.contains("lane"), "{text}");
        assert!(!text.contains("not-a-code"), "{text}");
    }
    // The code itself is nowhere: not in the store, not in the ledger.
    let everything = format!("{}{}", h.store_text(), h.ledger_text());
    assert!(!everything.contains(JUDGE_CODE));
    assert!(!everything.contains("not-a-code-anyone-knows"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ledger_refusals_answer_with_their_codes_and_the_kill_switch_trips() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    // Four $2.28 reservations ($9.12) spend the $10 day; the visitor's
    // would be the fifth.
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(
            h.ledger
                .reserve(consensus_api::live::budget::Lane::Public)
                .unwrap(),
        );
    }
    let (status, body, headers) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "live_budget_daily");
    assert!(headers.get(header::RETRY_AFTER).is_some());
    assert!(body["retry_after_secs"].as_u64().unwrap() >= 1);
    let text = body.to_string();
    assert!(
        !text.contains("1.88") && !text.contains("2.28") && !text.contains('$'),
        "no amounts: {text}"
    );
    assert_eq!(h.ledger_records("reservation").len(), 4, "nothing written");
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    // With the harness's 12k output bound and the default 32k input cap a
    // run's worst case is (70,000 + 12,000 + 32,000) × $20/M = $2.28: four
    // leave $0.88, which holds no fifth.
    assert_eq!(caps["live"]["budget"]["daily_remaining"], "0.880000");
    assert_eq!(caps["live"]["budget"]["tripped"], false);
    for r in held {
        h.ledger.release(&r).unwrap();
    }

    // The kill switch: the next admission trips the ledger and is refused;
    // capabilities and readiness say so without any detail.
    std::fs::write(h.kill_switch(), b"stop\n").unwrap();
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_tripped");
    assert!(!body.to_string().contains("kill"), "{body}");
    assert_eq!(h.ledger_records("trip").len(), 1);
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["tripped"], true);
    assert_eq!(caps["live"]["budget"]["trip_code"], "kill_switch");
    let (status, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    assert_eq!(ready["ready"], false);
    assert_eq!(ready["live"]["ledger_ok"], true);
    assert_eq!(ready["live"]["tripped"], true);
    assert_eq!(ready["live"]["trip_code"], "kill_switch");
    assert_eq!(ready["live"]["evidence_ok"], true);
    assert_eq!(meta.hits(), 0);
}

// ---------------------------------------------------------------------
// A metered live run, end to end
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_run_is_metered_streamed_and_published_without_the_key() {
    let meta = serve(one_run(Duration::from_millis(400)));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let (status, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(status, StatusCode::OK, "{caps}");
    assert_eq!(caps["live_enabled"], true);
    let live = &caps["live"];
    assert_eq!(live["requires_turnstile"], true);
    assert_eq!(live["requires_adult_attestation"], true);
    let presets = live["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 5);
    assert_eq!(presets[0]["id"], PRESET);
    assert_eq!(presets[0]["title"], "Fix payment client timeouts");
    assert_eq!(presets[0]["project"], "demo-repo");
    assert!(presets
        .iter()
        .all(|p| p["model"] == "live_meta" && p["evidence"] == "simulated"));
    assert_eq!(live["budget"]["daily_remaining"], "10.000000");
    assert_eq!(live["budget"]["daily_available_now"], "10.000000");
    assert_eq!(live["budget"]["lifetime_remaining"], "60.000000");
    assert_eq!(live["budget"]["tripped"], false);
    assert!(live["budget"].get("trip_code").is_none());
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["live"]["ledger_ok"], true);
    assert_eq!(ready["live"]["evidence_ok"], true);

    // The session whose pseudonym Meta must see as `safety_identifier`.
    let (status, session, headers) = h
        .server
        .json(Req::post("/api/v1/session").header("cf-connecting-ip", VISITOR_IP))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let pseudonym = session["pseudonym"].as_str().unwrap().to_string();
    let cookie = common::cookie_value(&headers).expect("session cookie");
    let (status, created, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    assert_eq!(created["run"]["title"], "Fix payment client timeouts");
    assert_eq!(
        created["run"]["provenance"]["declared"],
        json!({"model": "meta", "evidence": "simulated", "origin": "public", "preset": PRESET})
    );
    assert_eq!(created["run"]["provenance"]["model"], "undetermined");

    // The stream opened right after the 202 follows the deliberation live.
    let response = h
        .server
        .send(visitor(
            Req::get(&format!("/api/v1/runs/{run_id}/events")),
            &cookie,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let frames = read_sse(response.into_body(), 200).await;
    let end = frames.last().unwrap();
    assert_eq!(end.event, "stream_end", "{frames:?}");
    assert_eq!(end.json()["reason"], "awaiting_human_approval");
    let deliveries: Vec<String> = frames
        .iter()
        .filter(|f| f.event == "consensus")
        .map(|f| f.json()["delivery"].as_str().unwrap().to_string())
        .collect();
    assert!(deliveries.iter().any(|d| d == "live"), "{deliveries:?}");
    for frame in frames.iter().filter(|f| f.event == "consensus") {
        assert_eq!(
            frame.json()["summary"]["provenance"]["declared"]["preset"],
            PRESET
        );
    }

    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(
        detail["summary"]["state"], "awaiting_human_approval",
        "{detail}"
    );
    assert_eq!(detail["summary"]["recommendation"], "supported");
    let provenance = &detail["summary"]["provenance"];
    assert_eq!(provenance["model"], "live_meta");
    assert_eq!(provenance["evidence"], "simulated");
    assert_eq!(provenance["declared"]["model"], "meta");
    assert_eq!(provenance["declared"]["evidence"], "simulated");
    assert_eq!(provenance["declared"]["origin"], "public");
    assert_eq!(provenance["declared"]["preset"], PRESET);
    assert!(
        provenance["flags"].as_array().is_none_or(|f| f.is_empty()),
        "{provenance}"
    );
    assert_eq!(provenance["meta_request_ids"].as_array().unwrap().len(), 3);
    assert_eq!(detail["model_invocations"].as_array().unwrap().len(), 3);
    assert!(detail["model_invocations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["provider"] == "meta" && i["scripted"] == false));

    // The fake API saw three requests, each with the session pseudonym and
    // the live output bound, never the default one.
    assert_eq!(meta.hits(), 3);
    for raw in meta.requests() {
        let body = support::body_of(&raw);
        assert_eq!(body["safety_identifier"], pseudonym, "{body}");
        assert_eq!(
            body["max_completion_tokens"],
            json!(LIVE_MAX_COMPLETION_TOKENS),
            "{body}"
        );
    }

    // The ledger: one reservation, one draw and one settlement per call,
    // each settled with the fake usage (100 in, 50 out), then a release.
    let releases = h.wait_releases(1).await;
    let reservations = h.ledger_records("reservation");
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0]["lane"], "public");
    // (70,000 + 12,000 + 32,000) tokens × $20/M with the 12k output bound
    // and the default input cap.
    assert_eq!(reservations[0]["worst_case_usd"], "2.280000");
    assert_eq!(releases[0]["reservation"], reservations[0]["id"]);
    let draws = h.ledger_records("draw");
    assert_eq!(draws.len(), 3);
    assert_eq!(
        draws
            .iter()
            .map(|d| d["attempt"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(draws
        .iter()
        .all(|d| d["max_completion_tokens"] == json!(LIVE_MAX_COMPLETION_TOKENS)));
    let settlements = h.ledger_records("settlement");
    assert_eq!(settlements.len(), 3);
    for s in &settlements {
        assert_eq!(s["outcome"]["kind"], "usage", "{s}");
        assert_eq!(s["outcome"]["input"], 100);
        assert_eq!(s["outcome"]["completion"], 50);
        // 100 × $4/M + 50 × $20/M.
        assert_eq!(s["charged_usd"], "0.001400");
    }
    let snapshot = h.ledger.snapshot().unwrap();
    assert_eq!(snapshot.unsettled_reservations, 0);
    assert_eq!(snapshot.lifetime_spent.to_fixed(), "0.004200");

    // What the public sees of the budget moved by exactly what was
    // settled, and nothing about the judge lane, a path or an HTTP status.
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["daily_remaining"], "9.995800");
    assert_eq!(caps["live"]["budget"]["lifetime_remaining"], "59.995800");
    let live = caps["live"].to_string().to_ascii_lowercase();
    assert!(!live.contains("judge"), "{live}");
    // The budget object: amounts and a coarse code, never an upstream
    // status or a path (a preset title may well mention HTTP).
    let budget = caps["live"]["budget"].to_string().to_ascii_lowercase();
    assert!(!budget.contains("http"), "{budget}");
    assert!(
        !budget.contains('/') && !budget.contains('\\'),
        "no paths: {budget}"
    );
    assert_eq!(
        sorted_keys(&caps["live"]["budget"]),
        [
            "daily_available_now",
            "daily_remaining",
            "lifetime_remaining",
            "tripped"
        ]
    );

    // The key appears nowhere: responses, frames, receipt, store, ledger.
    let mut outputs: Vec<String> = vec![created.to_string(), detail.to_string(), caps.to_string()];
    outputs.extend(frames.iter().map(|f| f.data.clone()));
    let response = h
        .server
        .send(visitor(
            Req::get(&format!("/api/v1/runs/{run_id}/receipt")),
            &cookie,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    outputs.push(body_text(response).await);
    let (status, body, _) = h
        .server
        .json(visitor(
            Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")),
            &cookie,
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a live chain is not published verbatim: {body}"
    );
    assert_eq!(body["code"], "ndjson_unavailable");
    outputs.push(h.store_text());
    outputs.push(h.ledger_text());
    for output in &outputs {
        assert!(!output.contains(TEST_KEY), "the key leaked: {output}");
        assert!(!output.contains("test-key-not-a-secret"), "{output}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_that_fails_after_admission_releases_the_reservation() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(
        LiveOptions {
            factory: FactoryMode::FailingStart,
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let cookie = h.session().await;
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(h.siteverify.calls().len(), 1, "the token was spent");
    let releases = h.wait_releases(1).await;
    let reservations = h.ledger_records("reservation");
    assert_eq!(reservations.len(), 1);
    assert_eq!(releases[0]["reservation"], reservations[0]["id"]);
    assert!(h.ledger_records("draw").is_empty(), "nothing was drawn");
    assert_eq!(meta.hits(), 0);
    let snapshot = h.ledger.snapshot().unwrap();
    assert_eq!(snapshot.unsettled_reservations, 0);
    assert_eq!(snapshot.lifetime_spent.to_fixed(), "0.000000");
    assert_eq!(snapshot.active_public, 0);
    let (_, list, _) = h
        .server
        .json(visitor(Req::get("/api/v1/runs"), &cookie))
        .await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0, "no orphan run");
    // The lane is free again.
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["daily_remaining"], "10.000000");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panicking_deliberation_still_releases_its_reservation() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(
        LiveOptions {
            factory: FactoryMode::Panicking,
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let cookie = h.session().await;
    let (status, created, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    assert_eq!(detail["summary"]["failure"]["code"], "panic");
    assert_eq!(detail["summary"]["failure"]["stage"], "panic");
    assert!(!detail.to_string().contains("boom"));
    // The manager caught the panic and dropped the deliberation: the drop
    // released the reservation.
    let releases = h.wait_releases(1).await;
    assert_eq!(h.ledger_records("reservation").len(), 1);
    assert_eq!(releases.len(), 1);
    assert!(h.ledger_records("draw").is_empty());
    let snapshot = h.ledger.snapshot().unwrap();
    assert_eq!(snapshot.unsettled_reservations, 0);
    assert_eq!(snapshot.active_public, 0);
    assert_eq!(meta.hits(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draw_refused_mid_run_fails_the_run_at_its_stage_and_charges_once() {
    let meta = serve(one_run(Duration::from_millis(600)));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    let (status, created, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    // The first call is in flight (its draw is made) when the operator
    // flips the kill switch: the next draw trips the ledger and is refused.
    for _ in 0..300 {
        if meta.hits() >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(meta.hits(), 1);
    std::fs::write(h.kill_switch(), b"stop\n").unwrap();
    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    assert_eq!(detail["summary"]["failure"]["code"], "budget_refused");
    assert_eq!(detail["summary"]["failure"]["stage"], "critique");
    assert!(
        detail["summary"]["failure"]["reason"].is_null(),
        "public view keeps no reason"
    );
    assert_eq!(detail["model_invocations"].as_array().unwrap().len(), 1);
    assert_eq!(meta.hits(), 1, "nothing was sent after the refusal");
    let releases = h.wait_releases(1).await;
    assert_eq!(releases.len(), 1);
    assert_eq!(h.ledger_records("reservation").len(), 1);
    assert_eq!(
        h.ledger_records("draw").len(),
        1,
        "the refused draw wrote nothing"
    );
    let settlements = h.ledger_records("settlement");
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0]["outcome"]["kind"], "usage");
    assert_eq!(settlements[0]["charged_usd"], "0.001400");
    let trips = h.ledger_records("trip");
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0]["code"], "kill_switch");
    let snapshot = h.ledger.snapshot().unwrap();
    assert_eq!(
        snapshot.lifetime_spent.to_fixed(),
        "0.001400",
        "charged once"
    );
    assert_eq!(snapshot.unsettled_reservations, 0);
    assert!(snapshot.tripped);
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["tripped"], true);
    assert_eq!(caps["live"]["budget"]["trip_code"], "kill_switch");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_dropped_after_the_request_draws_twice_and_settles_both() {
    let mut replies = vec![Reply::hang_up()];
    replies.extend(one_run(Duration::ZERO));
    let meta = serve(replies);
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    let (status, created, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(
        detail["summary"]["state"], "awaiting_human_approval",
        "{detail}"
    );
    assert_eq!(detail["summary"]["recommendation"], "supported");
    assert_eq!(
        meta.hits(),
        4,
        "the dropped attempt, its retry, two more calls"
    );
    h.wait_releases(1).await;
    let draws = h.ledger_records("draw");
    assert_eq!(
        draws.len(),
        4,
        "one draw per HTTP attempt, the retry included"
    );
    let settlements = h.ledger_records("settlement");
    assert_eq!(settlements.len(), 4);
    // The lost attempt may have been billed: its full estimate, never
    // refunded. The retry and the two later calls settle with their usage.
    assert_eq!(settlements[0]["attempt"], 1);
    assert_eq!(settlements[0]["outcome"]["kind"], "unknown");
    assert_eq!(settlements[0]["detail"], "transport error after send");
    // 1024 + body bytes at $4/M plus 12,000 × $20/M: at least the output.
    let est = Usd::parse(settlements[0]["charged_usd"].as_str().unwrap()).unwrap();
    assert!(est > Usd::parse("0.24").unwrap(), "{est}");
    for s in &settlements[1..] {
        assert_eq!(s["outcome"]["kind"], "usage", "{s}");
        assert_eq!(s["charged_usd"], "0.001400");
    }
    // The run's own record shows one proposal from two attempts (the
    // attempt count is an operator-only field; the public view hides it).
    assert_eq!(detail["model_invocations"].as_array().unwrap().len(), 3);
    assert!(detail["model_invocations"][0].get("attempts").is_none());
    let snapshot = h.ledger.snapshot().unwrap();
    assert_eq!(snapshot.unsettled_reservations, 0);
}

// ---------------------------------------------------------------------
// One solved challenge, one run
// ---------------------------------------------------------------------

/// Cloudflare re-validates a token under the idempotency key of its first
/// validation, for every caller. Forwarding the request's own
/// `Idempotency-Key` to Siteverify therefore let a second session replay a
/// solved challenge (the route's replay is owner-scoped and recorded only
/// after the start): two reservations and two runs on one token. The gate
/// mints a fresh UUID per verification instead, and the token is spent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_replayed_from_another_session_starts_no_second_run() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    h.set_siteverify(SiteverifyMode::SingleUse);
    let alice = h.session().await;
    let (status, created, _) = h
        .server
        .json(
            submit(&alice, &live_body(true, None), Some(TOKEN))
                .header("idempotency-key", "shared-key-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    h.server.wait_resting(&alice, &run_id).await;
    h.server.wait_released(&run_id).await;

    // Another visitor (another session, another address) presents Alice's
    // key and token. The route knows nothing of her key under this owner;
    // the gate asks Siteverify under a UUID of its own; the token is spent.
    let other_ip = "203.0.113.9";
    let (status, cookie) = h.server.new_session_from_header(other_ip).await;
    assert_eq!(status, StatusCode::CREATED);
    let mallory = cookie.expect("session cookie");
    let (status, body, _) = h
        .server
        .json(
            Req::post("/api/v1/live-runs")
                .cookie(&mallory)
                .header("cf-connecting-ip", other_ip)
                .header("cf-turnstile-response", TOKEN)
                .header("idempotency-key", "shared-key-1")
                .json(&live_body(true, None)),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "turnstile_failed");
    let calls = h.siteverify.calls();
    assert_eq!(
        calls.len(),
        2,
        "only Siteverify knows a token is spent, so it is asked: {calls:?}"
    );
    assert_ne!(calls[0].idempotency_key, calls[1].idempotency_key);
    for call in &calls {
        assert_ne!(call.idempotency_key, "shared-key-1", "{call:?}");
        assert!(
            uuid::Uuid::parse_str(&call.idempotency_key).is_ok(),
            "{call:?}"
        );
    }
    assert_eq!(
        h.ledger_records("reservation").len(),
        1,
        "one solved challenge, one reservation"
    );
    let (_, list, _) = h
        .server
        .json(
            Req::get("/api/v1/runs")
                .cookie(&mallory)
                .header("cf-connecting-ip", other_ip),
        )
        .await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0, "{list}");

    // Alice's own retry is still answered from her stored creation: neither
    // Siteverify nor the ledger is asked again.
    let (status, again, _) = h
        .server
        .json(
            submit(&alice, &live_body(true, None), Some(TOKEN))
                .header("idempotency-key", "shared-key-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["run"]["run_id"], created["run"]["run_id"]);
    assert_eq!(h.siteverify.calls().len(), 2);
    assert_eq!(h.ledger_records("reservation").len(), 1);
    assert_eq!(meta.hits(), 3, "one run's calls");
}

/// Three identical submits at once from one session (same key, same
/// token). The route's replay has nothing stored yet for any of them, so
/// all three reach the gate; each asks Siteverify under its own UUID and
/// the token passes exactly once. With the header forwarded, all three
/// shared one key and all three were admitted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_duplicates_of_one_submit_start_one_run() {
    let meta = serve(one_run(Duration::from_millis(300)));
    let h = live_server(
        LiveOptions {
            // Three live slots: nothing but Turnstile may tell them apart.
            server_env: vec![("CONSENSUS_LIVE_CONCURRENCY", "3".into())],
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    h.set_siteverify(SiteverifyMode::SingleUse);
    let cookie = h.session().await;
    let duplicate =
        || submit(&cookie, &live_body(true, None), Some(TOKEN)).header("idempotency-key", "dup-1");
    let (a, b, c) = tokio::join!(
        h.server.json(duplicate()),
        h.server.json(duplicate()),
        h.server.json(duplicate())
    );
    let mut outcomes = vec![a, b, c];
    outcomes.sort_by_key(|(status, _, _)| status.as_u16());
    let statuses: Vec<u16> = outcomes.iter().map(|(s, _, _)| s.as_u16()).collect();
    assert_eq!(statuses, vec![202, 403, 403], "{outcomes:?}");
    for (_, body, _) in &outcomes[1..] {
        assert_eq!(body["code"], "turnstile_failed", "{body}");
    }
    let calls = h.siteverify.calls();
    assert_eq!(calls.len(), 3, "{calls:?}");
    let mut keys: Vec<&str> = calls.iter().map(|c| c.idempotency_key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 3, "three distinct keys: {calls:?}");
    assert!(
        keys.iter().all(|k| *k != "dup-1"),
        "never the header's: {calls:?}"
    );
    let run_id = run_id_of(&outcomes[0].1);
    h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(h.ledger_records("reservation").len(), 1);
    let (_, list, _) = h
        .server
        .json(visitor(Req::get("/api/v1/runs"), &cookie))
        .await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 1, "{list}");
    assert_eq!(meta.hits(), 3, "one run's calls");
}

// ---------------------------------------------------------------------
// Turnstile before the live slot
// ---------------------------------------------------------------------

/// Bogus tokens sent from many prefixes kept the one live slot busy while
/// Siteverify answered them, because the slot was taken first: a visitor
/// who solved a challenge meanwhile was refused `429 too_many_runs`. The
/// gate now verifies before the slot is taken: three requests waiting in
/// Siteverify hold nothing, and a valid one is admitted while they wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_waiting_on_siteverify_never_hold_the_live_slot() {
    let held = HeldSiteverify::new();
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(
        LiveOptions {
            siteverify: Some(held.clone()),
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    // One live slot (the harness's CONSENSUS_LIVE_CONCURRENCY=1) and a
    // 300 ms admission wait. Three bogus submitters on three prefixes.
    let mut bogus = Vec::new();
    for (i, ip) in ["198.51.100.1", "198.51.100.2", "2001:db8:7::1"]
        .iter()
        .enumerate()
    {
        let (status, cookie) = h.server.new_session_from_header(ip).await;
        assert_eq!(status, StatusCode::CREATED);
        bogus.push(
            Req::post("/api/v1/live-runs")
                .cookie(&cookie.unwrap())
                .header("cf-connecting-ip", ip)
                .header(
                    "cf-turnstile-response",
                    &format!("{HELD_TOKEN_PREFIX}bogus-{i}"),
                )
                .json(&live_body(true, None)),
        );
    }
    let visitor_cookie = h.session().await;
    let [b0, b1, b2]: [Req; 3] = bogus.try_into().unwrap_or_else(|_| unreachable!());
    let held_requests =
        async { tokio::join!(h.server.json(b0), h.server.json(b1), h.server.json(b2)) };
    let valid_request = async {
        for _ in 0..500 {
            if held.held() == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let in_siteverify = held.held();
        let outcome = h
            .server
            .json(submit(&visitor_cookie, &live_body(true, None), Some(TOKEN)))
            .await;
        // Siteverify answers the bogus ones now: refused.
        let still_held = held.held();
        held.release();
        (in_siteverify, still_held, outcome)
    };
    let (bogus_outcomes, (in_siteverify, still_held, (status, created, _))) =
        tokio::join!(held_requests, valid_request);
    assert_eq!(
        in_siteverify, 3,
        "every bogus request was inside Siteverify at once"
    );
    assert_eq!(
        still_held, 3,
        "and still there while the valid one was admitted"
    );
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    for (status, body, _) in [&bogus_outcomes.0, &bogus_outcomes.1, &bogus_outcomes.2] {
        assert_eq!(*status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["code"], "turnstile_failed", "{body}");
    }
    h.server
        .wait_resting(&visitor_cookie, &run_id_of(&created))
        .await;
    assert_eq!(h.ledger_records("reservation").len(), 1);
    assert_eq!(held.tokens().len(), 4);
}

// The test below holds exactly this many calls at Cloudflare.
const _: () = assert!(SITEVERIFY_MAX_IN_FLIGHT == 8);

/// Siteverify calls in flight are bounded: with SITEVERIFY_MAX_IN_FLIGHT of
/// them held at Cloudflare, the next request is refused at once with
/// `503 turnstile_unavailable` and a Retry-After, and its token never
/// reaches Siteverify (a genuine token stays usable); once the held calls
/// have answered, the same token verifies and the run is admitted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn siteverify_calls_in_flight_are_bounded_and_a_refused_token_is_not_spent() {
    let held = HeldSiteverify::new();
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(
        LiveOptions {
            siteverify: Some(held.clone()),
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let mut bogus = Vec::new();
    for i in 0..SITEVERIFY_MAX_IN_FLIGHT {
        let ip = format!("2001:db8:{:x}::1", 0x100 + i);
        let (status, cookie) = h.server.new_session_from_header(&ip).await;
        assert_eq!(status, StatusCode::CREATED);
        bogus.push(
            Req::post("/api/v1/live-runs")
                .cookie(&cookie.unwrap())
                .header("cf-connecting-ip", &ip)
                .header(
                    "cf-turnstile-response",
                    &format!("{HELD_TOKEN_PREFIX}bogus-{i}"),
                )
                .json(&live_body(true, None)),
        );
    }
    let visitor_cookie = h.session().await;
    let [b0, b1, b2, b3, b4, b5, b6, b7]: [Req; 8] =
        bogus.try_into().unwrap_or_else(|_| unreachable!());
    let held_requests = async {
        tokio::join!(
            h.server.json(b0),
            h.server.json(b1),
            h.server.json(b2),
            h.server.json(b3),
            h.server.json(b4),
            h.server.json(b5),
            h.server.json(b6),
            h.server.json(b7)
        )
    };
    let visitor = async {
        for _ in 0..500 {
            if held.held() == SITEVERIFY_MAX_IN_FLIGHT {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let in_flight = held.held();
        let refused = h
            .server
            .json(submit(&visitor_cookie, &live_body(true, None), Some(TOKEN)))
            .await;
        let tokens_seen = held.tokens();
        held.release();
        (in_flight, refused, tokens_seen)
    };
    let (_bogus_outcomes, (in_flight, (status, body, headers), tokens_seen)) =
        tokio::join!(held_requests, visitor);
    assert_eq!(in_flight, SITEVERIFY_MAX_IN_FLIGHT, "every slot was taken");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "turnstile_unavailable", "{body}");
    assert!(headers.get(header::RETRY_AFTER).is_some(), "{headers:?}");
    assert!(
        !tokens_seen.iter().any(|t| t == TOKEN),
        "the refused token never reached Siteverify: {tokens_seen:?}"
    );
    assert!(h.ledger_records("reservation").is_empty());
    // The same token, presented again once the calls have answered.
    let (status, created, _) = h
        .server
        .json(submit(&visitor_cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    h.server
        .wait_resting(&visitor_cookie, &run_id_of(&created))
        .await;
}

/// With the slot busy, a request is answered for Turnstile first: a token
/// Siteverify refuses is `403 turnstile_failed`, slot or no slot; a token it
/// accepts meets the slot and is `429 too_many_runs` with nothing reserved,
/// and stays spent (Siteverify accepts a token once), so its retry needs a
/// new challenge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_live_slot_refuses_only_after_turnstile_passed_and_the_token_stays_spent() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    let slot = h.server.state.manager.reserve(RunKind::Live).await.unwrap();

    h.set_siteverify(SiteverifyMode::Fail);
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some("tok-refused")))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "turnstile_failed");
    assert_eq!(h.siteverify.calls().len(), 1);

    h.set_siteverify(SiteverifyMode::SingleUse);
    let (status, body, headers) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "too_many_runs");
    assert!(headers.get(header::RETRY_AFTER).is_some());
    assert_eq!(
        h.siteverify.calls().len(),
        2,
        "Turnstile was asked before the slot"
    );
    assert!(
        h.ledger_records("reservation").is_empty(),
        "nothing charged"
    );
    drop(slot);

    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the token is spent: {body}");
    assert_eq!(body["code"], "turnstile_failed");
    let (status, created, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some("tok-live-2")))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    h.server.wait_resting(&cookie, &run_id_of(&created)).await;
    assert_eq!(h.ledger_records("reservation").len(), 1);
}

// ---------------------------------------------------------------------
// The daily quotas: per client key, per judge code
// ---------------------------------------------------------------------

/// A ledger clock the test sets: the day the quotas count in.
#[derive(Clone)]
struct TestClock(Arc<Mutex<DateTime<Utc>>>);

impl TestClock {
    fn at(iso: &str) -> Self {
        Self(Arc::new(Mutex::new(parse_time(iso))))
    }

    fn set(&self, iso: &str) {
        *self.0.lock().unwrap() = parse_time(iso);
    }

    fn clock(&self) -> LedgerClock {
        let shared = self.0.clone();
        Arc::new(move || *shared.lock().unwrap())
    }
}

fn parse_time(iso: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .with_timezone(&Utc)
}

fn client_key(ip: &str) -> ClientKey {
    ClientKey::from_ip(ip.parse().unwrap())
}

/// One run after the other, each brought to rest before the next.
async fn admit_and_finish(h: &LiveHarness, cookie: &str, req: Req) -> Value {
    let (status, created, _) = h.server.json(req).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    h.server.wait_resting(cookie, &run_id).await;
    h.server.wait_released(&run_id).await;
    created
}

/// R9: at most `CONSENSUS_LIVE_CLIENT_DAILY` (default 3) public-lane
/// admissions per client key per UTC day, counted when the reservation is
/// granted and never for a refusal; `429 live_client_quota` until 00:00
/// UTC; the judge lane exempt; another client unaffected; a new day starts
/// again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_starts_its_daily_share_of_public_runs_and_no_more() {
    let mut replies = Vec::new();
    for _ in 0..6 {
        replies.extend(one_run(Duration::ZERO));
    }
    let meta = serve(replies);
    let clock = TestClock::at("2026-10-28T21:00:00Z");
    let h = live_server(
        LiveOptions {
            // The production default, over the harness's out-of-the-way 100.
            live_env: vec![(vars::CLIENT_DAILY, live_defaults::CLIENT_DAILY.into())],
            // Seven creations from one client inside a minute.
            server_env: vec![("CONSENSUS_RATE_RUNS_PER_MIN", "60".into())],
            ledger_clock: Some(clock.clock()),
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let visitor_key = client_key(VISITOR_IP);
    let cookie = h.session().await;

    // A ledger refusal counts nothing: the day spent by four reservations
    // held outside the gate, the visitor is refused by the ledger.
    let held: Vec<_> = (0..4)
        .map(|_| h.ledger.reserve(Lane::Public).unwrap())
        .collect();
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "live_budget_daily");
    assert_eq!(h.gate.client_admissions_today(visitor_key), 0);
    for r in held {
        h.ledger.release(&r).unwrap();
    }

    // Three admissions, then the quota.
    for _ in 0..3 {
        admit_and_finish(
            &h,
            &cookie,
            submit(&cookie, &live_body(true, None), Some(TOKEN)),
        )
        .await;
    }
    assert_eq!(h.gate.client_admissions_today(visitor_key), 3);
    let reserved = h.ledger_records("reservation").len();
    let (status, body, headers) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "live_client_quota");
    assert_eq!(
        body["message"],
        refusal_message(ErrorCode::LiveClientQuota),
        "a fixed sentence"
    );
    assert!(
        !body["message"]
            .as_str()
            .unwrap()
            .contains(|c: char| c.is_ascii_digit()),
        "no figures: {body}"
    );
    // Until 00:00 UTC: three hours from 21:00.
    assert_eq!(body["retry_after_secs"], 10_800);
    assert_eq!(headers[header::RETRY_AFTER], "10800");
    assert_eq!(
        h.ledger_records("reservation").len(),
        reserved,
        "the refusal reserved nothing"
    );
    assert_eq!(h.gate.client_admissions_today(visitor_key), 3);

    // The judge lane is exempt, and another client has its own share.
    admit_and_finish(
        &h,
        &cookie,
        submit(&cookie, &live_body(true, Some(JUDGE_CODE)), Some(TOKEN)),
    )
    .await;
    let other_ip = "198.51.100.40";
    let (_, other) = h.server.new_session_from_header(other_ip).await;
    let other = other.unwrap();
    admit_and_finish(
        &h,
        &other,
        Req::post("/api/v1/live-runs")
            .cookie(&other)
            .header("cf-connecting-ip", other_ip)
            .header("cf-turnstile-response", TOKEN)
            .json(&live_body(true, None)),
    )
    .await;
    assert_eq!(h.gate.client_admissions_today(client_key(other_ip)), 1);

    // The next UTC day starts again.
    clock.set("2026-10-29T00:00:01Z");
    admit_and_finish(
        &h,
        &cookie,
        submit(&cookie, &live_body(true, None), Some(TOKEN)),
    )
    .await;
    assert_eq!(h.gate.client_admissions_today(visitor_key), 1);
    let lanes: Vec<Value> = h
        .ledger_records("reservation")
        .iter()
        .map(|r| r["lane"].clone())
        .collect();
    assert_eq!(
        lanes.iter().filter(|l| *l == "public").count(),
        4 + 3 + 1 + 1
    );
    assert_eq!(lanes.iter().filter(|l| *l == "judge").count(), 1);
}

/// A start that fails after the grant gives its count back, like its
/// reservation: with a quota of one, the second request is not refused by
/// the quota.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_start_gives_its_client_count_back() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(
        LiveOptions {
            factory: FactoryMode::FailingStart,
            live_env: vec![(vars::CLIENT_DAILY, "1".into())],
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let cookie = h.session().await;
    for attempt in 1..=2 {
        let (status, body, _) = h
            .server
            .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
            .await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "attempt {attempt}: {body}"
        );
        h.wait_releases(attempt).await;
        assert_eq!(h.gate.client_admissions_today(client_key(VISITOR_IP)), 0);
    }
    assert_eq!(h.ledger_records("reservation").len(), 2);
}

/// R8: at most `CONSENSUS_LIVE_JUDGE_CODE_DAILY` judge-lane admissions per
/// code per UTC day. A code over its limit is refused with the judge lane's
/// own daily refusal (no new code, so its holder learns nothing new),
/// whichever client presents it; another code has its own count; the next
/// day starts again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judge_code_starts_its_daily_limit_and_is_then_refused_like_the_judge_day() {
    let mut replies = Vec::new();
    for _ in 0..4 {
        replies.extend(one_run(Duration::ZERO));
    }
    let meta = serve(replies);
    let clock = TestClock::at("2026-10-28T22:30:00Z");
    let h = live_server(
        LiveOptions {
            live_env: vec![(vars::JUDGE_CODE_DAILY, "2".into())],
            ledger_clock: Some(clock.clock()),
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    let cookie = h.session().await;
    let judge = || submit(&cookie, &live_body(true, Some(JUDGE_CODE)), Some(TOKEN));
    admit_and_finish(&h, &cookie, judge()).await;
    admit_and_finish(&h, &cookie, judge()).await;
    let (status, body, headers) = h.server.json(judge()).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "live_budget_judge_daily");
    assert_eq!(
        body["message"],
        refusal_message(ErrorCode::LiveBudgetJudgeDaily)
    );
    // Until 00:00 UTC, as the judge lane's daily refusal says.
    assert_eq!(body["retry_after_secs"], 5_400);
    assert_eq!(headers[header::RETRY_AFTER], "5400");
    // The same code from another client: per code, not per client.
    let other_ip = "198.51.100.41";
    let (_, other) = h.server.new_session_from_header(other_ip).await;
    let other = other.unwrap();
    let (status, body, _) = h
        .server
        .json(
            Req::post("/api/v1/live-runs")
                .cookie(&other)
                .header("cf-connecting-ip", other_ip)
                .header("cf-turnstile-response", TOKEN)
                .json(&live_body(true, Some(JUDGE_CODE))),
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "live_budget_judge_daily");
    // Another code has its own count.
    admit_and_finish(
        &h,
        &cookie,
        submit(
            &cookie,
            &live_body(true, Some(SECOND_JUDGE_CODE)),
            Some(TOKEN),
        ),
    )
    .await;
    let judge_runs = |h: &LiveHarness| {
        h.ledger_records("reservation")
            .iter()
            .filter(|r| r["lane"] == "judge")
            .count()
    };
    assert_eq!(judge_runs(&h), 3, "the refusals reserved nothing");
    // The next UTC day starts again.
    clock.set("2026-10-29T00:00:01Z");
    admit_and_finish(&h, &cookie, judge()).await;
    assert_eq!(judge_runs(&h), 4);
}

// ---------------------------------------------------------------------
// What the operator sees before anyone submits
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_kill_switch_is_reported_before_any_admission_trips_the_ledger() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], true, "{ready}");
    assert_eq!(ready["live"]["tripped"], false);

    // The operator flips the switch during an incident and watches /ready:
    // it says paused at once, although no visitor has tripped the ledger.
    std::fs::write(h.kill_switch(), b"stop\n").unwrap();
    let (status, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    assert_eq!(ready["ready"], false, "{ready}");
    assert_eq!(ready["live"]["ledger_ok"], true);
    assert_eq!(ready["live"]["tripped"], true);
    assert_eq!(ready["live"]["trip_code"], "kill_switch");
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["tripped"], true, "{caps}");
    assert_eq!(caps["live"]["budget"]["trip_code"], "kill_switch");
    assert!(
        h.ledger_records("trip").is_empty(),
        "the routes read the file; the ledger records the trip at the next admission"
    );

    // Switched back before anyone came: nothing was recorded, live mode
    // simply resumes.
    std::fs::remove_file(h.kill_switch()).unwrap();
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], true, "{ready}");
    assert_eq!(ready["live"]["tripped"], false);
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["tripped"], false, "{caps}");

    // Once an admission has recorded the trip, removing the file no longer
    // helps: that takes the operator's reset.
    std::fs::write(h.kill_switch(), b"stop\n").unwrap();
    let cookie = h.session().await;
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_tripped");
    assert_eq!(h.ledger_records("trip").len(), 1);
    std::fs::remove_file(h.kill_switch()).unwrap();
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], false, "{ready}");
    assert_eq!(ready["live"]["tripped"], true);
    assert_eq!(ready["live"]["trip_code"], "kill_switch");
    assert_eq!(meta.hits(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_ledger_publishes_no_budget_and_admits_nobody() {
    let meta = serve(one_run(Duration::ZERO));
    let h = live_server(LiveOptions::default(), meta_config(&meta.base_url));
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live"]["budget"]["daily_remaining"], "10.000000");

    h.corrupt_ledger();
    // No figure is invented: the budget is absent, not a zero that would
    // read as a spent day.
    let (status, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(status, StatusCode::OK, "{caps}");
    assert_eq!(caps["live_enabled"], true);
    assert!(caps["live"].get("budget").is_none(), "{caps}");
    assert_eq!(caps["live"]["presets"].as_array().unwrap().len(), 5);
    let (status, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    assert_eq!(ready["ready"], false, "{ready}");
    assert_eq!(ready["live"]["ledger_ok"], false);
    assert_eq!(ready["live"]["tripped"], false);
    // An admission fails closed with the generic code, says nothing about
    // files, and reserves nothing.
    let cookie = h.session().await;
    let (status, body, _) = h
        .server
        .json(submit(&cookie, &live_body(true, None), Some(TOKEN)))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_unavailable");
    let text = body.to_string().to_ascii_lowercase();
    for leak in ["corrupt", ".jsonl", "byte", "ledger record"] {
        assert!(!text.contains(leak), "{text}");
    }
    assert!(!h.ledger_text().contains("\"type\":\"reservation\""));
    assert_eq!(meta.hits(), 0);
}

// ---------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------

#[test]
fn startup_refuses_live_mode_until_the_snapshot_client_is_wired() {
    let dir = common::TempDir::new("live-startup");
    let secret = dir.path().join("turnstile.secret");
    std::fs::write(&secret, "1x0000000000000000000000000000000AA\n").unwrap();
    let secret = secret.display().to_string();
    let live = live_config(
        dir.path(),
        &[
            (vars::LIVE_ENABLED, "true"),
            (vars::TURNSTILE_SECRET_FILE, &secret),
            (vars::TURNSTILE_HOSTNAME, "demo.example.org"),
        ],
    );
    assert!(live.enabled);
    let meta = meta_config("http://127.0.0.1:9/v1");
    let err = match build_live_gate(&live, meta.clone(), None) {
        Err(e) => e,
        Ok(_) => panic!("live mode must not start without an evidence factory"),
    };
    assert!(err.contains("CONSENSUS_SNAPSHOT_DIR"), "{err}");
    assert!(err.contains("not wired"), "{err}");
    assert!(!err.contains(TEST_KEY));
    // With a factory the gate assembles and publishes the presets.
    let factory = ScriptedEvidenceFactory::new(FactoryMode::Scripted);
    let gate = build_live_gate(&live, meta, Some(factory)).expect("gate builds");
    let caps = gate.capabilities().expect("live capabilities");
    assert_eq!(caps.presets.len(), 5);
    let budget = caps.budget.expect("a readable ledger publishes its budget");
    assert_eq!(budget.lifetime_remaining, "60.000000");
    let ready = gate.readiness().expect("live readiness");
    assert!(ready.ledger_ok && ready.evidence_ok && !ready.tripped);
}

/// What `serve-public` gets from `SnapshotFactory::from_env`, over a
/// variable map instead of the process environment: no factory without
/// the directory (so `build_live_gate` refuses live mode as above), and a
/// configuration error naming its variable for a directory without the
/// pinned anchor, before any file is read.
#[test]
fn the_snapshot_factory_is_absent_without_its_directory_and_fails_closed_without_the_anchor() {
    let none = SnapshotFactory::from_vars(|_| None).expect("no directory is not an error");
    assert!(none.is_none(), "no directory, no factory");
    let empty = SnapshotFactory::from_vars(|name| {
        (name == "CONSENSUS_SNAPSHOT_DIR").then(|| "   ".to_string())
    })
    .expect("an empty value is unset");
    assert!(empty.is_none());

    let dir = common::TempDir::new("live-snapshot-vars");
    let only_dir = dir.path().display().to_string();
    let err = SnapshotFactory::from_vars(|name| {
        (name == "CONSENSUS_SNAPSHOT_DIR").then(|| only_dir.clone())
    })
    .err()
    .expect("a directory without the pinned anchor fails closed");
    assert!(err.starts_with("CONSENSUS_HYPHAE_EXPECTED_ANCHOR"), "{err}");
    assert!(err.contains("required"), "{err}");
    assert!(err.contains("CONSENSUS_SNAPSHOT_DIR"), "{err}");
    assert!(!err.contains(&only_dir), "no path: {err}");
    assert!(!err.contains(TEST_KEY));
}
