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
//! 4. clone the stored record and release the lock;
//! 5. publish the record to the run's [`EventPublisher`].
//!
//! A refused or failed append is therefore never published, and every
//! published record is already on disk. The publisher runs without the
//! lock, so a slow or panicking subscriber can neither stall nor poison
//! the run's writers. The price: two clones appending at the same moment
//! may publish out of sequence order. Subscribers dedupe by sequence and
//! fill a gap from [`SharedRunLog::snapshot_from`], which reads the
//! durable log; one that subscribes first and then takes a snapshot
//! misses nothing.
//!
//! The runtime crate stays free of an async runtime: [`EventPublisher`]
//! is a plain trait, which the server implements over its per-run
//! broadcast channels.

use crate::event_log::{EventLog, EventLogError};
use crate::run_store::RunLease;
use crate::sink::EventSink;
use crate::state_machine::{RunAggregate, TransitionError};
use consensus_domain::{ConsensusEvent, NewEvent, RunId};
use std::sync::{Arc, Mutex, MutexGuard};

/// Receives every record a [`SharedRunLog`] stores, once it is durable.
pub trait EventPublisher: Send + Sync {
    /// Called once per newly stored record, after its fsync and without
    /// the run's lock held. A retried event id that is already stored
    /// publishes nothing. Should not block: a server forwards to a
    /// bounded channel and lets a lagging subscriber reconcile from the
    /// log.
    fn publish(&self, event: &ConsensusEvent);
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
    /// file) and replay cleanly.
    ///
    /// No ownership lock is taken here, so nothing stops a second
    /// [`EventLog`] over the same file, in this process or another, from
    /// writing beside it. Runs kept in a
    /// [`RunStore`](crate::run_store::RunStore) are shared through
    /// [`RunHandle::into_shared`](crate::run_store::RunHandle::into_shared),
    /// which holds the run's exclusive lock and indexes its terminal head.
    pub fn new(
        run_id: RunId,
        log: EventLog,
        publisher: Arc<dyn EventPublisher>,
    ) -> Result<Self, EventLogError> {
        if log.all_events().iter().any(|e| e.run_id != run_id) {
            return Err(TransitionError::RunMismatch.into());
        }
        log.rebuild(&run_id)?;
        Ok(Self {
            run_id,
            log: Arc::new(Mutex::new(log)),
            publisher,
            lease: None,
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
        }
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// Appends `new` if the run's stored history allows it, then
    /// publishes the stored record (see the module docs for the steps).
    ///
    /// - An event for another run is refused (`RunMismatch`) before the
    ///   lock is taken.
    /// - An `event_id` that is already stored returns the stored record,
    ///   as [`EventSink`] requires. It is neither validated again (a
    ///   retried decision must not fail because its first attempt ended
    ///   the run) nor published again.
    /// - A transition the stored history refuses is
    ///   [`EventLogError::Replay`]; nothing is written or published.
    /// - When the record ends the run and the run belongs to a store, its
    ///   head is recorded in the run index before the record is
    ///   published.
    pub fn append_validated(&self, new: NewEvent) -> Result<ConsensusEvent, EventLogError> {
        if new.run_id != self.run_id {
            return Err(TransitionError::RunMismatch.into());
        }
        let (stored, terminal) = {
            let mut log = self.lock()?;
            if log.contains(&new.event_id) {
                return EventSink::append(&mut *log, new);
            }
            // Validate against the history on disk, not against a copy a
            // writer kept: another clone may have appended since.
            let mut probe = log.rebuild(&self.run_id)?;
            probe.apply(&new.clone().into_sequenced(0))?;
            let stored = EventSink::append(&mut *log, new)?;
            (stored, probe.state.is_terminal())
        };
        if terminal {
            if let Some(lease) = &self.lease {
                lease.record_terminal(&stored);
            }
        }
        self.publisher.publish(&stored);
        Ok(stored)
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
    use std::sync::{Barrier, OnceLock};

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
}
