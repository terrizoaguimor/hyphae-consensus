// SPDX-License-Identifier: Apache-2.0
//! The public live mode over the sealed snapshot (ADR-0008), end to end
//! through the public router: the production `SnapshotFactory` boots from
//! a fixture snapshot directory with the fake `hyphae` binary, the gate
//! admits a visitor (fake Siteverify, temporary ledger, trusted proxy
//! headers), the fake Chat Completions server answers the roles, and the
//! run's public views carry sealed-snapshot evidence proven and verified
//! by that binary, with no proof, witness or container path anywhere. Then
//! a tampered snapshot refusing to boot, and the per-run verifier being
//! fresh. No Hyphae and no Meta API are reached.

mod common;
mod support;

use axum::http::StatusCode;
use chrono::{DateTime, FixedOffset};
use common::live::{live_config, live_server, FactoryMode, LiveOptions, VISITOR_IP};
use common::snapshot::{
    build, config as snapshot_config, fake_hyphae, flip_first_byte, logged_calls, proof_bytes,
    sha256_hex, verify_report, write_checksums, ANCHOR, CAPTURED_AT, LIST_ID, OTHER, PROJECT,
    RECORD_IDS, SNAPSHOT_ID,
};
use common::{body_text, Req};
use consensus_api::hyphae::SystemCommandRunner;
use consensus_api::live::config::vars;
use consensus_api::live::gate::DeliberationFactory;
use consensus_api::live::snapshot_factory::{status as factory_status, SnapshotFactory};
use consensus_api::server::build_live_gate;
use consensus_api::snapshot::{codes, verifier_from_config};
use consensus_domain::EvidenceMode;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use support::{config as meta_config, envelope, serve, Reply, TEST_KEY};

/// The reference preset: project `demo-repo`, the snapshot's project.
const PRESET: &str = "R01-reference";
/// A Turnstile token; the scripted Siteverify accepts any.
const TOKEN: &str = "tok-snapshot-1";
/// The record the fake Evaluator cites: the fact that payment client
/// timeouts correlate with retry bursts above 5 attempts.
const CITED: &str = RECORD_IDS[1];
/// The per-call input cap these tests run with (tokens); the production
/// default is 8,000, which the Evaluator's request over the seven snapshot
/// records exceeds (see [`Fixture::options`]).
const INPUT_CAP_TOKENS: u64 = 16_000;

/// The drafts the fake Chat Completions server answers, in call order: one
/// claim the cited snapshot record states, an empty critique, and an
/// evaluation keyed by the id the adapter mints for that claim (`prop-1`,
/// then `claim-2`) citing a retrieved record. Supported in one round.
const DRAFT_PROPOSAL: &str = r#"{"summary":"Keep retries capped at 3 attempts with jitter","plan_steps":["add jitter to the payment client backoff"],"claims":[{"statement":"Payment timeouts correlate with retry bursts above 5 attempts"}],"assumptions":[],"uncertainties":[]}"#;
const DRAFT_CHALLENGE: &str = r#"{"items":[]}"#;

fn draft_evaluation() -> String {
    json!({
        "assessments": {
            "claim-2": {
                "corroborated": true,
                "relevant_evidence": [CITED],
                "contradicting_evidence": [],
                "note": "the fact record states the correlation"
            }
        },
        "item_verdicts": {},
        "cleared_contradictions": [],
        "abstain": false,
        "abstain_reason": null
    })
    .to_string()
}

/// The three answers of one supported run.
fn one_run() -> Vec<Reply> {
    vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)),
        Reply::ok(envelope(DRAFT_CHALLENGE)),
        Reply::ok(envelope(&draft_evaluation())),
    ]
}

fn live_body() -> Value {
    json!({"preset_id": PRESET, "attest_adult": true})
}

/// A visitor's request on a listener that trusts the proxy headers.
fn visitor(req: Req, cookie: &str) -> Req {
    req.cookie(cookie).header("cf-connecting-ip", VISITOR_IP)
}

fn submit(cookie: &str) -> Req {
    visitor(Req::post("/api/v1/live-runs"), cookie)
        .json(&live_body())
        .header("cf-turnstile-response", TOKEN)
}

