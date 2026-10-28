// SPDX-License-Identifier: Apache-2.0
//! The sealed snapshot client (ADR-0008, plan B11): loading and checking a
//! snapshot directory, boot verification through the real
//! `HyphaeProofVerifier` (a scripted command runner, and a FAKE `hyphae`
//! binary that records every invocation), query-independent recalls, the
//! isolation and unknown-project paths, and an end-to-end run through the
//! orchestrator with the scripted model. No Hyphae runs here: the snapshot
//! is assembled from `tests/fixtures/snapshot/` with synthetic proof and
//! witness bytes.

mod common;

use common::snapshot::*;
use consensus_api::config::SnapshotConfig;
use consensus_api::hyphae::{
    CommandError, CommandOutput, CommandRunner, HyphaeProofVerifier, SystemCommandRunner,
};
use consensus_api::snapshot::{
    codes, verifier_from_config, Snapshot, SnapshotError, SnapshotMemoryClient,
};
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::fakes::FakeModelClient;
use consensus_runtime::orchestrator::{codes as run_codes, Orchestrator, TaskInput};
use consensus_runtime::ports::{MemoryClient, MemoryError, ProofCheck, ProofVerifier, RecallProof};
use consensus_runtime::scenarios::{assessment, claim, evaluation, proposal, TS};
use consensus_runtime::{EventLog, RunAggregate, RunLimits};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Answers every `hyphae proof verify` and records each argv. With an
/// `outcome`, every call gets it; without one, a call gets the capture-time
/// report of the capture whose synthetic proof bytes it was given, the way
/// a real verifier's digests follow the artifact's content.
struct ScriptedRunner {
    outcome: Option<Result<CommandOutput, CommandError>>,
    calls: Vec<Vec<String>>,
}

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, argv: &[String], _timeout: Duration) -> Result<CommandOutput, CommandError> {
        self.calls.push(argv.to_vec());
        match &self.outcome {
            Some(outcome) => outcome.clone(),
            None => {
                let at = argv.iter().position(|a| a == "--proof").expect("--proof");
                exited(0, &verify_report(&capture_of_proof(&argv[at + 1])), "")
            }
        }
    }
}

fn exited(code: i32, stdout: &str, stderr: &str) -> Result<CommandOutput, CommandError> {
    Ok(CommandOutput {
        code,
        stdout: stdout.into(),
        stderr: stderr.into(),
    })
}

/// `verify_report(id)` with one field changed.
fn report_with(id: &str, key: &str, value: Value) -> String {
    let mut report: Value = serde_json::from_str(&verify_report(id)).unwrap();
    report[key] = value;
    report.to_string()
}

fn scripted(
    cfg: &SnapshotConfig,
    outcome: Option<Result<CommandOutput, CommandError>>,
) -> HyphaeProofVerifier<ScriptedRunner> {
    verifier_from_config(
        cfg,
        ScriptedRunner {
            outcome,
            calls: vec![],
        },
    )
}

fn open(dir: &Path) -> (SnapshotMemoryClient, HyphaeProofVerifier<ScriptedRunner>) {
    let cfg = config(dir, "hyphae");
    let mut verifier = scripted(&cfg, None);
    let client = SnapshotMemoryClient::open(&cfg, &mut verifier).expect("open");
    (client, verifier)
}

fn load_error(dir: &Path) -> SnapshotError {
    match Snapshot::load(dir, None) {
        Ok(_) => panic!("load must fail"),
        Err(e) => e,
    }
}

fn open_error(dir: &Path) -> SnapshotError {
    let cfg = config(dir, "hyphae");
    let mut verifier = scripted(&cfg, None);
    match SnapshotMemoryClient::open(&cfg, &mut verifier) {
        Ok(_) => panic!("open must fail"),
        Err(e) => e,
    }
}

