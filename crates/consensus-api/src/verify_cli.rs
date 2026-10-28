// SPDX-License-Identifier: Apache-2.0
//! `consensus-api verify <log.jsonl> [--run <id>] [--receipt <receipt.json>]`
//!
//! Strictly read-only: the log is verified from its bytes
//! ([`verify_bytes`]: schema versions, gap-free sequences, unique ids, the
//! hash chain, no schema downgrade; a torn tail is an error, never
//! repaired). Every run (or the given one) is replayed through the state
//! machine, its recommendation re-derived and its receipt v2 built; with
//! `--receipt` the receipt's `run_id`, `event_count`, `head_hash` and
//! recommendation kind are checked against what the log yields. A report
//! is printed; the exit code is 0 only when everything verifies, so a
//! third party can check a downloaded log or receipt.

use consensus_domain::*;
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::policy::rationale_code;
use consensus_runtime::{build_receipt_v2, ReceiptError};
use std::io::Write;
use std::path::PathBuf;

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyArgs {
    pub log: PathBuf,
    pub run: Option<String>,
    pub receipt: Option<PathBuf>,
}

pub const USAGE: &str =
    "usage: consensus-api verify <log.jsonl> [--run <run-id>] [--receipt <receipt.json>]";

impl VerifyArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut log = None;
        let mut run = None;
        let mut receipt = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--run" => {
                    run = Some(args.get(i + 1).ok_or("--run needs a run id")?.clone());
                    i += 2;
                }
                "--receipt" => {
                    receipt = Some(PathBuf::from(
                        args.get(i + 1).ok_or("--receipt needs a file")?,
                    ));
                    i += 2;
                }
                other if other.starts_with("--") => return Err(format!("unknown option {other}")),
                other => {
                    if log.is_some() {
                        return Err("only one log file is accepted".into());
                    }
                    log = Some(PathBuf::from(other));
                    i += 1;
                }
            }
        }
        Ok(Self {
            log: log.ok_or(USAGE)?,
            run,
            receipt,
        })
    }
}

/// The outcome of verifying one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub run_id: RunId,
    pub events: u64,
    pub state: RunState,
    pub recommendation: Option<RecommendationKind>,
    pub rationale_code: Option<String>,
    pub head_hash: Option<String>,
    pub chain_scope: ChainScope,
    pub protocol: String,
    pub error: Option<String>,
}

/// The outcome of a `--receipt` check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptCheck {
    pub run_id: String,
    pub problems: Vec<String>,
}

/// Everything `verify` found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub runs: Vec<RunReport>,
    pub receipt: Option<ReceiptCheck>,
    pub fatal: Option<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.fatal.is_none()
            && !self.runs.is_empty()
            && self.runs.iter().all(|r| r.error.is_none())
            && self.receipt.as_ref().is_none_or(|r| r.problems.is_empty())
    }
}

/// A receipt as downloaded: the plain receipt v2, or an envelope holding
/// one under `receipt`.
fn parse_receipt(text: &str) -> Result<RunReceiptV2, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("receipt is not JSON: {e}"))?;
    let inner = value.get("receipt").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("receipt is not a receipt v2: {e}"))
}

fn verify_run(events: &[ConsensusEvent]) -> RunReport {
    let run_id = events[0].run_id.clone();
    match build_receipt_v2(events, "verify".into()) {
        Ok(receipt) => RunReport {
            run_id,
            events: receipt.event_count,
            state: receipt.final_state,
            recommendation: receipt.recommendation.as_ref().map(|r| r.kind),
            rationale_code: receipt
                .policy_trace
                .as_ref()
                .map(|t| rationale_code(t.rule).to_string()),
            head_hash: receipt.chain.head_hash.clone(),
            chain_scope: receipt.chain.scope,
            protocol: receipt.protocol.clone(),
            error: None,
        },
        Err(e) => RunReport {
            run_id,
            events: events.len() as u64,
            state: RunState::Failed,
            recommendation: None,
            rationale_code: None,
            head_hash: None,
            chain_scope: ChainScope::Unchained,
            protocol: String::new(),
            error: Some(match &e {
                ReceiptError::Replay(inner) => format!("replay refused the history: {inner}"),
                other => other.to_string(),
            }),
        },
    }
}

