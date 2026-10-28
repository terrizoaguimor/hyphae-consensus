// SPDX-License-Identifier: Apache-2.0
//! `/api/v1` handlers. Every store read runs on the blocking pool; no
//! handler holds a lock across an await. Ownership is checked before any
//! run data, stream or receipt is produced, and the answer for a run the
//! caller does not own is the same 404 as for a run that does not exist.

use super::error::{ApiError, ErrorCode};
use super::types::*;
use super::{
    ApiJson, AppState, Client, CreationRecord, Identity, LiveAdmissionRequest, LiveGrant,
    READY_CACHE_TTL, TURNSTILE_HEADER, TURNSTILE_TOKEN_MAX,
};
use crate::guided;
use crate::public_config::BODY_LIMIT_BYTES;
use crate::public_view::{self, Surface};
use crate::runs::{
    CancelError, DecideError, DecisionInput, ReadError, RunKind, RunSnapshot, StartError,
    StartRequest,
};
use crate::stream::{run_stream, StreamParams};
use axum::body::Body;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::Sse;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use consensus_domain::*;
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::fakes::{FakeMemoryClient, FakeProofVerifier};
use consensus_runtime::policy::PolicyVersion;
use consensus_runtime::receipts::protocol_label;
use consensus_runtime::run_store::StoreHealth;
use consensus_runtime::scenarios::retry_constraint_scenario;
use consensus_runtime::{
    build_receipt_v2, parse_run_id, Clock, Orchestrator, Owner, Retention, TaskInput,
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Largest page of `GET /api/v1/runs`.
pub const LIST_PAGE_MAX: u32 = 50;
const LIST_PAGE_DEFAULT: u32 = 20;

/// Runs a blocking store operation off the async workers.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(format!("blocking task: {e}")))
}

fn source() -> SourceProvenance {
    SourceProvenance::ThisServer
}

fn summary_of(snapshot: &RunSnapshot, surface: Surface) -> RunSummaryV1 {
    public_view::summary(
        &snapshot.events,
        &snapshot.aggregate,
        source(),
        snapshot.active,
        surface,
    )
}

fn read_error(e: ReadError) -> ApiError {
    match e {
        ReadError::NotFound => ApiError::not_found(),
        ReadError::Busy => {
            ApiError::new(ErrorCode::RunBusy, "the run is busy; retry shortly").with_retry_after(1)
        }
        ReadError::Store(e) => ApiError::internal(e),
        ReadError::Log(e) => ApiError::internal(e),
    }
}

fn start_error(e: StartError) -> ApiError {
    match e {
        StartError::Busy { retry_after } => ApiError::new(
            ErrorCode::TooManyRuns,
            "every deliberation slot is busy; retry later",
        )
        .with_retry_after(retry_after.as_secs()),
        StartError::ShuttingDown => {
            ApiError::new(ErrorCode::ShuttingDown, "the server is shutting down")
                .with_retry_after(10)
        }
        StartError::Store(e) => ApiError::internal(e),
        StartError::Orchestrator(e) => ApiError::internal(e),
        StartError::Internal(e) => ApiError::internal(e),
    }
}

fn retention_for(state: &AppState) -> Retention {
    match state.surface() {
        Surface::Operator => Retention::Keep,
        Surface::Public => Retention::Expiring {
            access_for: state.config.visitor_access,
            delete_after: state.config.visitor_delete,
        },
    }
}

fn rate_limited(retry: Duration) -> ApiError {
    ApiError::new(ErrorCode::RateLimited, "too many requests from this client")
        .with_retry_after(retry.as_secs())
}

fn run_id_or_404(id: &str) -> Result<RunId, ApiError> {
    parse_run_id(id).map_err(|_| ApiError::not_found())
}

/// The run's current snapshot for `owner`, or 404.
async fn snapshot(state: &AppState, id: &str, owner: &Owner) -> Result<RunSnapshot, ApiError> {
    run_id_or_404(id)?;
    let manager = state.manager.clone();
    let id = id.to_string();
    let owner = owner.clone();
    blocking(move || manager.snapshot(&id, &owner))
        .await?
        .map_err(read_error)
}

