// SPDX-License-Identifier: Apache-2.0
//! The `/api/v1` contract: every request body, response body and SSE
//! frame, each with a JSON Schema (`contracts/api-v1/`).
//!
//! Requests use `deny_unknown_fields` and enums, never free strings: an
//! unknown field or a bad value is `422 invalid_request`. The public
//! server accepts no visitor text at all; the operator server's free
//! fields are bounded and belong to the operator.
//!
//! Response types are *views*: they are derived from a run's chained
//! events and never persisted. A view carries the provenance axes the
//! records prove ([`ProvenanceV1`]) and omits what a visitor must not
//! see; fields marked "operator surface only" are `None` on the public
//! server.

use consensus_domain::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------

/// The five guided scenarios: scripted model doubles over seeded records
/// with simulated proofs. Each reaches a documented outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuidedScenario {
    /// Reference: a proposal violates a stored constraint, the critic
    /// catches it, the revision complies. Expected `supported`.
    Retry,
    /// Memory offline: nothing can be retrieved. Expected
    /// `insufficient_evidence`.
    Offline,
    /// A proof fails verification. Expected `blocked`.
    InvalidProof,
    /// A hostile memory record tries to force approval. Expected
    /// `insufficient_evidence`.
    Injection,
    /// No evidence exists; the evaluator abstains. Expected `abstained`.
    Abstain,
}

impl GuidedScenario {
    pub const ALL: [GuidedScenario; 5] = [
        GuidedScenario::Retry,
        GuidedScenario::Offline,
        GuidedScenario::InvalidProof,
        GuidedScenario::Injection,
        GuidedScenario::Abstain,
    ];

    /// The serialized name, e.g. `invalid_proof`.
    pub fn as_str(self) -> &'static str {
        match self {
            GuidedScenario::Retry => "retry",
            GuidedScenario::Offline => "offline",
            GuidedScenario::InvalidProof => "invalid_proof",
            GuidedScenario::Injection => "injection",
            GuidedScenario::Abstain => "abstain",
        }
    }
}

/// `POST /api/v1/runs` (public): start a guided run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGuidedRunRequest {
    pub scenario: GuidedScenario,
}

/// Identifier of a curated live preset: 1 to 40 characters of
/// `[A-Za-z0-9-]`, the shape of the battery tags the presets are named
/// after (`R01-reference`). A key into a closed server-side set, matched
/// exactly by the gate; never text a model sees.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct PresetId(String);

impl PresetId {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.is_empty() || value.len() > 40 {
            return Err("preset_id must be 1 to 40 characters");
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("preset_id may only contain A-Z, a-z, 0-9 and -");
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PresetId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        PresetId::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Longest judge code accepted (the live gate hashes it; see
/// [`crate::live::judge::MAX_CODE_LEN`]).
pub const JUDGE_CODE_MAX_CHARS: usize = 256;

/// `POST /api/v1/live-runs` (public): start a live run from a preset, on
/// the owner's key (ADR-0007). Admission needs a Turnstile token in the
/// `cf-turnstile-response` header, `attest_adult: true`, and a country
/// Cloudflare reports; `503 live_disabled` when live mode is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLiveRunRequest {
    pub preset_id: PresetId,
    /// The visitor's 18+ and territory acknowledgment. Absent reads as
    /// false, which the gate refuses (`422 attestation_required`).
    #[serde(default)]
    pub attest_adult: bool,
    /// A judge-lane invite code, 1 to 256 characters once trimmed. An
    /// unknown code is not an error: the run takes the public lane, and
    /// nothing in the response says whether codes exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_code: Option<String>,
}

impl CreateLiveRunRequest {
    /// Field bounds, checked after deserialization. The trimmed code is
    /// what the gate verifies.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(code) = &self.judge_code {
            let trimmed = code.trim();
            if trimmed.is_empty() || trimmed.chars().count() > JUDGE_CODE_MAX_CHARS {
                return Err(format!(
                    "judge_code must be 1 to {JUDGE_CODE_MAX_CHARS} characters"
                ));
            }
            if trimmed.chars().any(char::is_control) {
                return Err("judge_code may not contain control characters".into());
            }
        }
        Ok(())
    }

