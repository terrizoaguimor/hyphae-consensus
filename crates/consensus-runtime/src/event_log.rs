// SPDX-License-Identifier: Apache-2.0
//! Append-only JSONL event log with crash-safe recovery.
//!
//! Guarantees:
//! - Sequences are gap-free, starting at 1, validated on open.
//! - Each append is flushed and fsync'd before it is acknowledged.
//! - A truncated final record (crash mid-write) is detected on open,
//!   removed, and reported via `recovered_truncated_tail`; all complete
//!   records before it remain valid.
//! - Corruption anywhere except the tail is a hard error, never guessed.
//! - Appending an event whose `event_id` already exists with the same
//!   content (run, actor, `occurred_at`, payload) is an idempotent no-op
//!   returning the original sequence (safe retries). The same id with
//!   different content is refused with `IdempotencyConflict`: it is a
//!   different event, never answered with the first one's sequence.
//! - Replay is read-only and deterministic; it performs no external
//!   effects and cannot re-trigger authorization or execution.
//! - Since schema 1.1, every record carries a SHA-256 hash-chain link
//!   (`prev_hash`/`hash`) validated on open: editing or deleting a past
//!   record is detectable. Pre-1.1 records (empty hashes) form a legacy
//!   prefix that is readable but not chain-verified; the first chained
//!   record anchors to `pre-1.1:<last legacy sequence>`.
//! - A failed append rolls the file back to its previous length, so a
//!   partial write never masquerades as a committed record. If the
//!   rollback fails too, the handle is poisoned: it refuses every later
//!   append (`Poisoned`), because its next record would land after the
//!   partial bytes. Reopening the log recovers them as a torn tail.
//!
//! Three ways to open a log, all validating exactly the same things:
//! - [`EventLog::open`] creates the file when missing and repairs it: it
//!   cuts a torn tail and terminates an unterminated final record.
//! - [`EventLog::open_locked`] is `open` plus an exclusive writer lock
//!   held for the handle's life, so a second writer, in this process or
//!   another, fails fast with `Locked` instead of forking the chain.
//! - [`EventLog::open_read_only`] never creates, truncates, repairs or
//!   appends: a torn tail is an error and the handle refuses appends. Use
//!   it for archived, downloaded or otherwise published logs.

use consensus_domain::{ConsensusEvent, EventId, NewEvent, RunId, READABLE_SCHEMA_VERSIONS};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
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
    #[error("unsupported schema_version {found:?} at line {line}")]
    SchemaVersion { line: usize, found: String },
    #[error("hash chain broken at line {line}: {detail}")]
    HashChain { line: usize, detail: String },
    #[error("replay transition failed: {0}")]
    Replay(#[from] TransitionError),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
    /// [`EventLog::open_read_only`] found an incomplete final record: a
    /// write cut short, or one still in progress. `open` would cut it.
    #[error("incomplete final record at line {line} (torn write, not repaired in read-only mode): {detail}")]
    TornTail { line: usize, detail: String },
    #[error("the event log was opened read-only; appends are refused")]
    ReadOnly,
    #[error("event log {} is locked by another writer (lock file {})", .path.display(), .lock_file.display())]
    Locked { path: PathBuf, lock_file: PathBuf },
    #[error("event log handle is poisoned and refuses appends; reopen the log: {detail}")]
    Poisoned { detail: String },
    #[error("event_id {event_id} is already stored at sequence {sequence} with a different {}", .fields.join(", "))]
    IdempotencyConflict {
        event_id: EventId,
        sequence: u64,
        /// Content fields that differ: `run_id`, `actor`, `occurred_at`,
        /// `payload`.
        fields: Vec<&'static str>,
    },
}

/// Chain anchor for the first record of an empty log.
pub(crate) const GENESIS: &str = "genesis";

