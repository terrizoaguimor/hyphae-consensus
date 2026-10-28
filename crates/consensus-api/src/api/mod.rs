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

use crate::limits::{ClientKey, ClientKeyPolicy, LruMap, RateTable, StreamCaps};
use crate::live::geo::{GeoDecision, GeoPolicy, Surface as GeoSurface};
use crate::public_config::{RateLimits, SseLimits, BODY_LIMIT_BYTES};
use crate::public_view::Surface;
use crate::runs::{Deliberation, RunManager};
use crate::session::{Session, SessionStore};
use crate::showcase::Showcase;
use crate::static_files::StaticSite;
use axum::error_handling::HandleErrorLayer;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{BoxError, Router};
use consensus_domain::{Authority, RunId};
use consensus_runtime::run_store::StoreHealth;
use consensus_runtime::{Owner, RunLimits, TaskInput};
use error::{ApiError, ErrorCode};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower::timeout::TimeoutLayer;
use tower::ServiceBuilder;
use types::{LiveCapabilitiesV1, LiveReadyV1, PresetId};

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
    /// How long a request may take to be handled (`408 request_timeout`
    /// past it).
    pub request_timeout: Duration,
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

/// The request header carrying the Turnstile response token the public
/// live route forwards to the gate (the widget's `cf-turnstile-response`
/// value). Nothing else of the request reaches the gate.
pub const TURNSTILE_HEADER: &str = "cf-turnstile-response";
/// Longest Turnstile token forwarded; Cloudflare documents up to 2048
/// characters. A longer header is treated as absent.
pub const TURNSTILE_TOKEN_MAX: usize = 2048;
/// Cloudflare's country header, read only from a listener that trusts
/// its proxy headers (the unix socket behind cloudflared, or a
/// development bind with `CONSENSUS_TRUST_PROXY_HEADERS=true`).
pub const COUNTRY_HEADER: &str = "cf-ipcountry";

/// What a live-run request offers the admission layer. Owned, so an
/// implementation can move it into an outbound HTTP call or a blocking
/// task. The session cookie and the other headers never reach the gate.
pub struct LiveAdmissionRequest {
    pub preset: PresetId,
    pub client: ClientKey,
    /// The session's pseudonym (`visitor:<16 hex>`), for the ledger's
    /// per-visitor budget and the `safety_identifier` sent to Meta; never
    /// the session id or its owner hash.
    pub session_pseudonym: String,
    /// The Turnstile response token, when the client sent one.
    pub turnstile_token: Option<String>,
    /// The visitor's 18+ and territory acknowledgment, as sent.
    pub attest_adult: bool,
    /// A judge-lane invite code, trimmed and bounded by the route; the
    /// gate hashes it and never stores or logs it.
    pub judge_code: Option<String>,
    /// The raw `CF-IPCountry` value, only when this listener trusts its
    /// proxy headers; `None` otherwise (a development bind), which the
    /// live surface treats as an unknown country.
    pub country: Option<String>,
}

/// An admitted live run: its canned task, the ports to deliberate it, and
/// how to undo the admission if the run cannot be started after all.
pub struct LiveGrant {
    pub input: TaskInput,
    pub deliberation: Box<dyn Deliberation>,
    /// Called exactly once if the run cannot be started after this grant
    /// (the store refused, the server is draining): the gate refunds
    /// whatever it charged or consumed (a ledger debit, a Turnstile
    /// token). Never called once the run's `TaskCreated` is durable, and
    /// never called twice. Must not block the async worker; spawn if the
    /// refund does I/O.
    pub on_start_failed: Option<Box<dyn FnOnce() + Send>>,
}

/// The future [`LiveGate::admit`] returns.
pub type AdmitFuture<'a> = Pin<Box<dyn Future<Output = Result<LiveGrant, ApiError>> + Send + 'a>>;

