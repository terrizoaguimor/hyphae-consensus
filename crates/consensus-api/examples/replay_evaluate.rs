//! Re-asks the live Evaluator a recorded round-1 question (diagnostics).
//!
//! Run: `cargo run -p consensus-api --example replay_evaluate -- <run.json> [repeats]`
//! where `<run.json>` is a record written by `scripts/live_battery.py`.
//! The round-1 proposal, challenge, retrieved evidence, and integrity
//! statuses are rebuilt exactly from the recorded events, then sent to the
//! Meta API through the production adapter. Set `CONSENSUS_META_DEBUG_DIR`
//! to keep each raw draft. Needs `META_API_KEY`; writes no event log and
//! records no decision.

use consensus_api::config::MetaConfig;
use consensus_api::meta::MetaModelClient;
use consensus_domain::*;
use consensus_runtime::ports::{ModelClient, RoleContext};
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: replay_evaluate <run.json> [repeats]");
    let repeats: usize = args.get(2).and_then(|r| r.parse().ok()).unwrap_or(1);
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read run record"))
            .expect("run record is JSON");
    let events: Vec<ConsensusEvent> =
        serde_json::from_value(record["detail"]["events"].clone()).expect("recorded events");

    // Round 1 = everything before the first evaluation.
    let mut proposal = None;
    let mut challenge = None;
    let mut evidence = Vec::new();
    for ev in &events {
        match &ev.payload {
            EventPayload::ProposalSubmitted { proposal: p, .. } => proposal = Some(p.clone()),
            EventPayload::EvidenceRetrieved { evidence: e } => evidence.push(e.clone()),
            EventPayload::ChallengeSubmitted { challenge: c, .. } => challenge = Some(c.clone()),
            EventPayload::EvaluationSubmitted { .. } => break,
            _ => {}
        }
    }
    let integrity: Vec<(EvidenceId, IntegrityStatus)> = events
        .iter()
        .filter_map(|ev| match &ev.payload {
            EventPayload::EvidenceVerified { verification } => Some((
                verification.evidence_id.clone(),
                verification.integrity_status,
            )),
            _ => None,
        })
        .filter(|(id, _)| evidence.iter().any(|e| &e.id == id))
        .collect();
    let (proposal, challenge) = (
        proposal.expect("recorded proposal"),
        challenge.expect("recorded challenge"),
    );
    let ctx = RoleContext {
        evidence,
        integrity,
    };
    let cfg = MetaConfig::from_env().expect("META_API_KEY (or MODEL_API_KEY) required");
    let mut client = MetaModelClient::new(cfg).expect("meta client");
    for attempt in 1..=repeats {
        match client.evaluate(&proposal, &challenge, &ctx) {
            Ok((evaluation, invocation)) => {
                let cited: usize = evaluation
                    .assessments
                    .iter()
                    .map(|a| a.relevant_evidence.len() + a.contradicting_evidence.len())
                    .sum();
                println!(
                    "attempt {attempt}: request {:?}: {} items challenged, {} unresolved, {} corroborated of {}, {} evidence citations kept, abstain {}",
                    invocation.request_id,
                    challenge.items.len(),
                    evaluation.unresolved_items.len(),
                    evaluation.assessments.iter().filter(|a| a.corroborated).count(),
                    evaluation.assessments.len(),
                    cited,
                    evaluation.abstain
                );
            }
            Err(e) => println!("attempt {attempt}: error {e}"),
        }
    }
}