#[test]
fn the_snapshot_config_requires_the_pinned_anchor() {
    let from = |pairs: &[(&str, &str)]| {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        SnapshotConfig::from_vars(|name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        })
    };
    assert_eq!(from(&[]).unwrap(), None, "no directory, no snapshot mode");
    assert_eq!(
        from(&[("CONSENSUS_SNAPSHOT_DIR", "  ")]).unwrap(),
        None,
        "an empty value is unset"
    );
    let err = from(&[("CONSENSUS_SNAPSHOT_DIR", "/srv/snap")]).unwrap_err();
    assert_eq!(err.setting, "CONSENSUS_HYPHAE_EXPECTED_ANCHOR");
    assert!(err.message.contains("required"), "{err}");
    let err = from(&[
        ("CONSENSUS_SNAPSHOT_DIR", "/srv/snap"),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", "not-hex"),
    ])
    .unwrap_err();
    assert_eq!(err.setting, "CONSENSUS_HYPHAE_EXPECTED_ANCHOR");

    let cfg = from(&[
        ("CONSENSUS_SNAPSHOT_DIR", "/srv/snap"),
        (
            "CONSENSUS_HYPHAE_EXPECTED_ANCHOR",
            &format!(" {}\r\n", ANCHOR.to_ascii_uppercase()),
        ),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(cfg.dir, PathBuf::from("/srv/snap"));
    assert_eq!(cfg.expected_anchor, ANCHOR, "normalized like HyphaeConfig");
    assert_eq!(cfg.verifier_dir, None);
    assert_eq!(cfg.binary, "hyphae");
    assert_eq!(cfg.verify_timeout, Duration::from_secs(60));

    let cfg = from(&[
        ("CONSENSUS_SNAPSHOT_DIR", "C:\\snapshots\\snap"),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR),
        ("CONSENSUS_SNAPSHOT_VERIFIER_DIR", "/snapshots/snap"),
        ("CONSENSUS_HYPHAE_BIN", "scripts/hyphae-docker.cmd"),
        ("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS", "7"),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(cfg.verifier_dir.as_deref(), Some("/snapshots/snap"));
    assert_eq!(cfg.binary, "scripts/hyphae-docker.cmd");
    assert_eq!(cfg.verify_timeout, Duration::from_secs(7));
    let err = from(&[
        ("CONSENSUS_SNAPSHOT_DIR", "/srv/snap"),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR),
        ("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS", "0"),
    ])
    .unwrap_err();
    assert_eq!(err.setting, "CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS");
    let verifier = verifier_from_config(
        &cfg,
        ScriptedRunner {
            outcome: Some(exited(0, "", "")),
            calls: vec![],
        },
    );
    assert_eq!(verifier.expected_anchor.as_deref(), Some(ANCHOR));
    assert_eq!(verifier.timeout, Duration::from_secs(7));
    assert_eq!(verifier.binary, "scripts/hyphae-docker.cmd");
}

#[test]
fn a_set_snapshot_dir_with_a_missing_or_malformed_anchor_is_a_boot_error() {
    // ADR-0008 decision 1: with the directory set, a missing or malformed
    // pinned anchor is an error naming the variable, never `None` (which a
    // caller would read as "snapshot mode not configured" and fall back
    // silently). `from_vars` and `try_from_env` are the only entry points.
    let from = |pairs: &[(&str, &str)]| {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        SnapshotConfig::from_vars(|name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        })
    };
    let short = ANCHOR[..63].to_string();
    let long = format!("{ANCHOR}0");
    let glyph = format!("{}g", &ANCHOR[..63]);
    let cases: [(&str, Option<&str>); 7] = [
        ("missing", None),
        ("blank", Some("   ")),
        ("not hex", Some("not-hex")),
        ("63 digits", Some(short.as_str())),
        ("65 digits", Some(long.as_str())),
        ("a non-hex glyph", Some(glyph.as_str())),
        ("the demo store's prefix only", Some("d91ac10d")),
    ];
    for (label, anchor) in cases {
        let mut pairs = vec![("CONSENSUS_SNAPSHOT_DIR", "/srv/snapshots/snap")];
        if let Some(anchor) = anchor {
            pairs.push(("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", anchor));
        }
        let err = match from(&pairs) {
            Err(err) => err,
            Ok(cfg) => {
                panic!("{label}: a set directory with a bad anchor must be an error, got {cfg:?}")
            }
        };
        assert_eq!(err.setting, "CONSENSUS_HYPHAE_EXPECTED_ANCHOR", "{label}");
        assert!(
            err.to_string().contains("CONSENSUS_HYPHAE_EXPECTED_ANCHOR"),
            "{label}: the message names the variable: {err}"
        );
    }
    // The directory alone unset is the one `None`: snapshot mode off.
    assert_eq!(
        from(&[("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR)]).unwrap(),
        None
    );
}

#[test]
fn a_well_formed_snapshot_loads_and_opens_after_verifying_both_captures() {
    let dir = build("well-formed");
    let snapshot = Snapshot::load(&dir, None).unwrap();
    assert_eq!(snapshot.manifest.snapshot_id, SNAPSHOT_ID);
    assert_eq!(snapshot.manifest.project, PROJECT);
    assert_eq!(snapshot.manifest.anchor_hex, ANCHOR);
    assert_eq!(snapshot.manifest.store.record_count, 7);
    assert_eq!(snapshot.list_capture().id, LIST_ID);
    assert_eq!(snapshot.isolation_capture().id, ISOLATION_ID);
    let ids: Vec<&str> = snapshot.records().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, RECORD_IDS, "capture order");
    assert!(snapshot.records()[0]
        .text
        .starts_with("Constraint: outbound retries"));
    let request = Snapshot::proof_request(snapshot.list_capture());
    assert_eq!(
        request,
        ProofRequest {
            operation: "list".into(),
            project: PROJECT.into(),
            query: Some(String::new()),
            limit: Some(64),
        }
    );

    let (client, verifier) = open(&dir);
    // Both required captures went through `hyphae proof verify`, with the
    // local artifact paths and the pinned anchor.
    assert_eq!(verifier.runner.calls.len(), 2);
    for (call, id) in verifier.runner.calls.iter().zip([LIST_ID, ISOLATION_ID]) {
        assert_eq!(&call[..3], ["hyphae", "proof", "verify"]);
        let after = |flag: &str| {
            let at = call.iter().position(|a| a == flag).unwrap();
            call[at + 1].clone()
        };
        let proof = after("--proof");
        assert!(
            Path::new(&proof) == dir.join("captures").join(format!("{id}.proof")),
            "{proof}"
        );
        assert!(after("--witness").ends_with(&format!("{id}.witness")));
        assert_eq!(after("--anchor"), ANCHOR);
    }

    let status = client.snapshot_status();
    assert_eq!(status.snapshot_id, SNAPSHOT_ID);
    assert_eq!(status.project, PROJECT);
    assert_eq!(status.isolation_project, OTHER);
    assert_eq!(status.captured_at, CAPTURED_AT);
    assert_eq!(status.anchor_hex, ANCHOR);
    assert_eq!(status.record_count, 7);
    assert_eq!(status.hyphae_version, "hyphae 4.0.0");
    assert!(status.verified_at.ends_with('Z'));
    assert_eq!(status.captures.len(), 2);
    for (capture, (id, count)) in status
        .captures
        .iter()
        .zip([(LIST_ID, 7), (ISOLATION_ID, 0)])
    {
        assert_eq!(capture.id, id);
        assert_eq!(capture.record_count, count);
        assert_eq!(capture.integrity, IntegrityStatus::Valid);
        assert_eq!(capture.verifier.as_deref(), Some("hyphae proof verify"));
        let report = capture.report.as_ref().expect("the verifier's report");
        assert_eq!(report.kind.as_deref(), Some("memory"));
        assert_eq!(report.scope.as_deref(), Some("semantic_reexecution"));
        assert_eq!(report.anchor_digest.as_deref(), Some(ANCHOR));
        // The boot report reproduced the capture-time record's digests.
        let recorded: Value = serde_json::from_str(&verify_report(id)).unwrap();
        assert_eq!(
            report.proof_digest.as_deref(),
            recorded["proof_digest"].as_str()
        );
        assert_eq!(
            report.result_digest.as_deref(),
            recorded["result_digest"].as_str()
        );
        let kept = snapshot
            .recorded_report(if id == LIST_ID {
                snapshot.list_capture()
            } else {
                snapshot.isolation_capture()
            })
            .expect("recorded");
        assert_eq!(
            kept.proof_digest,
            recorded["proof_digest"].as_str().unwrap()
        );
        assert_eq!(
            kept.result_digest,
            recorded["result_digest"].as_str().unwrap()
        );
    }
    assert_ne!(
        status.captures[0].report.as_ref().unwrap().proof_digest,
        status.captures[1].report.as_ref().unwrap().proof_digest,
        "two requests, two proofs"
    );
    // Public-facing strings: no path, no witness content.
    let shown = format!(
        "{} {}",
        serde_json::to_string(status).unwrap(),
        status.summary()
    );
    let local = dir.to_string_lossy().into_owned();
    assert!(!shown.contains(&local), "{shown}");
    assert!(
        !shown.contains("captures/") && !shown.contains(".witness"),
        "{shown}"
    );
    assert!(!shown.contains("synthetic witness bytes"), "{shown}");
    let mut client = client;
    let memory_status = client.status().unwrap();
    assert!(memory_status.healthy);
    assert!(memory_status.detail.contains(SNAPSHOT_ID) && memory_status.detail.contains(ANCHOR));
    assert!(!memory_status.detail.contains(&local));
}

#[test]
fn a_recall_of_the_served_project_is_the_whole_proven_list_whatever_the_query() {
    let dir = build("recall");
    let (client, _) = open(&dir);
    let mut client = client.with_time_source(|| TS.to_string());
    let recall = client
        .recall_with_proof(
            &ProjectId::new(PROJECT),
            "Payment timeouts come from retries",
            5,
        )
        .unwrap();
    assert!(recall.foreign.is_empty() && recall.proof_failure.is_none());
    let ids: Vec<&str> = recall.records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids, RECORD_IDS,
        "all seven, in capture order, limit 5 notwithstanding"
    );
    let expected_request = ProofRequest {
        operation: "list".into(),
        project: PROJECT.into(),
        query: Some(String::new()),
        limit: Some(64),
    };
    let proof_sha256 = sha256_hex(&proof_bytes(LIST_ID));
    let list: Value = serde_json::from_str(&fixture("list-demo-repo.response.json")).unwrap();
    for (rec, memory) in recall
        .records
        .iter()
        .zip(list["memories"].as_array().unwrap())
    {
        assert_eq!(rec.schema_version, SCHEMA_VERSION);
        assert_eq!(rec.project_id, ProjectId::new(PROJECT));
        assert_eq!(
            rec.backend_ref.as_deref(),
            Some(format!("hyphae-memory:{}", rec.id).as_str())
        );
        assert_eq!(rec.kind, memory["kind"].as_str().unwrap());
        assert_eq!(rec.content, memory["text"].as_str().unwrap());
        assert_eq!(rec.retrieved_at, TS);
        assert_eq!(rec.anchor.as_deref(), Some(ANCHOR));
        assert!(rec.proof_error.is_none());
        assert_eq!(rec.retrieval, Some(Retrieval::SealedSnapshot));
        assert_eq!(rec.snapshot_id.as_deref(), Some(SNAPSHOT_ID));
        assert_eq!(rec.captured_at.as_deref(), Some(CAPTURED_AT));
        assert_eq!(
            rec.proof_ref.as_deref(),
            Some(LIST_ID),
            "the capture id, no path"
        );
        assert_eq!(rec.proof_sha256.as_deref(), Some(proof_sha256.as_str()));
        assert_eq!(rec.proof_request.as_ref(), Some(&expected_request));
        assert_eq!(rec.backend_project.as_deref(), Some(PROJECT));
        assert_eq!(rec.backend_scope.as_deref(), Some("project"));
        // The artifact paths are the local ones the verifier reads.
        assert_eq!(
            Path::new(rec.proof_file.as_deref().unwrap()),
            dir.join("captures").join(format!("{LIST_ID}.proof"))
        );
        assert_eq!(
            Path::new(rec.witness_file.as_deref().unwrap()),
            dir.join("captures").join(format!("{LIST_ID}.witness"))
        );
    }
    // The recall carries the proof of its whole result: the orchestrator
    // records RecallProven with every captured id.
    assert_eq!(
        recall.proof,
        Some(RecallProof {
            request: expected_request,
            evidence_ids: RECORD_IDS.iter().map(|i| EvidenceId::new(*i)).collect(),
            proof_ref: LIST_ID.into(),
            anchor: ANCHOR.into(),
        })
    );
    // Query-independent: another query and limit, the same records.
    let again = client
        .recall(&ProjectId::new(PROJECT), "something else entirely", 64)
        .unwrap();
    assert_eq!(again, recall.records);
    let one = client.recall(&ProjectId::new(PROJECT), "", 1).unwrap();
    assert_eq!(one.len(), 7, "the proven set is indivisible");
    // A proof is attached only to records in its own captured result.
    for rec in &recall.records {
        assert!(
            recall
                .proof
                .as_ref()
                .unwrap()
                .evidence_ids
                .contains(&rec.id),
            "{} is covered by the proof it carries",
            rec.id
        );
    }
}

#[test]
fn a_recall_of_the_isolation_project_is_empty_with_its_own_proof() {
    let dir = build("isolation");
    let (mut client, _) = open(&dir);
    assert_eq!(client.isolation_project(), &ProjectId::new(OTHER));
    let recall = client
        .recall_with_proof(&ProjectId::new(OTHER), "retry cap", 8)
        .unwrap();
    assert!(recall.records.is_empty() && recall.foreign.is_empty());
    assert_eq!(
        recall.proof,
        Some(RecallProof {
            request: ProofRequest {
                operation: "recall".into(),
                project: OTHER.into(),
                query: Some(ISOLATION_QUERY.into()),
                limit: Some(8),
            },
            evidence_ids: vec![],
            proof_ref: ISOLATION_ID.into(),
            anchor: ANCHOR.into(),
        })
    );
    assert!(client
        .recall(&ProjectId::new(OTHER), "x", 5)
        .unwrap()
        .is_empty());
}

#[test]
fn a_recall_of_an_unknown_project_is_a_backend_error_with_a_reason() {
    let dir = build("unknown");
    let (mut client, _) = open(&dir);
    let err = client
        .recall_with_proof(&ProjectId::new("third-repo"), "anything", 5)
        .unwrap_err();
    let MemoryError::Backend(reason) = &err else {
        panic!("expected a Backend error, got {err:?}");
    };
    assert!(reason.contains(SNAPSHOT_ID), "{reason}");
    assert!(
        reason.contains("\"demo-repo\"") && reason.contains("\"third-repo\""),
        "{reason}"
    );
    assert!(reason.contains("not captured"), "{reason}");
    assert!(
        !reason.contains(&dir.to_string_lossy().into_owned()),
        "{reason}"
    );
    assert!(client.recall(&ProjectId::new("_global"), "x", 5).is_err());
}

#[test]
fn tampering_with_any_file_fails_open_with_its_code() {
    let capture_file = |dir: &Path, name: &str| dir.join("captures").join(name);
    // A flipped byte anywhere is caught by SHA256SUMS first.
    for (name, file) in [
        ("proof", format!("{LIST_ID}.proof")),
        ("witness", format!("{LIST_ID}.witness")),
        ("response", format!("{LIST_ID}.response.json")),
        ("verify", format!("{ISOLATION_ID}.verify.json")),
    ] {
        let dir = build(&format!("flip-{name}"));
        flip_first_byte(&capture_file(&dir, &file));
        let err = load_error(&dir);
        assert_eq!(err.code, codes::CHECKSUMS_MISMATCH, "{name}: {err}");
        assert!(err.message.contains(&format!("captures/{file}")), "{err}");
        assert_eq!(open_error(&dir), err, "open fails the same way");
    }
    let dir = build("flip-manifest");
    flip_first_byte(&dir.join("manifest.json"));
    assert_eq!(load_error(&dir).code, codes::CHECKSUMS_MISMATCH);

    // Consistent tampering (SHA256SUMS regenerated) is caught by the
    // manifest's own digests and id lists.
    let dir = build("proof-regenerated");
    flip_first_byte(&capture_file(&dir, &format!("{LIST_ID}.proof")));
    write_checksums(&dir);
    let err = load_error(&dir);
    assert_eq!(err.code, codes::DIGEST_MISMATCH, "{err}");
    assert!(
        err.message.contains(&format!("captures/{LIST_ID}.proof")),
        "{err}"
    );

    let dir = build("witness-regenerated");
    flip_first_byte(&capture_file(&dir, &format!("{ISOLATION_ID}.witness")));
    write_checksums(&dir);
    let err = load_error(&dir);
    assert_eq!(err.code, codes::DIGEST_MISMATCH, "{err}");
    assert!(
        err.message
            .contains(&format!("captures/{ISOLATION_ID}.witness")),
        "{err}"
    );

    let dir = build("response-ids");
    let response_path = capture_file(&dir, &format!("{LIST_ID}.response.json"));
    let mut response: Value =
        serde_json::from_slice(&std::fs::read(&response_path).unwrap()).unwrap();
    response["memories"][2]["id"] = json!("999");
    std::fs::write(&response_path, response.to_string()).unwrap();
    write_checksums(&dir);
    assert_eq!(load_error(&dir).code, codes::MEMORY_IDS_MISMATCH);

    let dir = build("manifest-ids-reordered");
    edit_manifest(&dir, |m| {
        let ids = m["captures"][0]["memory_ids"].as_array_mut().unwrap();
        ids.swap(0, 1);
    });
    write_checksums(&dir);
    assert_eq!(load_error(&dir).code, codes::MEMORY_IDS_MISMATCH);

    let dir = build("manifest-anchor");
    edit_manifest(&dir, |m| m["anchor_hex"] = json!("ab".repeat(32)));
    write_checksums(&dir);
    assert_eq!(load_error(&dir).code, codes::ANCHOR_MISMATCH);

    let dir = build("response-anchor");
    let response_path = capture_file(&dir, &format!("{ISOLATION_ID}.response.json"));
    let mut response: Value =
        serde_json::from_slice(&std::fs::read(&response_path).unwrap()).unwrap();
    response["proof"]["anchor_hex"] = json!("ab".repeat(32));
    std::fs::write(&response_path, response.to_string()).unwrap();
    write_checksums(&dir);
    let err = load_error(&dir);
    assert_eq!(err.code, codes::ANCHOR_MISMATCH, "{err}");

    let dir = build("verify-record");
    let verify_path = capture_file(&dir, &format!("{LIST_ID}.verify.json"));
    let mut report: Value = serde_json::from_slice(&std::fs::read(&verify_path).unwrap()).unwrap();
    report["kind"] = json!("catalog");
    std::fs::write(&verify_path, report.to_string()).unwrap();
    write_checksums(&dir);
    assert_eq!(load_error(&dir).code, codes::VERIFY_RECORD_INVALID);

    // A capture-time record verified against another anchor.
    let dir = build("verify-record-anchor");
    let verify_path = capture_file(&dir, &format!("{ISOLATION_ID}.verify.json"));
    std::fs::write(
        &verify_path,
        report_with(ISOLATION_ID, "anchor_digest", json!("ab".repeat(32))),
    )
    .unwrap();
    write_checksums(&dir);
    let err = load_error(&dir);
    assert_eq!(err.code, codes::ANCHOR_MISMATCH, "{err}");
    assert!(
        err.message
            .contains(&format!("captures/{ISOLATION_ID}.verify.json")),
        "{err}"
    );

    // A capture-time record without the digests the boot check reproduces.
    for key in ["proof_digest", "result_digest"] {
        let dir = build(&format!("verify-record-no-{key}"));
        let verify_path = capture_file(&dir, &format!("{LIST_ID}.verify.json"));
        let mut report: Value =
            serde_json::from_slice(&std::fs::read(&verify_path).unwrap()).unwrap();
        report.as_object_mut().unwrap().remove(key);
        std::fs::write(&verify_path, report.to_string()).unwrap();
        write_checksums(&dir);
        let err = load_error(&dir);
        assert_eq!(err.code, codes::VERIFY_RECORD_INVALID, "{key}: {err}");
        assert!(err.message.contains(key), "{err}");
    }

    let dir = build("store-count");
    edit_manifest(&dir, |m| m["store"]["record_count"] = json!(6));
    write_checksums(&dir);
    assert_eq!(load_error(&dir).code, codes::STORE_MISMATCH);

    let dir = build("foreign-record");
    let response_path = capture_file(&dir, &format!("{LIST_ID}.response.json"));
    let mut response: Value =
        serde_json::from_slice(&std::fs::read(&response_path).unwrap()).unwrap();
    response["memories"][4]["project"] = json!("tenant-b");
    std::fs::write(&response_path, response.to_string()).unwrap();
    write_checksums(&dir);
    let err = load_error(&dir);
    assert_eq!(err.code, codes::FOREIGN_RECORD, "{err}");
    assert!(err.message.contains("\"tenant-b\""), "{err}");
    assert!(
        !err.message.contains("constraint_cap_is_three_attempts"),
        "no record text: {err}"
    );

    // Files and listing.
    let dir = build("unlisted-file");
    std::fs::write(dir.join("captures").join("stray.txt"), "x").unwrap();
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CHECKSUMS_INCOMPLETE, "{err}");
    assert!(err.message.contains("captures/stray.txt"), "{err}");

    let dir = build("listed-missing");
    std::fs::remove_file(capture_file(&dir, &format!("{ISOLATION_ID}.verify.json"))).unwrap();
    assert_eq!(load_error(&dir).code, codes::CHECKSUMS_INCOMPLETE);

    let dir = build("no-sums");
    std::fs::remove_file(dir.join("SHA256SUMS")).unwrap();
    let err = load_error(&dir);
    assert_eq!(err.code, codes::FILE_UNREADABLE, "{err}");
    assert!(err.message.starts_with("SHA256SUMS"), "{err}");

    let dir = build("bad-sums-format");
    std::fs::write(dir.join("SHA256SUMS"), "not a checksum line\n").unwrap();
    assert_eq!(load_error(&dir).code, codes::CHECKSUMS_INVALID);

    // Manifest fields.
    for (name, edit, code) in [
        (
            "schema",
            Box::new(|m: &mut Value| m["schema"] = json!("other-schema-v9"))
                as Box<dyn FnOnce(&mut Value)>,
            codes::MANIFEST_SCHEMA,
        ),
        (
            "control-schema",
            Box::new(|m: &mut Value| m["hyphae"]["control_schema"] = json!("hyphae-mcp-v1")),
            codes::MANIFEST_SCHEMA,
        ),
        (
            "id-pattern",
            Box::new(|m: &mut Value| m["snapshot_id"] = json!("Snap_2026")),
            codes::SNAPSHOT_ID_INVALID,
        ),
        (
            "id-dir",
            Box::new(|m: &mut Value| m["snapshot_id"] = json!("snap-other")),
            codes::SNAPSHOT_ID_MISMATCH,
        ),
        (
            "captured-at",
            Box::new(|m: &mut Value| m["captured_at"] = json!("2026-10-29 15:04:05")),
            codes::TIMESTAMP_INVALID,
        ),
        (
            "anchor-uppercase",
            Box::new(|m: &mut Value| m["anchor_hex"] = json!(ANCHOR.to_ascii_uppercase())),
            codes::ANCHOR_INVALID,
        ),
        (
            "missing-field",
            Box::new(|m: &mut Value| {
                m["captures"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("proof_sha256");
            }),
            codes::MANIFEST_INVALID,
        ),
        (
            "wrong-file-name",
            Box::new(|m: &mut Value| {
                m["captures"][0]["proof_file"] = json!("captures/other.proof")
            }),
            codes::CAPTURE_INVALID,
        ),
        (
            "unproven",
            Box::new(|m: &mut Value| m["captures"][1]["arguments"]["prove"] = json!(false)),
            codes::CAPTURE_INVALID,
        ),
        (
            "duplicate-id",
            Box::new(|m: &mut Value| m["captures"][1]["id"] = json!(LIST_ID)),
            codes::CAPTURE_INVALID,
        ),
    ] {
        let dir = build_with(
            &format!("manifest-{name}"),
            &[list_spec(), isolation_spec()],
            edit,
        );
        let err = load_error(&dir);
        assert_eq!(err.code, code, "{name}: {err}");
        assert!(
            !err.message.contains("synthetic"),
            "no file contents: {err}"
        );
    }
    let root = TempRoot::new("not-a-dir");
    assert_eq!(
        load_error(&root.join("absent")).code,
        codes::SNAPSHOT_DIR_MISSING
    );
}

/// Creates the symbolic link `link` to `target`, or returns `None` after a
/// clear message when the OS refuses: on Windows, creating a symlink needs
/// Developer Mode or the SeCreateSymbolicLinkPrivilege (error 1314). For a
/// directory link on Windows without that privilege, a junction stands in:
/// it needs no privilege, and Windows reports it as a name-surrogate
/// reparse point, which `FileType::is_symlink` (the check `walk` relies on)
/// reports as a symbolic link, so a loop through it is the same hazard.
fn symlink(target: &Path, link: &Path, to_directory: bool) -> Option<()> {
    #[cfg(windows)]
    let result = if to_directory {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    #[cfg(not(windows))]
    let result = {
        let _ = to_directory;
        std::os::unix::fs::symlink(target, link)
    };
    match result {
        Ok(()) => Some(()),
        Err(e)
            if e.kind() == std::io::ErrorKind::PermissionDenied
                || e.raw_os_error() == Some(1314) =>
        {
            #[cfg(windows)]
            if to_directory {
                let made = std::process::Command::new("cmd")
                    .args(["/C", "mklink", "/J"])
                    .arg(link)
                    .arg(target)
                    .stdout(std::process::Stdio::null())
                    .status();
                if matches!(made, Ok(status) if status.success()) {
                    let kind = std::fs::symlink_metadata(link).unwrap().file_type();
                    assert!(
                        kind.is_symlink(),
                        "a junction is a name-surrogate reparse point, seen as a symlink"
                    );
                    eprintln!("note: no symlink privilege ({e}); a junction stands in for the directory link");
                    return Some(());
                }
            }
            eprintln!(
                "SKIPPED: this OS refuses to create a symbolic link ({e}); on Windows enable Developer Mode or run the test as administrator"
            );
            None
        }
        Err(e) => panic!("symlink {} -> {}: {e}", link.display(), target.display()),
    }
}

#[test]
fn a_symlink_loop_under_the_directory_is_refused_not_followed() {
    // `captures/loop` -> the snapshot directory itself: following it would
    // recurse without bound (a stack overflow at boot instead of an error).
    let dir = build("symlink-loop");
    let link = dir.join("captures").join("loop");
    if symlink(&dir, &link, true).is_none() {
        return;
    }
    let err = load_error(&dir);
    assert_eq!(err.code, codes::SYMLINK_REFUSED, "{err}");
    assert!(err.message.contains("captures/loop"), "{err}");
    assert_eq!(open_error(&dir).code, codes::SYMLINK_REFUSED);
}

#[test]
fn a_symlinked_artifact_pointing_outside_the_directory_is_refused() {
    // The isolation witness replaced by a link to a file outside the
    // directory holding the same bytes: every digest would match if the
    // link were followed.
    let dir = build("symlink-artifact");
    let outside = dir.parent().unwrap().join("outside.witness");
    let witness = dir.join("captures").join(format!("{ISOLATION_ID}.witness"));
    std::fs::copy(&witness, &outside).unwrap();
    std::fs::remove_file(&witness).unwrap();
    if symlink(&outside, &witness, false).is_none() {
        return;
    }
    let err = load_error(&dir);
    assert_eq!(err.code, codes::SYMLINK_REFUSED, "{err}");
    assert!(
        err.message
            .contains(&format!("captures/{ISOLATION_ID}.witness")),
        "{err}"
    );
    assert!(
        !err.message.contains("outside.witness"),
        "names the artifact, not the link target: {err}"
    );
    assert_eq!(open_error(&dir).code, codes::SYMLINK_REFUSED);
    // A link in place of SHA256SUMS itself is refused by the first read.
    let dir = build("symlink-sums");
    let sums = dir.join("SHA256SUMS");
    let outside = dir.parent().unwrap().join("outside.SHA256SUMS");
    std::fs::copy(&sums, &outside).unwrap();
    std::fs::remove_file(&sums).unwrap();
    symlink(&outside, &sums, false).unwrap();
    let err = load_error(&dir);
    assert_eq!(err.code, codes::SYMLINK_REFUSED, "{err}");
    assert!(err.message.starts_with("SHA256SUMS"), "{err}");
}

#[test]
fn a_directory_nested_deeper_than_the_flat_layout_is_refused() {
    // No symlink needed: the walk stops at MAX_DEPTH components.
    let dir = build("too-deep");
    let deep = dir.join("captures").join("deep");
    std::fs::create_dir_all(deep.join("deeper")).unwrap();
    std::fs::write(deep.join("deeper").join("x.txt"), "x").unwrap();
    let err = load_error(&dir);
    assert_eq!(err.code, codes::LAYOUT_TOO_DEEP, "{err}");
    assert!(err.message.contains("captures/deep"), "{err}");
    assert!(
        !err.message.contains("captures/deep/deeper"),
        "stops at the first directory too deep: {err}"
    );
    assert_eq!(open_error(&dir).code, codes::LAYOUT_TOO_DEEP);
    // A first-level directory is within the layout: it is walked, and its
    // unlisted file is reported as such.
    let dir = build("first-level-dir");
    std::fs::create_dir_all(dir.join("extra")).unwrap();
    std::fs::write(dir.join("extra").join("note.txt"), "x").unwrap();
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CHECKSUMS_INCOMPLETE, "{err}");
    assert!(err.message.contains("extra/note.txt"), "{err}");
}

#[test]
fn status_rehashes_the_served_artifacts_on_every_call() {
    let dir = build("status-rehash");
    let (mut client, _) = open(&dir);
    let boot_summary = client.snapshot_status().summary();
    let healthy = client.status().unwrap();
    assert!(healthy.healthy);
    assert_eq!(healthy.detail, boot_summary);
    client
        .check_artifacts()
        .expect("the artifacts are as verified");

    // The isolation witness altered after boot (a flipped byte): unhealthy
    // with the code and the artifact's relative path, the boot summary kept.
    let witness = dir.join("captures").join(format!("{ISOLATION_ID}.witness"));
    let original = std::fs::read(&witness).unwrap();
    flip_first_byte(&witness);
    let status = client.status().unwrap();
    assert!(!status.healthy, "{}", status.detail);
    assert!(
        status.detail.starts_with(codes::DIGEST_MISMATCH),
        "{}",
        status.detail
    );
    assert!(
        status
            .detail
            .contains(&format!("captures/{ISOLATION_ID}.witness")),
        "{}",
        status.detail
    );
    assert!(status.detail.contains(&boot_summary), "{}", status.detail);
    assert!(
        !status.detail.contains(&dir.to_string_lossy().into_owned()),
        "no filesystem path: {}",
        status.detail
    );
    assert!(
        !status.detail.contains("synthetic witness bytes"),
        "no witness content: {}",
        status.detail
    );
    let err = client.check_artifacts().unwrap_err();
    assert_eq!(
        err.code,
        codes::DIGEST_MISMATCH,
        "the code for /health: {err}"
    );
    // A clone handed to a run sees the same disk.
    assert!(!client.clone().status().unwrap().healthy);

    // Truncated.
    std::fs::write(&witness, &original[..original.len() / 2]).unwrap();
    let status = client.status().unwrap();
    assert!(
        !status.healthy && status.detail.starts_with(codes::DIGEST_MISMATCH),
        "{}",
        status.detail
    );
    // Deleted.
    std::fs::remove_file(&witness).unwrap();
    let status = client.status().unwrap();
    assert!(!status.healthy, "{}", status.detail);
    assert!(
        status.detail.starts_with(codes::FILE_UNREADABLE),
        "{}",
        status.detail
    );
    assert!(
        status
            .detail
            .contains(&format!("captures/{ISOLATION_ID}.witness")),
        "{}",
        status.detail
    );
    // Restored: the check is of the files as they are now.
    std::fs::write(&witness, &original).unwrap();
    assert!(client.status().unwrap().healthy);

    // The list capture's proof replaced, then the manifest rewritten.
    let proof = dir.join("captures").join(format!("{LIST_ID}.proof"));
    flip_first_byte(&proof);
    let status = client.status().unwrap();
    assert!(
        !status.healthy && status.detail.contains(&format!("captures/{LIST_ID}.proof")),
        "{}",
        status.detail
    );
    flip_first_byte(&proof);
    assert!(client.status().unwrap().healthy);
    edit_manifest(&dir, |m| m["captured_at"] = json!("2026-10-30T00:00:00Z"));
    let status = client.status().unwrap();
    assert!(
        !status.healthy
            && status.detail.starts_with(codes::DIGEST_MISMATCH)
            && status.detail.contains("manifest.json"),
        "{}",
        status.detail
    );
    // Recalls still answer from the records verified at boot (the client
    // is plain data); only the health report changes.
    assert_eq!(
        client
            .recall(&ProjectId::new(PROJECT), "q", 5)
            .unwrap()
            .len(),
        7
    );
}

#[test]
fn the_pinned_anchor_must_be_the_manifest_anchor() {
    let dir = build("pinned-anchor");
    let mut cfg = config(&dir, "hyphae");
    cfg.expected_anchor = "cd".repeat(32);
    let mut verifier = scripted(&cfg, None);
    let err = SnapshotMemoryClient::open(&cfg, &mut verifier).unwrap_err();
    assert_eq!(err.code, codes::PINNED_ANCHOR_MISMATCH, "{err}");
    assert!(
        err.message.contains("CONSENSUS_HYPHAE_EXPECTED_ANCHOR"),
        "{err}"
    );
    assert!(
        verifier.runner.calls.is_empty(),
        "nothing is verified against the wrong anchor"
    );
}

#[test]
fn the_required_captures_must_be_present_and_unique() {
    // Without the isolation capture.
    let dir = build_with("no-isolation", &[list_spec()], |_| {});
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CAPTURE_MISSING, "{err}");
    assert!(err.message.contains("recall"), "{err}");
    // Without the list capture.
    let dir = build_with("no-list", &[isolation_spec()], |_| {});
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CAPTURE_MISSING, "{err}");
    assert!(err.message.contains("list"), "{err}");
    // A list with another limit is not the required one.
    let dir = build_with("list-limit-8", &[list_spec(), isolation_spec()], |m| {
        m["captures"][0]["arguments"]["limit"] = json!(8)
    });
    assert_eq!(load_error(&dir).code, codes::CAPTURE_MISSING);
    // Two candidates for the list, neither named as the contract names the
    // required one (list-<project>): ambiguous.
    let mut first = list_spec();
    first.id = "list-demo-repo-first";
    let mut second = list_spec();
    second.id = "list-demo-repo-again";
    let dir = build_with(
        "two-lists",
        &[first.clone(), isolation_spec(), second.clone()],
        |_| {},
    );
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CAPTURE_AMBIGUOUS, "{err}");
    assert!(
        err.message.contains("list-demo-repo-first")
            && err.message.contains("list-demo-repo-again"),
        "{err}"
    );
    assert!(err.message.contains("contract"), "{err}");
    // With the contract-named capture among them, that one is served and
    // the others are extras (additional captures are allowed).
    let dir = build_with(
        "two-lists-one-named",
        &[second, list_spec(), isolation_spec(), first],
        |_| {},
    );
    let snapshot = Snapshot::load(&dir, None).unwrap();
    assert_eq!(snapshot.list_capture().id, LIST_ID);
    let (client, verifier) = open(&dir);
    assert_eq!(verifier.runner.calls.len(), 2);
    assert_eq!(client.snapshot_status().captures[0].id, LIST_ID);
    // The same for the isolation recall: a second empty recall of another
    // project (or the same one with another query) is an extra when the
    // contract-named one is present, ambiguous otherwise.
    let mut third = isolation_spec();
    third.id = "recall-third-repo-empty";
    third.project = "third-repo";
    let mut other_query = isolation_spec();
    other_query.id = "recall-other-repo-preset-2";
    other_query.query = "Raise payment retry attempts to 10";
    let dir = build_with(
        "two-isolations-one-named",
        &[
            third.clone(),
            list_spec(),
            other_query.clone(),
            isolation_spec(),
        ],
        |_| {},
    );
    let snapshot = Snapshot::load(&dir, None).unwrap();
    assert_eq!(snapshot.isolation_capture().id, ISOLATION_ID);
    assert_eq!(open(&dir).0.isolation_project(), &ProjectId::new(OTHER));
    let dir = build_with(
        "two-isolations-unnamed",
        &[list_spec(), third, other_query],
        |_| {},
    );
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CAPTURE_AMBIGUOUS, "{err}");
    assert!(err.message.contains("recall-third-repo-empty"), "{err}");
    // An isolation capture that returned records is not empty evidence.
    let mut full = isolation_spec();
    full.response_fixture = "list-demo-repo.response.json";
    full.proof_blake3 = list_spec().proof_blake3;
    let dir = build_with("isolation-not-empty", &[list_spec(), full], |_| {});
    assert_eq!(load_error(&dir).code, codes::CAPTURE_MISSING);
    // Additional captures are allowed; the runtime serves only the two.
    let extra = Spec {
        id: "recall-demo-repo-preset-1",
        operation: "recall",
        project: PROJECT,
        query: "Raise payment retry attempts to 10",
        limit: 8,
        response_fixture: "list-demo-repo.response.json",
        verify_fixture: "list-demo-repo.verify.json",
        proof_blake3: list_spec().proof_blake3,
    };
    let dir = build_with(
        "extra-capture",
        &[extra, list_spec(), isolation_spec()],
        |_| {},
    );
    let snapshot = Snapshot::load(&dir, None).unwrap();
    assert_eq!(snapshot.manifest.captures.len(), 3);
    assert_eq!(snapshot.list_capture().id, LIST_ID);
    let (mut client, verifier) = open(&dir);
    assert_eq!(
        verifier.runner.calls.len(),
        2,
        "only the served captures are boot-verified"
    );
    let recall = client
        .recall_with_proof(
            &ProjectId::new(PROJECT),
            "Raise payment retry attempts to 10",
            8,
        )
        .unwrap();
    assert_eq!(recall.proof.unwrap().proof_ref, LIST_ID);
}

