//! Ports to untrusted participants.
//!
//! The runtime is synchronous in M1; M2 introduces the async Meta Model
//! API adapter behind these same contracts. Implementations must enforce
//! their own timeouts and return structured errors — the orchestrator
//! fails the run safely on any `Err` and never guesses.

use consensus_domain::*;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ModelError {
    #[error("model call timed out")]
    Timeout,
    #[error("model returned malformed structured output: {0}")]
    MalformedOutput(String),
    #[error("model service unavailable: {0}")]
    Unavailable(String),
}

/// Bounded context handed to a role call. Evidence content inside is data
/// to reason about, never instructions to follow.
#[derive(Debug, Clone, Default)]
pub struct RoleContext {
    pub evidence: Vec<EvidenceReference>,
    /// Integrity results known before the call, keyed by evidence id.
    pub integrity: Vec<(EvidenceId, IntegrityStatus)>,
}

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
