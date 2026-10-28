// SPDX-License-Identifier: Apache-2.0
//! The run manager (B2): admission, durable creation, background
//! deliberation on a bounded blocking pool, cancellation, decisions and
//! shutdown, all over the per-run [`RunStore`].
//!
//! Order of a start (LC-07): the caller has already admitted the request
//! (session, quotas, input); a deliberation permit is taken *before*
//! anything durable exists, so a refusal never leaves an orphan run; the
//! run's files are created; `TaskCreated` is appended through the run's
//! [`SharedRunLog`] (validated, fsync'd, published); only then is the
//! deliberation spawned and `202` returned with the run's summary.
//!
//! Each deliberation runs under `catch_unwind`: a panic becomes
//! `RunFailed{stage: "panic", code: "panic"}` through the run's own sink
//! and poisons nothing but that run. A browser disconnect never reaches
//! the manager; only `cancel` and shutdown trip a run's [`RunControl`],
//! which the orchestrator honours at its next checkpoint.
//!
//! While a run deliberates it is *active*: its writer is the only one,
//! reads come from the writer's durable snapshot, and a decision is
//! refused (`409`). Once it rests (awaiting a decision, or terminal) its
//! writer is dropped, which releases the run's exclusive lock, and every
//! later read goes through [`RunStore::read_run`], every decision through
//! [`RunStore::open_run_as`], both owner-checked.

use crate::stream::Broadcasts;
use consensus_domain::*;
use consensus_runtime::orchestrator::OrchestratorError;
use consensus_runtime::ports::{MemoryClient, ModelClient, ProofVerifier};
use consensus_runtime::{
    parse_run_id, CancelReason, Clock, EventLogError, EventPublisher, IdGen, IndexKind,
    Orchestrator, Owner, PurgeReport, RecoveryReport, Retention, RunAggregate, RunControl, RunMeta,
    RunStore, RunStoreError, SharedRunLog, SystemClock, TaskInput, TransitionError,
};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The two steps of a run, over any ports. Implemented by every
/// [`Orchestrator`]; tests implement it with doubles that block or panic.
pub trait Deliberation: Send {
    fn create_task(
        &mut self,
        sink: &mut SharedRunLog,
        run_id: RunId,
        input: TaskInput,
    ) -> Result<(RunAggregate, Task), OrchestratorError>;

    fn deliberate(
        &mut self,
        sink: &mut SharedRunLog,
        agg: RunAggregate,
        task: &Task,
        control: &RunControl,
    ) -> Result<RunAggregate, OrchestratorError>;
}

impl<M, Mem, V, C, I> Deliberation for Orchestrator<M, Mem, V, C, I>
where
    M: ModelClient + Send,
    Mem: MemoryClient + Send,
    V: ProofVerifier + Send,
    C: Clock + Send,
    I: IdGen + Send,
{
    fn create_task(
        &mut self,
        sink: &mut SharedRunLog,
        run_id: RunId,
        input: TaskInput,
    ) -> Result<(RunAggregate, Task), OrchestratorError> {
        Orchestrator::create_task(self, sink, run_id, input)
    }

    fn deliberate(
        &mut self,
        sink: &mut SharedRunLog,
        agg: RunAggregate,
        task: &Task,
        control: &RunControl,
    ) -> Result<RunAggregate, OrchestratorError> {
        Orchestrator::deliberate(self, sink, agg, task, control)
    }
}

/// Which pool a run occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// Scripted doubles: milliseconds.
    Guided,
    /// A model is called: minutes.
    Live,
}

/// Everything a start needs once admitted.
pub struct StartRequest {
    pub owner: Owner,
    pub retention: Retention,
    pub input: TaskInput,
    pub kind: RunKind,
    pub deliberation: Box<dyn Deliberation>,
}

/// Bounds of the manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerSettings {
    pub guided_concurrency: usize,
    pub live_concurrency: usize,
    /// Wall-clock deadline of one deliberation.
    pub deadline: Duration,
    /// How long a start may wait for a permit before `Busy`.
    pub admission_wait: Duration,
    /// Starts that may wait for a permit at once.
    pub max_queue: usize,
    /// How long shutdown waits for cancelled runs to reach a checkpoint.
    pub shutdown_grace: Duration,
}

impl Default for ManagerSettings {
    fn default() -> Self {
        Self {
            guided_concurrency: 4,
            live_concurrency: 1,
            deadline: Duration::from_secs(900),
            admission_wait: Duration::from_secs(2),
            max_queue: 32,
            shutdown_grace: Duration::from_secs(60),
        }
    }
}

