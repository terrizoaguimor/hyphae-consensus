// SPDX-License-Identifier: Apache-2.0
//! Configuration of the two servers (`serve-public`, `serve-operator`),
//! read from the environment at startup. Every value is validated here;
//! an invalid one stops the server with the variable's name and the
//! reason, and nothing is silently replaced by a default. Every limit the
//! public server enforces is reported by `GET /api/v1/capabilities`.
//!
//! No secret is read here. The operator token comes from a file
//! (`CONSENSUS_OPERATOR_TOKEN_FILE`) read by the operator server itself,
//! and the Meta key stays with [`crate::config::MetaConfig`], which only
//! the operator surface consults.
//!
//! # Variables
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `CONSENSUS_PUBLIC_SOCKET` | `/run/consensus/public.sock` (unix) | Unix socket `serve-public` listens on (mode 0660). cloudflared reaches it; no local TCP port exists, so nobody else can forge `CF-*` headers. |
//! | `CONSENSUS_PUBLIC_BIND` | unset | Loopback TCP address for plain-HTTP development of the public server instead of the socket, e.g. `127.0.0.1:8788`. A non-loopback address is refused. On Windows, where unix sockets are unavailable, it is required. |
//! | `CONSENSUS_WEB_ROOT` | `apps/web/out` | Directory of the static web export served at `/`. |
//! | `CONSENSUS_CSP_MANIFEST` | `<web root>/csp-manifest.json` | Per-page inline script and style hashes produced by the web build (`apps/web/scripts/csp-manifest.mjs`). A missing file means every page gets the base policy without hashes. |
//! | `CONSENSUS_ALLOWED_ORIGINS` | derived from the bind address in TCP mode; required with the socket | Comma-separated `Origin` values accepted on state-changing requests, e.g. `https://demo.example.org`. |
//! | `CONSENSUS_ALLOWED_HOSTS` | derived from the bind address in TCP mode; required with the socket | Comma-separated `Host` values accepted, e.g. `demo.example.org`. |
//! | `CONSENSUS_TRUST_PROXY_HEADERS` | `true` with the socket, `false` with TCP | Whether `CF-Connecting-IP` names the client. When true, a request without it is refused (ABUSE-2). `false` is refused with the unix socket: a socket has no peer address, so every visitor would share one client key, one rate bucket and one stream cap. |
//! | `CONSENSUS_RUN_STORE_DIR` | `consensus-store` | Root of the per-run store (`runs/`). |
//! | `CONSENSUS_RESEARCH_DIR` | `research` | The read-only research archive served as the showcase. |
//! | `CONSENSUS_SESSION_TTL_SECS` | `86400` | Visitor session lifetime. |
//! | `CONSENSUS_SESSION_MAX` | `50000` | Bound of the server-side session table (LRU eviction). |
//! | `CONSENSUS_GUIDED_CONCURRENCY` | `4` | Guided deliberations running at once. |
//! | `CONSENSUS_LIVE_CONCURRENCY` | `1` | Live (Meta) deliberations running at once. |
//! | `CONSENSUS_RUN_DEADLINE_SECS` | `900` | Wall-clock deadline of one deliberation; the run fails at its next checkpoint once reached. A model call never outlives it: each call's budget is cut to it, and a call with less than 30 s left is not made (code `run_deadline_reached`, nothing drawn or sent; `crate::meta::MIN_CALL_WINDOW`). |
//! | `CONSENSUS_SSE_MAX_PER_RUN` | `8` | Concurrent event streams per run. |
//! | `CONSENSUS_SSE_MAX_PER_CLIENT` | `8` | Concurrent event streams per client key (IPv4 /32 or IPv6 /64). |
//! | `CONSENSUS_SSE_MAX_GLOBAL` | `512` | Concurrent event streams in the process. |
//! | `CONSENSUS_SSE_HEARTBEAT_SECS` | `15` | Interval of the SSE heartbeat comment. |
//! | `CONSENSUS_RATE_SESSION_PER_MIN` | `10` | Sessions one client key may mint per minute. |
//! | `CONSENSUS_RATE_RUNS_PER_MIN` | `6` | Runs one client key may create per minute. |
//! | `CONSENSUS_RATE_SSE_PER_MIN` | `30` | Event streams one client key may open per minute. |
//! | `CONSENSUS_RATE_TABLE_MAX` | `50000` | Bound of each per-client rate table (LRU eviction). |
//! | `CONSENSUS_GUIDED_RING` | `2000` | Visitor runs kept in the store at once, as a ring: when it is full, the oldest resting visitor runs (those past their access window first) are purged to admit a new one, so a visitor may lose a run before its access window ends under heavy load; only when nothing can be freed (every stored visitor run is deliberating or being read) does creation answer `503 storage_full`. Operator runs on a shared store neither count nor are purged. |
//! | `CONSENSUS_VISITOR_ACCESS_HOURS` | `24` | How long a visitor can reach a run (at most; see the ring). |
//! | `CONSENSUS_VISITOR_DELETE_DAYS` | `7` | When a visitor run's files are deleted whole (at the latest; see the ring). |
//! | `CONSENSUS_COOKIE_SECURE` | `true` | `Secure` on the session cookie, named `__Host-hc_session`. `false` is for plain-HTTP development only: the `__Host-` prefix requires `Secure` (a browser drops the cookie otherwise), so the cookie is then named `hc_session`. Chrome and Firefox treat `http://localhost` as a secure context, so `true` works there too. |
//! | `CONSENSUS_REQUEST_TIMEOUT_SECS` | `30` | How long a request may take to be handled, body read included; past it the answer is `408 request_timeout`. A stream's body is not covered once its headers are sent. Header-read timeouts and connection limits are cloudflared's (the socket's only client); `axum::serve` sets no timer, so hyper's own header timeout is inactive. |
//! | `CONSENSUS_OPERATOR_BIND` | `127.0.0.1:8787` | Loopback TCP address of `serve-operator`; non-loopback is refused. The accepted `Host` values are the bound address, `127.0.0.1:<port>` and `localhost:<port>`. |
//! | `CONSENSUS_OPERATOR_TOKEN_FILE` | unset | File holding the operator bearer token (at least 32 characters). Without it a token is generated and printed once. |
//! | `CONSENSUS_MAX_RUN_TOKENS` | runtime default | Per-run token cap (see [`crate::config::run_limits_from_env`]). |
//!
//! The `CONSENSUS_RATE_*` and `CONSENSUS_SSE_MAX_PER_CLIENT` limits are
//! visitor limits. The operator surface keys every request on its loopback
//! peer, so it uses its own generous bounds ([`operator_limits`]): 120 runs
//! and 600 streams per minute, and a per-client stream cap equal to the
//! global one.

