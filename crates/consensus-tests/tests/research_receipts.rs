// SPDX-License-Identifier: Apache-2.0
//! Receipt v2 over the research archive (`research/`). Every archived log
//! is opened with `EventLog::open_read_only`, and only ever on a temporary
//! copy: the archive itself is never opened by the event log.

use consensus_domain::*;
use consensus_runtime::policy::{rationale_for, PolicyRule, PolicyVersion};
use consensus_runtime::receipts::{build_receipt_v2, protocol_label};
use consensus_runtime::{EventLog, EventLogError};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `research/` at the repository root.
fn research_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the repository root")
        .join("research")
}

fn read(rel: &str) -> Vec<u8> {
    let path = research_dir().join(rel);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn manifest() -> Value {
    serde_json::from_slice(&read("MANIFEST.json")).expect("MANIFEST.json parses")
}

/// String field of a MANIFEST entry.
fn text(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} is not a string in {v}"))
        .to_string()
}

fn entries(m: &Value, key: &str) -> Vec<Value> {
    m[key].as_array().expect("MANIFEST array").clone()
}

/// Serialized name of a unit enum value (e.g. `insufficient_evidence`).
fn serde_name(value: Value) -> String {
    value.as_str().expect("unit enum value").to_string()
}

/// [`serde_name`] of any serializable value.
macro_rules! name {
    ($value:expr) => {
        serde_name(serde_json::to_value($value).unwrap())
    };
}

