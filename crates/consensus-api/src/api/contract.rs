// SPDX-License-Identifier: Apache-2.0
//! The frozen contract (C8): one JSON Schema per API type and SSE frame,
//! fixtures produced by running the five guided scenarios through the
//! same view builders the handlers use (fixed clock, sequential ids,
//! deterministic run ids), and the `contract_sha256` that `/health`
//! reports: SHA-256 over the sorted schema files.
//!
//! `cargo run -p consensus-api --example export_contract` writes
//! `contracts/api-v1/`; a test regenerates everything in memory and fails
//! on any drift with [`EXPORT_HINT`].

use super::error::{ApiError, ErrorCode};
use super::types::*;
use super::v1;
use crate::guided;
use crate::live::gate;
use crate::public_view::{self, Surface};
use crate::showcase::Showcase;
use crate::stream::replay_frames;
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::scenarios::TS;
use consensus_runtime::{build_receipt_v2, EventLog, Orchestrator, RunControl, RunLimits};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What to do when the committed contract differs from the code.
pub const EXPORT_HINT: &str = "run cargo run -p consensus-api --example export_contract";

/// The schemas, keyed by type name.
pub fn schemas() -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    fn add<T: JsonSchema>(out: &mut BTreeMap<String, Value>, name: &str) {
        let schema = schemars::schema_for!(T);
        out.insert(
            name.to_string(),
            serde_json::to_value(schema).expect("schema serializes"),
        );
    }
    add::<ApiError>(&mut out, "ApiError");
    add::<CreateGuidedRunRequest>(&mut out, "CreateGuidedRunRequest");
    add::<CreateLiveRunRequest>(&mut out, "CreateLiveRunRequest");
    add::<CreateOperatorRunRequest>(&mut out, "CreateOperatorRunRequest");
    add::<DecisionRequest>(&mut out, "DecisionRequest");
    add::<ListRunsQuery>(&mut out, "ListRunsQuery");
    add::<EventsQuery>(&mut out, "EventsQuery");
    add::<RunCreatedV1>(&mut out, "RunCreatedV1");
    add::<RunSummaryV1>(&mut out, "RunSummaryV1");
    add::<RunDetailV1>(&mut out, "RunDetailV1");
    add::<RunListV1>(&mut out, "RunListV1");
    add::<DecisionResponseV1>(&mut out, "DecisionResponseV1");
    add::<CancelResponseV1>(&mut out, "CancelResponseV1");
    add::<EventV1>(&mut out, "EventV1");
    add::<ProvenanceV1>(&mut out, "ProvenanceV1");
    add::<SseFrameV1>(&mut out, "SseFrameV1");
    add::<ConsensusFrameV1>(&mut out, "ConsensusFrameV1");
    add::<StreamEndV1>(&mut out, "StreamEndV1");
    add::<ResyncV1>(&mut out, "ResyncV1");
    add::<SessionV1>(&mut out, "SessionV1");
    add::<ScenariosV1>(&mut out, "ScenariosV1");
    add::<CapabilitiesV1>(&mut out, "CapabilitiesV1");
    add::<HealthV1>(&mut out, "HealthV1");
    add::<ReadyV1>(&mut out, "ReadyV1");
    add::<ShowcaseV1>(&mut out, "ShowcaseV1");
    add::<ShowcaseEntryV1>(&mut out, "ShowcaseEntryV1");
    add::<RunReceiptV2>(&mut out, "RunReceiptV2");
    add::<PolicyTrace>(&mut out, "PolicyTrace");
    out
}

/// Pretty JSON with LF line endings and a trailing newline.
fn render_json<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("JSON serializes");
    text.push('\n');
    text
}

