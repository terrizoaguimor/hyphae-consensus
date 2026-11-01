// SPDX-License-Identifier: Apache-2.0
//! `consensus-api baseline`: the operator's single-agent baseline
//! ([`consensus_runtime::baseline`]). Not part of any server or API: no
//! route, no public surface, no change to the `/api/v1` contract.
//!
//! ```text
//! consensus-api baseline run --title <t> --description <d> --out-dir <dir>
//!     [--project <p>] [--memory hyphae|fake] [--by <operator>]
//!     [--queries-from <consensus-log.jsonl> [--run <run-id>]]
//! consensus-api baseline verify <baseline-log.jsonl>
//! ```
//!
//! `run` makes ONE Meta call (the operator's Meta configuration, the same
//! variables `serve-operator` reads; refused in public mode) over evidence
//! recalled and verified by the runtime: with `--memory hyphae` (the
//! default) the Hyphae configuration `serve-operator` reads, the pinned
//! anchor and the verifier deadline included; with `--memory fake` the
//! scripted reference records with SIMULATED proofs, for smoke tests only.
//! The run is not metered by the live ledger (like an operator run without
//! one): the per-run token cap `CONSENSUS_MAX_RUN_TOKENS` still applies.
//!
//! The recall queries are the task's title and description, or, with
//! `--queries-from`, every claim statement of a recorded consensus run of
//! the same task (same project, title and description, checked), so the
//! baseline recalls what that run's final round held in context. The
//! consensus log is read from its bytes and verified first (never opened
//! writable); its SHA-256 and the run id are recorded in the plan.
//!
//! The log is written to `<dir>/<run_id>.jsonl`, a new file; a directory
//! inside `research/` or `demo-run/` is refused (they hold the research
//! evidence). The run's summary
//! ([`baseline_summary`](consensus_runtime::baseline::baseline_summary)) is
//! printed as JSON on stdout. Exit 0 once the run is recorded (completed or
//! failed, the record says which), 1 when it could not be run or recorded,
//! 2 on a usage error.
//!
//! `verify` reads a baseline log from its bytes, checks its chain and
//! replays it ([`read_baseline`]), and prints the same summary; exit 0 only
//! when it verifies.

use crate::config::{parse_max_run_tokens, HyphaeConfig, MetaConfig};
use crate::meta::MetaModelClient;
use consensus_domain::*;
use consensus_runtime::baseline::{
    baseline_summary, consensus_queries, read_baseline, task_text_queries, BaselineLog,
    BaselineModel, BaselineRunner, QuerySource,
};
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::fakes::FakeProofVerifier;
use consensus_runtime::ports::{MemoryClient, ProofVerifier};
use consensus_runtime::scenarios::retry_constraint_scenario;
use consensus_runtime::{RunControl, SystemClock, TaskInput, UuidIdGen};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub const USAGE: &str =
    "usage: consensus-api baseline run --title <title> --description <text> --out-dir <dir>
           [--project <project>] [--memory hyphae|fake] [--by <operator>]
           [--queries-from <consensus-log.jsonl> [--run <run-id>]]
       consensus-api baseline verify <baseline-log.jsonl>";

/// Directory names that hold the research evidence: never written below.
const PROTECTED_DIRS: [&str; 2] = ["demo-run", "research"];

/// Where the evidence comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    /// The operator's Hyphae deployment, proofs verified with
    /// `hyphae proof verify`.
    Hyphae,
    /// The scripted reference records, SIMULATED proofs (smoke tests).
    Fake,
}

/// `baseline run`'s parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    pub project: String,
    pub title: String,
    pub description: String,
    pub out_dir: PathBuf,
    pub memory: Memory,
    pub by: String,
    pub queries_from: Option<PathBuf>,
    pub run: Option<String>,
}