    /// The judge code as the gate verifies it: trimmed, or none.
    pub fn judge_code_trimmed(&self) -> Option<String> {
        self.judge_code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
    }
}

/// Operator surface: which model answers the roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperatorModel {
    /// Scripted doubles; a `scenario` is required.
    Fake,
    /// The Meta Model API, with the server's credentials.
    Meta,
}

/// Operator surface: which memory backend serves evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperatorMemory {
    /// Seeded records with simulated proofs.
    Fake,
    /// No backend: retrieval fails and the run degrades.
    Offline,
    /// A running Hyphae deployment (operator lab only).
    Hyphae,
}

/// Bounded text from an operator or a visitor (the decision note): at most
/// `max` characters, no control characters other than newlines and tabs,
/// and none of the invisible characters that make stored text read
/// differently from what it holds: the bidirectional embedding, override
/// and isolate controls (Trojan Source) and the zero-width space, word
/// joiner and byte-order mark. Joiners (U+200C, U+200D) and the
/// directional marks stay accepted: scripts and emoji need them.
fn check_text(name: &str, value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        return Err(format!("{name} may have at most {max} characters"));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        return Err(format!("{name} may not contain control characters"));
    }
    if value.chars().any(is_hidden_format_character) {
        return Err(format!(
            "{name} may not contain bidirectional controls or zero-width characters"
        ));
    }
    Ok(())
}

/// The invisible format characters [`check_text`] refuses.
fn is_hidden_format_character(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}'
    )
}

/// `POST /api/v1/operator/runs` (operator): start a run in any mode.
/// `model=fake` needs a `scenario` and `memory=fake`; `model=meta` takes
/// a bounded title and description for the Proposer and a project id for
/// `memory=hyphae`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateOperatorRunRequest {
    pub model: OperatorModel,
    pub memory: OperatorMemory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<GuidedScenario>,
    /// At most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// At most 4000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Hyphae project, `[A-Za-z0-9._-]{1,64}`. Default `demo-repo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl CreateOperatorRunRequest {
    /// Field bounds, checked after deserialization.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(title) = &self.title {
            check_text("title", title, 200)?;
            if title.trim().is_empty() {
                return Err("title may not be empty".into());
            }
        }
        if let Some(description) = &self.description {
            check_text("description", description, 4000)?;
        }
        if let Some(project) = &self.project_id {
            if project.is_empty()
                || project.len() > 64
                || !project
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            {
                return Err("project_id must match [A-Za-z0-9._-]{1,64}".into());
            }
        }
        match self.model {
            OperatorModel::Fake => {
                if self.scenario.is_none() {
                    return Err("model=fake requires a scenario".into());
                }
                if self.memory != OperatorMemory::Fake {
                    return Err(
                        "model=fake runs a scripted scenario with seeded fake memory; any other memory would be silently ignored, use memory=fake or model=meta"
                            .into(),
                    );
                }
            }
            OperatorModel::Meta => {
                if self.scenario.is_some() {
                    return Err("scenario applies to model=fake only".into());
                }
            }
        }
        Ok(())
    }
}

/// What the human saw when deciding; the server refuses the decision
/// (`409 decision_conflict`) if the run no longer matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpectedRunState {
    pub recommendation: RecommendationKind,
    /// The sequence of the run's last event as shown to the human.
    pub last_sequence: u64,
}

/// `POST /api/v1/runs/{id}/decision`, with an `Idempotency-Key` header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
    pub approved: bool,
    /// At most 2000 characters. Required to approve a `blocked` or
    /// `abstained` run (an override).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub expected: ExpectedRunState,
}

impl DecisionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(note) = &self.note {
            check_text("note", note, 2000)?;
        }
        Ok(())
    }
}

/// `GET /api/v1/runs` query: cursor pagination, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRunsQuery {
    /// The `next_cursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// 1 to 50; default 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `GET /api/v1/runs/{id}/events` query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsQuery {
    /// Replay events with a sequence greater than this; `Last-Event-ID`
    /// takes precedence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
}

