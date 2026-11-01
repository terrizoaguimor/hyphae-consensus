// SPDX-License-Identifier: Apache-2.0
//! The run manager (B2): admission, durable creation, background
//! deliberation on a bounded blocking pool, cancellation, decisions and
//! shutdown, all over the per-run [`RunStore`].
//!
//! Order of a start (LC-07): the caller has already admitted the request
//! (session, quotas, input); a deliberation permit is taken *before*
//! anything durable exists ([`RunManager::reserve`], so a live-mode
//! admission that charges a visitor can run after the permit and before
//! the run); the run's files are created; `TaskCreated` is appended
//! through the run's [`SharedRunLog`] (validated, fsync'd, published);
//! only then is the deliberation spawned and `202` returned with the
//! run's summary. A creation that fails after the files exist discards
//! them again ([`RunStore::discard_empty_run`]), so no indexed, empty run
//! outlives its request.
//!
//! The public server bounds the visitor runs it stores as a ring
//! ([`RunManager::make_room`]): when the configured number is reached,
//! the oldest resting visitor runs are purged to make room, those past
//! their access window first; only when nothing can be freed (every
//! stored visitor run is deliberating or being read) is a creation
//! refused.
//!
//! Each deliberation runs under `catch_unwind`: a panic becomes
//! `RunFailed{stage: "panic", code: "panic"}` through the run's own sink
//! and poisons nothing but that run. A browser disconnect never reaches
//! the manager; only `cancel` and shutdown trip a run's [`RunControl`],
//! which the orchestrator honours at its next checkpoint. The run's model
//! client is bound to the same control before the first call
//! ([`ModelClient::bind_run_control`]): no call outlives the run's
//! deadline, and a call still in progress when the shutdown grace runs
//! out is abandoned, settled and the run's reservation released
//! ([`RunManager::shutdown`]).
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
    parse_run_id, CancelReason, Clock, EventLogError, EventPublisher, IdGen, Orchestrator, Owner,
    PurgeReport, RecoveryReport, Retention, RunAggregate, RunControl, RunMeta, RunStore,
    RunStoreError, SharedRunLog, SystemClock, TaskInput, TransitionError,
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
        // The model bounds its calls by the run's deadline and gives one up
        // when shutdown stops waiting for it (see `shutdown`).
        self.model.bind_run_control(control);
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

/// A deliberation slot held for a run about to start: the permit is taken
/// before anything is charged or stored, and released with the run (or
/// with this value, if the start never happens).
pub struct Reservation {
    permit: OwnedSemaphorePermit,
    kind: RunKind,
}

impl Reservation {
    pub fn kind(&self) -> RunKind {
        self.kind
    }
}

