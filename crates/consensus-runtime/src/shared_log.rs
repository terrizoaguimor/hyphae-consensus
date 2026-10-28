// SPDX-License-Identifier: Apache-2.0
//! One run's event log, shared by every thread that writes to the run.
//!
//! A [`SharedRunLog`] wraps the [`EventLog`] of a single run behind a
//! mutex, and its clones share it. While a run deliberates, other writers
//! can still reach it (a human decision, an owner's cancel, the shutdown
//! coordinator), so each record is checked against the run's stored
//! history in the same critical section that appends it (LC-07):
//!
//! 1. take the run's lock;
//! 2. rebuild the run's aggregate from the stored events and apply the
//!    new event to it, so a transition the stored history does not allow
//!    (an event after a terminal one, a second decision) is refused
//!    before anything is written;
//! 3. append: sequence, chain hash, write and fsync;
//! 4. for a run kept in a [`RunStore`](crate::run_store::RunStore),
//!    record the run's head in the run index when the record leaves the
//!    run at a checkpoint (its task recorded, awaiting a decision) or ends
//!    it, still under the lock, so the index follows the run's sequence
//!    order;
//! 5. clone the stored record and release the lock;
//! 6. publish the record to the run's [`EventPublisher`].
//!
//! A refused or failed append is therefore never published, and every
//! published record is already on disk.
//!
//! Publication runs on the appending thread, after the lock is released.
//! A slow publisher delays that writer's return (and that record's
//! subscribers), never another writer's append. A publisher that panics
//! is caught: the record stays stored and acknowledged, the failure is
//! counted ([`SharedRunLog::publish_failures`]), and nothing unwinds into
//! the writer, which may be the orchestrator mid-deliberation.
//!
//! Publication order is not sequence order: two clones appending at
//! almost the same moment may publish in either order (publishing under
//! the lock would fix the order, at the cost of letting one slow
//! publisher stall every writer of the run). A record a publisher dropped
//! or a panic lost leaves a gap the same way. Subscribers therefore
//! dedupe by sequence, fill every gap from
//! [`SharedRunLog::snapshot_from`], which reads the durable log, and end
//! a stream only once the run's durable head is terminal. One that
//! subscribes first and then takes a snapshot misses nothing.
//!
//! The runtime crate stays free of an async runtime: [`EventPublisher`]
//! is a plain trait, which the server implements over its per-run
//! broadcast channels.

use crate::event_log::{EventLog, EventLogError};
use crate::run_store::{IndexKind, RunLease};
use crate::sink::EventSink;
use crate::state_machine::{RunAggregate, TransitionError};
use consensus_domain::{ConsensusEvent, EventPayload, NewEvent, RunId, RunState};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Receives every record a [`SharedRunLog`] stores, once it is durable.
pub trait EventPublisher: Send + Sync {
    /// Called once per newly stored record, after its fsync and without
    /// the run's lock held, on the thread that appended it. A retried
    /// event id that is already stored publishes nothing. Should not
    /// block, since the writer waits for it: a server forwards to a
    /// bounded channel and lets a lagging subscriber reconcile from the
    /// log. A panic here is caught and counted, never passed to the
    /// writer.
    fn publish(&self, event: &ConsensusEvent);
}

/// True when a record for `record_run` with `payload` does not belong in
/// `run_id`'s log: it names another run, or it is a task for another run.
/// The store refuses a log holding such a record, so no writer may store
/// one.
pub(crate) fn names_another_run(
    run_id: &RunId,
    record_run: &RunId,
    payload: &EventPayload,
) -> bool {
    record_run != run_id
        || matches!(payload, EventPayload::TaskCreated { task } if task.run_id != *run_id)
}

/// A publisher without subscribers.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullPublisher;

impl EventPublisher for NullPublisher {
    fn publish(&self, _event: &ConsensusEvent) {}
}

/// Validating, publishing writer for one run's event log. Cheap to
/// clone: clones share the log, its lock and the publisher.
#[derive(Clone)]
pub struct SharedRunLog {
    run_id: RunId,
    log: Arc<Mutex<EventLog>>,
    publisher: Arc<dyn EventPublisher>,
    /// Ownership of a [`RunStore`](crate::run_store::RunStore) run: its
    /// exclusive lock file, held while any clone lives, and its run
    /// index. None for a bare log.
    lease: Option<Arc<RunLease>>,
    /// Publications that panicked, across every clone.
    publish_failures: Arc<AtomicU64>,
}

impl std::fmt::Debug for SharedRunLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedRunLog")
            .field("run_id", &self.run_id)
            .field("leased", &self.lease.is_some())
            .finish_non_exhaustive()
    }
}

impl SharedRunLog {
    /// Shares `log`, which must hold only `run_id`'s events (one run per
    /// file, every task this run's) and replay cleanly.
    ///
    /// No ownership lock is taken here, so nothing stops a second
    /// [`EventLog`] over the same file, in this process or another, from
    /// writing beside it. Runs kept in a
    /// [`RunStore`](crate::run_store::RunStore) are shared through
    /// [`RunHandle::into_shared`](crate::run_store::RunHandle::into_shared),
    /// which holds the run's exclusive lock and indexes its heads.
    pub fn new(
        run_id: RunId,
        log: EventLog,
        publisher: Arc<dyn EventPublisher>,
    ) -> Result<Self, EventLogError> {
        if log
            .all_events()
            .iter()
            .any(|e| names_another_run(&run_id, &e.run_id, &e.payload))
        {
            return Err(TransitionError::RunMismatch.into());
        }
        log.rebuild(&run_id)?;
        Ok(Self {
            run_id,
            log: Arc::new(Mutex::new(log)),
            publisher,
            lease: None,
            publish_failures: Arc::default(),
        })
    }

