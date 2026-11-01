// SPDX-License-Identifier: Apache-2.0
//! Single-agent baseline runs in the research archive (`research/`): the
//! files `scripts/make_research_manifest.py` pins with kinds
//! `baseline_log` and `baseline_record`, and the runs it lists under
//! `baseline_runs` (see `docs/evaluation.md`, "Single-agent baseline").
//! None is archived yet; the same checks run here on a synthetic battery
//! recorded with the scripted doubles, laid out exactly as the driver and
//! the manifest script lay one out. Archived files are only ever read.

use consensus_domain::*;
use consensus_runtime::baseline::{
    baseline_summary, consensus_queries, read_baseline, BaselineLog, BaselineRunner, QuerySource,
    BASELINE_SCHEMA_VERSION, RUN_KIND,
};
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::orchestrator::Orchestrator;
use consensus_runtime::scenarios::{baseline_scenario, retry_constraint_scenario, TS};
use consensus_runtime::{EventLog, RunControl, RunLimits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `research/` at the repository root, or the directory
/// `CONSENSUS_RESEARCH_DIR` names: a copy with a new battery and the
/// manifest regenerated, to check before it is committed.
fn research_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CONSENSUS_RESEARCH_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the repository root")
        .join("research")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("{key} is not a string in {v}"))
}

fn array<'a>(m: &'a Value, key: &str) -> Vec<&'a Value> {
    m[key]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn files_of_kind<'a>(m: &'a Value, kind: &str) -> Vec<&'a Value> {
    array(m, "files")
        .into_iter()
        .filter(|f| f["kind"] == kind)
        .collect()
}