/// SHA-256 over (`prev_hash` || canonical JSON of the record with
/// `hash` blanked), lowercase hex. Serde struct serialization is
/// field-order deterministic, so the same record always hashes the
/// same; verification re-serializes the parsed record identically.
pub(crate) fn compute_hash(event: &ConsensusEvent) -> Result<String, serde_json::Error> {
    let mut blanked = event.clone();
    blanked.hash.clear();
    let canonical = serde_json::to_string(&blanked)?;
    let mut hasher = Sha256::new();
    hasher.update(event.prev_hash.as_bytes());
    hasher.update(canonical.as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Expected `prev_hash` for the record after `prev` (None = log start).
pub(crate) fn expected_prev_hash(prev: Option<&ConsensusEvent>) -> String {
    match prev {
        None => GENESIS.to_string(),
        Some(p) if p.hash.is_empty() => format!("pre-1.1:{}", p.sequence),
        Some(p) => p.hash.clone(),
    }
}

/// What precedes a record in its log file, as far as a checker knows.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Predecessor<'a> {
    /// The record is the first in the file.
    Start,
    /// The record right before it in the file.
    Record(&'a ConsensusEvent),
    /// Not at hand (in a multi-run log, another run's record): only the
    /// record's own hash can be checked, not its link.
    Unknown,
}

/// Why `ev` fails its chain check against `prev`, or None when it passes.
/// Empty-hash records are a legacy (pre-1.1) prefix only: schema must
/// be "1.0", `prev_hash` must also be empty, and no chained record may
/// precede them.
pub(crate) fn chain_link_problem(
    prev: Predecessor<'_>,
    ev: &ConsensusEvent,
) -> Result<Option<String>, serde_json::Error> {
    if ev.hash.is_empty() {
        if ev.schema_version != "1.0" {
            return Ok(Some(format!(
                "schema {} record without chain hash",
                ev.schema_version
            )));
        }
        if !ev.prev_hash.is_empty() {
            return Ok(Some("legacy record carries a prev_hash".into()));
        }
        if let Predecessor::Record(p) = prev {
            if !p.hash.is_empty() {
                return Ok(Some("unhashed record after hash-chain start".into()));
            }
        }
        return Ok(None);
    }
    let expected = match prev {
        Predecessor::Start => Some(expected_prev_hash(None)),
        Predecessor::Record(p) => Some(expected_prev_hash(Some(p))),
        Predecessor::Unknown => None,
    };
    if let Some(expected) = expected {
        if ev.prev_hash != expected {
            return Ok(Some(format!(
                "prev_hash mismatch: stored {:?}, expected {:?}",
                ev.prev_hash, expected
            )));
        }
    }
    let computed = compute_hash(ev)?;
    if computed != ev.hash {
        return Ok(Some(format!(
            "hash mismatch: stored {}, computed {}",
            ev.hash, computed
        )));
    }
    Ok(None)
}

/// Validate one record's chain link against the accepted prefix.
fn validate_chain_link(
    prev: Option<&ConsensusEvent>,
    ev: &ConsensusEvent,
    line: usize,
) -> Result<(), EventLogError> {
    let prev = prev.map_or(Predecessor::Start, Predecessor::Record);
    match chain_link_problem(prev, ev)? {
        Some(detail) => Err(EventLogError::HashChain { line, detail }),
        None => Ok(()),
    }
}

/// How a log file ends after its last complete record.
enum Tail {
    /// Empty, or the last record ends with its newline.
    Clean,
    /// The final record is complete but has no newline.
    Unterminated,
    /// The final chunk does not parse and has no newline: a write cut
    /// short. The complete records end at byte `valid_len`.
    Torn {
        line: usize,
        valid_len: u64,
        detail: String,
    },
}

/// The validated records of a log file, and how the file ends.
struct Parsed {
    events: Vec<ConsensusEvent>,
    by_id: HashMap<EventId, u64>,
    tail: Tail,
}

/// Parses and validates every record: schema version, gap-free
/// sequence, unique ids, and the hash chain. Never touches the file.
fn parse(bytes: &[u8]) -> Result<Parsed, EventLogError> {
    let mut events: Vec<ConsensusEvent> = Vec::new();
    let mut by_id: HashMap<EventId, u64> = HashMap::new();
    if bytes.is_empty() {
        return Ok(Parsed {
            events,
            by_id,
            tail: Tail::Clean,
        });
    }
    let ends_with_newline = bytes.last() == Some(&b'\n');
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.split('\n').collect();
    // Byte offset of each line start, for tail truncation.
    let mut offset = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim_end_matches('\r');
        let is_last_chunk = idx == lines.len() - 1;
        if trimmed.is_empty() {
            if is_last_chunk {
                break;
            }
            return Err(EventLogError::Corrupt {
                line: line_no,
                detail: "empty line inside log".into(),
            });
        }
        match serde_json::from_str::<ConsensusEvent>(trimmed) {
            Ok(ev) => {
                if !READABLE_SCHEMA_VERSIONS.contains(&ev.schema_version.as_str()) {
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
                validate_chain_link(events.last(), &ev, line_no)?;
                events.push(ev);
                offset += line.len() + 1;
            }
            Err(e) => {
                // Only an unparseable, non-newline-terminated final
                // chunk is a recoverable truncated write.
                if is_last_chunk && !ends_with_newline {
                    return Ok(Parsed {
                        events,
                        by_id,
                        tail: Tail::Torn {
                            line: line_no,
                            valid_len: offset as u64,
                            detail: e.to_string(),
                        },
                    });
                }
                return Err(EventLogError::Corrupt {
                    line: line_no,
                    detail: e.to_string(),
                });
            }
        }
    }
    let tail = if ends_with_newline {
        Tail::Clean
    } else {
        Tail::Unterminated
    };
    Ok(Parsed {
        events,
        by_id,
        tail,
    })
}

/// Whether a handle may append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    ReadWrite,
    ReadOnly,
}

/// Opens `path` for reading and writing, creating it and its parent
/// directory when missing. Never truncates.
fn open_for_writing(path: &Path) -> Result<File, EventLogError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?)
}

