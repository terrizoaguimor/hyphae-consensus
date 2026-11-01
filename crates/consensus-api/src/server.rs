// SPDX-License-Identifier: Apache-2.0
//! The two servers (C7): `serve-public` and `serve-operator`.
//!
//! - **`serve-public`** listens on a unix socket (mode 0660) that only
//!   cloudflared reaches, or, for development, on a loopback TCP address
//!   (`CONSENSUS_PUBLIC_BIND`; anything else is refused). It serves the
//!   static web export with per-page CSP, `/api/v1/*` for anonymous
//!   cookie sessions, `/research/*`, and a JSON 404 for unknown `/api`
//!   paths, all behind the portal-wide geographic block (ADR-0007 duty
//!   7). Visitors send no free text: guided runs are chosen by name, live
//!   runs by preset. Live mode (`CONSENSUS_LIVE_ENABLED=true`) runs on the
//!   owner's key behind [`crate::live::gate::LiveGate`]: Turnstile, the
//!   USD ledger, the geographic policy and the judge lane; it needs the
//!   Meta key (`CONSENSUS_META_API_KEY_FILE`), the live configuration
//!   ([`crate::live::config`]) and the sealed-snapshot evidence factory
//!   (`CONSENSUS_SNAPSHOT_DIR`, [`SnapshotFactory::live_evidence_from_env`],
//!   loaded only with live mode enabled); a missing or unusable setting
//!   stops startup naming the variable, a snapshot that does not boot
//!   leaves the portal up with live mode unavailable (`503
//!   live_unavailable`, `/ready` `live.evidence_ok: false`), and `503
//!   live_disabled` is the answer when live mode is off. Decisions carry
//!   `public_demo` authority.
//! - **`serve-operator`** listens on loopback only. Every `/api` request
//!   needs `Authorization: Bearer <token>` (constant-time compare; the
//!   token comes from `CONSENSUS_OPERATOR_TOKEN_FILE` or is generated and
//!   printed once), `Host` must be `127.0.0.1:<port>` or
//!   `localhost:<port>`, and a foreign `Origin` is rejected. It keeps the
//!   operator capabilities (`model=fake|meta`, `memory=fake|offline|hyphae`
//!   with a caller-chosen project) through the same run manager, store and
//!   streams; its decisions carry `operator` authority and its views keep
//!   the raw failure reasons. With `CONSENSUS_LIVE_LEDGER_DIR` set (the
//!   server, where the operator unit loads `public.env` too) every
//!   `model=meta` run is metered on that live ledger, on the operator lane
//!   ([`operator_ledger`]): the run's worst case reserved before it starts,
//!   every HTTP attempt drawn and settled, the reservation released when
//!   the run ends, so the caps, every trip and the kill switch apply to it.
//!   Without the variable those runs are unmetered, and the startup line
//!   says so ([`operator_metering_line`]).
//!
//! Both answer `202` on creation and stream the deliberation over SSE;
//! the old synchronous `POST /api/runs` no longer exists on either.

use crate::api::contract;
use crate::api::{
    meta_from_env, operator_router, public_router, AppState, ConsolePage, CreationKeys, LiveGate,
    MetaSource, OperatorAuth, RateTables, ServeConfig,
};
use crate::config::{MetaConfig, META_KEY_FILE_VARIABLE, META_KEY_VARIABLES};
use crate::limits::{ClientKeyPolicy, StreamCaps};
use crate::live::budget::Ledger;
use crate::live::config::{vars as live_vars, EnvSource, LiveConfig};
use crate::live::gate::{DeliberationFactory, GateParts, SNAPSHOT_DIR_VAR, SNAPSHOT_MAX_AGE};
use crate::live::geo::GeoPolicy;
use crate::live::snapshot_factory::{SnapshotFactory, UNUSED_NOTICE};
use crate::live::turnstile::{Siteverify, TurnstileVerifier};
use crate::public_config::{
    operator_limits, public_shutdown_grace, OperatorConfig, ProcessEnv, PublicConfig,
    PublicListener, RuntimeConfig, DEFAULT_OPERATOR_BIND,
};
use crate::public_view::Surface;
use crate::runs::{ManagerSettings, RunManager};
use crate::session::{random_hex_32, SessionStore};
use crate::showcase::Showcase;
use crate::static_files::{csp_hash, inline_bodies, StaticSite};
use crate::stream::Broadcasts;
use axum::Router;
use consensus_runtime::{RecoveryReport, RunLimits, RunStore};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CONSOLE_HTML: &str = include_str!("console.html");
/// Kept for callers that referred to the operator console's address.
pub const DEFAULT_BIND: &str = DEFAULT_OPERATOR_BIND;
/// Build identity reported by `/health`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How often expired visitor runs are purged.
const PURGE_INTERVAL: Duration = Duration::from_secs(600);

