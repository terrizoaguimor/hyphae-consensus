//! Operator console: Axum HTTP API + server-sent event replay.
//!
//! Concurrency model: one owning `EventLog` behind a mutex performs all
//! appends (runs, decisions). Read paths take a parsed snapshot of the
//! log file, tolerating a partially-written final line, so the console
//! can render while a run is in flight. Run ids and event ids use UUIDs
//! here (multi-run log), unlike the deterministic single-run demo CLI.
//!
//! Modes are explicit and honest:
//! - `model=fake`: runs the scripted reference scenario (labeled as such).
//! - `model=meta`: live Meta Model API; requires env credentials.
//! - `memory=fake`: seeded in-memory constraint/fact records (labeled).
//! - `memory=hyphae`: real MCP adapter; requires Hyphae env config.
//! - `memory=offline`: demonstrates documented offline degradation.

use crate::config::{HyphaeConfig, MetaConfig};
use crate::hyphae::{connect_from_config, HyphaeProofVerifier, SystemCommandRunner};
use crate::meta::MetaModelClient;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use consensus_domain::*;
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::fakes::{FakeMemoryClient, FakeProofVerifier};
use consensus_runtime::orchestrator::{
    build_receipt, record_human_decision, Orchestrator, TaskInput,
};
use consensus_runtime::scenarios::retry_constraint_scenario;
use consensus_runtime::state_machine::RunAggregate;
use consensus_runtime::EventLog;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CONSOLE_HTML: &str = include_str!("console.html");
pub const DEFAULT_BIND: &str = "127.0.0.1:8787";

#[derive(Clone)]
pub struct AppState {
    owner: Arc<Mutex<EventLog>>,
    log_path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct CreateRunRequest {
    #[serde(default = "default_fake")]
    pub model: String,
    #[serde(default = "default_fake")]
    pub memory: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub project_id: Option<String>,
    #[serde(default = "default_operator")]
    pub submitted_by: String,
}

fn default_fake() -> String {
    "fake".into()
}
fn default_operator() -> String {
    "operator".into()
}

#[derive(Debug, Deserialize)]
pub struct DecisionRequest {
    pub approved: bool,
    pub authorized_by: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub state: RunState,
    pub recommendation: Option<RecommendationKind>,
    pub authorization: AuthorizationState,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunDetail {
    pub summary: RunSummary,
    pub revision_round: u32,
    pub recommendation: Option<Recommendation>,
    pub verifications: Vec<EvidenceVerification>,
    pub model_invocations: Vec<ModelInvocation>,
    pub events: Vec<ConsensusEvent>,
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({"error": msg.into()}))).into_response()
}

/// Parses the log file without the owning handle; tolerates a partial
/// final line (writer mid-append).
fn snapshot_events(path: &PathBuf) -> Vec<ConsensusEvent> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<ConsensusEvent>(l).ok())
        .collect()
}

fn aggregates(events: &[ConsensusEvent]) -> Vec<RunAggregate> {
    let mut order: Vec<RunId> = Vec::new();
    let mut by_run: HashMap<RunId, RunAggregate> = HashMap::new();
    for ev in events {
        let agg = by_run.entry(ev.run_id.clone()).or_insert_with(|| {
            order.push(ev.run_id.clone());
            RunAggregate::new(ev.run_id.clone())
        });
        let _ = agg.apply(ev);
    }
    order.iter().filter_map(|id| by_run.remove(id)).collect()
}

fn summary_of(agg: &RunAggregate) -> RunSummary {
    RunSummary {
        run_id: agg.run_id.to_string(),
        state: agg.state,
        recommendation: agg.recommendation.as_ref().map(|r| r.kind),
        authorization: agg.authorization,
        title: agg.task.as_ref().map(|t| t.title.clone()),
    }
}

fn detail_of(state: &AppState, run_id: &RunId) -> Option<RunDetail> {
    let events = snapshot_events(&state.log_path);
    let agg = aggregates(&events)
        .into_iter()
        .find(|a| &a.run_id == run_id)?;
    let run_events: Vec<ConsensusEvent> =
        events.into_iter().filter(|e| &e.run_id == run_id).collect();
    Some(RunDetail {
        summary: summary_of(&agg),
        revision_round: agg.revision_round,
        recommendation: agg.recommendation,
        verifications: agg.verifications,
        model_invocations: agg.invocations,
        events: run_events,
    })
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Html(CONSOLE_HTML) }))
        .route("/api/health", get(health))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/runs/{id}/decision", post(decide))
        .route("/api/runs/{id}/receipt", get(receipt))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "meta_configured": MetaConfig::from_env().is_some(),
        "hyphae_configured": HyphaeConfig::from_env().is_some(),
        "executor": "deferred (approval records only; no external mutation)",
    }))
}

