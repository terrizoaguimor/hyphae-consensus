// SPDX-License-Identifier: Apache-2.0
//! The `/api/v1` HTTP surface shared by `serve-public` and
//! `serve-operator`: application state, request security middleware
//! (Host, CSRF, security headers, body limit, bearer token on the operator
//! surface), the identity and client-key extractors, and the routers.
//!
//! Both servers run the same handlers ([`v1`]) over the same
//! [`RunManager`]; what differs is the [`ServeConfig`]: who the caller is
//! (a session cookie or the operator token), what the views show, which
//! routes exist, and which authority a decision carries.

pub mod contract;
pub mod error;
pub mod types;
pub mod v1;

use crate::limits::{ClientKey, ClientKeyPolicy, RateTable, StreamCaps};
use crate::public_config::{RateLimits, SseLimits, BODY_LIMIT_BYTES};
use crate::public_view::Surface;
use crate::runs::{Deliberation, RunManager};
use crate::session::{Session, SessionStore};
use crate::showcase::Showcase;
use crate::static_files::StaticSite;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use consensus_domain::Authority;
use consensus_runtime::{Owner, RunLimits, TaskInput};
use error::{ApiError, ErrorCode};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use types::PresetId;

/// Everything the middleware, the extractors and `/capabilities` need to
/// know about the server they run in.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub surface: Surface,
    /// `Origin` values accepted on state-changing requests.
    pub allowed_origins: Vec<String>,
    /// `Host` values accepted on every request.
    pub allowed_hosts: Vec<String>,
    /// Whether a state-changing request must carry an `Origin` (public:
    /// yes; operator: only checked when present, so scripts work).
    pub origin_required: bool,
    pub client_key_policy: ClientKeyPolicy,
    pub cookie_secure: bool,
    pub sse: SseLimits,
    pub rates: RateLimits,
    pub session_ttl: Duration,
    pub session_max: usize,
    pub guided_ring: usize,
    pub visitor_access: Duration,
    pub visitor_delete: Duration,
    pub run_limits: RunLimits,
    pub guided_concurrency: usize,
    pub live_concurrency: usize,
    pub run_deadline: Duration,
    /// Build identity reported by `/health`.
    pub version: String,
}

/// The bearer token of the operator surface, compared in constant time.
pub struct OperatorAuth {
    token: Vec<u8>,
}

impl OperatorAuth {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.as_bytes().to_vec(),
        }
    }

    /// True when `presented` equals the token, in time independent of
    /// where they differ.
    pub fn accepts(&self, presented: &[u8]) -> bool {
        constant_time_eq(&self.token, presented)
    }
}

/// Constant-time byte comparison (length included in the result, not in
/// the timing of the loop).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// The operator console page and its hash-based CSP.
pub struct ConsolePage {
    pub html: String,
    pub csp: String,
}

/// What a live-run request offers the admission layer.
pub struct LiveAdmissionRequest<'a> {
    pub preset: &'a PresetId,
    pub client: ClientKey,
    pub session_pseudonym: &'a str,
    pub headers: &'a HeaderMap,
}

/// An admitted live run: its canned task and the ports to deliberate it.
pub struct LiveGrant {
    pub input: TaskInput,
    pub deliberation: Box<dyn Deliberation>,
}

/// Extension point for the public live mode (plan B9, lane S-b2):
/// Turnstile, the USD ledger, presets and the geographic policy decide
/// whether a visitor may spend the operator's key. Until an
/// implementation is wired into [`AppState::live_gate`], the public live
/// route answers `503 live_disabled`.
pub trait LiveGate: Send + Sync {
    fn admit(&self, request: LiveAdmissionRequest<'_>) -> Result<LiveGrant, ApiError>;
}

/// Per-client request rate tables.
pub struct RateTables {
    pub session: RateTable,
    pub runs: RateTable,
    pub sse: RateTable,
}

impl RateTables {
    pub fn new(rates: &RateLimits) -> Self {
        let minute = Duration::from_secs(60);
        Self {
            session: RateTable::new(rates.session_per_min, minute, rates.table_max),
            runs: RateTable::new(rates.runs_per_min, minute, rates.table_max),
            sse: RateTable::new(rates.sse_per_min, minute, rates.table_max),
        }
    }
}

/// Shared state of one server.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServeConfig>,
    pub manager: Arc<RunManager>,
    pub sessions: Arc<SessionStore>,
    pub rates: Arc<RateTables>,
    pub stream_caps: Arc<StreamCaps>,
    pub showcase: Arc<Showcase>,
    pub static_site: Option<Arc<StaticSite>>,
    pub operator_auth: Option<Arc<OperatorAuth>>,
    pub live_gate: Option<Arc<dyn LiveGate>>,
    pub console: Option<Arc<ConsolePage>>,
    pub contract_sha256: Arc<String>,
}