impl RunArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut project = None;
        let mut title = None;
        let mut description = None;
        let mut out_dir = None;
        let mut memory = Memory::Hyphae;
        let mut by = None;
        let mut queries_from = None;
        let mut run = None;
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            let value = || {
                args.get(i + 1)
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag {
                "--project" => project = Some(value()?),
                "--title" => title = Some(value()?),
                "--description" => description = Some(value()?),
                "--out-dir" => out_dir = Some(PathBuf::from(value()?)),
                "--by" => by = Some(value()?),
                "--queries-from" => queries_from = Some(PathBuf::from(value()?)),
                "--run" => run = Some(value()?),
                "--memory" => {
                    memory = match value()?.as_str() {
                        "hyphae" => Memory::Hyphae,
                        "fake" => Memory::Fake,
                        other => {
                            return Err(format!("--memory must be hyphae or fake, not {other:?}"))
                        }
                    }
                }
                other => return Err(format!("unknown argument {other:?}")),
            }
            i += 2;
        }
        if run.is_some() && queries_from.is_none() {
            return Err("--run names a run of the --queries-from log".into());
        }
        let nonempty = |v: Option<String>, name: &str| {
            v.filter(|s| !s.trim().is_empty())
                .ok_or_else(|| format!("{name} is required"))
        };
        Ok(Self {
            project: project.unwrap_or_else(|| "demo-repo".into()),
            title: nonempty(title, "--title")?,
            description: nonempty(description, "--description")?,
            out_dir: out_dir.ok_or("--out-dir is required")?,
            memory,
            by: by.unwrap_or_else(|| "operator".into()),
            queries_from,
            run,
        })
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `dir` when it lies outside the research directories.
fn safe_out_dir(dir: &Path) -> Result<(), String> {
    let inside = dir.components().any(|c| match c {
        Component::Normal(name) => PROTECTED_DIRS
            .iter()
            .any(|p| name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(p))),
        _ => false,
    });
    if inside {
        return Err(format!(
            "--out-dir may not lie inside {} (they hold the research evidence); archive a finished run by copying it",
            PROTECTED_DIRS.join(" or ")
        ));
    }
    Ok(())
}

/// The recall plan: the task text, or a recorded consensus run's claim
/// statements (the run must be of the same task).
fn plan(args: &RunArgs) -> Result<(QuerySource, Vec<String>), String> {
    let Some(path) = &args.queries_from else {
        return Ok((
            QuerySource::TaskText,
            task_text_queries(&args.title, &args.description),
        ));
    };
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let events = verify_bytes(&bytes)
        .map_err(|e| format!("{}: not a verified consensus log: {e}", path.display()))?;
    let mut runs: Vec<&RunId> = Vec::new();
    for e in &events {
        if !runs.contains(&&e.run_id) {
            runs.push(&e.run_id);
        }
    }
    let run_id = match &args.run {
        Some(id) => RunId::new(id),
        None if runs.len() == 1 => runs[0].clone(),
        None => {
            return Err(format!(
                "{} holds {} runs: name one with --run",
                path.display(),
                runs.len()
            ))
        }
    };
    let task = events
        .iter()
        .filter(|e| e.run_id == run_id)
        .find_map(|e| match &e.payload {
            EventPayload::TaskCreated { task } => Some(task),
            _ => None,
        })
        .ok_or_else(|| format!("{}: run {run_id} has no task", path.display()))?;
    if task.project_id.as_str() != args.project
        || task.title != args.title
        || task.description != args.description
    {
        return Err(format!(
            "run {run_id} is of another task (project, title or description differ): the baseline must run the consensus run's own task"
        ));
    }
    let queries = consensus_queries(&events, &run_id)
        .ok_or_else(|| format!("run {run_id} recorded no proposal, so no queries"))?;
    Ok((
        QuerySource::ConsensusLog {
            log_sha256: sha256_hex(&bytes),
            run_id,
        },
        queries,
    ))
}

/// `baseline` with the process environment.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run_with(
        args,
        &|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()),
        out,
        err,
    )
}

/// `baseline` with the variables `env` gives (tests pass their own).
pub fn run_with(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match args.first().map(String::as_str) {
        Some("run") => match RunArgs::parse(&args[1..]) {
            Ok(parsed) => match run_baseline(&parsed, env) {
                Ok(summary) => {
                    let _ = writeln!(out, "{summary:#}");
                    0
                }
                Err(e) => {
                    let _ = writeln!(err, "baseline run: {e}");
                    1
                }
            },
            Err(e) => {
                let _ = writeln!(err, "{e}\n{USAGE}");
                2
            }
        },
        Some("verify") if args.len() == 2 => {
            let path = Path::new(&args[1]);
            match std::fs::read(path) {
                Ok(bytes) => match read_baseline(&bytes) {
                    Ok(run) => {
                        let summary = baseline_summary(&run, &sha256_hex(&bytes));
                        let _ = writeln!(out, "{summary:#}");
                        0
                    }
                    Err(e) => {
                        let _ = writeln!(err, "baseline verify: {}: {e}", path.display());
                        1
                    }
                },
                Err(e) => {
                    let _ = writeln!(err, "baseline verify: {}: {e}", path.display());
                    1
                }
            }
        }
        _ => {
            let _ = writeln!(err, "{USAGE}");
            2
        }
    }
}

