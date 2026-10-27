// SPDX-License-Identifier: Apache-2.0
//! Receipt v2, derived from one run's events alone.
//!
//! [`build_receipt_v2`] replays the run's events through the state
//! machine, so no receipt is ever issued for a history that replay
//! rejects. It checks every hash link the run's own records can prove,
//! and re-derives the [`PolicyTrace`] from exactly the evaluation and
//! verification records the recommendation was issued on. It reads no
//! file and needs no other run's events, so it works the same for a
//! per-run file, a multi-run log or the research archive. Nothing here is
//! stored: receipts are computed at read time.
//!
//! Re-derivation is only as strong as the schema labels it selects rules
//! by, so the labels are checked too: every record's version must be
//! readable, a run's version never goes back, a chained record is never
//! labelled 1.0, and an unchained record never follows a chained one of
//! the same run. A recommendation recorded under 1.1 or later must
//! re-derive, and its stored rationale and unresolved assumptions must be
//! the ones the runtime derives from the same records.

use crate::event_log::{chain_link_problem, is_downgrade, schema_order, Predecessor};
use crate::policy::{explain, rationale_for, PolicyVersion};
use crate::state_machine::{RunAggregate, TransitionError};
use consensus_domain::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiptError {
    #[error("a receipt needs the run's events; none were given")]
    NoEvents,
    #[error("event at sequence {sequence} belongs to run {found}, not {expected}")]
    MixedRuns {
        expected: RunId,
        found: RunId,
        sequence: u64,
    },
    #[error("events out of order: sequence {found} follows {previous}")]
    OutOfOrder { previous: u64, found: u64 },
    #[error("hash chain broken at sequence {sequence}: {detail}")]
    HashChain { sequence: u64, detail: String },
    #[error("replay failed: {0}")]
    Replay(#[from] TransitionError),
    /// A record whose schema version this build cannot read.
    #[error("unsupported schema_version {found:?} at sequence {sequence}")]
    SchemaVersion { sequence: u64, found: String },
    /// A record whose schema version is older than an earlier record of
    /// the same run. No writer downgrades a run, so it was relabelled, and
    /// its label would pick the rules replay re-derives it with.
    #[error("schema_version {found:?} at sequence {sequence} is older than {earlier:?}, recorded earlier in the run")]
    SchemaDowngrade {
        sequence: u64,
        found: String,
        earlier: String,
    },
    /// A recommendation recorded under schema 1.1 or later that cannot be
    /// re-derived: no rules for its version, or no evaluation to derive
    /// from. Its outcome would be unverified, so no receipt is issued.
    #[error("recommendation at sequence {sequence} (schema {schema_version}) cannot be re-derived: {detail}")]
    PolicyNotDerived {
        sequence: u64,
        schema_version: String,
        detail: String,
    },
    /// The stored rationale is not `rationale_for` of the stored kind and
    /// the records the recommendation was derived from.
    #[error("recommendation at sequence {sequence}: stored rationale {stored:?} is not the derived {derived:?}")]
    RationaleMismatch {
        sequence: u64,
        stored: String,
        derived: String,
    },
    /// The stored unresolved assumptions are not the final proposal's
    /// assumptions, which the runtime copies into the recommendation.
    #[error("recommendation at sequence {sequence}: stored unresolved assumptions {stored:?} are not the final proposal's {expected:?}")]
    AssumptionsMismatch {
        sequence: u64,
        stored: Vec<String>,
        expected: Vec<String>,
    },
}

/// Authority of a decision recorded before schema 1.3, which carried
/// none. It is never read as operator authority.
pub const UNSPECIFIED_AUTHORITY: &str = "unspecified";

/// Schema versions recorded before protocol 1.3, under which each
/// evaluation judged only its own round's challenge items. Used for the
/// protocol label only: what a run's evaluations judged is read from its
/// records, never from its label.
const BEFORE_CARRY_FORWARD: [&str; 3] = ["1.0", "1.1", "1.2"];

/// What the chain cannot show, whatever its scope.
pub const CHAIN_LIMITATIONS: &str = "The chain is unsigned SHA-256. A successful check shows that the records received are internally consistent; it detects a change only against a hash obtained earlier, such as this receipt's head_hash or a published manifest. It cannot detect a run rewritten and re-hashed, a truncated tail or a deleted run, and it says nothing about whether the content is true.";

/// Chain note for [`ChainScope::Run`].
pub const CHAIN_NOTE_RUN: &str = "This run's events form a complete hash chain from genesis on their own: recomputing every record's hash and link from them alone verifies the run. head_hash is the hash of its last event.";

/// Chain note for [`ChainScope::Log`].
pub const CHAIN_NOTE_LOG: &str = "This run is one segment of a multi-run log: its records are linked to records of other runs, so verifying its chain needs the whole log file, from genesis. head_hash is the hash of this run's last event, not the head of the file; it commits to every earlier record in the file, not to records written after it.";

/// Chain note for [`ChainScope::PartlyChained`].
pub const CHAIN_NOTE_PARTLY_CHAINED: &str = "This run began before schema 1.1: its pre-1.1 records carry no hash chain and are not tamper-evident, and the chain after them commits to them only by count (pre-1.1:<sequence>), not by content. head_hash is the hash of this run's last event; it covers only the chained records, and verifying them needs the whole log file, from genesis.";

/// Chain note for [`ChainScope::Unchained`].
pub const CHAIN_NOTE_UNCHAINED: &str = "Pre-1.1 records carry no hash chain: nothing in this run is tamper-evident, and there is no head_hash.";

/// Label of the protocol and policy rules a run was recorded under:
/// `policy 1.1` and `policy 1.2` for runs before carry-forward, and
/// `protocol <version> · policy rules <rules>` from protocol 1.3 on,
/// where the rules are unchanged but carry-forward changed their inputs.
/// Pre-1.1 runs predate replay re-derivation.
pub fn protocol_label(schema_version: &str) -> String {
    label(schema_version, PolicyVersion::for_schema(schema_version))
}

fn label(schema_version: &str, rules: Option<PolicyVersion>) -> String {
    let Some(rules) = rules else {
        return if schema_version == "1.0" {
            "pre-1.1 (not re-derived on replay)".into()
        } else {
            format!("schema {schema_version} (no policy rules)")
        };
    };
    let rules = match rules {
        PolicyVersion::V1_1 => "1.1",
        PolicyVersion::V1_2 => "1.2",
    };
    if BEFORE_CARRY_FORWARD.contains(&schema_version) {
        format!("policy {rules}")
    } else {
        format!("protocol {schema_version} · policy rules {rules}")
    }
}

/// Builds receipt v2 from one run's events, in log order: every event of
/// the run and nothing else. Fails when the events mix runs or go out of
/// order; when a record's schema version is unreadable or older than an
/// earlier record's of the run; when a record's hash or a link the run's
/// records can show is broken; when replay rejects the history; or when a
/// recommendation recorded under 1.1 or later does not re-derive, with its
/// stored rationale and unresolved assumptions, from the records before it.
pub fn build_receipt_v2(
    run_events: &[ConsensusEvent],
    generated_at: String,
) -> Result<RunReceiptV2, ReceiptError> {
    let first = run_events.first().ok_or(ReceiptError::NoEvents)?;
    let run_id = first.run_id.clone();
    if let Some(other) = run_events.iter().find(|e| e.run_id != run_id) {
        return Err(ReceiptError::MixedRuns {
            expected: run_id,
            found: other.run_id.clone(),
            sequence: other.sequence,
        });
    }
    check_schema_versions(run_events)?;
    let chain = check_chain(run_events)?;

    let mut agg = RunAggregate::new(run_id.clone());
    let mut history = History::default();
    let mut policy_trace = None;
    for ev in run_events {
        agg.apply(ev)?;
        if let EventPayload::RecommendationIssued { recommendation } = &ev.payload {
            policy_trace = re_derive(ev, recommendation, &agg, &history)?;
        }
        history.record(ev);
    }

    let run_schema_version = first.schema_version.clone();
    let deliberation = history.deliberation();
    // No record before schema 1.3 carries an authority, so each decision
    // reads as unspecified, never as operator authority. A 1.3 record's
    // own authority belongs here once HumanAuthorization carries one.
    let authorization = ReceiptAuthorization {
        state: agg.authorization,
        authority: history
            .decision
            .as_ref()
            .map(|_| UNSPECIFIED_AUTHORITY.to_string()),
        decided_at: history.decision.as_ref().map(|d| d.decided_at.clone()),
        note: history.decision.as_ref().and_then(|d| d.note.clone()),
    };
    Ok(RunReceiptV2 {
        format: RunReceiptV2::FORMAT.into(),
        schema_version: SCHEMA_VERSION.into(),
        run_id,
        protocol: protocol_label(&run_schema_version),
        run_schema_version,
        task: agg.task,
        final_state: agg.state,
        failure: history.failure,
        recommendation: agg.recommendation,
        policy_trace,
        deliberation,
        authorization,
        model_invocations: agg.invocations,
        verifications: history.verifications,
        event_count: agg.event_count,
        chain,
        generated_at,
        proof_limitations: RunReceipt::DEFAULT_LIMITATIONS.into(),
    })
}

/// Refuses a record this build cannot read, and a record older than an
/// earlier record of the run: no writer downgrades a run (an older build
/// cannot read the newer records), so an older label could only be there
/// to pick other rules for replay's re-derivation, or none.
fn check_schema_versions(events: &[ConsensusEvent]) -> Result<(), ReceiptError> {
    let mut newest: Option<&str> = None;
    for ev in events {
        let version = ev.schema_version.as_str();
        if !READABLE_SCHEMA_VERSIONS.contains(&version) {
            return Err(ReceiptError::SchemaVersion {
                sequence: ev.sequence,
                found: version.to_string(),
            });
        }
        match newest {
            Some(earlier) if is_downgrade(version, earlier) => {
                return Err(ReceiptError::SchemaDowngrade {
                    sequence: ev.sequence,
                    found: version.to_string(),
                    earlier: earlier.to_string(),
                });
            }
            _ => newest = Some(version),
        }
    }
    Ok(())
}

/// The trace of a recommendation whose record replay just accepted: from
/// the latest evaluation and the verifications recorded after it, under
/// the rules of the record's own schema version, exactly the inputs and
/// rules replay re-derived its kind from. A pre-1.1 record predates
/// re-derivation and has none. From 1.1 on, a record that cannot be
/// re-derived, or whose rationale or unresolved assumptions are not what
/// the runtime writes for the same records, gets no receipt.
fn re_derive(
    ev: &ConsensusEvent,
    recommendation: &Recommendation,
    agg: &RunAggregate,
    history: &History,
) -> Result<Option<PolicyTrace>, ReceiptError> {
    if !schema_order(&ev.schema_version).is_some_and(|version| version >= (1, 1)) {
        return Ok(None);
    }
    let not_derived = |detail: &str| ReceiptError::PolicyNotDerived {
        sequence: ev.sequence,
        schema_version: ev.schema_version.clone(),
        detail: detail.to_string(),
    };
    let rules = PolicyVersion::for_schema(&ev.schema_version)
        .ok_or_else(|| not_derived("no policy rules for its schema version"))?;
    let evaluation = agg
        .last_evaluation
        .as_ref()
        .ok_or_else(|| not_derived("no evaluation was recorded before it"))?;
    let verifications = &agg.round_verifications;
    let trace = explain(rules, evaluation, verifications);
    // Replay checked this already; checked again so the trace shown can
    // never disagree with the kind stored.
    if trace.kind != recommendation.kind {
        return Err(TransitionError::RecommendationMismatch {
            stored: recommendation.kind,
            derived: trace.kind,
        }
        .into());
    }
    // The runtime writes both fields from the same records; replay checks
    // only the kind, so a re-hashed log could otherwise carry a sentence
    // that contradicts it.
    let derived = rationale_for(recommendation.kind, evaluation, verifications);
    if recommendation.rationale != derived {
        return Err(ReceiptError::RationaleMismatch {
            sequence: ev.sequence,
            stored: recommendation.rationale.clone(),
            derived,
        });
    }
    let expected = history
        .final_proposal
        .as_ref()
        .map(|p| p.assumptions.clone())
        .unwrap_or_default();
    if recommendation.unresolved_assumptions != expected {
        return Err(ReceiptError::AssumptionsMismatch {
            sequence: ev.sequence,
            stored: recommendation.unresolved_assumptions.clone(),
            expected,
        });
    }
    Ok(Some(trace))
}

/// Checks what the run's own records can prove, then states the chain's
/// scope. Every chained record's hash is recomputed. A record's link is
/// checked when its predecessor in the file is at hand: the run's
/// previous event at the previous sequence, or the start of the file.
/// Unchained (pre-1.1) records may only precede the run's chained ones,
/// across gaps too, as they may only precede chained records in a file.
fn check_chain(events: &[ConsensusEvent]) -> Result<ReceiptChain, ReceiptError> {
    let mut previous: Option<&ConsensusEvent> = None;
    let mut chained = false;
    for ev in events {
        let predecessor = match previous {
            Some(p) if ev.sequence <= p.sequence => {
                return Err(ReceiptError::OutOfOrder {
                    previous: p.sequence,
                    found: ev.sequence,
                });
            }
            Some(p) if ev.sequence == p.sequence + 1 => Predecessor::Record(p),
            None if ev.sequence == 1 => Predecessor::Start,
            // The record before it belongs to another run.
            _ => Predecessor::Unknown,
        };
        let problem = chain_link_problem(predecessor, ev).map_err(|e| ReceiptError::HashChain {
            sequence: ev.sequence,
            detail: format!("record does not serialize: {e}"),
        })?;
        if let Some(detail) = problem {
            return Err(ReceiptError::HashChain {
                sequence: ev.sequence,
                detail,
            });
        }
        if ev.hash.is_empty() && chained {
            return Err(ReceiptError::HashChain {
                sequence: ev.sequence,
                detail: "unhashed record after a chained record of the same run".into(),
            });
        }
        chained |= !ev.hash.is_empty();
        previous = Some(ev);
    }

    let (first, last) = match (events.first(), events.last()) {
        (Some(first), Some(last)) => (first, last),
        _ => return Err(ReceiptError::NoEvents),
    };
    let head_hash = Some(last.hash.clone()).filter(|h| !h.is_empty());
    let has_unchained = events.iter().any(|e| e.hash.is_empty());
    // Sequence 1 passed the check above against genesis; contiguous
    // sequences passed it against each other.
    let self_contained = first.sequence == 1
        && events
            .windows(2)
            .all(|w| w[1].sequence == w[0].sequence + 1)
        && !has_unchained;
    let (scope, note) = match head_hash {
        None => (ChainScope::Unchained, CHAIN_NOTE_UNCHAINED.to_string()),
        // Pre-1.1 records first, chained ones after (the check above):
        // only the chained part is tamper-evident.
        Some(_) if has_unchained => (
            ChainScope::PartlyChained,
            format!("{CHAIN_NOTE_PARTLY_CHAINED} {CHAIN_LIMITATIONS}"),
        ),
        Some(_) if self_contained => (
            ChainScope::Run,
            format!("{CHAIN_NOTE_RUN} {CHAIN_LIMITATIONS}"),
        ),
        Some(_) => (
            ChainScope::Log,
            format!("{CHAIN_NOTE_LOG} {CHAIN_LIMITATIONS}"),
        ),
    };
    Ok(ReceiptChain {
        scope,
        head_hash,
        first_sequence: first.sequence,
        last_sequence: last.sequence,
        note,
    })
}

/// What the receipt needs beyond the aggregate, collected in one pass.
#[derive(Default)]
struct History {
    /// Evaluations in order: round k is `evaluations[k - 1]`.
    evaluations: Vec<Evaluation>,
    /// Challenges with the round they belong to (the evaluation that
    /// follows them).
    challenges: Vec<(u32, Challenge)>,
    final_proposal: Option<Proposal>,
    revisions: Vec<ReceiptRevision>,
    verifications: Vec<RoundVerification>,
    failure: Option<ReceiptFailure>,
    decision: Option<HumanAuthorization>,
}

impl History {
    /// Completed evaluation rounds so far.
    fn rounds(&self) -> u32 {
        self.evaluations.len() as u32
    }

    fn record(&mut self, ev: &ConsensusEvent) {
        match &ev.payload {
            EventPayload::ProposalSubmitted { proposal, .. } => {
                self.final_proposal = Some(proposal.clone());
            }
            EventPayload::RevisionSubmitted { revision, .. } => {
                self.revisions.push(ReceiptRevision {
                    after_round: self.rounds(),
                    proposal_id: revision.proposal.id.clone(),
                    supersedes: revision.supersedes.clone(),
                    summary: revision.proposal.summary.clone(),
                    addressed_items: revision.addressed_items.clone(),
                });
                self.final_proposal = Some(revision.proposal.clone());
            }
            EventPayload::ChallengeSubmitted { challenge, .. } => {
                self.challenges.push((self.rounds() + 1, challenge.clone()));
            }
            EventPayload::EvaluationSubmitted { evaluation, .. } => {
                self.evaluations.push(evaluation.clone());
            }
            EventPayload::EvidenceVerified { verification } => {
                self.verifications.push(RoundVerification {
                    round: self.rounds(),
                    verification: verification.clone(),
                });
            }
            EventPayload::RunFailed { reason, stage } => {
                self.failure = Some(ReceiptFailure {
                    stage: stage.clone(),
                    reason: reason.clone(),
                });
            }
            EventPayload::HumanDecisionRecorded { authorization } => {
                self.decision = Some(authorization.clone());
            }
            EventPayload::TaskCreated { .. }
            | EventPayload::EvidenceRetrieved { .. }
            | EventPayload::EvidenceUnavailable { .. }
            | EventPayload::RecommendationIssued { .. } => {}
        }
    }

    fn deliberation(&self) -> Deliberation {
        let proposal = self.final_proposal.as_ref();
        let evaluation = self.evaluations.last();
        // Assessments attach to the final proposal's claims only when the
        // final evaluation judged that proposal; whatever does not attach
        // is listed as unmatched, never dropped.
        let mut unmatched: Vec<ClaimAssessment> = evaluation
            .map(|e| e.assessments.clone())
            .unwrap_or_default();
        let judged_final = matches!(
            (evaluation, proposal),
            (Some(e), Some(p)) if e.target_proposal == p.id
        );
        let claims = proposal
            .map(|p| {
                p.claims
                    .iter()
                    .map(|claim| {
                        let assessment = judged_final
                            .then(|| unmatched.iter().position(|a| a.claim_id == claim.id))
                            .flatten()
                            .map(|index| unmatched.remove(index))
                            .map(|a| ReceiptAssessment {
                                corroborated: a.corroborated,
                                relevant_evidence: a.relevant_evidence,
                                contradicting_evidence: a.contradicting_evidence,
                                note: a.note,
                            });
                        ReceiptClaim {
                            id: claim.id.clone(),
                            statement: claim.statement.clone(),
                            evidence_refs: claim.evidence_refs.clone(),
                            assessment,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let open_items: Vec<OpenItem> = evaluation
            .map(|e| {
                e.unresolved_items
                    .iter()
                    .map(|id| self.open_item(id))
                    .collect()
            })
            .unwrap_or_default();
        let missing_tests = open_items
            .iter()
            .filter(|item| item.kind == Some(ChallengeKind::MissingTest))
            .cloned()
            .collect();
        Deliberation {
            rounds: self.rounds(),
            final_proposal_id: proposal.map(|p| p.id.clone()),
            final_proposal_summary: proposal.map(|p| p.summary.clone()),
            evaluated_proposal_id: evaluation.map(|e| e.target_proposal.clone()),
            plan_steps: proposal.map(|p| p.plan_steps.clone()).unwrap_or_default(),
            claims,
            unmatched_assessments: unmatched,
            open_items,
            unevaluated_items: self.unevaluated_items(),
            assumptions: proposal.map(|p| p.assumptions.clone()).unwrap_or_default(),
            uncertainties: proposal
                .map(|p| p.uncertainties.clone())
                .unwrap_or_default(),
            missing_tests,
            revisions: self.revisions.clone(),
            earlier_round_objections_never_rejudged: self.unrejudged(),
        }
    }

    /// Items of the challenges recorded after the final evaluation: their
    /// round has no evaluation (the run failed or stopped first), so they
    /// are neither open items nor earlier-round objections.
    fn unevaluated_items(&self) -> Vec<OpenItem> {
        let final_round = self.rounds();
        self.challenges
            .iter()
            .filter(|(round, _)| *round > final_round)
            .flat_map(|(round, challenge)| {
                challenge.items.iter().map(|item| OpenItem {
                    id: item.id.clone(),
                    raised_in_round: Some(*round),
                    kind: Some(item.kind),
                    claim_id: item.claim_id.clone(),
                    description: Some(item.description.clone()),
                })
            })
            .collect()
    }

    /// The challenge item an unresolved id names, latest round first:
    /// before carry-forward an evaluation judged its own round's items.
    fn open_item(&self, id: &str) -> OpenItem {
        let found = self.challenges.iter().rev().find_map(|(round, challenge)| {
            challenge
                .items
                .iter()
                .find(|item| !item.id.is_empty() && item.id == id)
                .map(|item| (*round, item))
        });
        OpenItem {
            id: id.to_string(),
            raised_in_round: found.map(|(round, _)| round),
            kind: found.map(|(_, item)| item.kind),
            claim_id: found.and_then(|(_, item)| item.claim_id.clone()),
            description: found.map(|(_, item)| item.description.clone()),
        }
    }

    /// Items raised before the final round that the final evaluation never
    /// listed. Before protocol 1.3 no later round saw them again. This is
    /// read from the records whatever the run's protocol: a label claiming
    /// carry-forward proves nothing, and the list is empty only when the
    /// final evaluation does list every earlier item. No record carries
    /// per-item verdicts yet; once evaluations do, an explicit verdict on
    /// an item must count as judging it again here.
    fn unrejudged(&self) -> Vec<UnrejudgedObjection> {
        let Some(last) = self.evaluations.last() else {
            return Vec::new();
        };
        let final_round = self.rounds();
        let mut out = Vec::new();
        for (round, challenge) in &self.challenges {
            if *round >= final_round {
                continue;
            }
            // Rounds before the final one all have their evaluation.
            let own = &self.evaluations[*round as usize - 1];
            for item in &challenge.items {
                if lists(last, challenge, item) {
                    continue;
                }
                out.push(UnrejudgedObjection {
                    id: item.id.clone(),
                    raised_in_round: *round,
                    kind: item.kind,
                    claim_id: item.claim_id.clone(),
                    description: item.description.clone(),
                    unresolved_in_its_round: lists(own, challenge, item),
                });
            }
        }
        out
    }
}

/// Whether `evaluation` lists `item` as unresolved. Pre-1.1 items carry no
/// id, and evaluations then listed challenge ids instead.
fn lists(evaluation: &Evaluation, challenge: &Challenge, item: &ChallengeItem) -> bool {
    let id = if item.id.is_empty() {
        challenge.id.as_str()
    } else {
        item.id.as_str()
    };
    evaluation.unresolved_items.iter().any(|u| u == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, SequentialIdGen};
    use crate::event_log::{compute_hash, expected_prev_hash, EventLog, EventLogError};
    use crate::fakes::{FakeMemoryClient, FakeModelClient, FakeProofVerifier};
    use crate::orchestrator::{build_receipt, record_human_decision, Orchestrator, RunLimits};
    use crate::policy::rationale_code;
    use crate::ports::ModelError;
    use crate::scenarios::{retry_constraint_scenario, Scenario, TS};
    use std::path::PathBuf;

    fn temp_log(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyphae-consensus-receipts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    type Orch = Orchestrator<
        FakeModelClient,
        FakeMemoryClient,
        FakeProofVerifier,
        FixedClock,
        SequentialIdGen,
    >;

    fn orchestrator(s: Scenario) -> (Orch, crate::orchestrator::TaskInput) {
        let orch = Orchestrator {
            model: s.model,
            memory: s.memory,
            verifier: s.verifier,
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        (orch, s.input)
    }

    /// Runs `s` into a fresh log; returns the log and the run's id.
    fn run(name: &str, s: Scenario) -> (EventLog, RunId) {
        let mut log = EventLog::open(temp_log(name)).unwrap();
        let (mut orch, input) = orchestrator(s);
        let agg = orch.run(&mut log, input).unwrap();
        (log, agg.run_id)
    }

    fn events(log: &EventLog, run: &RunId) -> Vec<ConsensusEvent> {
        log.events_for(run).cloned().collect()
    }

    fn receipt(log: &EventLog, run: &RunId) -> RunReceiptV2 {
        build_receipt_v2(&events(log, run), TS.into()).unwrap()
    }

    /// Recomputes every chain link and hash after an edit, as a forger
    /// would, so only replay can catch the edit.
    fn rehash(events: &mut [ConsensusEvent]) {
        for i in 0..events.len() {
            let prev = if i == 0 { None } else { Some(&events[i - 1]) };
            let prev_hash = expected_prev_hash(prev);
            events[i].prev_hash = prev_hash;
            events[i].hash = compute_hash(&events[i]).unwrap();
        }
    }

    #[test]
    fn reference_receipt_shows_the_deliberation_and_the_dropped_objections() {
        let (mut log, run) = run("reference.jsonl", retry_constraint_scenario());
        let all = events(&log, &run);
        let r = build_receipt_v2(&all, TS.into()).unwrap();

        assert_eq!(r.format, RunReceiptV2::FORMAT);
        assert_eq!(r.run_schema_version, SCHEMA_VERSION);
        assert_eq!(r.protocol, "policy 1.2");
        assert_eq!(r.final_state, RunState::AwaitingHumanApproval);
        assert_eq!(r.failure, None);
        assert_eq!(
            r.task.as_ref().unwrap().title,
            "Fix payment client timeouts"
        );
        let trace = r.policy_trace.as_ref().unwrap();
        assert_eq!(trace.kind, RecommendationKind::Supported);
        assert_eq!(trace.rule, PolicyRule::AllChecksPassed);
        assert!(trace.reasons.is_empty());
        assert_eq!(rationale_code(trace.rule), "supported.all_checks_passed");
        assert_eq!(
            r.recommendation.as_ref().map(|rec| rec.kind),
            Some(trace.kind)
        );

        let d = &r.deliberation;
        assert_eq!(d.rounds, 2);
        assert_eq!(d.final_proposal_id, Some(ProposalId::new("prop-2")));
        assert_eq!(d.evaluated_proposal_id, d.final_proposal_id);
        assert_eq!(
            d.final_proposal_summary.as_deref(),
            Some("Cap retries at 3 attempts with jitter per the documented constraint")
        );
        assert_eq!(
            d.plan_steps,
            ["Apply the change", "Run the existing test suite"]
        );
        assert_eq!(d.claims.len(), 2);
        let claim2 = &d.claims[1];
        assert_eq!(claim2.id, ClaimId::new("claim-2"));
        assert_eq!(
            claim2.statement,
            "Capping retries at 3 attempts with jitter matches the documented constraint"
        );
        let assessed = claim2.assessment.as_ref().unwrap();
        assert!(assessed.corroborated);
        assert_eq!(
            assessed.relevant_evidence,
            [EvidenceId::new("ev-constraint")]
        );
        assert!(assessed.contradicting_evidence.is_empty());
        assert!(d.claims.iter().all(|c| c.assessment.is_some()));
        assert!(d.unmatched_assessments.is_empty());
        assert!(d.open_items.is_empty() && d.missing_tests.is_empty());
        assert!(d.unevaluated_items.is_empty());
        assert_eq!(
            d.assumptions,
            ["Upstream services tolerate the proposed change"]
        );
        assert_eq!(d.uncertainties, ["No load-test data available"]);
        assert_eq!(
            d.revisions,
            [ReceiptRevision {
                after_round: 1,
                proposal_id: ProposalId::new("prop-2"),
                supersedes: ProposalId::new("prop-1"),
                summary: "Cap retries at 3 attempts with jitter per the documented constraint"
                    .into(),
                addressed_items: vec!["item-1".into(), "item-2".into()],
            }]
        );
        // Under protocol 1.2 round 2 judged only its own (empty) challenge:
        // both round-1 objections were never judged again, though the
        // revision said it addressed them.
        assert_eq!(
            d.earlier_round_objections_never_rejudged,
            [
                UnrejudgedObjection {
                    id: "item-1".into(),
                    raised_in_round: 1,
                    kind: ChallengeKind::ConstraintConflict,
                    claim_id: Some(ClaimId::new("claim-2")),
                    description: "Documented constraint caps retries at 3 attempts".into(),
                    unresolved_in_its_round: true,
                },
                UnrejudgedObjection {
                    id: "item-2".into(),
                    raised_in_round: 1,
                    kind: ChallengeKind::UnsupportedClaim,
                    claim_id: Some(ClaimId::new("claim-1")),
                    description: "No evidence ties timeouts to insufficient retries".into(),
                    unresolved_in_its_round: true,
                },
            ]
        );

        // Round 1 contradicted ev-constraint; round 2, which the policy
        // used, corroborated it. Each record says which round it is.
        let rounds: Vec<(u32, &str, FactualStatus)> = r
            .verifications
            .iter()
            .map(|v| {
                (
                    v.round,
                    v.verification.evidence_id.as_str(),
                    v.verification.factual_status,
                )
            })
            .collect();
        assert_eq!(
            rounds,
            [
                (1, "ev-constraint", FactualStatus::Contradicted),
                (1, "ev-fact", FactualStatus::Uncorroborated),
                (2, "ev-constraint", FactualStatus::Corroborated),
                (2, "ev-fact", FactualStatus::Corroborated),
            ]
        );
        assert_eq!(r.model_invocations.len(), 6);
        assert_eq!(r.event_count, all.len() as u64);
        assert_eq!(r.chain.scope, ChainScope::Run);
        assert_eq!(r.chain.head_hash.as_deref(), log.head_hash());
        assert_eq!(
            (r.chain.first_sequence, r.chain.last_sequence),
            (1, all.len() as u64)
        );
        assert!(r.chain.note.contains("from genesis on their own"));
        assert!(r.chain.note.contains("unsigned"));
        assert_eq!(r.authorization.state, AuthorizationState::Pending);
        assert_eq!(r.authorization.authority, None);
        assert_eq!(r.proof_limitations, RunReceipt::DEFAULT_LIMITATIONS);

        // The facts v1 reports agree with v2's.
        let v1 = build_receipt(&log, &run, TS.into()).unwrap();
        assert_eq!(v1.recommendation, r.recommendation);
        assert_eq!(v1.model_invocations, r.model_invocations);
        let flat: Vec<EvidenceVerification> = r
            .verifications
            .iter()
            .map(|v| v.verification.clone())
            .collect();
        assert_eq!(v1.verifications, flat);
        assert_eq!(v1.event_count, r.event_count);
        assert_eq!(v1.head_hash, r.chain.head_hash);
        assert_eq!(v1.final_state, r.final_state);

        // A recorded decision: pre-1.3 records carry no authority.
        let decision = HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: run.clone(),
            authorized_by: "mario".into(),
            approved: false,
            decided_at: "2026-10-27T10:00:00Z".into(),
            idempotency_key: "k1".into(),
            note: Some("two round-1 objections were never judged again".into()),
        };
        record_human_decision(&mut log, decision, TS.into()).unwrap();
        let decided = receipt(&log, &run);
        assert_eq!(decided.final_state, RunState::Rejected);
        assert_eq!(
            decided.authorization,
            ReceiptAuthorization {
                state: AuthorizationState::Rejected,
                authority: Some(UNSPECIFIED_AUTHORITY.into()),
                decided_at: Some("2026-10-27T10:00:00Z".into()),
                note: Some("two round-1 objections were never judged again".into()),
            }
        );
        // The trace still explains the recommendation the human decided on.
        assert_eq!(decided.policy_trace, r.policy_trace);
    }

    #[test]
    fn blocked_receipt_names_the_invalid_proof_and_the_open_items() {
        let mut s = retry_constraint_scenario();
        s.verifier
            .by_id
            .insert(EvidenceId::new("ev-constraint"), IntegrityStatus::Invalid);
        let (log, run) = run("blocked.jsonl", s);
        let r = receipt(&log, &run);
        let trace = r.policy_trace.as_ref().unwrap();
        assert_eq!(trace.kind, RecommendationKind::Blocked);
        assert_eq!(rationale_code(trace.rule), "blocked.invalid_proof");
        assert_eq!(
            trace.reasons[0],
            TraceReason::InvalidProofs {
                evidence_ids: vec![EvidenceId::new("ev-constraint")]
            }
        );
        // One round, so nothing earlier to drop; both items stay open.
        let d = &r.deliberation;
        assert_eq!(d.rounds, 1);
        assert!(d.earlier_round_objections_never_rejudged.is_empty());
        assert_eq!(
            d.open_items,
            [
                OpenItem {
                    id: "item-1".into(),
                    raised_in_round: Some(1),
                    kind: Some(ChallengeKind::ConstraintConflict),
                    claim_id: Some(ClaimId::new("claim-2")),
                    description: Some("Documented constraint caps retries at 3 attempts".into()),
                },
                OpenItem {
                    id: "item-2".into(),
                    raised_in_round: Some(1),
                    kind: Some(ChallengeKind::UnsupportedClaim),
                    claim_id: Some(ClaimId::new("claim-1")),
                    description: Some("No evidence ties timeouts to insufficient retries".into()),
                },
            ]
        );
        assert_eq!(d.open_items.len() as u64, trace.counts.unresolved_items);
        // The claims shown are the ones the final evaluation judged.
        let claim2 = d.claims[1].assessment.as_ref().unwrap();
        assert!(!claim2.corroborated);
        assert_eq!(
            claim2.contradicting_evidence,
            [EvidenceId::new("ev-constraint")]
        );
    }

    #[test]
    fn offline_receipt_names_the_evidence_nobody_checked() {
        let mut s = retry_constraint_scenario();
        s.memory = FakeMemoryClient::offline();
        let (log, run) = run("offline.jsonl", s);
        let r = receipt(&log, &run);
        let trace = r.policy_trace.as_ref().unwrap();
        assert_eq!(trace.kind, RecommendationKind::InsufficientEvidence);
        assert_eq!(
            trace.reasons,
            [TraceReason::CitedEvidenceNotValid {
                evidence_ids: vec![EvidenceId::new("ev-fact"), EvidenceId::new("ev-constraint")]
            }]
        );
        assert!(r.verifications.is_empty());
    }

    #[test]
    fn missing_tests_are_the_open_items_of_that_kind() {
        let mut s = retry_constraint_scenario();
        // One round only: the evaluation leaves a missing test open.
        let mut model = FakeModelClient::new();
        model.proposals = s.model.proposals.clone();
        let mut challenge = s.model.critiques[0].clone().unwrap();
        challenge.items[1].kind = ChallengeKind::MissingTest;
        model.critiques.push_back(Ok(challenge));
        model.evaluations.push_back(s.model.evaluations[0].clone());
        s.model = model;
        let (log, run) = run("missing-test.jsonl", s);
        let r = receipt(&log, &run);
        let d = &r.deliberation;
        assert_eq!(d.open_items.len(), 2);
        assert_eq!(d.missing_tests, [d.open_items[1].clone()]);
        assert_eq!(d.missing_tests[0].kind, Some(ChallengeKind::MissingTest));
    }

    #[test]
    fn failed_run_receipt_records_the_stage_and_has_no_trace() {
        let mut s = retry_constraint_scenario();
        s.model.proposals.clear();
        s.model.proposals.push_back(Err(ModelError::Timeout));
        let (log, run) = run("failed.jsonl", s);
        let r = receipt(&log, &run);
        assert_eq!(r.final_state, RunState::Failed);
        let failure = r.failure.as_ref().unwrap();
        assert_eq!(failure.stage, "propose");
        assert!(failure.reason.contains("timed out"), "{}", failure.reason);
        assert_eq!(r.recommendation, None);
        assert_eq!(r.policy_trace, None);
        assert_eq!(r.deliberation.rounds, 0);
        assert_eq!(r.deliberation.final_proposal_id, None);
        assert!(r.deliberation.claims.is_empty());
        assert!(r.deliberation.unevaluated_items.is_empty());
        assert_eq!(r.chain.scope, ChainScope::Run);
    }

    #[test]
    fn interleaved_runs_need_the_whole_log_and_head_is_the_runs_own() {
        let mut log = EventLog::open(temp_log("interleaved.jsonl")).unwrap();
        // One orchestrator, so its id generator keeps every id distinct.
        let (mut orch, input) = orchestrator(retry_constraint_scenario());
        let mut runs = vec![orch.run(&mut log, input).unwrap().run_id];
        let s = retry_constraint_scenario();
        (orch.model, orch.memory, orch.verifier) = (s.model, s.memory, s.verifier);
        runs.push(orch.run(&mut log, s.input).unwrap().run_id);
        let decision = HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: runs[0].clone(),
            authorized_by: "mario".into(),
            approved: true,
            decided_at: TS.into(),
            idempotency_key: "k1".into(),
            note: None,
        };
        record_human_decision(&mut log, decision, TS.into()).unwrap();

        // The first run now has a record after the second run's records.
        let first = receipt(&log, &runs[0]);
        assert_eq!(first.chain.scope, ChainScope::Log);
        assert!(first.chain.note.contains("needs the whole log file"));
        assert_eq!(first.chain.first_sequence, 1);
        assert_eq!(first.chain.head_hash.as_deref(), log.head_hash());
        // The second run starts mid-file: its head is its own last event,
        // not the file's head.
        let second = receipt(&log, &runs[1]);
        assert_eq!(second.chain.scope, ChainScope::Log);
        let own_last = log.events_for(&runs[1]).last().unwrap();
        assert_eq!(
            second.chain.head_hash.as_deref(),
            Some(own_last.hash.as_str())
        );
        assert_ne!(second.chain.head_hash.as_deref(), log.head_hash());
        assert_eq!(second.chain.last_sequence, own_last.sequence);
    }

    #[test]
    fn receipts_refuse_events_that_are_not_one_runs_intact_history() {
        let (log, run) = run("refused.jsonl", retry_constraint_scenario());
        let all = events(&log, &run);
        assert!(matches!(
            build_receipt_v2(&[], TS.into()),
            Err(ReceiptError::NoEvents)
        ));

        // An edited record whose hashes were not recomputed.
        let mut edited = all.clone();
        if let EventPayload::TaskCreated { task } = &mut edited[0].payload {
            task.title = "forged".into();
        }
        match build_receipt_v2(&edited, TS.into()) {
            Err(ReceiptError::HashChain {
                sequence: 1,
                detail,
            }) => {
                assert!(detail.contains("hash mismatch"), "{detail}")
            }
            other => panic!("expected a broken chain, got {other:?}"),
        }
        // An edited record re-hashed on its own no longer links to its
        // successor.
        let mut relinked = all.clone();
        if let EventPayload::ProposalSubmitted { proposal, .. } = &mut relinked[1].payload {
            proposal.summary = "forged".into();
        }
        relinked[1].hash = compute_hash(&relinked[1]).unwrap();
        match build_receipt_v2(&relinked, TS.into()) {
            Err(ReceiptError::HashChain {
                sequence: 3,
                detail,
            }) => {
                assert!(detail.contains("prev_hash mismatch"), "{detail}")
            }
            other => panic!("expected a broken link, got {other:?}"),
        }
        // A record cut from the middle looks exactly like another run's
        // records in between: the run's own events cannot tell the two
        // apart, so the receipt says verification needs the whole log.
        let mut cut = all.clone();
        cut.remove(3);
        let r = build_receipt_v2(&cut, TS.into()).unwrap();
        assert_eq!(r.chain.scope, ChainScope::Log);
        assert_eq!(r.event_count, all.len() as u64 - 1);
        // Out of order, or another run's record mixed in.
        let mut swapped = all.clone();
        swapped.swap(1, 2);
        assert!(matches!(
            build_receipt_v2(&swapped, TS.into()),
            Err(ReceiptError::OutOfOrder {
                previous: 3,
                found: 2
            })
        ));
        let mut mixed = all.clone();
        mixed[4].run_id = RunId::new("run-other");
        assert!(matches!(
            build_receipt_v2(&mixed, TS.into()),
            Err(ReceiptError::MixedRuns { sequence: 5, .. })
        ));

        // A forger who upgrades the outcome and re-hashes the whole chain
        // gets past the hashes. Replay's re-derivation catches this edit
        // because the record keeps a label that maps to a policy; a label
        // changed to escape re-derivation is refused on its own (see
        // relabelling_a_record_cannot_escape_re_derivation).
        let mut forged = all.clone();
        let rec = forged
            .iter_mut()
            .find_map(|e| match &mut e.payload {
                EventPayload::RecommendationIssued { recommendation } => Some(recommendation),
                _ => None,
            })
            .unwrap();
        rec.kind = RecommendationKind::Contested;
        rehash(&mut forged);
        assert!(matches!(
            build_receipt_v2(&forged, TS.into()),
            Err(ReceiptError::Replay(
                TransitionError::RecommendationMismatch { .. }
            ))
        ));
    }

    #[test]
    fn legacy_run_is_unchained_and_not_rederived() {
        let (log, run) = run("legacy.jsonl", retry_constraint_scenario());
        let legacy: Vec<ConsensusEvent> = events(&log, &run)
            .into_iter()
            .map(|mut e| {
                e.schema_version = "1.0".into();
                e.prev_hash.clear();
                e.hash.clear();
                e
            })
            .collect();
        let r = build_receipt_v2(&legacy, TS.into()).unwrap();
        assert_eq!(r.run_schema_version, "1.0");
        assert_eq!(r.protocol, "pre-1.1 (not re-derived on replay)");
        assert_eq!(r.chain.scope, ChainScope::Unchained);
        assert_eq!(r.chain.head_hash, None);
        assert_eq!(r.chain.note, CHAIN_NOTE_UNCHAINED);
        assert_eq!(r.policy_trace, None, "pre-1.1 records are not re-derived");
        assert_eq!(
            r.recommendation.map(|rec| rec.kind),
            Some(RecommendationKind::Supported)
        );
        assert_eq!(
            r.deliberation.earlier_round_objections_never_rejudged.len(),
            2
        );
    }

    #[test]
    fn a_protocol_label_alone_never_hides_dropped_objections() {
        // The reference history drops two round-1 objections. Labelled as
        // a carry-forward protocol, with no record showing that a later
        // round judged them, it drops them all the same, and the receipt
        // still lists them. (The history is not replayed as 1.3 here: the
        // 1.3 contract checks apply only to records written under it.)
        let (log, run) = run("protocol-1-3.jsonl", retry_constraint_scenario());
        for label in BEFORE_CARRY_FORWARD.into_iter().chain(["1.3", "1.4"]) {
            let mut history = History::default();
            for ev in log.events_for(&run) {
                let mut relabelled = ev.clone();
                relabelled.schema_version = label.into();
                history.record(&relabelled);
            }
            let never = history
                .deliberation()
                .earlier_round_objections_never_rejudged;
            let ids: Vec<&str> = never.iter().map(|o| o.id.as_str()).collect();
            assert_eq!(ids, ["item-1", "item-2"], "{label}");
        }
    }

    /// Index of the recommendation record in `events`.
    fn recommendation_index(events: &[ConsensusEvent]) -> usize {
        events
            .iter()
            .position(|e| matches!(e.payload, EventPayload::RecommendationIssued { .. }))
            .expect("a recommendation was issued")
    }

    fn recommendation_mut(ev: &mut ConsensusEvent) -> &mut Recommendation {
        match &mut ev.payload {
            EventPayload::RecommendationIssued { recommendation } => recommendation,
            other => panic!("not a recommendation: {other:?}"),
        }
    }

    /// One line per event, as the event log writes them.
    fn write_log(name: &str, events: &[ConsensusEvent]) -> PathBuf {
        let path = temp_log(name);
        let lines: String = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        std::fs::write(&path, lines).unwrap();
        path
    }

    #[test]
    fn relabelling_a_record_cannot_escape_re_derivation() {
        // The adversarial review's probe (P-1): a blocked run whose
        // recommendation is upgraded to supported and relabelled so that
        // replay would derive it under other rules, or not at all, with the
        // whole chain re-hashed so every hash checks out.
        let mut s = retry_constraint_scenario();
        s.verifier
            .by_id
            .insert(EvidenceId::new("ev-constraint"), IntegrityStatus::Invalid);
        let (log, run) = run("relabelled.jsonl", s);
        let genuine = events(&log, &run);
        let at = recommendation_index(&genuine);
        let trace = receipt(&log, &run).policy_trace.unwrap();
        assert_eq!(trace.kind, RecommendationKind::Blocked);
        let forge = |label: &str, every_record: bool| {
            let mut forged = genuine.clone();
            recommendation_mut(&mut forged[at]).kind = RecommendationKind::Supported;
            for (i, e) in forged.iter_mut().enumerate() {
                if every_record || i == at {
                    e.schema_version = label.into();
                }
            }
            rehash(&mut forged);
            forged
        };

        // 1.0, which replay never re-derives, and 1.1, whose rules it would
        // use: both older than the run's records before it.
        for label in ["1.0", "1.1"] {
            match build_receipt_v2(&forge(label, false), TS.into()) {
                Err(ReceiptError::SchemaDowngrade {
                    sequence,
                    found,
                    earlier,
                }) => {
                    assert_eq!(sequence, genuine[at].sequence);
                    assert_eq!((found.as_str(), earlier.as_str()), (label, SCHEMA_VERSION));
                }
                other => panic!("{label}: expected a refused downgrade, got {other:?}"),
            }
        }
        // The event log refuses the same forged file, read-only or not.
        let forged_1_0 = write_log("relabelled-1-0.jsonl", &forge("1.0", false));
        for opened in [
            EventLog::open_read_only(&forged_1_0),
            EventLog::open(&forged_1_0),
        ] {
            match opened {
                Err(EventLogError::HashChain { detail, .. }) => {
                    assert_eq!(detail, "schema 1.0 record with chain hash")
                }
                other => panic!("expected a refused 1.0 label, got {other:?}"),
            }
        }
        let forged_1_1 = write_log("relabelled-1-1.jsonl", &forge("1.1", false));
        assert!(matches!(
            EventLog::open_read_only(&forged_1_1),
            Err(EventLogError::SchemaDowngrade { .. })
        ));

        // Every record relabelled 1.0 yet chained: 1.0 predates the chain.
        match build_receipt_v2(&forge("1.0", true), TS.into()) {
            Err(ReceiptError::HashChain {
                sequence: 1,
                detail,
            }) => assert_eq!(detail, "schema 1.0 record with chain hash"),
            other => panic!("expected a refused 1.0 label, got {other:?}"),
        }
        // A label no rules exist for, on one record or on all of them.
        for every_record in [false, true] {
            match build_receipt_v2(&forge("9.9", every_record), TS.into()) {
                Err(ReceiptError::SchemaVersion { found, .. }) => assert_eq!(found, "9.9"),
                other => panic!("expected an unreadable label, got {other:?}"),
            }
        }
        // Every record relabelled 1.1, a consistent older run: replay
        // re-derives it under the 1.1 rules, which block it too.
        assert!(matches!(
            build_receipt_v2(&forge("1.1", true), TS.into()),
            Err(ReceiptError::Replay(
                TransitionError::RecommendationMismatch {
                    stored: RecommendationKind::Supported,
                    derived: RecommendationKind::Blocked,
                }
            ))
        ));
    }

    #[test]
    fn an_unchained_record_never_follows_a_chained_one_of_the_run() {
        // Across a gap (another run's records, or a cut one) a record's
        // link cannot be checked, but a run still never goes back to
        // unchained records.
        let (log, run) = run("unchained-after-gap.jsonl", retry_constraint_scenario());
        let mut events = events(&log, &run);
        events.remove(3);
        let sequence = events[3].sequence;
        let legacy = &mut events[3];
        legacy.schema_version = "1.0".into();
        legacy.prev_hash.clear();
        legacy.hash.clear();
        match check_chain(&events) {
            Err(ReceiptError::HashChain {
                sequence: at,
                detail,
            }) => {
                assert_eq!(at, sequence);
                assert_eq!(
                    detail,
                    "unhashed record after a chained record of the same run"
                );
            }
            other => panic!("expected a refused unchained record, got {other:?}"),
        }
        // The receipt refuses it even before the chain: its label is
        // older than the run's records before it.
        assert!(matches!(
            build_receipt_v2(&events, TS.into()),
            Err(ReceiptError::SchemaDowngrade { .. })
        ));
    }

    #[test]
    fn a_recommendation_recorded_from_1_1_on_must_re_derive() {
        let (log, run) = run("not-derived.jsonl", retry_constraint_scenario());
        let all = events(&log, &run);
        let at = recommendation_index(&all);
        let mut agg = RunAggregate::new(run.clone());
        let mut history = History::default();
        for ev in &all[..at] {
            agg.apply(ev).unwrap();
            history.record(ev);
        }
        let mut issued = all[at].clone();
        let rec = recommendation_mut(&mut issued).clone();
        let trace = re_derive(&issued, &rec, &agg, &history).unwrap();
        assert_eq!(trace.map(|t| t.rule), Some(PolicyRule::AllChecksPassed));

        // A version past 1.1 that no rules map (a build that reads a new
        // schema before the policy maps it) gets no receipt, never one
        // without a trace.
        let mut unmapped = issued.clone();
        unmapped.schema_version = "2.0".into();
        match re_derive(&unmapped, &rec, &agg, &history) {
            Err(ReceiptError::PolicyNotDerived {
                sequence,
                schema_version,
                detail,
            }) => {
                assert_eq!(
                    (sequence, schema_version.as_str()),
                    (issued.sequence, "2.0")
                );
                assert_eq!(detail, "no policy rules for its schema version");
            }
            other => panic!("expected an underivable recommendation, got {other:?}"),
        }
        let no_evaluation = RunAggregate::new(run.clone());
        assert!(matches!(
            re_derive(&issued, &rec, &no_evaluation, &history),
            Err(ReceiptError::PolicyNotDerived { .. })
        ));
        // A pre-1.1 record predates re-derivation: no trace, by design.
        let mut legacy = issued;
        legacy.schema_version = "1.0".into();
        assert_eq!(re_derive(&legacy, &rec, &agg, &history).unwrap(), None);
    }

    #[test]
    fn a_forged_rationale_or_assumption_list_gets_no_receipt() {
        // Replay checks only the kind; the sentence and the assumptions
        // beside it must also be what the runtime derives.
        let (log, run) = run("rationale.jsonl", retry_constraint_scenario());
        let genuine = events(&log, &run);
        let at = recommendation_index(&genuine);
        let recorded = recommendation_mut(&mut genuine[at].clone()).clone();
        assert_eq!(
            recorded.rationale,
            "Supported: 2/2 claims corroborated; proofs: 2 valid, 0 invalid, 0 unavailable/error; 0 unresolved challenges."
        );

        let mut forged = genuine.clone();
        recommendation_mut(&mut forged[at]).rationale =
            format!("{} Security review passed.", recorded.rationale);
        rehash(&mut forged);
        match build_receipt_v2(&forged, TS.into()) {
            Err(ReceiptError::RationaleMismatch {
                sequence,
                stored,
                derived,
            }) => {
                assert_eq!(sequence, genuine[at].sequence);
                assert!(stored.ends_with("Security review passed."), "{stored}");
                assert_eq!(derived, recorded.rationale);
            }
            other => panic!("expected a refused rationale, got {other:?}"),
        }

        let mut forged = genuine.clone();
        recommendation_mut(&mut forged[at])
            .unresolved_assumptions
            .clear();
        rehash(&mut forged);
        match build_receipt_v2(&forged, TS.into()) {
            Err(ReceiptError::AssumptionsMismatch {
                stored, expected, ..
            }) => {
                assert!(stored.is_empty());
                assert_eq!(expected, recorded.unresolved_assumptions);
                assert_eq!(expected, ["Upstream services tolerate the proposed change"]);
            }
            other => panic!("expected refused assumptions, got {other:?}"),
        }
    }

    #[test]
    fn unevaluated_items_are_the_items_no_evaluation_judged() {
        // The run fails at its first evaluation: the Critic raised both
        // items, and nothing ever judged them.
        let mut s = retry_constraint_scenario();
        s.model.evaluations.clear();
        s.model.evaluations.push_back(Err(ModelError::Timeout));
        let (log, run_id) = run("unevaluated-round-1.jsonl", s);
        let r = receipt(&log, &run_id);
        assert_eq!(
            r.failure.as_ref().map(|f| f.stage.as_str()),
            Some("evaluate")
        );
        let d = &r.deliberation;
        assert_eq!(d.rounds, 0);
        assert!(d.open_items.is_empty());
        assert!(d.earlier_round_objections_never_rejudged.is_empty());
        assert_eq!(
            d.unevaluated_items,
            [
                OpenItem {
                    id: "item-1".into(),
                    raised_in_round: Some(1),
                    kind: Some(ChallengeKind::ConstraintConflict),
                    claim_id: Some(ClaimId::new("claim-2")),
                    description: Some("Documented constraint caps retries at 3 attempts".into()),
                },
                OpenItem {
                    id: "item-2".into(),
                    raised_in_round: Some(1),
                    kind: Some(ChallengeKind::UnsupportedClaim),
                    claim_id: Some(ClaimId::new("claim-1")),
                    description: Some("No evidence ties timeouts to insufficient retries".into()),
                },
            ]
        );

        // The run fails at its second evaluation, after a challenge that
        // raised a new item: round 1's evaluation is the final one, so its
        // items are the open ones, and the new item is unevaluated.
        let mut s = retry_constraint_scenario();
        let mut second = s.model.critiques[1].clone().unwrap();
        second.items.push(ChallengeItem {
            id: "item-3".into(),
            kind: ChallengeKind::MissingTest,
            claim_id: Some(ClaimId::new("claim-2")),
            description: "No test covers the jittered backoff".into(),
            evidence_refs: vec![],
        });
        s.model.critiques[1] = Ok(second);
        s.model.evaluations[1] = Err(ModelError::Timeout);
        let (log, run_id) = run("unevaluated-round-2.jsonl", s);
        let d = receipt(&log, &run_id).deliberation;
        assert_eq!(d.rounds, 1);
        let open: Vec<&str> = d.open_items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(open, ["item-1", "item-2"]);
        assert!(d.earlier_round_objections_never_rejudged.is_empty());
        assert_eq!(
            d.unevaluated_items,
            [OpenItem {
                id: "item-3".into(),
                raised_in_round: Some(2),
                kind: Some(ChallengeKind::MissingTest),
                claim_id: Some(ClaimId::new("claim-2")),
                description: Some("No test covers the jittered backoff".into()),
            }]
        );
        // Missing tests are the open items of that kind; an unevaluated
        // one is listed with the unevaluated items.
        assert!(d.missing_tests.is_empty());
    }

    #[test]
    fn a_run_continued_after_its_pre_1_1_records_is_only_partly_chained() {
        // A run deliberated under 1.0 whose decision a later build
        // recorded: only the decision is chained, and it commits to the
        // records before it by count, not content.
        let (mut log, run) = run("partly-chained.jsonl", retry_constraint_scenario());
        let decision = HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: run.clone(),
            authorized_by: "mario".into(),
            approved: false,
            decided_at: TS.into(),
            idempotency_key: "k1".into(),
            note: None,
        };
        record_human_decision(&mut log, decision, TS.into()).unwrap();
        let mut events = events(&log, &run);
        let last = events.len() - 1;
        for e in &mut events[..last] {
            e.schema_version = "1.0".into();
            e.prev_hash.clear();
            e.hash.clear();
        }
        let prev_hash = expected_prev_hash(Some(&events[last - 1]));
        assert_eq!(prev_hash, format!("pre-1.1:{last}"));
        events[last].prev_hash = prev_hash;
        events[last].hash = compute_hash(&events[last]).unwrap();

        let r = build_receipt_v2(&events, TS.into()).unwrap();
        assert_eq!(r.chain.scope, ChainScope::PartlyChained);
        assert_eq!(
            r.chain.head_hash.as_deref(),
            Some(events[last].hash.as_str())
        );
        assert!(r.chain.note.starts_with(CHAIN_NOTE_PARTLY_CHAINED));
        assert!(r.chain.note.contains("unsigned"));
        assert_eq!(r.protocol, "pre-1.1 (not re-derived on replay)");
        assert_eq!(r.policy_trace, None);
        assert_eq!(r.final_state, RunState::Rejected);
    }

    #[test]
    fn protocol_labels_name_the_protocol_and_the_rules() {
        assert_eq!(protocol_label("1.1"), "policy 1.1");
        assert_eq!(protocol_label("1.2"), "policy 1.2");
        assert_eq!(protocol_label("1.0"), "pre-1.1 (not re-derived on replay)");
        // Carry-forward keeps the 1.2 rules but changes their inputs, so
        // the label names both.
        assert_eq!(
            label("1.3", Some(PolicyVersion::V1_2)),
            "protocol 1.3 · policy rules 1.2"
        );
        assert_eq!(label("9.9", None), "schema 9.9 (no policy rules)");
    }

    #[test]
    fn receipt_round_trips_through_json_with_nested_round_verifications() {
        let (log, run) = run("json.jsonl", retry_constraint_scenario());
        let r = receipt(&log, &run);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["format"], "hyphae-consensus-run-receipt-v2");
        assert_eq!(json["policy_trace"]["rule"], "all_checks_passed");
        assert_eq!(json["chain"]["scope"], "run");
        // A verification is nested beside its round, never flattened into
        // the same object, so no field the record gains can collide.
        let v = &json["verifications"][0];
        assert_eq!(v["round"], 1);
        assert_eq!(v["verification"]["evidence_id"], "ev-constraint");
        assert_eq!(v["verification"]["factual_status"], "contradicted");
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["round", "verification"]);
        let back: RunReceiptV2 = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back, r);

        // Verification records that carry a round of their own (schema 1.3
        // adds one) still read back, and the receipt's tags are unchanged.
        let mut tagged = json;
        for v in tagged["verifications"].as_array_mut().unwrap() {
            v["verification"]["round"] = serde_json::json!(7);
        }
        let back: RunReceiptV2 = serde_json::from_value(tagged).unwrap();
        let rounds: Vec<u32> = back.verifications.iter().map(|v| v.round).collect();
        assert_eq!(rounds, [1, 1, 2, 2]);
        let again: RunReceiptV2 =
            serde_json::from_value(serde_json::to_value(&back).unwrap()).unwrap();
        assert_eq!(again, back);
    }
}