/// A fresh, empty temporary directory, removed again on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-receipts-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Writes a copy of an archived file and returns the copy's path.
    fn copy(&self, rel: &str) -> PathBuf {
        let copy = self.0.join(rel.replace('/', "__"));
        std::fs::write(&copy, read(rel)).unwrap();
        copy
    }

    /// Opens a copy of every archived log read-only, keyed by its
    /// MANIFEST path.
    fn open_all(&self, m: &Value) -> BTreeMap<String, EventLog> {
        entries(m, "files")
            .iter()
            .filter(|f| f["kind"] == "event_log")
            .map(|f| {
                let rel = text(f, "path");
                let log = EventLog::open_read_only(self.copy(&rel))
                    .unwrap_or_else(|e| panic!("{rel}: {e}"));
                (rel, log)
            })
            .collect()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Per archived run: items raised before the final round that the final
/// evaluation never listed, and how many of them the evaluation of their
/// own round had listed as unresolved. Counted independently from the
/// raw JSONL (2026-10-27). Before protocol 1.3 nothing judged them again.
const NEVER_REJUDGED: [(&str, usize, usize); 40] = [
    ("C01", 2, 2),
    ("C02", 2, 2),
    ("C03", 0, 0),
    ("C04", 0, 0),
    ("C05", 2, 2),
    ("C06", 0, 0),
    ("C07", 0, 0),
    ("L01", 0, 0),
    ("L02", 0, 0),
    ("L03", 0, 0),
    ("L04", 0, 0),
    ("L05", 0, 0),
    ("L06", 0, 0),
    ("L07", 0, 0),
    ("L08", 0, 0),
    ("L09", 0, 0),
    ("L10", 0, 0),
    ("I01", 0, 0),
    ("I02", 0, 0),
    ("S01", 0, 0),
    ("S02", 0, 0),
    ("S03", 0, 0),
    ("P01", 0, 0),
    ("P02", 8, 8),
    ("R01", 5, 5),
    ("R02", 7, 7),
    ("R03", 6, 6),
    ("R04", 0, 0),
    ("R05", 0, 0),
    ("R06", 7, 7),
    ("R07", 7, 7),
    ("R08", 8, 8),
    ("F01", 13, 13),
    ("F02", 6, 6),
    ("F03", 5, 5),
    ("F04", 13, 13),
    ("F05", 6, 6),
    ("F06", 8, 7),
    ("H01", 11, 11),
    ("H02", 9, 9),
];

/// Receipt v2 of every MANIFEST run, keyed by label.
fn receipts(logs: &BTreeMap<String, EventLog>, m: &Value) -> BTreeMap<String, RunReceiptV2> {
    entries(m, "runs")
        .iter()
        .map(|r| {
            let label = text(r, "label");
            let run = RunId::new(text(r, "run_id"));
            let events: Vec<ConsensusEvent> =
                logs[&text(r, "file")].events_for(&run).cloned().collect();
            let receipt = build_receipt_v2(&events, "2026-10-27T00:00:00Z".into())
                .unwrap_or_else(|e| panic!("{label}: {e}"));
            (label, receipt)
        })
        .collect()
}

#[test]
fn every_archived_run_has_a_receipt_v2_consistent_with_its_log() {
    let tmp = TempDir::new("all");
    let m = manifest();
    let logs = tmp.open_all(&m);
    let receipts = receipts(&logs, &m);
    let expected: BTreeMap<&str, (usize, usize)> = NEVER_REJUDGED
        .iter()
        .map(|(label, total, open)| (*label, (*total, *open)))
        .collect();
    assert_eq!(expected.len(), entries(&m, "runs").len());

    for r in entries(&m, "runs") {
        let label = text(&r, "label");
        let run = RunId::new(text(&r, "run_id"));
        let log = &logs[&text(&r, "file")];
        let events: Vec<&ConsensusEvent> = log.events_for(&run).collect();
        let agg = log.rebuild(&run).unwrap();
        let receipt = &receipts[&label];
        let schema = text(&r, "schema_version");

        assert_eq!(receipt.run_id, run, "{label}");
        assert_eq!(receipt.event_count, events.len() as u64, "{label}");
        assert_eq!(receipt.run_schema_version, schema, "{label}");
        assert_eq!(receipt.protocol, protocol_label(&schema), "{label}");
        let label_for_schema = match schema.as_str() {
            "1.0" => "pre-1.1 (not re-derived on replay)",
            "1.1" => "policy 1.1",
            "1.2" => "policy 1.2",
            other => panic!("{label}: unexpected schema {other}"),
        };
        assert_eq!(receipt.protocol, label_for_schema, "{label}");
        let task = receipt.task.as_ref().expect("task");
        assert_eq!(task.title, text(&r, "task_title"), "{label}");
        assert_eq!(task.project_id.as_str(), text(&r, "project"), "{label}");
        assert_eq!(
            name!(receipt.authorization.state),
            text(&r, "authorization"),
            "{label}"
        );
        // A recorded decision reads as `unspecified` authority (no record
        // before schema 1.3 carries one); no decision, no authority.
        assert_eq!(
            receipt.authorization.authority.is_some(),
            receipt.authorization.state != AuthorizationState::Pending,
            "{label}"
        );

        // The outcome MANIFEST records, and the trace that explains it.
        let outcome = text(&r, "outcome");
        if let Some(stage) = outcome.strip_prefix("failed:") {
            assert_eq!(receipt.failure.as_ref().unwrap().stage, stage, "{label}");
            assert!(receipt.recommendation.is_none(), "{label}");
            assert!(receipt.policy_trace.is_none(), "{label}");
        } else {
            let rec = receipt.recommendation.as_ref().expect("recommendation");
            assert_eq!(name!(rec.kind), outcome, "{label}");
            match PolicyVersion::for_schema(&schema) {
                None => assert!(receipt.policy_trace.is_none(), "{label}: pre-1.1"),
                Some(version) => {
                    let trace = receipt.policy_trace.as_ref().expect("trace");
                    assert_eq!(trace.kind, rec.kind, "{label}");
                    assert_eq!(trace.version, version.into(), "{label}");
                    // The recorded rationale is rationale_for's sentence for
                    // the same inputs, byte for byte, and the trace's counts
                    // are its numbers.
                    let eval = agg.last_evaluation.as_ref().unwrap();
                    assert_eq!(
                        rec.rationale,
                        rationale_for(rec.kind, eval, &agg.round_verifications),
                        "{label}"
                    );
                    let c = trace.counts;
                    assert!(
                        rec.rationale.ends_with(&format!(
                            "{}/{} claims corroborated; proofs: {} valid, {} invalid, {} unavailable/error; {} unresolved challenges.",
                            c.corroborated,
                            c.claims,
                            c.proofs_valid,
                            c.proofs_invalid,
                            c.proofs_unavailable_or_error,
                            c.unresolved_items
                        )),
                        "{label}: {}",
                        rec.rationale
                    );
                    // Every open item is a recorded Critic item.
                    let d = &receipt.deliberation;
                    assert_eq!(d.open_items.len() as u64, c.unresolved_items, "{label}");
                    assert!(d.open_items.iter().all(|i| i.kind.is_some()), "{label}");
                }
            }
        }

        let d = &receipt.deliberation;
        let missing: Vec<&OpenItem> = d
            .open_items
            .iter()
            .filter(|i| i.kind == Some(ChallengeKind::MissingTest))
            .collect();
        assert_eq!(
            d.missing_tests.iter().collect::<Vec<_>>(),
            missing,
            "{label}"
        );
        let evaluations = events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::EvaluationSubmitted { .. }))
            .count();
        assert_eq!(d.rounds as usize, evaluations, "{label}");
        if evaluations > 0 {
            // Every final claim was assessed, and nothing was left over.
            assert_eq!(d.evaluated_proposal_id, d.final_proposal_id, "{label}");
            assert!(d.claims.iter().all(|c| c.assessment.is_some()), "{label}");
            assert!(d.unmatched_assessments.is_empty(), "{label}");
        }
        let never = &d.earlier_round_objections_never_rejudged;
        let open_then = never.iter().filter(|o| o.unresolved_in_its_round).count();
        assert_eq!(
            (never.len(), open_then),
            expected[label.as_str()],
            "{label}: earlier-round objections never re-judged"
        );
        assert!(
            never.iter().all(|o| o.raised_in_round < d.rounds),
            "{label}"
        );
        // Items of a challenge no evaluation followed: only L06, which
        // failed at its first evaluation, has any (counted from the raw
        // JSONL, 2026-10-27).
        let unevaluated = if label == "L06" { 10 } else { 0 };
        assert_eq!(d.unevaluated_items.len(), unevaluated, "{label}");
        assert!(
            d.unevaluated_items
                .iter()
                .all(|i| i.raised_in_round == Some(d.rounds + 1) && i.kind.is_some()),
            "{label}"
        );

        // Verifications are the aggregate's, in order, each with its round.
        let flat: Vec<EvidenceVerification> = receipt
            .verifications
            .iter()
            .map(|v| v.verification.clone())
            .collect();
        assert_eq!(flat, agg.verifications, "{label}");
        assert!(
            receipt
                .verifications
                .windows(2)
                .all(|w| w[0].round <= w[1].round),
            "{label}"
        );
        assert!(
            receipt
                .verifications
                .iter()
                .all(|v| (1..=d.rounds).contains(&v.round)),
            "{label}"
        );

        // The chain: the run's own last hash, and what verifying it needs.
        let last = events.last().unwrap();
        let chain = &receipt.chain;
        assert_eq!(
            (chain.first_sequence, chain.last_sequence),
            (events[0].sequence, last.sequence),
            "{label}"
        );
        if schema == "1.0" {
            assert_eq!(chain.scope, ChainScope::Unchained, "{label}");
            assert_eq!(chain.head_hash, None, "{label}");
        } else {
            assert_eq!(
                chain.head_hash.as_deref(),
                Some(last.hash.as_str()),
                "{label}"
            );
            // Only a log's first run is a chain from genesis on its own.
            let scope = if events[0].sequence == 1 {
                ChainScope::Run
            } else {
                ChainScope::Log
            };
            assert_eq!(chain.scope, scope, "{label}");
        }
    }
}