use crate::config::parse_max_run_tokens;
use consensus_runtime::RunLimits;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

/// Default unix socket path of the public server.
pub const DEFAULT_PUBLIC_SOCKET: &str = "/run/consensus/public.sock";
/// Default loopback address of the operator server.
pub const DEFAULT_OPERATOR_BIND: &str = "127.0.0.1:8787";
/// Request body limit on every API route, in bytes.
pub const BODY_LIMIT_BYTES: usize = 16 * 1024;
/// The least a server waits at shutdown for its runs to reach a
/// checkpoint.
pub const SHUTDOWN_GRACE_FLOOR: Duration = Duration::from_secs(60);
/// What the public server's shutdown grace adds to one model call's
/// budget.
pub const SHUTDOWN_GRACE_MARGIN: Duration = Duration::from_secs(10);

/// The public server's shutdown grace with live mode enabled: one model
/// call's whole budget plus [`SHUTDOWN_GRACE_MARGIN`], at least
/// [`SHUTDOWN_GRACE_FLOOR`] (250 s at the 240 s public timeout). A call in
/// progress at the stop signal can then be answered and recorded; one
/// still in progress after the grace is abandoned and settled at its
/// estimate, and its reservation released, before the server returns
/// ([`crate::runs::RunManager::shutdown`]). The service's stop timeout
/// must exceed the grace plus [`crate::runs::ABORT_SETTLE_WAIT`] (the
/// unit's `TimeoutStopSec=280` at 250 s).
pub fn public_shutdown_grace(model_timeout: Duration) -> Duration {
    model_timeout
        .saturating_add(SHUTDOWN_GRACE_MARGIN)
        .max(SHUTDOWN_GRACE_FLOOR)
}

