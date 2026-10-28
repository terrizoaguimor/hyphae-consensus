// SPDX-License-Identifier: Apache-2.0
//! Harness for the public live gate: the gate assembled as the server
//! assembles it, over a temporary ledger, a scripted Siteverify, a judge
//! digest file and an evidence factory: a scripted one built on the
//! runtime's fakes, or the production [`SnapshotFactory`] over a fixture
//! snapshot and the fake `hyphae` binary ([`FactoryMode::Snapshot`]), or
//! whatever `serve-public`'s own entry point makes of a set of snapshot
//! variables ([`FactoryMode::SnapshotFromVars`]: the booted factory, or the
//! unavailable evidence of a snapshot that does not boot). The Meta
//! configuration points at the test's fake Chat Completions server.

use super::doubles::{FailingDeliberation, PanickingModel};
use super::{public_server, TestServer};
use chrono::Utc;
use consensus_api::config::{MetaConfig, SnapshotConfig};
use consensus_api::hyphae::SystemCommandRunner;
use consensus_api::live::budget::{Ledger, LedgerClock, LedgerOptions};
use consensus_api::live::config::{vars, Exposure, LiveConfig};
use consensus_api::live::gate::{
    DeliberationFactory, FactoryError, FactoryStatus, GateParts, LiveGate as BudgetedGate,
};
use consensus_api::live::judge::JudgeCodes;
use consensus_api::live::snapshot_factory::{LogSink, SnapshotFactory};
use consensus_api::live::turnstile::{
    passing_answer, FakeSiteverify, Siteverify, SiteverifyResponse, TurnstileError,
    TurnstileVerifier, Verified, SITEVERIFY_TIMEOUT,
};
use consensus_api::meta::MetaModelClient;
use consensus_api::runs::Deliberation;
use consensus_api::server::BuildOptions;
use consensus_api::snapshot::verifier_from_config;
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
/// A second judge code in the same file (the per-code limit is per code).
pub const SECOND_JUDGE_CODE: &str = "judge-bravo-9900";
/// The live output bound the harness configures: distinct from the Meta
/// default (16,000), so a request proves the live value was sent.
pub const LIVE_MAX_COMPLETION_TOKENS: u64 = 12_000;
/// The visitor's client address and country the harness sends by default.
pub const VISITOR_IP: &str = "203.0.113.7";