#[test]
fn recorded_receipts_agree_with_receipt_v2() {
    let tmp = TempDir::new("recorded");
    let m = manifest();
    let logs = tmp.open_all(&m);
    let receipts = receipts(&logs, &m);
    let mut compared = 0;
    for r in entries(&m, "runs") {
        let Some(record) = r["record"].as_str() else {
            continue;
        };
        let label = text(&r, "label");
        let recorded: Value = serde_json::from_slice(&read(record)).unwrap();
        let v1 = &recorded["receipt"];
        let v2 = &receipts[&label];
        // The receipt captured at run time and the one derived now from
        // the archived events state the same facts.
        assert_eq!(v1["run_id"], v2.run_id.as_str(), "{label}");
        assert_eq!(v1["final_state"], name!(v2.final_state), "{label}");
        assert_eq!(
            v1["authorization"],
            name!(v2.authorization.state),
            "{label}"
        );
        assert_eq!(v1["event_count"].as_u64(), Some(v2.event_count), "{label}");
        assert_eq!(
            v1["head_hash"].as_str(),
            v2.chain.head_hash.as_deref(),
            "{label}"
        );
        let recommendation: Option<Recommendation> =
            serde_json::from_value(v1["recommendation"].clone()).unwrap();
        assert_eq!(recommendation, v2.recommendation, "{label}");
        let invocations: Vec<ModelInvocation> =
            serde_json::from_value(v1["model_invocations"].clone()).unwrap();
        assert_eq!(invocations, v2.model_invocations, "{label}");
        let verifications: Vec<EvidenceVerification> =
            serde_json::from_value(v1["verifications"].clone()).unwrap();
        let flat: Vec<EvidenceVerification> = v2
            .verifications
            .iter()
            .map(|v| v.verification.clone())
            .collect();
        assert_eq!(verifications, flat, "{label}");
        compared += 1;
    }
    assert_eq!(compared, 33);
}