// ---------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------

/// Which model answered the roles, read from the chained
/// `ModelInvocation` records (provider and request id), never from the
/// request or the declared mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvenance {
    /// Every recorded call came from a scripted double (`provider=fake`):
    /// no model was called.
    Scripted,
    /// Every recorded call went to the Meta Model API (`provider=meta`
    /// with a request id).
    LiveMeta,
    /// Calls from more than one provider, or Meta calls without request
    /// ids: shown with a flag, never as LIVE.
    Mixed,
    /// No model call recorded yet.
    Undetermined,
}

/// Where the evidence came from, read from the verification and
/// retrieval records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceProvenance {
    /// Seeded records checked by the scripted verifier: SIMULATED.
    Simulated,
    /// Records of a sealed Hyphae snapshot, verified with `hyphae proof
    /// verify`.
    HyphaeSnapshot,
    /// Records recalled from a running Hyphae deployment.
    HyphaeLive,
    /// No evidence record in the run.
    None,
    /// Records of more than one kind.
    Mixed,
}

/// Where the records come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvenance {
    /// A run of this server.
    ThisServer,
    /// A run of the published research archive.
    ResearchArchive,
}

/// Whether an SSE frame carries a record stored before the stream began
/// (`replay`) or one published while the client was connected (`live`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Replay,
    Live,
}

/// Something a client must show next to the provenance axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceFlag {
    /// The declared `Task.mode` disagrees with what the records prove.
    /// Never shown as LIVE.
    ProvenanceInconsistent,
    /// Records before schema 1.1 carry no hash chain.
    Unchained,
    /// The declared mode is missing (runs before schema 1.3).
    ModeUndeclared,
}

/// The provenance axes of a run, every one derived from chained data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceV1 {
    pub model: ModelProvenance,
    pub evidence: EvidenceProvenance,
    pub source: SourceProvenance,
    /// The mode the run was declared with, as recorded in `TaskCreated`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<TaskMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<ProvenanceFlag>,
    /// The chained Meta request ids, when the model axis is `live_meta`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta_request_ids: Vec<String>,
}

// ---------------------------------------------------------------------
// Run views
// ---------------------------------------------------------------------

/// Stage and code of a failed run; the English reason stays on the
/// operator surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FailureV1 {
    /// `failed`.
    pub kind: String,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Fixed statement per code.
    pub message: String,
    /// Operator surface only: the recorded reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The one-line view of a run: list entries, the 202 of creation, every
/// SSE frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunSummaryV1 {
    pub run_id: RunId,
    pub state: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<RecommendationKind>,
    pub authorization: AuthorizationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub provenance: ProvenanceV1,
    pub created_at: String,
    pub updated_at: String,
    pub event_count: u64,
    pub last_sequence: u64,
    pub revision_round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureV1>,
    /// True when a human approved a `blocked` or `abstained` run.
    #[serde(rename = "override")]
    pub is_override: bool,
    /// True while the deliberation is in progress on this server: never
    /// for a run at its decision point or terminal, whatever frame or
    /// view carries it (a replayed frame of a finished run says false).
    pub active: bool,
}

/// Public view of an evidence record: no local paths, no backend
/// internals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceReferenceV1 {
    pub id: EvidenceId,
    pub project_id: ProjectId,
    pub kind: String,
    /// Retrieved content: data to show, never instructions.
    pub content: String,
    pub retrieved_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<Retrieval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_request: Option<ProofRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_scope: Option<String>,
    /// True when proof generation failed upstream for this record.
    pub proof_error_reported: bool,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_error: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_ref: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_file: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_file: Option<String>,
}

