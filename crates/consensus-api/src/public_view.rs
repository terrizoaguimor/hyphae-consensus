// SPDX-License-Identifier: Apache-2.0
//! Views of a run for the API, derived from its chained events alone.
//!
//! Provenance (B4, LC-05) is never taken from the request or from the
//! sidecar: the model axis comes from the recorded `ModelInvocation`s,
//! the evidence axis from the retrieval and verification records, and
//! the declared `Task.mode` is only cross-checked against them. A
//! disagreement yields the `provenance_inconsistent` flag, which a client
//! must show and must never render as LIVE.
//!
//! The public surface strips what a visitor must not see: proof and
//! witness paths, backend references, raw failure reasons (a fixed
//! statement per stable code replaces them) and verifier output. The
//! operator surface keeps them.

use crate::api::types::*;
use consensus_domain::*;
use consensus_runtime::policy::{explain, rationale_code, PolicyVersion};
use consensus_runtime::receipts::{protocol_label, UNSPECIFIED_AUTHORITY};
use consensus_runtime::RunAggregate;

/// Which server renders the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Public,
    Operator,
}

impl Surface {
    pub fn v1(self) -> SurfaceV1 {
        match self {
            Surface::Public => SurfaceV1::Public,
            Surface::Operator => SurfaceV1::Operator,
        }
    }
}

/// The verifier identity the scripted double records.
pub const FAKE_VERIFIER: &str = "fake-verifier";
/// Prefix of the backend reference the scripted memory records.
const FAKE_MEMORY_REF: &str = "fake-memory:";

/// Statement shown to the public for a `public_demo` decision.
pub const PUBLIC_DEMO_NOTICE: &str =
    "This decision carries no operational authority: it records a visitor's choice on a simulated run.";

/// Fixed public statement for a failure code (never the recorded reason).
pub fn failure_message(code: Option<&str>, stage: &str) -> String {
    match code {
        Some("cancelled") => "The run was cancelled by its owner.".into(),
        Some("shutdown") => "The server shut down before the run completed.".into(),
        Some("deadline") => "The run's deadline passed.".into(),
        Some("interrupted") => "The server restarted before the run completed.".into(),
        Some("panic") => "The run stopped because of an internal error.".into(),
        Some("model_timeout") => "A model call timed out.".into(),
        Some("model_unavailable") => "The model service was unavailable.".into(),
        Some("model_malformed_output") => "The model returned output that was refused.".into(),
        Some("evaluation_contract") | Some("challenge_contract") | Some("proposal_contract") => {
            "A model document broke the deliberation protocol and was refused.".into()
        }
        Some("token_budget_exceeded") => "The run crossed its token budget.".into(),
        Some("token_usage_missing") => {
            "A model call reported no token usage, so the budget could not be enforced.".into()
        }
        Some("persistence") => "The run's record could not be written.".into(),
        Some(other) => format!("The run failed ({other})."),
        None => format!("The run failed at stage {stage}."),
    }
}

/// Fixed public statement for an `EvidenceUnavailable` code.
pub fn unavailable_message(code: Option<&str>) -> String {
    match code {
        Some("memory_unavailable") => "The memory backend was unreachable.".into(),
        Some("memory_error") => "The memory backend answered with an error.".into(),
        Some("evidence_cap") => "Records past the per-run evidence cap were not processed.".into(),
        _ => "Evidence could not be retrieved.".into(),
    }
}

fn operator_only<T>(surface: Surface, value: Option<T>) -> Option<T> {
    match surface {
        Surface::Operator => value,
        Surface::Public => None,
    }
}

pub fn invocation_v1(inv: &ModelInvocation, surface: Surface) -> ModelInvocationV1 {
    ModelInvocationV1 {
        provider: inv.provider.clone(),
        model: inv.model.clone(),
        request_id: inv.request_id.clone(),
        latency_ms: inv.latency_ms,
        input_tokens: inv.input_tokens,
        output_tokens: inv.output_tokens,
        reasoning_tokens: inv.reasoning_tokens,
        finish_reason: inv.finish_reason.clone(),
        scripted: inv.provider == "fake",
        max_completion_tokens: operator_only(surface, inv.max_completion_tokens),
        schema_name: operator_only(surface, inv.schema_name.clone()),
        schema_sha256: operator_only(surface, inv.schema_sha256.clone()),
        prompt_sha256: operator_only(surface, inv.prompt_sha256.clone()),
        attempts: operator_only(surface, inv.attempts),
    }
}

