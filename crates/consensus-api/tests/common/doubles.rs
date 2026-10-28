// SPDX-License-Identifier: Apache-2.0
//! Doubles the lifecycle tests need: a model that blocks at a chosen
//! stage until the test lets it go, one that panics, one that fails with
//! an upstream-looking reason under a declared live mode, a deliberation
//! whose task creation fails, and a scripted live gate.

use consensus_api::api::error::{ApiError, ErrorCode};
use consensus_api::api::types::GuidedScenario;
use consensus_api::api::{
    AdmitFuture, LiveAdmissionRequest, LiveGate, LiveGrant, VerifiedLiveRequest, VerifyFuture,
};
use consensus_api::guided;
use consensus_api::runs::{Deliberation, RunKind, StartRequest};
use consensus_domain::*;
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::fakes::FakeModelClient;
use consensus_runtime::orchestrator::OrchestratorError;
use consensus_runtime::ports::{ModelClient, ModelError, RoleContext};
use consensus_runtime::{
    EventLogError, Orchestrator, Owner, Retention, RunAggregate, RunControl, RunLimits,
    SharedRunLog, TaskInput,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

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

/// A model whose first call fails as a live adapter does when the API
/// answers an error: the reason quotes the upstream response, and the
/// orchestrator records it verbatim in `RunFailed.reason`.
pub struct UnavailableModel {
    pub reason: String,
}

impl ModelClient for UnavailableModel {
    fn propose(&mut self, _task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
        Err(ModelError::Unavailable(self.reason.clone()))
    }

    fn critique(
        &mut self,
        _proposal: &Proposal,
        _ctx: &RoleContext,
    ) -> Result<(Challenge, ModelInvocation), ModelError> {
        unreachable!("propose fails first")
    }

    fn evaluate(
        &mut self,
        _proposal: &Proposal,
        _challenge: &Challenge,
        _ctx: &RoleContext,
    ) -> Result<(Evaluation, ModelInvocation), ModelError> {
        unreachable!("propose fails first")
    }

    fn revise(
        &mut self,
        _proposal: &Proposal,
        _challenge: &Challenge,
        _evaluation: &Evaluation,
        _ctx: &RoleContext,
    ) -> Result<(Revision, ModelInvocation), ModelError> {
        unreachable!("propose fails first")
    }
}

/// A start request declared as a live (Meta) run whose first model call
/// fails with `reason`.
pub fn unavailable_start(
    owner: Owner,
    retention: Retention,
    origin: RunOrigin,
    reason: &str,
) -> StartRequest {
    let mut s = guided::build(GuidedScenario::Retry);
    s.input.mode = Some(TaskMode {
        model: ModelMode::Meta,
        evidence: EvidenceMode::Simulated,
        origin,
        preset: None,
    });
    let orchestrator = Orchestrator {
        model: UnavailableModel {
            reason: reason.to_string(),
        },
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
        kind: RunKind::Live,
        deliberation: Box::new(orchestrator),
    }
}

/// A deliberation whose task creation fails before anything is stored.
pub struct FailingDeliberation;

impl Deliberation for FailingDeliberation {
    fn create_task(
        &mut self,
        _sink: &mut SharedRunLog,
        _run_id: RunId,
        _input: TaskInput,
    ) -> Result<(RunAggregate, Task), OrchestratorError> {
        Err(OrchestratorError::Log(EventLogError::Io(
            std::io::Error::other("synthetic task creation failure"),
        )))
    }

    fn deliberate(
        &mut self,
        _sink: &mut SharedRunLog,
        _agg: RunAggregate,
        _task: &Task,
        _control: &RunControl,
    ) -> Result<RunAggregate, OrchestratorError> {
        unreachable!("create_task fails first")
    }
}

/// What the scripted live gate does with the next request in its second
/// phase (the first always passes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    /// Refuse with a budget error.
    Refuse,
    /// Grant a scripted deliberation.
    Grant,
    /// Grant a deliberation whose start fails.
    GrantFailingStart,
}

