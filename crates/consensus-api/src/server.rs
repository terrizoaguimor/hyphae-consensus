// SPDX-License-Identifier: Apache-2.0
//! The two servers (C7): `serve-public` and `serve-operator`.
//!
//! - **`serve-public`** listens on a unix socket (mode 0660) that only
//!   cloudflared reaches, or, for development, on a loopback TCP address
//!   (`CONSENSUS_PUBLIC_BIND`; anything else is refused). It serves the
//!   static web export with per-page CSP, `/api/v1/*` for anonymous
//!   cookie sessions, `/research/*`, and a JSON 404 for unknown `/api`
//!   paths. Visitors send no free text: guided runs are chosen by name,
//!   live runs by preset (answered `503 live_disabled` until the
//!   admission layer is wired). Decisions carry `public_demo` authority.
//! - **`serve-operator`** listens on loopback only. Every `/api` request
//!   needs `Authorization: Bearer <token>` (constant-time compare; the
//!   token comes from `CONSENSUS_OPERATOR_TOKEN_FILE` or is generated and
//!   printed once), `Host` must be `127.0.0.1:<port>` or
//!   `localhost:<port>`, and a foreign `Origin` is rejected. It keeps the
//!   operator capabilities (`model=fake|meta`, `memory=fake|offline|hyphae`
//!   with a caller-chosen project) through the same run manager, store and
//!   streams; its decisions carry `operator` authority and its views keep
//!   the raw failure reasons.
//!
//! Both answer `202` on creation and stream the deliberation over SSE;
//! the old synchronous `POST /api/runs` no longer exists on either.

use crate::api::contract;
use crate::api::{
    operator_router, public_router, AppState, ConsolePage, LiveGate, OperatorAuth, RateTables,
    ServeConfig,
};
use crate::limits::{ClientKeyPolicy, StreamCaps};
use crate::public_config::{
    OperatorConfig, ProcessEnv, PublicConfig, PublicListener, RuntimeConfig, DEFAULT_OPERATOR_BIND,
};
use crate::public_view::Surface;
use crate::runs::{ManagerSettings, RunManager};
use crate::session::{random_hex_32, SessionStore};
use crate::showcase::Showcase;
use crate::static_files::{csp_hash, inline_bodies, StaticSite};
use crate::stream::Broadcasts;
use axum::Router;
use consensus_runtime::{RecoveryReport, RunStore};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
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
    let config = ServeConfig {
        surface: Surface::Operator,
        allowed_origins: origins,
        allowed_hosts: hosts,
        origin_required: false,
        client_key_policy: ClientKeyPolicy::PeerAddress,
        cookie_secure: false,
        sse: cfg.runtime.sse,
        rates: cfg.runtime.rates,
        session_ttl: Duration::from_secs(0),
        session_max: 0,
        guided_ring: 0,
        visitor_access: Duration::from_secs(0),
        visitor_delete: Duration::from_secs(0),
        run_limits: cfg.runtime.max_run_tokens,
        guided_concurrency: cfg.runtime.guided_concurrency,
        live_concurrency: cfg.runtime.live_concurrency,
        run_deadline: cfg.runtime.run_deadline,
        version: VERSION.into(),
    };
    let state = AppState {
        sessions: Arc::new(SessionStore::new(Duration::from_secs(60), 1, false)),
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
        static_site: None,
        operator_auth: Some(Arc::new(OperatorAuth::new(token))),
        live_gate: None,
        console: Some(Arc::new(console_page())),
        contract_sha256: Arc::new(contract::contract_sha256()),
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

/// `serve-public`.
pub async fn serve_public() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = PublicConfig::from_env(&ProcessEnv)?;
    let built = build_public(&cfg, BuildOptions::default())?;
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

/// `serve-operator`.
pub async fn serve_operator() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = OperatorConfig::from_env(&ProcessEnv)?;
    let (token, generated) = operator_token(&cfg)?;
    let built = build_operator(&cfg, &token, BuildOptions::default())?;
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
