// SPDX-License-Identifier: Apache-2.0
//! The public live mode over the sealed snapshot (ADR-0008), end to end
//! through the public router: the production `SnapshotFactory` boots from
//! a fixture snapshot directory with the fake `hyphae` binary, the gate
//! admits a visitor (fake Siteverify, temporary ledger, trusted proxy
//! headers), the fake Chat Completions server answers the roles, and the
//! run's public views carry sealed-snapshot evidence proven and verified
//! by that binary, with no proof, witness or container path anywhere. Then
//! a tampered snapshot refusing to boot, the per-run verifier being fresh
//! and pinned, a two-round run under the default per-call input cap, the
//! served files changed after boot (live mode unavailable at once, every
//! admission refused, a run whose files are swapped under it never
//! verified valid), the status cache next to the admission's full
//! re-check, snapshots that do not boot leaving the portal up, and the
//! real `serve-public` binary with live mode disabled and enabled. No
//! Hyphae and no Meta API are reached.

mod common;
mod support;

use axum::http::StatusCode;
use chrono::{DateTime, FixedOffset};
use common::live::{
    live_config, live_server, pinned_snapshot_factory, snapshot_factory, FactoryMode, LiveOptions,
    VISITOR_IP,
};
use common::snapshot::{
    build, config as snapshot_config, copy_dir, edit_manifest, edit_record_text, fake_hyphae,
    flip_first_byte, logged_calls, manifest_sha256, proof_bytes, recompute_digests,
    record_capture_binary, report_printing_wrapper, restore_dir, sha256_hex,
    swap_list_with_isolation, verify_report, write_checksums, SnapshotDir, TempRoot, ANCHOR,
    CAPTURED_AT, ISOLATION_ID, LIST_ID, OTHER, PROJECT, RECORD_IDS, SNAPSHOT_ID,
};
use common::{body_text, Req};
use consensus_api::hyphae::SystemCommandRunner;
use consensus_api::live::config::{defaults, vars};
use consensus_api::live::gate::DeliberationFactory;
use consensus_api::live::snapshot_factory::{
    status as factory_status, unavailable_line, LogSink, SnapshotFactory, OK_AGAIN_LINE,
    UNUSED_NOTICE,
};
use consensus_api::meta::MetaModelClient;
use consensus_api::server::build_live_gate;
use consensus_api::snapshot::{
    codes, verifier_from_config, SnapshotMemoryClient, NOT_THE_BOOT_ARTIFACTS,
};
use consensus_domain::EvidenceMode;
use consensus_runtime::RunLimits;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::{config as meta_config, envelope, serve, Reply, TEST_KEY};

/// The reference preset: project `demo-repo`, the snapshot's project.
const PRESET: &str = "R01-reference";
/// A Turnstile token; the scripted Siteverify accepts any.
const TOKEN: &str = "tok-snapshot-1";
/// The record the fake Evaluator cites: the fact that payment client
/// timeouts correlate with retry bursts above 5 attempts.
const CITED: &str = RECORD_IDS[1];
/// The text a tampered snapshot would serve instead of the 2026-09
/// decision (review R6): the proofs untouched, which do not bind the text.
const EDITED_TEXT: &str =
    "Decision (edited after capture): raise the payment-client retry cap to 10 attempts.";
/// The explicit per-call input cap (tokens) of the first tests, which are
/// about paths and verification, not the budget: below the production
/// default (32,000, [`defaults::INPUT_CAP_TOKENS`]), it keeps their bound on
/// the measured one-round figure. The default-cap run is
/// [`a_two_round_run_over_the_snapshot_fits_the_default_input_cap`].
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

/// A fixture snapshot directory with the fake `hyphae` binary next to it,
/// removed with it when the fixture is dropped. The fake binary is the
/// snapshot's capture build: its real SHA-256 is in the manifest
/// (`hyphae.binary_sha256`), and the fixture boots, as production does,
/// with the verifier binary and the manifest pinned.
struct Fixture {
    snapshot: SnapshotDir,
    dir: PathBuf,
    hyphae_bin: String,
    /// The fake binary's SHA-256 (`CONSENSUS_HYPHAE_BIN_SHA256`).
    hyphae_bin_sha256: String,
    log: PathBuf,
}

impl Fixture {
    fn new(test: &str) -> Self {
        Self::of(build(&format!("live-{test}")))
    }

    /// The fake binary next to an already assembled snapshot, recorded in
    /// its manifest as the capture build.
    fn of(snapshot: SnapshotDir) -> Self {
        let dir = snapshot.path().to_path_buf();
        let (hyphae_bin, log) = fake_hyphae(snapshot.root());
        let hyphae_bin_sha256 = record_capture_binary(&dir, &hyphae_bin);
        Self {
            snapshot,
            dir,
            hyphae_bin,
            hyphae_bin_sha256,
            log,
        }
    }

    /// The production factory over this snapshot, both digests pinned (the
    /// manifest as it is now).
    fn factory_mode(&self) -> FactoryMode {
        FactoryMode::Snapshot {
            dir: self.dir.clone(),
            verifier_dir: None,
            hyphae_bin: self.hyphae_bin.clone(),
            anchor: ANCHOR.into(),
            manifest_sha256: Some(manifest_sha256(&self.dir)),
            hyphae_bin_sha256: Some(self.hyphae_bin_sha256.clone()),
        }
    }

    /// The harness over the production factory on this snapshot, with the
    /// explicit 16,000-token per-call input cap of [`INPUT_CAP_TOKENS`]
    /// (the ledger bounds a call's input at `request bytes + 1024` tokens;
    /// the Evaluator's request over the seven snapshot records, their real
    /// texts, measures about 8,100 in one round).
    fn options(&self) -> LiveOptions {
        LiveOptions {
            live_env: vec![(vars::INPUT_CAP_TOKENS, INPUT_CAP_TOKENS.to_string())],
            ..self.default_options()
        }
    }

    /// The harness over the production factory with every live setting at
    /// its production default, the per-call input cap included.
    fn default_options(&self) -> LiveOptions {
        LiveOptions {
            factory: self.factory_mode(),
            ..LiveOptions::default()
        }
    }