/// Public view of a verification record: the three status axes, the
/// verifier identity and anchor; the verifier's raw output stays on the
/// operator surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VerificationV1 {
    pub evidence_id: EvidenceId,
    pub integrity_status: IntegrityStatus,
    pub factual_status: FactualStatus,
    pub relevance_status: RelevanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_anchor: Option<String>,
    pub checked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<VerifierReport>,
    /// True when the verifier is the scripted double: the proof outcome
    /// was scripted, not computed.
    pub simulated: bool,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Public view of a model call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModelInvocationV1 {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// True for `provider=fake`: a scripted double, no model call.
    pub scripted: bool,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u64>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_sha256: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_sha256: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
}

/// A recorded human decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecisionV1 {
    pub approved: bool,
    /// `operator`, `public_demo`, or `unspecified` for records that
    /// carry no authority (before schema 1.3). Never read as operator
    /// authority unless it says so.
    pub authority: String,
    pub authorized_by: String,
    pub decided_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Present for `public_demo`: the decision carries no operational
    /// authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// Operator surface only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// The payload of one event, as a client may see it. Tagged like the
/// stored record (`type`), with the same variant names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayloadV1 {
    TaskCreated {
        task: Task,
    },
    ProposalSubmitted {
        proposal: Proposal,
        invocation: ModelInvocationV1,
    },
    EvidenceRetrieved {
        evidence: EvidenceReferenceV1,
    },
    EvidenceUnavailable {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        message: String,
        /// Operator surface only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    RecallProven {
        request: ProofRequest,
        evidence_ids: Vec<EvidenceId>,
        proof_ref: String,
        anchor: String,
        empty: bool,
    },
    EvidenceVerified {
        verification: VerificationV1,
    },
    ChallengeSubmitted {
        challenge: Challenge,
        invocation: ModelInvocationV1,
    },
    EvaluationSubmitted {
        evaluation: Evaluation,
        invocation: ModelInvocationV1,
    },
    RevisionSubmitted {
        revision: Revision,
        invocation: ModelInvocationV1,
    },
    RecommendationIssued {
        recommendation: Recommendation,
    },
    HumanDecisionRecorded {
        decision: DecisionV1,
    },
    RunFailed {
        stage: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        message: String,
        /// Operator surface only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation: Option<ModelInvocationV1>,
    },
}

/// One event of a run, with its chain fields, so a client can show the
/// chain. The verbatim stored line is at `events.ndjson`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventV1 {
    pub sequence: u64,
    pub event_id: EventId,
    pub run_id: RunId,
    pub schema_version: String,
    pub actor: Actor,
    pub occurred_at: String,
    pub prev_hash: String,
    pub hash: String,
    pub payload: EventPayloadV1,
}

/// One deliberation round: the proposal judged, the critique, the
/// evaluation and the verifications recorded for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoundV1 {
    /// 1-based.
    pub round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<ProposalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<Challenge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<Evaluation>,
    pub verifications: Vec<VerificationV1>,
}

/// What became of a Critic objection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeItemStatus {
    /// Still open: unresolved by the latest evaluation, or never judged.
    Open,
    /// Resolved by an explicit verdict with a reason.
    Resolved,
    /// Raised in an earlier round and never judged again (pre-1.3
    /// protocol): counted as resolved then, without a stated reason.
    NeverRejudged,
}

/// One Critic objection across the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChallengeItemV1 {
    pub id: String,
    pub kind: ChallengeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<ClaimId>,
    pub description: String,
    pub status: ChallengeItemStatus,
    pub raised_in_round: u32,
    /// The reason of the verdict that resolved it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_reason: Option<String>,
}

/// The recommendation with its policy trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecommendationV1 {
    pub kind: RecommendationKind,
    pub rationale: String,
    /// Stable code of the rule that fired, e.g.
    /// `contested.unresolved_items`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale_code: Option<String>,
    pub unresolved_assumptions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_uncertainties: Vec<String>,
    pub issued_at: String,
    /// `policy::explain` over the records the recommendation was issued
    /// on. None for a recommendation recorded before schema 1.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<PolicyTrace>,
    /// `policy 1.2`, or `protocol 1.3 · policy rules 1.2`.
    pub protocol: String,
}

/// What the chain of this run's events looks like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChainV1 {
    pub scope: ChainScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_hash: Option<String>,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