async fn list_runs(State(state): State<AppState>) -> Json<Vec<RunSummary>> {
    let events = snapshot_events(&state.log_path);
    Json(aggregates(&events).iter().map(summary_of).collect())
}

async fn create_run(State(state): State<AppState>, Json(req): Json<CreateRunRequest>) -> Response {
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || execute_run(&state2, req)).await;
    match result {
        Ok(Ok(detail)) => (StatusCode::CREATED, Json(detail)).into_response(),
        Ok(Err((status, msg))) => err(status, msg),
        Err(join) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("run task: {join}"),
        ),
    }
}

fn execute_run(state: &AppState, req: CreateRunRequest) -> Result<RunDetail, (StatusCode, String)> {
    let bad = |m: &str| (StatusCode::BAD_REQUEST, m.to_string());
    let mut log = state.owner.lock().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "event log lock poisoned".into(),
        )
    })?;

    let agg = match (req.model.as_str(), req.memory.as_str()) {
        ("fake", _) => {
            let s = retry_constraint_scenario();
            let mut orch = Orchestrator {
                model: s.model,
                memory: s.memory,
                verifier: s.verifier,
                clock: SystemClock,
                ids: UuidIdGen,
            };
            orch.run(&mut log, s.input)
        }
        ("meta", mem) => {
            let cfg = MetaConfig::from_env()
                .ok_or_else(|| bad("model=meta requires META_API_KEY (or MODEL_API_KEY) in the server environment"))?;
            let model = MetaModelClient::new(cfg)
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
            let input = TaskInput {
                project_id: ProjectId::new(req.project_id.as_deref().unwrap_or("demo-repo")),
                title: req.title.clone().unwrap_or_else(|| "Fix payment client timeouts".into()),
                description: req
                    .description
                    .clone()
                    .unwrap_or_else(|| "Payment requests intermittently time out under load. Propose a fix.".into()),
                submitted_by: req.submitted_by.clone(),
            };
            match mem {
                "fake" => {
                    let s = retry_constraint_scenario();
                    let mut orch = Orchestrator {
                        model,
                        memory: s.memory,
                        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
                        clock: SystemClock,
                        ids: UuidIdGen,
                    };
                    orch.run(&mut log, input)
                }
                "offline" => {
                    let mut orch = Orchestrator {
                        model,
                        memory: FakeMemoryClient::offline(),
                        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
                        clock: SystemClock,
                        ids: UuidIdGen,
                    };
                    orch.run(&mut log, input)
                }
                "hyphae" => {
                    let hcfg = HyphaeConfig::from_env().ok_or_else(|| {
                        bad("memory=hyphae requires HYPHAE_NATIVE_API_KEY_FILE and HYPHAE_NATIVE_ENDPOINT or HYPHAE_BASE_URL")
                    })?;
                    let memory = connect_from_config(&hcfg)
                        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("hyphae connect: {e}")))?;
                    let verifier = HyphaeProofVerifier {
                        binary: hcfg.binary.clone(),
                        runner: SystemCommandRunner,
                    };
                    let mut orch = Orchestrator {
                        model,
                        memory,
                        verifier,
                        clock: SystemClock,
                        ids: UuidIdGen,
                    };
                    orch.run(&mut log, input)
                }
                other => return Err(bad(&format!("unknown memory mode: {other}"))),
            }
        }
        (other, _) => return Err(bad(&format!("unknown model mode: {other}"))),
    }
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    drop(log);
    detail_of(state, &agg.run_id).ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "run missing after execution".into(),
        )
    })
}

async fn get_run(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match detail_of(&state, &RunId::new(id)) {
        Some(d) => Json(d).into_response(),
        None => err(StatusCode::NOT_FOUND, "run not found"),
    }
}