/// What the HTTP drain and the process exit may take after the aborted
/// runs have settled.
pub const STOP_DRAIN_MARGIN: Duration = Duration::from_secs(10);

/// The shortest service stop timeout (`TimeoutStopSec`) under which a
/// shutdown with `grace` still settles and releases every call in
/// progress: the grace, [`crate::runs::ABORT_SETTLE_WAIT`] and
/// [`STOP_DRAIN_MARGIN`] (270 s at the 250 s public grace; the unit sets
/// 280 s). A shorter one lets systemd kill the process before the abort
/// runs, and the reservation stays open until the next start's recovery.
/// The boot line prints it.
pub fn required_stop_timeout(grace: Duration) -> Duration {
    grace
        .saturating_add(crate::runs::ABORT_SETTLE_WAIT)
        .saturating_add(STOP_DRAIN_MARGIN)
}

/// Where `serve-public` listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicListener {
    /// A unix socket, the deployment path.
    UnixSocket(PathBuf),
    /// A loopback TCP address, for development only.
    Tcp(SocketAddr),
}

/// Caps on concurrent event streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SseLimits {
    pub max_per_run: usize,
    pub max_per_client: usize,
    pub max_global: usize,
    pub heartbeat: Duration,
}

/// Per-client-key request rates, per minute, and the bound of each table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimits {
    pub session_per_min: u32,
    pub runs_per_min: u32,
    pub sse_per_min: u32,
    pub table_max: usize,
}

/// What both servers share: the store, the archive, the run manager's
/// bounds and the stream caps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub run_store_dir: PathBuf,
    pub research_dir: PathBuf,
    pub guided_concurrency: usize,
    pub live_concurrency: usize,
    pub run_deadline: Duration,
    pub sse: SseLimits,
    pub rates: RateLimits,
    pub max_run_tokens: RunLimits,
    /// How long a request may take to be handled.
    pub request_timeout: Duration,
}

/// The operator surface's own request limits: one operator on a loopback
/// peer, so the visitor rates would only get in the console's way. Sessions
/// are not minted there; the rate stays for the table's shape.
pub fn operator_limits(rt: &RuntimeConfig) -> (RateLimits, SseLimits) {
    (
        RateLimits {
            session_per_min: 60,
            runs_per_min: 120,
            sse_per_min: 600,
            table_max: rt.rates.table_max,
        },
        SseLimits {
            max_per_client: rt.sse.max_global,
            ..rt.sse
        },
    )
}

/// `serve-public` configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicConfig {
    pub listener: PublicListener,
    pub web_root: PathBuf,
    pub csp_manifest: PathBuf,
    pub allowed_origins: Vec<String>,
    pub allowed_hosts: Vec<String>,
    pub trust_proxy_headers: bool,
    pub session_ttl: Duration,
    pub session_max: usize,
    pub guided_ring: usize,
    pub visitor_access: Duration,
    pub visitor_delete: Duration,
    pub cookie_secure: bool,
    pub runtime: RuntimeConfig,
}

/// `serve-operator` configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorConfig {
    pub bind: SocketAddr,
    pub token_file: Option<PathBuf>,
    pub runtime: RuntimeConfig,
}

/// Where a configuration value comes from: the process environment, or a
/// map in tests.
pub trait EnvSource {
    fn get(&self, name: &str) -> Option<String>;
}

/// The process environment.
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl<F: Fn(&str) -> Option<String>> EnvSource for F {
    fn get(&self, name: &str) -> Option<String> {
        self(name)
    }
}

