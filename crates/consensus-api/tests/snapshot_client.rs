// SPDX-License-Identifier: Apache-2.0
//! The sealed snapshot client (ADR-0008, plan B11): loading and checking a
//! snapshot directory, boot verification through the real
//! `HyphaeProofVerifier` (a scripted command runner, and a FAKE `hyphae`
//! binary that records every invocation), query-independent recalls, the
//! isolation and unknown-project paths, and an end-to-end run through the
//! orchestrator with the scripted model. No Hyphae runs here: the snapshot
//! is assembled from `tests/fixtures/snapshot/` with synthetic proof and
//! witness bytes.

mod support;

use consensus_api::config::SnapshotConfig;
use consensus_api::hyphae::{
    CommandError, CommandOutput, CommandRunner, HyphaeProofVerifier, SystemCommandRunner,
};
use consensus_api::snapshot::{
    codes, verifier_from_config, Snapshot, SnapshotError, SnapshotMemoryClient, MANIFEST_SCHEMA,
};
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::fakes::FakeModelClient;
use consensus_runtime::orchestrator::{codes as run_codes, Orchestrator, TaskInput};
use consensus_runtime::ports::{MemoryClient, MemoryError, ProofVerifier, RecallProof};
use consensus_runtime::scenarios::{assessment, claim, evaluation, proposal, TS};
use consensus_runtime::{EventLog, RunAggregate, RunLimits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/snapshot");
const SNAPSHOT_ID: &str = "snap-2026-10-29-demo-repo";
const ANCHOR: &str = "4e1f0c9a7b3d5e2f8a6c4b0d9e7f1a3c5b8d2e4f6a0c1b3d5e7f9a2c4e6b8d0f";
const PROJECT: &str = "demo-repo";
const OTHER: &str = "other-repo";
const LIST_ID: &str = "list-demo-repo";
const ISOLATION_ID: &str = "recall-other-repo-isolation";
const ISOLATION_QUERY: &str = "Fix payment client timeouts";
const CAPTURED_AT: &str = "2026-10-29T15:04:05Z";
const VERIFIED_AT: &str = "2026-10-29T15:04:09Z";
/// The seven demo-repo records, in capture order (see the fixture README).
const RECORD_IDS: [&str; 7] = [
    "186670067796945807561772405393844769735",
    "8864874238917673930644756936982232317",
    "287567405605917891851290763137356115373",
    "161896895843658017381255677281559699870",
    "237412834382378687445660534362297845298",
    "328114099256634798832222840773545327795",
    "295729039551322334710406856728557559035",
];

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).expect(name)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One capture to assemble: its manifest entry's fields and which fixture
/// response it carries.
#[derive(Clone)]
struct Spec {
    id: &'static str,
    operation: &'static str,
    project: &'static str,
    query: &'static str,
    limit: u32,
    response_fixture: &'static str,
    proof_blake3: &'static str,
}

fn list_spec() -> Spec {
    Spec {
        id: LIST_ID,
        operation: "list",
        project: PROJECT,
        query: "",
        limit: 64,
        response_fixture: "list-demo-repo.response.json",
        proof_blake3: "a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4",
    }
}

fn isolation_spec() -> Spec {
    Spec {
        id: ISOLATION_ID,
        operation: "recall",
        project: OTHER,
        query: ISOLATION_QUERY,
        limit: 8,
        response_fixture: "recall-other-repo-isolation.response.json",
        proof_blake3: "e5f60718e5f60718e5f60718e5f60718e5f60718e5f60718e5f60718e5f60718",
    }
}

fn proof_bytes(id: &str) -> Vec<u8> {
    format!("synthetic proof bytes of {id}\n").into_bytes()
}

fn witness_bytes(id: &str) -> Vec<u8> {
    format!("synthetic witness bytes of {id}: a private copy of the data directory\n").into_bytes()
}

/// Every file under `dir` as `/`-separated relative paths, sorted.
fn files_under(dir: &Path) -> Vec<String> {
    fn visit(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.path().is_dir() {
                visit(&entry.path(), &relative, out);
            } else {
                out.push(relative);
            }
        }
    }
    let mut out = Vec::new();
    visit(dir, "", &mut out);
    out.sort();
    out
}

