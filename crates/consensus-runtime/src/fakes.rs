// SPDX-License-Identifier: Apache-2.0
//! Deterministic test doubles for every port. These are shipping test
//! infrastructure (used by `consensus-tests` and the M1 demo binary),
//! never presented as real model or Hyphae behavior: fake invocations
//! are labeled `provider: "fake"`.

use crate::ports::*;
use consensus_domain::*;
use std::collections::{HashMap, VecDeque};

/// Scripted model: each role call pops the next queued response. A
/// scripted document is returned as written (only its correlation ids are
/// rewritten): the fake never completes an assessment, synthesizes a
/// verdict or clears a contradiction, so scripts face the same protocol
/// 1.3 evaluation contract as a live model.
#[derive(Default)]
pub struct FakeModelClient {
    pub proposals: VecDeque<Result<Proposal, ModelError>>,
    pub critiques: VecDeque<Result<Challenge, ModelError>>,
    pub evaluations: VecDeque<Result<Evaluation, ModelError>>,
    pub revisions: VecDeque<Result<Revision, ModelError>>,
    pub calls: Vec<String>,
    /// The context of every critique, evaluate and revise call, with the
    /// call's name, in call order.
    pub contexts: Vec<(String, RoleContext)>,
    /// Reported (input, output) tokens per call; (0, 0) by default.
    pub tokens_per_call: (u64, u64),
    /// Report no usage at all, like a provider that omits it.
    pub omit_usage: bool,
    /// Invocations to report instead of the generated one, one per call in
    /// call order while any is left (e.g. an adapter's re-asked call with
    /// its `rejected_calls`).
    pub scripted_invocations: VecDeque<ModelInvocation>,
}

impl FakeModelClient {
    pub fn new() -> Self {
        Self::default()
    }

    fn invocation(&mut self, stage: &str) -> ModelInvocation {
        if let Some(scripted) = self.scripted_invocations.pop_front() {
            return scripted;
        }
        let (input, output) = self.tokens_per_call;
        ModelInvocation {
            provider: "fake".into(),
            // Mirrors the production model identity so receipt plumbing is
            // exercised end-to-end; `provider` marks it as a double.
            model: "muse-wonder-1.0".into(),
            request_id: Some(format!("fake-{stage}-{}", self.calls.len())),
            latency_ms: Some(1),
            input_tokens: (!self.omit_usage).then_some(input),
            output_tokens: (!self.omit_usage).then_some(output),
            // No HTTP call, schema or prompt exists behind a scripted
            // response, so nothing is claimed about one.
            finish_reason: None,
            reasoning_tokens: None,
            max_completion_tokens: None,
            schema_name: None,
            schema_sha256: None,
            prompt_sha256: None,
            reasoning_effort: None,
            safety_identifier_sent: None,
            attempts: None,
            rejected_calls: None,
            corrective_sha256: None,
        }
    }

    fn record(&mut self, stage: &str, ctx: &RoleContext) {
        self.calls.push(stage.into());
        self.contexts.push((stage.into(), ctx.clone()));
    }

    fn pop<T>(q: &mut VecDeque<Result<T, ModelError>>, stage: &str) -> Result<T, ModelError> {
        q.pop_front().unwrap_or_else(|| {
            Err(ModelError::Unavailable(format!(
                "fake model: no scripted {stage} response"
            )))
        })
    }
}

