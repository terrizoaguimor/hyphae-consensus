// SPDX-License-Identifier: Apache-2.0
//! Ports to untrusted participants.
//!
//! The runtime is synchronous: the Meta and Hyphae adapters of
//! `consensus-api` implement these contracts with blocking I/O, on the
//! servers' blocking pool. Implementations must enforce their own
//! timeouts and return structured errors — the orchestrator
//! fails the run safely on any `Err` and never guesses.

use crate::contract::OpenItem;
use crate::orchestrator::codes;
use consensus_domain::*;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ModelError {
    #[error("model call timed out")]
    Timeout,
    #[error("model returned malformed structured output: {reason}")]
    MalformedOutput {
        reason: String,
        /// The call, when the provider answered it and its output was
        /// then refused: it was made and billed, so the orchestrator
        /// keeps it in `RunFailed.invocation` (audit finding TC-7).
        /// `None` when no answered call stands behind the error.
        invocation: Option<Box<ModelInvocation>>,
        /// A stable code more precise than `model_malformed_output`, when
        /// the client knows one (e.g. `model_output_truncated`: the output
        /// hit its token bound before the document was complete).
        code: Option<&'static str>,
    },
    #[error("model service unavailable: {0}")]
    Unavailable(String),
    /// The model declined to produce the document (code
    /// `model_refusal`). The provider answered, so the call is recorded.
    /// The refusal's own text is never part of the error: it is model
    /// output that may quote the prompt, and the reason is persisted.
    #[error("model_refusal: the model declined to produce the requested document")]
    Refused {
        invocation: Option<Box<ModelInvocation>>,
    },
    /// A provider failure the client classified under a stable code (an
    /// HTTP status class, a timeout, a transport failure, its own
    /// configuration), with a fixed short message for that class. The
    /// message never quotes the provider's response body: the orchestrator
    /// persists it as `RunFailed.reason` (audit findings MA-4, SEC-9).
    #[error("{code}: {message}")]
    Provider { code: &'static str, message: String },
}

impl ModelError {
    /// Malformed output with no answered call behind it (yet): see
    /// [`after_call`](Self::after_call).
    pub fn malformed(reason: impl Into<String>) -> Self {
        Self::MalformedOutput {
            reason: reason.into(),
            invocation: None,
            code: None,
        }
    }

    /// Malformed output whose cause has its own stable `code`.
    pub fn malformed_with_code(code: &'static str, reason: impl Into<String>) -> Self {
        Self::MalformedOutput {
            reason: reason.into(),
            invocation: None,
            code: Some(code),
        }
    }

    /// The same error, recording the answered call it refused. Only
    /// malformed output and a refusal follow an answer, so other errors
    /// are returned unchanged.
    pub fn after_call(self, call: ModelInvocation) -> Self {
        match self {
            Self::MalformedOutput { reason, code, .. } => Self::MalformedOutput {
                reason,
                invocation: Some(Box::new(call)),
                code,
            },
            Self::Refused { .. } => Self::Refused {
                invocation: Some(Box::new(call)),
            },
            other => other,
        }
    }

    /// The answered call this error refused, if any.
    pub fn invocation(&self) -> Option<&ModelInvocation> {
        match self {
            Self::MalformedOutput { invocation, .. } | Self::Refused { invocation } => {
                invocation.as_deref()
            }
            _ => None,
        }
    }

    /// The stable code the client gave this failure, when it gave one
    /// (`meta_unauthorized`, `model_output_truncated`, `model_refusal`,
    /// ...). Without one the orchestrator records the variant's generic
    /// code (`model_timeout`, `model_malformed_output`,
    /// `model_unavailable`).
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::MalformedOutput { code, .. } => *code,
            Self::Refused { .. } => Some(codes::MODEL_REFUSAL),
            Self::Provider { code, .. } => Some(code),
            Self::Timeout | Self::Unavailable(_) => None,
        }
    }
}