/// Knobs a test or an embedding sets; the defaults are production's.
pub struct BuildOptions {
    pub broadcast_capacity: usize,
    pub admission_wait: Duration,
    pub max_queue: usize,
    pub shutdown_grace: Duration,
    pub live_gate: Option<Arc<dyn LiveGate>>,
    /// Run store recovery at build time (always on in the servers).
    pub recover: bool,
    /// The geographic policy of the public listener. `serve-public` loads
    /// it from the `CONSENSUS_GEO_*` variables whether or not live mode is
    /// enabled; the default is the owner's documented lists, which is what
    /// those variables default to.
    pub geo: GeoPolicy,
    /// The operator server's metering: the live ledger its `model=meta`
    /// runs reserve, draw and settle on ([`operator_ledger`] in
    /// `serve-operator`); `None` leaves them unmetered.
    pub operator_ledger: Option<Ledger>,
    /// Where the operator server reads a `model=meta` run's Meta
    /// configuration; `None` is the process environment
    /// ([`crate::api::meta_from_env`]).
    pub operator_meta: Option<MetaSource>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            broadcast_capacity: 256,
            admission_wait: Duration::from_secs(2),
            max_queue: 32,
            shutdown_grace: Duration::from_secs(60),
            live_gate: None,
            recover: true,
            geo: GeoPolicy::default_policy(),
            operator_ledger: None,
            operator_meta: None,
        }
    }
}

/// A built server: its state, its router, and what building found.
pub struct Built {
    pub state: AppState,
    pub router: Router,
    pub recovery: Option<RecoveryReport>,
    pub warnings: Vec<String>,
}

/// What both servers build first.
struct RuntimeParts {
    manager: Arc<RunManager>,
    showcase: Arc<Showcase>,
    recovery: Option<RecoveryReport>,
}

/// Opens the store, recovers it, builds the manager and the showcase.
fn runtime_parts(rt: &RuntimeConfig, opts: &BuildOptions) -> Result<RuntimeParts, String> {
    let store = RunStore::open(&rt.run_store_dir)
        .map_err(|e| format!("run store {}: {e}", rt.run_store_dir.display()))?;
    let store = Arc::new(store);
    let recovery = if opts.recover {
        Some(
            store
                .recover()
                .map_err(|e| format!("run store recovery: {e}"))?,
        )
    } else {
        None
    };
    let broadcasts = Broadcasts::new(opts.broadcast_capacity);
    let manager = RunManager::new(
        store,
        broadcasts,
        ManagerSettings {
            guided_concurrency: rt.guided_concurrency,
            live_concurrency: rt.live_concurrency,
            deadline: rt.run_deadline,
            admission_wait: opts.admission_wait,
            max_queue: opts.max_queue,
            shutdown_grace: opts.shutdown_grace,
        },
    );
    let showcase = Arc::new(Showcase::load(&rt.research_dir));
    Ok(RuntimeParts {
        manager,
        showcase,
        recovery,
    })
}

fn console_page() -> ConsolePage {
    let scripts: Vec<String> = inline_bodies(CONSOLE_HTML, "script")
        .iter()
        .map(|b| csp_hash(b))
        .collect();
    let styles: Vec<String> = inline_bodies(CONSOLE_HTML, "style")
        .iter()
        .map(|b| csp_hash(b))
        .collect();
    let quote = |hashes: &[String]| -> String {
        hashes
            .iter()
            .map(|h| format!(" '{h}'"))
            .collect::<Vec<_>>()
            .join("")
    };
    ConsolePage {
        html: CONSOLE_HTML.to_string(),
        csp: format!(
            "default-src 'none'; script-src{}; style-src{}; style-src-attr 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            quote(&scripts),
            quote(&styles)
        ),
    }
}