fn run_id_of(created: &Value) -> String {
    created["run"]["run_id"].as_str().unwrap().to_string()
}

/// The request the list capture's proof seals.
fn list_request() -> Value {
    json!({"operation": "list", "project": PROJECT, "query": "", "limit": 64})
}

/// A fixture snapshot directory with the fake `hyphae` binary next to it.
struct Fixture {
    dir: PathBuf,
    hyphae_bin: String,
    log: PathBuf,
}

impl Fixture {
    fn new(test: &str) -> Self {
        let dir = build(&format!("live-{test}"));
        let root = dir.parent().unwrap().to_path_buf();
        let (hyphae_bin, log) = fake_hyphae(&root);
        Self {
            dir,
            hyphae_bin,
            log,
        }
    }

    /// The harness over the production factory on this snapshot. The
    /// per-call input cap is raised above the production default (8,000):
    /// the ledger bounds a call's input at `request bytes + 1024` tokens,
    /// and the Evaluator's request over the seven snapshot records (their
    /// real texts) measures 8,135, which the default cap refuses at the
    /// evaluate stage (`budget_refused`). See [`INPUT_CAP_TOKENS`].
    fn options(&self) -> LiveOptions {
        LiveOptions {
            factory: FactoryMode::Snapshot {
                dir: self.dir.clone(),
                verifier_dir: None,
                hyphae_bin: self.hyphae_bin.clone(),
                anchor: ANCHOR.into(),
            },
            live_env: vec![(vars::INPUT_CAP_TOKENS, INPUT_CAP_TOKENS.to_string())],
            ..LiveOptions::default()
        }
    }

    /// Every invocation of the fake binary so far.
    fn calls(&self) -> Vec<String> {
        logged_calls(&self.log)
    }

    fn capture_file(&self, name: &str) -> PathBuf {
        self.dir.join("captures").join(name)
    }

    /// Fails when `text` names the snapshot directory (in any spelling a
    /// JSON body could carry), an artifact file, the mapped container
    /// directory or synthetic artifact contents.
    fn assert_no_path(&self, what: &str, text: &str) {
        let local = self.dir.to_string_lossy().into_owned();
        let escaped = local.replace('\\', "\\\\");
        let slashed = local.replace('\\', "/");
        for leak in [
            local.as_str(),
            escaped.as_str(),
            slashed.as_str(),
            "captures/",
            "captures\\",
            ".witness",
            ".proof",
            "/snapshots/",
            "synthetic",
            "hyphae-calls.log",
        ] {
            assert!(!text.contains(leak), "{what} leaks {leak:?}: {text}");
        }
    }
}

/// The public payloads of one `type` in a run detail's events.
fn payloads<'a>(detail: &'a Value, kind: &str) -> Vec<&'a Value> {
    detail["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["payload"])
        .filter(|p| p["type"] == kind)
        .collect()
}

fn checked_at(detail: &Value) -> Vec<DateTime<FixedOffset>> {
    payloads(detail, "evidence_verified")
        .iter()
        .map(|p| {
            let at = p["verification"]["checked_at"]
                .as_str()
                .expect("checked_at");
            DateTime::parse_from_rfc3339(at).unwrap_or_else(|e| panic!("{at}: {e}"))
        })
        .collect()
}