/// What the visitor-run ring did to admit one more run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RingReport {
    /// Visitor runs stored before this call.
    pub stored: usize,
    /// Oldest resting visitor runs purged to make room.
    pub purged: Vec<RunId>,
    /// Candidates skipped because a deliberation or a reader holds them.
    pub in_use: Vec<RunId>,
    /// False only when the ring is full and nothing could be freed.
    pub admitted: bool,
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

    /// Visitor (session-owned) runs the store holds: what the ring counts.
    pub fn session_run_count(&self) -> Result<usize, RunStoreError> {
        self.store.session_run_count()
    }

    /// The visitor-run ring: with `cap` or more visitor runs stored, the
    /// oldest resting ones are purged (those past their access window
    /// first) so that one more fits. A run that is deliberating or being
    /// read is skipped; `admitted` is false only when nothing could be
    /// freed, which is the one case the caller refuses the creation.
    /// Operator and showcase runs neither count nor are ever purged.
    pub fn make_room(&self, cap: usize) -> Result<RingReport, RunStoreError> {
        let stored = self.store.session_run_count()?;
        let need = (stored + 1).saturating_sub(cap);
        if need == 0 {
            return Ok(RingReport {
                stored,
                admitted: true,
                ..RingReport::default()
            });
        }
        let report = self
            .store
            .purge_oldest_session_runs(chrono::Utc::now(), need)?;
        for run_id in &report.purged {
            self.broadcasts.unregister(run_id);
        }
        Ok(RingReport {
            stored,
            admitted: report.purged.len() >= need,
            purged: report.purged,
            in_use: report.in_use,
        })
    }

    /// Takes a deliberation slot of `kind` before anything durable or
    /// chargeable happens: `Busy` when every slot stays taken for the
    /// admission wait (or too many starts already wait), `ShuttingDown`
    /// when the server drains. Hand the reservation to
    /// [`start_reserved`](Self::start_reserved); dropping it frees the slot.
    pub async fn reserve(&self, kind: RunKind) -> Result<Reservation, StartError> {
        if self.is_shutting_down() {
            return Err(StartError::ShuttingDown);
        }
        let semaphore = match kind {
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
        Ok(Reservation { permit, kind })
    }

    /// Starts a run: permit, files, durable `TaskCreated`, then the
    /// deliberation in the background. Returns once the task is durable.
    pub async fn start(self: &Arc<Self>, req: StartRequest) -> Result<RunSnapshot, StartError> {
        let reservation = self.reserve(req.kind).await?;
        self.start_reserved(reservation, req).await
    }

    /// [`start`](Self::start) with a slot already taken by
    /// [`reserve`](Self::reserve): files, durable `TaskCreated`, then the
    /// deliberation in the background. A failure after the run's files
    /// exist discards them again and leaves no orphan run.
    pub async fn start_reserved(
        self: &Arc<Self>,
        reservation: Reservation,
        req: StartRequest,
    ) -> Result<RunSnapshot, StartError> {
        if reservation.kind != req.kind {
            return Err(StartError::Internal(format!(
                "a {:?} reservation cannot start a {:?} run",
                reservation.kind, req.kind
            )));
        }
        if self.is_shutting_down() {
            return Err(StartError::ShuttingDown);
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.start_blocking(req, reservation.permit))
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
                // LC-07: nothing durable describes this run yet (or only a
                // task nobody will deliberate); let it go whole.
                self.abandon_start(&run_id, log, "task creation failed");
                return Err(e.into());
            }
        };
        let events = match log.snapshot_from(0) {
            Ok(events) => events,
            Err(e) => {
                self.abandon_start(&run_id, log, "the stored task could not be read back");
                return Err(RunStoreError::Log(e).into());
            }
        };
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

    /// A start that failed after the run's files were created: closes the
    /// run's channel, lets its writer go, and discards the files if no
    /// record was stored. A run whose task did get stored is closed as
    /// failed instead, so it never lingers in `created` until a restart.
    fn abandon_start(&self, run_id: &RunId, log: SharedRunLog, why: &str) {
        eprintln!("[runs] run {run_id} abandoned at start: {why}");
        self.broadcasts.unregister(run_id);
        let stored = log.snapshot_from(0).map(|e| e.len()).unwrap_or(0);
        if stored > 0 {
            self.try_fail(&log, "internal", "internal", "the run could not be started");
        }
        // The writer holds the run's lock; the discard needs it free.
        drop(log);
        match self.store.discard_empty_run(run_id.as_str()) {
            Ok(true) => eprintln!("[runs] run {run_id} discarded: no record was stored"),
            Ok(false) => {}
            Err(e) => eprintln!("[runs] run {run_id} could not be discarded: {e}"),
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

    /// Whether `owner` may reach the run, without reading its log: the
    /// active map first, then the run's sidecar (cached after its first
    /// read). This is the cheap check a route makes before it commits a
    /// stream slot or a full read; anyone else gets `NotFound`, as from
    /// [`snapshot`](Self::snapshot).
    pub fn owned_meta(&self, id: &str, owner: &Owner) -> Result<RunMeta, ReadError> {
        let run_id = parse_run_id(id).map_err(|_| ReadError::NotFound)?;
        {
            let active = self.active_map();
            if let Some(run) = active.get(&run_id) {
                return if run.owner == *owner {
                    Ok(run.meta.clone())
                } else {
                    Err(ReadError::NotFound)
                };
            }
        }
        Ok(self.store.meta_for(id, owner)?)
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
        // One critical section for both: a record the writer appends between
        // two reads would otherwise be in the aggregate and not in the
        // events (a failed run with no failure record).
        let (events, aggregate) = log.read(|stored| {
            let events = stored.all_events().to_vec();
            stored.rebuild(run_id).map(|aggregate| (events, aggregate))
        })??;
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
    /// then a bounded wait for them to reach a checkpoint (a model call in
    /// progress is answered and recorded first). Runs still active once
    /// [`ManagerSettings::shutdown_grace`] has passed have their call in
    /// progress aborted ([`RunControl::abort_in_flight`]): the Meta adapter
    /// stops waiting, settles the attempt at its estimate and fails the
    /// run with code `shutdown`, and the run's end releases its live
    /// reservation; this waits up to [`ABORT_SETTLE_WAIT`] more for that,
    /// so nothing is left reserved when the server returns.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        {
            let active = self.active_map();
            for run in active.values() {
                run.control.cancel(CancelReason::Shutdown);
            }
        }
        let deadline = Instant::now() + self.settings.shutdown_grace;
        self.wait_idle(deadline).await;
        let aborted: Vec<RunId> = {
            let active = self.active_map();
            active
                .iter()
                .filter(|(_, run)| run.control.abort_in_flight())
                .map(|(id, _)| id.clone())
                .collect()
        };
        if aborted.is_empty() {
            return;
        }
        for run_id in &aborted {
            eprintln!(
                "[shutdown] run {run_id} still deliberating after the {} s grace: its model call in progress is abandoned and settled at its estimate",
                self.settings.shutdown_grace.as_secs()
            );
        }
        self.wait_idle(Instant::now() + ABORT_SETTLE_WAIT).await;
        let left = self.active_map().len();
        if left > 0 {
            eprintln!(
                "[shutdown] {left} run(s) still active after the abort; recovery closes them at the next start (an unreleased live reservation is listed by `consensus-api ledger status`)"
            );
        }
    }

    /// Waits until no run is active or `deadline` passes.
    async fn wait_idle(&self, deadline: Instant) {
        while !self.active_map().is_empty() && Instant::now() < deadline {
            let _ = tokio::time::timeout(Duration::from_millis(50), self.idle.notified()).await;
        }
    }
}