/// The full view of one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunDetailV1 {
    pub summary: RunSummaryV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    /// The proposal as it stands: the last proposal or revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_proposal: Option<Proposal>,
    pub rounds: Vec<RoundV1>,
    pub challenge_items: Vec<ChallengeItemV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<RecommendationV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionV1>,
    pub model_invocations: Vec<ModelInvocationV1>,
    pub events: Vec<EventV1>,
    pub chain: ChainV1,
}

/// Routes of a run, relative to the server origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunLinksV1 {
    #[serde(rename = "self")]
    pub self_: String,
    pub events: String,
    pub events_ndjson: String,
    pub receipt: String,
    pub decision: String,
    pub cancel: String,
}

impl RunLinksV1 {
    pub fn for_run(run_id: &RunId) -> Self {
        let base = format!("/api/v1/runs/{run_id}");
        Self {
            self_: base.clone(),
            events: format!("{base}/events"),
            events_ndjson: format!("{base}/events.ndjson"),
            receipt: format!("{base}/receipt"),
            decision: format!("{base}/decision"),
            cancel: format!("{base}/cancel"),
        }
    }
}

/// `202 Accepted` of run creation: the run exists and its task is
/// durable; the deliberation follows in the background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunCreatedV1 {
    pub run: RunSummaryV1,
    pub links: RunLinksV1,
}

/// `GET /api/v1/runs`: the caller's runs, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunListV1 {
    pub runs: Vec<RunSummaryV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `200` of a recorded decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecisionResponseV1 {
    pub run: RunSummaryV1,
    pub decision: DecisionV1,
}

/// `202` of a cancel request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CancelResponseV1 {
    pub run_id: RunId,
    /// The run stops at its next checkpoint and records
    /// `run_failed{stage: cancelled}`.
    pub cancelling: bool,
}

// ---------------------------------------------------------------------
// SSE frames
// ---------------------------------------------------------------------

/// `event: consensus`: one event of the run, with its delivery and the
/// summary after applying it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsensusFrameV1 {
    pub event: EventV1,
    pub delivery: Delivery,
    pub summary: RunSummaryV1,
}

/// Why a stream closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamEndReason {
    /// The run awaits a human decision; decide, then fetch the detail.
    AwaitingHumanApproval,
    /// The run reached a terminal state.
    Terminal,
}

/// `event: stream_end`: the last frame of a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StreamEndV1 {
    pub reason: StreamEndReason,
    pub state: RunState,
    pub last_sequence: u64,
}

/// Why the server re-read the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    /// The subscriber fell behind the broadcast buffer.
    Lagged,
    /// A published sequence skipped one or more records.
    Gap,
    /// The run's live channel closed; the durable tail follows.
    Closed,
}

/// `event: resync`: the frames that follow were re-read from the durable
/// log (delivery `replay`), starting after `from_sequence`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResyncV1 {
    pub reason: ResyncReason,
    pub from_sequence: u64,
}

/// One SSE frame, for the contract and tests. On the wire each variant
/// is an SSE event of the same name whose `data` is the inner JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum SseFrameV1 {
    Consensus(Box<ConsensusFrameV1>),
    Resync(ResyncV1),
    StreamEnd(StreamEndV1),
}

// ---------------------------------------------------------------------
// Sessions, scenarios, capabilities, health, showcase
// ---------------------------------------------------------------------

/// `POST /api/v1/session`, `GET /api/v1/session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionV1 {
    /// True when this response minted the session.
    pub created: bool,
    pub expires_at: String,
    /// The pseudonym recorded as `authorized_by` on this session's
    /// decisions.
    pub pseudonym: String,
}

/// One guided scenario, as `GET /api/v1/scenarios` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioV1 {
    pub id: GuidedScenario,
    pub title: String,
    pub description: String,
    pub expected_outcome: RecommendationKind,
    /// Always true: scripted model, simulated proofs.
    pub simulated: bool,
    pub model: ModelProvenance,
    pub evidence: EvidenceProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenariosV1 {
    pub scenarios: Vec<ScenarioV1>,
}

