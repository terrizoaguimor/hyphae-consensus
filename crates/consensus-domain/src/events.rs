// SPDX-License-Identifier: Apache-2.0
//! Append-only event records. The runtime's event log is the source of
//! truth for run state; aggregates are rebuilt by deterministic replay.
//! Events contain claims and concise justifications only — never secrets
//! and never raw model reasoning traces.

use crate::evidence::{EvidenceReference, EvidenceVerification, ProofRequest};
use crate::ids::{EventId, EvidenceId, RunId};
use crate::proposal::{Challenge, Evaluation, Proposal, Revision};
use crate::run::{HumanAuthorization, Recommendation, Task};
use crate::Actor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Metadata for one model call, recorded so Meta model participation is
/// demonstrable from receipts. `model` preserves the exact Model API
/// identity string returned/configured for the call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelInvocation {
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
    /// Schema 1.3: why the model stopped generating, as the API said it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Schema 1.3: reasoning tokens, when the API reports them apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    /// Schema 1.3: the output bound sent with the request, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u64>,
    /// Schema 1.3: name of the structured-output schema requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,
    /// Schema 1.3: SHA-256 of that schema as sent, lowercase hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_sha256: Option<String>,
    /// Schema 1.3: SHA-256 of the prompt template, lowercase hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_sha256: Option<String>,
    /// Schema 1.3: reasoning effort requested, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Schema 1.3: whether a pseudonymous safety identifier was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_identifier_sent: Option<bool>,
    /// Schema 1.3: HTTP attempts made for this call (retries included).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
    /// Protocol 1.3 hardening (prompt-freeze-3): the answered calls of
    /// this role call whose output the adapter refused as a form defect or
    /// as unreadable, and asked again for once (never a judgment). When
    /// present, `input_tokens`, `output_tokens` and `reasoning_tokens`
    /// above are the SUM over those calls and this one, so the run's token
    /// budget charges every billed call; every other field is this call's
    /// own. Absent on every record written before it existed. Not part of
    /// the api-v1 contract schema in this window (`schemars(skip)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub rejected_calls: Option<Vec<RejectedCall>>,
    /// Protocol 1.3 hardening: SHA-256 (lowercase hex) of the fixed
    /// corrective sentence appended to the user message of this call's
    /// re-ask, when there was one. Never model text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub corrective_sha256: Option<String>,
}

/// An answered model call whose output the adapter refused before asking
/// again (see [`ModelInvocation::rejected_calls`]): what was billed and
/// why it was refused, by stable code. Never model text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RejectedCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The refusal's stable code (`model_no_claims`, `meta_bad_envelope`,
    /// `model_malformed_output`, ...).
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    TaskCreated {
        task: Task,
    },
    ProposalSubmitted {
        proposal: Proposal,
        invocation: ModelInvocation,
    },
    EvidenceRetrieved {
        evidence: EvidenceReference,
    },
    /// Retrieval was attempted but the memory backend could not serve it
    /// (e.g. Hyphae offline). The run continues with reduced evidence;
    /// see the README's offline-behavior statement.
    EvidenceUnavailable {
        query: String,
        reason: String,
        /// Schema 1.3: stable machine code next to the English `reason`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// Schema 1.3: a proof the memory backend produced, described by what
    /// it seals: its own request, the records its result holds (none for
    /// an empty recall, which still proves absence, e.g. for project
    /// isolation), the proof artifact and the anchor it is bound to.
    RecallProven {
        request: ProofRequest,
        evidence_ids: Vec<EvidenceId>,
        proof_ref: String,
        anchor: String,
        empty: bool,
    },
    EvidenceVerified {
        verification: EvidenceVerification,
    },
    ChallengeSubmitted {
        challenge: Challenge,
        invocation: ModelInvocation,
    },
    EvaluationSubmitted {
        evaluation: Evaluation,
        invocation: ModelInvocation,
    },
    RevisionSubmitted {
        revision: Revision,
        invocation: ModelInvocation,
    },
    RecommendationIssued {
        recommendation: Recommendation,
    },
    HumanDecisionRecorded {
        authorization: HumanAuthorization,
    },
    RunFailed {
        reason: String,
        stage: String,
        /// Schema 1.3: stable machine code next to the English `reason`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// Schema 1.3: the model call whose output was refused after the
        /// provider answered it (by the adapter, or by the runtime's
        /// contract), so the call and its tokens stay in the run's record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation: Option<ModelInvocation>,
    },
}

/// One line in the JSONL event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsensusEvent {
    pub schema_version: String,
    /// Monotonic, gap-free sequence within the log file, starting at 1.
    /// Assigned by the event log on append; validated on open/replay.
    pub sequence: u64,
    /// Idempotency key: re-appending an event with an existing id is a
    /// no-op that returns the original sequence.
    pub event_id: EventId,
    pub run_id: RunId,
    pub actor: Actor,
    pub occurred_at: String,
    /// Hash-chain link: `hash` of the previous record in the file
    /// ("genesis" for the first). Empty only on pre-1.1 records.
    #[serde(default)]
    pub prev_hash: String,
    /// SHA-256 over (prev_hash || canonical JSON of this record with
    /// `hash` blanked). Assigned by the event log on append; validated
    /// on open/replay. Empty only on pre-1.1 records.
    #[serde(default)]
    pub hash: String,
    pub payload: EventPayload,
}

/// An event before the log assigns its sequence number.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub event_id: EventId,
    pub run_id: RunId,
    pub actor: Actor,
    pub occurred_at: String,
    pub payload: EventPayload,
}

impl NewEvent {
    pub fn into_sequenced(self, sequence: u64) -> ConsensusEvent {
        ConsensusEvent {
            schema_version: crate::SCHEMA_VERSION.to_string(),
            sequence,
            event_id: self.event_id,
            run_id: self.run_id,
            actor: self.actor,
            occurred_at: self.occurred_at,
            prev_hash: String::new(),
            hash: String::new(),
            payload: self.payload,
        }
    }
}