/// Builds the public server from its configuration.
pub fn build_public(cfg: &PublicConfig, opts: BuildOptions) -> Result<Built, String> {
    let RuntimeParts {
        manager,
        showcase,
        recovery,
    } = runtime_parts(&cfg.runtime, &opts)?;
    let mut warnings = Vec::new();
    let static_site = match StaticSite::load(&cfg.web_root, &cfg.csp_manifest) {
        Ok((site, warning)) => {
            warnings.extend(warning);
            Some(Arc::new(site))
        }
        Err(e) => {
            warnings.push(format!("{e}: the API runs without the web export"));
            None
        }
    };
    let status = showcase.status();
    if !status.ok {
        warnings.push(format!(
            "showcase: {} entries, errors: {}",
            status.entries,
            status.errors.join("; ")
        ));
    }
    let config = ServeConfig {
        surface: Surface::Public,
        allowed_origins: cfg.allowed_origins.clone(),
        allowed_hosts: cfg.allowed_hosts.clone(),
        origin_required: true,
        client_key_policy: if cfg.trust_proxy_headers {
            ClientKeyPolicy::TrustedProxyHeader
        } else {
            ClientKeyPolicy::PeerAddress
        },
        cookie_secure: cfg.cookie_secure,
        sse: cfg.runtime.sse,
        rates: cfg.runtime.rates,
        session_ttl: cfg.session_ttl,
        session_max: cfg.session_max,
        guided_ring: cfg.guided_ring,
        visitor_access: cfg.visitor_access,
        visitor_delete: cfg.visitor_delete,
        run_limits: cfg.runtime.max_run_tokens,
        guided_concurrency: cfg.runtime.guided_concurrency,
        live_concurrency: cfg.runtime.live_concurrency,
        run_deadline: cfg.runtime.run_deadline,
        request_timeout: cfg.runtime.request_timeout,
        version: VERSION.into(),
    };
    let state = AppState {
        sessions: Arc::new(SessionStore::new(
            cfg.session_ttl,
            cfg.session_max,
            cfg.cookie_secure,
        )),
        rates: Arc::new(RateTables::new(&cfg.runtime.rates)),
        stream_caps: Arc::new(StreamCaps::new(
            cfg.runtime.sse.max_per_run,
            cfg.runtime.sse.max_per_client,
            cfg.runtime.sse.max_global,
            cfg.runtime.rates.table_max,
        )),
        config: Arc::new(config),
        manager,
        showcase,
        static_site,
        operator_auth: None,
        live_gate: opts.live_gate,
        console: None,
        contract_sha256: Arc::new(contract::contract_sha256()),
        creation_keys: Arc::new(CreationKeys::new(cfg.runtime.rates.table_max)),
        ready_cache: Arc::new(Mutex::new(None)),
        geo: Arc::new(opts.geo),
        // The public listener has no operator route: its live runs are the
        // gate's.
        operator_ledger: None,
        operator_meta: meta_from_env(),
    };
    let router = public_router(state.clone());
    Ok(Built {
        state,
        router,
        recovery,
        warnings,
    })
}

/// Builds the operator server with `token` as its bearer token.
pub fn build_operator(
    cfg: &OperatorConfig,
    token: &str,
    opts: BuildOptions,
) -> Result<Built, String> {
    let RuntimeParts {
        manager,
        showcase,
        recovery,
    } = runtime_parts(&cfg.runtime, &opts)?;
    let (hosts, origins) = cfg.allowed_names();
    // One operator on a loopback peer: the visitor rates would only get
    // in the console's way.
    let (rates, sse) = operator_limits(&cfg.runtime);
    let config = ServeConfig {
        surface: Surface::Operator,
        allowed_origins: origins,
        allowed_hosts: hosts,
        origin_required: false,
        client_key_policy: ClientKeyPolicy::PeerAddress,
        cookie_secure: false,
        sse,
        rates,
        session_ttl: Duration::from_secs(0),
        session_max: 0,
        guided_ring: 0,
        visitor_access: Duration::from_secs(0),
        visitor_delete: Duration::from_secs(0),
        run_limits: cfg.runtime.max_run_tokens,
        guided_concurrency: cfg.runtime.guided_concurrency,
        live_concurrency: cfg.runtime.live_concurrency,
        run_deadline: cfg.runtime.run_deadline,
        request_timeout: cfg.runtime.request_timeout,
        version: VERSION.into(),
    };
    let state = AppState {
        sessions: Arc::new(SessionStore::new(Duration::from_secs(60), 1, false)),
        rates: Arc::new(RateTables::new(&rates)),
        stream_caps: Arc::new(StreamCaps::new(
            sse.max_per_run,
            sse.max_per_client,
            sse.max_global,
            rates.table_max,
        )),
        config: Arc::new(config),
        manager,
        showcase,
        static_site: None,
        operator_auth: Some(Arc::new(OperatorAuth::new(token))),
        live_gate: None,
        console: Some(Arc::new(console_page())),
        contract_sha256: Arc::new(contract::contract_sha256()),
        creation_keys: Arc::new(CreationKeys::new(rates.table_max)),
        ready_cache: Arc::new(Mutex::new(None)),
        // Never consulted: the operator router has no geographic block
        // (it listens on loopback, behind SSH).
        geo: Arc::new(opts.geo),
        operator_ledger: opts.operator_ledger,
        operator_meta: opts.operator_meta.unwrap_or_else(meta_from_env),
    };
    let router = operator_router(state.clone());
    Ok(Built {
        state,
        router,
        recovery,
        warnings: Vec::new(),
    })
}

/// The operator token: from the configured file (at least 32
/// characters), or generated. The second value says it was generated and
/// must be printed once.
pub fn operator_token(cfg: &OperatorConfig) -> Result<(String, bool), String> {
    match &cfg.token_file {
        Some(path) => {
            let token = std::fs::read_to_string(path)
                .map_err(|e| format!("CONSENSUS_OPERATOR_TOKEN_FILE {}: {e}", path.display()))?;
            let token = token.trim().to_string();
            if token.len() < 32 {
                return Err(format!(
                    "CONSENSUS_OPERATOR_TOKEN_FILE {}: the token must have at least 32 characters",
                    path.display()
                ));
            }
            Ok((token, false))
        }
        None => Ok((random_hex_32()?, true)),
    }
}