fn run_baseline(
    args: &RunArgs,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<serde_json::Value, String> {
    safe_out_dir(&args.out_dir)?;
    let (source, queries) = plan(args)?;
    let meta = MetaConfig::from_vars(env)
        .map_err(|e| format!("model=meta: {e}"))?
        .ok_or("the baseline requires META_API_KEY (or MODEL_API_KEY, or a key file) in the environment")?;
    if meta.public_mode {
        return Err(
            "the baseline is operator-only: refused in public mode (CONSENSUS_PUBLIC_MODE)".into(),
        );
    }
    let limits = parse_max_run_tokens(env("CONSENSUS_MAX_RUN_TOKENS").as_deref())?;
    let model = MetaModelClient::new(meta).map_err(|e| format!("model client: {e}"))?;
    let evidence = match args.memory {
        Memory::Hyphae => EvidenceMode::LiveHyphae,
        Memory::Fake => EvidenceMode::Simulated,
    };
    let input = TaskInput {
        project_id: ProjectId::new(&args.project),
        title: args.title.clone(),
        description: args.description.clone(),
        submitted_by: args.by.clone(),
        mode: Some(TaskMode {
            model: ModelMode::Meta,
            evidence,
            origin: RunOrigin::Cli,
            preset: None,
        }),
    };
    match args.memory {
        Memory::Hyphae => {
            let hcfg = HyphaeConfig::from_vars(env)
                .map_err(|e| format!("memory=hyphae: {e}"))?
                .ok_or("memory=hyphae requires HYPHAE_NATIVE_API_KEY_FILE and HYPHAE_NATIVE_ENDPOINT or HYPHAE_BASE_URL")?;
            let memory = crate::hyphae::connect_from_config(&hcfg)
                .map_err(|e| format!("the memory backend could not be reached: {e}"))?;
            let verifier = crate::hyphae::HyphaeProofVerifier::from_config(
                &hcfg,
                crate::hyphae::SystemCommandRunner,
            );
            execute(
                model, memory, verifier, limits, args, input, source, queries,
            )
        }
        Memory::Fake => execute(
            model,
            retry_constraint_scenario().memory,
            FakeProofVerifier::all(IntegrityStatus::Valid),
            limits,
            args,
            input,
            source,
            queries,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute<M: BaselineModel, Mem: MemoryClient, V: ProofVerifier>(
    model: M,
    memory: Mem,
    verifier: V,
    limits: consensus_runtime::RunLimits,
    args: &RunArgs,
    input: TaskInput,
    source: QuerySource,
    queries: Vec<String>,
) -> Result<serde_json::Value, String> {
    let mut runner = BaselineRunner {
        model,
        memory,
        verifier,
        clock: SystemClock,
        ids: UuidIdGen,
        limits,
    };
    let run_id = runner.new_run_id();
    let path = args.out_dir.join(format!("{run_id}.jsonl"));
    let mut log = BaselineLog::create(&path).map_err(|e| e.to_string())?;
    let run = runner
        .run(&mut log, run_id, input, source, queries, &RunControl::new())
        .map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut summary = baseline_summary(&run, &sha256_hex(&bytes));
    summary["log"] = serde_json::Value::String(path.display().to_string());
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn run_arguments_parse_strictly() {
        let parsed = RunArgs::parse(&args(&[
            "--title",
            "T",
            "--description",
            "D",
            "--out-dir",
            "out",
        ]))
        .unwrap();
        assert_eq!(parsed.project, "demo-repo");
        assert_eq!(parsed.memory, Memory::Hyphae);
        assert_eq!(parsed.by, "operator");
        for bad in [
            &["--title", "T", "--description", "D"][..],
            &["--title", "T", "--out-dir", "o"],
            &["--title", "", "--description", "D", "--out-dir", "o"],
            &[
                "--title",
                "T",
                "--description",
                "D",
                "--out-dir",
                "o",
                "--memory",
                "live",
            ],
            &[
                "--title",
                "T",
                "--description",
                "D",
                "--out-dir",
                "o",
                "--run",
                "r",
            ],
            &[
                "--title",
                "T",
                "--description",
                "D",
                "--out-dir",
                "o",
                "--extra",
                "x",
            ],
            &["--title"],
        ] {
            assert!(RunArgs::parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_research_directories_are_never_written() {
        assert!(safe_out_dir(Path::new("/tmp/baseline")).is_ok());
        assert!(safe_out_dir(Path::new("/tmp/research-notes")).is_ok());
        for dir in [
            "/repo/research/2026-11-01",
            "/repo/demo-run",
            "/repo/Research",
        ] {
            assert!(safe_out_dir(Path::new(dir)).is_err(), "{dir}");
        }
    }
}