/// Which server answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceV1 {
    Public,
    Operator,
}

/// Every limit the server enforces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LimitsV1 {
    pub body_limit_bytes: u64,
    pub guided_concurrency: u64,
    pub live_concurrency: u64,
    pub run_deadline_secs: u64,
    pub max_run_tokens: u64,
    pub sse_max_per_run: u64,
    pub sse_max_per_client: u64,
    pub sse_max_global: u64,
    pub sse_heartbeat_secs: u64,
    pub rate_session_per_min: u64,
    pub rate_runs_per_min: u64,
    pub rate_sse_per_min: u64,
    pub session_ttl_secs: u64,
    pub session_max: u64,
    /// Visitor runs the store keeps at once; when full, the oldest
    /// resting visitor runs are purged to admit a new one.
    pub guided_ring: u64,
    pub visitor_access_hours: u64,
    pub visitor_delete_days: u64,
    pub list_page_max: u64,
    /// Seconds a request may take to be handled (body read included;
    /// a stream's body is not covered once its headers are sent).
    pub request_timeout_secs: u64,
}

/// One curated live preset, as `/capabilities` lists it: a canned task
/// the recorded batteries ran, never visitor text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresetV1 {
    /// The battery tag, e.g. `R03-raise-to-10`: the `preset_id` to send.
    pub id: String,
    pub title: String,
    pub description: String,
    /// The project every preset runs against (`demo-repo`).
    pub project: String,
    /// Always `live_meta`: the owner's Meta key answers every role.
    pub model: ModelProvenance,
    /// Where the evidence comes from in this deployment (the sealed
    /// snapshot in production).
    pub evidence: EvidenceProvenance,
}

/// What `/capabilities` publishes about the live budget: remaining
/// amounts as the ledger renders them (USD, six decimals), whether live
/// mode is tripped and the coarse reason code. Never an amount spent, a
/// detail string, an upstream status or a path; the judge lane is not
/// mentioned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublicBudgetV1 {
    /// Today's remaining public budget (UTC day).
    pub daily_remaining: String,
    /// What the public lane could reserve right now, pacing included.
    pub daily_available_now: String,
    pub lifetime_remaining: String,
    /// True when the ledger refuses every admission until an operator
    /// resets it, or when the operator's kill-switch file exists (reported
    /// at once, before the next admission records the trip).
    pub tripped: bool,
    /// `kill_switch`, `lifetime_cap`, `output_bound`, `charge_bound`,
    /// `provider_refused_key` or `inconsistent`, when tripped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip_code: Option<String>,
}

/// The live section of `/capabilities`, present when live mode is wired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LiveCapabilitiesV1 {
    pub presets: Vec<PresetV1>,
    /// The visitor-safe budget. Absent when the ledger could not be read:
    /// no run can be admitted until it can (`/ready` then reports
    /// `live.ledger_ok: false`), and no figure is invented in its place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<PublicBudgetV1>,
    /// Always true: a Turnstile token must accompany every live run.
    pub requires_turnstile: bool,
    /// Always true: `attest_adult` must be true on every live run.
    pub requires_adult_attestation: bool,
}

/// `GET /api/v1/capabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilitiesV1 {
    pub surface: SurfaceV1,
    pub live_enabled: bool,
    pub guided_enabled: bool,
    pub showcase_enabled: bool,
    pub limits: LimitsV1,
    pub schema_version: String,
    pub policy_version: String,
    pub protocol: String,
    /// Authority recorded on this surface's decisions.
    pub decision_authority: Authority,
    /// Presets, budget and requirements of the public live mode; absent
    /// when live mode is not wired on this server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<LiveCapabilitiesV1>,
}

/// `GET /api/v1/health`: static facts only, no environment probing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HealthV1 {
    pub status: String,
    pub version: String,
    pub schema_version: String,
    pub policy_version: String,
    pub contract_sha256: String,
    /// Always `disabled`: approval records a decision and executes
    /// nothing.
    pub executor: String,
}