fn print_recovery(report: &RecoveryReport) {
    if !report.interrupted.is_empty() {
        eprintln!(
            "[recovery] closed {} interrupted run(s) as failed",
            report.interrupted.len()
        );
    }
    for (run_id, reason) in &report.unreplayable {
        eprintln!("[recovery] run {run_id} fails replay and was left untouched: {reason}");
    }
    for (run_id, reason) in &report.inconsistent {
        eprintln!("[recovery] run {run_id} is inconsistent and was left untouched: {reason}");
    }
    if !report.missing.is_empty() {
        eprintln!(
            "[recovery] {} indexed run(s) have no files",
            report.missing.len()
        );
    }
    if !report.unindexed.is_empty() {
        eprintln!(
            "[recovery] {} run file(s) are not in the index and are not served",
            report.unindexed.len()
        );
    }
}

/// Resolves when the process is asked to stop (Ctrl-C, and SIGTERM on
/// unix).
async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Runs the purge loop and the shutdown coordination around `serve`.
async fn run_server<F>(state: &AppState, serve: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = std::io::Result<()>>,
{
    let manager = state.manager.clone();
    let purge = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PURGE_INTERVAL);
        loop {
            ticker.tick().await;
            let m = manager.clone();
            if let Ok(Ok(report)) = tokio::task::spawn_blocking(move || m.purge_expired()).await {
                if !report.purged.is_empty() {
                    eprintln!("[retention] purged {} expired run(s)", report.purged.len());
                }
            }
        }
    });
    serve.await?;
    purge.abort();
    Ok(())
}

/// The shutdown future handed to axum: waits for the stop signal, then
/// cancels every active run and waits for them to reach a checkpoint.
async fn graceful(manager: Arc<RunManager>) {
    stop_signal().await;
    eprintln!("[shutdown] stopping: cancelling active runs");
    manager.shutdown().await;
}

/// The Meta configuration of the public server, in public mode: the key
/// from `CONSENSUS_META_API_KEY_FILE` (or a key variable), every setting
/// validated, the debug directory refused. An absent key is an error:
/// live mode is enabled, so the key is required.
fn public_meta_config() -> Result<MetaConfig, String> {
    let mut meta = MetaConfig::try_from_env()
        .map_err(|e| format!("public live mode: {e}"))?
        .ok_or_else(|| {
            format!(
                "public live mode requires the Meta key: set {META_KEY_FILE_VARIABLE} (a credential file) or one of {}",
                META_KEY_VARIABLES.join(", ")
            )
        })?;
    meta.public_mode = true;
    meta.validate().map_err(|e| match e.setting {
        "debug_dir" => format!("CONSENSUS_META_DEBUG_DIR: {}", e.message),
        _ => format!("public live mode: {e}"),
    })?;
    Ok(meta)
}

/// Assembles the public live gate from an enabled, validated live
/// configuration: the ledger on `CONSENSUS_LIVE_LEDGER_DIR`, the Turnstile
/// verifier over the HTTP transport (test secrets were refused by the
/// configuration load in public mode), the judge codes, the geographic
/// policy, the Meta configuration and the evidence factory. The factory is
/// the sealed snapshot ([`SnapshotFactory::live_evidence_from_env`],
/// plugged in by [`serve_public`]; the snapshot directory is
/// `CONSENSUS_SNAPSHOT_DIR`), possibly unavailable when it did not boot
/// (the gate then refuses every admission). Without one, startup fails
/// naming that variable: there is no fallback to simulated evidence.
pub fn build_live_gate(
    live: &LiveConfig,
    meta: MetaConfig,
    factory: Option<Arc<dyn DeliberationFactory>>,
) -> Result<Arc<dyn LiveGate>, String> {
    let factory = factory.ok_or_else(|| {
        format!(
            "public live mode requires the sealed snapshot ({SNAPSHOT_DIR_VAR}): the evidence factory is not wired while that variable is unset; set it (with CONSENSUS_HYPHAE_EXPECTED_ANCHOR) or set {} to false",
            live_vars::LIVE_ENABLED
        )
    })?;
    let ledger_dir = live.ledger_dir.as_ref().ok_or_else(|| {
        format!(
            "{} is required when {} is true",
            live_vars::LEDGER_DIR,
            live_vars::LIVE_ENABLED
        )
    })?;
    let ledger = Ledger::open(ledger_dir, live.budget.clone())
        .map_err(|e| format!("{}: {e}", live_vars::LEDGER_DIR))?;
    let turnstile = live.turnstile.as_ref().ok_or_else(|| {
        format!(
            "{} and {} are required when {} is true",
            live_vars::TURNSTILE_SECRET_FILE,
            live_vars::TURNSTILE_HOSTNAME,
            live_vars::LIVE_ENABLED
        )
    })?;
    let siteverify: Arc<dyn Siteverify> = Arc::new(TurnstileVerifier::from_config(turnstile));
    let gate = crate::live::gate::LiveGate::new(GateParts {
        ledger,
        siteverify,
        geo: live.geo.clone(),
        judge_codes: live.judge_codes.clone(),
        meta,
        run_limits: RunLimits {
            max_run_tokens: live.budget.run_token_cap,
        },
        factory,
        snapshot_max_age: SNAPSHOT_MAX_AGE,
        config: live.clone(),
    });
    Ok(Arc::new(gate))
}

