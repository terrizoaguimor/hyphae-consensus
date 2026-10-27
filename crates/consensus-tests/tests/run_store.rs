// SPDX-License-Identifier: Apache-2.0
//! The run store end to end, through the public API only: concurrent
//! orchestrated runs, each in its own file; a decision race on a stored
//! run; and a deliberation whose thread died, closed when the store
//! starts again.

use consensus_domain::*;
use consensus_runtime::fakes::FakeModelClient;
use consensus_runtime::ports::{ModelClient, ModelError, RoleContext};
use consensus_runtime::scenarios::{retry_constraint_scenario, TS};
use consensus_runtime::{
    EventLog, EventLogError, EventPublisher, FixedClock, IndexKind, NullPublisher, Orchestrator,
    Owner, Retention, RunAggregate, RunControl, RunLimits, RunStore, SequentialIdGen, SharedRunLog,
    TransitionError, UuidIdGen,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};

/// A fresh, empty directory, removed again on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-run-store-it-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_store(root: &TempDir) -> RunStore {
    RunStore::open_with_clock(root.path(), Box::new(FixedClock::new(TS))).unwrap()
}

/// Records every published record.
#[derive(Default)]
struct Recorder(Mutex<Vec<ConsensusEvent>>);

impl EventPublisher for Recorder {
    fn publish(&self, event: &ConsensusEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

/// The reference scenario, created and deliberated through `log` to its
/// human decision point.
fn deliberate(log: &mut SharedRunLog) -> RunAggregate {
    let s = retry_constraint_scenario();
    let mut orch = Orchestrator {
        model: s.model,
        memory: s.memory,
        verifier: s.verifier,
        clock: FixedClock::new(TS),
        ids: SequentialIdGen::new(),
        limits: RunLimits::default(),
    };
    let run_id = log.run_id().clone();
    let (agg, task) = orch.create_task(log, run_id, s.input).unwrap();
    orch.deliberate(log, agg, &task, &RunControl::new())
        .unwrap()
}

fn decision(run_id: &RunId, approved: bool) -> NewEvent {
    let key = if approved { "approve" } else { "reject" };
    NewEvent {
        event_id: EventId::new(format!("human-decision:{run_id}:{key}")),
        run_id: run_id.clone(),
        actor: Actor::Human { id: "mario".into() },
        occurred_at: TS.into(),
        payload: EventPayload::HumanDecisionRecorded {
            authorization: HumanAuthorization {
                schema_version: SCHEMA_VERSION.into(),
                run_id: run_id.clone(),
                authorized_by: "mario".into(),
                approved,
                decided_at: TS.into(),
                idempotency_key: key.into(),
                note: None,
            },
        },
    }
}

#[test]
fn concurrent_runs_stay_isolated_ordered_and_gap_free() {
    const RUNS: usize = 8;
    let root = TempDir::new("concurrent");
    let store = Arc::new(open_store(&root));
    let start = Arc::new(Barrier::new(RUNS));
    let workers: Vec<_> = (0..RUNS)
        .map(|_| {
            let store = store.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
                let run_id = run.run_id().clone();
                let recorder = Arc::new(Recorder::default());
                let mut log = run.into_shared(recorder.clone());
                let agg = deliberate(&mut log);
                (run_id, agg, recorder, log)
            })
        })
        .collect();
    let runs: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();

    let distinct: BTreeSet<RunId> = runs.iter().map(|(run_id, ..)| run_id.clone()).collect();
    assert_eq!(distinct.len(), RUNS);
    let mut first_ids: Option<Vec<EventId>> = None;
    for (run_id, agg, recorder, log) in &runs {
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
        // The run's own file: only its records, sequences 1..n, a chain
        // from genesis that verifies on a fresh open, and a replay equal
        // to what the orchestrator returned.
        let file = EventLog::open(store.runs_dir().join(format!("{run_id}.jsonl"))).unwrap();
        let events = file.all_events();
        assert!(events.iter().all(|e| &e.run_id == run_id));
        let sequences: Vec<u64> = events.iter().map(|e| e.sequence).collect();
        assert_eq!(sequences, (1..=agg.event_count).collect::<Vec<_>>());
        assert_eq!(events[0].prev_hash, "genesis");
        assert_eq!(file.rebuild(run_id).unwrap(), *agg);
        // Published once each, in sequence order: one writer per run.
        assert_eq!(*recorder.0.lock().unwrap(), events);
        assert_eq!(log.snapshot_from(0).unwrap(), events);
        // Every orchestrator counted its ids from 1, so the runs share
        // event ids, and still none was taken for another run's retry.
        let ids: Vec<EventId> = events.iter().map(|e| e.event_id.clone()).collect();
        match &first_ids {
            None => first_ids = Some(ids),
            Some(first) => assert_eq!(first, &ids),
        }
    }

    // One `created` record per run, and the index verifies from scratch.
    drop(runs);
    let records = open_store(&root).index_records().unwrap();
    assert_eq!(records.len(), RUNS);
    assert!(records.iter().all(|r| r.kind == IndexKind::Created));
    let indexed: BTreeSet<RunId> = records.into_iter().map(|r| r.run_id).collect();
    assert_eq!(indexed, distinct);
}