/// Why the run index could not be verified, as `/ready` publishes it: the
/// kind of failure, never the store's own text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IndexErrorCodeV1 {
    /// The index failed verification: a record was edited, removed or
    /// added since the store opened.
    IndexTampered,
    /// A lock the store needs is held by another writer or unusable.
    IndexLocked,
    /// The file system refused a read or a write.
    IndexIo,
    /// Anything else the store could not read as it writes it.
    IndexUnreadable,
}

/// The run store as `/ready` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunStoreStatusV1 {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_head: Option<String>,
    pub index_tampered: bool,
    pub index_tail_repaired: bool,
    /// Why the run index could not be verified at the last check, on both
    /// surfaces; absent while it verifies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_error_code: Option<IndexErrorCodeV1>,
    /// Operator surface only: the store's own error text, which can name
    /// files by path. The server logs it once per change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_error: Option<String>,
}

/// The showcase as `/ready` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShowcaseStatusV1 {
    pub ok: bool,
    pub entries: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// The live mode as `/ready` reports it. `ready` is false while the
/// ledger is tripped or unreadable, or the evidence source is not ok.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LiveReadyV1 {
    /// The ledger could be read.
    pub ledger_ok: bool,
    pub tripped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip_code: Option<String>,
    /// The evidence source (the sealed snapshot in production) passed its
    /// check.
    pub evidence_ok: bool,
}

/// `GET /api/v1/ready`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReadyV1 {
    pub ready: bool,
    pub run_store: RunStoreStatusV1,
    pub showcase: ShowcaseStatusV1,
    pub active_runs: u64,
    pub queued_runs: u64,
    pub shutting_down: bool,
    /// Present when live mode is wired on this server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<LiveReadyV1>,
}

/// One archived run of the research showcase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShowcaseEntryV1 {
    pub run_id: RunId,
    /// Archive label, e.g. `F03`.
    pub label: String,
    pub battery: String,
    pub title: String,
    /// The recommendation kind, or `failed:<stage>`.
    pub outcome: String,
    pub authorization: String,
    pub schema_version: String,
    pub provenance: ProvenanceV1,
    /// The event log inside `research/`, served at `/research/<file>`.
    pub file: String,
    /// SHA-256 of that file, as `research/MANIFEST.json` pins it.
    pub sha256: String,
    /// Hash of the run's last event; None for unchained (1.0) runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_hash: Option<String>,
    /// The run record (task, detail, receipt as downloaded), when the
    /// archive has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_file: Option<String>,
    pub event_count: u64,
}

