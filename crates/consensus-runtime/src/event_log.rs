//! Append-only JSONL event log with crash-safe recovery.
//!
//! Guarantees:
//! - Sequences are gap-free, starting at 1, validated on open.
//! - Each append is flushed and fsync'd before it is acknowledged.
//! - A truncated final record (crash mid-write) is detected on open,
//!   removed, and reported via `recovered_truncated_tail`; all complete
//!   records before it remain valid.
//! - Corruption anywhere except the tail is a hard error, never guessed.
//! - Appending an event whose `event_id` already exists is an idempotent
//!   no-op returning the original sequence (safe retries).
//! - Replay is read-only and deterministic; it performs no external
//!   effects and cannot re-trigger authorization or execution.

use consensus_domain::{ConsensusEvent, EventId, NewEvent, RunId, SCHEMA_VERSION};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::state_machine::{RunAggregate, TransitionError};

#[derive(Debug, Error)]
pub enum EventLogError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt event log at line {line}: {detail}")]
    Corrupt { line: usize, detail: String },
    #[error("sequence gap: expected {expected}, found {found} at line {line}")]
    SequenceGap {
        line: usize,
        expected: u64,
        found: u64,
    },
    #[error("duplicate event_id in stored log at line {line}")]
    DuplicateStoredId { line: usize },
    #[error("unsupported schema_version {found:?} at line {line} (expected {SCHEMA_VERSION:?})")]
    SchemaVersion { line: usize, found: String },
    #[error("replay transition failed: {0}")]
    Replay(#[from] TransitionError),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    file: File,
    events: Vec<ConsensusEvent>,
    by_id: HashMap<EventId, u64>,
    /// True when open() discarded an incomplete final record.
    pub recovered_truncated_tail: bool,
}