async fn run_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let run_id = RunId::new(id);
    let stream = async_stream::try_stream! {
        let events = snapshot_events(&state.log_path);
        for ev in events.iter().filter(|e| e.run_id == run_id) {
            let line = serde_json::to_string(ev).unwrap_or_default();
            yield Event::default().id(ev.sequence.to_string()).event("consensus").data(line);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        yield Event::default().event("replay_complete").data("{}");
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn decide(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DecisionRequest>,
) -> Response {
    let run_id = RunId::new(id);
    let clock = SystemClock;
    let now = consensus_runtime::Clock::now(&clock);
    let auth = HumanAuthorization {
        schema_version: SCHEMA_VERSION.into(),
        run_id: run_id.clone(),
        authorized_by: req.authorized_by,
        approved: req.approved,
        decided_at: now.clone(),
        idempotency_key: req.idempotency_key,
        note: req.note,
    };
    let mut log = match state.owner.lock() {
        Ok(g) => g,
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "event log lock poisoned"),
    };
    match record_human_decision(&mut log, auth, now) {
        Ok(agg) => Json(summary_of(&agg)).into_response(),
        Err(e) => err(StatusCode::CONFLICT, e.to_string()),
    }
}

async fn receipt(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let run_id = RunId::new(&id);
    let log = match state.owner.lock() {
        Ok(g) => g,
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "event log lock poisoned"),
    };
    match build_receipt(&log, &run_id, consensus_runtime::Clock::now(&SystemClock)) {
        Ok(r) => {
            if r.event_count == 0 {
                return err(StatusCode::NOT_FOUND, "run not found");
            }
            let body = serde_json::to_string_pretty(&r).unwrap_or_default();
            (
                [
                    (header::CONTENT_TYPE, "application/json".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"receipt-{id}.json\""),
                    ),
                ],
                body,
            )
                .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let bind = std::env::var("CONSENSUS_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    let log_path = PathBuf::from(
        std::env::var("CONSENSUS_EVENT_LOG").unwrap_or_else(|_| "demo-run/events.jsonl".into()),
    );
    let log = EventLog::open(&log_path)?;
    if log.recovered_truncated_tail {
        eprintln!(
            "[recovery] discarded truncated tail record in {}",
            log_path.display()
        );
    }
    let state = AppState {
        owner: Arc::new(Mutex::new(log)),
        log_path,
    };
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!("Hyphae Consensus console: http://{bind}/");
    println!("Executor deferred: approval records decisions only.");
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(name: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("hyphae-consensus-srv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join(name);
        let _ = std::fs::remove_file(&log_path);
        let log = EventLog::open(&log_path).unwrap();
        AppState {
            owner: Arc::new(Mutex::new(log)),
            log_path,
        }
    }

    #[test]
    fn fake_run_then_decision_via_handlers_logic() {
        let state = temp_state("server-test.jsonl");
        let detail = execute_run(
            &state,
            CreateRunRequest {
                model: "fake".into(),
                memory: "fake".into(),
                title: None,
                description: None,
                project_id: None,
                submitted_by: "mario".into(),
            },
        )
        .unwrap();
        assert_eq!(detail.summary.state, RunState::AwaitingHumanApproval);
        assert_eq!(
            detail.summary.recommendation,
            Some(RecommendationKind::Supported)
        );
        assert!(!detail.events.is_empty());

        // Second run in the same log gets distinct ids (UuidIdGen).
        let detail2 = execute_run(
            &state,
            CreateRunRequest {
                model: "fake".into(),
                memory: "fake".into(),
                title: None,
                description: None,
                project_id: None,
                submitted_by: "mario".into(),
            },
        )
        .unwrap();
        assert_ne!(detail.summary.run_id, detail2.summary.run_id);
        let summaries = aggregates(&snapshot_events(&state.log_path));
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn meta_mode_without_credentials_is_explicit_error() {
        // Only meaningful when the test environment has no key; skip if set.
        if MetaConfig::from_env().is_some() {
            return;
        }
        let state = temp_state("server-meta-test.jsonl");
        let result = execute_run(
            &state,
            CreateRunRequest {
                model: "meta".into(),
                memory: "fake".into(),
                title: None,
                description: None,
                project_id: None,
                submitted_by: "mario".into(),
            },
        );
        match result {
            Err((StatusCode::BAD_REQUEST, msg)) => assert!(msg.contains("META_API_KEY"), "{msg}"),
            other => panic!(
                "expected 400 with credential guidance, got {:?}",
                other.map(|d| d.summary)
            ),
        }
    }
}