/// `GET /api/v1/showcase`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShowcaseV1 {
    pub entries: Vec<ShowcaseEntryV1>,
    /// `research/MANIFEST.json` as served at `/research/MANIFEST.json`.
    pub manifest_file: String,
    pub manifest_sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_text_refuses_hidden_format_characters() {
        for hidden in [
            "\u{202A}", "\u{202E}", "\u{2066}", "\u{2069}", "\u{200B}", "\u{2060}", "\u{FEFF}",
        ] {
            let text = format!("approve{hidden} this");
            let err = check_text("note", &text, 2000).unwrap_err();
            assert!(err.contains("bidirectional"), "{hidden:?}: {err}");
        }
        // Joiners, directional marks and ordinary text stay accepted.
        for fine in [
            "\u{0642}\u{0631}\u{0627}\u{0631}\u{200F} ok",
            "\u{1F469}\u{200D}\u{1F4BB} ok",
            "\u{0632}\u{0628}\u{0627}\u{0646}\u{200C}\u{0647}\u{0627}",
            "d\u{00E9}cision approuv\u{00E9}e",
            "line one\nline two\tend",
        ] {
            assert!(check_text("note", fine, 2000).is_ok(), "{fine:?}");
        }
    }

    #[test]
    fn requests_refuse_unknown_fields_and_free_strings() {
        assert!(serde_json::from_str::<CreateGuidedRunRequest>(r#"{"scenario":"retry"}"#).is_ok());
        assert!(serde_json::from_str::<CreateGuidedRunRequest>(
            r#"{"scenario":"retry","title":"x"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<CreateGuidedRunRequest>(r#"{"scenario":"nope"}"#).is_err());
        assert!(serde_json::from_str::<CreateLiveRunRequest>(r#"{"preset_id":"Bad Id"}"#).is_err());
        assert!(
            serde_json::from_str::<CreateLiveRunRequest>(r#"{"preset_id":"retry-cap-3"}"#).is_ok()
        );
        // Every curated preset id (a battery tag with an uppercase letter)
        // is a valid request id; a space, a slash or a dot is not.
        for id in crate::live::presets::PresetId::ALL {
            assert!(PresetId::parse(id.as_str()).is_ok(), "{id}");
        }
        for bad in ["R01 reference", "R01/reference", "r01.ref", ""] {
            assert!(PresetId::parse(bad).is_err(), "{bad:?}");
        }
        // The attestation defaults to false (refused later by the gate), a
        // judge code is bounded and trimmed, and no other field is read.
        let bare: CreateLiveRunRequest =
            serde_json::from_str(r#"{"preset_id":"retry-cap-3"}"#).unwrap();
        assert!(!bare.attest_adult);
        assert_eq!(bare.judge_code_trimmed(), None);
        assert!(bare.validate().is_ok());
        let full: CreateLiveRunRequest = serde_json::from_str(
            r#"{"preset_id":"retry-cap-3","attest_adult":true,"judge_code":"  alpha-7231 "}"#,
        )
        .unwrap();
        assert!(full.attest_adult);
        assert_eq!(full.judge_code_trimmed().as_deref(), Some("alpha-7231"));
        assert!(full.validate().is_ok());
        let blank = CreateLiveRunRequest {
            judge_code: Some("   ".into()),
            ..full.clone()
        };
        assert!(blank.validate().is_err());
        let long = CreateLiveRunRequest {
            judge_code: Some("x".repeat(JUDGE_CODE_MAX_CHARS + 1)),
            ..full.clone()
        };
        assert!(long.validate().is_err());
        assert!(serde_json::from_str::<CreateLiveRunRequest>(
            r#"{"preset_id":"retry-cap-3","attest_adult":true,"title":"free text"}"#
        )
        .is_err());
        let decision: DecisionRequest = serde_json::from_str(
            r#"{"approved":true,"expected":{"recommendation":"supported","last_sequence":14}}"#,
        )
        .unwrap();
        assert!(decision.validate().is_ok());
        assert!(serde_json::from_str::<DecisionRequest>(
            r#"{"approved":true,"authorized_by":"mario","expected":{"recommendation":"supported","last_sequence":1}}"#
        )
        .is_err());
    }

    #[test]
    fn operator_request_bounds_its_fields() {
        let req: CreateOperatorRunRequest =
            serde_json::from_str(r#"{"model":"fake","memory":"hyphae","scenario":"retry"}"#)
                .unwrap();
        assert!(req.validate().unwrap_err().contains("silently ignored"));
        let req: CreateOperatorRunRequest =
            serde_json::from_str(r#"{"model":"fake","memory":"fake"}"#).unwrap();
        assert!(req.validate().unwrap_err().contains("scenario"));
        let req: CreateOperatorRunRequest =
            serde_json::from_str(r#"{"model":"meta","memory":"hyphae","project_id":"../other"}"#)
                .unwrap();
        assert!(req.validate().unwrap_err().contains("project_id"));
        let long = "x".repeat(201);
        let req = CreateOperatorRunRequest {
            model: OperatorModel::Meta,
            memory: OperatorMemory::Fake,
            scenario: None,
            title: Some(long),
            description: None,
            project_id: None,
        };
        assert!(req.validate().unwrap_err().contains("title"));
    }

    #[test]
    fn scenario_names_round_trip() {
        for scenario in GuidedScenario::ALL {
            let json = serde_json::to_value(scenario).unwrap();
            assert_eq!(json, scenario.as_str());
            let back: GuidedScenario = serde_json::from_value(json).unwrap();
            assert_eq!(back, scenario);
        }
    }
}
