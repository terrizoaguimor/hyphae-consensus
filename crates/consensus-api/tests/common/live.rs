// SPDX-License-Identifier: Apache-2.0
//! Harness for the public live gate: the gate assembled as the server
//! assembles it, over a temporary ledger, a scripted Siteverify, a judge
//! digest file and a scripted evidence factory built on the runtime's
//! fakes (the production factory is the sealed snapshot, lane D2). The
//! Meta configuration points at the test's fake Chat Completions server.

use super::doubles::{FailingDeliberation, PanickingModel};
use super::{public_server, TestServer};
use chrono::Utc;
use consensus_api::config::MetaConfig;
use consensus_api::live::budget::{Ledger, LedgerOptions};
use consensus_api::live::config::{vars, Exposure, LiveConfig};
use consensus_api::live::gate::{
    DeliberationFactory, FactoryError, FactoryStatus, GateParts, LiveGate as BudgetedGate,
};
use consensus_api::live::judge::JudgeCodes;
use consensus_api::live::turnstile::{
    passing_answer, FakeSiteverify, Siteverify, SiteverifyResponse, TurnstileError,
    TurnstileVerifier,
};
use consensus_api::meta::MetaModelClient;
use consensus_api::runs::Deliberation;
use consensus_api::server::BuildOptions;
use consensus_domain::{EvidenceMode, IntegrityStatus};
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::fakes::FakeProofVerifier;
use consensus_runtime::scenarios::retry_constraint_scenario;
use consensus_runtime::{Orchestrator, RunLimits};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The Turnstile hostname and live action the harness verifies against.
pub const TURNSTILE_HOST: &str = "demo.example.org";
pub const TURNSTILE_ACTION: &str = "live-run";
/// A judge code whose digest the harness writes to the codes file.
pub const JUDGE_CODE: &str = "judge-alpha-7231";
/// The live output bound the harness configures: distinct from the Meta
/// default (16,000), so a request proves the live value was sent.
pub const LIVE_MAX_COMPLETION_TOKENS: u64 = 12_000;
/// The visitor's client address and country the harness sends by default.
pub const VISITOR_IP: &str = "203.0.113.7";

/// What the scripted factory builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryMode {
    /// The reference records with every proof Valid, over the metered
    /// Meta client the gate made.
    Scripted,
    /// A deliberation whose task creation fails: the start fails after
    /// admission.
    FailingStart,
    /// A deliberation whose first model call panics (the metered client
    /// is dropped unused).
    Panicking,
}

/// Test-only [`DeliberationFactory`] over `consensus_runtime::fakes`,
/// declaring simulated evidence.
pub struct ScriptedEvidenceFactory {
    pub mode: FactoryMode,
    pub ok: bool,
}

impl ScriptedEvidenceFactory {
    pub fn new(mode: FactoryMode) -> Arc<Self> {
        Arc::new(Self { mode, ok: true })
    }
}

impl DeliberationFactory for ScriptedEvidenceFactory {
    fn evidence_mode(&self) -> EvidenceMode {
        EvidenceMode::Simulated
    }

    fn build(
        &self,
        model: MetaModelClient,
        limits: RunLimits,
    ) -> Result<Box<dyn Deliberation>, FactoryError> {
        let s = retry_constraint_scenario();
        Ok(match self.mode {
            FactoryMode::Scripted => Box::new(Orchestrator {
                model,
                memory: s.memory,
                verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
                clock: SystemClock,
                ids: UuidIdGen,
                limits,
            }),
            FactoryMode::FailingStart => Box::new(FailingDeliberation),
            FactoryMode::Panicking => Box::new(Orchestrator {
                model: PanickingModel,
                memory: s.memory,
                verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
                clock: SystemClock,
                ids: UuidIdGen,
                limits,
            }),
        })
    }

    fn status(&self) -> FactoryStatus {
        FactoryStatus {
            ok: self.ok,
            kind: EvidenceMode::Simulated,
            detail: Some("scripted".into()),
        }
    }
}

/// What the scripted Siteverify answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteverifyMode {
    Pass,
    Fail,
    Outage,
}