pub fn evidence_v1(ev: &EvidenceReference, surface: Surface) -> EvidenceReferenceV1 {
    EvidenceReferenceV1 {
        id: ev.id.clone(),
        project_id: ev.project_id.clone(),
        kind: ev.kind.clone(),
        content: ev.content.clone(),
        retrieved_at: ev.retrieved_at.clone(),
        anchor: ev.anchor.clone(),
        retrieval: ev.retrieval,
        snapshot_id: ev.snapshot_id.clone(),
        captured_at: ev.captured_at.clone(),
        proof_ref: ev.proof_ref.clone(),
        proof_sha256: ev.proof_sha256.clone(),
        proof_request: ev.proof_request.clone(),
        backend_project: ev.backend_project.clone(),
        backend_scope: ev.backend_scope.clone(),
        proof_error_reported: ev.proof_error.is_some(),
        proof_error: operator_only(surface, ev.proof_error.clone()),
        backend_ref: operator_only(surface, ev.backend_ref.clone()),
        proof_file: operator_only(surface, ev.proof_file.clone()),
        witness_file: operator_only(surface, ev.witness_file.clone()),
    }
}

pub fn verification_v1(v: &EvidenceVerification, surface: Surface) -> VerificationV1 {
    let simulated = v.verifier.as_deref() == Some(FAKE_VERIFIER);
    VerificationV1 {
        evidence_id: v.evidence_id.clone(),
        integrity_status: v.integrity_status,
        factual_status: v.factual_status,
        relevance_status: v.relevance_status,
        verifier: v.verifier.clone(),
        verified_anchor: v.verified_anchor.clone(),
        checked_at: v.checked_at.clone(),
        round: v.round,
        proof_ref: v.proof_ref.clone(),
        report: v.report.clone(),
        simulated,
        // The scripted verifier's detail says SIMULATED, which the public
        // should read; a real verifier's output stays with the operator.
        detail: if simulated {
            v.detail.clone()
        } else {
            operator_only(surface, v.detail.clone())
        },
    }
}

/// The authority label of a decision record: never operator unless the
/// record says so.
pub fn authority_label(authority: Option<Authority>) -> String {
    match authority {
        Some(Authority::Operator) => "operator".into(),
        Some(Authority::PublicDemo) => "public_demo".into(),
        None => UNSPECIFIED_AUTHORITY.into(),
    }
}

pub fn decision_v1(auth: &HumanAuthorization, surface: Surface) -> DecisionV1 {
    DecisionV1 {
        approved: auth.approved,
        authority: authority_label(auth.authority),
        authorized_by: auth.authorized_by.clone(),
        decided_at: auth.decided_at.clone(),
        note: auth.note.clone(),
        notice: (auth.authority == Some(Authority::PublicDemo)).then(|| PUBLIC_DEMO_NOTICE.into()),
        idempotency_key: operator_only(surface, Some(auth.idempotency_key.clone())),
    }
}

pub fn payload_v1(payload: &EventPayload, surface: Surface) -> EventPayloadV1 {
    match payload {
        EventPayload::TaskCreated { task } => EventPayloadV1::TaskCreated { task: task.clone() },
        EventPayload::ProposalSubmitted {
            proposal,
            invocation,
        } => EventPayloadV1::ProposalSubmitted {
            proposal: proposal.clone(),
            invocation: invocation_v1(invocation, surface),
        },
        EventPayload::EvidenceRetrieved { evidence } => EventPayloadV1::EvidenceRetrieved {
            evidence: evidence_v1(evidence, surface),
        },
        EventPayload::EvidenceUnavailable {
            query,
            reason,
            code,
        } => EventPayloadV1::EvidenceUnavailable {
            query: query.clone(),
            code: code.clone(),
            message: unavailable_message(code.as_deref()),
            reason: operator_only(surface, Some(reason.clone())),
        },
        EventPayload::RecallProven {
            request,
            evidence_ids,
            proof_ref,
            anchor,
            empty,
        } => EventPayloadV1::RecallProven {
            request: request.clone(),
            evidence_ids: evidence_ids.clone(),
            proof_ref: proof_ref.clone(),
            anchor: anchor.clone(),
            empty: *empty,
        },
        EventPayload::EvidenceVerified { verification } => EventPayloadV1::EvidenceVerified {
            verification: verification_v1(verification, surface),
        },
        EventPayload::ChallengeSubmitted {
            challenge,
            invocation,
        } => EventPayloadV1::ChallengeSubmitted {
            challenge: challenge.clone(),
            invocation: invocation_v1(invocation, surface),
        },
        EventPayload::EvaluationSubmitted {
            evaluation,
            invocation,
        } => EventPayloadV1::EvaluationSubmitted {
            evaluation: evaluation.clone(),
            invocation: invocation_v1(invocation, surface),
        },
        EventPayload::RevisionSubmitted {
            revision,
            invocation,
        } => EventPayloadV1::RevisionSubmitted {
            revision: revision.clone(),
            invocation: invocation_v1(invocation, surface),
        },
        EventPayload::RecommendationIssued { recommendation } => {
            EventPayloadV1::RecommendationIssued {
                recommendation: recommendation.clone(),
            }
        }
        EventPayload::HumanDecisionRecorded { authorization } => {
            EventPayloadV1::HumanDecisionRecorded {
                decision: decision_v1(authorization, surface),
            }
        }
        EventPayload::RunFailed {
            reason,
            stage,
            code,
            invocation,
        } => EventPayloadV1::RunFailed {
            stage: stage.clone(),
            code: code.clone(),
            message: failure_message(code.as_deref(), stage),
            reason: operator_only(surface, Some(reason.clone())),
            invocation: invocation.as_ref().map(|i| invocation_v1(i, surface)),
        },
    }
}