/// Whether `owner` may reach the run, without reading its log: the active
/// map, else the run index and the cached sidecar. 404 for anyone else.
async fn owned(state: &AppState, id: &str, owner: &Owner) -> Result<(), ApiError> {
    run_id_or_404(id)?;
    let manager = state.manager.clone();
    let id = id.to_string();
    let owner = owner.clone();
    blocking(move || manager.owned_meta(&id, &owner))
        .await?
        .map(|_| ())
        .map_err(read_error)
}

fn created_response(state: &AppState, snapshot: &RunSnapshot) -> Response {
    let summary = summary_of(snapshot, state.surface());
    let location = format!("/api/v1/runs/{}", summary.run_id);
    let body = RunCreatedV1 {
        links: RunLinksV1::for_run(&summary.run_id),
        run: summary,
    };
    let mut response = (StatusCode::ACCEPTED, Json(body)).into_response();
    if let Ok(v) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, v);
    }
    response
}

/// Admission shared by every run creation: shutdown, rate, and on the
/// public surface the visitor-run ring, which purges the oldest resting
/// visitor runs to make room and refuses only when nothing can be freed.
async fn admit_creation(state: &AppState, client: &Client) -> Result<(), ApiError> {
    if state.manager.is_shutting_down() {
        return Err(
            ApiError::new(ErrorCode::ShuttingDown, "the server is shutting down")
                .with_retry_after(10),
        );
    }
    state.rates.runs.check(client.0).map_err(rate_limited)?;
    if state.surface() == Surface::Public {
        let manager = state.manager.clone();
        let cap = state.config.guided_ring;
        let ring = blocking(move || manager.make_room(cap))
            .await?
            .map_err(ApiError::internal)?;
        if !ring.purged.is_empty() {
            eprintln!(
                "[ring] {} visitor run(s) stored at the cap of {cap}: purged {} to admit a new one",
                ring.stored,
                ring.purged.len()
            );
        }
        if !ring.admitted {
            return Err(ApiError::new(
                ErrorCode::StorageFull,
                "the server holds its maximum number of visitor runs and none can be freed right now; retry shortly",
            )
            .with_retry_after(30));
        }
    }
    Ok(())
}

/// `Idempotency-Key` on a creation is optional; when present it must be
/// well formed, and a retried submit with it gets the run it already made.
fn creation_key(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    if headers.get("idempotency-key").is_none() {
        return Ok(None);
    }
    idempotency_key(headers).map(Some)
}