impl AppState {
    pub fn surface(&self) -> Surface {
        self.config.surface
    }
}

/// Who the caller is, on either surface.
#[derive(Debug, Clone)]
pub struct Identity {
    pub owner: Owner,
    /// Recorded as `authorized_by` on decisions: `operator`, or the
    /// session's pseudonym. Never taken from the body (LC-11).
    pub authorized_by: String,
    pub authority: Authority,
    pub session: Option<Session>,
}

impl FromRequestParts<AppState> for Identity {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        match state.config.surface {
            // The bearer middleware already admitted the request.
            Surface::Operator => Ok(Identity {
                owner: Owner::Operator,
                authorized_by: "operator".into(),
                authority: Authority::Operator,
                session: None,
            }),
            Surface::Public => {
                let cookie = parts
                    .headers
                    .get(header::COOKIE)
                    .and_then(|v| v.to_str().ok());
                let session = state.sessions.from_cookie_header(cookie).map_err(|_| {
                    ApiError::new(
                        ErrorCode::SessionRequired,
                        "a session is required: POST /api/v1/session first",
                    )
                })?;
                Ok(Identity {
                    owner: session.owner(),
                    authorized_by: session.pseudonym(),
                    authority: Authority::PublicDemo,
                    session: Some(session),
                })
            }
        }
    }
}

/// The caller's client key for limits.
#[derive(Debug, Clone, Copy)]
pub struct Client(pub ClientKey);

impl FromRequestParts<AppState> for Client {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        let header = parts
            .headers
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok());
        crate::limits::client_key(state.config.client_key_policy, header, peer)
            .map(Client)
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::ClientUnidentified,
                    "the request carries no CF-Connecting-IP header; this listener serves cloudflared only",
                )
            })
    }
}

/// `Json<T>` whose rejections are [`ApiError`]s: 415 for the content
/// type, 413 for the body limit, 422 for anything the body gets wrong.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(ApiJson(value)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::new(
                ErrorCode::UnsupportedMediaType,
                "Content-Type must be application/json",
            )),
            Err(JsonRejection::BytesRejection(e)) => {
                let text = e.to_string();
                if text.to_ascii_lowercase().contains("length limit") {
                    Err(ApiError::new(
                        ErrorCode::PayloadTooLarge,
                        format!("the request body may have at most {BODY_LIMIT_BYTES} bytes"),
                    ))
                } else {
                    Err(ApiError::invalid("the request body could not be read"))
                }
            }
            Err(e) => Err(ApiError::invalid(bounded_detail(&e.body_text()))),
        }
    }
}

/// The first 200 characters of a deserialization message: it names the
/// field and the problem, never the body.
fn bounded_detail(text: &str) -> String {
    let short: String = text.chars().take(200).collect();
    format!("invalid request: {short}")
}

fn header_eq_ignore_case(value: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|a| a.eq_ignore_ascii_case(value.trim()))
}

fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// Security headers on every response; `Cache-Control: no-store` on
/// `/api` and on the console.
async fn security_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    let set = |headers: &mut HeaderMap, name: header::HeaderName, value: &'static str| {
        headers.insert(name, HeaderValue::from_static(value));
    };
    set(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(headers, header::REFERRER_POLICY, "no-referrer");
    set(headers, header::X_FRAME_OPTIONS, "DENY");
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    if is_api_path(&path) {
        set(headers, header::CACHE_CONTROL, "no-store");
    }
    response
}

/// `Host` must be one this server answers for.
async fn host_check(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    match host {
        Some(h) if header_eq_ignore_case(&h, &state.config.allowed_hosts) => next.run(req).await,
        _ => ApiError::new(
            ErrorCode::HostNotAllowed,
            "the Host header is not one this server answers for",
        )
        .into_response(),
    }
}

/// Paths whose POST carries no body, so no JSON content type is required.
fn bodyless_post(path: &str) -> bool {
    path == "/api/v1/session" || path.ends_with("/cancel")
}

