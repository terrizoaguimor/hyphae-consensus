// SPDX-License-Identifier: Apache-2.0
//! The Evaluator's per-call keyed schema, end to end through the loopback
//! fake and the orchestrator (audit finding MA-1; live run R03 of
//! 2026-10-27, whose Evaluator returned an empty `assessments` list in
//! both rounds under strict mode, so every claim was completed as
//! uncorroborated and the run ended insufficient_evidence). The evaluate
//! request's schema requires exactly this call's claim ids and item ids,
//! in proposal and prompt order; a complete answer in any key order is
//! mapped into those orders; a missing key or an unknown key (served by
//! the fake as if the API ignored strict mode) is malformed output naming
//! it, and the refused call stays on the record; and the per-call schema
//! keeps every strict-schema invariant.

mod support;

use consensus_api::meta::{MetaModelClient, Role};
use consensus_domain::*;
use consensus_runtime::clock::{FixedClock, SequentialIdGen};
use consensus_runtime::fakes::{FakeMemoryClient, FakeProofVerifier};
use consensus_runtime::orchestrator::{build_receipt, Orchestrator, TaskInput};
use consensus_runtime::{EventLog, RunAggregate, RunLimits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::*;

/// Nine claims, so that sorted ids differ from proposal order: the
/// proposal mints prop-1 and claim-2 .. claim-10 (a sorted map would put
/// "claim-10" first); the critique mints item-11, item-12 and chal-13 (the
/// adapter numbers every id it assigns in turn).
const CLAIM_IDS: [&str; 9] = [
    "claim-2", "claim-3", "claim-4", "claim-5", "claim-6", "claim-7", "claim-8", "claim-9",
    "claim-10",
];
const ITEM_IDS: [&str; 2] = ["item-11", "item-12"];

fn proposal_draft() -> String {
    let claims: Vec<Value> = (1..=9)
        .map(|n| {
            json!({"statement": format!("Retries are capped at 3 attempts with jitter (part {n})")})
        })
        .collect();
    json!({"summary": "Cap retries at 3 with jitter", "plan_steps": ["Set the cap", "Add jitter"],
        "claims": claims, "assumptions": [], "uncertainties": []})
    .to_string()
}

const CRITIQUE: &str = r#"{"items":[{"kind":"missing_test","claim_id":"claim-2","description":"No test covers the attempt cap","evidence_refs":[]},{"kind":"unsupported_claim","claim_id":"claim-10","description":"No record states the jitter floor","evidence_refs":[]}]}"#;

fn records() -> Vec<EvidenceReference> {
    vec![
        consensus_runtime::scenarios::evidence(
            "ev-1",
            "demo-repo",
            "constraint",
            "Constraint: outbound retries must be capped at 3 attempts with jitter.",
        ),
        consensus_runtime::scenarios::evidence(
            "ev-2",
            "demo-repo",
            "decision",
            "Decision: payment retries use a jitter of at least 50 ms.",
        ),
    ]
}

/// A keyed draft entry, `(id, body)`: the id is the key the per-call
/// schema requires, never a field of the body.
type Entry = (String, Value);

fn assessment(claim: &str, corroborated: bool, relevant: &[&str]) -> Entry {
    (
        claim.to_string(),
        json!({"corroborated": corroborated, "relevant_evidence": relevant,
            "contradicting_evidence": [], "note": format!("scripted for {claim}")}),
    )
}

fn verdict(item: &str, status: &str) -> Entry {
    (
        item.to_string(),
        json!({"status": status, "reason": format!("scripted for {item}")}),
    )
}

/// `entries` as a JSON object in that order (a repeated key written
/// twice; a `serde_json` map would collapse it).
fn object(entries: &[Entry]) -> String {
    let members: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{}:{value}", Value::String(key.clone())))
        .collect();
    format!("{{{}}}", members.join(","))
}

fn evaluation(assessments: &[Entry], verdicts: &[Entry]) -> String {
    format!(
        r#"{{"assessments":{},"item_verdicts":{},"cleared_contradictions":[],"abstain":false,"abstain_reason":null}}"#,
        object(assessments),
        object(verdicts)
    )
}

fn every_assessment() -> Vec<Entry> {
    CLAIM_IDS
        .iter()
        .map(|claim| assessment(claim, true, &["ev-1"]))
        .collect()
}

fn every_verdict() -> Vec<Entry> {
    ITEM_IDS
        .iter()
        .map(|item| verdict(item, "resolved"))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One deliberation through the real adapter and orchestrator (scripted
/// memory, every proof Valid); the fake answers the run's model calls in
/// order with `drafts`.
fn deliberate(name: &str, drafts: &[String]) -> (RunAggregate, EventLog, FakeServer) {
    let server = serve(drafts.iter().map(|d| Reply::ok(envelope(d))).collect());
    let mut orch = Orchestrator {
        model: MetaModelClient::new(config(&server.base_url)).unwrap(),
        memory: FakeMemoryClient::new(records()),
        verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
        clock: FixedClock::new("2026-10-27T09:00:00Z"),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let path = temp_dir(&format!("meta-keyed-{name}")).join("events.jsonl");
    let mut log = EventLog::open(&path).unwrap();
    let input = TaskInput {
        project_id: ProjectId::new("demo-repo"),
        title: "Fix payment retries".into(),
        description: "Retries burst under load.".into(),
        submitted_by: "test".into(),
        mode: None,
    };
    let agg = orch.run(&mut log, input).unwrap();
    (agg, log, server)
}

/// (stage, code, reason) of the log's last record, which must be RunFailed.
fn run_failure(log: &EventLog) -> (String, Option<String>, String) {
    match &log.all_events().last().expect("a record").payload {
        EventPayload::RunFailed {
            stage,
            code,
            reason,
            ..
        } => (stage.clone(), code.clone(), reason.clone()),
        other => panic!("expected RunFailed last, got {other:?}"),
    }
}

fn recorded_evaluations(log: &EventLog) -> Vec<Evaluation> {
    log.all_events()
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::EvaluationSubmitted { evaluation, .. } => Some(evaluation.clone()),
            _ => None,
        })
        .collect()
}

fn required(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .unwrap_or_else(|| panic!("no required list in {schema}"))
        .iter()
        .map(|v| v.as_str().expect("a property name").to_string())
        .collect()
}

/// The strict-schema invariants at every object schema of `schema`
/// (definitions included): `required` names every property exactly once
/// and nothing else, and no other property is allowed.
fn assert_strict(schema: &Value) {
    match schema {
        Value::Object(map) => {
            if let Some(properties) = map.get("properties").and_then(|p| p.as_object()) {
                let names: Vec<&str> = properties.keys().map(String::as_str).collect();
                let mut required: Vec<&str> = map["required"]
                    .as_array()
                    .unwrap_or_else(|| panic!("no required list in {schema}"))
                    .iter()
                    .map(|v| v.as_str().expect("a property name"))
                    .collect();
                required.sort_unstable();
                assert_eq!(required, names, "required = properties in {schema}");
                assert_eq!(map["additionalProperties"], json!(false), "{schema}");
            }
            map.values().for_each(assert_strict);
        }
        Value::Array(items) => items.iter().for_each(assert_strict),
        _ => {}
    }
}

/// The raw text of the schema inside a captured request, exactly as sent
/// (a parsed `Value` would re-sort its keys): what `schema_sha256` digests.
fn raw_schema(request: &str) -> &str {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .expect("a body");
    let marker = "\"json_schema\":{\"name\":\"DraftEvaluation\",\"schema\":";
    let start = body.find(marker).expect("the evaluate schema") + marker.len();
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (n, &b) in body.as_bytes()[start..].iter().enumerate() {
        match (in_string, b) {
            (true, b'\\') if !escaped => escaped = true,
            (true, b'"') if !escaped => in_string = false,
            (true, _) => escaped = false,
            (false, b'"') => in_string = true,
            (false, b'{') => depth += 1,
            (false, b'}') => {
                depth -= 1;
                if depth == 0 {
                    return &body[start..=start + n];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced schema in {body}")
}

#[test]
fn the_evaluate_request_is_keyed_by_this_calls_claim_and_item_ids() {
    let complete = evaluation(&every_assessment(), &every_verdict());
    let (agg, log, server) = deliberate("request", &[proposal_draft(), CRITIQUE.into(), complete]);
    assert_eq!(server.hits(), 3);
    let requests = server.requests();
    let request = &requests[2];
    let body = body_of(request);
    let format = &body["response_format"]["json_schema"];
    assert_eq!(format["name"], json!("DraftEvaluation"));
    assert_eq!(format["strict"], json!(true));
    let schema = &format["schema"];

    // Exactly the proposal's claim ids, each required, no other allowed,
    // each an assessment; the claim id is the key, not a field.
    let assessments = &schema["properties"]["assessments"];
    assert_eq!(required(assessments), CLAIM_IDS);
    let mut keys: Vec<&str> = assessments["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut expected = CLAIM_IDS.to_vec();
    keys.sort_unstable();
    expected.sort_unstable();
    assert_eq!(keys, expected);
    assert_eq!(assessments["additionalProperties"], json!(false));
    for id in CLAIM_IDS {
        assert_eq!(
            assessments["properties"][id],
            json!({"$ref": "#/definitions/DraftAssessment"}),
            "{id}"
        );
    }
    assert!(schema["definitions"]["DraftAssessment"]["properties"]
        .get("claim_id")
        .is_none());
    // Exactly the objections under evaluation, in the order shown.
    let verdicts = &schema["properties"]["item_verdicts"];
    assert_eq!(required(verdicts), ITEM_IDS);
    assert_eq!(verdicts["additionalProperties"], json!(false));
    for id in ITEM_IDS {
        assert_eq!(
            verdicts["properties"][id],
            json!({"$ref": "#/definitions/DraftItemVerdict"}),
            "{id}"
        );
    }
    assert!(schema["definitions"]["DraftItemVerdict"]["properties"]
        .get("item_id")
        .is_none());
    // Clearing a carried contradiction stays optional: still a list.
    assert_eq!(
        schema["properties"]["cleared_contradictions"]["type"],
        json!("array")
    );
    // The per-call schema is still a strict schema everywhere.
    assert_strict(schema);
    assert_eq!(
        schema["properties"]["abstain_reason"]["type"],
        json!(["string", "null"])
    );

    // As sent, the properties follow proposal order (claim-10 last, where
    // a sorted map would put it first) and prompt order, and the recorded
    // digest is of exactly these bytes.
    let raw = raw_schema(request);
    let at = |needle: &str| {
        raw.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing in {raw}"))
    };
    for pair in CLAIM_IDS.windows(2) {
        assert!(
            at(&format!("\"{}\":{{\"$ref\"", pair[0])) < at(&format!("\"{}\":{{\"$ref\"", pair[1])),
            "{} before {} in {raw}",
            pair[0],
            pair[1]
        );
    }
    assert!(at("\"item-11\":{\"$ref\"") < at("\"item-12\":{\"$ref\""));
    let evaluate_call = &agg.invocations[2];
    assert_eq!(
        evaluate_call.schema_name.as_deref(),
        Some("DraftEvaluation")
    );
    assert_eq!(
        evaluate_call.schema_sha256,
        Some(hex(&Sha256::digest(raw.as_bytes())))
    );
    assert_eq!(
        evaluate_call.prompt_sha256,
        Some(Role::Evaluate.prompt_sha256())
    );
    // The prompt names the same ids in the same order.
    let prompt = body["messages"][1]["content"].as_str().unwrap();
    assert!(
        prompt.contains(&format!(
            "\nAssess these claims, one assessment each, keyed by claim id: {}\n",
            CLAIM_IDS.join(", ")
        )),
        "{prompt}"
    );
    // Every claim corroborated and every objection resolved: supported.
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

#[test]
fn a_complete_answer_in_any_key_order_is_mapped_in_proposal_and_prompt_order() {
    let mut assessments = every_assessment();
    assessments.reverse();
    let mut verdicts = every_verdict();
    verdicts.reverse();
    let (agg, log, server) = deliberate(
        "order",
        &[
            proposal_draft(),
            CRITIQUE.into(),
            evaluation(&assessments, &verdicts),
        ],
    );
    assert_eq!(server.hits(), 3);
    let recorded = recorded_evaluations(&log);
    assert_eq!(recorded.len(), 1);
    let evaluation = &recorded[0];
    let claims: Vec<&str> = evaluation
        .assessments
        .iter()
        .map(|a| a.claim_id.as_str())
        .collect();
    assert_eq!(claims, CLAIM_IDS);
    let items: Vec<&str> = evaluation
        .item_verdicts
        .iter()
        .map(|v| v.item_id.as_str())
        .collect();
    assert_eq!(items, ITEM_IDS);
    // Each entry is the one written under its key, nothing completed.
    for a in &evaluation.assessments {
        assert_eq!(a.note, format!("scripted for {}", a.claim_id.as_str()));
        assert!(a.corroborated);
        assert_eq!(a.relevant_evidence, vec![EvidenceId::new("ev-1")]);
    }
    for v in &evaluation.item_verdicts {
        assert_eq!(v.reason, format!("scripted for {}", v.item_id));
        assert_eq!(v.status, ItemStatus::Resolved);
    }
    assert!(evaluation.unresolved_items.is_empty());
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
    let receipt = build_receipt(&log, &agg.run_id, "2026-10-27T09:00:00Z".into()).unwrap();
    assert_eq!(receipt.model_invocations, agg.invocations);
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg);
}

/// Every run below fails at evaluate with the named fragments in the
/// reason, records no evaluation, keeps the refused (billed) call, and
/// replays.
fn assert_refused_at_evaluate(label: &str, draft: String, named: &[&str]) {
    let name: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let (agg, log, server) = deliberate(&name, &[proposal_draft(), CRITIQUE.into(), draft]);
    assert_eq!(server.hits(), 3, "{label}");
    assert_eq!(agg.state, RunState::Failed, "{label}");
    assert!(agg.recommendation.is_none(), "{label}");
    let (stage, code, reason) = run_failure(&log);
    assert_eq!(
        (stage.as_str(), code.as_deref()),
        ("evaluate", Some("model_malformed_output")),
        "{label}"
    );
    for fragment in named {
        assert!(
            reason.contains(fragment),
            "{label}: {fragment} missing from {reason}"
        );
    }
    assert!(
        recorded_evaluations(&log).is_empty(),
        "{label}: nothing completed, nothing recorded"
    );
    assert!(
        !std::fs::read_to_string(log.path())
            .unwrap()
            .contains("treated as uncorroborated"),
        "{label}: no completion anywhere in the record"
    );
    assert_eq!(
        agg.invocations.len(),
        3,
        "{label}: the refused evaluate call is billed and recorded"
    );
    assert_eq!(log.rebuild(&agg.run_id).unwrap(), agg, "{label}");
}

#[test]
fn a_missing_key_is_malformed_output_naming_the_claim_or_item() {
    // Served as if the API had ignored the schema: the exact shape of live
    // R03 (no assessments at all), one claim left out, one objection left
    // out. Under the list schema the first two ended insufficient_evidence
    // with every omitted claim completed as uncorroborated.
    let mut but_one = every_assessment();
    but_one.remove(5);
    let mut one_verdict = every_verdict();
    one_verdict.pop();
    assert_refused_at_evaluate(
        "no assessments at all",
        evaluation(&[], &every_verdict()),
        &["assessments", "no assessment", "\"claim-2\""],
    );
    assert_refused_at_evaluate(
        "one claim left out",
        evaluation(&but_one, &every_verdict()),
        &["assessments", "no assessment", "\"claim-7\""],
    );
    assert_refused_at_evaluate(
        "one objection left out",
        evaluation(&every_assessment(), &one_verdict),
        &["item_verdicts", "no verdict", "\"item-12\""],
    );
}

#[test]
fn an_unknown_key_is_malformed_output_naming_it() {
    let mut extra_claim = every_assessment();
    extra_claim.push(assessment("claim-99", true, &["ev-1"]));
    let mut cased = every_verdict();
    cased[1] = verdict("Item-12", "resolved");
    let mut claim_as_item = every_verdict();
    claim_as_item.push(verdict("claim-2", "resolved"));
    let mut twice = every_assessment();
    twice.push(assessment("claim-2", false, &[]));
    assert_refused_at_evaluate(
        "a claim id the proposal does not have",
        evaluation(&extra_claim, &every_verdict()),
        &["assessments", "\"claim-99\"", "not a claim of the proposal"],
    );
    assert_refused_at_evaluate(
        "an item id in another case",
        evaluation(&every_assessment(), &cased),
        &[
            "item_verdicts",
            "\"Item-12\"",
            "not an objection under evaluation",
        ],
    );
    assert_refused_at_evaluate(
        "a claim id as an item key",
        evaluation(&every_assessment(), &claim_as_item),
        &[
            "item_verdicts",
            "\"claim-2\"",
            "not an objection under evaluation",
        ],
    );
    assert_refused_at_evaluate(
        "a claim assessed under its key twice",
        evaluation(&twice, &every_verdict()),
        &["assessments", "\"claim-2\"", "more than once"],
    );
}

#[test]
fn a_round_without_objections_keys_an_empty_verdict_object() {
    let (agg, _, server) = deliberate(
        "no-objections",
        &[
            proposal_draft(),
            r#"{"items":[]}"#.into(),
            evaluation(&every_assessment(), &[]),
        ],
    );
    let schema = &body_of(&server.requests()[2])["response_format"]["json_schema"]["schema"];
    let verdicts = &schema["properties"]["item_verdicts"];
    assert_eq!(verdicts["properties"], json!({}));
    assert_eq!(verdicts["required"], json!([]));
    assert_eq!(verdicts["additionalProperties"], json!(false));
    assert_eq!(required(&schema["properties"]["assessments"]), CLAIM_IDS);
    assert_strict(schema);
    assert_eq!(
        agg.recommendation.as_ref().map(|r| r.kind),
        Some(RecommendationKind::Supported)
    );
}