#[test]
fn boot_verification_requires_a_valid_memory_proof_at_the_anchor() {
    let dir = build("boot-verification");
    let cfg = config(&dir, "hyphae");
    let report_with = |key: &str, value: Value| report_with(LIST_ID, key, value);
    let corruption = r#"{"error":{"category":"corruption","code":"corruption","message":"native durable state is invalid"},"exit_class":9}"#;
    let io = r#"{"error":{"category":"io","code":"io","message":"native filesystem operation failed"},"exit_class":11}"#;
    for (label, outcome, status) in [
        (
            "another proof kind",
            exited(0, &report_with("kind", json!("catalog")), ""),
            "Invalid",
        ),
        (
            "another anchor",
            exited(0, &report_with("anchor_digest", json!("ab".repeat(32))), ""),
            "Invalid",
        ),
        (
            "a lesser scope",
            exited(0, &report_with("scope", json!("artifact_integrity")), ""),
            "VerificationError",
        ),
        ("no report at all", exited(0, "", ""), "VerificationError"),
        ("rejected artifacts", exited(9, "", corruption), "Invalid"),
        ("an io failure", exited(11, "", io), "VerificationError"),
        (
            "a verifier past its deadline",
            Err(CommandError::TimedOut(Duration::from_secs(60))),
            "VerificationError",
        ),
    ] {
        let mut verifier = scripted(&cfg, Some(outcome));
        let err = SnapshotMemoryClient::open(&cfg, &mut verifier).unwrap_err();
        assert_eq!(err.code, codes::BOOT_VERIFICATION_FAILED, "{label}: {err}");
        assert!(
            err.message.contains(LIST_ID),
            "{label}: names the capture: {err}"
        );
        assert!(err.message.contains(status), "{label}: {err}");
        assert_eq!(
            verifier.runner.calls.len(),
            1,
            "{label}: stops at the first failure"
        );
    }

    // A verified memory proof at the right anchor whose digests are not the
    // capture-time record's: the artifacts verified now are not the ones
    // verified at capture (here the list capture gets the isolation
    // recall's report, as it would if its files had been overwritten).
    let other_proof = report_with(
        "proof_digest",
        json!("7b7b7b7b1c1c1c1c7b7b7b7b1c1c1c1c7b7b7b7b1c1c1c1c7b7b7b7b1c1c1c1c"),
    );
    let other_result = report_with("result_digest", json!("00".repeat(32)));
    let mut no_proof_digest: Value = serde_json::from_str(&verify_report(LIST_ID)).unwrap();
    no_proof_digest
        .as_object_mut()
        .unwrap()
        .remove("proof_digest");
    for (label, stdout, what) in [
        ("another proof", verify_report(ISOLATION_ID), "proof digest"),
        ("another proof digest", other_proof, "proof digest"),
        ("another result digest", other_result, "result digest"),
        (
            "no proof digest",
            no_proof_digest.to_string(),
            "proof digest",
        ),
    ] {
        let mut verifier = scripted(&cfg, Some(exited(0, &stdout, "")));
        let err = SnapshotMemoryClient::open(&cfg, &mut verifier).unwrap_err();
        assert_eq!(err.code, codes::BOOT_VERIFICATION_FAILED, "{label}: {err}");
        assert!(
            err.message.contains(LIST_ID) && err.message.contains(what),
            "{label}: {err}"
        );
        assert!(
            err.message
                .contains(&format!("captures/{LIST_ID}.verify.json")),
            "{label}: names the capture-time record: {err}"
        );
        assert_eq!(verifier.runner.calls.len(), 1, "{label}");
    }
}