pub fn event_v1(ev: &ConsensusEvent, surface: Surface) -> EventV1 {
    EventV1 {
        sequence: ev.sequence,
        event_id: ev.event_id.clone(),
        run_id: ev.run_id.clone(),
        schema_version: ev.schema_version.clone(),
        actor: ev.actor.clone(),
        occurred_at: ev.occurred_at.clone(),
        prev_hash: ev.prev_hash.clone(),
        hash: ev.hash.clone(),
        payload: payload_v1(&ev.payload, surface),
    }
}

/// The chain shape of a run's events.
pub fn chain_v1(events: &[ConsensusEvent]) -> ChainV1 {
    let first = events.first();
    let last = events.last();
    let chained = |e: &ConsensusEvent| !e.hash.is_empty();
    let scope = match (first, last) {
        (Some(f), Some(l)) => {
            if events.iter().all(|e| !chained(e)) {
                ChainScope::Unchained
            } else if !chained(f) {
                ChainScope::PartlyChained
            } else if f.prev_hash == "genesis" {
                ChainScope::Run
            } else {
                let _ = l;
                ChainScope::Log
            }
        }
        _ => ChainScope::Unchained,
    };
    ChainV1 {
        scope,
        head_hash: last.map(|e| e.hash.clone()).filter(|h| !h.is_empty()),
        first_sequence: first.map(|e| e.sequence).unwrap_or(0),
        last_sequence: last.map(|e| e.sequence).unwrap_or(0),
    }
}

/// Where an evidence record came from, from its own fields.
fn evidence_kind(ev: &EvidenceReference) -> EvidenceProvenance {
    match ev.retrieval {
        Some(Retrieval::Simulated) => EvidenceProvenance::Simulated,
        Some(Retrieval::SealedSnapshot) => EvidenceProvenance::HyphaeSnapshot,
        Some(Retrieval::LiveRecall) => EvidenceProvenance::HyphaeLive,
        None => {
            if ev
                .backend_ref
                .as_deref()
                .is_some_and(|r| r.starts_with(FAKE_MEMORY_REF))
            {
                EvidenceProvenance::Simulated
            } else {
                EvidenceProvenance::HyphaeLive
            }
        }
    }
}

