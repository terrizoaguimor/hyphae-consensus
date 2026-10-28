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
//! demo persists events to `demo-run/events.jsonl`; delete that directory
//! for a fresh run — the binary replays it instead of re-running when a
//! completed run exists.

use consensus_domain::*;
use consensus_runtime::clock::{SequentialIdGen, SystemClock};
use consensus_runtime::orchestrator::{build_receipt, record_human_decision, Orchestrator};
use consensus_runtime::scenarios::retry_constraint_scenario;
use consensus_runtime::{EventLog, RunLimits};

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
        _ => {
            if let Err(e) = run() {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
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

    let mut log = EventLog::open("demo-run/events.jsonl")?;
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