    /// The snapshot variables `serve-public` reads for this fixture, as
    /// production sets them: both digests pinned (the manifest as it is
    /// now, the fake binary as recorded).
    fn vars(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "CONSENSUS_SNAPSHOT_DIR",
                self.dir.to_string_lossy().into_owned(),
            ),
            ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR.into()),
            ("CONSENSUS_HYPHAE_BIN", self.hyphae_bin.clone()),
            (
                "CONSENSUS_SNAPSHOT_MANIFEST_SHA256",
                manifest_sha256(&self.dir),
            ),
            (
                "CONSENSUS_HYPHAE_BIN_SHA256",
                self.hyphae_bin_sha256.clone(),
            ),
        ]
    }

    /// [`vars`](Self::vars) without the two digest pins.
    fn unpinned_vars(&self) -> Vec<(&'static str, String)> {
        self.vars()
            .into_iter()
            .filter(|(k, _)| !k.ends_with("_SHA256"))
            .collect()
    }

    /// [`vars`](Self::vars) with `name` set to `value` (added when absent).
    fn vars_with(&self, name: &'static str, value: &str) -> Vec<(&'static str, String)> {
        let mut vars: Vec<_> = self
            .vars()
            .into_iter()
            .filter(|(k, _)| *k != name)
            .collect();
        vars.push((name, value.to_string()));
        vars
    }

    /// A copy of the snapshot as it is now, next to it.
    fn backup(&self) -> PathBuf {
        let backup = self.snapshot.root().join("backup");
        copy_dir(&self.dir, &backup);
        backup
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
    // And it ran it against the operator-pinned anchor.
    for call in &fx.calls()[2..] {
        assert!(call.starts_with("proof verify --proof"), "{call}");
        assert!(call.contains(&format!("--anchor {ANCHOR}")), "{call}");
    }

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
    // The run's verifier was the pinned one: the record's anchor is the
    // same value, so only the verifier's own detail (operator-only, in the
    // store) tells a pinned check from a self-consistency check.
    let stored = h.store_text();
    assert!(
        stored.contains("the anchor is the operator-pinned one"),
        "the run's verifier was not pinned"
    );
    assert!(!stored.contains("none pinned"));
    assert_eq!(
        snapshot.run_verifier().inner().expected_anchor.as_deref(),
        Some(ANCHOR)
    );
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

    // What the strict entry point reports (`SnapshotFactory::from_vars`;
    // serve-public uses the degrading one below): the variable, the code,
    // the relative file name; never the local directory.
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

    // What serve-public makes of it with live mode enabled: not an error
    // (the portal stays up), but evidence that is not ok, with the code
    // and nothing else, logged once.
    let lines = Arc::new(Mutex::new(Vec::new()));
    let evidence = SnapshotFactory::live_evidence_from_vars(
        |name| {
            settings
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.clone())
        },
        collector(&lines),
    )
    .expect("a snapshot that does not boot is not a configuration error")
    .expect("a configured snapshot");
    let status = evidence.status();
    assert!(!status.ok);
    assert_eq!(status.kind, EvidenceMode::SealedSnapshot);
    assert_eq!(status.detail.as_deref(), Some(codes::DIGEST_MISMATCH));
    assert_eq!(evidence.evidence_mode(), EvidenceMode::SealedSnapshot);
    assert_eq!(
        *lines.lock().unwrap(),
        [unavailable_line(codes::DIGEST_MISMATCH)]
    );
    assert!(fx.calls().is_empty(), "still nothing was verified");

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
    // Each run's verifier was given the operator-pinned anchor.
    for call in &fx.calls()[2..] {
        assert!(call.contains(&format!("--anchor {ANCHOR}")), "{call}");
    }
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

// ---------------------------------------------------------------------
// Helpers of the tests below
// ---------------------------------------------------------------------

/// A log sink collecting the `[snapshot]` lines into `lines`.
fn collector(lines: &Arc<Mutex<Vec<String>>>) -> LogSink {
    let lines = lines.clone();
    Arc::new(move |line: &str| lines.lock().unwrap().push(line.to_string()))
}

/// A variable lookup over `vars`.
fn lookup<'a>(vars: &'a [(&'static str, String)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    }
}

/// A model client the factory can build a run around; never called.
fn unused_model() -> MetaModelClient {
    MetaModelClient::new(meta_config("http://127.0.0.1:9/v1")).expect("model client")
}

/// Waits (up to ten seconds) until `done` holds.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A snapshot whose list capture's capture-time record names another
/// proof digest than the verifier reports: everything else is consistent
/// (the manifest's digests, `SHA256SUMS`), so it loads, and its boot
/// verification fails (`boot_verification_failed`).
fn unverifiable(test: &str) -> SnapshotDir {
    let snapshot = build(&format!("live-{test}"));
    let record = snapshot
        .join("captures")
        .join(format!("{LIST_ID}.verify.json"));
    let mut report: Value =
        serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    report["proof_digest"] = json!("ab".repeat(32));
    std::fs::write(&record, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    recompute_digests(&snapshot);
    snapshot
}

/// The round-1 Critic: one objection to the claim (`item-3`; the
/// challenge is `chal-4`).
const DRAFT_CHALLENGE_ONE_ITEM: &str = r#"{"items":[{"kind":"missing_test","claim_id":"claim-2","description":"No load test covers the jitter range","evidence_refs":[]}]}"#;
/// The revision: the same claim (now `claim-6` of `prop-5`) and a plan
/// with the test, the objection addressed.
const DRAFT_REVISION: &str = r#"{"summary":"Keep retries capped at 3 attempts with jitter and add a load test","plan_steps":["add jitter to the payment client backoff","add a load test for the jitter range"],"claims":[{"statement":"Payment timeouts correlate with retry bursts above 5 attempts"}],"assumptions":[],"uncertainties":[],"addressed_items":["item-3"]}"#;

/// An evaluation corroborating `claim` from the cited record, with one
/// verdict on `item-3`.
fn evaluation_with_verdict(claim: &str, status: &str, reason: &str) -> String {
    let mut assessments = serde_json::Map::new();
    assessments.insert(
        claim.to_string(),
        json!({
            "corroborated": true,
            "relevant_evidence": [CITED],
            "contradicting_evidence": [],
            "note": "the fact record states the correlation"
        }),
    );
    json!({
        "assessments": assessments,
        "item_verdicts": {"item-3": {"status": status, "reason": reason}},
        "cleared_contradictions": [],
        "abstain": false,
        "abstain_reason": null
    })
    .to_string()
}

/// The six answers of a two-round run: the Critic objects, the Evaluator
/// leaves the objection open, the Proposer revises, the second Critic is
/// silent and the second Evaluator resolves it. Supported in round 2.
fn two_rounds() -> Vec<Reply> {
    vec![
        Reply::ok(envelope(DRAFT_PROPOSAL)),
        Reply::ok(envelope(DRAFT_CHALLENGE_ONE_ITEM)),
        Reply::ok(envelope(&evaluation_with_verdict(
            "claim-2",
            "unresolved",
            "no load test covers the jitter range yet",
        ))),
        Reply::ok(envelope(DRAFT_REVISION)),
        Reply::ok(envelope(DRAFT_CHALLENGE)),
        Reply::ok(envelope(&evaluation_with_verdict(
            "claim-6",
            "resolved",
            "the revision adds the load test",
        ))),
    ]
}

// ---------------------------------------------------------------------
// The default per-call input cap (review B1)
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_two_round_run_over_the_snapshot_fits_the_default_input_cap() {
    let fx = Fixture::new("default-cap");
    let meta = serve(two_rounds());
    // No override: every live setting, the input cap included, is the
    // production default.
    let h = live_server(fx.default_options(), meta_config(&meta.base_url));
    let cap: u64 = defaults::INPUT_CAP_TOKENS.parse().unwrap();
    assert_eq!(cap, 32_000);
    let cookie = h.session().await;
    let (code, created, _) = h.server.json(submit(&cookie)).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(
        detail["summary"]["state"], "awaiting_human_approval",
        "{detail}"
    );
    assert_eq!(detail["summary"]["recommendation"], "supported", "{detail}");
    assert_eq!(
        meta.hits(),
        6,
        "propose, critique, evaluate, revise, critique, evaluate"
    );
    // Through evaluate and revise, twice evaluated over the whole proven set.
    assert_eq!(payloads(&detail, "revision_submitted").len(), 1, "{detail}");
    assert_eq!(payloads(&detail, "evaluation_submitted").len(), 2);
    let verified = payloads(&detail, "evidence_verified");
    assert_eq!(verified.len(), 14, "seven records, two rounds");
    for p in &verified {
        assert_eq!(p["verification"]["integrity_status"], "valid", "{p}");
    }
    assert!(!detail.to_string().contains("budget_refused"), "{detail}");

    // The ledger: the reservation is the default worst case at the
    // harness's 12k output bound, (70,000 + 12,000 + 32,000) × $20/M, and
    // every one of the six calls was drawn within the default cap.
    let releases = h.wait_releases(1).await;
    let reservations = h.ledger_records("reservation");
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0]["worst_case_usd"], "2.280000");
    assert_eq!(releases[0]["reservation"], reservations[0]["id"]);
    let bounds: Vec<u64> = h
        .ledger_records("draw")
        .iter()
        .map(|d| d["input_bound_tokens"].as_u64().unwrap())
        .collect();
    assert_eq!(bounds.len(), 6, "{bounds:?}");
    let largest = *bounds.iter().max().unwrap();
    println!(
        "per-call input bounds over the seven snapshot records (propose, critique, evaluate, revise, critique, evaluate): {bounds:?}; largest {largest} tokens under the default cap of {cap}"
    );
    assert!(largest <= cap, "{bounds:?}");
    // The former default (8,000) would have refused this run part way.
    assert!(largest > 8_000, "{bounds:?}");
}