/// The provenance axes of a run, derived from its events.
pub fn provenance(events: &[ConsensusEvent], source: SourceProvenance) -> ProvenanceV1 {
    let mut providers: Vec<(&str, bool)> = Vec::new();
    let mut meta_request_ids = Vec::new();
    let mut evidence_kinds: Vec<EvidenceProvenance> = Vec::new();
    let mut declared: Option<TaskMode> = None;
    let mut retrieved: Vec<&EvidenceReference> = Vec::new();
    let note = |kind: EvidenceProvenance, kinds: &mut Vec<EvidenceProvenance>| {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    };
    let mut record_invocation = |inv: &ModelInvocation| {
        providers.push((
            if inv.provider == "fake" {
                "fake"
            } else if inv.provider == "meta" {
                "meta"
            } else {
                "other"
            },
            inv.request_id.is_some(),
        ));
        if inv.provider == "meta" {
            if let Some(id) = &inv.request_id {
                meta_request_ids.push(id.clone());
            }
        }
    };
    for ev in events {
        match &ev.payload {
            EventPayload::TaskCreated { task } => declared = task.mode.clone(),
            EventPayload::ProposalSubmitted { invocation, .. }
            | EventPayload::ChallengeSubmitted { invocation, .. }
            | EventPayload::EvaluationSubmitted { invocation, .. }
            | EventPayload::RevisionSubmitted { invocation, .. } => record_invocation(invocation),
            EventPayload::RunFailed {
                invocation: Some(invocation),
                ..
            } => record_invocation(invocation),
            EventPayload::EvidenceRetrieved { evidence } => {
                retrieved.push(evidence);
                note(evidence_kind(evidence), &mut evidence_kinds);
            }
            EventPayload::EvidenceVerified { verification } => {
                let kind = match verification.verifier.as_deref() {
                    Some(FAKE_VERIFIER) => EvidenceProvenance::Simulated,
                    Some(_) => retrieved
                        .iter()
                        .find(|r| r.id == verification.evidence_id)
                        .map(|r| evidence_kind(r))
                        .unwrap_or(EvidenceProvenance::HyphaeLive),
                    // No verifier ran (proof unavailable): the record's
                    // own retrieval says where it came from.
                    None => match retrieved.iter().find(|r| r.id == verification.evidence_id) {
                        Some(r) => evidence_kind(r),
                        None => continue,
                    },
                };
                note(kind, &mut evidence_kinds);
            }
            _ => {}
        }
    }

    let model = if providers.is_empty() {
        ModelProvenance::Undetermined
    } else if providers.iter().all(|(p, _)| *p == "fake") {
        ModelProvenance::Scripted
    } else if providers.iter().all(|(p, has_id)| *p == "meta" && *has_id) {
        ModelProvenance::LiveMeta
    } else {
        ModelProvenance::Mixed
    };
    let evidence = match evidence_kinds.as_slice() {
        [] => EvidenceProvenance::None,
        [one] => *one,
        _ => EvidenceProvenance::Mixed,
    };

    let mut flags = Vec::new();
    match &declared {
        None => flags.push(ProvenanceFlag::ModeUndeclared),
        Some(mode) => {
            let model_ok = matches!(
                (mode.model, model),
                (_, ModelProvenance::Undetermined)
                    | (ModelMode::Scripted, ModelProvenance::Scripted)
                    | (ModelMode::Meta, ModelProvenance::LiveMeta)
            );
            // Offline declares that nothing can be retrieved, so any
            // evidence at all contradicts it.
            let evidence_ok = matches!(
                (mode.evidence, evidence),
                (_, EvidenceProvenance::None)
                    | (EvidenceMode::Simulated, EvidenceProvenance::Simulated)
                    | (
                        EvidenceMode::SealedSnapshot,
                        EvidenceProvenance::HyphaeSnapshot
                    )
                    | (EvidenceMode::LiveHyphae, EvidenceProvenance::HyphaeLive)
            );
            if !model_ok || !evidence_ok {
                flags.push(ProvenanceFlag::ProvenanceInconsistent);
            }
        }
    }
    if events.iter().any(|e| e.hash.is_empty()) {
        flags.push(ProvenanceFlag::Unchained);
    }
    ProvenanceV1 {
        model,
        evidence,
        source,
        declared,
        flags,
        meta_request_ids,
    }
}

/// The recorded failure of a failed run.
fn failure_v1(events: &[ConsensusEvent], surface: Surface) -> Option<FailureV1> {
    events.iter().rev().find_map(|e| match &e.payload {
        EventPayload::RunFailed {
            reason,
            stage,
            code,
            ..
        } => Some(FailureV1 {
            kind: "failed".into(),
            stage: stage.clone(),
            code: code.clone(),
            message: failure_message(code.as_deref(), stage),
            reason: operator_only(surface, Some(reason.clone())),
        }),
        _ => None,
    })
}

fn decision_of(events: &[ConsensusEvent]) -> Option<&HumanAuthorization> {
    events.iter().rev().find_map(|e| match &e.payload {
        EventPayload::HumanDecisionRecorded { authorization } => Some(authorization),
        _ => None,
    })
}

/// True while a run with this aggregate can still be deliberating: not at
/// its decision point and not terminal. A server's "active" claim is
/// bounded by it, so no view (and no replayed SSE frame) calls a resting
/// run active.
pub fn can_be_active(agg: &RunAggregate) -> bool {
    agg.state != RunState::AwaitingHumanApproval && !agg.state.is_terminal()
}

/// The one-line view of a run. `active` says whether this server is
/// deliberating the run; the records bound it (see [`can_be_active`]).
pub fn summary(
    events: &[ConsensusEvent],
    agg: &RunAggregate,
    source: SourceProvenance,
    active: bool,
    surface: Surface,
) -> RunSummaryV1 {
    let decision = decision_of(events);
    let is_override = decision.is_some_and(|d| d.approved)
        && matches!(
            agg.recommendation.as_ref().map(|r| r.kind),
            Some(RecommendationKind::Blocked | RecommendationKind::Abstained)
        );
    let active = active && can_be_active(agg);
    RunSummaryV1 {
        run_id: agg.run_id.clone(),
        state: agg.state,
        recommendation: agg.recommendation.as_ref().map(|r| r.kind),
        authorization: agg.authorization,
        title: agg.task.as_ref().map(|t| t.title.clone()),
        provenance: provenance(events, source),
        created_at: events
            .first()
            .map(|e| e.occurred_at.clone())
            .unwrap_or_default(),
        updated_at: events
            .last()
            .map(|e| e.occurred_at.clone())
            .unwrap_or_default(),
        event_count: agg.event_count,
        last_sequence: events.last().map(|e| e.sequence).unwrap_or(0),
        revision_round: agg.revision_round,
        failure: (agg.state == RunState::Failed)
            .then(|| failure_v1(events, surface))
            .flatten(),
        is_override,
        active,
    }
}