/// (Re)writes `SHA256SUMS` for every other file under `dir`.
fn write_checksums(dir: &Path) {
    let mut text = String::new();
    for relative in files_under(dir) {
        if relative == "SHA256SUMS" {
            continue;
        }
        let bytes = std::fs::read(dir.join(&relative)).unwrap();
        text.push_str(&format!("{}  {relative}\n", sha256_hex(&bytes)));
    }
    std::fs::write(dir.join("SHA256SUMS"), text).unwrap();
}

fn capture_entry(spec: &Spec, dir: &Path) -> Value {
    let response: Value = serde_json::from_str(&fixture(spec.response_fixture)).unwrap();
    let memory_ids: Vec<Value> = response["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    let id = spec.id;
    let proof = std::fs::read(dir.join("captures").join(format!("{id}.proof"))).unwrap();
    let witness = std::fs::read(dir.join("captures").join(format!("{id}.witness"))).unwrap();
    json!({
        "id": id,
        "operation": spec.operation,
        "arguments": {"project": spec.project, "query": spec.query, "limit": spec.limit, "prove": true},
        "response_file": format!("captures/{id}.response.json"),
        "proof_file": format!("captures/{id}.proof"),
        "witness_file": format!("captures/{id}.witness"),
        "verify_file": format!("captures/{id}.verify.json"),
        "proof_blake3": spec.proof_blake3,
        "proof_sha256": sha256_hex(&proof),
        "witness_sha256": sha256_hex(&witness),
        "memory_ids": memory_ids,
        "verification": {"verified_at": VERIFIED_AT, "status": "verified", "kind": "memory",
                         "scope": "semantic_reexecution", "anchor_hex": ANCHOR}
    })
}

/// Assembles a complete snapshot directory `<temp>/<SNAPSHOT_ID>` from the
/// fixtures and synthetic artifacts, lets `edit` change the manifest, then
/// writes `SHA256SUMS` over the result.
fn build_with(test: &str, specs: &[Spec], edit: impl FnOnce(&mut Value)) -> PathBuf {
    let root = support::temp_dir(&format!("snapshot-{test}"));
    let dir = root.join(SNAPSHOT_ID);
    std::fs::create_dir_all(dir.join("captures")).unwrap();
    for spec in specs {
        let captures = dir.join("captures");
        let id = spec.id;
        std::fs::write(
            captures.join(format!("{id}.response.json")),
            fixture(spec.response_fixture),
        )
        .unwrap();
        std::fs::write(captures.join(format!("{id}.proof")), proof_bytes(id)).unwrap();
        std::fs::write(captures.join(format!("{id}.witness")), witness_bytes(id)).unwrap();
        std::fs::write(
            captures.join(format!("{id}.verify.json")),
            fixture("verify-report.json"),
        )
        .unwrap();
    }
    let list: Value = serde_json::from_str(&fixture("list-demo-repo.response.json")).unwrap();
    let mut manifest = json!({
        "schema": MANIFEST_SCHEMA,
        "snapshot_id": SNAPSHOT_ID,
        "captured_at": CAPTURED_AT,
        "project": PROJECT,
        "anchor_hex": ANCHOR,
        "hyphae": {"version": "hyphae 4.0.0", "binary_sha256": "f0e1d2c3".repeat(8),
                   "control_schema": "hyphae-omarchy-control-v1"},
        "store": {"record_count": 7, "root_digest": list["snapshot"]["root_digest"],
                  "visible_csn": list["snapshot"]["visible_csn"],
                  "logical_time_micros": list["snapshot"]["logical_time_micros"]},
        "captures": specs.iter().map(|s| capture_entry(s, &dir)).collect::<Vec<_>>()
    });
    edit(&mut manifest);
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    write_checksums(&dir);
    dir
}

fn build(test: &str) -> PathBuf {
    build_with(test, &[list_spec(), isolation_spec()], |_| {})
}

fn config(dir: &Path, binary: &str) -> SnapshotConfig {
    let vars = [
        ("CONSENSUS_SNAPSHOT_DIR", dir.to_string_lossy().into_owned()),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", ANCHOR.to_string()),
        ("CONSENSUS_HYPHAE_BIN", binary.to_string()),
    ];
    SnapshotConfig::from_vars(|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    })
    .unwrap()
    .expect("a snapshot config")
}