/// With live mode enabled on the unix socket (the deployment path), the
/// sealed snapshot's manifest and the verifier binary must both be pinned
/// ([`crate::config::MANIFEST_SHA256_VAR`],
/// [`crate::config::HYPHAE_BIN_SHA256_VAR`]): production never serves an
/// unpinned snapshot, and a missing pin is a configuration error naming
/// the variable. A development TCP bind may leave them unset (a
/// containerised verifier is a wrapper, not the pinned binary); its
/// startup line says what is not pinned.
pub fn require_snapshot_pins(
    listener: &PublicListener,
    get: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    if !matches!(listener, PublicListener::UnixSocket(_)) {
        return Ok(());
    }
    let missing: Vec<&str> = [
        crate::config::MANIFEST_SHA256_VAR,
        crate::config::HYPHAE_BIN_SHA256_VAR,
    ]
    .into_iter()
    .filter(|name| get(name).is_none_or(|value| value.trim().is_empty()))
    .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "public live mode on the unix socket requires {}: production serves only a pinned snapshot and a pinned verifier (runbook 3.3 and 4.1); a development server on CONSENSUS_PUBLIC_BIND may leave them unset",
            missing.join(" and ")
        ))
    }
}

/// The Meta call limits the public live gate runs with, read from its
/// validated [`MetaConfig`] without the key (for the boot line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLimits {
    /// One call's budget (`CONSENSUS_MODEL_TIMEOUT_SECS`).
    pub timeout: Duration,
    /// The connect timeout inside it.
    pub connect_timeout: Duration,
    /// HTTP attempts per call (`CONSENSUS_META_MAX_ATTEMPTS`).
    pub max_attempts: u32,
    /// Whether `reasoning_effort` is sent (its value is not printed).
    pub reasoning_effort_set: bool,
}

impl CallLimits {
    pub fn of(meta: &MetaConfig) -> Self {
        Self {
            timeout: meta.timeout,
            connect_timeout: crate::meta::connect_timeout(meta.timeout),
            max_attempts: meta.max_attempts,
            reasoning_effort_set: meta.reasoning_effort.is_some(),
        }
    }
}

/// `serve-public`'s boot line of its effective limits, printed to stderr
/// (the journal): the Meta call limits when live mode is enabled (`call`),
/// the output bound and run token cap the live gate applies, the run
/// deadline, the shutdown grace, the live concurrency (the manager's slots
/// and the ledger's), the burst fraction and the daily quotas. Only
/// numbers and set/unset flags: never a key, a secret or a path.
pub fn public_limits_line(
    call: Option<&CallLimits>,
    live: &LiveConfig,
    rt: &RuntimeConfig,
    shutdown_grace: Duration,
) -> String {
    let b = &live.budget;
    let calls = match call {
        Some(c) => format!(
            "per-call timeout {} s, connect timeout {} s, max attempts {}, reasoning_effort {}",
            c.timeout.as_secs(),
            c.connect_timeout.as_secs(),
            c.max_attempts,
            if c.reasoning_effort_set {
                "set"
            } else {
                "unset"
            },
        ),
        None => "per-call timeout n/a (live mode disabled)".to_string(),
    };
    format!(
        "[startup] limits: {calls}, max_completion_tokens {}, run deadline {} s, live run token cap {}, shutdown grace {} s, live concurrency {} (ledger {}), burst fraction {} (${} at 00:00 UTC), client daily {}, judge code daily {}",
        b.max_completion_tokens,
        rt.run_deadline.as_secs(),
        b.run_token_cap,
        shutdown_grace.as_secs(),
        rt.live_concurrency,
        b.max_concurrent,
        crate::live::money::Usd::from_micros(u64::from(b.burst_fraction.parts_per_million())),
        b.burst(),
        live.client_daily,
        live.judge_code_daily,
    )
}

/// The boot WARNING when one call's budget cannot hold the output bound at
/// [`crate::config::MIN_OUTPUT_TOKENS_PER_SEC`]: such a call can time out
/// although it would have completed (live run M01). The server still
/// starts.
pub fn timeout_warning(call: &CallLimits, max_completion_tokens: u64) -> Option<String> {
    if crate::config::timeout_holds_output(call.timeout, max_completion_tokens) {
        return None;
    }
    Some(format!(
        "[startup] WARNING: CONSENSUS_MODEL_TIMEOUT_SECS={} cannot hold max_completion_tokens={} at {} tokens/s (needs at least {} s): a long generation can time out and fail its run",
        call.timeout.as_secs(),
        max_completion_tokens,
        crate::config::MIN_OUTPUT_TOKENS_PER_SEC,
        max_completion_tokens.div_ceil(crate::config::MIN_OUTPUT_TOKENS_PER_SEC)
    ))
}