/// SHA-256 of a parsed request body, so the same key with another body is
/// told apart.
fn body_sha256<T: serde::Serialize>(body: &T) -> String {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The 202 of the creation this owner already made with `key`, if its run
/// still exists; 409 when the key was used with another body; None when
/// the key is new (or its run is gone), so the creation proceeds.
async fn replayed_creation(
    state: &AppState,
    identity: &Identity,
    key: Option<&str>,
    body_sha256: &str,
) -> Result<Option<Response>, ApiError> {
    let Some(key) = key else {
        return Ok(None);
    };
    let Some(record) = state.creation_keys.get(&identity.owner, key) else {
        return Ok(None);
    };
    if record.body_sha256 != body_sha256 {
        return Err(ApiError::new(
            ErrorCode::IdempotencyKeyReused,
            "this Idempotency-Key was already used to create a different run",
        ));
    }
    match snapshot(state, record.run_id.as_str(), &identity.owner).await {
        Ok(snapshot) => Ok(Some(created_response(state, &snapshot))),
        // Purged meanwhile: the key is free again.
        Err(e) if e.code == ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn remember_creation(
    state: &AppState,
    identity: &Identity,
    key: Option<String>,
    body_sha256: String,
    snapshot: &RunSnapshot,
) {
    if let Some(key) = key {
        state.creation_keys.insert(
            &identity.owner,
            &key,
            CreationRecord {
                run_id: snapshot.aggregate.run_id.clone(),
                body_sha256,
            },
        );
    }
}

fn guided_start(
    state: &AppState,
    identity: &Identity,
    scenario: GuidedScenario,
    origin: RunOrigin,
) -> StartRequest {
    let mut s = guided::build(scenario);
    s.input.mode = Some(guided::task_mode(scenario, origin));
    s.input.submitted_by = identity.authorized_by.clone();
    let orchestrator = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: SystemClock,
        ids: UuidIdGen,
        limits: state.config.run_limits,
    };
    StartRequest {
        owner: identity.owner.clone(),
        retention: retention_for(state),
        input: s.input,
        kind: RunKind::Guided,
        deliberation: Box::new(orchestrator),
    }
}

// ---------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------

fn session_body(session: &crate::session::Session, created: bool) -> SessionV1 {
    SessionV1 {
        created,
        expires_at: session.expires_at_rfc3339(),
        pseudonym: session.pseudonym(),
    }
}

pub async fn create_session(
    State(state): State<AppState>,
    client: Client,
    headers: HeaderMap,
) -> Response {
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    if let Ok(existing) = state.sessions.from_cookie_header(cookie) {
        return Json(session_body(&existing, false)).into_response();
    }
    if let Err(retry) = state.rates.session.check(client.0) {
        return rate_limited(retry).into_response();
    }
    let session = match state.sessions.create() {
        Ok(s) => s,
        Err(e) => return ApiError::internal(e).into_response(),
    };
    let mut response = (StatusCode::CREATED, Json(session_body(&session, true))).into_response();
    if let Ok(v) = HeaderValue::from_str(&state.sessions.set_cookie(&session)) {
        response.headers_mut().insert(header::SET_COOKIE, v);
    }
    response
}

pub async fn get_session(identity: Identity) -> Result<Json<SessionV1>, ApiError> {
    let session = identity
        .session
        .ok_or_else(|| ApiError::new(ErrorCode::SessionRequired, "no session"))?;
    Ok(Json(session_body(&session, false)))
}

pub async fn delete_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    if let Ok(session) = state.sessions.from_cookie_header(cookie) {
        state.sessions.remove(session.id());
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = HeaderValue::from_str(&state.sessions.clear_cookie()) {
        response.headers_mut().insert(header::SET_COOKIE, v);
    }
    response
}

// ---------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------

pub async fn create_guided_run(
    State(state): State<AppState>,
    client: Client,
    identity: Identity,
    headers: HeaderMap,
    ApiJson(req): ApiJson<CreateGuidedRunRequest>,
) -> Result<Response, ApiError> {
    let key = creation_key(&headers)?;
    let body_hash = body_sha256(&req);
    if let Some(replayed) = replayed_creation(&state, &identity, key.as_deref(), &body_hash).await?
    {
        return Ok(replayed);
    }
    admit_creation(&state, &client).await?;
    let start = guided_start(&state, &identity, req.scenario, RunOrigin::Public);
    let snapshot = state.manager.start(start).await.map_err(start_error)?;
    remember_creation(&state, &identity, key, body_hash, &snapshot);
    Ok(created_response(&state, &snapshot))
}

/// The Turnstile token a request carries, bounded; a longer or empty
/// header counts as none.
fn turnstile_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(TURNSTILE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|t| !t.is_empty() && t.len() <= TURNSTILE_TOKEN_MAX)
        .map(str::to_string)
}

/// See [`super::LiveGate`] for the order of admission: creation
/// admission, the live slot, the gate, then the start; a start failure
/// after the grant calls the grant's refund.
pub async fn create_live_run(
    State(state): State<AppState>,
    client: Client,
    identity: Identity,
    headers: HeaderMap,
    ApiJson(req): ApiJson<CreateLiveRunRequest>,
) -> Result<Response, ApiError> {
    let Some(gate) = state.live_gate.clone() else {
        return Err(ApiError::new(
            ErrorCode::LiveDisabled,
            "the public live mode is not enabled on this server",
        ));
    };
    let key = creation_key(&headers)?;
    let body_hash = body_sha256(&req);
    if let Some(replayed) = replayed_creation(&state, &identity, key.as_deref(), &body_hash).await?
    {
        return Ok(replayed);
    }
    admit_creation(&state, &client).await?;
    // The live slot before the gate: a refusal here charges nobody.
    let reservation = state
        .manager
        .reserve(RunKind::Live)
        .await
        .map_err(start_error)?;
    let grant = gate
        .admit(LiveAdmissionRequest {
            preset: req.preset_id.clone(),
            client: client.0,
            session_pseudonym: identity.authorized_by.clone(),
            turnstile_token: turnstile_token(&headers),
        })
        .await?;
    let LiveGrant {
        mut input,
        deliberation,
        on_start_failed,
    } = grant;
    input.submitted_by = identity.authorized_by.clone();
    let start = StartRequest {
        owner: identity.owner.clone(),
        retention: retention_for(&state),
        input,
        kind: RunKind::Live,
        deliberation,
    };
    match state.manager.start_reserved(reservation, start).await {
        Ok(snapshot) => {
            remember_creation(&state, &identity, key, body_hash, &snapshot);
            Ok(created_response(&state, &snapshot))
        }
        Err(e) => {
            // Nothing durable describes the run: whatever the gate charged
            // is given back.
            if let Some(undo) = on_start_failed {
                undo();
            }
            Err(start_error(e))
        }
    }
}

pub async fn create_operator_run(
    State(state): State<AppState>,
    client: Client,
    identity: Identity,
    headers: HeaderMap,
    ApiJson(req): ApiJson<CreateOperatorRunRequest>,
) -> Result<Response, ApiError> {
    req.validate().map_err(ApiError::invalid)?;
    let key = creation_key(&headers)?;
    let body_hash = body_sha256(&req);
    if let Some(replayed) = replayed_creation(&state, &identity, key.as_deref(), &body_hash).await?
    {
        return Ok(replayed);
    }
    admit_creation(&state, &client).await?;
    let start = match req.model {
        OperatorModel::Fake => {
            let scenario = req
                .scenario
                .ok_or_else(|| ApiError::invalid("model=fake requires a scenario"))?;
            guided_start(&state, &identity, scenario, RunOrigin::Operator)
        }
        OperatorModel::Meta => {
            // Client construction may spawn threads and read files: off
            // the async workers.
            let (state2, identity2, req2) = (state.clone(), identity.clone(), req.clone());
            blocking(move || operator_meta_start(&state2, &identity2, &req2)).await??
        }
    };
    let snapshot = state.manager.start(start).await.map_err(start_error)?;
    remember_creation(&state, &identity, key, body_hash, &snapshot);
    Ok(created_response(&state, &snapshot))
}

/// Builds a live Meta deliberation on the operator surface: the only
/// place the Meta credentials are consulted.
fn operator_meta_start(
    state: &AppState,
    identity: &Identity,
    req: &CreateOperatorRunRequest,
) -> Result<StartRequest, ApiError> {
    use crate::config::{HyphaeConfig, MetaConfig};
    use crate::hyphae::{connect_from_config, HyphaeProofVerifier, SystemCommandRunner};
    use crate::meta::MetaModelClient;

    let cfg = MetaConfig::from_env().ok_or_else(|| {
        ApiError::new(
            ErrorCode::ModelNotConfigured,
            "model=meta requires META_API_KEY (or MODEL_API_KEY) in the server environment",
        )
    })?;
    let model = MetaModelClient::new(cfg).map_err(|e| {
        eprintln!("[operator] meta client init failed: {e}");
        ApiError::new(
            ErrorCode::UpstreamUnavailable,
            "the model client could not be initialised",
        )
    })?;
    let evidence = match req.memory {
        OperatorMemory::Fake => EvidenceMode::Simulated,
        OperatorMemory::Offline => EvidenceMode::Offline,
        OperatorMemory::Hyphae => EvidenceMode::LiveHyphae,
    };
    let input = TaskInput {
        project_id: ProjectId::new(req.project_id.as_deref().unwrap_or("demo-repo")),
        title: req
            .title
            .clone()
            .unwrap_or_else(|| "Fix payment client timeouts".into()),
        description: req.description.clone().unwrap_or_else(|| {
            "Payment requests intermittently time out under load. Propose a fix.".into()
        }),
        submitted_by: identity.authorized_by.clone(),
        mode: Some(TaskMode {
            model: ModelMode::Meta,
            evidence,
            origin: RunOrigin::Operator,
            preset: None,
        }),
    };
    let limits = state.config.run_limits;
    let deliberation: Box<dyn crate::runs::Deliberation> = match req.memory {
        OperatorMemory::Fake => {
            let s = retry_constraint_scenario();
            Box::new(Orchestrator {
                model,
                memory: s.memory,
                verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
                clock: SystemClock,
                ids: UuidIdGen,
                limits,
            })
        }
        OperatorMemory::Offline => Box::new(Orchestrator {
            model,
            memory: FakeMemoryClient::offline(),
            verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
            clock: SystemClock,
            ids: UuidIdGen,
            limits,
        }),
        OperatorMemory::Hyphae => {
            let hcfg = HyphaeConfig::from_env().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::MemoryNotConfigured,
                    "memory=hyphae requires HYPHAE_NATIVE_API_KEY_FILE and HYPHAE_NATIVE_ENDPOINT or HYPHAE_BASE_URL",
                )
            })?;
            let memory = connect_from_config(&hcfg).map_err(|e| {
                eprintln!("[operator] hyphae connect failed: {e}");
                ApiError::new(
                    ErrorCode::UpstreamUnavailable,
                    "the memory backend could not be reached",
                )
            })?;
            let mut verifier = HyphaeProofVerifier::new(hcfg.binary.clone(), SystemCommandRunner);
            verifier.expected_anchor = hcfg.expected_anchor.clone();
            Box::new(Orchestrator {
                model,
                memory,
                verifier,
                clock: SystemClock,
                ids: UuidIdGen,
                limits,
            })
        }
    };
    Ok(StartRequest {
        owner: identity.owner.clone(),
        retention: Retention::Keep,
        input,
        kind: RunKind::Live,
        deliberation,
    })
}