/// A verifier double answering `Valid` with whatever anchors it is told
/// to: the boot check must not take `Valid` on faith.
struct AnchorDouble {
    verified_anchor: Option<String>,
    report_anchor: Option<String>,
    calls: usize,
}

impl ProofVerifier for AnchorDouble {
    fn verify(&mut self, evidence: &EvidenceReference) -> ProofCheck {
        self.calls += 1;
        let id = capture_of_proof(evidence.proof_file.as_deref().unwrap());
        let recorded: Value = serde_json::from_str(&verify_report(&id)).unwrap();
        ProofCheck {
            status: IntegrityStatus::Valid,
            verifier: Some("anchor double".into()),
            verified_anchor: self.verified_anchor.clone(),
            detail: Some("scripted".into()),
            report: Some(VerifierReport {
                status: Some("verified".into()),
                scope: Some("semantic_reexecution".into()),
                kind: Some("memory".into()),
                anchor_digest: self.report_anchor.clone(),
                result_digest: recorded["result_digest"].as_str().map(str::to_string),
                proof_digest: recorded["proof_digest"].as_str().map(str::to_string),
            }),
        }
    }
}

#[test]
fn boot_verification_requires_the_manifest_anchor_from_the_verifier_itself() {
    let dir = build("boot-anchors");
    let snapshot = Snapshot::load(&dir, None).unwrap();
    let other = "ab".repeat(32);
    for (label, verified_anchor, report_anchor) in [
        (
            "another verified anchor",
            Some(other.clone()),
            Some(ANCHOR.to_string()),
        ),
        ("no verified anchor", None, Some(ANCHOR.to_string())),
        (
            "another report anchor",
            Some(ANCHOR.to_string()),
            Some(other.clone()),
        ),
        ("no report anchor", Some(ANCHOR.to_string()), None),
    ] {
        let mut double = AnchorDouble {
            verified_anchor,
            report_anchor,
            calls: 0,
        };
        let err =
            SnapshotMemoryClient::from_snapshot(snapshot.clone(), ANCHOR, &mut double).unwrap_err();
        assert_eq!(err.code, codes::BOOT_VERIFICATION_FAILED, "{label}: {err}");
        assert!(err.message.contains(LIST_ID), "{label}: {err}");
        assert_eq!(double.calls, 1, "{label}: stops at the first failure");
    }
    // The same double with the manifest anchor in both places is accepted:
    // the anchors were what failed above. Case and whitespace are
    // normalized as the live adapter does.
    let mut double = AnchorDouble {
        verified_anchor: Some(format!(" {}\n", ANCHOR.to_ascii_uppercase())),
        report_anchor: Some(ANCHOR.to_ascii_uppercase()),
        calls: 0,
    };
    let client = SnapshotMemoryClient::from_snapshot(snapshot, ANCHOR, &mut double).unwrap();
    assert_eq!(double.calls, 2);
    assert_eq!(
        client.snapshot_status().captures[1].verifier.as_deref(),
        Some("anchor double")
    );
}