/// Extension point for the public live mode (plan B9, ADR-0007):
/// Turnstile, the USD ledger, presets and the geographic policy decide
/// whether a visitor may spend the operator's key. The production
/// implementation is [`crate::live::gate::LiveGate`]; until one is wired
/// into [`AppState::live_gate`], the public live route answers
/// `503 live_disabled`.
///
/// Contract of the route (`POST /api/v1/live-runs`), in order:
///
/// 1. the body is validated (bounds of `judge_code`); a repeated
///    `Idempotency-Key` is answered from the stored creation, and one
///    reused with another body is `409`, before anything else, so a
///    retried submit whose first attempt succeeded costs nothing more. The
///    key itself never reaches the gate: Siteverify is called with a fresh
///    UUID per request, which is what keeps one solved Turnstile challenge
///    to one run (the route's replay is owner-scoped and recorded only
///    after the start, so it could not);
/// 2. the request is admitted like every creation (session, rates, the
///    visitor-run ring, not shutting down);
/// 3. a live deliberation slot is reserved ([`RunManager::reserve`]):
///    `429 too_many_runs` or `503 shutting_down` happen *before* the gate
///    is asked, so a refusal there charges nobody;
/// 4. `admit` runs, asynchronously (Turnstile siteverify is an outbound
///    call; nothing here blocks a worker). An `Err` is answered as is and
///    releases the slot;
/// 5. the run is started with the reserved slot. If that fails,
///    [`LiveGrant::on_start_failed`] is called and the error answered.
///
/// `admit` must be idempotent-safe against its own retries only: the
/// route calls it once per request.
pub trait LiveGate: Send + Sync {
    fn admit(&self, request: LiveAdmissionRequest) -> AdmitFuture<'_>;

    /// What `/capabilities` publishes about live mode: presets, the
    /// visitor-safe budget projection and the requirements. `None` when
    /// the gate has nothing to publish (a test double). May read the
    /// ledger's files: the handler calls it from a blocking task.
    fn capabilities(&self) -> Option<LiveCapabilitiesV1> {
        None
    }

    /// What `/ready` reports about live mode (`None` when not applicable).
    /// May read the ledger's files: called from a blocking task.
    fn readiness(&self) -> Option<LiveReadyV1> {
        None
    }
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

/// A run creation remembered by its `Idempotency-Key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreationRecord {
    pub run_id: RunId,
    /// SHA-256 of the request body as parsed, so the same key with another
    /// body is refused.
    pub body_sha256: String,
}

/// Run creations remembered by `Idempotency-Key`, per owner (UI-1): a
/// retried submit returns the run it already made instead of a second
/// one. A bounded LRU; the key is scoped to the owner, so one visitor's
/// keys never meet another's.
pub struct CreationKeys {
    inner: Mutex<LruMap<String, CreationRecord>>,
}

impl CreationKeys {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: Mutex::new(LruMap::new(cap)),
        }
    }

    fn scope(owner: &Owner, key: &str) -> String {
        let owner = match owner {
            Owner::Session { hash } => hash.as_str().to_string(),
            Owner::Operator => "operator".into(),
            Owner::Showcase => "showcase".into(),
        };
        format!("{owner}:{key}")
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LruMap<String, CreationRecord>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn get(&self, owner: &Owner, key: &str) -> Option<CreationRecord> {
        self.lock()
            .get_mut(&Self::scope(owner, key))
            .map(|record| record.clone())
    }

    pub fn insert(&self, owner: &Owner, key: &str, record: CreationRecord) {
        self.lock().insert(Self::scope(owner, key), record);
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How long `/ready` reuses the store's health: [`RunStore::health`]
/// re-verifies the whole run index, so it is asked at health-check rates,
/// not per request.
///
/// [`RunStore::health`]: consensus_runtime::RunStore::health
pub const READY_CACHE_TTL: Duration = Duration::from_secs(2);

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
    /// Run creations by `Idempotency-Key`.
    pub creation_keys: Arc<CreationKeys>,
    /// The store health `/ready` answered last, and when.
    pub ready_cache: Arc<Mutex<Option<(Instant, StoreHealth)>>>,
    /// The geographic policy (ADR-0007 duty 7): the portal-wide block is
    /// applied to every public route by the `portal_geo` middleware, the
    /// live-only block by the live gate. Loaded from the `CONSENSUS_GEO_*` variables
    /// with their defaults whether or not live mode is enabled.
    pub geo: Arc<GeoPolicy>,
}

impl AppState {
    pub fn surface(&self) -> Surface {
        self.config.surface
    }

    /// True when `CF-*` headers name the client and its country: the
    /// listener sits behind cloudflared (or a development bind said so).
    pub fn trusts_proxy_headers(&self) -> bool {
        self.config.client_key_policy == ClientKeyPolicy::TrustedProxyHeader
    }
}

/// The raw `CF-IPCountry` value of a request, only from a listener that
/// trusts its proxy headers; `None` otherwise, whatever the request says.
pub fn trusted_country(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if !state.trusts_proxy_headers() {
        return None;
    }
    headers
        .get(COUNTRY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
}

/// Fixed public statement for a geographic refusal.
pub fn geo_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::GeoBlockedPortal => "this service is not available in your country",
        ErrorCode::GeoBlockedLive => "live mode is not available in your country",
        _ => "your country could not be determined, so the request was refused",
    }
}