pub async fn list_runs(
    State(state): State<AppState>,
    identity: Identity,
    query: Result<Query<ListRunsQuery>, QueryRejection>,
) -> Result<Json<RunListV1>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::invalid(e.body_text()))?;
    let limit = query.limit.unwrap_or(LIST_PAGE_DEFAULT);
    if limit == 0 || limit > LIST_PAGE_MAX {
        return Err(ApiError::invalid(format!(
            "limit must be 1 to {LIST_PAGE_MAX}"
        )));
    }
    let manager = state.manager.clone();
    let owner = identity.owner.clone();
    let mut metas = blocking(move || manager.list(&owner))
        .await?
        .map_err(ApiError::internal)?;
    metas.reverse(); // newest first
    if let Some(cursor) = &query.cursor {
        let cursor =
            run_id_or_404(cursor).map_err(|_| ApiError::invalid("cursor is not a run id"))?;
        match metas.iter().position(|m| m.run_id == cursor) {
            Some(pos) => metas.drain(..=pos),
            None => return Err(ApiError::invalid("cursor is not in the list")),
        };
    }
    let page: Vec<_> = metas.iter().take(limit as usize).cloned().collect();
    let next_cursor = (metas.len() > page.len())
        .then(|| page.last().map(|m| m.run_id.to_string()))
        .flatten();
    let mut runs = Vec::with_capacity(page.len());
    for meta in page {
        match snapshot(&state, meta.run_id.as_str(), &identity.owner).await {
            Ok(snapshot) => runs.push(summary_of(&snapshot, state.surface())),
            // Expired access or a run purged meanwhile: left out, not an error.
            Err(e) if e.code == ErrorCode::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Json(RunListV1 { runs, next_cursor }))
}