/// A Siteverify over a [`FakeSiteverify`] whose answer the test switches.
pub fn scripted_siteverify(
    mode: Arc<Mutex<SiteverifyMode>>,
) -> (Arc<dyn Siteverify>, Arc<FakeSiteverify>) {
    let fake = Arc::new(FakeSiteverify::new(move |_, _| {
        match *mode.lock().unwrap() {
            SiteverifyMode::Pass => {
                Ok(passing_answer(TURNSTILE_HOST, TURNSTILE_ACTION, Utc::now()))
            }
            SiteverifyMode::Fail => Ok(SiteverifyResponse {
                success: false,
                error_codes: vec!["invalid-input-response".into()],
                ..SiteverifyResponse::default()
            }),
            SiteverifyMode::Outage => Err(TurnstileError::Outage("connection reset".into())),
        }
    }));
    let verifier = TurnstileVerifier::new(fake.clone(), TURNSTILE_HOST, TURNSTILE_ACTION);
    (Arc::new(verifier), fake)
}

/// The live configuration the harness loads: the owner's prices and caps
/// with pacing and slots out of the way (ten ledger slots, a hundred
/// admissions an hour, the whole day available at once), a 12k output
/// bound, the ledger and judge codes under `dir`. Live mode itself stays
/// `false` here: the gate needs no Turnstile secret when the test gives
/// it a verifier, and the server's enabling path is tested separately.
pub fn live_config(dir: &Path, overrides: &[(&'static str, &str)]) -> LiveConfig {
    let ledger_dir = dir.join("ledger");
    let codes = dir.join("judge-codes.txt");
    std::fs::write(
        &codes,
        format!("# judges\n{}\n", JudgeCodes::digest_hex(JUDGE_CODE)),
    )
    .unwrap();
    let mut env: BTreeMap<&'static str, String> = BTreeMap::new();
    env.insert(vars::LEDGER_DIR, ledger_dir.display().to_string());
    env.insert(vars::JUDGE_CODES_FILE, codes.display().to_string());
    env.insert(vars::MAX_CONCURRENT, "10".into());
    env.insert(vars::HOURLY_ADMISSIONS, "100".into());
    env.insert(vars::BURST_FRACTION, "1".into());
    env.insert(
        vars::MAX_COMPLETION_TOKENS,
        LIVE_MAX_COMPLETION_TOKENS.to_string(),
    );
    for (k, v) in overrides {
        env.insert(k, v.to_string());
    }
    LiveConfig::load(Exposure::Local, &env).expect("live config")
}

/// How to build a live server under test.
pub struct LiveOptions {
    pub factory: FactoryMode,
    /// Overrides of the live configuration variables.
    pub live_env: Vec<(&'static str, String)>,
    /// Overrides of the public server's variables.
    pub server_env: Vec<(&'static str, String)>,
    /// Whether the listener trusts `CF-*` headers (the harness then sends
    /// `CF-Connecting-IP` on session requests).
    pub trust_proxy: bool,
}

impl Default for LiveOptions {
    fn default() -> Self {
        Self {
            factory: FactoryMode::Scripted,
            live_env: Vec::new(),
            server_env: Vec::new(),
            trust_proxy: true,
        }
    }
}

/// A public server with the budgeted live gate wired in, and handles on
/// everything the tests inspect.
pub struct LiveHarness {
    pub server: TestServer,
    pub gate: Arc<BudgetedGate>,
    pub ledger: Ledger,
    pub ledger_dir: PathBuf,
    pub siteverify_mode: Arc<Mutex<SiteverifyMode>>,
    pub siteverify: Arc<FakeSiteverify>,
    pub factory: Arc<ScriptedEvidenceFactory>,
}

/// Builds the live server. `meta` is the configuration of the fake Chat
/// Completions server (see `tests/support`); the gate forces nothing on
/// it but the per-run pseudonym and the live output bound.
pub fn live_server(opts: LiveOptions, mut meta: MetaConfig) -> LiveHarness {
    // The ledger and the codes file live next to the store, so they go
    // with the server's temporary directory.
    let scratch = super::TempDir::new("live-cfg");
    let live_env: Vec<(&'static str, &str)> = opts
        .live_env
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    let config = live_config(scratch.path(), &live_env);
    let ledger_dir = config.ledger_dir.clone().expect("ledger dir");
    let ledger = Ledger::open_with(
        &ledger_dir,
        config.budget.clone(),
        Arc::new(Utc::now),
        LedgerOptions {
            lock_wait: Duration::from_secs(5),
            lock_poll: Duration::from_millis(5),
        },
    )
    .expect("ledger opens");
    let siteverify_mode = Arc::new(Mutex::new(SiteverifyMode::Pass));
    let (verifier, siteverify) = scripted_siteverify(siteverify_mode.clone());
    let factory = ScriptedEvidenceFactory::new(opts.factory);
    meta.public_mode = true;
    let gate = Arc::new(BudgetedGate::new(GateParts {
        ledger: ledger.clone(),
        siteverify: verifier,
        geo: config.geo.clone(),
        judge_codes: config.judge_codes.clone(),
        meta,
        run_limits: RunLimits {
            max_run_tokens: config.budget.run_token_cap,
        },
        factory: factory.clone(),
        config: config.clone(),
    }));
    let mut env: Vec<(&str, &str)> = vec![("CONSENSUS_LIVE_CONCURRENCY", "1")];
    let trust = if opts.trust_proxy { "true" } else { "false" };
    env.push(("CONSENSUS_TRUST_PROXY_HEADERS", trust));
    env.extend(opts.server_env.iter().map(|(k, v)| (*k, v.as_str())));
    let mut server = public_server(
        &env,
        BuildOptions {
            live_gate: Some(gate.clone()),
            admission_wait: Duration::from_millis(300),
            geo: config.geo.clone(),
            ..BuildOptions::default()
        },
    );
    // Keep the scratch directory alive as long as the server.
    server.extra_dirs.push(scratch);
    LiveHarness {
        server,
        gate,
        ledger,
        ledger_dir,
        siteverify_mode,
        siteverify,
        factory,
    }
}

impl LiveHarness {
    pub fn set_siteverify(&self, mode: SiteverifyMode) {
        *self.siteverify_mode.lock().unwrap() = mode;
    }

    /// Records of one `type` across every day file of the ledger.
    pub fn ledger_records(&self, kind: &str) -> Vec<Value> {
        ledger_records(&self.ledger_dir, kind)
    }

    /// Every ledger file's text, concatenated (for canaries).
    pub fn ledger_text(&self) -> String {
        files_text(&self.ledger_dir)
    }

    /// Every stored run file's text, concatenated (for canaries).
    pub fn store_text(&self) -> String {
        files_text(&self.server.dir.path().join("store"))
    }

    /// Waits until the ledger holds `n` release records.
    pub async fn wait_releases(&self, n: usize) -> Vec<Value> {
        for _ in 0..300 {
            let releases = self.ledger_records("release");
            if releases.len() >= n {
                return releases;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "expected {n} release record(s), ledger has {:?}",
            self.ledger_records("release")
        );
    }

    /// The kill-switch file the ledger watches.
    pub fn kill_switch(&self) -> PathBuf {
        self.ledger_dir.join("kill-switch")
    }

    /// A session for the harness visitor (the listener trusts the proxy
    /// header, so the client address travels in `CF-Connecting-IP`).
    pub async fn session(&self) -> String {
        let (status, cookie) = self.server.new_session_from_header(VISITOR_IP).await;
        assert_eq!(status, axum::http::StatusCode::CREATED);
        cookie.expect("session cookie")
    }
}

/// Records of one `type` across every `*.jsonl` file in `dir`.
pub fn ledger_records(dir: &Path, kind: &str) -> Vec<Value> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
        .iter()
        .flat_map(|path| {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| serde_json::from_str::<Value>(l).expect("ledger record"))
                .collect::<Vec<_>>()
        })
        .filter(|v| v["type"] == kind)
        .collect()
}

fn files_text(dir: &Path) -> String {
    let mut out = String::new();
    fn walk(dir: &Path, out: &mut String) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    out.push_str(&String::from_utf8_lossy(&bytes));
                    out.push('\n');
                }
            }
        }
    }
    walk(dir, &mut out);
    out
}