/// A live gate for the sequencing tests: it counts first-phase
/// verifications, second-phase admissions and refunds, and keeps the last
/// request's preset, pseudonym and Turnstile token.
pub struct TestGate {
    mode: Mutex<GateMode>,
    pub verified: AtomicUsize,
    pub admitted: Arc<AtomicUsize>,
    pub refunded: Arc<AtomicUsize>,
    pub last: Mutex<Option<(String, String, Option<String>)>>,
}

impl TestGate {
    pub fn new(mode: GateMode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            verified: AtomicUsize::new(0),
            admitted: Arc::new(AtomicUsize::new(0)),
            refunded: Arc::new(AtomicUsize::new(0)),
            last: Mutex::new(None),
        })
    }

    pub fn set_mode(&self, mode: GateMode) {
        *self.mode.lock().unwrap() = mode;
    }

    /// First-phase calls ([`LiveGate::verify`], before the slot).
    pub fn verified(&self) -> usize {
        self.verified.load(Ordering::SeqCst)
    }

    /// Second-phase calls ([`VerifiedLiveRequest::admit`], with the slot
    /// held): what would have charged the ledger.
    pub fn admitted(&self) -> usize {
        self.admitted.load(Ordering::SeqCst)
    }

    pub fn refunded(&self) -> usize {
        self.refunded.load(Ordering::SeqCst)
    }

    pub fn last_request(&self) -> Option<(String, String, Option<String>)> {
        self.last.lock().unwrap().clone()
    }
}

impl LiveGate for TestGate {
    fn verify(&self, request: LiveAdmissionRequest) -> VerifyFuture<'_> {
        Box::pin(async move {
            self.verified.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some((
                request.preset.as_str().to_string(),
                request.session_pseudonym.clone(),
                request.turnstile_token.clone(),
            ));
            // An outbound verification would await here.
            tokio::task::yield_now().await;
            let ticket: Box<dyn VerifiedLiveRequest> = Box::new(TestTicket {
                mode: *self.mode.lock().unwrap(),
                preset: request.preset.as_str().to_string(),
                admitted: self.admitted.clone(),
                refunded: self.refunded.clone(),
            });
            Ok(ticket)
        })
    }
}

/// What [`TestGate::verify`] hands the route for the second phase.
struct TestTicket {
    mode: GateMode,
    preset: String,
    admitted: Arc<AtomicUsize>,
    refunded: Arc<AtomicUsize>,
}

impl VerifiedLiveRequest for TestTicket {
    fn admit(self: Box<Self>) -> AdmitFuture<'static> {
        Box::pin(async move {
            self.admitted.fetch_add(1, Ordering::SeqCst);
            match self.mode {
                GateMode::Refuse => Err(ApiError::new(
                    ErrorCode::TooManyRuns,
                    "the live budget of this visitor is spent",
                )
                .with_retry_after(60)),
                GateMode::Grant | GateMode::GrantFailingStart => {
                    let refunded = self.refunded.clone();
                    let mut s = guided::build(GuidedScenario::Retry);
                    s.input.mode = Some(TaskMode {
                        model: ModelMode::Scripted,
                        evidence: EvidenceMode::Simulated,
                        origin: RunOrigin::Public,
                        preset: Some(format!("live:{}", self.preset)),
                    });
                    let deliberation: Box<dyn Deliberation> = if self.mode == GateMode::Grant {
                        Box::new(Orchestrator {
                            model: s.model,
                            memory: s.memory,
                            verifier: s.verifier,
                            clock: SystemClock,
                            ids: UuidIdGen,
                            limits: RunLimits::default(),
                        })
                    } else {
                        Box::new(FailingDeliberation)
                    };
                    Ok(LiveGrant {
                        input: s.input,
                        deliberation,
                        on_start_failed: Some(Box::new(move || {
                            refunded.fetch_add(1, Ordering::SeqCst);
                        })),
                    })
                }
            }
        })
    }
}