// ---------------------------------------------------------------------
// The served files changed after boot (review H1, L1 T1/T2)
// ---------------------------------------------------------------------

/// One way the served files can change after boot, and the code the
/// factory then reports.
type Tamper = (&'static str, &'static str, fn(&Path));

fn tampering() -> Vec<Tamper> {
    vec![
        (
            "the list witness deleted",
            codes::FILE_UNREADABLE,
            |dir: &Path| {
                std::fs::remove_file(dir.join("captures").join(format!("{LIST_ID}.witness")))
                    .unwrap()
            },
        ),
        (
            "the list proof truncated",
            codes::DIGEST_MISMATCH,
            |dir: &Path| {
                let proof = dir.join("captures").join(format!("{LIST_ID}.proof"));
                let bytes = std::fs::read(&proof).unwrap();
                std::fs::write(&proof, &bytes[..bytes.len() / 2]).unwrap();
            },
        ),
        (
            "manifest.json edited",
            codes::DIGEST_MISMATCH,
            |dir: &Path| edit_manifest(dir, |m| m["captured_at"] = json!("2026-10-30T00:00:00Z")),
        ),
        (
            "the isolation proof deleted",
            codes::FILE_UNREADABLE,
            |dir: &Path| {
                std::fs::remove_file(dir.join("captures").join(format!("{ISOLATION_ID}.proof")))
                    .unwrap()
            },
        ),
        (
            "the isolation capture's proof and witness copied over the list capture's",
            codes::DIGEST_MISMATCH,
            |dir: &Path| swap_list_with_isolation(dir),
        ),
        ("captures/ deleted", codes::FILE_UNREADABLE, |dir: &Path| {
            std::fs::remove_dir_all(dir.join("captures")).unwrap()
        }),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_changed_after_boot_make_live_mode_unavailable_at_once_without_a_path() {
    let fx = Fixture::new("changed-after-boot");
    let backup = fx.backup();
    let meta = serve(one_run());
    let h = live_server(fx.default_options(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    let kinds = tampering();
    for (what, code, tamper) in &kinds {
        tamper(&fx.dir);
        // The factory's status says so, with the stable code.
        let status = h.factory.status();
        assert!(!status.ok, "{what}");
        assert_eq!(status.detail.as_deref(), Some(*code), "{what}");
        // /ready: not ready, the evidence not ok, the ledger fine.
        let (status, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
        assert_eq!(status, StatusCode::OK, "{what}: {ready}");
        assert_eq!(ready["ready"], false, "{what}: {ready}");
        assert_eq!(ready["live"]["evidence_ok"], false, "{what}: {ready}");
        assert_eq!(ready["live"]["ledger_ok"], true, "{what}: {ready}");
        // /capabilities: live mode disabled, as on a server without it.
        let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
        assert_eq!(caps["live_enabled"], false, "{what}: {caps}");
        assert!(caps.get("live").is_none(), "{what}: {caps}");
        assert_eq!(caps["guided_enabled"], true, "{what}");
        // An admission: 503 live_unavailable, before Turnstile.
        let (status, body, headers) = h.server.json(submit(&cookie)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{what}: {body}");
        assert_eq!(body["code"], "live_unavailable", "{what}: {body}");
        assert!(headers.get("retry-after").is_some(), "{what}");
        for (part, text) in [
            ("ready", ready.to_string()),
            ("capabilities", caps.to_string()),
            ("503", body.to_string()),
        ] {
            fx.assert_no_path(&format!("{what}: {part}"), &text);
            assert!(
                !text.contains(code),
                "{what}: the code is the log's: {text}"
            );
        }
        // Restored: ok again at once.
        restore_dir(&fx.dir, &backup);
        assert!(h.factory.status().ok, "{what}: restored");
        let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
        assert_eq!(ready["ready"], true, "{what}: restored: {ready}");
        assert_eq!(ready["live"]["evidence_ok"], true);
        let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
        assert_eq!(caps["live_enabled"], true, "{what}: restored: {caps}");
        assert_eq!(caps["live"]["presets"].as_array().unwrap().len(), 5);
    }
    // Nothing reached Turnstile, the ledger or the model.
    assert!(h.siteverify.calls().is_empty(), "no token was spent");
    assert!(h.ledger_records("reservation").is_empty());
    assert_eq!(meta.hits(), 0);
    // One line per change between ok and not ok, with the code only.
    let mut expected = Vec::new();
    for (_, code, _) in &kinds {
        expected.push(unavailable_line(code));
        expected.push(OK_AGAIN_LINE.to_string());
    }
    let lines = h.evidence_lines();
    assert_eq!(lines, expected);
    for line in &lines {
        fx.assert_no_path("log line", line);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_record_text_or_a_report_edited_after_boot_makes_live_mode_unavailable_at_once() {
    // Review R6: the served captures' responses and capture-time reports
    // are re-hashed with the proofs and witnesses, so an edit after boot
    // turns live mode unavailable at once, with the code in the log only.
    let edits: Vec<Tamper> = vec![
        (
            "a record text of the list response edited",
            codes::DIGEST_MISMATCH,
            |dir: &Path| edit_record_text(dir, LIST_ID, 0, EDITED_TEXT),
        ),
        (
            "the isolation capture-time report edited",
            codes::DIGEST_MISMATCH,
            |dir: &Path| {
                let path = dir
                    .join("captures")
                    .join(format!("{ISOLATION_ID}.verify.json"));
                let mut report: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                report["total_file_bytes"] = json!(1);
                std::fs::write(&path, report.to_string()).unwrap();
            },
        ),
    ];
    let fx = Fixture::new("edited-after-boot");
    let backup = fx.backup();
    let meta = serve(one_run());
    let h = live_server(fx.default_options(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    for (what, code, edit) in &edits {
        edit(&fx.dir);
        let status = h.factory.status();
        assert!(!status.ok, "{what}");
        assert_eq!(status.detail.as_deref(), Some(*code), "{what}");
        let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
        assert_eq!(ready["ready"], false, "{what}: {ready}");
        assert_eq!(ready["live"]["evidence_ok"], false, "{what}: {ready}");
        let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
        assert_eq!(caps["live_enabled"], false, "{what}: {caps}");
        let (status, body, _) = h.server.json(submit(&cookie)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{what}: {body}");
        assert_eq!(body["code"], "live_unavailable", "{what}: {body}");
        for (part, text) in [
            ("ready", ready.to_string()),
            ("capabilities", caps.to_string()),
            ("503", body.to_string()),
        ] {
            fx.assert_no_path(&format!("{what}: {part}"), &text);
            assert!(!text.contains(code), "{what}: {part}: {text}");
            assert!(!text.contains(EDITED_TEXT), "{what}: {part}: {text}");
        }
        restore_dir(&fx.dir, &backup);
        assert!(h.factory.status().ok, "{what}: restored");
        let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
        assert_eq!(ready["live"]["evidence_ok"], true, "{what}: restored");
    }
    assert!(h.siteverify.calls().is_empty(), "no token was spent");
    assert!(h.ledger_records("reservation").is_empty());
    assert_eq!(meta.hits(), 0);
    let mut expected = Vec::new();
    for (_, code, _) in &edits {
        expected.push(unavailable_line(code));
        expected.push(OK_AGAIN_LINE.to_string());
    }
    assert_eq!(h.evidence_lines(), expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_isolation_artifacts_copied_over_the_list_capture_are_never_verified_valid() {
    // (a) After boot, while /ready still holds its cached read: the
    // admission's own full re-check refuses before any model call.
    let fx = Fixture::new("swap-at-admission");
    let meta = serve(one_run());
    let h = live_server(
        LiveOptions {
            snapshot_status_ttl: Duration::from_secs(3600),
            ..fx.default_options()
        },
        meta_config(&meta.base_url),
    );
    let cookie = h.session().await;
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["live"]["evidence_ok"], true, "{ready}");
    swap_list_with_isolation(&fx.dir);
    assert!(h.factory.status().ok, "the cached read, an hour long here");
    let (status, body, _) = h.server.json(submit(&cookie)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_unavailable");
    fx.assert_no_path("503", &body.to_string());
    assert_eq!(
        h.siteverify.calls().len(),
        1,
        "the cached status let the request reach Turnstile"
    );
    // The reservation was taken and released; nothing was drawn, no model
    // was called, no run exists.
    let releases = h.wait_releases(1).await;
    let reservations = h.ledger_records("reservation");
    assert_eq!(reservations.len(), 1);
    assert_eq!(releases[0]["reservation"], reservations[0]["id"]);
    assert!(h.ledger_records("draw").is_empty());
    assert_eq!(meta.hits(), 0);
    let (_, list, _) = h
        .server
        .json(visitor(Req::get("/api/v1/runs"), &cookie))
        .await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 0, "{list}");
    // The re-check refreshed the status the routes read.
    let status = h.factory.status();
    assert!(!status.ok);
    assert_eq!(status.detail.as_deref(), Some(codes::DIGEST_MISMATCH));
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], false, "{ready}");
    assert_eq!(ready["live"]["evidence_ok"], false);
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live_enabled"], false, "{caps}");
    assert_eq!(
        h.evidence_lines(),
        [unavailable_line(codes::DIGEST_MISMATCH)]
    );

    // (b) Swapped while a run is under way, after its admission and before
    // its first verification: every record is Invalid with the fixed
    // detail, the run is blocked, and nothing says valid.
    let fx = Fixture::new("swap-mid-run");
    let mut replies = one_run();
    replies[0] = Reply::ok(envelope(DRAFT_PROPOSAL)).delayed(Duration::from_millis(1500));
    let meta = serve(replies);
    let h = live_server(fx.default_options(), meta_config(&meta.base_url));
    let cookie = h.session().await;
    let (status, created, _) = h.server.json(submit(&cookie)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let run_id = run_id_of(&created);
    // The proposal call is in flight (the fake server holds its answer).
    wait_until("the proposal call", || meta.hits() >= 1).await;
    swap_list_with_isolation(&fx.dir);
    let detail = h.server.wait_resting(&cookie, &run_id).await;
    assert_eq!(detail["summary"]["recommendation"], "blocked", "{detail}");
    let isolation: Value = serde_json::from_str(&verify_report(ISOLATION_ID)).unwrap();
    let verified = payloads(&detail, "evidence_verified");
    assert_eq!(verified.len(), 7, "{detail}");
    for p in &verified {
        let v = &p["verification"];
        assert_eq!(v["integrity_status"], "invalid", "{v}");
        assert_eq!(v["proof_ref"], LIST_ID);
        // What the verifier saw: the isolation capture's proof.
        assert_eq!(
            v["report"]["proof_digest"], isolation["proof_digest"],
            "{v}"
        );
        assert!(v.get("detail").is_none(), "operator-only: {v}");
    }
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
    let in_receipt = receipt["verifications"].as_array().unwrap();
    assert_eq!(in_receipt.len(), 7);
    for r in in_receipt {
        assert_eq!(r["verification"]["integrity_status"], "invalid", "{r}");
    }
    for (what, text) in [("detail", detail.to_string()), ("receipt", receipt_text)] {
        assert!(!text.contains("\"valid\""), "{what} says valid: {text}");
        fx.assert_no_path(what, &text);
    }
    // The operator's record carries the fixed reason.
    assert!(h.store_text().contains(NOT_THE_BOOT_ARTIFACTS));
    assert_eq!(meta.hits(), 3, "a blocked run is not revised");
    h.wait_releases(1).await;
    // And live mode is unavailable from then on.
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["live"]["evidence_ok"], false, "{ready}");
}

// ---------------------------------------------------------------------
// The status cache and the admission's full re-check (review M2, H1a)
// ---------------------------------------------------------------------

#[test]
fn the_status_is_cached_and_every_admission_rechecks_the_files_in_full() {
    let fx = Fixture::new("status-cache");
    let lines = Arc::new(Mutex::new(Vec::new()));
    let factory = snapshot_factory(&fx.dir, None, &fx.hyphae_bin, ANCHOR)
        .with_status_ttl(Duration::from_secs(3600))
        .with_log(collector(&lines));
    assert!(factory.status().ok);
    let proof = fx.capture_file(&format!("{LIST_ID}.proof"));
    flip_first_byte(&proof);
    // Within the cache's life a status read re-hashes nothing: a flood of
    // /ready cannot make the server hash the artifacts at will.
    for _ in 0..50 {
        assert!(factory.status().ok, "the cached read");
    }
    assert!(lines.lock().unwrap().is_empty());
    // An admission re-checks in full, whatever the cache says, and builds
    // no run over changed files.
    let err = factory
        .build(unused_model(), RunLimits::default())
        .err()
        .expect("changed files build no run");
    assert!(err.0.contains("changed since boot"), "{err}");
    assert!(
        !err.0.contains(codes::DIGEST_MISMATCH),
        "the code is logged once, by the factory's status line: {err}"
    );
    fx.assert_no_path("factory error", &err.0);
    // ...and refreshes the status the routes read.
    let status = factory.status();
    assert!(!status.ok);
    assert_eq!(status.detail.as_deref(), Some(codes::DIGEST_MISMATCH));
    assert_eq!(
        *lines.lock().unwrap(),
        [unavailable_line(codes::DIGEST_MISMATCH)]
    );
    // Restored: the cached read lags until the next admission looks again.
    flip_first_byte(&proof);
    assert!(!factory.status().ok);
    assert!(factory.build(unused_model(), RunLimits::default()).is_ok());
    assert!(factory.status().ok);
    assert_eq!(
        *lines.lock().unwrap(),
        [
            unavailable_line(codes::DIGEST_MISMATCH),
            OK_AGAIN_LINE.to_string()
        ]
    );

    // Without a cache every read looks at the files.
    let uncached = snapshot_factory(&fx.dir, None, &fx.hyphae_bin, ANCHOR)
        .with_status_ttl(Duration::ZERO)
        .with_log(collector(&Arc::new(Mutex::new(Vec::new()))));
    assert!(uncached.status().ok);
    flip_first_byte(&proof);
    assert!(!uncached.status().ok);
    flip_first_byte(&proof);
    assert!(uncached.status().ok);

    // The per-run verifier: pinned, and bound to what boot accepted.
    assert_eq!(
        factory.run_verifier().inner().expected_anchor.as_deref(),
        Some(ANCHOR)
    );
}

// ---------------------------------------------------------------------
// Snapshots that do not boot (review M1, M3, L1, L2)
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snapshot_that_fails_boot_verification_leaves_the_portal_up_with_live_mode_unavailable() {
    let fx = Fixture::of(unverifiable("boot-degrade"));
    let meta = serve(one_run());
    // What serve-public does with live mode enabled and these variables.
    let h = live_server(
        LiveOptions {
            factory: FactoryMode::SnapshotFromVars(fx.vars()),
            ..LiveOptions::default()
        },
        meta_config(&meta.base_url),
    );
    assert!(h.snapshot.is_none(), "nothing booted");
    let status = h.factory.status();
    assert!(!status.ok);
    assert_eq!(status.kind, EvidenceMode::SealedSnapshot);
    assert_eq!(
        status.detail.as_deref(),
        Some(codes::BOOT_VERIFICATION_FAILED)
    );
    assert_eq!(h.factory.evidence_mode(), EvidenceMode::SealedSnapshot);
    assert_eq!(fx.calls().len(), 1, "the list capture was refused");

    // The portal serves: the static site, the showcase, guided runs.
    let response = h.server.send(Req::get("/")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = h.server.send(Req::get("/api/v1/showcase")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = h.session().await;
    let (status, created, _) = h
        .server
        .json(visitor(Req::post("/api/v1/runs"), &cookie).json(&json!({"scenario": "retry"})))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let guided = h.server.wait_resting(&cookie, &run_id_of(&created)).await;
    assert_ne!(guided["summary"]["state"], "failed", "{guided}");

    // Live mode: refused before Turnstile, nothing reserved, no model.
    let (status, body, _) = h.server.json(submit(&cookie)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "live_unavailable");
    assert!(h.siteverify.calls().is_empty());
    assert!(h.ledger_records("reservation").is_empty());
    assert_eq!(meta.hits(), 0);
    // /ready says why live mode is down; /capabilities says it is off.
    let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
    assert_eq!(ready["ready"], false, "{ready}");
    assert_eq!(ready["live"]["evidence_ok"], false);
    assert_eq!(ready["live"]["ledger_ok"], true);
    assert_eq!(ready["live"]["tripped"], false);
    let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
    assert_eq!(caps["live_enabled"], false, "{caps}");
    assert!(caps.get("live").is_none(), "{caps}");
    assert_eq!(caps["guided_enabled"], true);
    for (what, text) in [
        ("503", body.to_string()),
        ("ready", ready.to_string()),
        ("capabilities", caps.to_string()),
    ] {
        fx.assert_no_path(what, &text);
        assert!(
            !text.contains(codes::BOOT_VERIFICATION_FAILED),
            "{what}: {text}"
        );
    }
    // The code was logged once, at boot, and only the code.
    assert_eq!(
        h.evidence_lines(),
        [unavailable_line(codes::BOOT_VERIFICATION_FAILED)]
    );
}

#[test]
fn a_snapshot_of_another_project_does_not_boot_for_live_mode() {
    let fx = Fixture::new("other-project");
    // A consistent snapshot of `payments-repo`: its records, its manifest
    // (the response's digest included), its list capture.
    let response = fx.capture_file(&format!("{LIST_ID}.response.json"));
    let text = std::fs::read_to_string(&response).unwrap().replace(
        "\"project\": \"demo-repo\"",
        "\"project\": \"payments-repo\"",
    );
    std::fs::write(&response, text).unwrap();
    edit_manifest(&fx.dir, |m| {
        m["project"] = json!("payments-repo");
        m["captures"][0]["arguments"]["project"] = json!("payments-repo");
    });
    recompute_digests(&fx.dir);
    let cfg = snapshot_config(&fx.dir, &fx.hyphae_bin);
    let mut verifier = verifier_from_config(&cfg, SystemCommandRunner);
    // The snapshot client serves any one project...
    let client = SnapshotMemoryClient::open(&cfg, &mut verifier).expect("a sound snapshot");
    assert_eq!(client.project().as_str(), "payments-repo");
    let verified = fx.calls().len();
    // ...but every public live preset runs against demo-repo: the factory
    // refuses it, before running the verifier.
    let err = SnapshotFactory::open(cfg, &mut verifier)
        .expect_err("another project must not boot for live mode");
    assert_eq!(err.code, codes::PROJECT_MISMATCH, "{err}");
    assert!(
        err.message.contains("payments-repo") && err.message.contains("demo-repo"),
        "{err}"
    );
    assert_eq!(fx.calls().len(), verified, "nothing verified for it");
    // serve-public: the portal up, live mode unavailable with that code.
    let lines = Arc::new(Mutex::new(Vec::new()));
    let vars = fx.vars();
    let evidence = SnapshotFactory::live_evidence_from_vars(lookup(&vars), collector(&lines))
        .unwrap()
        .unwrap();
    assert_eq!(
        evidence.status().detail.as_deref(),
        Some(codes::PROJECT_MISMATCH)
    );
    assert_eq!(
        *lines.lock().unwrap(),
        [unavailable_line(codes::PROJECT_MISMATCH)]
    );
}

#[test]
fn a_proof_tampered_with_consistently_is_refused_by_the_verifier_on_every_platform() {
    // The list proof's first byte flipped, then every digest regenerated
    // (manifest and SHA256SUMS): only the verifier can tell, and the fake
    // one keys its reports by the proof's whole content, as Hyphae's
    // digests follow it (the Windows script used to key them by one word).
    let fx = Fixture::new("regenerated-proof");
    flip_first_byte(&fx.capture_file(&format!("{LIST_ID}.proof")));
    recompute_digests(&fx.dir);
    let cfg = snapshot_config(&fx.dir, &fx.hyphae_bin);
    let mut verifier = verifier_from_config(&cfg, SystemCommandRunner);
    let err =
        SnapshotFactory::open(cfg, &mut verifier).expect_err("a tampered proof must not boot");
    assert_eq!(err.code, codes::BOOT_VERIFICATION_FAILED, "{err}");
    assert_eq!(
        fx.calls().len(),
        1,
        "the verifier saw the list proof and refused it"
    );
}

// ---------------------------------------------------------------------
// The record texts, the manifest and the verifier binary bound (review R6)
// ---------------------------------------------------------------------

#[test]
fn the_startup_line_says_whether_the_manifest_and_the_verifier_binary_are_pinned() {
    let fx = Fixture::new("startup-pins");
    let manifest = manifest_sha256(&fx.dir);
    let binary = fx.hyphae_bin_sha256.clone();
    // Pinned, as in production: each digest's prefix, never the digest.
    let factory = pinned_snapshot_factory(
        &fx.dir,
        None,
        &fx.hyphae_bin,
        ANCHOR,
        Some(&manifest),
        Some(&binary),
    );
    let line = factory.startup_line();
    assert!(
        line.contains(&format!("manifest sha256 pinned {}…", &manifest[..12])),
        "{line}"
    );
    assert!(
        line.contains(&format!("verifier binary sha256 pinned {}…", &binary[..12])),
        "{line}"
    );
    assert!(!line.contains("not pinned"), "{line}");
    assert!(
        !line.contains(&manifest) && !line.contains(&binary),
        "prefixes only: {line}"
    );
    fx.assert_no_path("pinned startup line", &line);
    // Unpinned: said so, with the variable that would pin it.
    let line = snapshot_factory(&fx.dir, None, &fx.hyphae_bin, ANCHOR).startup_line();
    assert!(
        line.contains("manifest not pinned (CONSENSUS_SNAPSHOT_MANIFEST_SHA256 unset)"),
        "{line}"
    );
    assert!(
        line.contains("verifier binary not pinned (CONSENSUS_HYPHAE_BIN_SHA256 unset)"),
        "{line}"
    );
    fx.assert_no_path("unpinned startup line", &line);
    // One pinned, the other not.
    let line = pinned_snapshot_factory(&fx.dir, None, &fx.hyphae_bin, ANCHOR, None, Some(&binary))
        .startup_line();
    assert!(
        line.contains("manifest not pinned") && line.contains("verifier binary sha256 pinned"),
        "{line}"
    );
}

/// How a snapshot breaks its binding before boot: what is done to the
/// fixture, and the variables `serve-public` then gets.
type Unbind = fn(&Fixture) -> Vec<(&'static str, String)>;

fn unbound() -> Vec<(&'static str, &'static str, Unbind)> {
    vec![
        (
            "a record text edited, SHA256SUMS regenerated",
            codes::DIGEST_MISMATCH,
            |fx: &Fixture| {
                edit_record_text(&fx.dir, LIST_ID, 0, EDITED_TEXT);
                write_checksums(&fx.dir);
                fx.vars()
            },
        ),
        (
            "a record text edited, the manifest's digests and SHA256SUMS rewritten to match, the published manifest pinned",
            codes::MANIFEST_PIN_MISMATCH,
            |fx: &Fixture| {
                let published = manifest_sha256(&fx.dir);
                edit_record_text(&fx.dir, LIST_ID, 0, EDITED_TEXT);
                recompute_digests(&fx.dir);
                fx.vars_with("CONSENSUS_SNAPSHOT_MANIFEST_SHA256", &published)
            },
        ),
        (
            "another manifest digest pinned",
            codes::MANIFEST_PIN_MISMATCH,
            |fx: &Fixture| fx.vars_with("CONSENSUS_SNAPSHOT_MANIFEST_SHA256", &"ab".repeat(32)),
        ),
        (
            "the verifier binary replaced by a wrapper printing the capture-time reports",
            codes::VERIFIER_BINARY_MISMATCH,
            |fx: &Fixture| {
                let (wrapper, _) = report_printing_wrapper(fx.snapshot.root());
                fx.vars_with("CONSENSUS_HYPHAE_BIN", &wrapper)
            },
        ),
        (
            "a snapshot captured with another Hyphae build",
            codes::CAPTURE_BINARY_MISMATCH,
            |fx: &Fixture| {
                edit_manifest(&fx.dir, |m| {
                    m["hyphae"]["binary_sha256"] = json!("f0e1d2c3".repeat(8))
                });
                write_checksums(&fx.dir);
                fx.vars()
            },
        ),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snapshot_that_breaks_its_binding_leaves_the_portal_up_with_live_mode_unavailable() {
    let captured = {
        let list: Value =
            serde_json::from_str(&common::snapshot::fixture("list-demo-repo.response.json"))
                .unwrap();
        list["memories"][0]["text"].as_str().unwrap().to_string()
    };
    for (n, (what, code, unbind)) in unbound().into_iter().enumerate() {
        let fx = Fixture::new(&format!("unbound-{n}"));
        let vars = unbind(&fx);
        let local = fx.dir.to_string_lossy().into_owned();
        // The strict entry point: the variable, the code, no path, no text.
        let err = SnapshotFactory::from_vars(lookup(&vars))
            .err()
            .unwrap_or_else(|| panic!("{what}: must not boot"));
        assert!(
            err.starts_with(&format!("CONSENSUS_SNAPSHOT_DIR: {code}: ")),
            "{what}: {err}"
        );
        for leak in [local.as_str(), EDITED_TEXT, captured.as_str(), "synthetic"] {
            assert!(!err.contains(leak), "{what}: {err} leaks {leak:?}");
        }
        // What serve-public does with live mode enabled: the portal up,
        // live mode unavailable, the code logged once and nowhere else.
        let meta = serve(one_run());
        let h = live_server(
            LiveOptions {
                factory: FactoryMode::SnapshotFromVars(vars),
                ..LiveOptions::default()
            },
            meta_config(&meta.base_url),
        );
        assert!(h.snapshot.is_none(), "{what}: nothing booted");
        let status = h.factory.status();
        assert!(!status.ok, "{what}");
        assert_eq!(status.detail.as_deref(), Some(code), "{what}");
        let (_, ready, _) = h.server.json(Req::get("/api/v1/ready")).await;
        assert_eq!(ready["ready"], false, "{what}: {ready}");
        assert_eq!(ready["live"]["evidence_ok"], false, "{what}: {ready}");
        let (_, caps, _) = h.server.json(Req::get("/api/v1/capabilities")).await;
        assert_eq!(caps["live_enabled"], false, "{what}: {caps}");
        let cookie = h.session().await;
        let (status, body, _) = h.server.json(submit(&cookie)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{what}: {body}");
        assert_eq!(body["code"], "live_unavailable", "{what}: {body}");
        let lines = h.evidence_lines();
        assert_eq!(lines, [unavailable_line(code)], "{what}");
        for (part, text) in [
            ("ready", ready.to_string()),
            ("capabilities", caps.to_string()),
            ("503", body.to_string()),
            ("log line", lines[0].clone()),
        ] {
            fx.assert_no_path(&format!("{what}: {part}"), &text);
            assert!(!text.contains(EDITED_TEXT), "{what}: {part}: {text}");
            assert!(!text.contains(&captured), "{what}: {part}: {text}");
            if part != "log line" {
                assert!(!text.contains(code), "{what}: {part}: {text}");
            }
        }
        // Refused before any verifier run, Turnstile, the ledger or a model.
        assert!(fx.calls().is_empty(), "{what}: {:?}", fx.calls());
        assert!(h.siteverify.calls().is_empty(), "{what}");
        assert!(h.ledger_records("reservation").is_empty(), "{what}");
        assert_eq!(meta.hits(), 0, "{what}");
    }
    // The wrapper of the case above never ran either.
    let fx = Fixture::new("unbound-wrapper");
    let (wrapper, wrapper_log) = report_printing_wrapper(fx.snapshot.root());
    let vars = fx.vars_with("CONSENSUS_HYPHAE_BIN", &wrapper);
    let evidence = SnapshotFactory::live_evidence_from_vars(
        lookup(&vars),
        collector(&Arc::new(Mutex::new(Vec::new()))),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        evidence.status().detail.as_deref(),
        Some(codes::VERIFIER_BINARY_MISMATCH)
    );
    assert!(
        logged_calls(&wrapper_log).is_empty(),
        "the wrapper never ran"
    );
    // Unpinned, the same wrapper boots: what the binary pin closes.
    let mut unpinned: Vec<_> = fx
        .unpinned_vars()
        .into_iter()
        .filter(|(k, _)| *k != "CONSENSUS_HYPHAE_BIN")
        .collect();
    unpinned.push(("CONSENSUS_HYPHAE_BIN", wrapper.clone()));
    let evidence = SnapshotFactory::live_evidence_from_vars(
        lookup(&unpinned),
        collector(&Arc::new(Mutex::new(Vec::new()))),
    )
    .unwrap()
    .unwrap();
    assert!(
        evidence.status().ok,
        "unpinned, nothing tells the wrapper from Hyphae"
    );
    assert_eq!(logged_calls(&wrapper_log).len(), 2);
}

// ---------------------------------------------------------------------
// The real serve-public binary (review M1, M3, L3)
// ---------------------------------------------------------------------

/// What the real `consensus-api serve-public` printed before it said it
/// listens (it is then stopped) or exited.
#[derive(Debug)]
struct Startup {
    listening: bool,
    exit_code: Option<i32>,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl Startup {
    fn snapshot_lines(&self) -> Vec<&str> {
        self.stderr
            .iter()
            .chain(&self.stdout)
            .filter(|l| l.starts_with("[snapshot]"))
            .map(String::as_str)
            .collect()
    }

    fn all(&self) -> String {
        format!("{}\n{}", self.stdout.join("\n"), self.stderr.join("\n"))
    }

    fn live_line(&self) -> &str {
        self.stdout
            .iter()
            .find(|l| l.starts_with("Public live mode:"))
            .unwrap_or_else(|| panic!("no live mode line: {self:?}"))
    }
}

/// The variables a process needs from the OS to start (and to run the fake
/// `hyphae.cmd`); nothing of the developer's environment beyond these.
const OS_VARIABLES: [&str; 9] = [
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATH",
    "PATHEXT",
    "TEMP",
    "TMP",
    "TMPDIR",
];

/// Runs `consensus-api serve-public` from `dir` with exactly `vars` (and
/// [`OS_VARIABLES`]) until it says it listens or exits.
fn serve_public(dir: &Path, vars: &[(&'static str, String)]) -> Startup {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    let mut command = Command::new(env!("CARGO_BIN_EXE_consensus-api"));
    command
        .arg("serve-public")
        .current_dir(dir)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in OS_VARIABLES {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, value) in vars {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("the consensus-api binary");
    let (tx, rx) = channel::<(bool, String)>();
    let out_tx = tx.clone();
    let stdout = child.stdout.take().unwrap();
    let out = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            let _ = out_tx.send((true, line));
        }
    });
    let stderr = child.stderr.take().unwrap();
    let err = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            let _ = tx.send((false, line));
        }
    });
    let mut startup = Startup {
        listening: false,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((true, line)) => {
                let listening = line.starts_with("Listening on");
                startup.stdout.push(line);
                if listening {
                    startup.listening = true;
                    break;
                }
            }
            Ok((false, line)) => startup.stderr.push(line),
            Err(RecvTimeoutError::Timeout) => {
                assert!(
                    Instant::now() < deadline,
                    "serve-public neither listened nor exited: {startup:?}"
                );
            }
            // Both pipes closed: the process ended.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if startup.listening {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    let _ = out.join();
    let _ = err.join();
    for (is_stdout, line) in rx.try_iter() {
        if is_stdout {
            startup.stdout.push(line);
        } else {
            startup.stderr.push(line);
        }
    }
    if !startup.listening {
        startup.exit_code = status.code();
    }
    startup
}

/// A public server on a loopback port the OS picks, its store under
/// `scratch`, no research archive and no web export (warnings only).
fn server_vars(scratch: &Path) -> Vec<(&'static str, String)> {
    let empty = scratch.join("research");
    std::fs::create_dir_all(&empty).unwrap();
    vec![
        ("CONSENSUS_PUBLIC_BIND", "127.0.0.1:0".into()),
        (
            "CONSENSUS_RUN_STORE_DIR",
            scratch.join("store").display().to_string(),
        ),
        ("CONSENSUS_RESEARCH_DIR", empty.display().to_string()),
        (
            "CONSENSUS_WEB_ROOT",
            scratch.join("no-web-export").display().to_string(),
        ),
    ]
}

/// Live mode enabled, with every setting it requires: the ledger, a
/// Turnstile secret (not a test one) and hostname, and a credential file
/// holding the test key. No request is ever sent to the server, so neither
/// Siteverify nor the Meta API is reached.
fn live_on_vars(scratch: &Path) -> Vec<(&'static str, String)> {
    let secret = scratch.join("turnstile.secret");
    std::fs::write(&secret, "0x4AAAAAAAzzzzzzzzzzzzzzzzzzzzzzzzzz\n").unwrap();
    let key = scratch.join("meta.key");
    std::fs::write(&key, format!("{TEST_KEY}\n")).unwrap();
    vec![
        ("CONSENSUS_LIVE_ENABLED", "true".into()),
        (
            "CONSENSUS_LIVE_LEDGER_DIR",
            scratch.join("ledger").display().to_string(),
        ),
        (
            "CONSENSUS_TURNSTILE_SECRET_FILE",
            secret.display().to_string(),
        ),
        ("CONSENSUS_TURNSTILE_HOSTNAME", "demo.example.org".into()),
        ("CONSENSUS_META_API_KEY_FILE", key.display().to_string()),
    ]
}

/// The snapshot lines of `deploy/etc/consensus/public.env.example` before
/// the capture exists.
fn placeholder_vars() -> Vec<(&'static str, String)> {
    vec![
        (
            "CONSENSUS_SNAPSHOT_DIR",
            "/var/lib/consensus/snapshots/<snapshot-id>".into(),
        ),
        (
            "CONSENSUS_HYPHAE_EXPECTED_ANCHOR",
            "<64-hex-anchor-from-the-snapshot-manifest>".into(),
        ),
        (
            "CONSENSUS_SNAPSHOT_MANIFEST_SHA256",
            "<64-hex-sha256-of-the-snapshot-manifest.json>".into(),
        ),
        (
            "CONSENSUS_HYPHAE_BIN_SHA256",
            "<64-hex-sha256-of-the-installed-hyphae-binary>".into(),
        ),
    ]
}

#[test]
fn with_live_mode_disabled_serve_public_never_reads_the_snapshot_and_starts() {
    let scratch = TempRoot::new("serve-public-live-off");
    let base = server_vars(&scratch);
    let disabled = "Public live mode: disabled (CONSENSUS_LIVE_ENABLED)";

    // No snapshot variable: nothing about a snapshot at all.
    let s = serve_public(&scratch, &base);
    assert!(s.listening, "{s:?}");
    assert!(s.snapshot_lines().is_empty(), "{s:?}");
    assert_eq!(s.live_line(), disabled);

    // The env example's placeholders: one notice, no error.
    let s = serve_public(&scratch, &[base.clone(), placeholder_vars()].concat());
    assert!(s.listening, "{s:?}");
    assert_eq!(s.snapshot_lines(), [UNUSED_NOTICE], "{s:?}");
    assert_eq!(s.live_line(), disabled);

    // A valid anchor before the capture exists (no directory yet).
    let absent = scratch.join("snapshot-not-captured-yet");
    let vars = [
        base.clone(),
        vec![
            ("CONSENSUS_SNAPSHOT_DIR", absent.display().to_string()),
            ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR.into()),
        ],
    ]
    .concat();
    let s = serve_public(&scratch, &vars);
    assert!(s.listening, "{s:?}");
    assert_eq!(s.snapshot_lines(), [UNUSED_NOTICE], "{s:?}");

    // A valid snapshot: never loaded nor verified (the fake binary is not
    // called), and no evidence line.
    let fx = Fixture::new("serve-public-live-off");
    let s = serve_public(&scratch, &[base.clone(), fx.vars()].concat());
    assert!(s.listening, "{s:?}");
    assert_eq!(s.snapshot_lines(), [UNUSED_NOTICE], "{s:?}");
    assert!(
        fx.calls().is_empty(),
        "the snapshot was verified: {:?}",
        fx.calls()
    );
    assert!(!s.all().contains("Sealed snapshot evidence"), "{s:?}");
    assert!(!s.all().contains(TEST_KEY));
}

#[test]
fn with_live_mode_enabled_serve_public_starts_on_a_snapshot_that_does_not_boot_and_refuses_bad_settings(
) {
    let scratch = TempRoot::new("serve-public-live-on");
    let live = [server_vars(&scratch), live_on_vars(&scratch)].concat();

    // A snapshot that fails its boot verification: the server starts, live
    // mode says its evidence is unavailable, the code is logged once and
    // nothing carries a path.
    let fx = Fixture::of(unverifiable("serve-public-degrade"));
    let s = serve_public(&scratch, &[live.clone(), fx.vars()].concat());
    assert!(s.listening, "{s:?}");
    assert_eq!(
        s.snapshot_lines(),
        [unavailable_line(codes::BOOT_VERIFICATION_FAILED)],
        "{s:?}"
    );
    let line = s.live_line();
    assert!(line.starts_with("Public live mode: enabled"), "{line}");
    assert!(line.contains("its evidence is UNAVAILABLE"), "{line}");
    for text in s.snapshot_lines().into_iter().chain([line]) {
        fx.assert_no_path("startup line", text);
    }
    assert_eq!(
        s.all().matches(codes::BOOT_VERIFICATION_FAILED).count(),
        1,
        "the code is logged once at startup: {s:?}"
    );
    assert!(!s.all().contains("Sealed snapshot evidence"), "{s:?}");

    // A snapshot that boots: the evidence line (no path), two verifier
    // runs, live mode enabled without a warning.
    let ok = Fixture::new("serve-public-boots");
    let s = serve_public(&scratch, &[live.clone(), ok.vars()].concat());
    assert!(s.listening, "{s:?}");
    assert!(s.snapshot_lines().is_empty(), "{s:?}");
    let evidence = s
        .stdout
        .iter()
        .find(|l| l.starts_with("Sealed snapshot evidence: "))
        .unwrap_or_else(|| panic!("no evidence line: {s:?}"));
    assert!(evidence.contains(SNAPSHOT_ID), "{evidence}");
    ok.assert_no_path("evidence line", evidence);
    assert_eq!(ok.calls().len(), 2, "{:?}", ok.calls());
    assert!(!s.live_line().contains("UNAVAILABLE"), "{s:?}");

    // A configuration error still stops startup, naming its variable: the
    // env example's anchor placeholder.
    let vars = [
        live.clone(),
        vec![
            (
                "CONSENSUS_SNAPSHOT_DIR",
                ok.dir.to_string_lossy().into_owned(),
            ),
            placeholder_vars()[1].clone(),
        ],
    ]
    .concat();
    let s = serve_public(&scratch, &vars);
    assert!(!s.listening, "{s:?}");
    assert_eq!(s.exit_code, Some(1), "{s:?}");
    assert!(
        s.stderr
            .iter()
            .any(|l| l.starts_with("error: CONSENSUS_HYPHAE_EXPECTED_ANCHOR")),
        "{s:?}"
    );
    // And so does live mode without a snapshot directory.
    let s = serve_public(&scratch, &live);
    assert!(!s.listening, "{s:?}");
    assert_eq!(s.exit_code, Some(1), "{s:?}");
    assert!(
        s.stderr
            .iter()
            .any(|l| l.starts_with("error: ") && l.contains("CONSENSUS_SNAPSHOT_DIR")),
        "{s:?}"
    );
    assert!(!s.all().contains(TEST_KEY));
}

#[test]
fn with_live_mode_enabled_serve_public_boots_pinned_and_refuses_an_unusable_pin() {
    let scratch = TempRoot::new("serve-public-pins");
    let live = [server_vars(&scratch), live_on_vars(&scratch)].concat();
    let fx = Fixture::new("serve-public-pinned");

    // Both digests pinned, as production runs: the evidence line says so,
    // with the prefixes only and no path.
    let s = serve_public(&scratch, &[live.clone(), fx.vars()].concat());
    assert!(s.listening, "{s:?}");
    assert!(s.snapshot_lines().is_empty(), "{s:?}");
    let evidence = s
        .stdout
        .iter()
        .find(|l| l.starts_with("Sealed snapshot evidence: "))
        .unwrap_or_else(|| panic!("no evidence line: {s:?}"));
    let manifest = manifest_sha256(&fx.dir);
    assert!(
        evidence.contains(&format!("manifest sha256 pinned {}…", &manifest[..12])),
        "{evidence}"
    );
    assert!(
        evidence.contains(&format!(
            "verifier binary sha256 pinned {}…",
            &fx.hyphae_bin_sha256[..12]
        )),
        "{evidence}"
    );
    fx.assert_no_path("evidence line", evidence);
    assert_eq!(fx.calls().len(), 2, "{:?}", fx.calls());

    // An unusable pin stops startup naming its variable, as an unusable
    // anchor does: the env example's two placeholders, and a pinned binary
    // that is not an absolute path.
    let placeholders = placeholder_vars();
    for (var, value, named) in [
        (
            placeholders[2].0,
            placeholders[2].1.clone(),
            "error: CONSENSUS_SNAPSHOT_MANIFEST_SHA256: ",
        ),
        (
            placeholders[3].0,
            placeholders[3].1.clone(),
            "error: CONSENSUS_HYPHAE_BIN_SHA256: ",
        ),
        (
            "CONSENSUS_HYPHAE_BIN",
            "hyphae".to_string(),
            "error: CONSENSUS_HYPHAE_BIN: ",
        ),
    ] {
        let s = serve_public(
            &scratch,
            &[live.clone(), fx.vars_with(var, &value)].concat(),
        );
        assert!(!s.listening, "{var}: {s:?}");
        assert_eq!(s.exit_code, Some(1), "{var}: {s:?}");
        assert!(
            s.stderr.iter().any(|l| l.starts_with(named)),
            "{var}: {s:?}"
        );
        assert!(
            !s.all().contains("Sealed snapshot evidence"),
            "{var}: {s:?}"
        );
        assert!(!s.all().contains(TEST_KEY));
    }
    assert_eq!(fx.calls().len(), 2, "nothing was verified for them");
}

/// On the unix socket (the deployment path) live mode needs both pins:
/// without either one `serve-public` stops before it listens, naming the
/// missing variable; with both it serves (require_snapshot_pins). The TCP
/// tests above run unpinned or pinned at will.
#[cfg(unix)]
#[test]
fn with_live_mode_enabled_on_the_socket_serve_public_requires_both_pins() {
    let scratch = TempRoot::new("serve-public-socket-pins");
    let mut socket: Vec<(&'static str, String)> = server_vars(&scratch)
        .into_iter()
        .filter(|(name, _)| *name != "CONSENSUS_PUBLIC_BIND")
        .collect();
    socket.push((
        "CONSENSUS_PUBLIC_SOCKET",
        scratch.join("public.sock").display().to_string(),
    ));
    socket.push((
        "CONSENSUS_ALLOWED_ORIGINS",
        "https://demo.example.org".into(),
    ));
    socket.push(("CONSENSUS_ALLOWED_HOSTS", "demo.example.org".into()));
    let live = [socket, live_on_vars(&scratch)].concat();
    let fx = Fixture::new("serve-public-socket-pins");
    for missing in [
        "CONSENSUS_SNAPSHOT_MANIFEST_SHA256",
        "CONSENSUS_HYPHAE_BIN_SHA256",
    ] {
        let vars: Vec<(&'static str, String)> = [live.clone(), fx.vars()]
            .concat()
            .into_iter()
            .filter(|(name, _)| *name != missing)
            .collect();
        let s = serve_public(&scratch, &vars);
        assert!(!s.listening, "{missing}: {s:?}");
        assert_eq!(s.exit_code, Some(1), "{missing}: {s:?}");
        assert!(s.all().contains(missing), "{missing}: {s:?}");
    }
    assert_eq!(fx.calls().len(), 0, "nothing was verified without the pins");
    let s = serve_public(&scratch, &[live, fx.vars()].concat());
    assert!(s.listening, "{s:?}");
}

/// `CONSENSUS_MODEL_CRITIC` on the real binary: a Critic on another model
/// with the ledger's default prices stops startup naming the variables,
/// before the snapshot is verified; with both prices set explicitly, or
/// with the Critic on the one model, the server starts; an unusable id
/// stops startup naming the override.
#[test]
fn serve_public_refuses_a_mixed_model_critic_without_explicit_ledger_prices() {
    let scratch = TempRoot::new("serve-public-critic-model");
    let live = [server_vars(&scratch), live_on_vars(&scratch)].concat();
    let fx = Fixture::new("serve-public-critic-model");
    let critic = ("CONSENSUS_MODEL_CRITIC", "muse-spark-1.3".to_string());

    let s = serve_public(
        &scratch,
        &[live.clone(), fx.vars(), vec![critic.clone()]].concat(),
    );
    assert!(!s.listening, "{s:?}");
    assert_eq!(s.exit_code, Some(1), "{s:?}");
    assert!(
        s.stderr
            .iter()
            .any(|l| l.starts_with("error: public live mode: ")
                && l.contains("CONSENSUS_MODEL_CRITIC")
                && l.contains("CONSENSUS_META_PRICE_INPUT_PER_M")
                && l.contains("CONSENSUS_META_PRICE_OUTPUT_PER_M")),
        "{s:?}"
    );
    assert!(fx.calls().is_empty(), "refused before the snapshot boot");
    assert!(!s.all().contains(TEST_KEY));

    let prices = vec![
        ("CONSENSUS_META_PRICE_INPUT_PER_M", "4.00".to_string()),
        ("CONSENSUS_META_PRICE_OUTPUT_PER_M", "20.00".to_string()),
    ];
    let s = serve_public(
        &scratch,
        &[live.clone(), fx.vars(), vec![critic], prices].concat(),
    );
    assert!(s.listening, "{s:?}");

    let one_model = ("CONSENSUS_MODEL_CRITIC", "muse-wonder-1.0".to_string());
    let s = serve_public(
        &scratch,
        &[live.clone(), fx.vars(), vec![one_model]].concat(),
    );
    assert!(s.listening, "{s:?}");

    let bad = ("CONSENSUS_MODEL_CRITIC", "muse spark".to_string());
    let s = serve_public(&scratch, &[live, fx.vars(), vec![bad]].concat());
    assert!(!s.listening, "{s:?}");
    assert_eq!(s.exit_code, Some(1), "{s:?}");
    assert!(
        s.stderr
            .iter()
            .any(|l| l.starts_with("error: public live mode: CONSENSUS_MODEL_CRITIC: ")),
        "{s:?}"
    );
}