/// Answers every `hyphae proof verify` with one scripted outcome and
/// records each argv.
struct ScriptedRunner {
    outcome: Result<CommandOutput, CommandError>,
    calls: Vec<Vec<String>>,
}

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, argv: &[String], _timeout: Duration) -> Result<CommandOutput, CommandError> {
        self.calls.push(argv.to_vec());
        self.outcome.clone()
    }
}

fn exited(code: i32, stdout: &str, stderr: &str) -> Result<CommandOutput, CommandError> {
    Ok(CommandOutput {
        code,
        stdout: stdout.into(),
        stderr: stderr.into(),
    })
}

fn verified_report() -> String {
    fixture("verify-report.json")
}

fn scripted(
    cfg: &SnapshotConfig,
    outcome: Result<CommandOutput, CommandError>,
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
    let mut verifier = scripted(&cfg, exited(0, &verified_report(), ""));
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
    let mut verifier = scripted(&cfg, exited(0, &verified_report(), ""));
    match SnapshotMemoryClient::open(&cfg, &mut verifier) {
        Ok(_) => panic!("open must fail"),
        Err(e) => e,
    }
}

fn flip_first_byte(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[0] ^= 0x01;
    std::fs::write(path, bytes).unwrap();
}

fn edit_manifest(dir: &Path, edit: impl FnOnce(&mut Value)) {
    let path = dir.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
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
            outcome: exited(0, "", ""),
            calls: vec![],
        },
    );
    assert_eq!(verifier.expected_anchor.as_deref(), Some(ANCHOR));
    assert_eq!(verifier.timeout, Duration::from_secs(7));
    assert_eq!(verifier.binary, "scripts/hyphae-docker.cmd");
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
    }
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
    let root = support::temp_dir("not-a-dir");
    assert_eq!(
        load_error(&root.join("absent")).code,
        codes::SNAPSHOT_DIR_MISSING
    );
}

#[test]
fn the_pinned_anchor_must_be_the_manifest_anchor() {
    let dir = build("pinned-anchor");
    let mut cfg = config(&dir, "hyphae");
    cfg.expected_anchor = "cd".repeat(32);
    let mut verifier = scripted(&cfg, exited(0, &verified_report(), ""));
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
    // Two candidates for the list.
    let mut twin = list_spec();
    twin.id = "list-demo-repo-again";
    let dir = build_with("two-lists", &[list_spec(), isolation_spec(), twin], |_| {});
    let err = load_error(&dir);
    assert_eq!(err.code, codes::CAPTURE_AMBIGUOUS, "{err}");
    assert!(err.message.contains("list-demo-repo-again"), "{err}");
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
    let report_with = |key: &str, value: Value| {
        let mut report: Value = serde_json::from_str(&verified_report()).unwrap();
        report[key] = value;
        report.to_string()
    };
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
        let mut verifier = scripted(&cfg, outcome);
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
}

#[test]
fn artifact_paths_are_mapped_to_the_verifier_directory_when_configured() {
    let dir = build("mapped");
    let mut cfg = config(&dir, "scripts/hyphae-docker.cmd");
    cfg.verifier_dir = Some("/snapshots/snap-2026-10-29-demo-repo/".into());
    let mut verifier = scripted(&cfg, exited(0, &verified_report(), ""));
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

/// A fake `hyphae` binary: appends its arguments to `log` and prints the
/// fixture verify report. Windows: a `.cmd` script (`std::process::Command`
/// runs it through `cmd.exe`); elsewhere a `sh` script.
fn fake_hyphae(root: &Path) -> (String, PathBuf) {
    let log = root.join("hyphae-calls.log");
    let report = root.join("verify-report.json");
    std::fs::write(&report, verified_report()).unwrap();
    let (name, script) = if cfg!(windows) {
        (
            "hyphae.cmd",
            format!(
                "@echo off\r\n>>\"{}\" echo %*\r\ntype \"{}\"\r\n",
                log.display(),
                report.display()
            ),
        )
    } else {
        (
            "hyphae",
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\ncat \"{}\"\n",
                log.display(),
                report.display()
            ),
        )
    };
    let path = root.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (path.to_string_lossy().into_owned(), log)
}

fn logged_calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
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
    let path = support::temp_dir("snapshot-runs").join(log_name);
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