/// A run as read at one moment: its sidecar, every stored event and the
/// aggregate replayed from them.
#[derive(Debug, Clone)]
pub struct RunSnapshot {
    pub meta: RunMeta,
    pub events: Vec<ConsensusEvent>,
    pub aggregate: RunAggregate,
    /// True while this server is deliberating the run.
    pub active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("every deliberation slot is busy")]
    Busy { retry_after: Duration },
    #[error("the server is shutting down")]
    ShuttingDown,
    #[error("run store: {0}")]
    Store(#[from] RunStoreError),
    #[error("task creation: {0}")]
    Orchestrator(#[from] OrchestratorError),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// Unknown, malformed, purged, expired, or someone else's.
    #[error("run not found")]
    NotFound,
    /// Held by another operation after retries.
    #[error("run busy")]
    Busy,
    #[error("run store: {0}")]
    Store(RunStoreError),
    #[error("event log: {0}")]
    Log(#[from] EventLogError),
}

impl From<RunStoreError> for ReadError {
    fn from(e: RunStoreError) -> Self {
        match e {
            RunStoreError::InvalidId(_) | RunStoreError::UnknownRun(_) => ReadError::NotFound,
            RunStoreError::InUse(_) => ReadError::Busy,
            other => ReadError::Store(other),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    #[error("run not found")]
    NotFound,
    #[error("run is not deliberating")]
    NotCancellable,
    #[error(transparent)]
    Read(ReadError),
}

/// A decision as the route validated it.
#[derive(Debug, Clone)]
pub struct DecisionInput {
    pub approved: bool,
    pub note: Option<String>,
    pub expected_recommendation: RecommendationKind,
    pub expected_last_sequence: u64,
    pub idempotency_key: String,
    pub authority: Authority,
    /// Set by the server from the authenticated identity, never from the
    /// body (LC-11).
    pub authorized_by: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    #[error("run not found")]
    NotFound,
    #[error("the run is not awaiting a decision")]
    NotAwaiting,
    #[error("the decision does not match the run: {0}")]
    Conflict(String),
    #[error("the idempotency key was used with a different decision")]
    KeyReused,
    #[error("approving a blocked or abstained run requires a note")]
    OverrideNoteRequired,
    #[error(transparent)]
    Read(ReadError),
}

/// Counts for `/ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ManagerStats {
    pub active: usize,
    pub queued: usize,
}

struct ActiveRun {
    log: SharedRunLog,
    control: RunControl,
    owner: Owner,
    meta: RunMeta,
}

/// The run manager; share it behind an `Arc`.
pub struct RunManager {
    store: Arc<RunStore>,
    broadcasts: Arc<Broadcasts>,
    active: Mutex<HashMap<RunId, ActiveRun>>,
    guided: Arc<Semaphore>,
    live: Arc<Semaphore>,
    queued: AtomicUsize,
    settings: ManagerSettings,
    shutting_down: AtomicBool,
    idle: tokio::sync::Notify,
}

/// How long and how often a store operation is retried while another
/// handle briefly holds the run (a reader building a response, a writer
/// finishing).
const IN_USE_RETRIES: u32 = 40;
const IN_USE_PAUSE: Duration = Duration::from_millis(10);

/// Runs `op`, retrying `InUse` a bounded number of times.
pub fn retry_in_use<T>(
    mut op: impl FnMut() -> Result<T, RunStoreError>,
) -> Result<T, RunStoreError> {
    let mut attempt = 0;
    loop {
        match op() {
            Err(RunStoreError::InUse(id)) if attempt < IN_USE_RETRIES => {
                attempt += 1;
                let _ = id;
                std::thread::sleep(IN_USE_PAUSE);
            }
            other => return other,
        }
    }
}

impl RunManager {
    pub fn new(
        store: Arc<RunStore>,
        broadcasts: Arc<Broadcasts>,
        settings: ManagerSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            broadcasts,
            active: Mutex::new(HashMap::new()),
            guided: Arc::new(Semaphore::new(settings.guided_concurrency.max(1))),
            live: Arc::new(Semaphore::new(settings.live_concurrency.max(1))),
            queued: AtomicUsize::new(0),
            settings,
            shutting_down: AtomicBool::new(false),
            idle: tokio::sync::Notify::new(),
        })
    }

    pub fn store(&self) -> &Arc<RunStore> {
        &self.store
    }

    pub fn broadcasts(&self) -> &Arc<Broadcasts> {
        &self.broadcasts
    }

    pub fn settings(&self) -> &ManagerSettings {
        &self.settings
    }

    fn active_map(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, ActiveRun>> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    pub fn stats(&self) -> ManagerStats {
        ManagerStats {
            active: self.active_map().len(),
            queued: self.queued.load(Ordering::SeqCst),
        }
    }

    pub fn is_active(&self, run_id: &RunId) -> bool {
        self.active_map().contains_key(run_id)
    }

    /// Startup recovery: closes runs a dead process left mid-deliberation
    /// and reports what it found. Call once before serving.
    pub fn recover(&self) -> Result<RecoveryReport, RunStoreError> {
        self.store.recover()
    }

    /// Deletes expired visitor runs.
    pub fn purge_expired(&self) -> Result<PurgeReport, RunStoreError> {
        self.store.purge_expired(chrono::Utc::now())
    }

    /// Runs the store holds (created, not purged), from the run index.
    pub fn stored_run_count(&self) -> Result<usize, RunStoreError> {
        let records = self.store.index_records()?;
        let created = records
            .iter()
            .filter(|r| r.kind == IndexKind::Created)
            .count();
        let purged = records
            .iter()
            .filter(|r| r.kind == IndexKind::Purged)
            .count();
        Ok(created.saturating_sub(purged))
    }

    /// Starts a run: permit, files, durable `TaskCreated`, then the
    /// deliberation in the background. Returns once the task is durable.
    pub async fn start(self: &Arc<Self>, req: StartRequest) -> Result<RunSnapshot, StartError> {
        if self.is_shutting_down() {
            return Err(StartError::ShuttingDown);
        }
        let semaphore = match req.kind {
            RunKind::Guided => self.guided.clone(),
            RunKind::Live => self.live.clone(),
        };
        let permit = match semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                if self.queued.load(Ordering::SeqCst) >= self.settings.max_queue {
                    return Err(StartError::Busy {
                        retry_after: Duration::from_secs(5),
                    });
                }
                self.queued.fetch_add(1, Ordering::SeqCst);
                let waited =
                    tokio::time::timeout(self.settings.admission_wait, semaphore.acquire_owned())
                        .await;
                self.queued.fetch_sub(1, Ordering::SeqCst);
                match waited {
                    Ok(Ok(permit)) => permit,
                    _ => {
                        return Err(StartError::Busy {
                            retry_after: Duration::from_secs(5),
                        })
                    }
                }
            }
        };
        if self.is_shutting_down() {
            return Err(StartError::ShuttingDown);
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.start_blocking(req, permit))
            .await
            .map_err(|e| StartError::Internal(format!("start task: {e}")))?
    }

    fn start_blocking(
        self: Arc<Self>,
        req: StartRequest,
        permit: OwnedSemaphorePermit,
    ) -> Result<RunSnapshot, StartError> {
        let StartRequest {
            owner,
            retention,
            input,
            kind,
            mut deliberation,
        } = req;
        let _ = kind;
        let handle = self.store.create_run(owner.clone(), retention)?;
        let run_id = handle.run_id().clone();
        let meta = handle.meta().clone();
        self.broadcasts.register(&run_id);
        let publisher: Arc<dyn EventPublisher> = self.broadcasts.clone();
        let mut log = handle.into_shared(publisher);
        let (agg, task) = match deliberation.create_task(&mut log, run_id.clone(), input) {
            Ok(created) => created,
            Err(e) => {
                self.broadcasts.unregister(&run_id);
                return Err(e.into());
            }
        };
        let events = log.snapshot_from(0).map_err(RunStoreError::Log)?;
        let control = RunControl::with_deadline(Instant::now() + self.settings.deadline);
        self.active_map().insert(
            run_id.clone(),
            ActiveRun {
                log: log.clone(),
                control: control.clone(),
                owner,
                meta: meta.clone(),
            },
        );
        let snapshot = RunSnapshot {
            meta,
            events,
            aggregate: agg.clone(),
            active: true,
        };
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            this.run_deliberation(run_id, log, agg, task, control, deliberation);
            drop(permit);
        });
        Ok(snapshot)
    }

    /// The background half of a run. Never panics out: a panic in the
    /// deliberation is recorded against the run and the run is let go.
    fn run_deliberation(
        self: Arc<Self>,
        run_id: RunId,
        mut log: SharedRunLog,
        agg: RunAggregate,
        task: Task,
        control: RunControl,
        mut deliberation: Box<dyn Deliberation>,
    ) {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            deliberation.deliberate(&mut log, agg, &task, &control)
        }));
        match outcome {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                eprintln!("[runs] run {run_id} stopped with an error: {e}");
                // LC-07: an internal error still tries to end the run.
                self.try_fail(
                    &log,
                    "internal",
                    "internal",
                    "the runtime could not continue the run",
                );
            }
            Err(payload) => {
                let what = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".into());
                eprintln!("[runs] run {run_id} panicked: {what}");
                self.try_fail(&log, "panic", "panic", "the deliberation thread panicked");
            }
        }
        // Drop the writer first so the run's lock is free before anyone
        // learns the run is no longer active.
        drop(log);
        self.active_map().remove(&run_id);
        self.broadcasts.unregister(&run_id);
        if self.active_map().is_empty() {
            self.idle.notify_waiters();
        }
    }

    /// Appends a terminal `RunFailed` unless the run already ended. The
    /// reason is fixed text, never a panic message.
    fn try_fail(&self, log: &SharedRunLog, stage: &str, code: &str, reason: &str) {
        if log.is_poisoned() {
            eprintln!(
                "[runs] run {} lock poisoned; recovery closes it at the next start",
                log.run_id()
            );
            return;
        }
        match log.aggregate() {
            Ok(agg) if agg.state.is_terminal() => return,
            Ok(_) => {}
            Err(e) => {
                eprintln!("[runs] run {}: cannot read aggregate: {e}", log.run_id());
                return;
            }
        }
        let new = NewEvent {
            event_id: EventId::new(format!("run-failed:{stage}:{}", uuid::Uuid::new_v4())),
            run_id: log.run_id().clone(),
            actor: Actor::Runtime,
            occurred_at: SystemClock.now(),
            payload: EventPayload::RunFailed {
                reason: reason.into(),
                stage: stage.into(),
                code: Some(code.into()),
                invocation: None,
            },
        };
        if let Err(e) = log.append_validated(new) {
            eprintln!("[runs] run {}: cannot record failure: {e}", log.run_id());
        }
    }

    /// The run as `owner` may see it: from its writer while it is active,
    /// from the store otherwise. Anyone else gets `NotFound`.
    pub fn snapshot(&self, id: &str, owner: &Owner) -> Result<RunSnapshot, ReadError> {
        let run_id = parse_run_id(id).map_err(|_| ReadError::NotFound)?;
        if let Some(snapshot) = self.active_snapshot(&run_id, owner)? {
            return Ok(snapshot);
        }
        let view = retry_in_use(|| self.store.read_run(id, owner))?;
        Ok(RunSnapshot {
            meta: view.meta().clone(),
            events: view.events().to_vec(),
            aggregate: view.aggregate().clone(),
            active: false,
        })
    }

    fn active_snapshot(
        &self,
        run_id: &RunId,
        owner: &Owner,
    ) -> Result<Option<RunSnapshot>, ReadError> {
        let (log, meta) = {
            let active = self.active_map();
            match active.get(run_id) {
                Some(run) if run.owner == *owner => (run.log.clone(), run.meta.clone()),
                Some(_) => return Err(ReadError::NotFound),
                None => return Ok(None),
            }
        };
        let events = log.snapshot_from(0)?;
        let aggregate = log.aggregate()?;
        Ok(Some(RunSnapshot {
            meta,
            events,
            aggregate,
            active: true,
        }))
    }

    /// Sidecars of `owner`'s runs, oldest first.
    pub fn list(&self, owner: &Owner) -> Result<Vec<RunMeta>, RunStoreError> {
        self.store.list_for(owner)
    }

    /// Asks a deliberating run to stop at its next checkpoint.
    pub fn cancel(&self, id: &str, owner: &Owner) -> Result<(), CancelError> {
        let run_id = parse_run_id(id).map_err(|_| CancelError::NotFound)?;
        {
            let active = self.active_map();
            if let Some(run) = active.get(&run_id) {
                if run.owner != *owner {
                    return Err(CancelError::NotFound);
                }
                run.control.cancel(CancelReason::Owner);
                return Ok(());
            }
        }
        // Not deliberating here: exists and owned -> not cancellable.
        match retry_in_use(|| self.store.read_run(id, owner)) {
            Ok(_) => Err(CancelError::NotCancellable),
            Err(e) => match ReadError::from(e) {
                ReadError::NotFound => Err(CancelError::NotFound),
                other => Err(CancelError::Read(other)),
            },
        }
    }

    /// Records a human decision on a resting run (owner-checked), with
    /// the idempotency and precondition rules of the route.
    pub fn decide(
        &self,
        id: &str,
        owner: &Owner,
        input: DecisionInput,
    ) -> Result<RunSnapshot, DecideError> {
        let run_id = parse_run_id(id).map_err(|_| DecideError::NotFound)?;
        {
            let active = self.active_map();
            if let Some(run) = active.get(&run_id) {
                return Err(if run.owner == *owner {
                    DecideError::NotAwaiting
                } else {
                    DecideError::NotFound
                });
            }
        }
        let handle =
            retry_in_use(|| self.store.open_run_as(id, owner)).map_err(
                |e| match ReadError::from(e) {
                    ReadError::NotFound => DecideError::NotFound,
                    other => DecideError::Read(other),
                },
            )?;
        let meta = handle.meta().clone();
        let event_id = EventId::new(format!("human-decision:{run_id}:{}", input.idempotency_key));
        let stored = handle
            .log()
            .all_events()
            .iter()
            .find(|e| e.event_id == event_id)
            .cloned();
        let now = SystemClock.now();
        let (occurred_at, decided_at) = match &stored {
            // A retry must reproduce the stored record exactly, so only a
            // different body can differ (IdempotencyConflict).
            Some(record) => {
                let decided = match &record.payload {
                    EventPayload::HumanDecisionRecorded { authorization } => {
                        authorization.decided_at.clone()
                    }
                    _ => record.occurred_at.clone(),
                };
                (record.occurred_at.clone(), decided)
            }
            None => {
                let agg = handle
                    .log()
                    .rebuild(&run_id)
                    .map_err(|e| DecideError::Read(ReadError::Log(e)))?;
                if agg.state != RunState::AwaitingHumanApproval {
                    return Err(DecideError::NotAwaiting);
                }
                let kind = agg.recommendation.as_ref().map(|r| r.kind);
                if kind != Some(input.expected_recommendation) {
                    return Err(DecideError::Conflict(format!(
                        "expected recommendation {:?}, the run has {:?}",
                        input.expected_recommendation, kind
                    )));
                }
                if agg.event_count != input.expected_last_sequence {
                    return Err(DecideError::Conflict(format!(
                        "expected last sequence {}, the run is at {}",
                        input.expected_last_sequence, agg.event_count
                    )));
                }
                if input.approved
                    && matches!(
                        kind,
                        Some(RecommendationKind::Blocked | RecommendationKind::Abstained)
                    )
                    && input
                        .note
                        .as_deref()
                        .map(str::trim)
                        .unwrap_or("")
                        .is_empty()
                {
                    return Err(DecideError::OverrideNoteRequired);
                }
                (now.clone(), now)
            }
        };
        let authorization = HumanAuthorization {
            schema_version: SCHEMA_VERSION.into(),
            run_id: run_id.clone(),
            authorized_by: input.authorized_by.clone(),
            approved: input.approved,
            decided_at,
            idempotency_key: input.idempotency_key.clone(),
            note: input.note.clone(),
            authority: Some(input.authority),
        };
        let new = NewEvent {
            event_id,
            run_id: run_id.clone(),
            actor: Actor::Human {
                id: input.authorized_by.clone(),
            },
            occurred_at,
            payload: EventPayload::HumanDecisionRecorded { authorization },
        };
        let publisher: Arc<dyn EventPublisher> = self.broadcasts.clone();
        let log = handle.into_shared(publisher);
        match log.append_validated(new) {
            Ok(_) => {}
            Err(EventLogError::IdempotencyConflict { .. }) => return Err(DecideError::KeyReused),
            Err(EventLogError::Replay(
                TransitionError::Terminal { .. } | TransitionError::InvalidTransition { .. },
            )) => return Err(DecideError::NotAwaiting),
            Err(e) => return Err(DecideError::Read(ReadError::Log(e))),
        }
        let events = log
            .snapshot_from(0)
            .map_err(|e| DecideError::Read(ReadError::Log(e)))?;
        let aggregate = log
            .aggregate()
            .map_err(|e| DecideError::Read(ReadError::Log(e)))?;
        drop(log);
        Ok(RunSnapshot {
            meta,
            events,
            aggregate,
            active: false,
        })
    }

    /// Graceful shutdown: no new runs, every active run asked to stop,
    /// then a bounded wait for them to reach a checkpoint.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        {
            let active = self.active_map();
            for run in active.values() {
                run.control.cancel(CancelReason::Shutdown);
            }
        }
        let deadline = Instant::now() + self.settings.shutdown_grace;
        while !self.active_map().is_empty() && Instant::now() < deadline {
            let _ = tokio::time::timeout(Duration::from_millis(50), self.idle.notified()).await;
        }
    }
}