pub async fn get_run(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> Result<Json<RunDetailV1>, ApiError> {
    let snapshot = snapshot(&state, &id, &identity.owner).await?;
    Ok(Json(public_view::detail(
        &snapshot.events,
        &snapshot.aggregate,
        source(),
        snapshot.active,
        state.surface(),
    )))
}

/// The sequence a resuming client already has: `Last-Event-ID` wins over
/// `?after`.
fn resume_point(headers: &HeaderMap, query: &EventsQuery) -> u64 {
    headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .or(query.after)
        .unwrap_or(0)
}

pub async fn run_events(
    State(state): State<AppState>,
    client: Client,
    identity: Identity,
    Path(id): Path<String>,
    headers: HeaderMap,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::invalid(e.body_text()))?;
    let run_id = run_id_or_404(&id)?;
    // The rate first, so the caller pays for the lookups that follow.
    state.rates.sse.check(client.0).map_err(rate_limited)?;
    // Ownership and existence without reading the log: a 404 JSON, never
    // a stream, and no file I/O for a run the caller does not own.
    owned(&state, &id, &identity.owner).await?;
    let permit = state
        .stream_caps
        .acquire(run_id.as_str(), client.0)
        .map_err(|cap| {
            ApiError::new(
                ErrorCode::TooManyStreams,
                format!("stream cap reached ({cap:?})").to_lowercase(),
            )
            .with_retry_after(5)
        })?;
    // Subscribe, then the one full read of this request (a damaged log is
    // an error here, never a stream of it); the stream replays that read.
    let subscription = state.manager.broadcasts().subscribe(&run_id);
    let first = snapshot(&state, &id, &identity.owner).await?;
    let after = resume_point(&headers, &query);
    let stream = run_stream(StreamParams {
        manager: state.manager.clone(),
        run_id,
        owner: identity.owner.clone(),
        surface: state.surface(),
        source: source(),
        heartbeat: state.config.sse.heartbeat,
        after,
        permit: Some(permit),
        subscription,
        first,
    });
    let mut response = Sse::new(stream).into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

/// The verbatim stored records, so the chain can be recomputed by the
/// client. On the public surface only a chain that cannot carry upstream
/// text is published ([`public_view::ndjson_publishable`]); the owner of
/// a live run gets `404 ndjson_unavailable` and keeps the detail and the
/// receipt.
pub async fn run_events_ndjson(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let snapshot = snapshot(&state, &id, &identity.owner).await?;
    if !public_view::ndjson_publishable(&snapshot.events, state.surface()) {
        return Err(ApiError::new(
            ErrorCode::NdjsonUnavailable,
            "the verbatim event log of a live run is not published on the public surface; the detail and the receipt carry its sanitized view",
        ));
    }
    let mut body = String::new();
    for ev in &snapshot.events {
        let line = serde_json::to_string(ev).map_err(ApiError::internal)?;
        body.push_str(&line);
        body.push('\n');
    }
    Ok(attachment(
        body,
        "application/x-ndjson",
        &format!("events-{id}.jsonl"),
    ))
}

fn attachment(body: String, content_type: &'static str, filename: &str) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, v);
    }
    response
}