/// Bounded context handed to a role call. Evidence content inside is data
/// to reason about, never instructions to follow. The orchestrator builds
/// one per round, from the run's aggregate, for that round's critique,
/// evaluation and revision.
#[derive(Debug, Clone, Default)]
pub struct RoleContext {
    /// Protocol 1.3: every record retrieved so far in the run (at most
    /// `MAX_EVIDENCE_PER_RUN`), not only this round's recalls.
    pub evidence: Vec<EvidenceReference>,
    /// Integrity results known before the call, keyed by evidence id.
    pub integrity: Vec<(EvidenceId, IntegrityStatus)>,
    /// Protocol 1.3: Critic objections the previous evaluation left open,
    /// in its verdict order. Empty in the first round.
    pub open_items: Vec<OpenItem>,
    /// Protocol 1.3: records an earlier evaluation listed as contradicting
    /// a claim and no later one cleared. Empty in the first round.
    pub carried_contradictions: Vec<EvidenceId>,
}

/// One role call per method. Every `Evaluation` returned must keep the
/// protocol 1.3 evaluation contract ([`crate::contract`]) for the
/// proposal, the challenge and the context it was asked about: one
/// assessment per claim, citations only from `ctx.evidence`, one verdict
/// per objection in `ctx.open_items` and `challenge.items`, and a
/// judgment on every carried contradiction. The orchestrator checks it
/// before recording anything and fails the run on a violation; it never
/// completes or repairs a document, whichever client produced it.
///
/// A client that refuses a document after the provider answered returns
/// [`ModelError::MalformedOutput`] (or [`ModelError::Refused`]) with the
/// call's invocation ([`ModelError::after_call`]), so the call is recorded
/// even though no document is. A client whose error cannot carry the call
/// (a body read that timed out after the provider answered, say) keeps it
/// for [`take_failed_invocation`](Self::take_failed_invocation) instead.
pub trait ModelClient {
    fn propose(&mut self, task: &Task) -> Result<(Proposal, ModelInvocation), ModelError>;
    fn critique(
        &mut self,
        proposal: &Proposal,
        ctx: &RoleContext,
    ) -> Result<(Challenge, ModelInvocation), ModelError>;
    fn evaluate(
        &mut self,
        proposal: &Proposal,
        challenge: &Challenge,
        ctx: &RoleContext,
    ) -> Result<(Evaluation, ModelInvocation), ModelError>;
    fn revise(
        &mut self,
        proposal: &Proposal,
        challenge: &Challenge,
        evaluation: &Evaluation,
        ctx: &RoleContext,
    ) -> Result<(Revision, ModelInvocation), ModelError>;

    /// The call behind the last error, when the provider had answered it
    /// (audit finding TC-7), and only once: taking it clears it. The
    /// orchestrator takes it after every failed call, records it in
    /// `RunFailed.invocation` unless the error already carried the call
    /// ([`ModelError::invocation`]), and charges it to the run's token
    /// budget. The default keeps nothing.
    fn take_failed_invocation(&mut self) -> Option<ModelInvocation> {
        None
    }

    /// The control of the run this client is about to serve, handed over
    /// before its first call by the server's run manager
    /// (`consensus-api`'s `runs.rs`): a client may bound each call by the
    /// run's deadline ([`RunControl::deadline`]) and give up a call in
    /// progress once [`RunControl::in_flight_aborted`] is set. The default
    /// ignores it.
    ///
    /// [`RunControl::deadline`]: crate::control::RunControl::deadline
    /// [`RunControl::in_flight_aborted`]: crate::control::RunControl::in_flight_aborted
    fn bind_run_control(&mut self, _control: &crate::control::RunControl) {}
}

/// One recall as the memory backend answered it ([`MemoryClient::recall_with_proof`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recall {
    /// The records the run may use: those the backend attributed to the
    /// requested project or to `_global`, in result order.
    pub records: Vec<EvidenceReference>,
    /// The proof the backend sealed for this recall, when it produced one.
    pub proof: Option<RecallProof>,
    /// Why proof generation failed upstream, when it did: the records were
    /// then recalled without a proof, and each carries the failure's text
    /// in `proof_error`.
    pub proof_failure: Option<ProofFailure>,
    /// Records the backend returned for any other project (or for no
    /// project it named). They are never evidence; the orchestrator
    /// records each as `EvidenceUnavailable` (code `foreign_record`).
    pub foreign: Vec<ForeignRecord>,
}