/// A pinned file's bytes, checked against its size and digest.
fn pinned(root: &Path, m: &Value, rel: &str) -> Result<Vec<u8>, String> {
    let entry = array(m, "files")
        .into_iter()
        .find(|f| f["path"] == rel)
        .ok_or_else(|| format!("{rel} is not pinned"))?;
    let bytes = std::fs::read(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    if entry["bytes"].as_u64() != Some(bytes.len() as u64)
        || text(entry, "sha256")? != sha256_hex(&bytes)
    {
        return Err(format!("{rel}: size or sha256 differs from the manifest"));
    }
    Ok(bytes)
}

macro_rules! ensure {
    ($cond:expr, $($msg:tt)*) => {
        if !$cond {
            return Err(format!($($msg)*));
        }
    };
}

/// Every check on an archive's baseline runs; the number of runs checked.
/// Each baseline log verifies and replays, is refused by the consensus
/// event log, and is listed by exactly one baseline run whose outcome,
/// task and run record (its summary is what the log yields) agree with it;
/// a paired consensus run is an archived run of the same task whose pinned
/// log is the one the baseline recalled with.
fn check_baseline_archive(root: &Path, m: &Value) -> Result<usize, String> {
    let logs = files_of_kind(m, "baseline_log");
    let records = files_of_kind(m, "baseline_record");
    let runs = array(m, "baseline_runs");
    let consensus_runs = array(m, "runs");
    for f in &logs {
        let rel = text(f, "path")?;
        let bytes = pinned(root, m, rel)?;
        let run = read_baseline(&bytes).map_err(|e| format!("{rel}: {e}"))?;
        ensure!(
            f["events"].as_u64() == Some(run.event_count),
            "{rel}: events"
        );
        ensure!(f["runs"].as_u64() == Some(1), "{rel}: one run per log");
        ensure!(
            f["schema_versions"] == json!([BASELINE_SCHEMA_VERSION]),
            "{rel}: schema versions"
        );
        ensure!(
            verify_bytes(&bytes).is_err(),
            "{rel}: the consensus event log must refuse a baseline log"
        );
    }
    let (mut used_logs, mut used_records, mut labels) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for r in &runs {
        let label = text(r, "label")?;
        ensure!(
            labels.insert(label.to_string()),
            "{label}: label used twice"
        );
        ensure!(
            !consensus_runs.iter().any(|c| c["label"] == label),
            "{label}: also a consensus run label"
        );
        ensure!(r["run_kind"] == RUN_KIND, "{label}: run_kind");
        let file = text(r, "file")?;
        ensure!(
            logs.iter().any(|f| f["path"] == file),
            "{label}: {file} is not a baseline log"
        );
        ensure!(
            used_logs.insert(file.to_string()),
            "{label}: log listed twice"
        );
        let bytes = pinned(root, m, file)?;
        let run = read_baseline(&bytes).map_err(|e| format!("{label}: {e}"))?;
        ensure!(run.run_id.as_str() == text(r, "run_id")?, "{label}: run id");
        ensure!(
            run.outcome_label() == text(r, "outcome")?,
            "{label}: outcome"
        );
        ensure!(
            run.task.title == text(r, "task_title")?,
            "{label}: task title"
        );
        ensure!(
            run.task.project_id.as_str() == text(r, "project")?,
            "{label}: project"
        );
        ensure!(
            file.ends_with(&format!("/runs/{}.jsonl", run.run_id)),
            "{label}: log path"
        );
        let record = text(r, "record")?;
        ensure!(
            records.iter().any(|f| f["path"] == record),
            "{label}: {record} is not a baseline record"
        );
        ensure!(
            used_records.insert(record.to_string()),
            "{label}: record listed twice"
        );
        let rec: Value = serde_json::from_slice(&pinned(root, m, record)?)
            .map_err(|e| format!("{record}: {e}"))?;
        ensure!(rec["name"] == label, "{label}: record name");
        ensure!(rec["exit_code"] == 0, "{label}: exit code");
        let mut summary = rec["summary"].clone();
        let log = summary
            .as_object_mut()
            .and_then(|s| s.remove("log"))
            .ok_or_else(|| format!("{label}: summary without log"))?;
        ensure!(
            log == format!("runs/{}.jsonl", run.run_id),
            "{label}: summary log path"
        );
        ensure!(
            summary == baseline_summary(&run, &sha256_hex(&bytes)),
            "{label}: the archived summary is not what the log yields"
        );
        match r["consensus_label"].as_str() {
            None => ensure!(
                rec["consensus"].is_null(),
                "{label}: a pairing in the record, none in the manifest"
            ),
            Some(paired) => {
                let c = consensus_runs
                    .iter()
                    .find(|c| c["label"] == paired)
                    .ok_or_else(|| format!("{label}: consensus run {paired} is not archived"))?;
                let digest = sha256_hex(&pinned(root, m, text(c, "file")?)?);
                let expected = QuerySource::ConsensusLog {
                    log_sha256: digest.clone(),
                    run_id: RunId::new(text(c, "run_id")?),
                };
                ensure!(
                    run.plan.source == expected,
                    "{label}: not recalled with {paired}'s log"
                );
                ensure!(
                    rec["consensus"]["sha256"] == digest.as_str()
                        && rec["consensus"]["run_id"] == c["run_id"]
                        && rec["consensus"]["label"] == paired,
                    "{label}: the record's pairing"
                );
                ensure!(
                    c["task_title"] == run.task.title.as_str()
                        && c["project"] == run.task.project_id.as_str(),
                    "{label}: {paired} is of another task"
                );
            }
        }
    }
    let all_logs: BTreeSet<String> = logs
        .iter()
        .map(|f| text(f, "path").map(str::to_string))
        .collect::<Result<_, _>>()?;
    let all_records: BTreeSet<String> = records
        .iter()
        .map(|f| text(f, "path").map(str::to_string))
        .collect::<Result<_, _>>()?;
    ensure!(used_logs == all_logs, "baseline logs without a run");
    ensure!(
        used_records == all_records,
        "baseline records without a run"
    );
    // A consensus run never points at a baseline file.
    for c in &consensus_runs {
        ensure!(
            !all_logs.contains(text(c, "file")?),
            "consensus run {} names a baseline log",
            c["label"]
        );
    }
    Ok(runs.len())
}

#[test]
fn archived_baseline_runs_verify() {
    let root = research_dir();
    let m: Value = serde_json::from_slice(&std::fs::read(root.join("MANIFEST.json")).unwrap())
        .expect("MANIFEST.json parses");
    let checked = check_baseline_archive(&root, &m).unwrap();
    // The key appears only once a baseline battery is archived.
    assert_eq!(checked == 0, m.get("baseline_runs").is_none());
}

/// A fresh, empty temporary directory, removed again on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-research-baseline-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn file_entry(root: &Path, rel: &str, kind: &str, events: Option<u64>) -> Value {
    let bytes = std::fs::read(root.join(rel)).unwrap();
    let mut entry = json!({
        "path": rel,
        "source": "synthetic",
        "bytes": bytes.len(),
        "sha256": sha256_hex(&bytes),
        "kind": kind,
        "schema_versions": [BASELINE_SCHEMA_VERSION],
        "runs": 1,
        "description": "synthetic",
    });
    if let Some(n) = events {
        entry["events"] = json!(n);
    }
    entry
}