#[test]
fn one_proof_cannot_stand_for_both_served_captures() {
    let capture_file = |dir: &Path, name: &str| dir.join("captures").join(name);
    // The isolation recall's proof and witness written into the list
    // capture's files, digests and SHA256SUMS recomputed afterwards (a
    // copy-step bug): the two served captures now name the same proof.
    let dir = build("shared-proof");
    for suffix in ["proof", "witness"] {
        std::fs::copy(
            capture_file(&dir, &format!("{ISOLATION_ID}.{suffix}")),
            capture_file(&dir, &format!("{LIST_ID}.{suffix}")),
        )
        .unwrap();
    }
    recompute_digests(&dir);
    let err = load_error(&dir);
    assert_eq!(err.code, codes::PROOF_SHARED, "{err}");
    assert!(
        err.message.contains(LIST_ID) && err.message.contains(ISOLATION_ID),
        "{err}"
    );
    assert_eq!(open_error(&dir).code, codes::PROOF_SHARED);

    // The same response-reported proof_blake3 on both captures.
    let dir = build("shared-blake3");
    let response_path = capture_file(&dir, &format!("{ISOLATION_ID}.response.json"));
    let mut response: Value =
        serde_json::from_slice(&std::fs::read(&response_path).unwrap()).unwrap();
    response["proof"]["proof_blake3"] = json!(list_spec().proof_blake3);
    std::fs::write(&response_path, response.to_string()).unwrap();
    edit_manifest(&dir, |m| {
        m["captures"][1]["proof_blake3"] = json!(list_spec().proof_blake3)
    });
    write_checksums(&dir);
    assert_eq!(load_error(&dir).code, codes::PROOF_SHARED);

    // Proof and witness swapped BETWEEN the captures, capture-time records
    // kept: distinct proofs, so the load passes; the boot verification
    // then reports the other capture's digests for each and fails.
    let dir = build("swapped-artifacts");
    for suffix in ["proof", "witness"] {
        let list = capture_file(&dir, &format!("{LIST_ID}.{suffix}"));
        let isolation = capture_file(&dir, &format!("{ISOLATION_ID}.{suffix}"));
        let kept = std::fs::read(&list).unwrap();
        std::fs::copy(&isolation, &list).unwrap();
        std::fs::write(&isolation, kept).unwrap();
    }
    recompute_digests(&dir);
    Snapshot::load(&dir, None).expect("structurally consistent");
    let err = open_error(&dir);
    assert_eq!(err.code, codes::BOOT_VERIFICATION_FAILED, "{err}");
    assert!(
        err.message.contains(LIST_ID) && err.message.contains("proof digest"),
        "{err}"
    );
}