/// Internal bookkeeping for one Critic objection while walking the events.
struct ItemTrack {
    item: ChallengeItem,
    challenge_id: ChallengeId,
    raised_in_round: u32,
    last_verdict: Option<(ItemStatus, String)>,
}

/// Every Critic objection of the run with its status, derived from the
/// challenges and evaluations in order.
pub fn challenge_items(events: &[ConsensusEvent]) -> Vec<ChallengeItemV1> {
    let mut tracks: Vec<ItemTrack> = Vec::new();
    let mut rounds_done: u32 = 0;
    let mut final_unresolved: Vec<String> = Vec::new();
    for ev in events {
        match &ev.payload {
            EventPayload::ChallengeSubmitted { challenge, .. } => {
                for item in &challenge.items {
                    tracks.push(ItemTrack {
                        item: item.clone(),
                        challenge_id: challenge.id.clone(),
                        raised_in_round: rounds_done + 1,
                        last_verdict: None,
                    });
                }
            }
            EventPayload::EvaluationSubmitted { evaluation, .. } => {
                rounds_done += 1;
                for verdict in &evaluation.item_verdicts {
                    if let Some(track) = tracks.iter_mut().find(|t| t.item.id == verdict.item_id) {
                        track.last_verdict = Some((verdict.status, verdict.reason.clone()));
                    }
                }
                final_unresolved = evaluation.unresolved_items.clone();
            }
            _ => {}
        }
    }
    tracks
        .into_iter()
        .map(|t| {
            let listed = final_unresolved.contains(&t.item.id)
                || final_unresolved.contains(&t.challenge_id.0);
            let (status, resolution_reason) = match &t.last_verdict {
                Some((ItemStatus::Resolved, reason)) => {
                    (ChallengeItemStatus::Resolved, Some(reason.clone()))
                }
                Some((ItemStatus::Unresolved, _)) => (ChallengeItemStatus::Open, None),
                None if listed => (ChallengeItemStatus::Open, None),
                None if t.raised_in_round > rounds_done => (ChallengeItemStatus::Open, None),
                None if t.raised_in_round == rounds_done => (ChallengeItemStatus::Resolved, None),
                None => (ChallengeItemStatus::NeverRejudged, None),
            };
            ChallengeItemV1 {
                id: t.item.id,
                kind: t.item.kind,
                claim_id: t.item.claim_id,
                description: t.item.description,
                status,
                raised_in_round: t.raised_in_round,
                resolution_reason,
            }
        })
        .collect()
}

/// The deliberation rounds, in order.
pub fn rounds(events: &[ConsensusEvent], surface: Surface) -> Vec<RoundV1> {
    let mut rounds: Vec<RoundV1> = Vec::new();
    let mut current_proposal: Option<ProposalId> = None;
    let mut evaluations: u32 = 0;
    let ensure = |rounds: &mut Vec<RoundV1>, round: u32, proposal: &Option<ProposalId>| {
        while (rounds.len() as u32) < round {
            let n = rounds.len() as u32 + 1;
            rounds.push(RoundV1 {
                round: n,
                proposal_id: proposal.clone(),
                challenge: None,
                evaluation: None,
                verifications: Vec::new(),
            });
        }
    };
    for ev in events {
        match &ev.payload {
            EventPayload::ProposalSubmitted { proposal, .. } => {
                current_proposal = Some(proposal.id.clone());
            }
            EventPayload::RevisionSubmitted { revision, .. } => {
                current_proposal = Some(revision.proposal.id.clone());
            }
            EventPayload::ChallengeSubmitted { challenge, .. } => {
                let round = evaluations + 1;
                ensure(&mut rounds, round, &current_proposal);
                let entry = &mut rounds[(round - 1) as usize];
                entry.proposal_id = current_proposal.clone();
                entry.challenge = Some(challenge.clone());
            }
            EventPayload::EvaluationSubmitted { evaluation, .. } => {
                evaluations += 1;
                ensure(&mut rounds, evaluations, &current_proposal);
                let entry = &mut rounds[(evaluations - 1) as usize];
                if entry.proposal_id.is_none() {
                    entry.proposal_id = current_proposal.clone();
                }
                entry.evaluation = Some(evaluation.clone());
            }
            EventPayload::EvidenceVerified { verification } => {
                // Records after the k-th evaluation belong to round k; the
                // ones before any evaluation are shown with round 1.
                let round = verification.round.unwrap_or(evaluations).max(1);
                ensure(&mut rounds, round, &current_proposal);
                rounds[(round - 1) as usize]
                    .verifications
                    .push(verification_v1(verification, surface));
            }
            _ => {}
        }
    }
    rounds
}