pub async fn run_receipt(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let snapshot = snapshot(&state, &id, &identity.owner).await?;
    let receipt =
        build_receipt_v2(&snapshot.events, SystemClock.now()).map_err(ApiError::internal)?;
    let receipt = public_view::sanitize_receipt(receipt, state.surface());
    let body = serde_json::to_string_pretty(&receipt).map_err(ApiError::internal)?;
    Ok(attachment(
        body,
        "application/json",
        &format!("receipt-{id}.json"),
    ))
}

/// `Idempotency-Key`: 1 to 128 characters of `[A-Za-z0-9_.:-]`.
fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::IdempotencyKeyRequired,
                "an Idempotency-Key header is required",
            )
        })?;
    if value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
    {
        return Err(ApiError::new(
            ErrorCode::IdempotencyKeyRequired,
            "Idempotency-Key must be 1 to 128 characters of A-Z, a-z, 0-9, _ . : -",
        ));
    }
    Ok(value.to_string())
}

pub async fn decide(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(req): ApiJson<DecisionRequest>,
) -> Result<Json<DecisionResponseV1>, ApiError> {
    req.validate().map_err(ApiError::invalid)?;
    run_id_or_404(&id)?;
    let key = idempotency_key(&headers)?;
    let input = DecisionInput {
        approved: req.approved,
        note: req.note.clone(),
        expected_recommendation: req.expected.recommendation,
        expected_last_sequence: req.expected.last_sequence,
        idempotency_key: key,
        authority: identity.authority,
        authorized_by: identity.authorized_by.clone(),
    };
    let manager = state.manager.clone();
    let owner = identity.owner.clone();
    let snapshot = blocking(move || manager.decide(&id, &owner, input))
        .await?
        .map_err(|e| match e {
            DecideError::NotFound => ApiError::not_found(),
            DecideError::NotAwaiting => ApiError::new(
                ErrorCode::RunNotAwaitingDecision,
                "the run is not awaiting a human decision",
            ),
            DecideError::Conflict(detail) => ApiError::new(ErrorCode::DecisionConflict, detail),
            DecideError::KeyReused => ApiError::new(
                ErrorCode::IdempotencyKeyReused,
                "this Idempotency-Key was already used with a different decision",
            ),
            DecideError::OverrideNoteRequired => ApiError::new(
                ErrorCode::OverrideNoteRequired,
                "approving a blocked or abstained run is an override and requires a note",
            ),
            DecideError::Read(e) => read_error(e),
        })?;
    let decision = snapshot
        .events
        .iter()
        .rev()
        .find_map(|e| match &e.payload {
            EventPayload::HumanDecisionRecorded { authorization } => {
                Some(public_view::decision_v1(authorization, state.surface()))
            }
            _ => None,
        })
        .ok_or_else(|| ApiError::internal("decision recorded but not found in the run"))?;
    Ok(Json(DecisionResponseV1 {
        run: summary_of(&snapshot, state.surface()),
        decision,
    }))
}