    /// A store-owned run; the store has already checked `log`.
    pub(crate) fn leased(
        run_id: RunId,
        log: EventLog,
        publisher: Arc<dyn EventPublisher>,
        lease: Arc<RunLease>,
    ) -> Self {
        Self {
            run_id,
            log: Arc::new(Mutex::new(log)),
            publisher,
            lease: Some(lease),
            publish_failures: Arc::default(),
        }
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// Appends `new` if the run's stored history allows it, then
    /// publishes the stored record (see the module docs for the steps).
    ///
    /// - An event for another run, or a task naming another run, is
    ///   refused (`RunMismatch`) before the lock is taken: the store would
    ///   refuse the log forever after.
    /// - An `event_id` that is already stored returns the stored record,
    ///   as [`EventSink`] requires. It is neither validated again (a
    ///   retried decision must not fail because its first attempt ended
    ///   the run) nor published again.
    /// - A transition the stored history refuses is
    ///   [`EventLogError::Replay`]; nothing is written or published.
    /// - When the run belongs to a store and the record records its task,
    ///   leaves it awaiting a decision or ends it, the run's head is
    ///   recorded in the run index under the lock, before the record is
    ///   published.
    /// - A publisher that panics does not make the append fail: the
    ///   record is stored, and the panic is counted in
    ///   [`publish_failures`](Self::publish_failures).
    pub fn append_validated(&self, new: NewEvent) -> Result<ConsensusEvent, EventLogError> {
        if names_another_run(&self.run_id, &new.run_id, &new.payload) {
            return Err(TransitionError::RunMismatch.into());
        }
        let stored = {
            let mut log = self.lock()?;
            if log.contains(&new.event_id) {
                return EventSink::append(&mut *log, new);
            }
            // Validate against the history on disk, not against a copy a
            // writer kept: another clone may have appended since.
            let mut probe = log.rebuild(&self.run_id)?;
            let task = matches!(new.payload, EventPayload::TaskCreated { .. });
            probe.apply(&new.clone().into_sequenced(0))?;
            let stored = EventSink::append(&mut *log, new)?;
            if let Some(lease) = &self.lease {
                if probe.state.is_terminal() {
                    lease.record_head(IndexKind::Terminal, &stored);
                } else if task || probe.state == RunState::AwaitingHumanApproval {
                    lease.record_head(IndexKind::Checkpoint, &stored);
                }
            }
            stored
        };
        self.publish(&stored);
        Ok(stored)
    }

    /// Publications that panicked so far, across every clone. Each such
    /// record is stored all the same; its subscribers miss it until they
    /// fill the gap from [`snapshot_from`](Self::snapshot_from).
    pub fn publish_failures(&self) -> u64 {
        self.publish_failures.load(Ordering::SeqCst)
    }

    fn publish(&self, stored: &ConsensusEvent) {
        // The record is durable and the lock released; a panic in the
        // publisher must not reach the writer, which would abandon a
        // deliberation whose record was in fact stored.
        let published = catch_unwind(AssertUnwindSafe(|| self.publisher.publish(stored)));
        if published.is_err() {
            self.publish_failures.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Stored records with `sequence > after`, in sequence order: `0`
    /// gives the whole run, and the last sequence an SSE client saw
    /// gives exactly what it missed. Sequences start at 1 with no gaps,
    /// so an `after` at or past the head gives nothing.
    pub fn snapshot_from(&self, after: u64) -> Result<Vec<ConsensusEvent>, EventLogError> {
        Ok(self
            .lock()?
            .all_events()
            .iter()
            .filter(|e| e.sequence > after)
            .cloned()
            .collect())
    }

    /// The run's aggregate, rebuilt from its stored events.
    pub fn aggregate(&self) -> Result<RunAggregate, EventLogError> {
        self.lock()?.rebuild(&self.run_id)
    }

    /// Runs `f` on the log under the run's lock, e.g. to build a
    /// receipt. Appends wait meanwhile, so keep `f` short; a panic in
    /// `f` poisons the run's lock.
    pub fn read<R>(&self, f: impl FnOnce(&EventLog) -> R) -> Result<R, EventLogError> {
        let log = self.lock()?;
        Ok(f(&log))
    }

    /// True once a thread panicked while holding the run's lock. The log
    /// may then differ from what is on disk, so every append and read
    /// through any clone fails from then on; reopening the run (startup
    /// recovery) re-verifies the file.
    pub fn is_poisoned(&self) -> bool {
        self.log.is_poisoned()
    }

    fn lock(&self) -> Result<MutexGuard<'_, EventLog>, EventLogError> {
        self.log.lock().map_err(|_| {
            EventLogError::Io(std::io::Error::other(format!(
                "event log lock of run {} poisoned: a thread panicked while holding it",
                self.run_id
            )))
        })
    }
}

impl EventSink for SharedRunLog {
    fn append(&mut self, new: NewEvent) -> Result<ConsensusEvent, EventLogError> {
        self.append_validated(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, IdGen, SequentialIdGen};
    use crate::control::RunControl;
    use crate::fakes::{FakeMemoryClient, FakeModelClient, FakeProofVerifier};
    use crate::orchestrator::{Orchestrator, OrchestratorError, RunLimits};
    use crate::ports::{ModelClient, ModelError, RoleContext};
    use crate::scenarios::{retry_constraint_scenario, TS};
    use consensus_domain::*;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::sync::{Barrier, Condvar, OnceLock};

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-shared-log-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    fn shared(path: &Path, run: &RunId, publisher: Arc<dyn EventPublisher>) -> SharedRunLog {
        SharedRunLog::new(run.clone(), EventLog::open(path).unwrap(), publisher).unwrap()
    }

    fn task_event(run: &RunId, id: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Human { id: "mario".into() },
            occurred_at: TS.into(),
            payload: EventPayload::TaskCreated {
                task: Task {
                    schema_version: SCHEMA_VERSION.into(),
                    id: TaskId::new("task-1"),
                    run_id: run.clone(),
                    project_id: ProjectId::new("p"),
                    title: "t".into(),
                    description: "d".into(),
                    created_at: TS.into(),
                    mode: None,
                },
            },
        }
    }

    fn fail_event(run: &RunId, id: &str, stage: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Runtime,
            occurred_at: TS.into(),
            payload: EventPayload::RunFailed {
                reason: format!("stopped at {stage}"),
                stage: stage.into(),
                code: None,
                invocation: None,
            },
        }
    }

    fn decision_event(run: &RunId, approved: bool, key: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(format!("human-decision:{run}:{key}")),
            run_id: run.clone(),
            actor: Actor::Human { id: "mario".into() },
            occurred_at: TS.into(),
            payload: EventPayload::HumanDecisionRecorded {
                authorization: HumanAuthorization {
                    schema_version: SCHEMA_VERSION.into(),
                    run_id: run.clone(),
                    authorized_by: "mario".into(),
                    approved,
                    decided_at: TS.into(),
                    idempotency_key: key.into(),
                    note: None,
                    authority: None,
                },
            },
        }
    }

