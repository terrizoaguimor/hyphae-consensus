// SPDX-License-Identifier: Apache-2.0
//! Credential canary (G4): the operator surface is started with a fake
//! Meta key in its environment and a model endpoint that refuses
//! connections; the key must appear in no response body, stored event,
//! receipt or ndjson line. A dedicated binary, because it sets process
//! environment variables.

mod common;

use axum::http::StatusCode;
use common::{body_text, operator_server, read_sse, Req};
use consensus_api::server::BuildOptions;
use serde_json::json;
use std::path::Path;

const CANARY: &str = "canary-key-7f3e9c2a-never-leak-0b1d";

fn files_under(dir: &Path, out: &mut Vec<Vec<u8>>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files_under(&path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push(bytes);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_meta_key_never_appears_in_any_output() {
    // Edition 2021: set_var is a safe function. The endpoint refuses
    // connections at once (port 9, discard), so no Meta API is reached.
    std::env::set_var("META_API_KEY", CANARY);
    std::env::set_var("CONSENSUS_META_BASE_URL", "http://127.0.0.1:9/v1");
    std::env::set_var("CONSENSUS_MODEL_TIMEOUT_SECS", "2");

    let server = operator_server(&[], BuildOptions::default());
    let (status, created, _) = server
        .json(
            Req::post("/api/v1/operator/runs")
                .json(&json!({"model":"meta","memory":"fake","title":"Canary run"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = created["run"]["run_id"].as_str().unwrap().to_string();
    assert_eq!(created["run"]["provenance"]["declared"]["model"], "meta");

    let mut outputs: Vec<String> = vec![created.to_string()];
    let mut detail = serde_json::Value::Null;
    for _ in 0..600 {
        let (status, body, _) = server
            .json(Req::get(&format!("/api/v1/runs/{run_id}")))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["summary"]["state"] == "failed" {
            detail = body;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(detail["summary"]["state"], "failed", "{detail}");
    // A refused connection reaches the adapter as a send error, which it
    // reports as a timeout or as unavailability; either is a model failure.
    let code = detail["summary"]["failure"]["code"].as_str().unwrap_or("");
    assert!(
        matches!(code, "model_unavailable" | "model_timeout"),
        "{detail}"
    );
    // The declared Meta mode is cross-checked: no Meta call was recorded.
    assert_eq!(detail["summary"]["provenance"]["model"], "undetermined");
    outputs.push(detail.to_string());

    for path in [
        format!("/api/v1/runs/{run_id}/receipt"),
        format!("/api/v1/runs/{run_id}/events.ndjson"),
        "/api/v1/runs".to_string(),
        "/api/v1/health".to_string(),
        "/api/v1/ready".to_string(),
        "/api/v1/capabilities".to_string(),
    ] {
        let response = server.send(Req::get(&path)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        outputs.push(body_text(response).await);
    }
    let response = server
        .send(Req::get(&format!("/api/v1/runs/{run_id}/events")))
        .await;
    for frame in read_sse(response.into_body(), 100).await {
        outputs.push(frame.data);
    }
    let mut stored = Vec::new();
    files_under(&server.dir.path().join("store"), &mut stored);
    assert!(!stored.is_empty(), "the run was stored");

    for output in &outputs {
        assert!(
            !output.contains(CANARY),
            "canary in an API response: {output}"
        );
    }
    for bytes in &stored {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(CANARY), "canary in a stored file");
    }
    // The health body probes nothing: it must not even say whether a key
    // is configured.
    let (_, health, _) = server.json(Req::get("/api/v1/health")).await;
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

    std::env::remove_var("META_API_KEY");
    std::env::remove_var("CONSENSUS_META_BASE_URL");
    std::env::remove_var("CONSENSUS_MODEL_TIMEOUT_SECS");
}