impl ModelClient for FakeModelClient {
    // Scripted documents carry placeholder correlation ids; the fake
    // rewrites run/target ids from its inputs so scenarios stay readable.
    fn propose(&mut self, task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
        self.calls.push("propose".into());
        let inv = self.invocation("propose");
        Self::pop(&mut self.proposals, "propose").map(|mut p| {
            p.run_id = task.run_id.clone();
            (p, inv)
        })
    }
    fn critique(
        &mut self,
        proposal: &Proposal,
        ctx: &RoleContext,
    ) -> Result<(Challenge, ModelInvocation), ModelError> {
        self.record("critique", ctx);
        let inv = self.invocation("critique");
        Self::pop(&mut self.critiques, "critique").map(|mut ch| {
            ch.run_id = proposal.run_id.clone();
            ch.target_proposal = proposal.id.clone();
            (ch, inv)
        })
    }
    fn evaluate(
        &mut self,
        proposal: &Proposal,
        _ch: &Challenge,
        ctx: &RoleContext,
    ) -> Result<(Evaluation, ModelInvocation), ModelError> {
        self.record("evaluate", ctx);
        let inv = self.invocation("evaluate");
        Self::pop(&mut self.evaluations, "evaluate").map(|mut e| {
            e.run_id = proposal.run_id.clone();
            e.target_proposal = proposal.id.clone();
            (e, inv)
        })
    }
    fn revise(
        &mut self,
        proposal: &Proposal,
        _ch: &Challenge,
        _e: &Evaluation,
        ctx: &RoleContext,
    ) -> Result<(Revision, ModelInvocation), ModelError> {
        self.record("revise", ctx);
        let inv = self.invocation("revise");
        Self::pop(&mut self.revisions, "revise").map(|mut r| {
            r.proposal.run_id = proposal.run_id.clone();
            r.supersedes = proposal.id.clone();
            (r, inv)
        })
    }
}

/// In-memory memory honoring Hyphae's documented isolation rule: a recall
/// sees the named project plus `_global`, nothing else.
pub struct FakeMemoryClient {
    pub records: Vec<EvidenceReference>,
    pub offline: bool,
}

impl FakeMemoryClient {
    pub fn new(records: Vec<EvidenceReference>) -> Self {
        Self {
            records,
            offline: false,
        }
    }
    pub fn offline() -> Self {
        Self {
            records: Vec::new(),
            offline: true,
        }
    }
}

impl MemoryClient for FakeMemoryClient {
    fn recall(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EvidenceReference>, MemoryError> {
        if self.offline {
            return Err(MemoryError::Unavailable("fake memory is offline".into()));
        }
        let tokens: Vec<String> = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 3)
            .map(str::to_string)
            .collect();
        let mut out: Vec<EvidenceReference> = self
            .records
            .iter()
            .filter(|r| &r.project_id == project || r.project_id.as_str() == "_global")
            .filter(|r| {
                let content = r.content.to_lowercase();
                tokens.is_empty() || tokens.iter().any(|t| content.contains(t))
            })
            .cloned()
            .collect();
        out.truncate(limit);
        Ok(out)
    }

    fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
        if self.offline {
            Err(MemoryError::Unavailable("fake memory is offline".into()))
        } else {
            Ok(MemoryStatus {
                healthy: true,
                detail: format!("{} records", self.records.len()),
            })
        }
    }
}

/// Scripted proof outcomes per evidence id, with a default.
pub struct FakeProofVerifier {
    pub by_id: HashMap<EvidenceId, IntegrityStatus>,
    pub default: IntegrityStatus,
}

impl FakeProofVerifier {
    pub fn all(status: IntegrityStatus) -> Self {
        Self {
            by_id: HashMap::new(),
            default: status,
        }
    }
}

impl ProofVerifier for FakeProofVerifier {
    fn verify(&mut self, evidence: &EvidenceReference) -> ProofCheck {
        let status = self
            .by_id
            .get(&evidence.id)
            .copied()
            .unwrap_or(self.default);
        // Receipts carry this text: a simulated proof must say so there,
        // not only in the UI.
        ProofCheck {
            status,
            verifier: Some("fake-verifier".into()),
            verified_anchor: evidence.anchor.clone(),
            detail: Some(format!(
                "SIMULATED proof (scripted outcome, no cryptography checked): {status:?}"
            )),
            report: None,
        }
    }
}

/// Counts execution attempts so tests can prove the deferred-executor
/// boundary: approval in M1-M3 must never reach this port.
#[derive(Default)]
pub struct CountingExecutor {
    pub calls: usize,
}

impl ExecutorPort for CountingExecutor {
    fn execute_approved(&mut self, _run_id: &RunId) -> Result<(), String> {
        self.calls += 1;
        Ok(())
    }
}