#[test]
fn artifact_paths_are_mapped_to_the_verifier_directory_when_configured() {
    let dir = build("mapped");
    let mut cfg = config(&dir, "scripts/hyphae-docker.cmd");
    cfg.verifier_dir = Some("/snapshots/snap-2026-10-29-demo-repo/".into());
    let mut verifier = scripted(&cfg, None);
    let mut client = SnapshotMemoryClient::open(&cfg, &mut verifier).unwrap();
    let mapped = |name: &str| format!("/snapshots/snap-2026-10-29-demo-repo/captures/{name}");
    // The verifier was given the container paths, joined with `/`.
    let call = &verifier.runner.calls[0];
    assert_eq!(call[0], "scripts/hyphae-docker.cmd");
    assert!(
        call.contains(&mapped(&format!("{LIST_ID}.proof"))),
        "{call:?}"
    );
    assert!(
        call.contains(&mapped(&format!("{LIST_ID}.witness"))),
        "{call:?}"
    );
    assert!(verifier.runner.calls[1].contains(&mapped(&format!("{ISOLATION_ID}.proof"))));
    // And so are the records, so the per-run verifier reads the same paths.
    let records = client.recall(&ProjectId::new(PROJECT), "q", 5).unwrap();
    assert_eq!(
        records[0].proof_file.as_deref(),
        Some(mapped(&format!("{LIST_ID}.proof")).as_str())
    );
    assert_eq!(
        records[0].witness_file.as_deref(),
        Some(mapped(&format!("{LIST_ID}.witness")).as_str())
    );
    // The digests still come from the local files.
    assert_eq!(
        records[0].proof_sha256.as_deref(),
        Some(sha256_hex(&proof_bytes(LIST_ID)).as_str())
    );
    // A mapped path is not readable here, so the verifier's content-digest
    // cache cannot apply: every check runs the binary (observed, HY-13).
    let before = verifier.runner.calls.len();
    verifier.verify(&records[0]);
    verifier.verify(&records[1]);
    assert_eq!(verifier.runner.calls.len(), before + 2);
}