fn setting(env: &dyn EnvSource, name: &str) -> Option<String> {
    env.get(name)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_u64(env: &dyn EnvSource, name: &str, default: u64, min: u64) -> Result<u64, String> {
    match setting(env, name) {
        None => Ok(default),
        Some(v) => v
            .parse::<u64>()
            .ok()
            .filter(|n| *n >= min)
            .ok_or_else(|| format!("{name} must be an integer of at least {min}, got {v:?}")),
    }
}

fn parse_usize(
    env: &dyn EnvSource,
    name: &str,
    default: usize,
    min: usize,
) -> Result<usize, String> {
    parse_u64(env, name, default as u64, min as u64).map(|n| n as usize)
}

fn parse_bool(env: &dyn EnvSource, name: &str, default: bool) -> Result<bool, String> {
    match setting(env, name).as_deref() {
        None => Ok(default),
        Some("true") | Some("1") | Some("yes") => Ok(true),
        Some("false") | Some("0") | Some("no") => Ok(false),
        Some(v) => Err(format!("{name} must be true or false, got {v:?}")),
    }
}

fn parse_list(env: &dyn EnvSource, name: &str) -> Vec<String> {
    setting(env, name)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn parse_loopback(name: &str, value: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = value
        .parse()
        .map_err(|_| format!("{name} must be an ip:port address, got {value:?}"))?;
    if !addr.ip().is_loopback() {
        return Err(format!(
            "{name} must be a loopback address (127.0.0.1 or ::1): {value:?} would expose the listener directly; the public server is reached only through its unix socket and the operator server only over SSH"
        ));
    }
    Ok(addr)
}

/// `Host` values and `Origin` values a loopback listener answers for: the
/// bound address as a browser writes it (`[::1]:port` for IPv6), plus
/// `127.0.0.1:port` and `localhost:port`.
fn loopback_names(addr: &SocketAddr) -> (Vec<String>, Vec<String>) {
    let port = addr.port();
    let bound = match addr.ip() {
        IpAddr::V4(ip) => format!("{ip}:{port}"),
        IpAddr::V6(ip) => format!("[{ip}]:{port}"),
    };
    let mut hosts = vec![bound];
    for name in [format!("127.0.0.1:{port}"), format!("localhost:{port}")] {
        if !hosts.contains(&name) {
            hosts.push(name);
        }
    }
    let origins = hosts.iter().map(|h| format!("http://{h}")).collect();
    (hosts, origins)
}

fn validate_origin(name: &str, origin: &str) -> Result<(), String> {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .ok_or_else(|| format!("{name}: an origin is scheme://host[:port], got {origin:?}"))?;
    if rest.is_empty() || rest.contains('/') || rest.contains(' ') {
        return Err(format!("{name}: an origin has no path, got {origin:?}"));
    }
    Ok(())
}

fn validate_host(name: &str, host: &str) -> Result<(), String> {
    if host.contains('/') || host.contains(' ') || host.contains("://") {
        return Err(format!(
            "{name}: a host is host[:port] without a scheme or path, got {host:?}"
        ));
    }
    Ok(())
}

impl RuntimeConfig {
    pub fn from_env(env: &dyn EnvSource) -> Result<Self, String> {
        let max_run_tokens = match env.get("CONSENSUS_MAX_RUN_TOKENS") {
            None => parse_max_run_tokens(None)?,
            Some(v) => parse_max_run_tokens(Some(&v))?,
        };
        Ok(Self {
            run_store_dir: PathBuf::from(
                setting(env, "CONSENSUS_RUN_STORE_DIR").unwrap_or_else(|| "consensus-store".into()),
            ),
            research_dir: PathBuf::from(
                setting(env, "CONSENSUS_RESEARCH_DIR").unwrap_or_else(|| "research".into()),
            ),
            guided_concurrency: parse_usize(env, "CONSENSUS_GUIDED_CONCURRENCY", 4, 1)?,
            live_concurrency: parse_usize(env, "CONSENSUS_LIVE_CONCURRENCY", 1, 1)?,
            run_deadline: Duration::from_secs(parse_u64(
                env,
                "CONSENSUS_RUN_DEADLINE_SECS",
                900,
                1,
            )?),
            sse: SseLimits {
                max_per_run: parse_usize(env, "CONSENSUS_SSE_MAX_PER_RUN", 8, 1)?,
                max_per_client: parse_usize(env, "CONSENSUS_SSE_MAX_PER_CLIENT", 8, 1)?,
                max_global: parse_usize(env, "CONSENSUS_SSE_MAX_GLOBAL", 512, 1)?,
                heartbeat: Duration::from_secs(parse_u64(
                    env,
                    "CONSENSUS_SSE_HEARTBEAT_SECS",
                    15,
                    1,
                )?),
            },
            rates: RateLimits {
                session_per_min: parse_u64(env, "CONSENSUS_RATE_SESSION_PER_MIN", 10, 1)? as u32,
                runs_per_min: parse_u64(env, "CONSENSUS_RATE_RUNS_PER_MIN", 6, 1)? as u32,
                sse_per_min: parse_u64(env, "CONSENSUS_RATE_SSE_PER_MIN", 30, 1)? as u32,
                table_max: parse_usize(env, "CONSENSUS_RATE_TABLE_MAX", 50_000, 16)?,
            },
            max_run_tokens,
            request_timeout: Duration::from_secs(parse_u64(
                env,
                "CONSENSUS_REQUEST_TIMEOUT_SECS",
                30,
                1,
            )?),
        })
    }
}

impl PublicConfig {
    /// Reads the public server's configuration; see the module docs for
    /// every variable. Any invalid value is an error naming the variable.
    pub fn from_env(env: &dyn EnvSource) -> Result<Self, String> {
        let runtime = RuntimeConfig::from_env(env)?;
        let bind = setting(env, "CONSENSUS_PUBLIC_BIND");
        let socket = setting(env, "CONSENSUS_PUBLIC_SOCKET");
        let listener = match (&bind, cfg!(unix)) {
            (Some(bind), _) => PublicListener::Tcp(parse_loopback("CONSENSUS_PUBLIC_BIND", bind)?),
            (None, true) => PublicListener::UnixSocket(PathBuf::from(
                socket.unwrap_or_else(|| DEFAULT_PUBLIC_SOCKET.into()),
            )),
            (None, false) => {
                return Err(
                    "CONSENSUS_PUBLIC_BIND is required on this platform: unix sockets are unavailable, so the public server can only listen on a loopback TCP address for development"
                        .into(),
                )
            }
        };
        let trust_default = matches!(listener, PublicListener::UnixSocket(_));
        let trust_proxy_headers = parse_bool(env, "CONSENSUS_TRUST_PROXY_HEADERS", trust_default)?;
        if trust_default && !trust_proxy_headers {
            return Err(
                "CONSENSUS_TRUST_PROXY_HEADERS=false is refused with the unix socket: a socket connection has no peer address, so every visitor would share one client key (one rate bucket, one stream cap); keep it true behind cloudflared, or set CONSENSUS_PUBLIC_BIND for development"
                    .into(),
            );
        }

        let mut allowed_origins = parse_list(env, "CONSENSUS_ALLOWED_ORIGINS");
        let mut allowed_hosts = parse_list(env, "CONSENSUS_ALLOWED_HOSTS");
        if let PublicListener::Tcp(addr) = &listener {
            let (hosts, origins) = loopback_names(addr);
            if allowed_hosts.is_empty() {
                allowed_hosts = hosts;
            }
            if allowed_origins.is_empty() {
                allowed_origins = origins;
            }
        }
        if allowed_origins.is_empty() {
            return Err(
                "CONSENSUS_ALLOWED_ORIGINS is required with the unix socket: the public hostname's origin, e.g. https://demo.example.org"
                    .into(),
            );
        }
        if allowed_hosts.is_empty() {
            return Err(
                "CONSENSUS_ALLOWED_HOSTS is required with the unix socket: the public hostname, e.g. demo.example.org"
                    .into(),
            );
        }
        for origin in &allowed_origins {
            validate_origin("CONSENSUS_ALLOWED_ORIGINS", origin)?;
        }
        for host in &allowed_hosts {
            validate_host("CONSENSUS_ALLOWED_HOSTS", host)?;
        }

        let web_root = PathBuf::from(
            setting(env, "CONSENSUS_WEB_ROOT").unwrap_or_else(|| "apps/web/out".into()),
        );
        let csp_manifest = setting(env, "CONSENSUS_CSP_MANIFEST")
            .map(PathBuf::from)
            .unwrap_or_else(|| web_root.join("csp-manifest.json"));

        let access_hours = parse_u64(env, "CONSENSUS_VISITOR_ACCESS_HOURS", 24, 1)?;
        let delete_days = parse_u64(env, "CONSENSUS_VISITOR_DELETE_DAYS", 7, 1)?;
        let visitor_access = Duration::from_secs(access_hours * 3600);
        let visitor_delete = Duration::from_secs(delete_days * 24 * 3600);
        if visitor_access > visitor_delete {
            return Err(format!(
                "CONSENSUS_VISITOR_ACCESS_HOURS ({access_hours} h) may not outlast CONSENSUS_VISITOR_DELETE_DAYS ({delete_days} d)"
            ));
        }

        Ok(Self {
            listener,
            web_root,
            csp_manifest,
            allowed_origins,
            allowed_hosts,
            trust_proxy_headers,
            session_ttl: Duration::from_secs(parse_u64(
                env,
                "CONSENSUS_SESSION_TTL_SECS",
                86_400,
                60,
            )?),
            session_max: parse_usize(env, "CONSENSUS_SESSION_MAX", 50_000, 16)?,
            guided_ring: parse_usize(env, "CONSENSUS_GUIDED_RING", 2000, 1)?,
            visitor_access,
            visitor_delete,
            cookie_secure: parse_bool(env, "CONSENSUS_COOKIE_SECURE", true)?,
            runtime,
        })
    }
}

impl OperatorConfig {
    /// Reads the operator server's configuration. The bind address must
    /// be loopback.
    pub fn from_env(env: &dyn EnvSource) -> Result<Self, String> {
        let runtime = RuntimeConfig::from_env(env)?;
        let bind =
            setting(env, "CONSENSUS_OPERATOR_BIND").unwrap_or_else(|| DEFAULT_OPERATOR_BIND.into());
        let bind = parse_loopback("CONSENSUS_OPERATOR_BIND", &bind)?;
        Ok(Self {
            bind,
            token_file: setting(env, "CONSENSUS_OPERATOR_TOKEN_FILE").map(PathBuf::from),
            runtime,
        })
    }

    /// `Host` and `Origin` values the operator server accepts: the bound
    /// address, `127.0.0.1:<port>` and `localhost:<port>`.
    pub fn allowed_names(&self) -> (Vec<String>, Vec<String>) {
        loopback_names(&self.bind)
    }
}

/// True for an address a loopback listener may bind.
pub fn is_loopback(ip: IpAddr) -> bool {
    ip.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn lookup(map: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        move |name| map.get(name).cloned()
    }

    #[test]
    fn tcp_dev_mode_derives_hosts_and_origins_and_trusts_no_proxy() {
        let map = env(&[("CONSENSUS_PUBLIC_BIND", "127.0.0.1:8788")]);
        let cfg = PublicConfig::from_env(&lookup(&map)).unwrap();
        assert_eq!(
            cfg.listener,
            PublicListener::Tcp("127.0.0.1:8788".parse().unwrap())
        );
        assert_eq!(cfg.allowed_hosts, ["127.0.0.1:8788", "localhost:8788"]);
        assert_eq!(
            cfg.allowed_origins,
            ["http://127.0.0.1:8788", "http://localhost:8788"]
        );
        assert!(!cfg.trust_proxy_headers);
        assert!(cfg.cookie_secure);
        assert_eq!(cfg.guided_ring, 2000);
        assert_eq!(cfg.runtime.sse.heartbeat, Duration::from_secs(15));
        assert_eq!(cfg.runtime.request_timeout, Duration::from_secs(30));
        let map = env(&[
            ("CONSENSUS_PUBLIC_BIND", "127.0.0.1:8788"),
            ("CONSENSUS_REQUEST_TIMEOUT_SECS", "5"),
        ]);
        let cfg = PublicConfig::from_env(&lookup(&map)).unwrap();
        assert_eq!(cfg.runtime.request_timeout, Duration::from_secs(5));
    }

    #[test]
    fn loopback_names_include_the_bound_address_bracketed_for_ipv6() {
        let (hosts, origins) = loopback_names(&"[::1]:8787".parse().unwrap());
        assert_eq!(hosts, ["[::1]:8787", "127.0.0.1:8787", "localhost:8787"]);
        assert_eq!(origins[0], "http://[::1]:8787");
        let (hosts, _) = loopback_names(&"127.0.0.1:8788".parse().unwrap());
        assert_eq!(hosts, ["127.0.0.1:8788", "localhost:8788"], "no duplicate");
        let map = env(&[("CONSENSUS_OPERATOR_BIND", "[::1]:8787")]);
        let cfg = OperatorConfig::from_env(&lookup(&map)).unwrap();
        assert!(cfg.allowed_names().0.contains(&"[::1]:8787".to_string()));
    }

    #[test]
    fn the_operator_surface_has_its_own_generous_limits() {
        let map = env(&[("CONSENSUS_SSE_MAX_GLOBAL", "64")]);
        let cfg = OperatorConfig::from_env(&lookup(&map)).unwrap();
        let (rates, sse) = operator_limits(&cfg.runtime);
        assert_eq!(rates.runs_per_min, 120);
        assert_eq!(rates.sse_per_min, 600);
        assert_eq!(sse.max_per_client, 64);
        assert_eq!(sse.max_global, 64);
        assert_eq!(rates.table_max, cfg.runtime.rates.table_max);
    }

    #[test]
    fn non_loopback_public_bind_is_refused() {
        let map = env(&[("CONSENSUS_PUBLIC_BIND", "0.0.0.0:8788")]);
        let err = PublicConfig::from_env(&lookup(&map)).unwrap_err();
        assert!(
            err.contains("CONSENSUS_PUBLIC_BIND") && err.contains("loopback"),
            "{err}"
        );
        let map = env(&[("CONSENSUS_OPERATOR_BIND", "192.168.1.5:8787")]);
        let err = OperatorConfig::from_env(&lookup(&map)).unwrap_err();
        assert!(err.contains("CONSENSUS_OPERATOR_BIND"), "{err}");
    }

    #[test]
    fn invalid_values_name_the_variable() {
        for (name, value) in [
            ("CONSENSUS_SESSION_TTL_SECS", "soon"),
            ("CONSENSUS_SESSION_MAX", "0"),
            ("CONSENSUS_GUIDED_CONCURRENCY", "0"),
            ("CONSENSUS_SSE_MAX_GLOBAL", "-1"),
            ("CONSENSUS_COOKIE_SECURE", "maybe"),
            ("CONSENSUS_TRUST_PROXY_HEADERS", "2"),
            ("CONSENSUS_RATE_RUNS_PER_MIN", "many"),
            ("CONSENSUS_REQUEST_TIMEOUT_SECS", "0"),
            ("CONSENSUS_MAX_RUN_TOKENS", "0"),
            ("CONSENSUS_ALLOWED_ORIGINS", "demo.example.org"),
            ("CONSENSUS_ALLOWED_HOSTS", "https://demo.example.org"),
        ] {
            let map = env(&[("CONSENSUS_PUBLIC_BIND", "127.0.0.1:8788"), (name, value)]);
            let err = PublicConfig::from_env(&lookup(&map)).unwrap_err();
            assert!(err.contains(name), "{name}: {err}");
        }
        let map = env(&[
            ("CONSENSUS_PUBLIC_BIND", "127.0.0.1:8788"),
            ("CONSENSUS_VISITOR_ACCESS_HOURS", "200"),
            ("CONSENSUS_VISITOR_DELETE_DAYS", "1"),
        ]);
        let err = PublicConfig::from_env(&lookup(&map)).unwrap_err();
        assert!(err.contains("CONSENSUS_VISITOR_ACCESS_HOURS"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn socket_mode_requires_explicit_origins_and_hosts() {
        let map = env(&[]);
        let err = PublicConfig::from_env(&lookup(&map)).unwrap_err();
        assert!(err.contains("CONSENSUS_ALLOWED_ORIGINS"), "{err}");
        let map = env(&[
            ("CONSENSUS_ALLOWED_ORIGINS", "https://demo.example.org"),
            ("CONSENSUS_ALLOWED_HOSTS", "demo.example.org"),
        ]);
        let cfg = PublicConfig::from_env(&lookup(&map)).unwrap();
        assert_eq!(
            cfg.listener,
            PublicListener::UnixSocket(PathBuf::from(DEFAULT_PUBLIC_SOCKET))
        );
        assert!(cfg.trust_proxy_headers);
        // A socket has no peer address: refusing the proxy header would
        // key every visitor alike, so that combination is refused.
        let map = env(&[
            ("CONSENSUS_ALLOWED_ORIGINS", "https://demo.example.org"),
            ("CONSENSUS_ALLOWED_HOSTS", "demo.example.org"),
            ("CONSENSUS_TRUST_PROXY_HEADERS", "false"),
        ]);
        let err = PublicConfig::from_env(&lookup(&map)).unwrap_err();
        assert!(err.contains("CONSENSUS_TRUST_PROXY_HEADERS"), "{err}");
    }

    #[cfg(not(unix))]
    #[test]
    fn without_unix_sockets_the_public_bind_is_required() {
        let map = env(&[]);
        let err = PublicConfig::from_env(&lookup(&map)).unwrap_err();
        assert!(err.contains("CONSENSUS_PUBLIC_BIND"), "{err}");
    }

    #[test]
    fn operator_defaults_to_loopback_8787() {
        let map = env(&[]);
        let cfg = OperatorConfig::from_env(&lookup(&map)).unwrap();
        assert_eq!(cfg.bind, DEFAULT_OPERATOR_BIND.parse().unwrap());
        let (hosts, origins) = cfg.allowed_names();
        assert_eq!(hosts, ["127.0.0.1:8787", "localhost:8787"]);
        assert_eq!(origins, ["http://127.0.0.1:8787", "http://localhost:8787"]);
        assert!(cfg.token_file.is_none());
    }

    #[test]
    fn the_public_shutdown_grace_is_one_call_budget_plus_ten_seconds() {
        let secs = Duration::from_secs;
        // The public timeout: 240 s, so a 250 s grace (TimeoutStopSec=280).
        assert_eq!(public_shutdown_grace(secs(240)), secs(250));
        assert_eq!(
            public_shutdown_grace(secs(240)),
            secs(240) + SHUTDOWN_GRACE_MARGIN
        );
        assert_eq!(public_shutdown_grace(secs(120)), secs(130));
        // Never below the 60 s floor the servers had.
        assert_eq!(public_shutdown_grace(secs(1)), secs(60));
        assert_eq!(public_shutdown_grace(secs(50)), secs(60));
        assert_eq!(public_shutdown_grace(secs(51)), secs(61));
        // Grace, the abort's settle wait and a margin fit the unit's 280 s.
        assert!(public_shutdown_grace(secs(240)) + crate::runs::ABORT_SETTLE_WAIT < secs(280));
        // The stop timeout the boot line asks for: 270 s at 240 s, inside
        // the unit's 280 s; the VPS's current 90 s is too short even at
        // the default 120 s budget (130 s grace, 150 s needed).
        let needed = required_stop_timeout(public_shutdown_grace(secs(240)));
        assert_eq!(needed, secs(270));
        assert!(needed < secs(280));
        assert_eq!(
            required_stop_timeout(public_shutdown_grace(secs(120))),
            secs(150)
        );
        assert!(required_stop_timeout(public_shutdown_grace(secs(120))) > secs(90));
    }
}