fn ids_in_order(payloads: &[&Value], path: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for p in payloads {
        let mut v = *p;
        for key in path {
            v = &v[*key];
        }
        let id = v.as_str().unwrap().to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

// ---------------------------------------------------------------------
// Capabilities, readiness and one proven run
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_run_over_the_sealed_snapshot_is_proven_verified_and_published_without_paths() {
    let fx = Fixture::new("run");
    let meta = serve(one_run());
    let h = live_server(fx.options(), meta_config(&meta.base_url));

    // Boot: both required captures went through the fake binary with the
    // pinned anchor, before the server existed.
    let boot = fx.calls();
    assert_eq!(boot.len(), 2, "{boot:?}");
    for call in &boot {
        assert!(call.starts_with("proof verify --proof"), "{call}");
        assert!(call.contains(&format!("--anchor {ANCHOR}")), "{call}");
    }
    let snapshot = h.snapshot.as_ref().expect("the snapshot factory");
    let status = snapshot.snapshot_status();
    assert_eq!(status.snapshot_id, SNAPSHOT_ID);
    assert_eq!(status.project, PROJECT);
    assert_eq!(status.isolation_project, OTHER);
    assert_eq!(status.record_count, 7);
    assert_eq!(status.anchor_hex, ANCHOR);
    assert_eq!(status.captures.len(), 2);
    assert_eq!(snapshot.evidence_mode(), EvidenceMode::SealedSnapshot);
    let factory_state = snapshot.status();
    assert!(factory_state.ok);
    assert_eq!(factory_state.kind, EvidenceMode::SealedSnapshot);
    assert_eq!(factory_state.detail.as_deref(), Some(factory_status::OK));
    let line = snapshot.startup_line();
    for part in [
        SNAPSHOT_ID,
        PROJECT,
        "7 records",
        &ANCHOR[..12],
        status.verified_at.as_str(),
    ] {
        assert!(line.contains(part), "{line}");
    }
    fx.assert_no_path("startup line", &line);
    fx.assert_no_path("debug", &format!("{snapshot:?}"));

    // (a) Published: every preset's evidence is the snapshot; ready.
    let (code, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(code, StatusCode::OK, "{caps}");
    assert_eq!(caps["live_enabled"], true);
    let presets = caps["live"]["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 5);
    assert!(
        presets
            .iter()
            .all(|p| p["evidence"] == "hyphae_snapshot" && p["model"] == "live_meta"),
        "{presets:?}"
    );
    assert_eq!(presets[0]["id"], PRESET);
    assert_eq!(presets[0]["project"], PROJECT);
    let (code, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(code, StatusCode::OK, "{ready}");
    assert_eq!(ready["ready"], true, "{ready}");
    assert_eq!(ready["live"]["ledger_ok"], true);
    assert_eq!(ready["live"]["evidence_ok"], true);
    assert_eq!(ready["live"]["tripped"], false);

    // (b) The run: declared sealed_snapshot on the 202, completed over the
    // fake Meta server's three answers.
    let cookie = h.session().await;
    let (code, created, _) = h.server.json(submit(&cookie)).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    let declared = json!({
        "model": "meta",
        "evidence": "sealed_snapshot",
        "origin": "public",
        "preset": PRESET
    });
    assert_eq!(created["run"]["provenance"]["declared"], declared);
    assert_eq!(created["run"]["provenance"]["model"], "undetermined");
    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(
        detail["summary"]["state"], "awaiting_human_approval",
        "{detail}"
    );
    assert_eq!(detail["summary"]["recommendation"], "supported");
    assert_eq!(detail["task"]["mode"], declared);
    let provenance = &detail["summary"]["provenance"];
    assert_eq!(provenance["model"], "live_meta");
    assert_eq!(provenance["evidence"], "hyphae_snapshot");
    assert_eq!(provenance["declared"], declared);
    assert!(
        provenance["flags"].as_array().is_none_or(|f| f.is_empty()),
        "{provenance}"
    );
    assert_eq!(meta.hits(), 3);

    // Every retrieved record is the snapshot's: sealed_snapshot retrieval,
    // the list capture as its proof, the proof's own request, and none of
    // the operator-only fields (the artifact paths among them).
    let retrieved = payloads(&detail, "evidence_retrieved");
    assert!(retrieved.len() >= 7, "{}", retrieved.len());
    assert_eq!(
        ids_in_order(&retrieved, &["evidence", "id"]),
        RECORD_IDS,
        "the whole proven set, in capture order"
    );
    let proof_sha256 = sha256_hex(&proof_bytes(LIST_ID));
    for p in &retrieved {
        let ev = &p["evidence"];
        assert_eq!(ev["retrieval"], "sealed_snapshot", "{ev}");
        assert_eq!(ev["project_id"], PROJECT);
        assert_eq!(ev["snapshot_id"], SNAPSHOT_ID);
        assert_eq!(ev["captured_at"], CAPTURED_AT);
        assert_eq!(ev["proof_ref"], LIST_ID, "the capture id, no path");
        assert_eq!(ev["proof_sha256"], proof_sha256);
        assert_eq!(ev["proof_request"], list_request());
        assert_eq!(ev["anchor"], ANCHOR);
        assert_eq!(ev["backend_project"], PROJECT);
        assert_eq!(ev["proof_error_reported"], false);
        for hidden in ["proof_file", "witness_file", "backend_ref", "proof_error"] {
            assert!(ev.get(hidden).is_none(), "{hidden} is operator-only: {ev}");
        }
    }

    // RecallProven names the list capture, every captured id, the anchor.
    let proven = payloads(&detail, "recall_proven");
    assert!(!proven.is_empty(), "{detail}");
    for p in &proven {
        assert_eq!(p["proof_ref"], LIST_ID, "{p}");
        assert_eq!(p["evidence_ids"], json!(RECORD_IDS));
        assert_eq!(p["anchor"], ANCHOR);
        assert_eq!(p["empty"], false);
        assert_eq!(p["request"], list_request());
    }

    // Every record verified Valid by the fake binary's report: a memory
    // proof, semantically re-executed, at the anchor, with the digests the
    // capture-time record holds; the capture id as proof_ref; the
    // verifier's raw detail kept from the public.
    let verified = payloads(&detail, "evidence_verified");
    assert_eq!(verified.len(), 7, "{detail}");
    let mut verified_ids = ids_in_order(&verified, &["verification", "evidence_id"]);
    verified_ids.sort();
    let mut expected: Vec<String> = RECORD_IDS.iter().map(|i| i.to_string()).collect();
    expected.sort();
    assert_eq!(verified_ids, expected);
    let recorded: Value = serde_json::from_str(&verify_report(LIST_ID)).unwrap();
    for p in &verified {
        let v = &p["verification"];
        assert_eq!(v["integrity_status"], "valid", "{v}");
        assert_eq!(v["verifier"], "hyphae proof verify");
        assert_eq!(v["verified_anchor"], ANCHOR);
        assert_eq!(v["proof_ref"], LIST_ID);
        assert_eq!(v["round"], 1);
        assert_eq!(v["simulated"], false);
        assert!(v.get("detail").is_none(), "operator-only: {v}");
        let report = &v["report"];
        assert_eq!(report["status"], "verified");
        assert_eq!(report["kind"], "memory");
        assert_eq!(report["scope"], "semantic_reexecution");
        assert_eq!(report["anchor_digest"], ANCHOR);
        assert_eq!(report["proof_digest"], recorded["proof_digest"]);
        assert_eq!(report["result_digest"], recorded["result_digest"]);
        assert!(
            DateTime::parse_from_rfc3339(v["checked_at"].as_str().unwrap()).is_ok(),
            "{v}"
        );
    }
    // Seven records, one shared proof: the run's verifier ran the binary
    // once and served the rest from its content-digest cache (HY-13).
    assert_eq!(fx.calls().len(), 3, "{:?}", fx.calls());

    // The public receipt: the same seven verifications, no detail, and
    // nothing resembling a path.
    let response = h
        .server
        .send(visitor(
            Req::get(&format!("/api/v1/runs/{run_id}/receipt")),
            &cookie,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt_text = body_text(response).await;
    let receipt: Value = serde_json::from_str(&receipt_text).unwrap();
    assert_eq!(receipt["final_state"], "awaiting_human_approval");
    assert_eq!(receipt["task"]["mode"], declared);
    let in_receipt = receipt["verifications"].as_array().unwrap();
    assert_eq!(in_receipt.len(), 7);
    let key = |v: &Value| {
        (
            v["evidence_id"].as_str().unwrap().to_string(),
            v["integrity_status"].as_str().unwrap().to_string(),
            v["checked_at"].as_str().unwrap().to_string(),
            v["proof_ref"].as_str().unwrap().to_string(),
        )
    };
    let mut from_receipt: Vec<_> = in_receipt.iter().map(|r| key(&r["verification"])).collect();
    let mut from_detail: Vec<_> = verified.iter().map(|p| key(&p["verification"])).collect();
    from_receipt.sort();
    from_detail.sort();
    assert_eq!(from_receipt, from_detail);
    for r in in_receipt {
        assert_eq!(r["round"], 1);
        assert!(r["verification"].get("detail").is_none(), "{r}");
        assert_eq!(r["verification"]["report"]["kind"], "memory");
    }

    // (c) The verbatim chain of a live-declared run is not published.
    let (code, body, _) = h
        .server
        .json(visitor(
            Req::get(&format!("/api/v1/runs/{run_id}/events.ndjson")),
            &cookie,
        ))
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "ndjson_unavailable");

    // The ledger: one reservation, three draws (one per call, each within
    // the raised input cap), released when the run ended.
    let releases = h.wait_releases(1).await;
    let reservations = h.ledger_records("reservation");
    assert_eq!(reservations.len(), 1);
    assert_eq!(releases[0]["reservation"], reservations[0]["id"]);
    assert_eq!(h.ledger.snapshot().unwrap().unsettled_reservations, 0);
    let draws = h.ledger_records("draw");
    assert_eq!(draws.len(), 3, "{draws:?}");
    let largest_input_bound = draws
        .iter()
        .map(|d| d["input_bound_tokens"].as_u64().unwrap())
        .max()
        .unwrap();
    assert!(largest_input_bound <= INPUT_CAP_TOKENS, "{draws:?}");
    println!("largest per-call input bound over the seven snapshot records: {largest_input_bound} tokens");

    // No proof, witness or container path, no artifact contents and no key
    // in anything the visitor got.
    for (what, text) in [
        ("202", created.to_string()),
        ("detail", detail.to_string()),
        ("receipt", receipt_text),
        ("capabilities", caps.to_string()),
        ("ready", ready.to_string()),
    ] {
        fx.assert_no_path(what, &text);
        assert!(!text.contains(TEST_KEY), "{what}: {text}");
    }
    // The store keeps the operator's artifact paths, never a witness.
    assert!(!h.store_text().contains("synthetic witness bytes"));
}

// ---------------------------------------------------------------------
// Boot failure
// ---------------------------------------------------------------------

#[test]
fn a_tampered_snapshot_does_not_boot_and_live_mode_stays_refused_without_a_factory() {
    let fx = Fixture::new("tampered");
    flip_first_byte(&fx.capture_file(&format!("{LIST_ID}.proof")));
    let cfg = snapshot_config(&fx.dir, &fx.hyphae_bin);

    // The listing catches the flipped byte before anything is verified.
    let mut verifier = verifier_from_config(&cfg, SystemCommandRunner);
    let err = SnapshotFactory::open(cfg.clone(), &mut verifier)
        .expect_err("a flipped proof byte must not boot");
    assert_eq!(err.code, codes::CHECKSUMS_MISMATCH, "{err}");
    assert!(
        err.message.contains(&format!("captures/{LIST_ID}.proof")),
        "{err}"
    );
    assert!(err.to_string().starts_with(codes::CHECKSUMS_MISMATCH));
    assert!(fx.calls().is_empty(), "nothing was handed to the verifier");

    // Consistent tampering (the listing regenerated): the manifest's own
    // digest of the proof catches it, still before any verifier run.
    write_checksums(&fx.dir);
    let err = SnapshotFactory::open(cfg, &mut verifier)
        .expect_err("a proof that is not the manifest's must not boot");
    assert_eq!(err.code, codes::DIGEST_MISMATCH, "{err}");
    assert!(fx.calls().is_empty());

    // What serve-public reports: the variable, the code, the relative file
    // name; never the local directory.
    let local = fx.dir.to_string_lossy().into_owned();
    let settings = [
        ("CONSENSUS_SNAPSHOT_DIR", local.clone()),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR.to_string()),
        ("CONSENSUS_HYPHAE_BIN", fx.hyphae_bin.clone()),
    ];
    let err = SnapshotFactory::from_vars(|name| {
        settings
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    })
    .err()
    .expect("startup fails");
    assert!(
        err.starts_with(&format!(
            "CONSENSUS_SNAPSHOT_DIR: {}: ",
            codes::DIGEST_MISMATCH
        )),
        "{err}"
    );
    assert!(err.contains(&format!("captures/{LIST_ID}.proof")), "{err}");
    assert!(!err.contains(&local), "no local path: {err}");
    assert!(!err.contains("synthetic"), "no contents: {err}");

    // With no factory at all, live mode cannot be enabled: the startup
    // error names the variable.
    let scratch = common::TempDir::new("live-snapshot-startup");
    let secret = scratch.path().join("turnstile.secret");
    std::fs::write(&secret, "1x0000000000000000000000000000000AA\n").unwrap();
    let secret = secret.display().to_string();
    let live = live_config(
        scratch.path(),
        &[
            (vars::LIVE_ENABLED, "true"),
            (vars::TURNSTILE_SECRET_FILE, &secret),
            (vars::TURNSTILE_HOSTNAME, "demo.example.org"),
        ],
    );
    let err = build_live_gate(&live, meta_config("http://127.0.0.1:9/v1"), None)
        .err()
        .expect("live mode must not start without an evidence factory");
    assert!(err.contains("CONSENSUS_SNAPSHOT_DIR"), "{err}");
    assert!(err.contains("not wired"), "{err}");
    assert!(!err.contains(TEST_KEY));
}

// ---------------------------------------------------------------------
// A fresh verifier per run
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_run_verifies_the_proofs_again_with_a_fresh_verifier() {
    let fx = Fixture::new("fresh-verifier");
    let mut replies = one_run();
    replies.extend(one_run());
    let meta = serve(replies);
    let h = live_server(fx.options(), meta_config(&meta.base_url));
    assert_eq!(fx.calls().len(), 2, "boot");
    let cookie = h.session().await;
    let mut checked: Vec<Vec<DateTime<FixedOffset>>> = Vec::new();
    for n in 1..=2usize {
        let (code, created, _) = h.server.json(submit(&cookie)).await;
        assert_eq!(code, StatusCode::ACCEPTED, "run {n}: {created}");
        let run_id = run_id_of(&created);
        let detail = h.server.wait_resting(&cookie, &run_id).await;
        assert_eq!(
            detail["summary"]["state"], "awaiting_human_approval",
            "run {n}: {detail}"
        );
        h.server.wait_released(&run_id).await;
        let times = checked_at(&detail);
        assert_eq!(times.len(), 7, "run {n}");
        // One binary call per run: a verifier shared between runs would
        // have answered the second run from its cache without a call.
        assert_eq!(
            fx.calls().len(),
            2 + n,
            "run {n}: the proof was verified again by a fresh verifier: {:?}",
            fx.calls()
        );
        checked.push(times);
    }
    // Two runs, two sets of verification times: every record of the second
    // run was checked after every record of the first.
    let first_latest = checked[0].iter().max().unwrap();
    let second_earliest = checked[1].iter().min().unwrap();
    assert!(
        second_earliest > first_latest,
        "{second_earliest} is not after {first_latest}"
    );
    assert!(
        checked[0].iter().all(|t| !checked[1].contains(t)),
        "{checked:?}"
    );
    assert_eq!(meta.hits(), 6);
    h.wait_releases(2).await;
    assert_eq!(h.ledger_records("reservation").len(), 2);
}

/// The fixture is built under the system temporary directory: a sanity
/// check that the harness really wrote a snapshot there (so the tests
/// above exercised the loader, not an absent directory).
#[test]
fn the_fixture_snapshot_is_a_complete_directory() {
    let fx = Fixture::new("layout");
    for file in [
        "manifest.json",
        "SHA256SUMS",
        &format!("captures/{LIST_ID}.proof"),
        &format!("captures/{LIST_ID}.witness"),
        &format!("captures/{LIST_ID}.response.json"),
        &format!("captures/{LIST_ID}.verify.json"),
    ] {
        assert!(fx.dir.join(file).is_file(), "{file}");
    }
    assert!(Path::new(&fx.hyphae_bin).is_file());
}
