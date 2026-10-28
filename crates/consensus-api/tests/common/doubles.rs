// SPDX-License-Identifier: Apache-2.0
//! Model doubles the lifecycle tests need: one that blocks at a chosen
//! stage until the test lets it go, and one that panics.

use consensus_api::api::types::GuidedScenario;
use consensus_api::guided;
use consensus_api::runs::{RunKind, StartRequest};
use consensus_domain::*;
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::fakes::FakeModelClient;
use consensus_runtime::ports::{ModelClient, ModelError, RoleContext};
use consensus_runtime::{Orchestrator, Owner, Retention, RunLimits};
use std::sync::mpsc::{channel, Receiver, Sender};

/// The reference scenario's scripted model, blocking once at `stage`
/// (`propose`, `critique`, `evaluate` or `revise`) until the test sends
/// on the release channel or drops its sender. `entered` tells the test
/// the model reached the gate.
pub struct GatedModel {
    inner: FakeModelClient,
    stage: &'static str,
    entered: Sender<&'static str>,
    release: Receiver<()>,
    released: bool,
}

impl GatedModel {
    fn wait(&mut self, stage: &'static str) {
        if stage == self.stage && !self.released {
            let _ = self.entered.send(stage);
            let _ = self.release.recv();
            self.released = true;
        }
    }
}

impl ModelClient for GatedModel {
    fn propose(&mut self, task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
        self.wait("propose");
        self.inner.propose(task)
    }

    fn critique(
        &mut self,
        proposal: &Proposal,
        ctx: &RoleContext,
    ) -> Result<(Challenge, ModelInvocation), ModelError> {
        self.wait("critique");
        self.inner.critique(proposal, ctx)
    }

    fn evaluate(
        &mut self,
        proposal: &Proposal,
        challenge: &Challenge,
        ctx: &RoleContext,
    ) -> Result<(Evaluation, ModelInvocation), ModelError> {
        self.wait("evaluate");
        self.inner.evaluate(proposal, challenge, ctx)
    }

    fn revise(
        &mut self,
        proposal: &Proposal,
        challenge: &Challenge,
        evaluation: &Evaluation,
        ctx: &RoleContext,
    ) -> Result<(Revision, ModelInvocation), ModelError> {
        self.wait("revise");
        self.inner.revise(proposal, challenge, evaluation, ctx)
    }
}

/// Handles the test keeps for a gated run.
pub struct Gate {
    pub release: Sender<()>,
    pub entered: Receiver<&'static str>,
}

impl Gate {
    /// Blocks until the model reaches its gate.
    pub fn wait_entered(&self) -> &'static str {
        self.entered
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the model reached its gate")
    }

    pub fn open(&self) {
        let _ = self.release.send(());
    }
}

/// A start request for the reference scenario whose model blocks at
/// `stage`, owned by `owner`.
pub fn gated_start(
    owner: Owner,
    retention: Retention,
    stage: &'static str,
) -> (StartRequest, Gate) {
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let mut s = guided::build(GuidedScenario::Retry);
    s.input.mode = Some(guided::task_mode(GuidedScenario::Retry, RunOrigin::Public));
    let model = GatedModel {
        inner: s.model,
        stage,
        entered: entered_tx,
        release: release_rx,
        released: false,
    };
    let orchestrator = Orchestrator {
        model,
        memory: s.memory,
        verifier: s.verifier,
        clock: SystemClock,
        ids: UuidIdGen,
        limits: RunLimits::default(),
    };
    (
        StartRequest {
            owner,
            retention,
            input: s.input,
            kind: RunKind::Guided,
            deliberation: Box::new(orchestrator),
        },
        Gate {
            release: release_tx,
            entered: entered_rx,
        },
    )
}

/// A model whose first call panics.
pub struct PanickingModel;

impl ModelClient for PanickingModel {
    fn propose(&mut self, _task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
        panic!("boom: synthetic model panic for the isolation test");
    }

    fn critique(
        &mut self,
        _proposal: &Proposal,
        _ctx: &RoleContext,
    ) -> Result<(Challenge, ModelInvocation), ModelError> {
        unreachable!("propose panics first")
    }

    fn evaluate(
        &mut self,
        _proposal: &Proposal,
        _challenge: &Challenge,
        _ctx: &RoleContext,
    ) -> Result<(Evaluation, ModelInvocation), ModelError> {
        unreachable!("propose panics first")
    }

    fn revise(
        &mut self,
        _proposal: &Proposal,
        _challenge: &Challenge,
        _evaluation: &Evaluation,
        _ctx: &RoleContext,
    ) -> Result<(Revision, ModelInvocation), ModelError> {
        unreachable!("propose panics first")
    }
}

/// A start request whose deliberation panics at its first model call.
pub fn panicking_start(owner: Owner, retention: Retention, origin: RunOrigin) -> StartRequest {
    let mut s = guided::build(GuidedScenario::Retry);
    s.input.mode = Some(guided::task_mode(GuidedScenario::Retry, origin));
    let orchestrator = Orchestrator {
        model: PanickingModel,
        memory: s.memory,
        verifier: s.verifier,
        clock: SystemClock,
        ids: UuidIdGen,
        limits: RunLimits::default(),
    };
    StartRequest {
        owner,
        retention,
        input: s.input,
        kind: RunKind::Guided,
        deliberation: Box::new(orchestrator),
    }
}