/// The recommendation with its re-derived policy trace.
pub fn recommendation_v1(
    events: &[ConsensusEvent],
    agg: &RunAggregate,
) -> Option<RecommendationV1> {
    let rec = agg.recommendation.as_ref()?;
    let issued_version = events
        .iter()
        .rev()
        .find(|e| matches!(e.payload, EventPayload::RecommendationIssued { .. }))
        .map(|e| e.schema_version.as_str())
        .unwrap_or(SCHEMA_VERSION);
    let run_version = events
        .first()
        .map(|e| e.schema_version.as_str())
        .unwrap_or(SCHEMA_VERSION);
    let trace = match (
        PolicyVersion::for_schema(issued_version),
        &agg.last_evaluation,
    ) {
        (Some(version), Some(evaluation)) => {
            Some(explain(version, evaluation, &agg.round_verifications))
        }
        _ => None,
    };
    Some(RecommendationV1 {
        kind: rec.kind,
        rationale: rec.rationale.clone(),
        rationale_code: trace.as_ref().map(|t| rationale_code(t.rule).to_string()),
        unresolved_assumptions: rec.unresolved_assumptions.clone(),
        unresolved_uncertainties: rec.unresolved_uncertainties.clone(),
        issued_at: rec.issued_at.clone(),
        trace,
        protocol: protocol_label(run_version),
    })
}

fn current_proposal(events: &[ConsensusEvent]) -> Option<Proposal> {
    events.iter().rev().find_map(|e| match &e.payload {
        EventPayload::ProposalSubmitted { proposal, .. } => Some(proposal.clone()),
        EventPayload::RevisionSubmitted { revision, .. } => Some(revision.proposal.clone()),
        _ => None,
    })
}

/// The full view of a run.
pub fn detail(
    events: &[ConsensusEvent],
    agg: &RunAggregate,
    source: SourceProvenance,
    active: bool,
    surface: Surface,
) -> RunDetailV1 {
    RunDetailV1 {
        summary: summary(events, agg, source, active, surface),
        task: agg.task.clone(),
        current_proposal: current_proposal(events),
        rounds: rounds(events, surface),
        challenge_items: challenge_items(events),
        recommendation: recommendation_v1(events, agg),
        decision: decision_of(events).map(|d| decision_v1(d, surface)),
        model_invocations: agg
            .invocations
            .iter()
            .map(|i| invocation_v1(i, surface))
            .collect(),
        events: events.iter().map(|e| event_v1(e, surface)).collect(),
        chain: chain_v1(events),
    }
}

/// A receipt as the surface may show it: on the public surface the
/// recorded failure reason becomes the fixed statement for its code, a
/// real verifier's output is dropped, and a model call keeps the same
/// fields the public views show (provider, model, request id, latency,
/// tokens, finish reason), never the prompt and schema digests, the
/// output bound or the attempt count. The chain fields are untouched:
/// they commit to the events, not to the receipt.
pub fn sanitize_receipt(mut receipt: RunReceiptV2, surface: Surface) -> RunReceiptV2 {
    if surface == Surface::Operator {
        return receipt;
    }
    if let Some(failure) = &mut receipt.failure {
        failure.reason = failure_message(failure.code.as_deref(), &failure.stage);
    }
    for rv in &mut receipt.verifications {
        if rv.verification.verifier.as_deref() != Some(FAKE_VERIFIER) {
            rv.verification.detail = None;
        }
    }
    for inv in &mut receipt.model_invocations {
        inv.max_completion_tokens = None;
        inv.schema_name = None;
        inv.schema_sha256 = None;
        inv.prompt_sha256 = None;
        inv.attempts = None;
        // Prompt-freeze-3's re-ask record is outside the api-v1 contract
        // in this window: the operator receipt carries it, the public one
        // keeps its schema (the summed tokens stay).
        inv.rejected_calls = None;
        inv.corrective_sha256 = None;
    }
    receipt
}

/// Whether a run's verbatim stored records (`events.ndjson`) may be
/// published on `surface`. The operator sees every chain. The public
/// surface serves only a chain that cannot carry upstream text: the run
/// was declared with a scripted model over simulated (or no) evidence,
/// and its records prove nothing else. A live adapter's failure reason
/// may quote the upstream response, and the chain commits to that text,
/// so sanitizing it would break verification; such a run's detail and
/// receipt, which strip the reason, stay available, and its chain is
/// published whole only to the operator until the adapters record stable
/// codes instead of upstream text (lane R-b), when this rule can be lifted
/// here in one place.
pub fn ndjson_publishable(events: &[ConsensusEvent], surface: Surface) -> bool {
    if surface == Surface::Operator {
        return true;
    }
    let p = provenance(events, SourceProvenance::ThisServer);
    let declared_scripted = p.declared.as_ref().is_some_and(|mode| {
        mode.model == ModelMode::Scripted
            && matches!(
                mode.evidence,
                EvidenceMode::Simulated | EvidenceMode::Offline
            )
    });
    declared_scripted
        && matches!(
            p.model,
            ModelProvenance::Scripted | ModelProvenance::Undetermined
        )
        && matches!(
            p.evidence,
            EvidenceProvenance::Simulated | EvidenceProvenance::None
        )
}