/// Portal-wide geographic block (ADR-0007 duty 7, ABUSE-9) on every public
/// route, static pages and `/research` included: behind cloudflared a
/// request whose `CF-IPCountry` is on the portal-wide list is `451
/// geo_blocked_portal`, and one without the header is `451 geo_unknown`
/// (the origin never guesses). A development bind that trusts no proxy
/// header blocks nothing. The live-only list is the live gate's.
async fn portal_geo(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.trusts_proxy_headers() {
        return next.run(req).await;
    }
    let Some(country) = trusted_country(&state, req.headers()) else {
        return ApiError::new(ErrorCode::GeoUnknown, geo_message(ErrorCode::GeoUnknown))
            .into_response();
    };
    match state.geo.decide(Some(&country), GeoSurface::Portal) {
        GeoDecision::Allow => next.run(req).await,
        GeoDecision::Block { code, .. } => {
            let code = ErrorCode::from(code);
            ApiError::new(code, geo_message(code)).into_response()
        }
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

/// `Host` must be one this server answers for. Over HTTP/2 the authority
/// travels in the request target instead of a `Host` header, so that is
/// what is checked when the header is absent.
async fn host_check(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.as_str().to_string()));
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

/// The request outlived the server's request timeout.
async fn timed_out(_: BoxError) -> Response {
    ApiError::new(
        ErrorCode::RequestTimeout,
        "the request was not handled within the server's request timeout",
    )
    .into_response()
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
/// static export, and a JSON 404 for unknown `/api` paths, all behind the
/// portal-wide geographic block.
pub fn public_router(state: AppState) -> Router {
    let api = common_routes()
        .route(
            "/api/v1/session",
            post(v1::create_session)
                .get(v1::get_session)
                .delete(v1::delete_session),
        )
        .route("/api/v1/runs", post(v1::create_guided_run))
        .route("/api/v1/live-runs", post(v1::create_live_run))
        // Both fallbacks are set once every route exists and before any
        // layer, so the layers wrap them too: the geographic block covers
        // the static export and unknown paths as well.
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(v1::public_fallback)
        .layer(middleware::from_fn_with_state(state.clone(), portal_geo));
    finish(api, state)
}

/// The operator server's router: the console at `/`, every common route
/// plus `POST /api/v1/operator/runs`, all `/api` behind the bearer token.
pub fn operator_router(state: AppState) -> Router {
    // Both fallbacks are attached before the bearer layer, so an unknown
    // `/api` path and a wrong method on a known one are refused with 401
    // like every other request: no route-existence oracle without the
    // token. (A method-not-allowed fallback set after the layer would
    // replace the layered default with an unlayered handler.)
    let api = common_routes()
        .route("/", get(v1::console))
        .route("/api/v1/operator/runs", post(v1::create_operator_run))
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(v1::operator_fallback)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            operator_bearer,
        ));
    finish(api, state)
}

/// The layers both surfaces share, outermost last: the request timeout
/// covers the whole handling of a request, body read included (a
/// stream's body is not covered once its headers are sent).
fn finish(router: Router<AppState>, state: AppState) -> Router {
    let timeout = state.config.request_timeout;
    router
        .layer(middleware::from_fn_with_state(state.clone(), csrf_check))
        .layer(middleware::from_fn_with_state(state.clone(), host_check))
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(timed_out))
                .layer(TimeoutLayer::new(timeout)),
        )
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

    #[test]
    fn creation_keys_are_scoped_per_owner_and_bounded() {
        let keys = CreationKeys::new(2);
        let a = Owner::session(&[1; 32]);
        let b = Owner::session(&[2; 32]);
        let record = |n: u32| CreationRecord {
            run_id: RunId::new(format!("run-00000000-0000-4000-8000-{n:012}")),
            body_sha256: "x".repeat(64),
        };
        keys.insert(&a, "k", record(1));
        assert_eq!(keys.get(&a, "k"), Some(record(1)));
        assert_eq!(keys.get(&b, "k"), None, "another owner's key");
        assert_eq!(keys.get(&Owner::Operator, "k"), None);
        keys.insert(&b, "k", record(2));
        keys.insert(&Owner::Operator, "k", record(3));
        assert_eq!(keys.len(), 2, "bounded");
        assert_eq!(keys.get(&a, "k"), None, "the oldest was evicted");
    }
}