    /// Records every published record.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<ConsensusEvent>>);

    impl Recorder {
        fn events(&self) -> Vec<ConsensusEvent> {
            self.0.lock().unwrap().clone()
        }
    }

    impl EventPublisher for Recorder {
        fn publish(&self, event: &ConsensusEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    type TestOrchestrator<M> =
        Orchestrator<M, FakeMemoryClient, FakeProofVerifier, FixedClock, SequentialIdGen>;

    fn orchestrator<M: ModelClient>(model: M, memory: FakeMemoryClient) -> TestOrchestrator<M> {
        Orchestrator {
            model,
            memory,
            verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        }
    }

    /// The reference scenario, deliberated through `log` to its human
    /// decision point.
    fn deliberate_to_awaiting(log: &mut SharedRunLog) -> RunAggregate {
        let s = retry_constraint_scenario();
        let mut orch = orchestrator(s.model, s.memory);
        let run_id = log.run_id().clone();
        let (agg, task) = orch.create_task(log, run_id, s.input).unwrap();
        let agg = orch
            .deliberate(log, agg, &task, &RunControl::new())
            .unwrap();
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
        agg
    }

    /// Re-reads the run file as another process would and looks for the
    /// exact line of each record it is handed. Also checks that the run's
    /// lock is free while it is called.
    struct DurabilityProbe {
        path: PathBuf,
        log: OnceLock<Arc<Mutex<EventLog>>>,
        found: Mutex<Vec<u64>>,
        problems: Mutex<Vec<String>>,
    }

    impl EventPublisher for DurabilityProbe {
        fn publish(&self, event: &ConsensusEvent) {
            let line = serde_json::to_string(event).unwrap();
            let on_disk = std::fs::read_to_string(&self.path).unwrap();
            if on_disk.lines().any(|l| l == line) {
                self.found.lock().unwrap().push(event.sequence);
            } else {
                self.problems.lock().unwrap().push(format!(
                    "{} published before it was on disk",
                    event.event_id
                ));
            }
            let lock_free = self.log.get().is_some_and(|log| log.try_lock().is_ok());
            if !lock_free {
                self.problems
                    .lock()
                    .unwrap()
                    .push(format!("{} published under the run lock", event.event_id));
            }
        }
    }

    #[test]
    fn publishes_each_record_once_after_it_is_durable_and_outside_the_lock() {
        let path = temp_log("durable.jsonl");
        let probe = Arc::new(DurabilityProbe {
            path: path.clone(),
            log: OnceLock::new(),
            found: Mutex::default(),
            problems: Mutex::default(),
        });
        let mut log = shared(&path, &RunId::new("run-durable"), probe.clone());
        probe.log.set(log.log.clone()).unwrap();

        let agg = deliberate_to_awaiting(&mut log);
        let stored = log.snapshot_from(0).unwrap();
        assert_eq!(stored.len() as u64, agg.event_count);
        assert!(
            probe.problems.lock().unwrap().is_empty(),
            "{:?}",
            probe.problems
        );
        let expected: Vec<u64> = (1..=agg.event_count).collect();
        assert_eq!(
            *probe.found.lock().unwrap(),
            expected,
            "each record once, in order"
        );
        // What was published is what a fresh open verifies.
        drop(log);
        assert_eq!(EventLog::open(&path).unwrap().all_events(), stored);
    }

    #[test]
    fn refused_appends_are_neither_stored_nor_published() {
        let path = temp_log("refused.jsonl");
        let run = RunId::new("run-refused");
        let recorder = Arc::new(Recorder::default());
        let mut log = shared(&path, &run, recorder.clone());

        // Before the task exists, deliberation events are refused.
        let early = decision_event(&run, true, "k1");
        assert!(matches!(
            log.append_validated(early),
            Err(EventLogError::Replay(TransitionError::InvalidTransition {
                state: RunState::Created,
                ..
            }))
        ));
        // Another run's record never enters this file.
        let foreign = task_event(&RunId::new("run-other"), "e-foreign");
        assert!(matches!(
            log.append(foreign),
            Err(EventLogError::Replay(TransitionError::RunMismatch))
        ));
        log.append(task_event(&run, "e1")).unwrap();
        // A second task for the same run fails replay, so it is refused
        // here rather than stored (the create_task caveat for EventLog).
        assert!(matches!(
            log.append(task_event(&run, "e2")),
            Err(EventLogError::Replay(TransitionError::DuplicateTask))
        ));
        let ended = log.append(fail_event(&run, "e3", "propose")).unwrap();
        assert!(matches!(
            log.append(fail_event(&run, "e4", "shutdown")),
            Err(EventLogError::Replay(TransitionError::Terminal {
                state: RunState::Failed
            }))
        ));

        let stored = log.snapshot_from(0).unwrap();
        let ids: Vec<&str> = stored.iter().map(|e| e.event_id.as_str()).collect();
        assert_eq!(ids, ["e1", "e3"]);
        assert_eq!(
            recorder.events(),
            stored,
            "only stored records were published"
        );
        assert_eq!(log.aggregate().unwrap().state, RunState::Failed);
        assert_eq!(stored.last(), Some(&ended));
        drop(log);
        assert_eq!(EventLog::open(&path).unwrap().all_events(), stored);
    }

    #[test]
    fn a_retried_event_id_returns_the_stored_record_and_publishes_nothing() {
        let path = temp_log("retry.jsonl");
        let run = RunId::new("run-retry");
        let recorder = Arc::new(Recorder::default());
        let log = shared(&path, &run, recorder.clone());
        let task = log.append_validated(task_event(&run, "e1")).unwrap();
        let ended = log
            .append_validated(fail_event(&run, "e2", "propose"))
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();

        // Both retries come back as stored, although the run has ended:
        // a retry is not a new transition.
        assert_eq!(log.append_validated(task_event(&run, "e1")).unwrap(), task);
        assert_eq!(
            log.append_validated(fail_event(&run, "e2", "propose"))
                .unwrap(),
            ended
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        assert_eq!(recorder.events(), [task, ended], "published once each");
    }

    #[test]
    fn two_handles_racing_two_decisions_record_exactly_one() {
        for round in 0..12 {
            let path = temp_log(&format!("decision-race-{round}.jsonl"));
            let run = RunId::new(format!("run-race-{round}"));
            let recorder = Arc::new(Recorder::default());
            let mut log = shared(&path, &run, recorder.clone());
            deliberate_to_awaiting(&mut log);
            let before = recorder.events().len();

            let barrier = Arc::new(Barrier::new(2));
            let racers: Vec<_> = [(true, "approve"), (false, "reject")]
                .into_iter()
                .map(|(approved, key)| {
                    let handle = log.clone();
                    let barrier = barrier.clone();
                    let event = decision_event(&run, approved, key);
                    std::thread::spawn(move || {
                        barrier.wait();
                        (approved, handle.append_validated(event))
                    })
                })
                .collect();
            let outcomes: Vec<_> = racers.into_iter().map(|t| t.join().unwrap()).collect();

            let winners: Vec<bool> = outcomes
                .iter()
                .filter(|(_, r)| r.is_ok())
                .map(|(approved, _)| *approved)
                .collect();
            assert_eq!(winners.len(), 1, "round {round}: {outcomes:?}");
            let state = if winners[0] {
                RunState::Approved
            } else {
                RunState::Rejected
            };
            let loser = outcomes.iter().find(|(_, r)| r.is_err()).unwrap();
            assert!(
                matches!(
                    &loser.1,
                    Err(EventLogError::Replay(TransitionError::Terminal { state: s })) if *s == state
                ),
                "round {round}: {loser:?}"
            );

            let decisions = |events: &[ConsensusEvent]| {
                events
                    .iter()
                    .filter(|e| matches!(e.payload, EventPayload::HumanDecisionRecorded { .. }))
                    .count()
            };
            let stored = log.snapshot_from(0).unwrap();
            assert_eq!(decisions(&stored), 1, "round {round}");
            assert_eq!(decisions(&recorder.events()[before..]), 1, "round {round}");
            assert_eq!(log.aggregate().unwrap().state, state);
            drop(log);
            let reopened = EventLog::open(&path).unwrap();
            assert_eq!(reopened.rebuild(&run).unwrap().state, state);
        }
    }

    #[test]
    fn a_decision_while_the_run_deliberates_is_refused() {
        let path = temp_log("early-decision.jsonl");
        let run = RunId::new("run-early");
        let log = shared(&path, &run, Arc::new(NullPublisher));
        log.append_validated(task_event(&run, "e1")).unwrap();
        let err = log
            .append_validated(decision_event(&run, true, "k"))
            .unwrap_err();
        assert!(
            matches!(
                err,
                EventLogError::Replay(TransitionError::InvalidTransition {
                    state: RunState::Created,
                    event: "human_decision"
                })
            ),
            "{err}"
        );
        assert_eq!(log.snapshot_from(0).unwrap().len(), 1);
    }

    /// The reference model, except that the first critique call waits
    /// until the test lets it go, so another writer can act meanwhile.
    struct GatedModel {
        inner: FakeModelClient,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl ModelClient for GatedModel {
        fn propose(&mut self, task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
            self.inner.propose(task)
        }
        fn critique(
            &mut self,
            proposal: &Proposal,
            ctx: &RoleContext,
        ) -> Result<(Challenge, ModelInvocation), ModelError> {
            if self.inner.calls.len() == 1 {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            self.inner.critique(proposal, ctx)
        }
        fn evaluate(
            &mut self,
            proposal: &Proposal,
            challenge: &Challenge,
            ctx: &RoleContext,
        ) -> Result<(Evaluation, ModelInvocation), ModelError> {
            self.inner.evaluate(proposal, challenge, ctx)
        }
        fn revise(
            &mut self,
            proposal: &Proposal,
            challenge: &Challenge,
            evaluation: &Evaluation,
            ctx: &RoleContext,
        ) -> Result<(Revision, ModelInvocation), ModelError> {
            self.inner.revise(proposal, challenge, evaluation, ctx)
        }
    }

    #[test]
    fn a_second_writer_ending_the_run_mid_deliberation_never_breaks_replay() {
        let path = temp_log("second-writer.jsonl");
        let run = RunId::new("run-second-writer");
        let recorder = Arc::new(Recorder::default());
        let log = shared(&path, &run, recorder.clone());
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();

        let mut deliberating = log.clone();
        let worker = std::thread::spawn(move || {
            let s = retry_constraint_scenario();
            let model = GatedModel {
                inner: s.model,
                entered: entered_tx,
                release: release_rx,
            };
            let mut orch = orchestrator(model, s.memory);
            let run_id = deliberating.run_id().clone();
            let (agg, task) = orch
                .create_task(&mut deliberating, run_id, s.input)
                .unwrap();
            orch.deliberate(&mut deliberating, agg, &task, &RunControl::new())
        });

        // While the critique call is in flight, a shutdown coordinator
        // ends the run through its own handle.
        entered.recv().unwrap();
        let mut ids = SequentialIdGen::new();
        let shutdown = fail_event(&run, &ids.next("coordinator"), "shutdown");
        log.append_validated(shutdown).unwrap();
        release.send(()).unwrap();

        // The orchestrator's local aggregate still allowed its critique,
        // but the run's stored history no longer does: the append is
        // refused instead of landing after the terminal event.
        let err = worker.join().unwrap().unwrap_err();
        assert!(
            matches!(
                err,
                OrchestratorError::Log(EventLogError::Replay(TransitionError::Terminal {
                    state: RunState::Failed
                }))
            ),
            "{err}"
        );
        let stored = log.snapshot_from(0).unwrap();
        let last = stored.last().unwrap();
        assert!(
            matches!(&last.payload, EventPayload::RunFailed { stage, .. } if stage == "shutdown"),
            "{last:?}"
        );
        assert_eq!(recorder.events(), stored);
        drop(log);
        let reopened = EventLog::open(&path).unwrap();
        let agg = reopened.rebuild(&run).unwrap();
        assert_eq!(agg.state, RunState::Failed);
        assert_eq!(agg.event_count as usize, stored.len());
    }

    /// Ends the run (`RunFailed{shutdown}`) through a second handle as
    /// soon as record `at` is published. Publication runs without the
    /// run's lock, so this is another writer acting between two of the
    /// orchestrator's appends, at a chosen point.
    struct EndAfter {
        at: u64,
        second: OnceLock<SharedRunLog>,
    }

    impl EventPublisher for EndAfter {
        fn publish(&self, event: &ConsensusEvent) {
            if event.sequence == self.at {
                let second = self.second.get().unwrap();
                let run = second.run_id().clone();
                second
                    .append_validated(fail_event(&run, "coordinator", "shutdown"))
                    .unwrap();
            }
        }
    }

    #[test]
    fn a_writer_ending_the_run_at_any_emit_point_never_breaks_replay() {
        let run = RunId::new("run-every-point");
        let records = {
            let path = temp_log("every-point-reference.jsonl");
            let mut log = shared(&path, &run, Arc::new(NullPublisher));
            deliberate_to_awaiting(&mut log).event_count
        };
        for at in 1..=records {
            let path = temp_log(&format!("every-point-{at}.jsonl"));
            let ender = Arc::new(EndAfter {
                at,
                second: OnceLock::new(),
            });
            let mut log = shared(&path, &run, ender.clone());
            // The second writer shares the run's log and lock, not its
            // publisher (which would make a reference cycle).
            let second = SharedRunLog {
                run_id: run.clone(),
                log: log.log.clone(),
                publisher: Arc::new(NullPublisher),
                lease: None,
                publish_failures: Arc::default(),
            };
            ender.second.set(second).unwrap();

            let s = retry_constraint_scenario();
            let mut orch = orchestrator(s.model, s.memory);
            let (agg, task) = orch.create_task(&mut log, run.clone(), s.input).unwrap();
            match orch.deliberate(&mut log, agg, &task, &RunControl::new()) {
                // Only the last record leaves nothing for the orchestrator
                // to append afterwards.
                Ok(agg) => assert_eq!(at, records, "at {at}: {:?}", agg.state),
                Err(err) => assert!(
                    matches!(
                        err,
                        OrchestratorError::Log(EventLogError::Replay(TransitionError::Terminal {
                            state: RunState::Failed
                        }))
                    ),
                    "at {at}: {err}"
                ),
            }

            // Exactly one terminal record, last, right after record `at`;
            // the orchestrator's next append was refused, not stored.
            let stored = log.snapshot_from(0).unwrap();
            assert_eq!(stored.len() as u64, at + 1, "at {at}");
            let failed: Vec<u64> = stored
                .iter()
                .filter(|e| matches!(e.payload, EventPayload::RunFailed { .. }))
                .map(|e| e.sequence)
                .collect();
            assert_eq!(failed, [at + 1], "at {at}");
            let reopened = EventLog::open(&path).unwrap();
            assert_eq!(reopened.all_events(), stored, "at {at}");
            assert_eq!(reopened.rebuild(&run).unwrap().state, RunState::Failed);
        }
    }

    #[test]
    fn a_poisoned_lock_is_reported_as_an_error_never_unwrapped() {
        let path = temp_log("poisoned.jsonl");
        let run = RunId::new("run-poisoned");
        let log = shared(&path, &run, Arc::new(NullPublisher));
        log.append_validated(task_event(&run, "e1")).unwrap();
        assert!(!log.is_poisoned());

        let holder = log.clone();
        let panicked = std::thread::spawn(move || {
            let _guard = holder.log.lock().unwrap();
            panic!("a writer dies while holding the run lock");
        })
        .join();
        assert!(panicked.is_err());

        assert!(log.is_poisoned());
        let errors = [
            log.append_validated(fail_event(&run, "e2", "propose"))
                .unwrap_err(),
            log.snapshot_from(0).unwrap_err(),
            log.aggregate().unwrap_err(),
            log.read(|l| l.len()).unwrap_err(),
        ];
        for err in errors {
            assert!(
                matches!(&err, EventLogError::Io(e) if e.to_string().contains("poisoned")),
                "{err}"
            );
        }
        // The failed append wrote nothing.
        drop(log);
        assert_eq!(EventLog::open(&path).unwrap().len(), 1);
    }

    #[test]
    fn snapshot_from_returns_the_records_after_a_sequence() {
        let path = temp_log("snapshot.jsonl");
        let mut log = shared(&path, &RunId::new("run-snapshot"), Arc::new(NullPublisher));
        let agg = deliberate_to_awaiting(&mut log);
        let all = log.snapshot_from(0).unwrap();
        assert_eq!(all.len() as u64, agg.event_count);
        for after in 0..=agg.event_count + 2 {
            let tail = log.snapshot_from(after).unwrap();
            let expected: Vec<u64> = (after + 1..=agg.event_count).collect();
            let got: Vec<u64> = tail.iter().map(|e| e.sequence).collect();
            assert_eq!(got, expected, "after {after}");
        }
        assert_eq!(
            log.read(|l| l.head_hash().map(str::to_owned)).unwrap(),
            Some(all.last().unwrap().hash.clone())
        );
    }

    /// Starts recording only once `subscribe` is set, like a broadcast
    /// receiver created mid-run.
    #[derive(Default)]
    struct LateSubscriber {
        subscribed: std::sync::atomic::AtomicBool,
        seen: Mutex<Vec<u64>>,
    }

    impl EventPublisher for LateSubscriber {
        fn publish(&self, event: &ConsensusEvent) {
            if self.subscribed.load(std::sync::atomic::Ordering::SeqCst) {
                self.seen.lock().unwrap().push(event.sequence);
            }
        }
    }

    #[test]
    fn subscribing_first_then_snapshotting_misses_nothing_while_appends_race() {
        for round in 0..8 {
            let path = temp_log(&format!("resume-{round}.jsonl"));
            let run = RunId::new(format!("run-resume-{round}"));
            let subscriber = Arc::new(LateSubscriber::default());
            let log = shared(&path, &run, subscriber.clone());
            log.append_validated(task_event(&run, "e0")).unwrap();

            // Two writers keep appending while a client joins.
            let writers: Vec<_> = (0..2)
                .map(|w| {
                    let log = log.clone();
                    let run = run.clone();
                    std::thread::spawn(move || {
                        for i in 0..20 {
                            let event = NewEvent {
                                event_id: EventId::new(format!("w{w}-{i}")),
                                run_id: run.clone(),
                                actor: Actor::Runtime,
                                occurred_at: TS.into(),
                                payload: EventPayload::EvidenceUnavailable {
                                    query: "q".into(),
                                    reason: "offline".into(),
                                    code: None,
                                },
                            };
                            // Evidence events need a proposal first; until
                            // one exists they are refused, which is fine:
                            // only stored records count.
                            let _ = log.append_validated(event);
                        }
                    })
                })
                .collect();
            let proposer = {
                let log = log.clone();
                let run = run.clone();
                std::thread::spawn(move || {
                    let s = retry_constraint_scenario();
                    let mut model = s.model;
                    let task = log.aggregate().unwrap().task.unwrap();
                    let (proposal, invocation) = model.propose(&task).unwrap();
                    log.append_validated(NewEvent {
                        event_id: EventId::new("proposal"),
                        run_id: run,
                        actor: Actor::Agent {
                            role: AgentRole::Proposer,
                        },
                        occurred_at: TS.into(),
                        payload: EventPayload::ProposalSubmitted {
                            proposal,
                            invocation,
                        },
                    })
                    .unwrap();
                })
            };

            // Join: subscribe, then replay what is already durable.
            std::thread::yield_now();
            subscriber
                .subscribed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let replayed: Vec<u64> = log
                .snapshot_from(0)
                .unwrap()
                .iter()
                .map(|e| e.sequence)
                .collect();
            for t in writers {
                t.join().unwrap();
            }
            proposer.join().unwrap();

            // Dedupe by sequence, as an SSE client does.
            let mut merged: Vec<u64> = replayed;
            merged.extend(subscriber.seen.lock().unwrap().iter().copied());
            merged.sort_unstable();
            merged.dedup();
            let head = log.read(|l| l.len() as u64).unwrap();
            let expected: Vec<u64> = (1..=head).collect();
            assert_eq!(merged, expected, "round {round}: no gap, no extra");
        }
    }

    #[test]
    fn new_refuses_a_log_holding_another_runs_events() {
        let path = temp_log("mixed.jsonl");
        let mut raw = EventLog::open(&path).unwrap();
        raw.append(task_event(&RunId::new("run-a"), "e1")).unwrap();
        raw.append(task_event(&RunId::new("run-b"), "e2")).unwrap();
        drop(raw);
        let err = SharedRunLog::new(
            RunId::new("run-a"),
            EventLog::open(&path).unwrap(),
            Arc::new(NullPublisher),
        )
        .unwrap_err();
        assert!(
            matches!(err, EventLogError::Replay(TransitionError::RunMismatch)),
            "{err}"
        );
    }

    #[test]
    fn a_task_naming_another_run_is_neither_stored_nor_published() {
        let path = temp_log("foreign-task.jsonl");
        let run = RunId::new("run-foreign-task");
        let recorder = Arc::new(Recorder::default());
        let log = shared(&path, &run, recorder.clone());
        // The record names this run; the task inside it names another,
        // which the state machine alone does not check.
        let mut foreign = task_event(&run, "e1");
        if let EventPayload::TaskCreated { task } = &mut foreign.payload {
            task.run_id = RunId::new("run-elsewhere");
        }
        let err = log.append_validated(foreign.clone()).unwrap_err();
        assert!(
            matches!(err, EventLogError::Replay(TransitionError::RunMismatch)),
            "{err}"
        );
        assert!(log.snapshot_from(0).unwrap().is_empty());
        assert!(recorder.events().is_empty(), "nothing published");
        drop(log);
        assert!(std::fs::read(&path).unwrap().is_empty(), "nothing written");
        // A log that already holds such a task is refused when shared.
        let mut raw = EventLog::open(&path).unwrap();
        raw.append(foreign).unwrap();
        drop(raw);
        let err = SharedRunLog::new(run, EventLog::open(&path).unwrap(), Arc::new(NullPublisher))
            .unwrap_err();
        assert!(
            matches!(err, EventLogError::Replay(TransitionError::RunMismatch)),
            "{err}"
        );
    }

    /// Panics on the record with sequence `at`, as a buggy subscriber
    /// would, and records the others.
    struct PanicsAt {
        at: u64,
        seen: Recorder,
    }

    impl EventPublisher for PanicsAt {
        fn publish(&self, event: &ConsensusEvent) {
            if event.sequence == self.at {
                panic!("subscriber bug at record {}", event.sequence);
            }
            self.seen.publish(event);
        }
    }

    #[test]
    fn a_panicking_publisher_never_unwinds_into_the_writer() {
        let path = temp_log("panicking-publisher.jsonl");
        let publisher = Arc::new(PanicsAt {
            at: 3,
            seen: Recorder::default(),
        });
        let mut log = shared(&path, &RunId::new("run-panicking"), publisher.clone());
        // The deliberation runs to its end although one publication
        // panicked in the orchestrator's own thread.
        let agg = deliberate_to_awaiting(&mut log);
        assert_eq!(log.publish_failures(), 1);
        assert!(!log.is_poisoned());
        let stored = log.snapshot_from(0).unwrap();
        assert_eq!(stored.len() as u64, agg.event_count);
        // Every record is stored, the lost one included; only its
        // publication is missing.
        let published: Vec<u64> = publisher.seen.events().iter().map(|e| e.sequence).collect();
        let expected: Vec<u64> = (1..=agg.event_count).filter(|seq| *seq != 3).collect();
        assert_eq!(published, expected);
        drop(log);
        assert_eq!(EventLog::open(&path).unwrap().all_events(), stored);
    }

    /// What a server's stream (B3) does with the records a run publishes:
    /// it sends each sequence once and in order, fills any gap from
    /// [`SharedRunLog::snapshot_from`], and ends once, right after the
    /// record that makes the run's durable head terminal.
    struct Stream {
        log: SharedRunLog,
        /// The run as replayed from what the stream sent: a frame out of
        /// order would fail to apply.
        run: RunAggregate,
        sent: Vec<u64>,
        ends: u32,
        filled_from_log: u32,
    }

    impl Stream {
        fn on_published(&mut self, event: ConsensusEvent) {
            if self.ends > 0 {
                return;
            }
            let next = self.sent.len() as u64 + 1;
            let frames = match event.sequence {
                seq if seq < next => return,
                seq if seq == next => vec![event],
                _ => {
                    self.filled_from_log += 1;
                    self.log.snapshot_from(next - 1).unwrap()
                }
            };
            for frame in frames {
                self.run.apply(&frame).unwrap();
                self.sent.push(frame.sequence);
                if self.run.state.is_terminal() {
                    self.ends += 1;
                    return;
                }
            }
        }
    }

    /// How long the stream test waits for one step before it fails, so a
    /// regression fails the test instead of hanging it.
    const STEP: std::time::Duration = std::time::Duration::from_secs(30);

    /// Forwards records to a stream, except that the record at `lost`
    /// panics (a subscriber bug, caught by the log) and the record at
    /// `held` waits until the record after it has gone out: the stream
    /// gets those two the other way round.
    struct Reordering {
        lost: u64,
        held: u64,
        to_stream: Mutex<mpsc::Sender<ConsensusEvent>>,
        /// How many records the stream has sent, reported after each one.
        /// The writer is held at the record after the lost one until the
        /// stream has filled that gap, so the fill finds the log ending
        /// there; a stream that lags behind the whole deliberation would
        /// otherwise take the recommendation in the same fill and merge
        /// the two gaps into one.
        caught_up: Mutex<mpsc::Receiver<u64>>,
        holding: Mutex<Option<mpsc::Sender<()>>>,
        released: (Mutex<bool>, Condvar),
    }

    impl EventPublisher for Reordering {
        fn publish(&self, event: &ConsensusEvent) {
            if event.sequence == self.lost {
                panic!("subscriber bug: record {} lost", event.sequence);
            }
            if event.sequence == self.held {
                if let Some(holding) = self.holding.lock().unwrap().take() {
                    holding.send(()).unwrap();
                }
                let (released, changed) = &self.released;
                let released = released.lock().unwrap();
                let (released, _) = changed
                    .wait_timeout_while(released, STEP, |released| !*released)
                    .unwrap();
                assert!(
                    *released,
                    "the record after {} was never published",
                    self.held
                );
            }
            self.to_stream.lock().unwrap().send(event.clone()).unwrap();
            if event.sequence == self.lost + 1 {
                let caught_up = self.caught_up.lock().unwrap();
                loop {
                    let sent = caught_up
                        .recv_timeout(STEP)
                        .expect("the stream never filled the lost record's gap");
                    if sent > self.lost {
                        break;
                    }
                }
            }
            if event.sequence == self.held + 1 {
                let (released, changed) = &self.released;
                *released.lock().unwrap() = true;
                changed.notify_all();
            }
        }
    }

    #[test]
    fn a_stream_dedupes_fills_gaps_from_the_log_and_ends_once_at_the_terminal_head() {
        let run = RunId::new("run-stream");
        // The deliberation's last record is the recommendation; the human
        // decision that follows it ends the run.
        let held = {
            let path = temp_log("stream-reference.jsonl");
            let mut log = shared(&path, &run, Arc::new(NullPublisher));
            deliberate_to_awaiting(&mut log).event_count
        };
        let path = temp_log("stream.jsonl");
        let (to_stream, published) = mpsc::channel();
        let (report, caught_up) = mpsc::channel();
        let (holding, recommendation_held) = mpsc::channel();
        let publisher = Arc::new(Reordering {
            lost: 3,
            held,
            to_stream: Mutex::new(to_stream),
            caught_up: Mutex::new(caught_up),
            holding: Mutex::new(Some(holding)),
            released: (Mutex::new(false), Condvar::new()),
        });
        let log = shared(&path, &run, publisher.clone());

        // The stream reads the durable log through a handle without a
        // publisher, so the channel closes once the run's writers are gone.
        let stream = {
            let log = SharedRunLog {
                run_id: run.clone(),
                log: log.log.clone(),
                publisher: Arc::new(NullPublisher),
                lease: None,
                publish_failures: Arc::default(),
            };
            let run = run.clone();
            std::thread::spawn(move || {
                let mut stream = Stream {
                    log,
                    run: RunAggregate::new(run),
                    sent: Vec::new(),
                    ends: 0,
                    filled_from_log: 0,
                };
                for event in published {
                    stream.on_published(event);
                    // The publisher is gone once the run's writers are.
                    let _ = report.send(stream.sent.len() as u64);
                }
                stream
            })
        };
        let deliberation = {
            let mut writer = log.clone();
            std::thread::spawn(move || deliberate_to_awaiting(&mut writer))
        };
        // The recommendation is stored and its publication held back; the
        // decision is recorded and published first.
        recommendation_held
            .recv_timeout(STEP)
            .expect("the deliberation never reached its recommendation");
        let decision = log
            .append_validated(decision_event(&run, true, "approve"))
            .unwrap();
        assert_eq!(decision.sequence, held + 1);
        deliberation.join().unwrap();
        assert_eq!(log.publish_failures(), 1, "the lost record's panic");
        drop(log);
        drop(publisher);

        let stream = stream.join().unwrap();
        let expected: Vec<u64> = (1..=held + 1).collect();
        assert_eq!(stream.sent, expected, "every record once, in order");
        assert_eq!(stream.ends, 1, "the stream ended once");
        assert_eq!(stream.run.state, RunState::Approved);
        // Two gaps, each filled once: the lost record (the writer waited
        // for that fill) and the decision before the recommendation.
        assert_eq!(stream.filled_from_log, 2);
    }

    /// Windows locks are mandatory: while another handle holds the whole
    /// log file locked, the log's write fails as a full disk or a failing
    /// device would. A fault seam that needs no hook in the event log.
    #[cfg(windows)]
    #[test]
    fn an_append_the_disk_refuses_is_neither_stored_nor_published() {
        let path = temp_log("disk-refuses.jsonl");
        let run = RunId::new("run-disk-refuses");
        let recorder = Arc::new(Recorder::default());
        let log = shared(&path, &run, recorder.clone());
        let task = log.append_validated(task_event(&run, "e1")).unwrap();
        let bytes = std::fs::read(&path).unwrap();

        let blocker = std::fs::File::open(&path).unwrap();
        blocker.lock().unwrap();
        let err = log
            .append_validated(fail_event(&run, "e2", "propose"))
            .unwrap_err();
        blocker.unlock().unwrap();
        drop(blocker);
        assert!(matches!(err, EventLogError::Io(_)), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        assert_eq!(log.snapshot_from(0).unwrap(), vec![task.clone()]);
        assert_eq!(recorder.events(), vec![task.clone()], "nothing published");

        // Once the disk takes writes again, the same event is stored and
        // published once.
        let ended = log
            .append_validated(fail_event(&run, "e2", "propose"))
            .unwrap();
        assert_eq!(ended.sequence, 2);
        assert_eq!(recorder.events(), [task, ended]);
        drop(log);
        assert_eq!(EventLog::open(&path).unwrap().len(), 2);
    }
}
