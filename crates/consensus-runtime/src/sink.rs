// SPDX-License-Identifier: Apache-2.0
//! Where the orchestrator writes a run's events.
//!
//! An [`EventSink`] appends one event and returns the record exactly as
//! stored, with its sequence and chain hash. The orchestrator applies
//! that returned record to its aggregate, so a different store (one file
//! per run, a shared writer that also publishes) plugs in without
//! changing orchestration. [`EventLog`] is the reference implementation.

use crate::event_log::{EventLog, EventLogError};
use consensus_domain::{ConsensusEvent, NewEvent};

/// Append-only destination for events.
pub trait EventSink {
    /// Appends `new` and returns the stored record: sequenced, chained
    /// and hashed. An `event_id` that is already stored is not written
    /// again; the record stored under it is returned instead.
    fn append(&mut self, new: NewEvent) -> Result<ConsensusEvent, EventLogError>;
}

impl EventSink for EventLog {
    fn append(&mut self, new: NewEvent) -> Result<ConsensusEvent, EventLogError> {
        let event_id = new.event_id.clone();
        // The inherent append: fsync'd, idempotent on event_id, and it
        // returns the record's sequence.
        let seq = EventLog::append(self, new)?;
        // Sequences start at 1 with no gaps, so record `seq` is at index
        // `seq - 1`.
        seq.checked_sub(1)
            .and_then(|index| self.all_events().get(index as usize))
            .filter(|stored| stored.sequence == seq && stored.event_id == event_id)
            .cloned()
            .ok_or_else(|| EventLogError::Corrupt {
                line: seq as usize,
                detail: format!("appended event {event_id} is not stored at sequence {seq}"),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consensus_domain::*;
    use std::path::PathBuf;

    fn temp_log(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyphae-consensus-sink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    fn task_event(run: &RunId, id: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Human { id: "mario".into() },
            occurred_at: "2026-10-27T00:00:00Z".into(),
            payload: EventPayload::TaskCreated {
                task: Task {
                    schema_version: SCHEMA_VERSION.into(),
                    id: TaskId::new("task-1"),
                    run_id: run.clone(),
                    project_id: ProjectId::new("p"),
                    title: "t".into(),
                    description: "d".into(),
                    created_at: "2026-10-27T00:00:00Z".into(),
                },
            },
        }
    }

    fn fail_event(run: &RunId, id: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Runtime,
            occurred_at: "2026-10-27T00:00:00Z".into(),
            payload: EventPayload::RunFailed {
                reason: "boom".into(),
                stage: "propose".into(),
            },
        }
    }

    #[test]
    fn event_log_sink_returns_the_stored_sequenced_chained_record() {
        let path = temp_log("chain.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();

        let first = EventSink::append(&mut log, task_event(&run, "e1")).unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(first.event_id, EventId::new("e1"));
        assert_eq!(first.prev_hash, "genesis");
        assert_eq!(first.hash.len(), 64);

        let second = EventSink::append(&mut log, fail_event(&run, "e2")).unwrap();
        assert_eq!(second.sequence, 2);
        assert_eq!(second.prev_hash, first.hash);
        assert_eq!(log.head_hash(), Some(second.hash.as_str()));
        assert_eq!(log.all_events(), [first.clone(), second.clone()]);

        // Reopening recomputes every hash: the returned records are the
        // verified chain, not copies made before the write.
        drop(log);
        let reopened = EventLog::open(&path).unwrap();
        assert_eq!(reopened.all_events(), [first, second]);
    }

    #[test]
    fn event_log_sink_retry_returns_the_original_record_without_writing() {
        let path = temp_log("idempotent.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();
        let original = EventSink::append(&mut log, task_event(&run, "e1")).unwrap();
        let head = EventSink::append(&mut log, fail_event(&run, "e2")).unwrap();
        let bytes = std::fs::read(&path).unwrap();

        // A retry of the first event, after a later one: the record stored
        // first comes back, not the chain head.
        let retried = EventSink::append(&mut log, task_event(&run, "e1")).unwrap();
        assert_eq!(retried, original);
        assert_eq!(log.len(), 2);
        assert_eq!(log.head_hash(), Some(head.hash.as_str()));
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
    }
}