pub async fn cancel(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let run_id = run_id_or_404(&id)?;
    let manager = state.manager.clone();
    let owner = identity.owner.clone();
    blocking(move || manager.cancel(&id, &owner))
        .await?
        .map_err(|e| match e {
            CancelError::NotFound => ApiError::not_found(),
            CancelError::NotCancellable => ApiError::new(
                ErrorCode::RunNotCancellable,
                "only a run that is still deliberating can be cancelled",
            ),
            CancelError::Read(e) => read_error(e),
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(CancelResponseV1 {
            run_id,
            cancelling: true,
        }),
    )
        .into_response())
}

// ---------------------------------------------------------------------
// Catalogue, capabilities, health
// ---------------------------------------------------------------------

pub async fn scenarios() -> Json<ScenariosV1> {
    Json(ScenariosV1 {
        scenarios: GuidedScenario::ALL
            .iter()
            .map(|s| guided::scenario_v1(*s))
            .collect(),
    })
}

/// The policy rules label, e.g. `1.2`.
pub fn policy_version_label() -> String {
    match PolicyVersion::CURRENT {
        PolicyVersion::V1_1 => "1.1".into(),
        PolicyVersion::V1_2 => "1.2".into(),
    }
}

pub fn capabilities_of(state: &AppState) -> CapabilitiesV1 {
    let c = &state.config;
    CapabilitiesV1 {
        surface: c.surface.v1(),
        live_enabled: state.live_gate.is_some(),
        guided_enabled: true,
        showcase_enabled: state.showcase.is_enabled(),
        limits: LimitsV1 {
            body_limit_bytes: BODY_LIMIT_BYTES as u64,
            guided_concurrency: c.guided_concurrency as u64,
            live_concurrency: c.live_concurrency as u64,
            run_deadline_secs: c.run_deadline.as_secs(),
            max_run_tokens: c.run_limits.max_run_tokens,
            sse_max_per_run: c.sse.max_per_run as u64,
            sse_max_per_client: c.sse.max_per_client as u64,
            sse_max_global: c.sse.max_global as u64,
            sse_heartbeat_secs: c.sse.heartbeat.as_secs(),
            rate_session_per_min: c.rates.session_per_min as u64,
            rate_runs_per_min: c.rates.runs_per_min as u64,
            rate_sse_per_min: c.rates.sse_per_min as u64,
            session_ttl_secs: c.session_ttl.as_secs(),
            session_max: c.session_max as u64,
            guided_ring: c.guided_ring as u64,
            visitor_access_hours: c.visitor_access.as_secs() / 3600,
            visitor_delete_days: c.visitor_delete.as_secs() / 86_400,
            list_page_max: LIST_PAGE_MAX as u64,
            request_timeout_secs: c.request_timeout.as_secs(),
        },
        schema_version: SCHEMA_VERSION.into(),
        policy_version: policy_version_label(),
        protocol: protocol_label(SCHEMA_VERSION),
        decision_authority: match c.surface {
            Surface::Public => Authority::PublicDemo,
            Surface::Operator => Authority::Operator,
        },
    }
}

pub async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesV1> {
    Json(capabilities_of(&state))
}

pub fn health_of(state: &AppState) -> HealthV1 {
    HealthV1 {
        status: "ok".into(),
        version: state.config.version.clone(),
        schema_version: SCHEMA_VERSION.into(),
        policy_version: policy_version_label(),
        contract_sha256: state.contract_sha256.to_string(),
        executor: "disabled".into(),
    }
}

/// Static facts only: nothing here probes the environment.
pub async fn health(State(state): State<AppState>) -> Json<HealthV1> {
    Json(health_of(&state))
}