/// The writer lock file of a log: `<file name>.lock` beside its canonical
/// path, so every spelling of the path (relative, `.`/`..`, a symlink)
/// names the same lock.
fn lock_file_for(path: &Path) -> Result<PathBuf, EventLogError> {
    let canonical = std::fs::canonicalize(path)?;
    let mut name = canonical.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    Ok(canonical.with_file_name(name))
}

/// The content fields in which `new` differs from the record already
/// stored under its id.
fn differing_fields(stored: &ConsensusEvent, new: &NewEvent) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if stored.run_id != new.run_id {
        fields.push("run_id");
    }
    if stored.actor != new.actor {
        fields.push("actor");
    }
    if stored.occurred_at != new.occurred_at {
        fields.push("occurred_at");
    }
    if stored.payload != new.payload {
        fields.push("payload");
    }
    fields
}

#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    file: File,
    events: Vec<ConsensusEvent>,
    by_id: HashMap<EventId, u64>,
    /// True when open() discarded an incomplete final record.
    pub recovered_truncated_tail: bool,
    /// Opened with `open_read_only`: every append is refused.
    read_only: bool,
    /// Set when a failed append could not be rolled back: every later
    /// append is refused.
    poisoned: Option<String>,
    /// The lock file `open_locked` holds exclusively; dropping the handle
    /// closes it, which releases the lock.
    lock: Option<File>,
    /// Test-only fault injection: the next append writes this many bytes
    /// of its record, then fails as a mid-record I/O error would.
    #[cfg(test)]
    fail_next_append_after: Option<usize>,
    /// Test-only fault injection: the next rollback fails.
    #[cfg(test)]
    fail_next_rollback: bool,
}