/// SHA-256 over the sorted schema files: `<name>\n<content>` for each.
pub fn contract_sha256_of(schemas: &BTreeMap<String, Value>) -> String {
    let mut hasher = Sha256::new();
    for (name, schema) in schemas {
        hasher.update(name.as_bytes());
        hasher.update(b"\n");
        hasher.update(render_json(schema).as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn contract_sha256() -> String {
    contract_sha256_of(&schemas())
}

/// `research/` at the repository root, for the showcase fixture.
pub fn research_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the repository root")
        .join("research")
}

/// `contracts/api-v1/` at the repository root.
pub fn contracts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the repository root")
        .join("contracts")
        .join("api-v1")
}

/// A deterministic run id that `parse_run_id` accepts.
fn fixture_run_id(n: u32) -> RunId {
    RunId::new(format!("run-00000000-0000-4000-8000-{n:012}"))
}

struct FixtureRun {
    events: Vec<ConsensusEvent>,
}

/// Runs a guided scenario deterministically into a temporary log.
fn deliberate(
    dir: &Path,
    n: u32,
    scenario: GuidedScenario,
    decide: Option<bool>,
) -> Result<FixtureRun, String> {
    let path = dir.join(format!("{}.jsonl", scenario.as_str()));
    let mut log = EventLog::open(&path).map_err(|e| e.to_string())?;
    let mut s = guided::build(scenario);
    s.input.mode = Some(guided::task_mode(scenario, RunOrigin::Public));
    s.input.submitted_by = "visitor:fixture0000000".into();
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let run_id = fixture_run_id(n);
    let (agg, task) = orch
        .create_task(&mut log, run_id.clone(), s.input)
        .map_err(|e| e.to_string())?;
    orch.deliberate(&mut log, agg, &task, &RunControl::new())
        .map_err(|e| e.to_string())?;
    if let Some(approved) = decide {
        let key = if approved {
            "fixture-approve"
        } else {
            "fixture-reject"
        };
        let authorization = HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: run_id.clone(),
            authorized_by: "visitor:fixture0000000".into(),
            approved,
            decided_at: TS.into(),
            idempotency_key: key.into(),
            note: Some("fixture decision on a simulated run".into()),
            authority: Some(Authority::PublicDemo),
        };
        log.append(NewEvent {
            event_id: EventId::new(format!("human-decision:{run_id}:{key}")),
            run_id: run_id.clone(),
            actor: Actor::Human {
                id: "visitor:fixture0000000".into(),
            },
            occurred_at: TS.into(),
            payload: EventPayload::HumanDecisionRecorded { authorization },
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(FixtureRun {
        events: log.all_events().to_vec(),
    })
}

/// Every fixture, keyed by file name (without `.json`).
pub fn fixtures() -> Result<BTreeMap<String, Value>, String> {
    let dir = std::env::temp_dir().join(format!(
        "hyphae-contract-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = fixtures_in(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn fixtures_in(dir: &Path) -> Result<BTreeMap<String, Value>, String> {
    let mut out = BTreeMap::new();
    let source = SourceProvenance::ThisServer;
    let to_value = |v: &dyn erased::Erased| v.to_value();

    for (i, scenario) in GuidedScenario::ALL.iter().enumerate() {
        // The reference scenario also gets a recorded public decision.
        let decide = (*scenario == GuidedScenario::Retry).then_some(true);
        let run = deliberate(dir, i as u32 + 1, *scenario, decide)?;
        let agg = public_view::aggregate_of(&run.events).ok_or("empty run")?;
        let name = scenario.as_str();
        let detail = public_view::detail(&run.events, &agg, source, false, Surface::Public);
        out.insert(format!("run-detail-{name}"), to_value(&detail));
        out.insert(
            format!("run-summary-{name}"),
            to_value(&public_view::summary(
                &run.events,
                &agg,
                source,
                false,
                Surface::Public,
            )),
        );
        let receipt = build_receipt_v2(&run.events, TS.into()).map_err(|e| e.to_string())?;
        out.insert(
            format!("receipt-v2-{name}"),
            to_value(&public_view::sanitize_receipt(receipt, Surface::Public)),
        );
        if *scenario == GuidedScenario::Retry {
            // The SSE frames of the deliberation (before the decision).
            let deliberation: Vec<ConsensusEvent> = run
                .events
                .iter()
                .take_while(|e| !matches!(e.payload, EventPayload::HumanDecisionRecorded { .. }))
                .cloned()
                .collect();
            // A finished run's replay: no frame says active.
            let frames = replay_frames(&deliberation, false, Surface::Public, source);
            out.insert("sse-frames-retry".into(), to_value(&frames));
            // The 202 of creation: only the task exists.
            let created = public_view::aggregate_of(&run.events[..1]).ok_or("empty run")?;
            let summary =
                public_view::summary(&run.events[..1], &created, source, true, Surface::Public);
            out.insert(
                "run-created-retry".into(),
                to_value(&RunCreatedV1 {
                    links: RunLinksV1::for_run(&summary.run_id),
                    run: summary,
                }),
            );
            // The operator view of the same run keeps the extra fields.
            out.insert(
                "run-detail-retry-operator".into(),
                to_value(&public_view::detail(
                    &run.events,
                    &agg,
                    source,
                    false,
                    Surface::Operator,
                )),
            );
        }
    }

    // A public live run at creation: mode {meta, sealed_snapshot, public,
    // preset}, model undetermined until the first recorded Meta call.
    {
        let run = live_created(dir)?;
        let created = public_view::aggregate_of(&run.events).ok_or("empty run")?;
        let summary = public_view::summary(&run.events, &created, source, true, Surface::Public);
        out.insert(
            "run-created-live-R01-reference".into(),
            to_value(&RunCreatedV1 {
                links: RunLinksV1::for_run(&summary.run_id),
                run: summary.clone(),
            }),
        );
        out.insert("run-summary-live-R01-reference".into(), to_value(&summary));
        out.insert(
            "run-detail-live-R01-reference".into(),
            to_value(&public_view::detail(
                &run.events,
                &created,
                source,
                true,
                Surface::Public,
            )),
        );
    }

    out.insert(
        "api-error-samples".into(),
        to_value(&vec![
            ApiError::new(ErrorCode::InvalidRequest, "invalid request: unknown field `title`"),
            ApiError::new(ErrorCode::CsrfRejected, "the Origin header is not allowed"),
            ApiError::new(ErrorCode::SessionRequired, "a session is required: POST /api/v1/session first"),
            ApiError::not_found(),
            ApiError::new(ErrorCode::RateLimited, "too many requests from this client").with_retry_after(30),
            ApiError::new(ErrorCode::TooManyRuns, "every deliberation slot is busy; retry later").with_retry_after(5),
            ApiError::new(ErrorCode::LiveDisabled, "the public live mode is not enabled on this server"),
            ApiError::new(ErrorCode::RunNotAwaitingDecision, "the run is not awaiting a human decision"),
            ApiError::new(ErrorCode::DecisionConflict, "expected last sequence 14, the run is at 15"),
            ApiError::new(ErrorCode::IdempotencyKeyReused, "this Idempotency-Key was already used with a different decision"),
            ApiError::new(ErrorCode::OverrideNoteRequired, "approving a blocked or abstained run is an override and requires a note"),
            ApiError::new(ErrorCode::RunNotCancellable, "only a run that is still deliberating can be cancelled"),
            ApiError::new(ErrorCode::StorageFull, "the server holds its maximum number of visitor runs and none can be freed right now; retry shortly").with_retry_after(30),
            ApiError::new(ErrorCode::NdjsonUnavailable, "the verbatim event log of a live run is not published on the public surface; the detail and the receipt carry its sanitized view"),
            ApiError::new(ErrorCode::RequestTimeout, "the request was not handled within the server's request timeout"),
            ApiError::new(ErrorCode::PresetUnknown, gate::refusal_message(ErrorCode::PresetUnknown)),
            ApiError::new(ErrorCode::GeoBlockedPortal, crate::api::geo_message(ErrorCode::GeoBlockedPortal)),
            ApiError::new(ErrorCode::GeoBlockedLive, crate::api::geo_message(ErrorCode::GeoBlockedLive)),
            ApiError::new(ErrorCode::GeoUnknown, crate::api::geo_message(ErrorCode::GeoUnknown)),
            ApiError::new(ErrorCode::AttestationRequired, gate::refusal_message(ErrorCode::AttestationRequired)),
            ApiError::new(ErrorCode::TurnstileRequired, gate::refusal_message(ErrorCode::TurnstileRequired)),
            ApiError::new(ErrorCode::TurnstileFailed, gate::refusal_message(ErrorCode::TurnstileFailed)),
            ApiError::new(ErrorCode::TurnstileUnavailable, gate::refusal_message(ErrorCode::TurnstileUnavailable)).with_retry_after(5),
            ApiError::new(ErrorCode::LiveBudgetDaily, gate::refusal_message(ErrorCode::LiveBudgetDaily)).with_retry_after(1800),
            ApiError::new(ErrorCode::LiveBudgetLifetime, gate::refusal_message(ErrorCode::LiveBudgetLifetime)),
            ApiError::new(ErrorCode::LiveConcurrency, gate::refusal_message(ErrorCode::LiveConcurrency)).with_retry_after(30),
            ApiError::new(ErrorCode::LiveHourlyAdmissions, gate::refusal_message(ErrorCode::LiveHourlyAdmissions)).with_retry_after(600),
            ApiError::new(ErrorCode::LiveTripped, gate::refusal_message(ErrorCode::LiveTripped)),
            ApiError::new(ErrorCode::LiveUnavailable, gate::refusal_message(ErrorCode::LiveUnavailable)).with_retry_after(30),
        ]),
    );
    out.insert(
        "health".into(),
        to_value(&HealthV1 {
            status: "ok".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            schema_version: SCHEMA_VERSION.into(),
            policy_version: v1::policy_version_label(),
            contract_sha256: contract_sha256(),
            executor: "disabled".into(),
        }),
    );
    out.insert(
        "capabilities-public".into(),
        to_value(&default_capabilities()),
    );
    out.insert(
        "scenarios".into(),
        to_value(&ScenariosV1 {
            scenarios: GuidedScenario::ALL
                .iter()
                .map(|s| guided::scenario_v1(*s))
                .collect(),
        }),
    );
    out.insert(
        "session".into(),
        to_value(&SessionV1 {
            created: true,
            expires_at: "2026-10-27T09:00:00Z".into(),
            pseudonym: "visitor:0123456789abcdef".into(),
        }),
    );

    let showcase = Showcase::load(&research_dir());
    if let Some(entry) = showcase.list().entries.iter().find(|e| e.label == "F03") {
        out.insert("showcase-entry-F03".into(), to_value(entry));
    }
    Ok(out)
}

/// The public capabilities under the default configuration.
fn default_capabilities() -> CapabilitiesV1 {
    use consensus_runtime::receipts::protocol_label;
    CapabilitiesV1 {
        surface: SurfaceV1::Public,
        live_enabled: false,
        guided_enabled: true,
        showcase_enabled: true,
        limits: LimitsV1 {
            body_limit_bytes: crate::public_config::BODY_LIMIT_BYTES as u64,
            guided_concurrency: 4,
            live_concurrency: 1,
            run_deadline_secs: 900,
            max_run_tokens: RunLimits::default().max_run_tokens,
            sse_max_per_run: 8,
            sse_max_per_client: 8,
            sse_max_global: 512,
            sse_heartbeat_secs: 15,
            rate_session_per_min: 10,
            rate_runs_per_min: 6,
            rate_sse_per_min: 30,
            session_ttl_secs: 86_400,
            session_max: 50_000,
            guided_ring: 2000,
            visitor_access_hours: 24,
            visitor_delete_days: 7,
            list_page_max: v1::LIST_PAGE_MAX as u64,
            request_timeout_secs: 30,
        },
        schema_version: SCHEMA_VERSION.into(),
        policy_version: v1::policy_version_label(),
        protocol: protocol_label(SCHEMA_VERSION),
        decision_authority: Authority::PublicDemo,
        live: None,
    }
}

/// The 202 of a public live run: only its task exists, declared with the
/// production live mode (the owner's Meta key, the sealed snapshot, the
/// public origin, the preset). Deterministic: the task is created with
/// the fixed clock and no model is called.
fn live_created(dir: &Path) -> Result<FixtureRun, String> {
    let preset = crate::live::presets::PresetId::R01Reference.preset();
    let path = dir.join("live-R01-reference.jsonl");
    let mut log = EventLog::open(&path).map_err(|e| e.to_string())?;
    // Any ports do for task creation; nothing is called.
    let s = guided::build(GuidedScenario::Retry);
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let mut input = preset.task_input();
    input.submitted_by = "visitor:fixture0000000".into();
    orch.create_task(&mut log, fixture_run_id(6), input)
        .map_err(|e| e.to_string())?;
    Ok(FixtureRun {
        events: log.all_events().to_vec(),
    })
}

/// Type-erased serialization helper so one closure handles every fixture.
mod erased {
    use serde::Serialize;
    use serde_json::Value;

    pub trait Erased {
        fn to_value(&self) -> Value;
    }

    impl<T: Serialize> Erased for T {
        fn to_value(&self) -> Value {
            serde_json::to_value(self).expect("fixture serializes")
        }
    }
}

/// Every file of `contracts/api-v1/`, keyed by its relative path with
/// `/` separators, as LF text.
pub fn render_files() -> Result<BTreeMap<String, String>, String> {
    let schemas = schemas();
    let mut files = BTreeMap::new();
    for (name, schema) in &schemas {
        files.insert(format!("{name}.schema.json"), render_json(schema));
    }
    for (name, fixture) in fixtures()? {
        files.insert(format!("fixtures/{name}.json"), render_json(&fixture));
    }
    files.insert(
        "contract-sha256.txt".into(),
        format!("{}\n", contract_sha256_of(&schemas)),
    );
    files.insert(
        "README.md".into(),
        "# API v1 contract\n\nGenerated: do not edit. `*.schema.json` is one JSON Schema per request, response or SSE frame type of `/api/v1`; `fixtures/` holds responses produced by the real view builders over the five guided scenarios (fixed clock, sequential ids). `contract-sha256.txt` is the SHA-256 over the sorted schema files, reported by `GET /api/v1/health`.\n\nRegenerate with `cargo run -p consensus-api --example export_contract`; `cargo test -p consensus-api --test api_contract` fails on drift.\n".into(),
    );
    Ok(files)
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Compares the committed contract with a fresh render. Returns the list
/// of differing or missing files.
pub fn drift(dir: &Path) -> Result<Vec<String>, String> {
    let rendered = render_files()?;
    let mut problems = Vec::new();
    for (rel, expected) in &rendered {
        let path = dir.join(rel);
        match std::fs::read_to_string(&path) {
            Ok(actual) if normalize(&actual) == normalize(expected) => {}
            Ok(_) => problems.push(format!("{rel}: differs")),
            Err(_) => problems.push(format!("{rel}: missing")),
        }
    }
    // Stale files: anything in the directory the render no longer makes.
    for sub in ["", "fixtures"] {
        let folder = if sub.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(sub)
        };
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if sub.is_empty() {
                name
            } else {
                format!("{sub}/{name}")
            };
            if !rendered.contains_key(&rel) {
                problems.push(format!("{rel}: stale (not generated any more)"));
            }
        }
    }
    Ok(problems)
}

/// Writes the contract to `dir`, removing stale generated files.
pub fn export(dir: &Path) -> Result<Vec<String>, String> {
    let rendered = render_files()?;
    std::fs::create_dir_all(dir.join("fixtures")).map_err(|e| e.to_string())?;
    for sub in ["", "fixtures"] {
        let folder = if sub.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(sub)
        };
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = if sub.is_empty() {
                    name
                } else {
                    format!("{sub}/{name}")
                };
                if !rendered.contains_key(&rel) {
                    std::fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    let mut written = Vec::new();
    for (rel, content) in &rendered {
        std::fs::write(dir.join(rel), content.as_bytes()).map_err(|e| format!("{rel}: {e}"))?;
        written.push(rel.clone());
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_deterministic() {
        let a = fixtures().unwrap();
        let b = fixtures().unwrap();
        assert_eq!(a, b);
        assert!(a.contains_key("run-detail-retry"));
        assert!(a.contains_key("sse-frames-retry"));
        let frames = a["sse-frames-retry"].as_array().unwrap();
        assert_eq!(frames.last().unwrap()["frame"], "stream_end");
        assert!(frames
            .iter()
            .all(|f| f["frame"] != "consensus" || f["delivery"] == "replay"));
    }

    #[test]
    fn the_contract_hash_covers_every_schema() {
        let schemas = schemas();
        assert!(schemas.len() >= 28);
        let hash = contract_sha256_of(&schemas);
        assert_eq!(hash.len(), 64);
        let mut fewer = schemas.clone();
        fewer.remove("HealthV1");
        assert_ne!(contract_sha256_of(&fewer), hash);
    }
}