/// What the harness's evidence factory builds.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// The production factory ([`SnapshotFactory`]) over a fixture snapshot
    /// directory (`super::snapshot::build`) and the fake `hyphae` binary
    /// (`super::snapshot::fake_hyphae`), boot-verified when the harness is
    /// built: sealed-snapshot evidence end to end.
    Snapshot {
        dir: PathBuf,
        /// `CONSENSUS_SNAPSHOT_VERIFIER_DIR`, when the artifact paths handed
        /// to the binary differ from the local ones.
        verifier_dir: Option<String>,
        /// `CONSENSUS_HYPHAE_BIN`.
        hyphae_bin: String,
        /// `CONSENSUS_HYPHAE_EXPECTED_ANCHOR`: must be the manifest's.
        anchor: String,
        /// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`; `None` leaves the manifest
        /// unpinned.
        manifest_sha256: Option<String>,
        /// `CONSENSUS_HYPHAE_BIN_SHA256` (the fixture records the fake
        /// binary's digest as the capture build,
        /// `super::snapshot::record_capture_binary`); `None` leaves the
        /// verifier binary unpinned.
        hyphae_bin_sha256: Option<String>,
    },
    /// What `serve-public` makes of these snapshot variables with live mode
    /// enabled ([`SnapshotFactory::live_evidence_from_vars`]): a factory
    /// over a snapshot that boots, the unavailable evidence of one that
    /// does not. The variables must be a usable configuration.
    SnapshotFromVars(Vec<(&'static str, String)>),
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
            FactoryMode::Snapshot { .. } | FactoryMode::SnapshotFromVars(_) => {
                return Err(FactoryError(
                    "the scripted factory builds no snapshot deliberation; the harness uses SnapshotFactory for that mode".into(),
                ))
            }
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
    /// Every token passes, however often it is presented.
    Pass,
    /// Cloudflare's real rule: a token passes once, or again only under
    /// the idempotency key of its first validation; any other repeat is
    /// `timeout-or-duplicate`.
    SingleUse,
    Fail,
    Outage,
}

/// A Siteverify over a [`FakeSiteverify`] whose answer the test switches.
pub fn scripted_siteverify(
    mode: Arc<Mutex<SiteverifyMode>>,
) -> (Arc<dyn Siteverify>, Arc<FakeSiteverify>) {
    let fake = Arc::new(FakeSiteverify::new(move |call, earlier| {
        match *mode.lock().unwrap() {
            SiteverifyMode::Pass => {
                Ok(passing_answer(TURNSTILE_HOST, TURNSTILE_ACTION, Utc::now()))
            }
            SiteverifyMode::SingleUse => {
                let spent_under_another_key = earlier
                    .iter()
                    .any(|c| c.token == call.token && c.idempotency_key != call.idempotency_key);
                if spent_under_another_key {
                    Ok(SiteverifyResponse {
                        success: false,
                        error_codes: vec!["timeout-or-duplicate".into()],
                        ..SiteverifyResponse::default()
                    })
                } else {
                    Ok(passing_answer(TURNSTILE_HOST, TURNSTILE_ACTION, Utc::now()))
                }
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

/// The token prefix [`HeldSiteverify`] holds.
pub const HELD_TOKEN_PREFIX: &str = "held-";

/// A Siteverify whose calls for tokens starting with [`HELD_TOKEN_PREFIX`]
/// wait until the test releases them (Siteverify answering slowly, at most
/// [`consensus_api::live::turnstile::SITEVERIFY_TIMEOUT`]) and are then
/// refused, as a bogus token is; every other token passes at once.
pub struct HeldSiteverify {
    state: Mutex<HeldState>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
struct HeldState {
    waiting: usize,
    released: bool,
    tokens: Vec<String>,
}

impl HeldSiteverify {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(HeldState::default()),
            changed: std::sync::Condvar::new(),
        })
    }

    /// Calls waiting now.
    pub fn held(&self) -> usize {
        self.state.lock().unwrap().waiting
    }

    /// Lets every held call (and every later one) answer.
    pub fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.changed.notify_all();
    }

    /// Every token presented so far, in order.
    pub fn tokens(&self) -> Vec<String> {
        self.state.lock().unwrap().tokens.clone()
    }
}

impl Siteverify for HeldSiteverify {
    fn verify(
        &self,
        token: &str,
        _remote_ip: Option<std::net::IpAddr>,
        _idempotency_key: &str,
    ) -> Result<Verified, TurnstileError> {
        let mut state = self.state.lock().unwrap();
        state.tokens.push(token.to_string());
        if !token.starts_with(HELD_TOKEN_PREFIX) {
            return Ok(Verified {
                action: TURNSTILE_ACTION.into(),
                hostname: TURNSTILE_HOST.into(),
                challenge_ts: Utc::now(),
                cdata: None,
            });
        }
        state.waiting += 1;
        let deadline = std::time::Instant::now() + SITEVERIFY_TIMEOUT;
        while !state.released {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            state = self.changed.wait_timeout(state, left).unwrap().0;
        }
        state.waiting -= 1;
        Err(TurnstileError::Failed {
            error_codes: vec!["invalid-input-response".into()],
        })
    }
}

/// The live configuration the harness loads: the owner's prices and caps
/// with pacing, slots and the daily quotas out of the way (ten ledger
/// slots, a hundred admissions an hour, the whole day available at once, a
/// hundred admissions a day per client key and per judge code; the quota
/// tests set their own), a 12k output bound, the ledger and judge codes
/// under `dir`. Live mode itself stays `false` here: the gate needs no
/// Turnstile secret when the test gives it a verifier, and the server's
/// enabling path is tested separately.
pub fn live_config(dir: &Path, overrides: &[(&'static str, &str)]) -> LiveConfig {
    let ledger_dir = dir.join("ledger");
    let codes = dir.join("judge-codes.txt");
    std::fs::write(
        &codes,
        format!(
            "# judges\n{}\n{}\n",
            JudgeCodes::digest_hex(JUDGE_CODE),
            JudgeCodes::digest_hex(SECOND_JUDGE_CODE)
        ),
    )
    .unwrap();
    let mut env: BTreeMap<&'static str, String> = BTreeMap::new();
    env.insert(vars::LEDGER_DIR, ledger_dir.display().to_string());
    env.insert(vars::JUDGE_CODES_FILE, codes.display().to_string());
    env.insert(vars::MAX_CONCURRENT, "10".into());
    env.insert(vars::HOURLY_ADMISSIONS, "100".into());
    env.insert(vars::BURST_FRACTION, "1".into());
    env.insert(vars::CLIENT_DAILY, "100".into());
    env.insert(vars::JUDGE_CODE_DAILY, "100".into());
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
    /// How long the snapshot factory ([`FactoryMode::Snapshot`]) reuses a
    /// status read: zero by default, so `/ready`, `/capabilities` and the
    /// gate see a change to the files at once (production:
    /// `READY_CACHE_TTL`).
    pub snapshot_status_ttl: Duration,
    /// The gate's Siteverify instead of the scripted one (which then only
    /// records nothing): a test that needs to hold calls open.
    pub siteverify: Option<Arc<dyn Siteverify>>,
    /// The ledger's clock (and so the day the daily quotas count in);
    /// the system clock by default.
    pub ledger_clock: Option<LedgerClock>,
}

impl Default for LiveOptions {
    fn default() -> Self {
        Self {
            factory: FactoryMode::Scripted,
            live_env: Vec::new(),
            server_env: Vec::new(),
            trust_proxy: true,
            snapshot_status_ttl: Duration::ZERO,
            siteverify: None,
            ledger_clock: None,
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
    /// The gate's evidence factory: the scripted one, or the snapshot
    /// factory also held in `snapshot`.
    pub factory: Arc<dyn DeliberationFactory>,
    /// The production factory, with [`FactoryMode::Snapshot`].
    pub snapshot: Option<Arc<SnapshotFactory>>,
    /// The `[snapshot]` lines the evidence factory logged.
    pub evidence_log: Arc<Mutex<Vec<String>>>,
}

/// The production factory over a snapshot directory, as `serve-public`
/// would boot it from the four variables: loaded, checked and verified
/// with the binary at `hyphae_bin` (the fake one in tests). Panics when the
/// snapshot does not open: a boot failure is tested through
/// [`SnapshotFactory::open`] directly.
pub fn snapshot_factory(
    dir: &Path,
    verifier_dir: Option<&str>,
    hyphae_bin: &str,
    anchor: &str,
) -> SnapshotFactory {
    pinned_snapshot_factory(dir, verifier_dir, hyphae_bin, anchor, None, None)
}

/// [`snapshot_factory`] with the digest pins
/// (`CONSENSUS_SNAPSHOT_MANIFEST_SHA256`, `CONSENSUS_HYPHAE_BIN_SHA256`).
pub fn pinned_snapshot_factory(
    dir: &Path,
    verifier_dir: Option<&str>,
    hyphae_bin: &str,
    anchor: &str,
    manifest_sha256: Option<&str>,
    hyphae_bin_sha256: Option<&str>,
) -> SnapshotFactory {
    let vars = [
        (
            "CONSENSUS_SNAPSHOT_DIR",
            Some(dir.to_string_lossy().into_owned()),
        ),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", Some(anchor.to_string())),
        ("CONSENSUS_HYPHAE_BIN", Some(hyphae_bin.to_string())),
        (
            "CONSENSUS_SNAPSHOT_VERIFIER_DIR",
            verifier_dir.map(str::to_string),
        ),
        (
            "CONSENSUS_SNAPSHOT_MANIFEST_SHA256",
            manifest_sha256.map(str::to_string),
        ),
        (
            "CONSENSUS_HYPHAE_BIN_SHA256",
            hyphae_bin_sha256.map(str::to_string),
        ),
    ];
    let cfg = SnapshotConfig::from_vars(|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .and_then(|(_, v)| v.clone())
    })
    .expect("a usable snapshot configuration")
    .expect("a snapshot configuration");
    let mut verifier = verifier_from_config(&cfg, SystemCommandRunner);
    SnapshotFactory::open(cfg, &mut verifier).expect("the fixture snapshot boots")
}

/// Builds the live server. `meta` is the configuration of the fake Chat
/// Completions server (see `tests/support`); the gate forces nothing on
/// it but the per-run pseudonym and the live output bound.
pub fn live_server(opts: LiveOptions, mut meta: MetaConfig) -> LiveHarness {
    let LiveOptions {
        factory: mode,
        live_env,
        server_env,
        trust_proxy,
        snapshot_status_ttl,
        siteverify: custom_siteverify,
        ledger_clock,
    } = opts;
    // The ledger and the codes file live next to the store, so they go
    // with the server's temporary directory.
    let scratch = super::TempDir::new("live-cfg");
    let live_env: Vec<(&'static str, &str)> =
        live_env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let config = live_config(scratch.path(), &live_env);
    let ledger_dir = config.ledger_dir.clone().expect("ledger dir");
    let ledger = Ledger::open_with(
        &ledger_dir,
        config.budget.clone(),
        ledger_clock.unwrap_or_else(|| Arc::new(Utc::now)),
        LedgerOptions {
            lock_wait: Duration::from_secs(5),
            lock_poll: Duration::from_millis(5),
        },
    )
    .expect("ledger opens");
    let siteverify_mode = Arc::new(Mutex::new(SiteverifyMode::Pass));
    let (scripted, siteverify) = scripted_siteverify(siteverify_mode.clone());
    let verifier = custom_siteverify.unwrap_or(scripted);
    let mut snapshot = None;
    let evidence_log: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = evidence_log.clone();
    let log: LogSink = Arc::new(move |line: &str| sink.lock().unwrap().push(line.to_string()));
    let factory: Arc<dyn DeliberationFactory> = match mode {
        FactoryMode::Snapshot {
            dir,
            verifier_dir,
            hyphae_bin,
            anchor,
            manifest_sha256,
            hyphae_bin_sha256,
        } => {
            let factory = Arc::new(
                pinned_snapshot_factory(
                    &dir,
                    verifier_dir.as_deref(),
                    &hyphae_bin,
                    &anchor,
                    manifest_sha256.as_deref(),
                    hyphae_bin_sha256.as_deref(),
                )
                .with_status_ttl(snapshot_status_ttl)
                .with_log(log),
            );
            snapshot = Some(factory.clone());
            factory
        }
        FactoryMode::SnapshotFromVars(vars) => SnapshotFactory::live_evidence_from_vars(
            |name| {
                vars.iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.clone())
            },
            log,
        )
        .expect("a usable snapshot configuration")
        .expect("a snapshot directory"),
        scripted => ScriptedEvidenceFactory::new(scripted),
    };
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
        // Every `/capabilities` and `/ready` reads the ledger's files (when
        // the lock is free), so a test sees what it wrote to them at once.
        snapshot_max_age: Duration::ZERO,
        config: config.clone(),
    }));
    let mut env: Vec<(&str, &str)> = vec![("CONSENSUS_LIVE_CONCURRENCY", "1")];
    let trust = if trust_proxy { "true" } else { "false" };
    env.push(("CONSENSUS_TRUST_PROXY_HEADERS", trust));
    env.extend(server_env.iter().map(|(k, v)| (*k, v.as_str())));
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
        snapshot,
        evidence_log,
    }
}

impl LiveHarness {
    pub fn set_siteverify(&self, mode: SiteverifyMode) {
        *self.siteverify_mode.lock().unwrap() = mode;
    }

    /// The `[snapshot]` lines logged so far.
    pub fn evidence_lines(&self) -> Vec<String> {
        self.evidence_log.lock().unwrap().clone()
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

    /// Appends a line no record parser accepts to today's day file, as a
    /// damaged disk or a foreign writer would: every later read of the
    /// ledger fails with `Corrupt`.
    pub fn corrupt_ledger(&self) {
        let day = Utc::now().format("%Y-%m-%d");
        let path = self.ledger_dir.join(format!("{day}.jsonl"));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        std::io::Write::write_all(&mut file, b"not a ledger record\n").unwrap();
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
