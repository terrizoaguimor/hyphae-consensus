// SPDX-License-Identifier: Apache-2.0
//! The research showcase over HTTP: entries from `research/MANIFEST.json`,
//! archived run details in public view, verbatim files with their pinned
//! digest as `ETag`, and nothing ever written to `research/`.

mod common;

use axum::http::{header, StatusCode};
use common::{body_bytes, public_server, research_dir, Req};
use consensus_api::server::BuildOptions;
use std::collections::BTreeMap;
use std::path::Path;

/// Every file under `dir` with its length and modification time.
fn listing(dir: &Path, prefix: &str, out: &mut BTreeMap<String, (u64, std::time::SystemTime)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if path.is_dir() {
            listing(&path, &format!("{name}/"), out);
        } else {
            let meta = entry.metadata().unwrap();
            out.insert(name, (meta.len(), meta.modified().unwrap()));
        }
    }
}

fn snapshot_research() -> BTreeMap<String, (u64, std::time::SystemTime)> {
    let mut out = BTreeMap::new();
    listing(&research_dir(), "", &mut out);
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn showcase_lists_archived_runs_and_serves_their_public_views() {
    let before = snapshot_research();
    let server = public_server(&[], BuildOptions::default());

    let (status, list, _) = server.json(Req::get("/api/v1/showcase")).await;
    assert_eq!(status, StatusCode::OK);
    let entries = list["entries"].as_array().unwrap();
    assert!(entries.len() >= 33, "{}", entries.len());
    let f03 = entries
        .iter()
        .find(|e| e["label"] == "F03")
        .expect("F03 in the showcase");
    assert_eq!(f03["provenance"]["source"], "research_archive");
    assert_eq!(f03["provenance"]["model"], "live_meta");
    assert_eq!(f03["outcome"], "contested");
    assert!(f03["head_hash"].is_string());
    assert_eq!(
        f03["file"],
        "2026-10-27/live-battery-2026-10-27-final.jsonl"
    );
    let manifest_sha = list["manifest_sha256"].as_str().unwrap().to_string();
    assert_eq!(manifest_sha.len(), 64);

    let run_id = f03["run_id"].as_str().unwrap();
    let (status, detail, _) = server
        .json(Req::get(&format!("/api/v1/showcase/{run_id}")))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        detail["summary"]["provenance"]["source"],
        "research_archive"
    );
    assert_eq!(detail["chain"]["scope"], "log");
    let text = detail.to_string();
    assert!(
        !text.contains("/hyphae-home/"),
        "container paths leaked into a public view"
    );
    assert!(!text.contains("witness_file"), "witness paths leaked");
    assert!(
        detail["recommendation"]["trace"].is_object(),
        "policy trace rendered"
    );

    // The archived receipt is derived, never stored.
    let response = server
        .send(Req::get(&format!("/api/v1/showcase/{run_id}/receipt")))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    assert_eq!(receipt["run_id"], run_id);
    assert_eq!(receipt["chain"]["head_hash"], f03["head_hash"]);

    // Unknown ids, including traversal attempts, are 404 without I/O.
    for bad in [
        "run-00000000-0000-4000-8000-000000000000",
        "..%2FMANIFEST.json",
        "%2e%2e%2f%2e%2e%2fCargo.toml",
    ] {
        let (status, body, _) = server
            .json(Req::get(&format!("/api/v1/showcase/{bad}")))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{bad}: {body}");
        assert_eq!(body["code"], "not_found");
    }

    // A 1.0 console run is served unchained and says so.
    let c01 = entries.iter().find(|e| e["label"] == "C01").expect("C01");
    assert!(c01["head_hash"].is_null());
    assert!(c01["provenance"]["flags"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "unchained"));

    assert_eq!(snapshot_research(), before, "research/ was touched");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn research_files_are_served_verbatim_with_their_digest() {
    let before = snapshot_research();
    let server = public_server(&[], BuildOptions::default());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(research_dir().join("MANIFEST.json")).unwrap())
            .unwrap();
    let file = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "2026-10-27/live-battery-2026-10-27-final.jsonl")
        .unwrap();
    let sha = file["sha256"].as_str().unwrap();

    let response = server
        .send(Req::get(
            "/research/2026-10-27/live-battery-2026-10-27-final.jsonl",
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::ETAG], format!("\"{sha}\""));
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/x-ndjson"
    );
    assert_eq!(
        response.headers()["x-content-sha256"],
        format!("sha256={sha}")
    );
    let bytes = body_bytes(response).await;
    let on_disk =
        std::fs::read(research_dir().join("2026-10-27/live-battery-2026-10-27-final.jsonl"))
            .unwrap();
    assert_eq!(bytes.as_ref(), on_disk.as_slice(), "verbatim bytes");

    let response = server
        .send(
            Req::get("/research/2026-10-27/live-battery-2026-10-27-final.jsonl")
                .header("if-none-match", &format!("\"{sha}\"")),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

    let response = server.send(Req::get("/research/MANIFEST.json")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");

    for bad in [
        "/research/../Cargo.toml",
        "/research/..%2FCargo.toml",
        "/research/2026-10-27/..%2F..%2FCargo.toml",
        "/research/2026-10-27",
        "/research/nope.jsonl",
        "/research/C:%5CWindows%5Cwin.ini",
    ] {
        let response = server.send(Req::get(bad)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{bad}");
    }
    assert_eq!(snapshot_research(), before, "research/ was touched");
}
