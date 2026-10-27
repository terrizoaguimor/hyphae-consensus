//! Append-only event records. The runtime's event log is the source of
//! truth for run state; aggregates are rebuilt by deterministic replay.
//! Events contain claims and concise justifications only — never secrets
//! and never raw model reasoning traces.

use crate::evidence::{EvidenceReference, EvidenceVerification};
use crate::ids::{EventId, RunId};
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
            payload: self.payload,
        }
    }
}