/// `serve-public` over the sealed snapshot named by `CONSENSUS_SNAPSHOT_DIR`
/// ([`SnapshotFactory::live_evidence_from_env`]), loaded only with live
/// mode enabled; see [`serve_public_with`].
pub async fn serve_public() -> Result<(), Box<dyn std::error::Error>> {
    serve_public_with(SnapshotFactory::live_evidence_from_env).await
}

/// `serve-public` with `evidence`, the loader of the factory live runs
/// deliberate over (the sealed snapshot in production; a test's scripted
/// factory). The live configuration is read first (it is read whether or
/// not live mode is enabled: the geographic policy applies to the whole
/// portal either way), and `evidence` runs only when live mode is
/// enabled, before anything listens (ADR-0008 decision 1):
/// - live mode disabled: no snapshot is loaded or verified; a set
///   `CONSENSUS_SNAPSHOT_DIR` earns one notice ([`UNUSED_NOTICE`]), so a
///   missing, placeholder or broken snapshot never stops a portal that does
///   not use it;
/// - live mode enabled: a configuration error of the loader (a snapshot or
///   anchor variable missing or unusable), an unset
///   `CONSENSUS_SNAPSHOT_DIR` ([`build_live_gate`]) and, on the unix
///   socket, a missing manifest or verifier pin ([`require_snapshot_pins`])
///   stop startup naming the variable; a snapshot that does not boot does
///   not: the portal serves
///   and live mode is unavailable until the snapshot is fixed and the
///   server restarted (the code is logged; `/ready` reports
///   `live.evidence_ok: false`).
///
/// The live setup runs on the blocking pool: the snapshot boot runs the
/// verifier and hashes the artifacts, and the gate's Siteverify transport
/// is reqwest's blocking client, which must not be built on an async worker
/// (a debug build panics there: "Cannot drop a runtime in a context where
/// blocking is not allowed").
pub async fn serve_public_with<F>(evidence: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce() -> Result<Option<Arc<dyn DeliberationFactory>>, String> + Send + 'static,
{
    let cfg = PublicConfig::from_env(&ProcessEnv)?;
    let live = LiveConfig::from_env()?;
    // What the startup lines say about the evidence of an enabled live mode.
    let mut evidence_status = None;
    // The Meta call limits of an enabled live mode (no key).
    let mut call_limits: Option<CallLimits> = None;
    let live_gate = if live.enabled {
        require_snapshot_pins(&cfg.listener, |name| std::env::var(name).ok())?;
        let enabled = live.clone();
        let (gate, status, limits) = tokio::task::spawn_blocking(move || {
            let meta = public_meta_config()?;
            let factory = evidence()?;
            let status = factory.as_ref().map(|f| f.status());
            let limits = CallLimits::of(&meta);
            Ok::<_, String>((build_live_gate(&enabled, meta, factory)?, status, limits))
        })
        .await
        .map_err(|e| format!("public live mode setup: {e}"))??;
        evidence_status = status;
        call_limits = Some(limits);
        Some(gate)
    } else {
        let snapshot_dir_set =
            std::env::var(SNAPSHOT_DIR_VAR).is_ok_and(|dir| !dir.trim().is_empty());
        if snapshot_dir_set {
            eprintln!("{UNUSED_NOTICE}");
        }
        None
    };
    // A live call in progress at the stop signal may take its whole
    // budget: the grace covers it, then the call is abandoned and settled.
    let shutdown_grace = match &call_limits {
        Some(limits) => public_shutdown_grace(limits.timeout),
        None => BuildOptions::default().shutdown_grace,
    };
    let built = build_public(
        &cfg,
        BuildOptions {
            live_gate,
            geo: live.geo.clone(),
            shutdown_grace,
            ..BuildOptions::default()
        },
    )?;
    eprintln!(
        "{}",
        public_limits_line(call_limits.as_ref(), &live, &cfg.runtime, shutdown_grace)
    );
    if let Some(warning) = call_limits
        .as_ref()
        .and_then(|limits| timeout_warning(limits, live.budget.max_completion_tokens))
    {
        eprintln!("{warning}");
    }
    for warning in &built.warnings {
        eprintln!("[startup] {warning}");
    }
    if let Some(report) = &built.recovery {
        print_recovery(report);
    }
    let Built { state, router, .. } = built;
    println!(
        "Hyphae Consensus public server (schema {}, contract {})",
        consensus_domain::SCHEMA_VERSION,
        state.contract_sha256
    );
    println!("Executor disabled: decisions are recorded only.");
    println!(
        "Portal-wide geographic block: {} countries; live-only: {}",
        live.geo.portal_blocked().len(),
        live.geo
            .live_only_blocked()
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if live.enabled {
        // The code itself is the `[snapshot]` line's, logged once.
        let evidence = match &evidence_status {
            Some(status) if !status.ok => "; its evidence is UNAVAILABLE (the [snapshot] line says why): every live run is refused (503 live_unavailable) until the snapshot is fixed and the server restarted",
            _ => "",
        };
        let judge_lane = if live.judge_lane_open() {
            format!("open, {} runs per code a day", live.judge_code_daily)
        } else {
            "closed".to_string()
        };
        println!(
            "Public live mode: enabled on the owner's key (daily cap {}, lifetime cap {}, {} public runs per client a day, judge lane {judge_lane}){evidence}",
            live.budget.daily_cap,
            live.budget.lifetime_cap,
            live.client_daily
        );
    } else {
        println!("Public live mode: disabled (CONSENSUS_LIVE_ENABLED)");
    }
    match &cfg.listener {
        PublicListener::Tcp(addr) => {
            let listener = tokio::net::TcpListener::bind(addr).await?;
            println!("Listening on http://{addr}/ (development; loopback only)");
            let app = router.into_make_service_with_connect_info::<SocketAddr>();
            let manager = state.manager.clone();
            run_server(&state, async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(graceful(manager))
                    .await
            })
            .await
        }
        PublicListener::UnixSocket(path) => serve_unix(&state, router, path).await,
    }
}

#[cfg(unix)]
async fn serve_unix(
    state: &AppState,
    router: Router,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    println!("Listening on unix socket {} (mode 0660)", path.display());
    let manager = state.manager.clone();
    let result = run_server(state, async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(graceful(manager))
            .await
    })
    .await;
    let _ = std::fs::remove_file(path);
    result
}