/// Replays `events` into an aggregate. Stops at the first event replay
/// refuses, marking the run failed, so a damaged history is shown as such
/// rather than hidden.
pub fn aggregate_of(events: &[ConsensusEvent]) -> Option<RunAggregate> {
    let first = events.first()?;
    let mut agg = RunAggregate::new(first.run_id.clone());
    for ev in events {
        if let Err(e) = agg.apply(ev) {
            if agg.failure_reason.is_none() {
                agg.failure_reason = Some(format!("replay error at seq {}: {e}", ev.sequence));
                agg.state = RunState::Failed;
            }
            break;
        }
    }
    Some(agg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::GuidedScenario;
    use crate::guided;
    use consensus_runtime::clock::{FixedClock, SequentialIdGen};
    use consensus_runtime::scenarios::TS;
    use consensus_runtime::{EventLog, Orchestrator, RunLimits};

    fn guided_events(scenario: GuidedScenario, origin: RunOrigin) -> Vec<ConsensusEvent> {
        let dir = std::env::temp_dir().join(format!("hyphae-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "{}-{:?}-{}.jsonl",
            scenario.as_str(),
            origin,
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&path);
        let mut log = EventLog::open(&path).unwrap();
        let mut s = guided::build(scenario);
        s.input.mode = Some(guided::task_mode(scenario, origin));
        let mut orch = Orchestrator {
            model: s.model,
            memory: s.memory,
            verifier: s.verifier,
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        orch.run(&mut log, s.input).unwrap();
        log.all_events().to_vec()
    }

    #[test]
    fn provenance_is_derived_from_records_and_cross_checked() {
        let events = guided_events(GuidedScenario::Retry, RunOrigin::Public);
        let p = provenance(&events, SourceProvenance::ThisServer);
        assert_eq!(p.model, ModelProvenance::Scripted);
        assert_eq!(p.evidence, EvidenceProvenance::Simulated);
        assert!(p.flags.is_empty(), "{:?}", p.flags);
        assert!(p.meta_request_ids.is_empty());

        // A declaration the records contradict is flagged, never trusted.
        let mut forged = events.clone();
        if let EventPayload::TaskCreated { task } = &mut forged[0].payload {
            task.mode = Some(TaskMode {
                model: ModelMode::Meta,
                evidence: EvidenceMode::LiveHyphae,
                origin: RunOrigin::Public,
                preset: None,
            });
        }
        let p = provenance(&forged, SourceProvenance::ThisServer);
        assert_eq!(p.model, ModelProvenance::Scripted);
        assert!(p.flags.contains(&ProvenanceFlag::ProvenanceInconsistent));

        // Only the task so far: nothing is known about the model.
        let p = provenance(&events[..1], SourceProvenance::ThisServer);
        assert_eq!(p.model, ModelProvenance::Undetermined);
        assert_eq!(p.evidence, EvidenceProvenance::None);
        assert!(p.flags.is_empty());
    }

    #[test]
    fn public_views_strip_internals_and_operator_views_keep_them() {
        let events = guided_events(GuidedScenario::Offline, RunOrigin::Operator);
        let agg = aggregate_of(&events).unwrap();
        let public = detail(
            &events,
            &agg,
            SourceProvenance::ThisServer,
            false,
            Surface::Public,
        );
        let operator = detail(
            &events,
            &agg,
            SourceProvenance::ThisServer,
            false,
            Surface::Operator,
        );
        let unavailable_public = public
            .events
            .iter()
            .find_map(|e| match &e.payload {
                EventPayloadV1::EvidenceUnavailable {
                    reason, message, ..
                } => Some((reason.clone(), message.clone())),
                _ => None,
            })
            .unwrap();
        assert!(unavailable_public.0.is_none());
        assert!(!unavailable_public.1.is_empty());
        let unavailable_operator = operator
            .events
            .iter()
            .find_map(|e| match &e.payload {
                EventPayloadV1::EvidenceUnavailable { reason, .. } => reason.clone(),
                _ => None,
            })
            .unwrap();
        assert!(!unavailable_operator.is_empty());
        assert_eq!(
            public.summary.recommendation,
            Some(RecommendationKind::InsufficientEvidence)
        );
        assert_eq!(public.rounds.len(), 2);
        assert!(public.recommendation.as_ref().unwrap().trace.is_some());
        assert_eq!(public.chain.scope, ChainScope::Run);
    }

    #[test]
    fn challenge_items_follow_verdicts() {
        let events = guided_events(GuidedScenario::Retry, RunOrigin::Public);
        let items = challenge_items(&events);
        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .all(|i| i.status == ChallengeItemStatus::Resolved && i.resolution_reason.is_some()));
        assert!(items.iter().all(|i| i.raised_in_round == 1));
    }

    #[test]
    fn failure_messages_never_echo_the_reason() {
        let msg = failure_message(Some("panic"), "panic");
        assert!(!msg.contains("panicked at"));
        assert_eq!(
            failure_message(None, "propose"),
            "The run failed at stage propose."
        );
    }

    #[test]
    fn a_resting_run_is_never_active_whatever_the_server_claims() {
        let events = guided_events(GuidedScenario::Retry, RunOrigin::Public);
        let agg = aggregate_of(&events).unwrap();
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
        assert!(!can_be_active(&agg));
        let s = summary(
            &events,
            &agg,
            SourceProvenance::ThisServer,
            true,
            Surface::Public,
        );
        assert!(!s.active, "the claim is bounded by the records");
        // Mid-deliberation the claim stands.
        let created = aggregate_of(&events[..1]).unwrap();
        assert!(can_be_active(&created));
        let s = summary(
            &events[..1],
            &created,
            SourceProvenance::ThisServer,
            true,
            Surface::Public,
        );
        assert!(s.active);
        let s = summary(
            &events[..1],
            &created,
            SourceProvenance::ThisServer,
            false,
            Surface::Public,
        );
        assert!(!s.active);
    }

    #[test]
    fn ndjson_is_published_only_for_chains_that_cannot_carry_upstream_text() {
        let scripted = guided_events(GuidedScenario::Retry, RunOrigin::Public);
        assert!(ndjson_publishable(&scripted, Surface::Public));
        assert!(ndjson_publishable(&scripted, Surface::Operator));
        // Declared live: not on the public surface, even before any call.
        let mut live = scripted.clone();
        if let EventPayload::TaskCreated { task } = &mut live[0].payload {
            task.mode = Some(TaskMode {
                model: ModelMode::Meta,
                evidence: EvidenceMode::Simulated,
                origin: RunOrigin::Public,
                preset: Some("live:retry-cap-3".into()),
            });
        }
        assert!(!ndjson_publishable(&live, Surface::Public));
        assert!(!ndjson_publishable(&live[..1], Surface::Public));
        assert!(ndjson_publishable(&live, Surface::Operator));
        // No declaration at all (pre-1.3 records): not published either.
        let mut undeclared = scripted.clone();
        if let EventPayload::TaskCreated { task } = &mut undeclared[0].payload {
            task.mode = None;
        }
        assert!(!ndjson_publishable(&undeclared, Surface::Public));
        // The scripted offline scenario declares simulated evidence and
        // records none: published.
        let offline = guided_events(GuidedScenario::Offline, RunOrigin::Public);
        assert!(ndjson_publishable(&offline, Surface::Public));
    }

    #[test]
    fn public_receipts_keep_only_the_public_invocation_fields() {
        use consensus_runtime::build_receipt_v2;
        let events = guided_events(GuidedScenario::Retry, RunOrigin::Public);
        let mut receipt = build_receipt_v2(&events, TS.into()).unwrap();
        for inv in &mut receipt.model_invocations {
            inv.prompt_sha256 = Some("p".repeat(64));
            inv.schema_sha256 = Some("s".repeat(64));
            inv.schema_name = Some("proposal".into());
            inv.max_completion_tokens = Some(4096);
            inv.attempts = Some(2);
            inv.corrective_sha256 = Some("c".repeat(64));
            inv.rejected_calls = Some(vec![RejectedCall {
                request_id: Some("chatcmpl-rejected".into()),
                code: "model_no_claims".into(),
                finish_reason: Some("stop".into()),
                latency_ms: Some(9),
                input_tokens: Some(3),
                output_tokens: Some(4),
                reasoning_tokens: None,
                attempts: Some(1),
            }]);
        }
        let operator = sanitize_receipt(receipt.clone(), Surface::Operator);
        assert!(operator.model_invocations[0].prompt_sha256.is_some());
        assert!(operator.model_invocations[0].rejected_calls.is_some());
        assert!(operator.model_invocations[0].corrective_sha256.is_some());
        let text =
            serde_json::to_string(&sanitize_receipt(receipt.clone(), Surface::Public)).unwrap();
        assert!(
            !text.contains("rejected_calls") && !text.contains("corrective_sha256"),
            "{text}"
        );
        let public = sanitize_receipt(receipt, Surface::Public);
        assert!(!public.model_invocations.is_empty());
        for inv in &public.model_invocations {
            assert!(inv.prompt_sha256.is_none());
            assert!(inv.schema_sha256.is_none());
            assert!(inv.schema_name.is_none());
            assert!(inv.max_completion_tokens.is_none());
            assert!(inv.attempts.is_none());
            assert_eq!(inv.provider, "fake");
        }
    }
}