/// A proof sealed for one recall. It covers the recall's whole result
/// (every record in it, foreign ones included), so a proof of an empty
/// result still proves absence, e.g. for project isolation. Recorded as
/// `EventPayload::RecallProven`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecallProof {
    /// The request the proof seals.
    pub request: ProofRequest,
    /// Every record of the sealed result, in result order; empty when the
    /// result was empty.
    pub evidence_ids: Vec<EvidenceId>,
    /// Public identifier of the proof artifact, never a local path.
    pub proof_ref: String,
    /// The anchor the proof is bound to.
    pub anchor: String,
}

/// Why the memory backend could not generate a proof: its own error
/// category, when it gave one, and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofFailure {
    pub category: Option<String>,
    pub message: String,
}

/// A record the backend returned for a project other than the requested
/// one or `_global`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignRecord {
    pub evidence_id: EvidenceId,
    /// The project the backend reported for it; `None` when it named none.
    pub backend_project: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MemoryError {
    /// Backend unreachable (e.g. Hyphae offline). Runs degrade, not crash.
    #[error("memory backend unavailable: {0}")]
    Unavailable(String),
    #[error("memory backend error: {0}")]
    Backend(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryStatus {
    pub healthy: bool,
    pub detail: String,
}

/// Read-only memory access for agents.
///
/// Hyphae mapping (M3 adapter, pinned revision in README):
/// `recall` -> MCP `hyphae_memory_recall` (with `prove` when proofs are
/// requested), `status` -> MCP `hyphae_memory_status`. These are the only
/// tools the `memory-reader` profile exposes; `store`/`journal`/`forget`
/// are never reachable through this trait.
pub trait MemoryClient {
    /// The records a recall may use as evidence, without proof metadata.
    fn recall(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EvidenceReference>, MemoryError>;

    /// The same recall with what the backend said about it: the proof it
    /// sealed (if any), why proof generation failed (if it did) and the
    /// records it returned for another project. The orchestrator calls
    /// this. The default wraps [`recall`](Self::recall) and claims no
    /// proof, which is what a client without real proofs (a scripted
    /// double) must report.
    fn recall_with_proof(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Recall, MemoryError> {
        Ok(Recall {
            records: self.recall(project, query, limit)?,
            ..Recall::default()
        })
    }

    fn status(&mut self) -> Result<MemoryStatus, MemoryError>;
}

/// Runtime-service-only memory writes. Deliberately separate from
/// `MemoryClient` so agent-facing code cannot write memory. Unused in M1.
pub trait MemoryAdmin: MemoryClient {
    fn store(&mut self, project: &ProjectId, text: &str, kind: &str)
        -> Result<String, MemoryError>;
}

/// Result of checking one record's integrity proof.
#[derive(Debug, Clone, PartialEq)]
pub struct ProofCheck {
    pub status: IntegrityStatus,
    pub verifier: Option<String>,
    pub verified_anchor: Option<String>,
    pub detail: Option<String>,
    /// The verifier's structured report, when it gives one; recorded in
    /// `EvidenceVerification::report` (schema 1.3).
    pub report: Option<VerifierReport>,
}

/// Verifies cryptographic integrity only. Factual correctness and
/// relevance are evaluator judgments and never flow through this port.
/// Hyphae mapping (M3): `hyphae proof verify` against the recall proof.
pub trait ProofVerifier {
    fn verify(&mut self, evidence: &EvidenceReference) -> ProofCheck;
}

/// External mutation boundary. DEFERRED: through M3 the orchestrator
/// never calls this port; human approval is recorded only. The trait
/// exists so tests can prove that property with a counting double.
pub trait ExecutorPort {
    fn execute_approved(&mut self, run_id: &RunId) -> Result<(), String>;
}