#[cfg(not(unix))]
async fn serve_unix(
    _state: &AppState,
    _router: Router,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!(
        "unix sockets are unavailable on this platform (socket {}); set CONSENSUS_PUBLIC_BIND to a loopback address for development",
        path.display()
    )
    .into())
}

/// The live ledger `serve-operator` meters its `model=meta` runs on: the
/// one `CONSENSUS_LIVE_LEDGER_DIR` names (on the server it comes from
/// `public.env`, which the operator unit loads before `operator.env`),
/// opened with the same caps as `serve-public`'s and read credential-free
/// ([`LiveConfig::load_credential_free`]: the live-mode switch, the
/// Turnstile secret file and the judge-codes file are hidden from the
/// load). `Ok(None)` without the variable: a development machine running
/// the recorded batteries. An absent directory or an unusable budget
/// variable is an error naming its variable, never a fresh, empty ledger
/// ([`crate::ledger_cli::open_configured`]).
pub fn operator_ledger(env: &dyn EnvSource) -> Result<Option<Ledger>, String> {
    let cfg = LiveConfig::load_credential_free(env).map_err(|e| e.to_string())?;
    crate::ledger_cli::open_configured(&cfg)
}

/// The one line `serve-operator` prints about the metering of its
/// `model=meta` runs.
pub fn operator_metering_line(ledger: Option<&Ledger>) -> String {
    match ledger {
        Some(ledger) => {
            let cfg = ledger.config();
            format!(
                "Operator model=meta runs: metered on the live ledger at {} (operator lane: daily cap {}, lifetime cap {}; a trip or the kill switch refuses them)",
                ledger.dir().display(),
                cfg.daily_cap,
                cfg.lifetime_cap
            )
        }
        None => format!(
            "Operator model=meta runs: UNMETERED ({} is not set): no ledger cap, trip or kill switch applies to them",
            live_vars::LEDGER_DIR
        ),
    }
}

/// `serve-operator`.
pub async fn serve_operator() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = OperatorConfig::from_env(&ProcessEnv)?;
    let (token, generated) = operator_token(&cfg)?;
    // The ledger's files are read and written under its lock: off the
    // async workers.
    let ledger = tokio::task::spawn_blocking(|| operator_ledger(&crate::live::config::ProcessEnv))
        .await
        .map_err(|e| format!("operator ledger setup: {e}"))??;
    let metering = operator_metering_line(ledger.as_ref());
    let built = build_operator(
        &cfg,
        &token,
        BuildOptions {
            operator_ledger: ledger,
            ..BuildOptions::default()
        },
    )?;
    if let Some(report) = &built.recovery {
        print_recovery(report);
    }
    let Built { state, router, .. } = built;
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    println!("Hyphae Consensus operator console: http://{}/", cfg.bind);
    println!("Executor disabled: decisions are recorded only.");
    println!(
        "Per-run token cap: {} (CONSENSUS_MAX_RUN_TOKENS)",
        cfg.runtime.max_run_tokens.max_run_tokens
    );
    println!("{metering}");
    if generated {
        println!("Operator bearer token (required on every /api route; paste it into the console): {token}");
    } else {
        println!("Operator bearer token: read from CONSENSUS_OPERATOR_TOKEN_FILE");
    }
    let app = router.into_make_service_with_connect_info::<SocketAddr>();
    let manager = state.manager.clone();
    run_server(&state, async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(graceful(manager))
            .await
    })
    .await
}

