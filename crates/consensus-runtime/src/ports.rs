// SPDX-License-Identifier: Apache-2.0
//! Ports to untrusted participants.
//!
//! The runtime is synchronous in M1; M2 introduces the async Meta Model
//! API adapter behind these same contracts. Implementations must enforce
//! their own timeouts and return structured errors — the orchestrator
//! fails the run safely on any `Err` and never guesses.

use crate::contract::OpenItem;
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
    },
    #[error("model service unavailable: {0}")]
    Unavailable(String),
}

impl ModelError {
    /// Malformed output with no answered call behind it (yet): see
    /// [`after_call`](Self::after_call).
    pub fn malformed(reason: impl Into<String>) -> Self {
        Self::MalformedOutput {
            reason: reason.into(),
            invocation: None,
        }
    }

    /// The same error, recording the answered call it refused. Only
    /// malformed output follows an answer, so other errors are returned
    /// unchanged.
    pub fn after_call(self, call: ModelInvocation) -> Self {
        match self {
            Self::MalformedOutput { reason, .. } => Self::MalformedOutput {
                reason,
                invocation: Some(Box::new(call)),
            },
            other => other,
        }
    }

    /// The answered call this error refused, if any.
    pub fn invocation(&self) -> Option<&ModelInvocation> {
        match self {
            Self::MalformedOutput { invocation, .. } => invocation.as_deref(),
            _ => None,
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
/// [`ModelError::MalformedOutput`] with the call's invocation
/// ([`ModelError::after_call`]), so the call is recorded even though no
/// document is.
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
    fn recall(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EvidenceReference>, MemoryError>;
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