/// CSRF rules for state-changing `/api` requests: `Origin` allowed (and
/// required on the public surface), `Sec-Fetch-Site` absent or
/// `same-origin`, `Content-Type: application/json` where a body is read.
async fn csrf_check(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let safe = matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
    if safe || !is_api_path(&path) {
        return next.run(req).await;
    }
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    match &origin {
        Some(o) if header_eq_ignore_case(o, &state.config.allowed_origins) => {}
        Some(_) => {
            return ApiError::new(ErrorCode::CsrfRejected, "the Origin header is not allowed")
                .into_response()
        }
        None if state.config.origin_required => {
            return ApiError::new(
                ErrorCode::CsrfRejected,
                "state-changing requests must carry an allowed Origin header",
            )
            .into_response()
        }
        None => {}
    }
    if let Some(site) = req
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
    {
        if !site.eq_ignore_ascii_case("same-origin") {
            return ApiError::new(
                ErrorCode::CsrfRejected,
                "cross-site requests are not accepted",
            )
            .into_response();
        }
    }
    if matches!(method, Method::POST | Method::PUT | Method::PATCH) && !bodyless_post(&path) {
        let json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                let mime = v.split(';').next().unwrap_or("").trim();
                mime.eq_ignore_ascii_case("application/json")
            })
            .unwrap_or(false);
        if !json {
            return ApiError::new(
                ErrorCode::UnsupportedMediaType,
                "Content-Type must be application/json",
            )
            .into_response();
        }
    }
    next.run(req).await
}

/// Operator surface: every `/api` request needs the bearer token.
async fn operator_bearer(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !is_api_path(req.uri().path()) {
        return next.run(req).await;
    }
    let Some(auth) = &state.operator_auth else {
        return ApiError::new(ErrorCode::Unauthorized, "operator token not configured")
            .into_response();
    };
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    match presented {
        Some(token) if auth.accepts(token.as_bytes()) => next.run(req).await,
        _ => {
            let mut response = ApiError::new(
                ErrorCode::Unauthorized,
                "a valid Authorization: Bearer <operator token> header is required",
            )
            .into_response();
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"consensus-operator\""),
            );
            response
        }
    }
}

async fn method_not_allowed() -> Response {
    ApiError::new(ErrorCode::MethodNotAllowed, "method not allowed").into_response()
}

/// Routes both surfaces share.
fn common_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/scenarios", get(v1::scenarios))
        .route("/api/v1/capabilities", get(v1::capabilities))
        .route("/api/v1/health", get(v1::health))
        .route("/api/v1/ready", get(v1::ready))
        .route("/api/v1/runs", get(v1::list_runs))
        .route("/api/v1/runs/{id}", get(v1::get_run))
        .route("/api/v1/runs/{id}/events", get(v1::run_events))
        .route(
            "/api/v1/runs/{id}/events.ndjson",
            get(v1::run_events_ndjson),
        )
        .route("/api/v1/runs/{id}/receipt", get(v1::run_receipt))
        .route("/api/v1/runs/{id}/decision", post(v1::decide))
        .route("/api/v1/runs/{id}/cancel", post(v1::cancel))
        .route("/api/v1/showcase", get(v1::showcase_list))
        .route("/api/v1/showcase/{run_id}", get(v1::showcase_detail))
        .route(
            "/api/v1/showcase/{run_id}/receipt",
            get(v1::showcase_receipt),
        )
        .route("/research/{*path}", get(v1::research_file))
}

/// The public server's router: sessions, guided and live runs, the
/// static export, and a JSON 404 for unknown `/api` paths.
pub fn public_router(state: AppState) -> Router {
    let api = common_routes()
        .route(
            "/api/v1/session",
            post(v1::create_session)
                .get(v1::get_session)
                .delete(v1::delete_session),
        )
        .route("/api/v1/runs", post(v1::create_guided_run))
        .route("/api/v1/live-runs", post(v1::create_live_run));
    finish(api.fallback(v1::public_fallback), state)
}

/// The operator server's router: the console at `/`, every common route
/// plus `POST /api/v1/operator/runs`, all `/api` behind the bearer token.
pub fn operator_router(state: AppState) -> Router {
    // The fallback is attached before the bearer layer, so an unknown
    // `/api` path is refused with 401 like every other one.
    let api = common_routes()
        .route("/", get(v1::console))
        .route("/api/v1/operator/runs", post(v1::create_operator_run))
        .fallback(v1::operator_fallback)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            operator_bearer,
        ));
    finish(api, state)
}

fn finish(router: Router<AppState>, state: AppState) -> Router {
    router
        .method_not_allowed_fallback(method_not_allowed)
        .layer(middleware::from_fn_with_state(state.clone(), csrf_check))
        .layer(middleware::from_fn_with_state(state.clone(), host_check))
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_compares_whole_values() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
        let auth = OperatorAuth::new("secret-token");
        assert!(auth.accepts(b"secret-token"));
        assert!(!auth.accepts(b"secret-tokeN"));
    }

    #[test]
    fn api_paths_and_bodyless_posts_are_recognised() {
        assert!(is_api_path("/api/v1/runs"));
        assert!(!is_api_path("/apix"));
        assert!(bodyless_post("/api/v1/session"));
        assert!(bodyless_post("/api/v1/runs/run-x/cancel"));
        assert!(!bodyless_post("/api/v1/runs"));
    }
}