impl EventLog {
    /// Opens the log at `path` for appending, creating it (and its parent
    /// directory) when missing. Validates every record; a torn final
    /// record is cut and reported in `recovered_truncated_tail`, and an
    /// unterminated final record gets its newline.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EventLogError> {
        let path = path.as_ref().to_path_buf();
        let file = open_for_writing(&path)?;
        Self::load(path, file, Access::ReadWrite, None)
    }

    /// [`open`](Self::open), holding an exclusive writer lock for the
    /// handle's life: while it lives, every other `open_locked` of the
    /// same log, from this process or another, fails with
    /// [`EventLogError::Locked`]. The lock is taken before the file is
    /// read or repaired.
    ///
    /// The lock is the std file lock (`flock` on Unix, `LockFileEx` on
    /// Windows) on a lock file beside the log, `<file name>.lock`, never
    /// on the log itself: Windows locks are mandatory, and locking the
    /// log would also stop readers such as
    /// [`open_read_only`](Self::open_read_only). The lock file holds no
    /// data and is left in place when the handle drops (deleting it while
    /// another process is about to lock it could admit two writers). It is
    /// advisory: plain [`open`](Self::open) ignores it, so every writer of
    /// a log must use `open_locked` for the exclusion to hold.
    pub fn open_locked(path: impl AsRef<Path>) -> Result<Self, EventLogError> {
        let path = path.as_ref().to_path_buf();
        let file = open_for_writing(&path)?;
        let lock_file = lock_file_for(&path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_file)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(EventLogError::Locked { path, lock_file });
            }
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        Self::load(path, file, Access::ReadWrite, Some(lock))
    }

    /// Opens an existing log without ever writing to it: it validates
    /// everything [`open`](Self::open) validates, but never creates,
    /// truncates, repairs or appends. A torn final record is an error
    /// ([`EventLogError::TornTail`]) instead of being cut; an unterminated
    /// but complete final record is accepted, as `open` accepts it, and
    /// left without its newline. The handle refuses every append
    /// ([`EventLogError::ReadOnly`]). It takes no lock, so it can read a
    /// log a locked writer is appending to; a record caught mid-write then
    /// reads as a torn tail, and reopening later reads it whole.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, EventLogError> {
        let path = path.as_ref().to_path_buf();
        // Read access only: a missing file is an error, never created.
        let file = OpenOptions::new().read(true).open(&path)?;
        Self::load(path, file, Access::ReadOnly, None)
    }

    fn load(
        path: PathBuf,
        mut file: File,
        access: Access,
        lock: Option<File>,
    ) -> Result<Self, EventLogError> {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let Parsed {
            events,
            by_id,
            tail,
        } = parse(&bytes)?;

        let mut recovered = false;
        match (tail, access) {
            (Tail::Torn { line, detail, .. }, Access::ReadOnly) => {
                return Err(EventLogError::TornTail { line, detail });
            }
            (Tail::Torn { valid_len, .. }, Access::ReadWrite) => {
                file.set_len(valid_len)?;
                file.sync_all()?;
                recovered = true;
            }
            (Tail::Unterminated, Access::ReadWrite) => {
                // Complete final record missing its terminator: repair so
                // the next append starts on a fresh line.
                file.seek(SeekFrom::End(0))?;
                file.write_all(b"\n")?;
                file.sync_all()?;
            }
            // Read-only: accepted as `open` accepts it, never repaired.
            (Tail::Unterminated, Access::ReadOnly) | (Tail::Clean, _) => {}
        }
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            path,
            file,
            events,
            by_id,
            recovered_truncated_tail: recovered,
            read_only: access == Access::ReadOnly,
            poisoned: None,
            lock,
            #[cfg(test)]
            fail_next_append_after: None,
            #[cfg(test)]
            fail_next_rollback: false,
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

    /// `hash` of the last record in the file (the head of the whole
    /// file's chain), or None for an empty or pre-1.1-only log. This is
    /// not what receipts carry: a receipt's `head_hash` is the hash of its
    /// run's own last event, which in a multi-run log is usually an
    /// earlier record.
    pub fn head_hash(&self) -> Option<&str> {
        self.events
            .last()
            .map(|e| e.hash.as_str())
            .filter(|h| !h.is_empty())
    }

    /// True for a handle from [`open_read_only`](Self::open_read_only).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// True while this handle holds the writer lock of
    /// [`open_locked`](Self::open_locked).
    pub fn is_locked(&self) -> bool {
        self.lock.is_some()
    }

    /// True once a failed append could not be rolled back. The handle
    /// then refuses every append; reopen the log to continue.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.is_some()
    }

    /// Append one event and return its sequence. Assigns the chain link,
    /// fsyncs, and only then commits in memory; any failure rolls the file
    /// back to its previous length, and a failed rollback poisons the
    /// handle. An `event_id` already stored with the same content returns
    /// the stored sequence without writing; with different content it is
    /// [`EventLogError::IdempotencyConflict`]. Refused on a read-only or
    /// poisoned handle.
    pub fn append(&mut self, new: NewEvent) -> Result<u64, EventLogError> {
        if self.read_only {
            return Err(EventLogError::ReadOnly);
        }
        if let Some(detail) = &self.poisoned {
            return Err(EventLogError::Poisoned {
                detail: detail.clone(),
            });
        }
        if let Some(&seq) = self.by_id.get(&new.event_id) {
            // Sequences start at 1 with no gaps: record `seq` is at index
            // `seq - 1`.
            let Some(stored) = seq
                .checked_sub(1)
                .and_then(|index| self.events.get(index as usize))
            else {
                return Err(EventLogError::Corrupt {
                    line: seq as usize,
                    detail: format!("event {} is indexed but not stored", new.event_id),
                });
            };
            // A retry of the same event is a no-op. Anything else reusing
            // the id is a different event: answering it with the stored
            // record's sequence would silently drop it.
            let fields = differing_fields(stored, &new);
            if fields.is_empty() {
                return Ok(seq);
            }
            return Err(EventLogError::IdempotencyConflict {
                event_id: new.event_id,
                sequence: seq,
                fields,
            });
        }
        let seq = self.events.len() as u64 + 1;
        let mut ev = new.into_sequenced(seq);
        ev.prev_hash = expected_prev_hash(self.events.last());
        ev.hash = compute_hash(&ev)?;
        let mut line = serde_json::to_string(&ev)?;
        line.push('\n');
        let prev_len = self.file.metadata()?.len();
        let written = (|| -> std::io::Result<()> {
            #[cfg(test)]
            if let Some(cut) = self.fail_next_append_after.take() {
                self.file
                    .write_all(&line.as_bytes()[..cut.min(line.len())])?;
                self.file.flush()?;
                return Err(std::io::Error::other("injected mid-record write failure"));
            }
            self.file.write_all(line.as_bytes())?;
            self.file.flush()?;
            self.file.sync_all()
        })();
        if let Err(e) = written {
            // Roll back the partial append; the event never committed.
            if let Err(rollback) = self.roll_back(prev_len) {
                // Partial bytes may still end the file, and this handle's
                // next record would be written after them: a corrupt line
                // in the middle of the log. Refuse every later append.
                let detail = format!(
                    "append of event {} failed ({e}) and rolling it back failed too ({rollback})",
                    ev.event_id
                );
                self.poisoned = Some(detail.clone());
                return Err(EventLogError::Poisoned { detail });
            }
            return Err(e.into());
        }
        self.by_id.insert(ev.event_id.clone(), seq);
        self.events.push(ev);
        Ok(seq)
    }

    /// Cuts the file back to `len` bytes and makes that durable.
    fn roll_back(&mut self, len: u64) -> std::io::Result<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_rollback) {
            return Err(std::io::Error::other("injected rollback failure"));
        }
        self.file.set_len(len)?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.sync_all()
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
    fn failed_mid_record_append_rolls_back_and_log_stays_valid() {
        let path = temp_log("rollback.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();
        log.append(task_event(&run, "e1")).unwrap();
        let committed = std::fs::read(&path).unwrap();

        // The write dies 40 bytes into the second record.
        log.fail_next_append_after = Some(40);
        let err = log.append(fail_event(&run, "e2")).unwrap_err();
        assert!(err.to_string().contains("injected"), "{err}");
        assert_eq!(log.len(), 1);
        assert!(!log.contains(&EventId::new("e2")), "event must be absent");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            committed,
            "partial bytes rolled back"
        );

        // A fresh open sees a clean, fully valid log: no torn tail to
        // recover, and the chain still verifies.
        let reopened = EventLog::open(&path).unwrap();
        assert!(!reopened.recovered_truncated_tail);
        assert_eq!(reopened.len(), 1);
        assert!(!reopened.contains(&EventId::new("e2")));
        drop(reopened);

        // The same handle keeps working: the retry commits as sequence 2.
        assert_eq!(log.append(fail_event(&run, "e2")).unwrap(), 2);
        drop(log);
        let log = EventLog::open(&path).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log.rebuild(&run).unwrap().state, RunState::Failed);
    }

    #[test]
    fn tampered_record_breaks_hash_chain() {
        let path = temp_log("tampered.jsonl");
        let run = RunId::new("run-1");
        {
            let mut log = EventLog::open(&path).unwrap();
            log.append(task_event(&run, "e1")).unwrap();
            log.append(fail_event(&run, "e2")).unwrap();
            assert_eq!(log.head_hash().map(str::len), Some(64));
        }
        let content = std::fs::read_to_string(&path).unwrap();
        // Edit the stored title without touching the hashes.
        let forged = content.replacen("\"title\":\"t\"", "\"title\":\"forged\"", 1);
        assert_ne!(forged, content);
        std::fs::write(&path, forged).unwrap();
        match EventLog::open(&path) {
            Err(EventLogError::HashChain { line, .. }) => assert_eq!(line, 1),
            other => panic!("expected HashChain error, got {other:?}"),
        }
    }

    #[test]
    fn legacy_1_0_prefix_opens_and_chains_to_new_records() {
        let path = temp_log("legacy.jsonl");
        let run = RunId::new("run-1");
        // A raw pre-1.1 line: schema 1.0, no hash fields at all.
        let legacy = r#"{"schema_version":"1.0","sequence":1,"event_id":"e1","run_id":"run-1","actor":{"kind":"human","id":"mario"},"occurred_at":"2026-10-26T00:00:00Z","payload":{"type":"task_created","task":{"schema_version":"1.0","id":"task-1","run_id":"run-1","project_id":"p","title":"t","description":"d","created_at":"2026-10-26T00:00:00Z"}}}"#;
        std::fs::write(&path, format!("{legacy}\n")).unwrap();
        let mut log = EventLog::open(&path).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log.head_hash(), None);
        log.append(fail_event(&run, "e2")).unwrap();
        assert_eq!(log.all_events()[1].prev_hash, "pre-1.1:1");
        assert_eq!(log.head_hash().map(str::len), Some(64));
        // Reopen revalidates the mixed chain.
        let log = EventLog::open(&path).unwrap();
        assert_eq!(log.len(), 2);
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

    /// A committed two-event log at `name`; returns its path and bytes.
    fn two_event_log(name: &str) -> (PathBuf, Vec<u8>) {
        let path = temp_log(name);
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();
        log.append(task_event(&run, "e1")).unwrap();
        log.append(fail_event(&run, "e2")).unwrap();
        drop(log);
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    }

    #[test]
    fn read_only_open_reads_what_open_reads_and_never_writes() {
        let (path, bytes) = two_event_log("read-only.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open_read_only(&path).unwrap();
        assert!(log.is_read_only() && !log.is_locked());
        assert!(!log.recovered_truncated_tail);
        let writable = EventLog::open(&path).unwrap();
        assert_eq!(log.all_events(), writable.all_events());
        assert_eq!(log.head_hash(), writable.head_hash());
        drop(writable);
        assert_eq!(log.rebuild(&run).unwrap().state, RunState::Failed);

        // Every append is refused, even a retry of a stored event.
        for new in [fail_event(&run, "e3"), task_event(&run, "e1")] {
            assert!(matches!(log.append(new), Err(EventLogError::ReadOnly)));
        }
        assert!(matches!(
            crate::sink::EventSink::append(&mut log, fail_event(&run, "e3")),
            Err(EventLogError::ReadOnly)
        ));
        assert_eq!(log.len(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
    }

    #[test]
    fn read_only_open_refuses_a_torn_tail_and_leaves_it_in_place() {
        let (path, _) = two_event_log("read-only-torn.jsonl");
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(b"{\"schema_version\":\"1.2\",\"sequence\":3,\"eve")
                .unwrap();
        }
        let torn = std::fs::read(&path).unwrap();
        match EventLog::open_read_only(&path) {
            Err(EventLogError::TornTail { line: 3, .. }) => {}
            other => panic!("expected a torn tail, got {other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), torn, "never truncated");
        // `open` still repairs it, as before.
        let log = EventLog::open(&path).unwrap();
        assert!(log.recovered_truncated_tail);
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn read_only_open_accepts_an_unterminated_record_without_repairing_it() {
        let (path, bytes) = two_event_log("read-only-unterminated.jsonl");
        let unterminated = &bytes[..bytes.len() - 1];
        std::fs::write(&path, unterminated).unwrap();
        let log = EventLog::open_read_only(&path).unwrap();
        assert_eq!(log.len(), 2);
        drop(log);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            unterminated,
            "no newline added"
        );
        // `open` accepts the same record and terminates it.
        drop(EventLog::open(&path).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn read_only_open_never_creates_a_log() {
        let dir = temp_log("never-created");
        let path = dir.join("events.jsonl");
        match EventLog::open_read_only(&path) {
            Err(EventLogError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            other => panic!("expected not-found, got {other:?}"),
        }
        assert!(!dir.exists(), "no directory or file was created");
    }

    #[test]
    fn read_only_open_rejects_what_open_rejects() {
        let (path, bytes) = two_event_log("read-only-corrupt.jsonl");
        let good = String::from_utf8(bytes).unwrap();
        let first_line_end = good.find('\n').unwrap();
        let corruptions = [
            good.replacen("\"title\":\"t\"", "\"title\":\"forged\"", 1),
            good.replacen("\"sequence\":2", "\"sequence\":7", 1),
            format!("not json at all{}", &good[first_line_end..]),
            good.replacen('\n', "\n\n", 1),
        ];
        for corrupt in corruptions {
            assert_ne!(corrupt, good);
            std::fs::write(&path, &corrupt).unwrap();
            let read_only = EventLog::open_read_only(&path).unwrap_err().to_string();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
            let writable = EventLog::open(&path).unwrap_err().to_string();
            assert_eq!(read_only, writable);
        }
    }

    #[test]
    fn locked_open_shuts_out_a_second_writer_until_the_first_drops() {
        let path = temp_log("locked.jsonl");
        let run = RunId::new("run-1");
        let mut first = EventLog::open_locked(&path).unwrap();
        assert!(first.is_locked() && !first.is_read_only());
        first.append(task_event(&run, "e1")).unwrap();

        // Another handle on the same log, under any spelling of its path.
        let dotted = path
            .parent()
            .unwrap()
            .join(".")
            .join(path.file_name().unwrap());
        for other in [&path, &dotted] {
            match EventLog::open_locked(other) {
                Err(EventLogError::Locked { path: p, lock_file }) => {
                    assert_eq!(&p, other);
                    assert!(lock_file.ends_with("locked.jsonl.lock"), "{lock_file:?}");
                }
                other => panic!("expected Locked, got {other:?}"),
            }
        }
        // Readers are never shut out, and see what was committed.
        let reader = EventLog::open_read_only(&path).unwrap();
        assert_eq!(reader.all_events(), first.all_events());

        first.append(fail_event(&run, "e2")).unwrap();
        drop(first);
        let mut second = EventLog::open_locked(&dotted).unwrap();
        assert_eq!(second.len(), 2);
        let other = RunId::new("run-2");
        assert_eq!(second.append(task_event(&other, "e3")).unwrap(), 3);
        // The lock file stays beside the log; it holds no data.
        assert!(path.with_file_name("locked.jsonl.lock").exists());
    }

    #[test]
    fn locked_open_validates_and_repairs_like_open() {
        let (path, _) = two_event_log("locked-torn.jsonl");
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(b"{\"schema_version\":\"1.2\",\"seq").unwrap();
        }
        let mut log = EventLog::open_locked(&path).unwrap();
        assert!(log.recovered_truncated_tail);
        assert_eq!(log.len(), 2);
        let run = RunId::new("run-1");
        assert_eq!(log.append(fail_event(&run, "e3")).unwrap(), 3);
        drop(log);
        assert_eq!(EventLog::open_read_only(&path).unwrap().len(), 3);
    }

    #[test]
    fn failed_rollback_poisons_the_handle() {
        let path = temp_log("poisoned.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();
        log.append(task_event(&run, "e1")).unwrap();
        let committed = std::fs::read(&path).unwrap();

        // The write dies 40 bytes into the record, and the rollback fails.
        log.fail_next_append_after = Some(40);
        log.fail_next_rollback = true;
        match log.append(fail_event(&run, "e2")) {
            Err(EventLogError::Poisoned { detail }) => {
                assert!(
                    detail.contains("injected mid-record write failure"),
                    "{detail}"
                );
                assert!(detail.contains("injected rollback failure"), "{detail}");
            }
            other => panic!("expected Poisoned, got {other:?}"),
        }
        assert!(log.is_poisoned());
        assert_eq!(log.len(), 1);
        assert!(!log.contains(&EventId::new("e2")));
        let torn = std::fs::read(&path).unwrap();
        assert_eq!(torn.len(), committed.len() + 40, "the partial bytes stayed");

        // Every later append is refused, so nothing lands after them.
        for new in [
            fail_event(&run, "e2"),
            fail_event(&run, "e3"),
            task_event(&run, "e1"),
        ] {
            assert!(matches!(
                log.append(new),
                Err(EventLogError::Poisoned { .. })
            ));
        }
        assert_eq!(std::fs::read(&path).unwrap(), torn);
        drop(log);

        // A read-only open reports the torn tail; reopening for writing
        // cuts it, and the retry commits as sequence 2.
        assert!(matches!(
            EventLog::open_read_only(&path),
            Err(EventLogError::TornTail { line: 2, .. })
        ));
        let mut log = EventLog::open(&path).unwrap();
        assert!(log.recovered_truncated_tail);
        assert!(!log.is_poisoned());
        assert_eq!(log.append(fail_event(&run, "e2")).unwrap(), 2);
    }

    #[test]
    fn reused_event_id_with_different_content_is_a_conflict() {
        let path = temp_log("conflict.jsonl");
        let run = RunId::new("run-1");
        let mut log = EventLog::open(&path).unwrap();
        let original = task_event(&run, "e1");
        assert_eq!(log.append(original.clone()).unwrap(), 1);
        let bytes = std::fs::read(&path).unwrap();

        let mut other_run = original.clone();
        other_run.run_id = RunId::new("run-2");
        let mut other_actor = original.clone();
        other_actor.actor = Actor::Runtime;
        let mut other_time = original.clone();
        other_time.occurred_at = "2026-10-27T00:00:00Z".into();
        let mut other_payload = original.clone();
        if let EventPayload::TaskCreated { task } = &mut other_payload.payload {
            task.title = "a different task".into();
        }
        let everything = NewEvent {
            event_id: original.event_id.clone(),
            ..fail_event(&RunId::new("run-2"), "unused")
        };
        let cases: [(NewEvent, &[&str]); 5] = [
            (other_run, &["run_id"]),
            (other_actor, &["actor"]),
            (other_time, &["occurred_at"]),
            (other_payload, &["payload"]),
            (everything, &["run_id", "actor", "payload"]),
        ];
        for (new, expected) in cases {
            match log.append(new) {
                Err(EventLogError::IdempotencyConflict {
                    event_id,
                    sequence: 1,
                    fields,
                }) => {
                    assert_eq!(event_id, EventId::new("e1"));
                    assert_eq!(fields, expected);
                }
                other => panic!("expected a conflict on {expected:?}, got {other:?}"),
            }
        }
        assert_eq!(log.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");

        // The same content stays a no-op, also against the record as
        // parsed back from disk; the sink reports the conflict too.
        assert_eq!(log.append(original.clone()).unwrap(), 1);
        drop(log);
        let mut log = EventLog::open(&path).unwrap();
        assert_eq!(log.append(original.clone()).unwrap(), 1);
        let mut conflicting = original;
        conflicting.occurred_at = "2026-10-28T00:00:00Z".into();
        assert!(matches!(
            crate::sink::EventSink::append(&mut log, conflicting),
            Err(EventLogError::IdempotencyConflict { sequence: 1, .. })
        ));
        assert_eq!(log.len(), 1);
    }
}