/// A synthetic archive: one consensus run (scripted) and one baseline run
/// paired with it, laid out as the driver and the manifest script lay them
/// out. Returns the manifest.
fn synthetic_archive(root: &Path) -> Value {
    let day = "2026-11-01";
    // The consensus run, pinned like an archived store battery's.
    let consensus_rel = format!("{day}/consensus/runs/consensus.jsonl");
    std::fs::create_dir_all(root.join(format!("{day}/consensus/runs"))).unwrap();
    let s = retry_constraint_scenario();
    let mut log = EventLog::open(root.join(&consensus_rel)).unwrap();
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let consensus = orch.run(&mut log, s.input).unwrap();
    drop(log);
    let consensus_bytes = std::fs::read(root.join(&consensus_rel)).unwrap();
    let queries =
        consensus_queries(&verify_bytes(&consensus_bytes).unwrap(), &consensus.run_id).unwrap();

    // The baseline run, as `consensus-api baseline run` writes it.
    let base = format!("{day}/baseline-{day}");
    let b = baseline_scenario();
    let input = b.input.clone();
    let mut runner = BaselineRunner {
        model: b.model,
        memory: b.memory,
        verifier: b.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let run_id = runner.new_run_id();
    let log_rel = format!("{base}/runs/{run_id}.jsonl");
    let mut blog = BaselineLog::create(root.join(&log_rel)).unwrap();
    let source = QuerySource::ConsensusLog {
        log_sha256: sha256_hex(&consensus_bytes),
        run_id: consensus.run_id.clone(),
    };
    let run = runner
        .run(
            &mut blog,
            run_id,
            b.input,
            source,
            queries,
            &RunControl::new(),
        )
        .unwrap();
    let log_bytes = std::fs::read(root.join(&log_rel)).unwrap();
    let mut summary = baseline_summary(&run, &sha256_hex(&log_bytes));
    summary["log"] = json!(format!("runs/{}.jsonl", run.run_id));
    let record_rel = format!("{base}/B01.json");
    let record = json!({
        "name": "B01",
        "task_key": "R01-reference",
        "task": {"project": input.project_id, "title": input.title, "description": input.description},
        "consensus": {
            "label": "M91",
            "file": consensus_rel,
            "sha256": sha256_hex(&consensus_bytes),
            "run_id": consensus.run_id,
        },
        "exit_code": 0,
        "summary": summary,
        "wall_seconds": 1.0,
    });
    std::fs::write(
        root.join(&record_rel),
        serde_json::to_string_pretty(&record).unwrap() + "\n",
    )
    .unwrap();

    let mut consensus_file = file_entry(
        root,
        &consensus_rel,
        "event_log",
        Some(consensus.event_count),
    );
    consensus_file["schema_versions"] = json!([SCHEMA_VERSION]);
    json!({
        "schema": "hyphae-consensus-research-manifest-v1",
        "files": [
            consensus_file,
            file_entry(root, &log_rel, "baseline_log", Some(run.event_count)),
            file_entry(root, &record_rel, "baseline_record", None),
        ],
        "runs": [{
            "run_id": consensus.run_id,
            "label": "M91",
            "battery": "synthetic",
            "task_title": input.title,
            "project": input.project_id,
            "outcome": "supported",
            "authorization": "pending",
            "schema_version": SCHEMA_VERSION,
            "file": consensus_rel,
            "record": null,
        }],
        "baseline_runs": [{
            "run_id": run.run_id,
            "label": "B01",
            "battery": format!("baseline-{day}"),
            "run_kind": RUN_KIND,
            "task_title": input.title,
            "project": input.project_id,
            "outcome": run.outcome_label(),
            "consensus_label": "M91",
            "schema_version": BASELINE_SCHEMA_VERSION,
            "file": log_rel,
            "record": record_rel,
        }],
    })
}

#[test]
fn a_synthetic_baseline_battery_passes_the_archive_checks() {
    let tmp = TempDir::new("synthetic");
    let m = synthetic_archive(&tmp.0);
    assert_eq!(check_baseline_archive(&tmp.0, &m), Ok(1));

    // Each kind of disagreement is caught.
    let mut wrong_outcome = m.clone();
    wrong_outcome["baseline_runs"][0]["outcome"] = json!("contested");
    assert!(check_baseline_archive(&tmp.0, &wrong_outcome)
        .unwrap_err()
        .contains("outcome"));
    let mut wrong_pairing = m.clone();
    wrong_pairing["baseline_runs"][0]["consensus_label"] = json!("M92");
    assert!(check_baseline_archive(&tmp.0, &wrong_pairing)
        .unwrap_err()
        .contains("not archived"));
    let mut unlisted = m.clone();
    unlisted["baseline_runs"] = json!([]);
    assert!(check_baseline_archive(&tmp.0, &unlisted)
        .unwrap_err()
        .contains("without a run"));
    let mut as_consensus = m.clone();
    as_consensus["runs"][0]["file"] = m["baseline_runs"][0]["file"].clone();
    assert!(check_baseline_archive(&tmp.0, &as_consensus).is_err());

    // A record whose summary says something the log does not.
    let record_rel = m["baseline_runs"][0]["record"]
        .as_str()
        .unwrap()
        .to_string();
    let path = tmp.0.join(&record_rel);
    let original = std::fs::read(&path).unwrap();
    let mut rec: Value = serde_json::from_slice(&original).unwrap();
    rec["summary"]["agent_verdict"] = json!("contested");
    std::fs::write(&path, serde_json::to_string_pretty(&rec).unwrap() + "\n").unwrap();
    let mut repinned = m.clone();
    repinned["files"][2] = file_entry(&tmp.0, &record_rel, "baseline_record", None);
    assert!(check_baseline_archive(&tmp.0, &repinned)
        .unwrap_err()
        .contains("not what the log yields"));
    // And a log edited after pinning.
    std::fs::write(&path, &original).unwrap();
    let log_rel = m["baseline_runs"][0]["file"].as_str().unwrap().to_string();
    let log_path = tmp.0.join(&log_rel);
    let text = std::fs::read_to_string(&log_path).unwrap();
    std::fs::write(&log_path, text.replacen("10 attempts", "11 attempts", 1)).unwrap();
    assert!(check_baseline_archive(&tmp.0, &m)
        .unwrap_err()
        .contains("sha256"));
}
