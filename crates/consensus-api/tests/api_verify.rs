// SPDX-License-Identifier: Apache-2.0
//! The read-only `verify` subcommand: a good log and its receipt verify;
//! a single changed byte, in the log or in the receipt, does not.

mod common;

use common::{research_dir, TempDir};
use consensus_api::api::types::GuidedScenario;
use consensus_api::guided;
use consensus_api::verify_cli;
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::scenarios::TS;
use consensus_runtime::{build_receipt_v2, EventLog, Orchestrator, RunLimits};
use std::path::{Path, PathBuf};

fn run_verify(args: &[&str]) -> (i32, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let code = verify_cli::run(&args, &mut out);
    (code, String::from_utf8(out).unwrap())
}

/// A finished guided run in `dir`, and its receipt.
fn fixture(dir: &Path) -> (PathBuf, PathBuf, Vec<u8>) {
    let log_path = dir.join("run.jsonl");
    let mut log = EventLog::open(&log_path).unwrap();
    let s = guided::build(GuidedScenario::Retry);
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    orch.run(&mut log, s.input).unwrap();
    let receipt = build_receipt_v2(log.all_events(), TS.into()).unwrap();
    let receipt_path = dir.join("receipt.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let bytes = std::fs::read(&log_path).unwrap();
    (log_path, receipt_path, bytes)
}

#[test]
fn a_good_log_and_its_receipt_verify() {
    let dir = TempDir::new("verify-good");
    let (log, receipt, _) = fixture(dir.path());
    let (code, out) = run_verify(&[log.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("VERIFIED: 1/1 run(s) verified"), "{out}");
    assert!(out.contains("recommendation=supported"), "{out}");
    assert!(out.contains("rule=supported.all_checks_passed"), "{out}");
    let (code, out) = run_verify(&[
        log.to_str().unwrap(),
        "--receipt",
        receipt.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("receipt matches"), "{out}");
}

#[test]
fn one_changed_byte_in_the_log_fails() {
    let dir = TempDir::new("verify-byte");
    let (log, receipt, bytes) = fixture(dir.path());
    // Flip one byte inside a record's content (not in a hash): the chain
    // no longer verifies.
    let text = String::from_utf8(bytes.clone()).unwrap();
    let target = text
        .find("Raise the payment client retry limit")
        .expect("content");
    let mut tampered = bytes.clone();
    tampered[target] = b'r';
    std::fs::write(&log, &tampered).unwrap();
    let (code, out) = run_verify(&[log.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL"), "{out}");
    assert!(
        out.contains("hash chain") || out.contains("corrupt"),
        "{out}"
    );

    // The original log with a receipt whose head hash was changed.
    std::fs::write(&log, &bytes).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&receipt).unwrap()).unwrap();
    let head = value["chain"]["head_hash"].as_str().unwrap().to_string();
    let mut chars: Vec<char> = head.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    value["chain"]["head_hash"] = serde_json::Value::String(chars.into_iter().collect());
    std::fs::write(&receipt, value.to_string()).unwrap();
    let (code, out) = run_verify(&[
        log.to_str().unwrap(),
        "--receipt",
        receipt.to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("head_hash"), "{out}");

    // A torn tail is reported, never repaired.
    let mut torn = bytes.clone();
    torn.truncate(torn.len() - 10);
    std::fs::write(&log, &torn).unwrap();
    let before = std::fs::read(&log).unwrap();
    let (code, out) = run_verify(&[log.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("incomplete final record"), "{out}");
    assert_eq!(std::fs::read(&log).unwrap(), before, "verify never writes");
}

#[test]
fn the_research_archive_verifies_run_by_run() {
    // On a copy, as every reader of research/ must work.
    let dir = TempDir::new("verify-archive");
    let src = research_dir().join("2026-10-27/live-battery-2026-10-27-final.jsonl");
    let copy = dir.path().join("final.jsonl");
    std::fs::copy(&src, &copy).unwrap();
    let (code, out) = run_verify(&[copy.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("VERIFIED: 6/6 run(s) verified"), "{out}");
    assert!(out.contains("chain=Log"), "{out}");
    // A single run of a multi-run log.
    let first_line = std::fs::read_to_string(&copy).unwrap();
    let first: ConsensusEvent = serde_json::from_str(first_line.lines().next().unwrap()).unwrap();
    let (code, out) = run_verify(&[copy.to_str().unwrap(), "--run", first.run_id.as_str()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1/1 run(s) verified"), "{out}");
    let (code, out) = run_verify(&[copy.to_str().unwrap(), "--run", "run-nope"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("not in the log"), "{out}");
}

#[test]
fn usage_errors_exit_2() {
    let (code, out) = run_verify(&[]);
    assert_eq!(code, 2);
    assert!(out.contains("usage"), "{out}");
    let (code, _) = run_verify(&["definitely/missing.jsonl"]);
    assert_eq!(code, 1);
}