#[test]
fn the_client_is_plain_data_that_can_be_shared_between_runs() {
    fn assert_send_sync_clone<T: Send + Sync + Clone + 'static>() {}
    assert_send_sync_clone::<SnapshotMemoryClient>();
    let dir = build("shared");
    let (client, _) = open(&dir);
    let boxed: Box<dyn MemoryClient + Send> = Box::new(client.clone());
    drop(boxed);
    let mut copy = client.clone();
    assert_eq!(
        copy.recall(&ProjectId::new(PROJECT), "", 5).unwrap().len(),
        7
    );
    assert!(format!("{client:?}").contains(SNAPSHOT_ID));
}

/// A two-claim run: the Critic finds nothing, the Evaluator corroborates
/// both claims from the records `first` and `second` cite.
fn scripted_model(first: &[&str], second: &[&str]) -> FakeModelClient {
    let mut model = FakeModelClient::new();
    model.proposals.push_back(Ok(proposal(
        "p1",
        "Cap payment retries at 3 attempts with jitter",
        vec![
            claim(
                "c1",
                "Payment timeouts correlate with retry bursts above 5 attempts",
            ),
            claim(
                "c2",
                "The documented decision caps retries at 3 attempts with jitter",
            ),
        ],
    )));
    model.critiques.push_back(Ok(Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new("chal-1"),
        run_id: RunId::new("placeholder"),
        target_proposal: ProposalId::new("placeholder"),
        items: vec![],
        created_at: TS.into(),
    }));
    model.evaluations.push_back(Ok(evaluation(
        vec![
            assessment("c1", !first.is_empty(), first, &[]),
            assessment("c2", !second.is_empty(), second, &[]),
        ],
        &[],
        false,
    )));
    model
}