/// Kept for `cargo run -p consensus-api -- serve`: the operator console.
pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    serve_operator().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn live_mode_on_the_socket_requires_both_snapshot_pins() {
        let socket = PublicListener::UnixSocket(PathBuf::from("/run/consensus/public.sock"));
        let tcp = PublicListener::Tcp("127.0.0.1:8788".parse().unwrap());
        let digest = "a".repeat(64);
        let vars = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<String, String>>()
        };
        let manifest = crate::config::MANIFEST_SHA256_VAR;
        let binary = crate::config::HYPHAE_BIN_SHA256_VAR;
        let both = vars(&[(manifest, &digest), (binary, &digest)]);
        assert!(require_snapshot_pins(&socket, |n| both.get(n).cloned()).is_ok());
        for (present, missing) in [
            (vars(&[(binary, &digest)]), vec![manifest]),
            (vars(&[(manifest, &digest)]), vec![binary]),
            (vars(&[(manifest, "  "), (binary, &digest)]), vec![manifest]),
            (vars(&[]), vec![manifest, binary]),
        ] {
            let err = require_snapshot_pins(&socket, |n| present.get(n).cloned()).unwrap_err();
            for name in &missing {
                assert!(err.contains(name), "{err}");
            }
        }
        // A development TCP bind may run unpinned.
        let none = vars(&[]);
        assert!(require_snapshot_pins(&tcp, |n| none.get(n).cloned()).is_ok());
    }

    /// A key of the documented shape that must never appear in output.
    const CANARY: &str = "LLM_7739320002_canary-key-boot-line-never-leak-5c2e";

    fn public_values() -> (MetaConfig, LiveConfig, RuntimeConfig) {
        let meta_vars: BTreeMap<&str, &str> = [
            ("CONSENSUS_META_API_KEY", CANARY),
            ("CONSENSUS_MODEL_TIMEOUT_SECS", "240"),
            ("CONSENSUS_META_REASONING_EFFORT", "low"),
        ]
        .into_iter()
        .collect();
        let meta = MetaConfig::from_vars(|n| meta_vars.get(n).map(|v| v.to_string()))
            .unwrap()
            .unwrap();
        let live_vars: BTreeMap<&'static str, String> = [
            (live_vars::RUN_TOKEN_CAP, "90000"),
            (live_vars::BURST_FRACTION, "0.6"),
            (live_vars::CLIENT_DAILY, "4"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
        let live = LiveConfig::load(crate::live::config::Exposure::Public, &live_vars).unwrap();
        let rt_vars: BTreeMap<String, String> = [
            ("CONSENSUS_RUN_DEADLINE_SECS", "1200"),
            ("CONSENSUS_LIVE_CONCURRENCY", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let rt = RuntimeConfig::from_env(&|n: &str| rt_vars.get(n).cloned()).unwrap();
        (meta, live, rt)
    }

    #[test]
    fn the_boot_line_names_every_effective_limit_and_no_key() {
        let (meta, live, rt) = public_values();
        let limits = CallLimits::of(&meta);
        let grace = public_shutdown_grace(limits.timeout);
        let line = public_limits_line(Some(&limits), &live, &rt, grace);
        for field in [
            "per-call timeout 240 s",
            "connect timeout 10 s",
            "max attempts 3",
            "max_completion_tokens 16000",
            "reasoning_effort set",
            "run deadline 1200 s",
            "live run token cap 90000",
            "shutdown grace 250 s",
            "live concurrency 2 (ledger 1)",
            "burst fraction 0.60 ($6.00 at 00:00 UTC)",
            "client daily 4",
            "judge code daily 5",
        ] {
            assert!(line.contains(field), "{field:?} missing from {line}");
        }
        // Set or unset only: the value of reasoning_effort is not printed.
        assert!(!line.contains("low"), "{line}");
        // No key material, whole or in part (the canary pattern).
        assert!(!line.contains(CANARY), "{line}");
        assert!(!line.contains("LLM_"), "{line}");
        assert!(!line.contains("canary"), "{line}");
        // A 240 s budget holds 16,000 tokens at 80/s: no warning.
        assert_eq!(timeout_warning(&limits, 16_000), None);

        let disabled = public_limits_line(None, &live, &rt, Duration::from_secs(60));
        assert!(disabled.contains("live mode disabled"), "{disabled}");
        assert!(disabled.contains("shutdown grace 60 s"), "{disabled}");
        let unset = CallLimits {
            reasoning_effort_set: false,
            ..limits
        };
        assert!(
            public_limits_line(Some(&unset), &live, &rt, grace).contains("reasoning_effort unset")
        );
    }

    #[test]
    fn a_timeout_too_short_for_the_output_bound_is_a_warning_naming_both() {
        let (mut meta, _, _) = public_values();
        meta.timeout = Duration::from_secs(120);
        let warning = timeout_warning(&CallLimits::of(&meta), 16_000).expect("a warning");
        assert!(warning.contains("WARNING"), "{warning}");
        assert!(
            warning.contains("CONSENSUS_MODEL_TIMEOUT_SECS=120"),
            "{warning}"
        );
        assert!(warning.contains("max_completion_tokens=16000"), "{warning}");
        assert!(warning.contains("200 s"), "{warning}");
        assert!(!warning.contains(CANARY), "{warning}");
    }
}