/// The store's health, re-verified at most once per [`READY_CACHE_TTL`]:
/// verifying the whole run index per request would let an anonymous
/// caller make the server re-hash it at will.
async fn store_health(state: &AppState) -> Result<StoreHealth, ApiError> {
    {
        let cache = state
            .ready_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((at, health)) = cache.as_ref() {
            if at.elapsed() < READY_CACHE_TTL {
                return Ok(health.clone());
            }
        }
    }
    let manager = state.manager.clone();
    let health = blocking(move || manager.store().health()).await?;
    *state
        .ready_cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((Instant::now(), health.clone()));
    Ok(health)
}

pub async fn ready(State(state): State<AppState>) -> Result<Json<ReadyV1>, ApiError> {
    let health = store_health(&state).await?;
    let stats = state.manager.stats();
    let store = RunStoreStatusV1 {
        ok: health.index_head.is_some() && !health.index_tampered,
        index_seq: health.index_head.as_ref().map(|h| h.seq),
        index_head: health.index_head.as_ref().map(|h| h.hash.clone()),
        index_tampered: health.index_tampered,
        index_tail_repaired: health.index_tail_repaired,
        index_error: health.index_error.clone(),
    };
    let showcase = state.showcase.status();
    let ready = store.ok && !state.manager.is_shutting_down();
    Ok(Json(ReadyV1 {
        ready,
        run_store: store,
        showcase,
        active_runs: stats.active as u64,
        queued_runs: stats.queued as u64,
        shutting_down: state.manager.is_shutting_down(),
    }))
}

// ---------------------------------------------------------------------
// Showcase and research files
// ---------------------------------------------------------------------

pub async fn showcase_list(State(state): State<AppState>) -> Json<ShowcaseV1> {
    Json(state.showcase.list())
}

pub async fn showcase_detail(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunDetailV1>, ApiError> {
    state
        .showcase
        .detail(&run_id, state.surface())
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

pub async fn showcase_receipt(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Response, ApiError> {
    let receipt = state
        .showcase
        .receipt(&run_id, SystemClock.now())
        .ok_or_else(ApiError::not_found)?
        .map_err(ApiError::internal)?;
    let receipt = public_view::sanitize_receipt(receipt, state.surface());
    let body = serde_json::to_string_pretty(&receipt).map_err(ApiError::internal)?;
    Ok(attachment(
        body,
        "application/json",
        &format!("receipt-{run_id}.json"),
    ))
}

/// True when `If-None-Match` names `etag`.
fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
}

fn not_modified(etag: &str) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    if let Ok(v) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(header::ETAG, v);
    }
    response
}

pub async fn research_file(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // A conditional request is answered from the digest known at load,
    // without reading or hashing the file.
    if let Some(known) = state.showcase.known_sha256(&path) {
        let etag = format!("\"{known}\"");
        if etag_matches(&headers, &etag) {
            return Ok(not_modified(&etag));
        }
    }
    let showcase = state.showcase.clone();
    let file = blocking(move || showcase.research_file(&path))
        .await?
        .map_err(|e| match e {
            crate::showcase::ResearchError::NotFound => ApiError::not_found(),
            other => ApiError::internal(other),
        })?;
    let etag = format!("\"{}\"", file.sha256);
    if etag_matches(&headers, &etag) {
        return Ok(not_modified(&etag));
    }
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(file.bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(file.content_type),
    );
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("sha256={}", file.sha256)) {
        h.insert("x-content-sha256", v);
    }
    Ok(response)
}

// ---------------------------------------------------------------------
// Fallbacks and the console
// ---------------------------------------------------------------------

/// Unknown `/api` paths are a JSON 404; everything else is the export.
pub async fn public_fallback(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> Response {
    let path = req.uri().path().to_string();
    if super::is_api_path(&path) {
        return ApiError::not_found().into_response();
    }
    match &state.static_site {
        Some(site) => {
            let site = site.clone();
            let headers = req.headers().clone();
            match tokio::task::spawn_blocking(move || site.serve(&path, &headers)).await {
                Ok(response) => response,
                Err(e) => ApiError::internal(e).into_response(),
            }
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

pub async fn operator_fallback() -> Response {
    ApiError::not_found().into_response()
}

pub async fn console(State(state): State<AppState>) -> Response {
    match &state.console {
        Some(page) => {
            let mut response = Html(page.html.clone()).into_response();
            let h = response.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&page.csp) {
                h.insert(header::CONTENT_SECURITY_POLICY, v);
            }
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        None => ApiError::not_found().into_response(),
    }
}