#[test]
fn a_decision_race_on_a_stored_run_records_one_decision_and_one_terminal_head() {
    let root = TempDir::new("decision-race");
    let store = open_store(&root);
    let run = store
        .create_run(Owner::session(b"visitor"), Retention::VISITOR)
        .unwrap();
    let run_id = run.run_id().clone();
    let recorder = Arc::new(Recorder::default());
    let mut log = run.into_shared(recorder.clone());
    assert_eq!(deliberate(&mut log).state, RunState::AwaitingHumanApproval);
    let published = recorder.0.lock().unwrap().len();

    // Two handles on the one run, two different decisions at once.
    let start = Arc::new(Barrier::new(2));
    let racers: Vec<_> = [true, false]
        .into_iter()
        .map(|approved| {
            let handle = log.clone();
            let start = start.clone();
            let event = decision(&run_id, approved);
            std::thread::spawn(move || {
                start.wait();
                handle.append_validated(event)
            })
        })
        .collect();
    let results: Vec<_> = racers.into_iter().map(|r| r.join().unwrap()).collect();
    let recorded: Vec<&ConsensusEvent> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(recorded.len(), 1, "{results:?}");
    let winner = recorded[0].clone();
    // The loser gets a transition error, which an API answers with 409.
    let refused = results.iter().find_map(|r| r.as_ref().err()).unwrap();
    assert!(
        matches!(
            refused,
            EventLogError::Replay(TransitionError::Terminal { .. })
        ),
        "{refused}"
    );

    // One decision stored and published, and exactly one terminal head
    // in the run index: the winner's.
    let decisions = |events: &[ConsensusEvent]| {
        events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::HumanDecisionRecorded { .. }))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        decisions(&log.snapshot_from(0).unwrap()),
        vec![winner.clone()]
    );
    assert_eq!(
        decisions(&recorder.0.lock().unwrap()[published..]),
        vec![winner.clone()]
    );
    let terminal: Vec<_> = store
        .index_records()
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == IndexKind::Terminal)
        .collect();
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0].head_hash.as_deref(), Some(winner.hash.as_str()));

    // After the race the run reopens cleanly against its indexed head.
    drop(log);
    let handle = store.open_run(run_id.as_str()).unwrap();
    assert_eq!(handle.log().head_hash(), Some(winner.hash.as_str()));
}

/// The reference model, except that its first critique panics, as a bug
/// in an adapter would: the deliberating thread dies without recording
/// a terminal event.
struct PanickingModel(FakeModelClient);

impl ModelClient for PanickingModel {
    fn propose(&mut self, task: &Task) -> Result<(Proposal, ModelInvocation), ModelError> {
        self.0.propose(task)
    }
    fn critique(
        &mut self,
        _proposal: &Proposal,
        _ctx: &RoleContext,
    ) -> Result<(Challenge, ModelInvocation), ModelError> {
        panic!("adapter bug during the critique call");
    }
    fn evaluate(
        &mut self,
        proposal: &Proposal,
        challenge: &Challenge,
        ctx: &RoleContext,
    ) -> Result<(Evaluation, ModelInvocation), ModelError> {
        self.0.evaluate(proposal, challenge, ctx)
    }
    fn revise(
        &mut self,
        proposal: &Proposal,
        challenge: &Challenge,
        evaluation: &Evaluation,
        ctx: &RoleContext,
    ) -> Result<(Revision, ModelInvocation), ModelError> {
        self.0.revise(proposal, challenge, evaluation, ctx)
    }
}

#[test]
fn a_run_whose_deliberation_died_is_closed_when_the_store_starts_again() {
    let root = TempDir::new("restart");
    let run_id = {
        let store = open_store(&root);
        let run = store
            .create_run(Owner::session(b"visitor"), Retention::VISITOR)
            .unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        let died = std::thread::spawn(move || {
            let s = retry_constraint_scenario();
            let mut orch = Orchestrator {
                model: PanickingModel(s.model),
                memory: s.memory,
                verifier: s.verifier,
                clock: FixedClock::new(TS),
                ids: SequentialIdGen::new(),
                limits: RunLimits::default(),
            };
            let run_id = log.run_id().clone();
            let (agg, task) = orch.create_task(&mut log, run_id, s.input).unwrap();
            let _ = orch.deliberate(&mut log, agg, &task, &RunControl::new());
        })
        .join();
        assert!(died.is_err(), "the deliberation panicked");
        run_id
    };

    // Unwinding dropped the shared log and released the run's lock, as
    // the OS does for a process that dies. The lock was not held during
    // the model call, so nothing is poisoned and the file is intact.
    let store = open_store(&root);
    let report = store.recover(&mut UuidIdGen).unwrap();
    assert_eq!(report.interrupted, vec![run_id.clone()], "{report:?}");
    assert_eq!(report.terminal_recorded, vec![run_id.clone()]);
    assert!(report.unreplayable.is_empty() && report.inconsistent.is_empty());

    let handle = store.open_run(run_id.as_str()).unwrap();
    let agg = handle.log().rebuild(&run_id).unwrap();
    assert_eq!(agg.state, RunState::Failed);
    assert_eq!(
        agg.failure_reason.as_deref(),
        Some("process restarted before the run completed")
    );
    // What was recorded before the panic is kept: the proposal and its
    // evidence.
    let kinds: Vec<&str> = handle
        .log()
        .all_events()
        .iter()
        .map(|e| match &e.payload {
            EventPayload::TaskCreated { .. } => "task",
            EventPayload::ProposalSubmitted { .. } => "proposal",
            EventPayload::EvidenceRetrieved { .. } => "evidence",
            EventPayload::EvidenceVerified { .. } => "verified",
            EventPayload::RunFailed { .. } => "failed",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds.first(), Some(&"task"));
    assert_eq!(kinds.get(1), Some(&"proposal"));
    assert_eq!(kinds.last(), Some(&"failed"));
    assert!(!kinds.contains(&"other"), "{kinds:?}");
    let head = handle.log().head_hash().map(str::to_owned);
    let terminal = store
        .index_records()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == IndexKind::Terminal)
        .unwrap();
    assert_eq!((terminal.run_id, terminal.head_hash), (run_id, head));
}