fn run<V: ProofVerifier>(
    log_name: &str,
    project: &str,
    model: FakeModelClient,
    memory: SnapshotMemoryClient,
    verifier: V,
) -> (RunAggregate, EventLog, String) {
    let path = temp_dir("snapshot-runs").join(log_name);
    let mut log = EventLog::open(&path).unwrap();
    let mut orch = Orchestrator {
        model,
        memory,
        verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let input = TaskInput {
        project_id: ProjectId::new(project),
        title: "Fix payment client timeouts".into(),
        description: "Payment requests intermittently time out under load.".into(),
        submitted_by: "test".into(),
        mode: Some(TaskMode {
            model: ModelMode::Scripted,
            evidence: EvidenceMode::SealedSnapshot,
            origin: RunOrigin::Public,
            preset: Some("retry-fix".into()),
        }),
    };
    let agg = orch.run(&mut log, input).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    (agg, log, text)
}

#[test]
fn an_orchestrated_run_over_the_snapshot_is_proven_verified_and_recommended() {
    // The runs' event logs (`run` writes them there); removed last.
    let _runs = TempRoot::new("snapshot-runs");
    let dir = build("orchestrated");
    let root = dir.parent().unwrap().to_path_buf();
    let (binary, log_file) = fake_hyphae(&root);
    let cfg = config(&dir, &binary);

    // Boot: both captures through the fake binary.
    let mut boot_verifier = verifier_from_config(&cfg, SystemCommandRunner);
    let client = SnapshotMemoryClient::open(&cfg, &mut boot_verifier)
        .unwrap()
        .with_time_source(|| TS.to_string());
    let boot_calls = logged_calls(&log_file);
    assert_eq!(boot_calls.len(), 2, "{boot_calls:?}");
    for call in &boot_calls {
        assert!(call.starts_with("proof verify --proof"), "{call}");
        assert!(call.contains(&format!("--anchor {ANCHOR}")), "{call}");
    }
    assert_eq!(
        client.snapshot_status().captures[0].integrity,
        IntegrityStatus::Valid
    );

    // A run with a fresh per-run verifier: the proofs are re-verified.
    let constraint = RECORD_IDS[0];
    let fact = RECORD_IDS[1];
    let decision = RECORD_IDS[5];
    let (agg, log, text) = run(
        "snapshot-supported.jsonl",
        PROJECT,
        scripted_model(&[constraint, fact], &[decision]),
        client.clone(),
        verifier_from_config(&cfg, SystemCommandRunner),
    );
    assert_eq!(agg.state, RunState::AwaitingHumanApproval);
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Supported),
        "{:?}",
        agg.recommendation
    );
    let retrieved: Vec<&str> = agg.retrieved.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        retrieved, RECORD_IDS,
        "the whole proven set, once, in capture order"
    );
    for rec in &agg.retrieved {
        assert_eq!(rec.retrieval, Some(Retrieval::SealedSnapshot));
        assert_eq!(rec.proof_ref.as_deref(), Some(LIST_ID));
        assert_eq!(
            rec.proof_request.as_ref().map(|r| r.operation.as_str()),
            Some("list")
        );
    }
    let events = log.all_events();
    // One RecallProven per recall (two claims), each covering all seven.
    let proven: Vec<(Vec<&str>, &str, bool, &str)> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::RecallProven {
                request,
                evidence_ids,
                proof_ref,
                anchor,
                empty,
            } => {
                assert_eq!(anchor, ANCHOR);
                assert_eq!(request.project, PROJECT);
                assert_eq!(request.limit, Some(64));
                Some((
                    evidence_ids.iter().map(|i| i.as_str()).collect(),
                    proof_ref.as_str(),
                    *empty,
                    request.operation.as_str(),
                ))
            }
            _ => None,
        })
        .collect();
    assert_eq!(proven.len(), 2, "{proven:?}");
    for (ids, proof_ref, empty, operation) in &proven {
        assert_eq!(ids, &RECORD_IDS.to_vec());
        assert_eq!((*proof_ref, *empty, *operation), (LIST_ID, false, "list"));
    }
    assert!(
        !events
            .iter()
            .any(|e| matches!(e.payload, EventPayload::EvidenceUnavailable { .. })),
        "nothing was unavailable"
    );
    // Every record verified by the real verifier path, report kept.
    assert_eq!(agg.verifications.len(), 7);
    for v in &agg.verifications {
        assert_eq!(v.integrity_status, IntegrityStatus::Valid, "{v:?}");
        assert_eq!(v.verifier.as_deref(), Some("hyphae proof verify"));
        assert_eq!(v.verified_anchor.as_deref(), Some(ANCHOR));
        assert_eq!(v.proof_ref.as_deref(), Some(LIST_ID));
        assert_eq!(v.round, Some(1));
        let report = v.report.as_ref().expect("report");
        assert_eq!(report.kind.as_deref(), Some("memory"));
        assert_eq!(report.scope.as_deref(), Some("semantic_reexecution"));
        assert_eq!(report.status.as_deref(), Some("verified"));
        assert!(RECORD_IDS.contains(&v.evidence_id.as_str()));
    }
    // The per-run re-verification ran the binary once for the shared
    // proof; the other six records (same artifact bytes, same anchor) were
    // served from the verifier's content-digest cache (HY-13).
    let after_run = logged_calls(&log_file);
    assert_eq!(after_run.len(), boot_calls.len() + 1, "{after_run:?}");
    // Replays, and persists no witness content.
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
    assert!(!text.contains("synthetic witness bytes"));
    assert!(text.contains("\"retrieval\":\"sealed_snapshot\""));
    assert!(text.contains(&format!("\"snapshot_id\":\"{SNAPSHOT_ID}\"")));

    // The isolation project: no records, the empty proof recorded per
    // recall, nothing to cite.
    let (agg, log, _) = run(
        "snapshot-isolation.jsonl",
        OTHER,
        scripted_model(&[], &[]),
        client.clone(),
        verifier_from_config(&cfg, SystemCommandRunner),
    );
    assert!(agg.retrieved.is_empty());
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::InsufficientEvidence)
    );
    let empty_proofs = log
        .all_events()
        .iter()
        .filter(|e| {
            matches!(&e.payload, EventPayload::RecallProven { empty: true, evidence_ids, proof_ref, request, .. }
                if evidence_ids.is_empty() && proof_ref == ISOLATION_ID && request.project == OTHER
                    && request.operation == "recall")
        })
        .count();
    assert_eq!(empty_proofs, 2, "one per claim");
    assert_eq!(
        logged_calls(&log_file).len(),
        after_run.len(),
        "no record, nothing to verify"
    );

    // Any other project: the reason is on the record, with its code.
    let (agg, log, _) = run(
        "snapshot-unknown.jsonl",
        "third-repo",
        scripted_model(&[], &[]),
        client,
        verifier_from_config(&cfg, SystemCommandRunner),
    );
    assert!(agg.retrieved.is_empty());
    let unavailable: Vec<(Option<&str>, &str)> = log
        .all_events()
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::EvidenceUnavailable { code, reason, .. } => {
                Some((code.as_deref(), reason.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(unavailable.len(), 2, "{unavailable:?}");
    for (code, reason) in unavailable {
        assert_eq!(code, Some(run_codes::MEMORY_ERROR));
        assert!(
            reason.contains("\"third-repo\"") && reason.contains("not captured"),
            "{reason}"
        );
    }
}
