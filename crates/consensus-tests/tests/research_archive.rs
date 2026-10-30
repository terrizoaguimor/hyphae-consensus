// SPDX-License-Identifier: Apache-2.0
//! Checks on the research archive (`research/`): verbatim copies of the
//! recorded console and battery runs, pinned by `research/MANIFEST.json`
//! (see `research/README.md`). The archive is only ever read:
//! `EventLog::open` can repair or append, so it runs on temporary copies.

use consensus_domain::*;
use consensus_runtime::policy::{derive_recommendation_with, PolicyVersion};
use consensus_runtime::EventLog;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// `research/` at the repository root.
fn research_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the repository root")
        .join("research")
}

fn read(rel: &str) -> Vec<u8> {
    let path = research_dir().join(rel);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn manifest() -> Value {
    serde_json::from_slice(&read("MANIFEST.json")).expect("MANIFEST.json parses")
}

/// String field of a MANIFEST entry.
fn text(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} is not a string in {v}"))
        .to_string()
}

fn entries(m: &Value, key: &str) -> Vec<Value> {
    m[key].as_array().expect("MANIFEST array").clone()
}

/// MANIFEST file entries of one kind (`event_log` or `run_record`).
fn files_of_kind(m: &Value, kind: &str) -> Vec<Value> {
    entries(m, "files")
        .into_iter()
        .filter(|f| f["kind"] == kind)
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Serialized name of a unit enum value (e.g. `insufficient_evidence`).
fn serde_name(value: Value) -> String {
    value.as_str().expect("unit enum value").to_string()
}

/// A fresh, empty temporary directory, removed again on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-research-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Writes a copy of an archived file and returns the copy's path.
    fn copy(&self, rel: &str) -> PathBuf {
        let copy = self.0.join(rel.replace('/', "__"));
        std::fs::write(&copy, read(rel)).unwrap();
        copy
    }

    /// Opens a copy of an archived log, never the archive itself.
    fn open(&self, rel: &str) -> EventLog {
        let log = EventLog::open(self.copy(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(
            !log.recovered_truncated_tail,
            "{rel}: open recovered a truncated tail"
        );
        log
    }

    /// Opens a copy of every archived log, keyed by its MANIFEST path.
    fn open_all(&self, m: &Value) -> BTreeMap<String, EventLog> {
        files_of_kind(m, "event_log")
            .iter()
            .map(|f| {
                let rel = text(f, "path");
                let log = self.open(&rel);
                (rel, log)
            })
            .collect()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run ids in order of first appearance.
fn run_ids(log: &EventLog) -> Vec<RunId> {
    let mut ids: Vec<RunId> = Vec::new();
    for ev in log.all_events() {
        if !ids.contains(&ev.run_id) {
            ids.push(ev.run_id.clone());
        }
    }
    ids
}

/// Every file in research/ is pinned by MANIFEST.json, except these.
const UNPINNED: [&str; 3] = ["MANIFEST.json", "README.md", "SHA256SUMS"];

fn archive_files(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            archive_files(&entry.path(), &rel, out);
        } else {
            out.insert(rel);
        }
    }
}

#[test]
fn manifest_pins_every_archived_file() {
    let m = manifest();
    assert_eq!(m["schema"], "hyphae-consensus-research-manifest-v1");
    let mut pinned = BTreeMap::new();
    for f in entries(&m, "files") {
        let path = text(&f, "path");
        let bytes = read(&path);
        assert_eq!(
            Some(bytes.len() as u64),
            f["bytes"].as_u64(),
            "{path}: size"
        );
        let digest = sha256_hex(&bytes);
        assert_eq!(digest, text(&f, "sha256"), "{path}: sha256");
        assert!(
            pinned.insert(path.clone(), digest).is_none(),
            "{path} listed twice"
        );
    }
    // SHA256SUMS (`sha256sum -c` format) pins exactly the same digests.
    let sums = String::from_utf8(read("SHA256SUMS")).unwrap();
    let listed: BTreeMap<String, String> = sums
        .lines()
        .map(|line| {
            let (digest, path) = line.split_once("  ").expect("<sha256>  <path>");
            (path.to_string(), digest.to_string())
        })
        .collect();
    assert_eq!(listed, pinned, "SHA256SUMS and MANIFEST.json disagree");
    // Nothing sits in the archive unpinned.
    let mut on_disk = BTreeSet::new();
    archive_files(&research_dir(), "", &mut on_disk);
    let expected: BTreeSet<String> = pinned
        .keys()
        .cloned()
        .chain(UNPINNED.iter().map(|s| s.to_string()))
        .collect();
    assert_eq!(on_disk, expected, "files in research/ vs MANIFEST.json");
}

#[test]
fn event_logs_open_on_a_copy_without_repair() {
    let tmp = TempDir::new("open");
    let m = manifest();
    for f in files_of_kind(&m, "event_log") {
        let rel = text(&f, "path");
        let copy = tmp.copy(&rel);
        let log = EventLog::open(&copy).unwrap_or_else(|e| panic!("{rel}: {e}"));
        // Open validated sequences (gap-free from 1) and, for 1.1+
        // records, every hash-chain link; nothing needed repair.
        assert!(!log.recovered_truncated_tail, "{rel}: truncated tail");
        assert_eq!(Some(log.len() as u64), f["events"].as_u64(), "{rel}");
        assert_eq!(
            Some(run_ids(&log).len() as u64),
            f["runs"].as_u64(),
            "{rel}"
        );
        let versions: BTreeSet<String> = log
            .all_events()
            .iter()
            .map(|e| e.schema_version.clone())
            .collect();
        let recorded: BTreeSet<String> = f["schema_versions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(versions, recorded, "{rel}: schema versions");
        drop(log);
        assert!(
            std::fs::read(&copy).unwrap() == read(&rel),
            "{rel}: opening the copy changed its bytes"
        );
    }
}

#[test]
fn every_archived_run_replays() {
    let tmp = TempDir::new("replay");
    let m = manifest();
    let logs = tmp.open_all(&m);
    let mut total = 0;
    for (rel, log) in &logs {
        for run in run_ids(log) {
            let agg = log
                .rebuild(&run)
                .unwrap_or_else(|e| panic!("{rel} {run}: {e}"));
            assert!(agg.task.is_some(), "{rel} {run}: no task");
            assert_eq!(agg.event_count, log.events_for(&run).count() as u64);
            total += 1;
        }
    }
    assert_eq!(total, entries(&m, "runs").len());
}

/// Outcome as MANIFEST.json records it: the recommendation kind, or
/// `failed:<stage>` for a run that failed before one was issued.
fn replayed_outcome(log: &EventLog, run: &RunId) -> String {
    let agg = log.rebuild(run).unwrap();
    if agg.state == RunState::Failed {
        let stage = log
            .events_for(run)
            .find_map(|e| match &e.payload {
                EventPayload::RunFailed { stage, .. } => Some(stage.clone()),
                _ => None,
            })
            .expect("a failed run records its stage");
        return format!("failed:{stage}");
    }
    let kind = agg.recommendation.expect("recommendation issued").kind;
    serde_name(serde_json::to_value(kind).unwrap())
}

#[test]
fn manifest_runs_reproduce_their_recorded_outcomes() {
    let tmp = TempDir::new("outcomes");
    let m = manifest();
    let logs = tmp.open_all(&m);
    let mut listed = BTreeSet::new();
    let mut labels = BTreeSet::new();
    for r in entries(&m, "runs") {
        let label = text(&r, "label");
        assert!(labels.insert(label.clone()), "label {label} used twice");
        let file = text(&r, "file");
        let run = RunId::new(text(&r, "run_id"));
        let log = &logs[&file];
        let agg = log.rebuild(&run).unwrap();
        assert_eq!(replayed_outcome(log, &run), text(&r, "outcome"), "{label}");
        assert_eq!(
            serde_name(serde_json::to_value(agg.authorization).unwrap()),
            text(&r, "authorization"),
            "{label}"
        );
        let task = agg.task.as_ref().unwrap();
        assert_eq!(task.title, text(&r, "task_title"), "{label}");
        assert_eq!(task.project_id.as_str(), text(&r, "project"), "{label}");
        let schema = text(&r, "schema_version");
        assert!(
            log.events_for(&run).all(|e| e.schema_version == schema),
            "{label}: one schema version per run"
        );
        // Re-derive the recommendation with the rules of the run's own
        // schema version (rebuild() also rejects a mismatch for 1.1+).
        if let (Some(rec), Some(eval)) = (&agg.recommendation, &agg.last_evaluation) {
            match PolicyVersion::for_schema(&schema) {
                Some(policy) => assert_eq!(
                    derive_recommendation_with(policy, eval, &agg.round_verifications),
                    rec.kind,
                    "{label}"
                ),
                // Pre-1.1 records predate recomputation: never re-derived.
                None => assert_eq!(schema, "1.0", "{label}"),
            }
        }
        listed.insert((file, run));
    }
    // Every run of every archived log is listed exactly once.
    let mut in_logs = BTreeSet::new();
    for (rel, log) in &logs {
        for run in run_ids(log) {
            in_logs.insert((rel.clone(), run));
        }
    }
    assert_eq!(listed, in_logs);
}

#[test]
fn run_records_match_their_event_logs() {
    let tmp = TempDir::new("records");
    let m = manifest();
    let logs = tmp.open_all(&m);
    let mut records = 0;
    for r in entries(&m, "runs") {
        let Some(record) = r["record"].as_str() else {
            continue;
        };
        let label = text(&r, "label");
        let run = RunId::new(text(&r, "run_id"));
        let log = &logs[&text(&r, "file")];
        let rec: Value = serde_json::from_slice(&read(record)).unwrap();
        let events: Vec<Value> = log
            .events_for(&run)
            .map(|e| serde_json::to_value(e).unwrap())
            .collect();
        let receipt = &rec["receipt"];
        // Records up to 2026-10-27 hold the console's detail (the stored
        // events verbatim) and the first receipt format; those of the
        // operator server's battery of 2026-10-30 hold the API v1 detail
        // (each event as the view shows it) and receipt v2.
        let v2 = receipt["format"] == "hyphae-consensus-run-receipt-v2";
        if v2 {
            // Whole events, once the view's display fields and the
            // stored-only fields are dropped.
            let shown: Vec<Value> = rec["detail"]["events"]
                .as_array()
                .expect("detail events")
                .iter()
                .map(|e| shown_event(e, &label))
                .collect();
            let stored: Vec<Value> = events.iter().map(stored_event).collect();
            assert_eq!(shown, stored, "{label}");
        } else {
            // The console's run detail carried the run's chained events.
            assert_eq!(rec["detail"]["events"].as_array(), Some(&events), "{label}");
        }
        assert_eq!(receipt["run_id"], run.as_str(), "{label}");
        assert_eq!(
            receipt["event_count"].as_u64(),
            Some(events.len() as u64),
            "{label}"
        );
        // The receipt's head hash is the hash of the run's last event.
        let head_hash = if v2 {
            &receipt["chain"]["head_hash"]
        } else {
            &receipt["head_hash"]
        };
        assert_eq!(
            head_hash.as_str(),
            log.events_for(&run).last().map(|e| e.hash.as_str()),
            "{label}"
        );
        let outcome = text(&r, "outcome");
        let kind = receipt["recommendation"]["kind"].as_str();
        if outcome.starts_with("failed:") {
            assert_eq!(kind, None, "{label}");
        } else {
            assert_eq!(kind, Some(outcome.as_str()), "{label}");
        }
        records += 1;
    }
    assert_eq!(records, files_of_kind(&m, "run_record").len());
}

/// Fields the API v1 view adds to a stored event's payload object (display
/// only, each `false` in the archive).
const VIEW_FLAGS: [(&str, &str); 3] = [
    ("invocation", "scripted"),
    ("evidence", "proof_error_reported"),
    ("verification", "simulated"),
];

/// Fields of a stored event's payload object that the API v1 view leaves out.
const STORED_ONLY: [(&str, &str); 3] = [
    ("invocation", "safety_identifier_sent"),
    ("evidence", "schema_version"),
    ("verification", "schema_version"),
];

/// An API v1 event without its display fields, each checked before it is
/// dropped: `message` only on `run_failed` and a string, the flags `false`.
fn shown_event(event: &Value, label: &str) -> Value {
    let mut e = event.clone();
    let p = e["payload"].as_object_mut().expect("payload");
    let is_failure = p.get("type").and_then(Value::as_str) == Some("run_failed");
    if let Some(message) = p.remove("message") {
        assert!(is_failure && message.is_string(), "{label}: message");
    }
    for (object, field) in VIEW_FLAGS {
        if let Some(o) = p.get_mut(object).and_then(Value::as_object_mut) {
            if let Some(flag) = o.remove(field) {
                assert_eq!(flag, Value::Bool(false), "{label}: {object}.{field}");
            }
        }
    }
    e
}

/// A stored event without the fields the API v1 view leaves out.
fn stored_event(event: &Value) -> Value {
    let mut e = event.clone();
    let p = e["payload"].as_object_mut().expect("payload");
    for (object, field) in STORED_ONLY {
        if let Some(o) = p.get_mut(object).and_then(Value::as_object_mut) {
            o.remove(field);
        }
    }
    e
}

/// The lines of an archived log, read as plain text.
fn raw_lines(rel: &str) -> Vec<String> {
    let content = String::from_utf8(read(rel)).expect("utf-8");
    let body = content.strip_suffix('\n').expect("log ends with a newline");
    body.split('\n').map(str::to_string).collect()
}

/// The schema 1.0 console log: no chain links at all.
fn is_legacy_log(f: &Value) -> bool {
    f["schema_versions"] == serde_json::json!(["1.0"])
}

/// The chain checks out from the raw bytes alone, without this crate's
/// types: `hash` is the SHA-256 of `prev_hash` followed by the line with
/// its own hash value blanked. This is the recipe research/README.md gives
/// third parties; it agrees with `EventLog::open` because chained lines
/// round-trip byte for byte.
#[test]
fn chain_links_verify_from_the_raw_lines() {
    let m = manifest();
    for f in files_of_kind(&m, "event_log") {
        if is_legacy_log(&f) {
            continue;
        }
        let rel = text(&f, "path");
        let mut prev = "genesis".to_string();
        for (idx, line) in raw_lines(&rel).iter().enumerate() {
            let at = format!("{rel}:{}", idx + 1);
            let record: Value = serde_json::from_str(line).unwrap();
            assert_eq!(record["sequence"].as_u64(), Some(idx as u64 + 1), "{at}");
            assert_eq!(text(&record, "prev_hash"), prev, "{at}");
            let hash = text(&record, "hash");
            let field = format!(r#""hash":"{hash}""#);
            assert_eq!(line.matches(&field).count(), 1, "{at}");
            let blanked = line.replacen(&field, r#""hash":"""#, 1);
            assert_eq!(
                sha256_hex(format!("{prev}{blanked}").as_bytes()),
                hash,
                "{at}"
            );
            prev = hash;
        }
    }
}

/// Schema 1.0 records predate the hash chain (no `prev_hash`/`hash`),
/// per-item challenge ids, and the `unresolved_items`/`addressed_items`
/// field names (still read through serde aliases). Today's types write
/// them back with `"prev_hash":"","hash":""`, `"id":""` on every
/// challenge item, and the new names. Undoing exactly those changes must
/// give back the stored line.
fn undo_known_1_0_differences(reserialized: &str) -> String {
    reserialized
        .replacen(r#""prev_hash":"","hash":"","#, "", 1)
        .replace(r#"{"id":"","kind":"#, r#"{"kind":"#)
        .replace(r#""unresolved_items":"#, r#""unresolved_challenges":"#)
        .replace(r#""addressed_items":"#, r#""addressed_challenges":"#)
}

#[test]
fn every_log_line_round_trips_byte_for_byte() {
    let m = manifest();
    for f in files_of_kind(&m, "event_log") {
        let rel = text(&f, "path");
        for (idx, line) in raw_lines(&rel).iter().enumerate() {
            let at = format!("{rel}:{}", idx + 1);
            let event: ConsensusEvent = serde_json::from_str(line).unwrap();
            let again = serde_json::to_string(&event).unwrap();
            if !is_legacy_log(&f) {
                // The LC-01 baseline: every chained (1.1+) line is
                // reproduced byte for byte. Schema 1.3 must keep this.
                assert!(!event.hash.is_empty(), "{at}: unchained record");
                assert!(again == *line, "{at}: line does not round-trip");
                continue;
            }
            // Current behavior, asserted so a change is noticed: the
            // unchained schema 1.0 lines (all of console-events.jsonl) do
            // NOT round-trip, for the known reasons above and no other.
            // Tighten this to `again == line` if 1.0 records are ever
            // written back unchanged.
            assert_eq!(event.schema_version, "1.0", "{at}");
            assert!(event.hash.is_empty() && event.prev_hash.is_empty(), "{at}");
            assert!(again != *line, "{at}: now round-trips; tighten this test");
            assert!(
                undo_known_1_0_differences(&again) == *line,
                "{at}: unexplained re-serialization difference"
            );
        }
    }
}