/// Verifies per the arguments.
pub fn verify(args: &VerifyArgs) -> Report {
    let mut report = Report::default();
    let bytes = match std::fs::read(&args.log) {
        Ok(bytes) => bytes,
        Err(e) => {
            report.fatal = Some(format!("{}: {e}", args.log.display()));
            return report;
        }
    };
    let events = match verify_bytes(&bytes) {
        Ok(events) => events,
        Err(e) => {
            report.fatal = Some(format!("{}: {e}", args.log.display()));
            return report;
        }
    };
    let mut order: Vec<RunId> = Vec::new();
    for ev in &events {
        if !order.contains(&ev.run_id) {
            order.push(ev.run_id.clone());
        }
    }
    if let Some(wanted) = &args.run {
        let wanted = RunId::new(wanted);
        if !order.contains(&wanted) {
            report.fatal = Some(format!("run {wanted} is not in the log"));
            return report;
        }
        order.retain(|r| *r == wanted);
    }
    if order.is_empty() {
        report.fatal = Some("the log holds no events".into());
        return report;
    }
    for run_id in &order {
        let run_events: Vec<ConsensusEvent> = events
            .iter()
            .filter(|e| e.run_id == *run_id)
            .cloned()
            .collect();
        report.runs.push(verify_run(&run_events));
    }
    if let Some(path) = &args.receipt {
        let check = match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| parse_receipt(&t))
        {
            Err(e) => ReceiptCheck {
                run_id: String::new(),
                problems: vec![format!("{}: {e}", path.display())],
            },
            Ok(receipt) => {
                let mut problems = Vec::new();
                match report.runs.iter().find(|r| r.run_id == receipt.run_id) {
                    None => problems.push(format!(
                        "receipt run {} is not among the verified runs",
                        receipt.run_id
                    )),
                    Some(run) => {
                        if let Some(e) = &run.error {
                            problems.push(format!("the run itself fails verification: {e}"));
                        }
                        if receipt.event_count != run.events {
                            problems.push(format!(
                                "event_count {} in the receipt, {} in the log",
                                receipt.event_count, run.events
                            ));
                        }
                        if receipt.chain.head_hash != run.head_hash {
                            problems.push(format!(
                                "head_hash {:?} in the receipt, {:?} in the log",
                                receipt.chain.head_hash, run.head_hash
                            ));
                        }
                        let receipt_kind = receipt.recommendation.as_ref().map(|r| r.kind);
                        if receipt_kind != run.recommendation {
                            problems.push(format!(
                                "recommendation {:?} in the receipt, {:?} re-derived from the log",
                                receipt_kind, run.recommendation
                            ));
                        }
                    }
                }
                ReceiptCheck {
                    run_id: receipt.run_id.to_string(),
                    problems,
                }
            }
        };
        report.receipt = Some(check);
    }
    report
}

/// Prints the report.
pub fn print(report: &Report, out: &mut dyn Write) -> std::io::Result<()> {
    if let Some(fatal) = &report.fatal {
        writeln!(out, "FAIL  {fatal}")?;
        return Ok(());
    }
    for run in &report.runs {
        match &run.error {
            None => writeln!(
                out,
                "ok    {}  events={} state={:?} recommendation={} rule={} chain={:?} head={} ({})",
                run.run_id,
                run.events,
                run.state,
                run.recommendation
                    .map(|k| format!("{k:?}").to_lowercase())
                    .unwrap_or_else(|| "none".into()),
                run.rationale_code.as_deref().unwrap_or("-"),
                run.chain_scope,
                run.head_hash.as_deref().unwrap_or("-"),
                run.protocol
            )?,
            Some(e) => writeln!(out, "FAIL  {}  {e}", run.run_id)?,
        }
    }
    if let Some(check) = &report.receipt {
        if check.problems.is_empty() {
            writeln!(out, "ok    receipt {} matches the log", check.run_id)?;
        } else {
            for p in &check.problems {
                writeln!(out, "FAIL  receipt {}: {p}", check.run_id)?;
            }
        }
    }
    let verified = report.runs.iter().filter(|r| r.error.is_none()).count();
    writeln!(
        out,
        "{}: {verified}/{} run(s) verified{}",
        if report.ok() {
            "VERIFIED"
        } else {
            "NOT VERIFIED"
        },
        report.runs.len(),
        match &report.receipt {
            Some(c) if c.problems.is_empty() => ", receipt matches",
            Some(_) => ", receipt does not match",
            None => "",
        }
    )?;
    Ok(())
}

/// The subcommand: prints a report, returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write) -> i32 {
    let parsed = match VerifyArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(e) => {
            let _ = writeln!(out, "{e}\n{USAGE}");
            return 2;
        }
    };
    let report = verify(&parsed);
    let _ = print(&report, out);
    if report.ok() {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse_and_refuse_unknown_options() {
        let args: Vec<String> = ["a.jsonl", "--run", "run-x", "--receipt", "r.json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let parsed = VerifyArgs::parse(&args).unwrap();
        assert_eq!(parsed.run.as_deref(), Some("run-x"));
        assert_eq!(parsed.receipt, Some(PathBuf::from("r.json")));
        assert!(VerifyArgs::parse(&["--nope".to_string()]).is_err());
        assert!(VerifyArgs::parse(&[]).is_err());
    }
}