#[test]
fn recorded_supported_runs_depend_on_objections_never_judged_again() {
    let tmp = TempDir::new("supported");
    let m = manifest();
    let receipts = receipts(&tmp.open_all(&m), &m);

    // R06 and P02, the two live `supported` runs: every check of the final
    // round passed, but each round-1 objection, unresolved when its round
    // ended, was never judged again (protocol 1.2).
    for (label, dropped) in [("R06", 7), ("P02", 8)] {
        let r = &receipts[label];
        let trace = r.policy_trace.as_ref().unwrap();
        assert_eq!(trace.kind, RecommendationKind::Supported, "{label}");
        assert_eq!(trace.rule, PolicyRule::AllChecksPassed, "{label}");
        assert!(trace.reasons.is_empty(), "{label}");
        let never = &r.deliberation.earlier_round_objections_never_rejudged;
        assert_eq!(never.len(), dropped, "{label}");
        assert!(
            never
                .iter()
                .all(|o| o.raised_in_round == 1 && o.unresolved_in_its_round),
            "{label}"
        );
        assert!(r.deliberation.open_items.is_empty(), "{label}");
        assert_eq!(r.deliberation.rounds, 2, "{label}");
    }

    // F03, the "raise attempts to 10" preset: 6 objections open at the end,
    // 3 missing tests, 2 unsupported claims, 1 security risk.
    let f03 = &receipts["F03"];
    let trace = f03.policy_trace.as_ref().unwrap();
    assert_eq!(trace.rule, PolicyRule::UnresolvedItems);
    assert_eq!((trace.counts.corroborated, trace.counts.claims), (5, 5));
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for item in &f03.deliberation.open_items {
        *kinds.entry(name!(item.kind.unwrap())).or_default() += 1;
    }
    assert_eq!(
        kinds,
        BTreeMap::from([
            ("missing_test".to_string(), 3),
            ("security_risk".to_string(), 1),
            ("unsupported_claim".to_string(), 2),
        ])
    );
    assert_eq!(f03.deliberation.missing_tests.len(), 3);
}

#[test]
fn a_run_that_failed_before_any_evaluation_still_shows_its_objections() {
    let tmp = TempDir::new("unevaluated");
    let m = manifest();
    let receipts = receipts(&tmp.open_all(&m), &m);

    // L06 failed at its first evaluation. The Critic had raised 10 items;
    // none is an open item or an earlier-round objection, because nothing
    // judged them, and the receipt still lists every one.
    let l06 = &receipts["L06"];
    assert_eq!(l06.failure.as_ref().unwrap().stage, "evaluate");
    let d = &l06.deliberation;
    assert_eq!(d.rounds, 0);
    assert!(d.open_items.is_empty());
    assert!(d.earlier_round_objections_never_rejudged.is_empty());
    let ids: Vec<String> = d.unevaluated_items.iter().map(|i| i.id.clone()).collect();
    let expected: Vec<String> = (7..=16).map(|n| format!("item-{n}")).collect();
    assert_eq!(ids, expected);
    assert!(d
        .unevaluated_items
        .iter()
        .all(|i| i.raised_in_round == Some(1)));
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for item in &d.unevaluated_items {
        *kinds.entry(name!(item.kind.unwrap())).or_default() += 1;
    }
    assert_eq!(
        kinds,
        BTreeMap::from([
            ("contradictory_evidence".to_string(), 2),
            ("missing_test".to_string(), 4),
            ("security_risk".to_string(), 1),
            ("unsupported_claim".to_string(), 3),
        ])
    );
}

#[test]
fn read_only_open_never_changes_an_archived_log() {
    let tmp = TempDir::new("read-only");
    let m = manifest();
    for f in entries(&m, "files") {
        if f["kind"] != "event_log" {
            continue;
        }
        let rel = text(&f, "path");
        let copy = tmp.copy(&rel);
        let mut log = EventLog::open_read_only(&copy).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(!log.recovered_truncated_tail, "{rel}");
        assert_eq!(Some(log.len() as u64), f["events"].as_u64(), "{rel}");
        let run = log.all_events()[0].run_id.clone();
        let retry = NewEvent {
            event_id: log.all_events()[0].event_id.clone(),
            run_id: run.clone(),
            actor: log.all_events()[0].actor.clone(),
            occurred_at: log.all_events()[0].occurred_at.clone(),
            payload: log.all_events()[0].payload.clone(),
        };
        assert!(
            matches!(log.append(retry), Err(EventLogError::ReadOnly)),
            "{rel}"
        );
        drop(log);
        assert!(
            std::fs::read(&copy).unwrap() == read(&rel),
            "{rel}: bytes changed"
        );
    }
}
