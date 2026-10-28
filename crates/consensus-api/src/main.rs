// SPDX-License-Identifier: Apache-2.0
//! Entry point.
//!
//!   cargo run -p consensus-api -- serve-public     # public API + static export (unix socket, or CONSENSUS_PUBLIC_BIND)
//!   cargo run -p consensus-api -- serve-operator   # operator console (loopback, bearer token); `serve` is an alias
//!   cargo run -p consensus-api -- verify <log.jsonl> [--run <id>] [--receipt <receipt.json>]
//!   cargo run -p consensus-api -- ledger status    # operator: the live-mode USD ledger (also `reset --by <who>`, `release <id> --by <who>`)
//!   cargo run -p consensus-api                     # scripted demo to AwaitingHumanApproval
//!   cargo run -p consensus-api -- --approve        # also record human approval
//!   cargo run -p consensus-api -- --reject         # also record human rejection
//!
//! Approval is RECORDED ONLY in every mode. No repository or external
//! mutation occurs (the executor is deferred through M3). The scripted
//! demo persists its events to `scripted-demo/events.jsonl` under the
//! current directory and replays the run it finds there instead of
//! re-running; it refuses to run inside `demo-run/` or `research/`, which
//! hold the research evidence (`EventLog::open` opens a log read-write).
//! Anything else on the command line prints the usage.

use consensus_domain::*;
use consensus_runtime::clock::{SequentialIdGen, SystemClock};
use consensus_runtime::orchestrator::{build_receipt, record_human_decision, Orchestrator};
use consensus_runtime::scenarios::retry_constraint_scenario;
use consensus_runtime::{EventLog, RunLimits};
use std::path::{Component, Path, PathBuf};

const USAGE: &str = "usage: consensus-api serve-public | serve-operator | serve
       consensus-api verify <log.jsonl> [--run <id>] [--receipt <receipt.json>]
       consensus-api ledger status | reset --by <operator> | release <reservation-id> --by <operator>
       consensus-api [--approve | --reject]    (the scripted demo)";

/// Where the scripted demo keeps its log, under the current directory.
const DEMO_LOG: &str = "scripted-demo/events.jsonl";

/// Directory names whose logs are research evidence: the scripted demo
/// never writes below them.
const PROTECTED_DIRS: [&str; 2] = ["demo-run", "research"];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("serve-public") => serve(consensus_api::server::serve_public()),
        Some("serve-operator") | Some("serve") => serve(consensus_api::server::serve_operator()),
        Some("verify") => {
            let code = consensus_api::verify_cli::run(&args[2..], &mut std::io::stdout());
            std::process::exit(code);
        }
        Some("ledger") => {
            let code = consensus_api::ledger_cli::run(
                &args[2..],
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            );
            std::process::exit(code);
        }
        Some("-h") | Some("--help") | Some("help") => println!("{USAGE}"),
        None => demo(),
        Some("--approve") | Some("--reject") if args.len() == 2 => demo(),
        Some(other) => {
            eprintln!("unknown command or option {other:?}\n{USAGE}");
            std::process::exit(2);
        }
    }
}

fn demo() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// The scripted demo's log under `cwd`, refused when `cwd` lies inside a
/// directory that holds research evidence.
fn demo_log_path(cwd: &Path) -> Result<PathBuf, String> {
    let inside = cwd.components().any(|c| match c {
        Component::Normal(name) => PROTECTED_DIRS
            .iter()
            .any(|p| name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(p))),
        _ => false,
    });
    if inside {
        return Err(format!(
            "the scripted demo does not run inside {} (they hold the research evidence); run it from another directory",
            PROTECTED_DIRS.join(" or ")
        ));
    }
    Ok(cwd.join(DEMO_LOG))
}

fn serve<F>(server: F)
where
    F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(e) = runtime.block_on(server) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let decision = if args.iter().any(|a| a == "--approve") {
        Some(true)
    } else if args.iter().any(|a| a == "--reject") {
        Some(false)
    } else {
        None
    };

    let path = demo_log_path(&std::env::current_dir()?)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut log = EventLog::open(&path)?;
    if log.recovered_truncated_tail {
        println!("[recovery] discarded a truncated tail record from a previous crash");
    }

    let run_id = match log.all_events().first() {
        Some(first) => {
            println!("[replay] existing log found; replaying without new model calls");
            first.run_id.clone()
        }
        None => {
            let mut s = retry_constraint_scenario();
            s.input.mode = Some(TaskMode {
                model: ModelMode::Scripted,
                evidence: EvidenceMode::Simulated,
                origin: RunOrigin::Cli,
                preset: None,
            });
            let clock = SystemClock;
            let now = consensus_runtime::Clock::now(&clock);
            let mut orch = Orchestrator {
                model: s.model,
                memory: s.memory,
                verifier: s.verifier,
                clock,
                ids: SequentialIdGen::new(),
                limits: RunLimits::default(),
            };
            let agg = orch.run(&mut log, s.input)?;
            println!("[run] completed at state {:?} (started {now})", agg.state);
            agg.run_id
        }
    };

    for ev in log.events_for(&run_id) {
        let kind = match &ev.payload {
            EventPayload::TaskCreated { .. } => "task_created",
            EventPayload::ProposalSubmitted { .. } => "proposal_submitted",
            EventPayload::EvidenceRetrieved { .. } => "evidence_retrieved",
            EventPayload::EvidenceUnavailable { .. } => "evidence_unavailable",
            EventPayload::RecallProven { .. } => "recall_proven",
            EventPayload::EvidenceVerified { .. } => "evidence_verified",
            EventPayload::ChallengeSubmitted { .. } => "challenge_submitted",
            EventPayload::EvaluationSubmitted { .. } => "evaluation_submitted",
            EventPayload::RevisionSubmitted { .. } => "revision_submitted",
            EventPayload::RecommendationIssued { .. } => "recommendation_issued",
            EventPayload::HumanDecisionRecorded { .. } => "human_decision_recorded",
            EventPayload::RunFailed { .. } => "run_failed",
        };
        println!("  #{:<3} {:?}  {}", ev.sequence, ev.actor, kind);
    }

    if let Some(approved) = decision {
        let clock = SystemClock;
        let now = consensus_runtime::Clock::now(&clock);
        let auth = HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: run_id.clone(),
            authorized_by: "mario".into(),
            approved,
            decided_at: now.clone(),
            idempotency_key: "demo-decision-1".into(),
            note: Some("M1 demo decision (recording only; no execution)".into()),
            authority: None,
        };
        let agg = record_human_decision(&mut log, auth, now)?;
        println!(
            "[decision] recorded: state={:?} authorization={:?}",
            agg.state, agg.authorization
        );
        println!("[decision] no external action was executed (executor deferred)");
    }

    let receipt = build_receipt(&log, &run_id, consensus_runtime::Clock::now(&SystemClock))?;
    println!("\n{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scripted_demo_never_writes_below_the_research_directories() {
        let ok = demo_log_path(Path::new("/tmp/scratch")).unwrap();
        assert!(ok.ends_with("scripted-demo/events.jsonl"), "{ok:?}");
        for cwd in [
            "/repo/demo-run",
            "/repo/demo-run/live-battery",
            "/repo/research/2026-10-27",
            "/repo/Research",
        ] {
            assert!(demo_log_path(Path::new(cwd)).is_err(), "{cwd}");
        }
        // A directory whose name only contains the word is not protected.
        assert!(demo_log_path(Path::new("/repo/research-notes")).is_ok());
    }
}