/// How long [`RunManager::shutdown`] waits, after aborting the calls in
/// progress, for the aborted runs to settle, record their failure and
/// release their reservations. Inside the unit's stop timeout: grace plus
/// this plus the HTTP drain stays below `TimeoutStopSec`.
pub const ABORT_SETTLE_WAIT: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKey, MetaConfig};
    use crate::live::budget::{BudgetConfig, Lane, Ledger};
    use crate::live::gate::MeteredReservation;
    use crate::live::money::{Ppm, Usd};
    use crate::meta::MetaModelClient;
    use consensus_runtime::fakes::{FakeMemoryClient, FakeProofVerifier};
    use consensus_runtime::{RunLimits, SequentialIdGen};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;

    const PROPOSAL: &str = r#"{"summary":"Raise retries","plan_steps":["edit config"],"primary_claim":{"statement":"Timeouts come from too few retries"},"further_claims":[],"assumptions":[],"uncertainties":[]}"#;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-runs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A Chat Completions double that answers every request with one
    /// proposal after `delay`; returns its base URL.
    fn slow_meta(delay: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                std::thread::spawn(move || {
                    let mut data = Vec::new();
                    let mut buf = [0u8; 8192];
                    // Headers and the whole body (the request is small).
                    while let Ok(n) = s.read(&mut buf) {
                        if n == 0 {
                            return;
                        }
                        data.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&data);
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length = text[..end]
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            if data.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    std::thread::sleep(delay);
                    let body = serde_json::json!({
                        "id": "chatcmpl-runs-1",
                        "model": "muse-wonder-1.0",
                        "choices": [{"message": {"role": "assistant", "content": PROPOSAL}, "finish_reason": "stop"}],
                        "usage": {"prompt_tokens": 100, "completion_tokens": 50}
                    })
                    .to_string();
                    let _ = s.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    );
                });
            }
        });
        format!("http://{addr}/v1")
    }

    fn ledger(dir: &std::path::Path) -> Ledger {
        Ledger::open(
            dir,
            BudgetConfig {
                daily_cap: Usd::parse("10.00").unwrap(),
                lifetime_cap: Usd::parse("60.00").unwrap(),
                judge_slice: Usd::parse("15.00").unwrap(),
                judge_daily_cap: Usd::parse("15.00").unwrap(),
                max_concurrent: 1,
                hourly_admissions: 100,
                burst_fraction: Ppm::ONE,
                run_token_cap: 70_000,
                max_completion_tokens: 16_000,
                input_cap_tokens: 32_000,
                price_input_per_m: Usd::parse("4.00").unwrap(),
                price_output_per_m: Usd::parse("20.00").unwrap(),
                kill_switch_file: None,
            },
        )
        .unwrap()
    }

    /// Records of one `type` across the ledger's day files.
    fn records(dir: &std::path::Path, kind: &str) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                let text = std::fs::read_to_string(&path).unwrap();
                out.extend(
                    text.lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
                        .filter(|v| v["type"] == kind),
                );
            }
        }
        out
    }

    /// One metered live run against a Meta double answering after
    /// `call`, started, then the manager shut down with `grace`. Returns
    /// the run's last record, the ledger and its directory, and how long
    /// the shutdown took.
    async fn shut_down_during_a_call(
        name: &str,
        call: Duration,
        grace: Duration,
    ) -> (ConsensusEvent, Ledger, PathBuf, Duration) {
        let root = temp_dir(name);
        let ledger_dir = root.join("ledger");
        let ledger = ledger(&ledger_dir);
        let store = Arc::new(RunStore::open(root.join("store")).unwrap());
        let manager = RunManager::new(
            store,
            Broadcasts::new(64),
            ManagerSettings {
                shutdown_grace: grace,
                ..ManagerSettings::default()
            },
        );
        let base_url = slow_meta(call);
        let reserving = ledger.clone();
        // reqwest's blocking client is built (and dropped) off the async
        // workers, as the gate does.
        let deliberation = tokio::task::spawn_blocking(move || {
            let mut cfg = MetaConfig::from_parts(
                base_url,
                ApiKey::new("LLM_1000_test-key-not-a-secret"),
                "muse-wonder-1.0",
            )
            .unwrap();
            cfg.timeout = Duration::from_secs(60);
            let metered = MeteredReservation::reserve(&reserving, Lane::Public).unwrap();
            let model = metered.meter_model(MetaModelClient::new(cfg).unwrap());
            let orchestrator = Orchestrator {
                model,
                memory: FakeMemoryClient::new(vec![consensus_runtime::scenarios::evidence(
                    "ev-1",
                    "demo-repo",
                    "constraint",
                    "Constraint: outbound retries must be capped at 3 attempts.",
                )]),
                verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
                clock: SystemClock,
                ids: SequentialIdGen::new(),
                limits: RunLimits::default(),
            };
            Box::new(metered.wrap(Box::new(orchestrator))) as Box<dyn Deliberation>
        })
        .await
        .unwrap();
        let started = manager
            .start(StartRequest {
                owner: Owner::Operator,
                retention: Retention::Keep,
                input: TaskInput {
                    project_id: ProjectId::new("demo-repo"),
                    title: "Fix payment retries".into(),
                    description: "Retries burst under load.".into(),
                    submitted_by: "test".into(),
                    mode: None,
                },
                kind: RunKind::Live,
                deliberation,
            })
            .await
            .unwrap();
        let run_id = started.meta.run_id.clone();
        // The propose call is in flight once its attempt is drawn.
        let wait_for_draw = Instant::now() + Duration::from_secs(10);
        while records(&ledger_dir, "draw").is_empty() {
            assert!(Instant::now() < wait_for_draw, "the call never started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let stop = Instant::now();
        manager.shutdown().await;
        let took = stop.elapsed();
        assert!(
            !manager.is_active(&run_id),
            "the run ended before shutdown returned"
        );
        let view = manager.snapshot(run_id.as_str(), &Owner::Operator).unwrap();
        let last = view.events.last().cloned().unwrap();
        (last, ledger, ledger_dir, took)
    }

    fn failure(event: &ConsensusEvent) -> (String, Option<String>) {
        match &event.payload {
            EventPayload::RunFailed { stage, code, .. } => (stage.clone(), code.clone()),
            other => panic!("expected RunFailed, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_call_answered_within_the_grace_is_recorded_and_the_run_stops_at_shutdown() {
        let (last, ledger, dir, took) = shut_down_during_a_call(
            "grace-holds",
            Duration::from_secs(2),
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(failure(&last), ("shutdown".into(), Some("shutdown".into())));
        // Well inside the grace (wide margin for a loaded machine): the
        // answer ended the wait, not the grace.
        assert!(took < Duration::from_secs(9), "{took:?}");
        let draws = records(&dir, "draw");
        let settlements = records(&dir, "settlement");
        assert_eq!(draws.len(), 1);
        assert_eq!(settlements.len(), 1, "every draw settled");
        assert_eq!(settlements[0]["outcome"]["kind"], "usage", "answered");
        assert!(ledger.open_reservations().unwrap().is_empty(), "released");
        assert_eq!(ledger.snapshot().unwrap().active_public, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_call_past_the_grace_is_aborted_settled_at_its_estimate_and_released() {
        let (last, ledger, dir, took) = shut_down_during_a_call(
            "grace-runs-out",
            Duration::from_secs(40),
            Duration::from_secs(2),
        )
        .await;
        // The propose call was abandoned: the run failed at its stage with
        // the shutdown code, long before the double would have answered
        // (40 s): the abort is polled every 50 ms, so 5 s past the 2 s
        // grace is a wide margin, and still well below the settle wait.
        assert_eq!(failure(&last), ("propose".into(), Some("shutdown".into())));
        assert!(took < Duration::from_secs(7), "{took:?}");
        let draws = records(&dir, "draw");
        let settlements = records(&dir, "settlement");
        assert_eq!(draws.len(), 1);
        assert_eq!(settlements.len(), 1, "the abandoned attempt is settled");
        assert_eq!(
            settlements[0]["charged_usd"], draws[0]["est_usd"],
            "at its estimate"
        );
        assert!(ledger.open_reservations().unwrap().is_empty(), "released");
        assert_eq!(ledger.snapshot().unwrap().active_public, 0);
    }
}