impl EventLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EventLogError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        let mut events: Vec<ConsensusEvent> = Vec::new();
        let mut by_id: HashMap<EventId, u64> = HashMap::new();
        let mut recovered = false;

        if !bytes.is_empty() {
            let ends_with_newline = bytes.last() == Some(&b'\n');
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.split('\n').collect();
            // Byte offset of each line start, for tail truncation.
            let mut offset = 0usize;
            let mut valid_len = 0usize;
            for (idx, line) in lines.iter().enumerate() {
                let line_no = idx + 1;
                let trimmed = line.trim_end_matches('\r');
                let is_last_chunk = idx == lines.len() - 1;
                if trimmed.is_empty() {
                    if is_last_chunk {
                        valid_len = offset;
                        break;
                    }
                    return Err(EventLogError::Corrupt {
                        line: line_no,
                        detail: "empty line inside log".into(),
                    });
                }
                match serde_json::from_str::<ConsensusEvent>(trimmed) {
                    Ok(ev) => {
                        if ev.schema_version != SCHEMA_VERSION {
                            return Err(EventLogError::SchemaVersion {
                                line: line_no,
                                found: ev.schema_version.clone(),
                            });
                        }
                        let expected = events.len() as u64 + 1;
                        if ev.sequence != expected {
                            return Err(EventLogError::SequenceGap {
                                line: line_no,
                                expected,
                                found: ev.sequence,
                            });
                        }
                        if by_id.insert(ev.event_id.clone(), ev.sequence).is_some() {
                            return Err(EventLogError::DuplicateStoredId { line: line_no });
                        }
                        events.push(ev);
                        offset += line.len() + 1;
                        valid_len = offset;
                    }
                    Err(e) => {
                        // Only an unparseable, non-newline-terminated final
                        // chunk is a recoverable truncated write.
                        if is_last_chunk && !ends_with_newline {
                            recovered = true;
                            break;
                        }
                        return Err(EventLogError::Corrupt {
                            line: line_no,
                            detail: e.to_string(),
                        });
                    }
                }
            }
            if recovered {
                file.set_len(valid_len as u64)?;
                file.sync_all()?;
            } else if !ends_with_newline {
                // Complete final record missing its terminator: repair so
                // the next append starts on a fresh line.
                file.seek(SeekFrom::End(0))?;
                file.write_all(b"\n")?;
                file.sync_all()?;
            }
        }
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            path,
            file,
            events,
            by_id,
            recovered_truncated_tail: recovered,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn all_events(&self) -> &[ConsensusEvent] {
        &self.events
    }

    pub fn contains(&self, event_id: &EventId) -> bool {
        self.by_id.contains_key(event_id)
    }

    pub fn events_for<'a>(
        &'a self,
        run_id: &'a RunId,
    ) -> impl Iterator<Item = &'a ConsensusEvent> + 'a {
        self.events.iter().filter(move |e| &e.run_id == run_id)
    }

    /// Append one event. Idempotent on `event_id`. Returns the sequence.
    pub fn append(&mut self, new: NewEvent) -> Result<u64, EventLogError> {
        if let Some(seq) = self.by_id.get(&new.event_id) {
            return Ok(*seq);
        }
        let seq = self.events.len() as u64 + 1;
        let ev = new.into_sequenced(seq);
        let mut line = serde_json::to_string(&ev)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
        self.file.sync_all()?;
        self.by_id.insert(ev.event_id.clone(), seq);
        self.events.push(ev);
        Ok(seq)
    }

    /// Rebuild one run's aggregate by deterministic replay. Read-only.
    pub fn rebuild(&self, run_id: &RunId) -> Result<RunAggregate, EventLogError> {
        let mut agg = RunAggregate::new(run_id.clone());
        for ev in self.events_for(run_id) {
            agg.apply(ev)?;
        }
        Ok(agg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consensus_domain::*;

    fn temp_log(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyphae-consensus-tests-{}", std::process::id()));
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
            occurred_at: "2026-10-26T00:00:00Z".into(),
            payload: EventPayload::TaskCreated {
                task: Task {
                    schema_version: SCHEMA_VERSION.into(),
                    id: TaskId::new("task-1"),
                    run_id: run.clone(),
                    project_id: ProjectId::new("p"),
                    title: "t".into(),
                    description: "d".into(),
                    created_at: "2026-10-26T00:00:00Z".into(),
                },
            },
        }
    }

    fn fail_event(run: &RunId, id: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Runtime,
            occurred_at: "2026-10-26T00:00:00Z".into(),
            payload: EventPayload::RunFailed {
                reason: "boom".into(),
                stage: "propose".into(),
            },
        }
    }

    #[test]
    fn append_reopen_replay_round_trip() {
        let path = temp_log("roundtrip.jsonl");
        let run = RunId::new("run-1");
        {
            let mut log = EventLog::open(&path).unwrap();
            assert_eq!(log.append(task_event(&run, "e1")).unwrap(), 1);
            assert_eq!(log.append(fail_event(&run, "e2")).unwrap(), 2);
        }
        let log = EventLog::open(&path).unwrap();
        assert_eq!(log.len(), 2);
        assert!(!log.recovered_truncated_tail);
        let agg = log.rebuild(&run).unwrap();
        assert_eq!(agg.state, RunState::Failed);
        assert_eq!(agg.failure_reason.as_deref(), Some("boom"));
    }

    #[test]
    fn duplicate_event_id_is_idempotent() {
        let path = temp_log("idempotent.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();
        let first = log.append(task_event(&run, "dup")).unwrap();
        let second = log.append(task_event(&run, "dup")).unwrap();
        assert_eq!(first, second);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn truncated_tail_is_recovered_and_append_continues() {
        let path = temp_log("truncated.jsonl");
        let run = RunId::new("run-1");
        {
            let mut log = EventLog::open(&path).unwrap();
            log.append(task_event(&run, "e1")).unwrap();
        }
        // Simulate a crash mid-write: partial JSON, no newline.
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(b"{\"schema_version\":\"1.0\",\"sequence\":2,\"eve")
                .unwrap();
        }
        let mut log = EventLog::open(&path).unwrap();
        assert!(log.recovered_truncated_tail);
        assert_eq!(log.len(), 1);
        assert_eq!(log.append(fail_event(&run, "e2")).unwrap(), 2);
        let agg = log.rebuild(&run).unwrap();
        assert_eq!(agg.state, RunState::Failed);
    }

    #[test]
    fn corrupt_middle_line_is_hard_error() {
        let path = temp_log("corrupt.jsonl");
        let run = RunId::new("run-1");
        let good = {
            let mut log = EventLog::open(&path).unwrap();
            log.append(task_event(&run, "e1")).unwrap();
            log.append(fail_event(&run, "e2")).unwrap();
            std::fs::read_to_string(&path).unwrap()
        };
        let mut lines: Vec<&str> = good.split('\n').collect();
        lines[0] = "not json at all";
        std::fs::write(&path, lines.join("\n")).unwrap();
        match EventLog::open(&path) {
            Err(EventLogError::Corrupt { line: 1, .. }) => {}
            other => panic!("expected corrupt-line error, got {other:?}"),
        }
    }

    #[test]
    fn sequence_gap_is_detected() {
        let path = temp_log("gap.jsonl");
        let run = RunId::new("run-1");
        let good = {
            let mut log = EventLog::open(&path).unwrap();
            log.append(task_event(&run, "e1")).unwrap();
            log.append(fail_event(&run, "e2")).unwrap();
            std::fs::read_to_string(&path).unwrap()
        };
        let tampered = good.replacen("\"sequence\":2", "\"sequence\":7", 1);
        assert_ne!(good, tampered);
        std::fs::write(&path, tampered).unwrap();
        match EventLog::open(&path) {
            Err(EventLogError::SequenceGap {
                expected: 2,
                found: 7,
                ..
            }) => {}
            other => panic!("expected sequence-gap error, got {other:?}"),
        }
    }
}
