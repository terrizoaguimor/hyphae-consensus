// SPDX-License-Identifier: Apache-2.0
//! Per-run storage: one event log per run, plus the files that say who
//! owns the run, who may write it, and that it existed.
//!
//! Layout under the store's root:
//!
//! ```text
//! runs/<run_id>.jsonl      the run's EventLog: sequence from 1, chain from genesis
//! runs/<run_id>.meta.json  sidecar: owner and retention, written atomically
//! runs/<run_id>.lock       held exclusively by the run's one owner
//! runs/index.jsonl         hash-chained run index: created, terminal, purged
//! runs/index.lock          serializes run index updates across processes
//! ```
//!
//! - **Ids before paths (LC-15, ABUSE-7).** A file name is only ever built
//!   from an id [`parse_run_id`] accepted: `run-` and a lowercase hyphenated
//!   UUID v4, exactly what [`UuidIdGen`] makes. `../x`, absolute paths,
//!   drive letters, NUL, uppercase and every other string are refused
//!   before any path exists. Lookups create nothing: an unknown id is
//!   refused from the run index alone, and a known run's files are opened,
//!   never created.
//! - **One file per run (RT-3, RT-4).** Each run is its own [`EventLog`]
//!   (sequence, genesis-anchored chain, fsync, rollback and torn-tail
//!   recovery all reused), so its head is the run's own last record: a run
//!   verifies from its file alone, without anyone else's events, and a
//!   damaged file takes only that run offline.
//! - **One owner per run (RT-6).** Whoever creates or opens a run holds
//!   `<run_id>.lock` exclusively (`File::try_lock`) for as long as its
//!   [`RunHandle`] or [`SharedRunLog`] lives. A second owner, in this or
//!   any other process, gets [`RunStoreError::InUse`], and startup recovery
//!   skips a run another live process holds instead of marking it
//!   interrupted. The lock is a separate file because Windows locks are
//!   mandatory: a locked log could not even be read.
//! - **Reads (ABUSE-7).** [`RunStore::read_run`] is the path for every
//!   visitor-facing read (run, events, receipt, a finished run's replay).
//!   It checks the owner from the sidecar before the run's lock or log is
//!   touched, holds the lock *shared* (any number of readers at once;
//!   writers, recovery and purge are refused until they are gone), and
//!   verifies the log from its bytes without opening it for writing, so a
//!   read never creates, repairs or appends anything. A run some writer is
//!   appending to is read through that writer's
//!   [`SharedRunLog::snapshot_from`] instead. Visitor-facing writes (a
//!   decision, a cancel) take the run through [`RunStore::open_run_as`],
//!   which makes the same owner check first.
//! - **Sidecar.** `<run_id>.meta.json` holds the owner (a session only as
//!   the SHA-256 of its id, see [`SessionHash`]), the creation time and the
//!   retention. It is written to a temporary file, flushed, fsync'd and
//!   renamed into place, so no partial sidecar is ever visible. It is read
//!   strictly (only the exact bytes this store writes), checked against its
//!   file name, and checked against the SHA-256 of the sidecar that the
//!   run's `created` index record committed to, so a changed owner or
//!   retention is detected however it is formatted. Any mismatch is
//!   [`RunStoreError::Inconsistent`] ("provenance inconsistent"). The
//!   events never name the owner, so the sidecar is not checked against
//!   them.
//! - **Run index (LC-12).** Per-run chains have no cross-run anchor, so a
//!   run rewritten and re-hashed, truncated or deleted would otherwise go
//!   unnoticed. `index.jsonl` records each run's creation (with its
//!   sidecar's digest), a checkpoint of its head (last sequence and hash)
//!   once its task is recorded and whenever it comes to rest awaiting a
//!   human decision, its terminal head, and its purge. Records chain like
//!   event records: SHA-256 over `prev_hash` and the record's canonical
//!   JSON with `hash` blanked. The index is verified whenever the store
//!   opens; any edit is [`RunStoreError::IndexTampered`], which keeps the
//!   whole store closed, because an index that fails cannot vouch for any
//!   run. A run's log is compared with the latest head the index recorded
//!   for it before anything is written to it: the record at that sequence
//!   must be there, whole, with that hash, and nothing may follow a
//!   terminal head. A log that fails this is reported and left exactly as
//!   it is, never repaired, closed again or re-indexed. Recovery reports
//!   indexed runs whose files are gone, and a lost index keeps the store
//!   from opening ([`RunStoreError::IndexMissing`]) instead of being
//!   replaced by an empty one.
//!
//!   What this cannot detect: records appended after a run's latest
//!   checkpoint (in the middle of a deliberation) and then removed, and,
//!   since the index is unsigned too, a consistent rewrite of the index
//!   and the logs together. The index detects changes made after someone
//!   obtained its head ([`RunStore::index_head`]); publishing that head is
//!   what anchors it.
//! - **Retention.** [`RunStore::purge_expired`] deletes an expired visitor
//!   run's files whole and records the purge. Operator and showcase runs
//!   are never purged.

use crate::clock::{Clock, IdGen, SystemClock, UuidIdGen};
use crate::event_log::{EventLog, EventLogError};
use crate::orchestrator::mark_interrupted_runs;
use crate::shared_log::{names_another_run, EventPublisher, SharedRunLog};
use crate::state_machine::RunAggregate;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use consensus_domain::{ConsensusEvent, EventId, RunId, RunState, READABLE_SCHEMA_VERSIONS};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use thiserror::Error;

/// Directory under the store root that holds every run file.
pub const RUNS_DIR: &str = "runs";
/// The run index, inside [`RUNS_DIR`].
pub const INDEX_FILE: &str = "index.jsonl";
/// Serializes run index updates across processes.
const INDEX_LOCK_FILE: &str = "index.lock";
/// `prev_hash` of the run index's first record.
pub const INDEX_GENESIS: &str = "index-genesis";
/// Sidecar format this version writes and reads.
pub const META_VERSION: u32 = 1;
/// Length of every accepted run id: `run-` and a 36-character UUID.
pub const RUN_ID_LEN: usize = 40;
/// The files a run can have: log, sidecar, temporary sidecar, lock.
const RUN_FILE_SUFFIXES: [&str; 4] = [".jsonl", ".meta.json", ".meta.json.tmp", ".lock"];

const HEAD_MISMATCH: &str = "the event log does not hold the head the run index recorded: the log was truncated or rewritten";
const HEAD_MISSING: &str =
    "the event log is shorter than the head the run index recorded: records were removed";
const AFTER_TERMINAL: &str =
    "records follow the terminal head the run index recorded: the log was extended by hand";
/// Chain anchor of a run log's first record, as in [`EventLog`].
const LOG_GENESIS: &str = "genesis";

/// Why [`parse_run_id`] refused a string. The input itself is not kept:
/// it is untrusted and may be anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid run id: {reason}")]
pub struct InvalidRunId {
    reason: &'static str,
}

impl InvalidRunId {
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

/// Accepts exactly the run ids [`UuidIdGen`] makes: `run-` and a
/// lowercase, hyphenated UUID of version 4 with the RFC 4122 variant,
/// `run-xxxxxxxx-xxxx-4xxx-[89ab]xxx-xxxxxxxxxxxx`. Everything else is
/// refused before any path is built from it: separators, `..`, absolute
/// paths, drive letters, NUL, uppercase, other UUID versions or variants,
/// any other length. A store builds file names from accepted ids only.
pub fn parse_run_id(s: &str) -> Result<RunId, InvalidRunId> {
    let invalid = |reason| Err(InvalidRunId { reason });
    if s.len() != RUN_ID_LEN {
        return invalid("expected run- and a 36-character UUID");
    }
    let Some(uuid) = s.strip_prefix("run-") else {
        return invalid("expected the prefix run-");
    };
    for (i, b) in uuid.bytes().enumerate() {
        let (ok, reason) = match i {
            8 | 13 | 18 | 23 => (b == b'-', "expected a hyphenated UUID"),
            14 => (b == b'4', "expected a version 4 UUID"),
            19 => (
                matches!(b, b'8' | b'9' | b'a' | b'b'),
                "expected the RFC 4122 UUID variant",
            ),
            _ => (
                matches!(b, b'0'..=b'9' | b'a'..=b'f'),
                "expected lowercase hexadecimal digits",
            ),
        };
        if !ok {
            return invalid(reason);
        }
    }
    Ok(RunId::new(s))
}

/// Who a run belongs to: decides who may read it and whether retention
/// may delete it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Owner {
    /// A visitor's session, stored only as the SHA-256 of the session id:
    /// the sidecar never holds the id itself. Build it with
    /// [`Owner::session`].
    Session { hash: SessionHash },
    /// The operator.
    Operator,
    /// A curated showcase run.
    Showcase,
}

impl Owner {
    /// The owner for the session whose secret id is `session_id`. The id
    /// is hashed here; code has no other way to turn a session id into a
    /// session owner (see [`SessionHash`]).
    pub fn session(session_id: &[u8]) -> Self {
        Owner::Session {
            hash: SessionHash(sha256_hex(session_id)),
        }
    }
}

/// The lowercase hex SHA-256 of a session id: how a session owner is
/// stored. Its field is private, so the only way to make one in code is
/// [`Owner::session`], which hashes the raw id it is given. A raw id
/// therefore never reaches a sidecar by mistake, even one that looks like
/// a digest (a hex-encoded 256-bit session id does).
///
/// ```
/// use consensus_runtime::Owner;
/// let cookie = "0f".repeat(32);
/// let owner = Owner::session(cookie.as_bytes());
/// let Owner::Session { hash } = &owner else { unreachable!() };
/// assert_ne!(hash.as_str(), cookie);
/// ```
///
/// Putting the raw id in its place does not compile:
///
/// ```compile_fail
/// use consensus_runtime::Owner;
/// let cookie = "0f".repeat(32);
/// let owner = Owner::Session { hash: cookie };
/// ```
///
/// ```compile_fail
/// use consensus_runtime::{Owner, SessionHash};
/// let cookie = "0f".repeat(32);
/// let owner = Owner::Session { hash: SessionHash(cookie) };
/// ```
///
/// Deserializing accepts only a SHA-256 hex digest. It is how a stored
/// sidecar is read back; it cannot tell a digest from a raw id that has
/// the same form, so no caller should deserialize a session id into one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionHash(String);

impl SessionHash {
    /// The digest, as stored.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for SessionHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SessionHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hash = String::deserialize(deserializer)?;
        if !is_sha256_hex(&hash) {
            return Err(serde::de::Error::custom(
                "a session owner is the SHA-256 hex digest of the session id, never the id itself",
            ));
        }
        Ok(Self(hash))
    }
}

/// How long a run stays reachable and stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Kept until removed by hand.
    Keep,
    /// The owner's access ends `access_for` after creation. The files are
    /// deleted whole by [`RunStore::purge_expired`] once `delete_after`
    /// has passed, if the owner is a session. `access_for` may not exceed
    /// `delete_after`.
    Expiring {
        access_for: Duration,
        delete_after: Duration,
    },
}

impl Retention {
    /// The visitor default: access for 24 hours, deleted after 7 days.
    pub const VISITOR: Retention = Retention::Expiring {
        access_for: Duration::from_secs(24 * 3600),
        delete_after: Duration::from_secs(7 * 24 * 3600),
    };
}

/// The sidecar `<run_id>.meta.json`, written once, when the run is
/// created. Times are RFC 3339, UTC, whole seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunMeta {
    /// Sidecar format version ([`META_VERSION`]).
    pub version: u32,
    pub run_id: RunId,
    pub owner: Owner,
    pub created_at: String,
    /// When the owner's access ends; None: never.
    pub access_expires_at: Option<String>,
    /// When retention deletes a session-owned run; None: never.
    pub delete_after: Option<String>,
}

impl RunMeta {
    fn for_new_run(
        run_id: RunId,
        owner: Owner,
        now: DateTime<Utc>,
        retention: Retention,
    ) -> Result<Self, RunStoreError> {
        let (access_expires_at, delete_after) = match retention {
            Retention::Keep => (None, None),
            Retention::Expiring {
                access_for,
                delete_after,
            } => {
                if access_for > delete_after {
                    return Err(RunStoreError::InvalidRetention(
                        "access may not outlast the run's files",
                    ));
                }
                let after = |period: Duration| {
                    TimeDelta::from_std(period)
                        .ok()
                        .and_then(|period| now.checked_add_signed(period))
                        .map(stamp)
                        .ok_or(RunStoreError::InvalidRetention(
                            "retention period out of range",
                        ))
                };
                (Some(after(access_for)?), Some(after(delete_after)?))
            }
        };
        Ok(Self {
            version: META_VERSION,
            run_id,
            owner,
            created_at: stamp(now),
            access_expires_at,
            delete_after,
        })
    }

    /// True while the owner may still reach the run.
    pub fn access_open_at(&self, now: DateTime<Utc>) -> bool {
        match self.access_expires_at.as_deref().map(parse_time) {
            None => true,
            Some(Ok(expires)) => now < expires,
            Some(Err(_)) => false,
        }
    }

    /// True when retention may delete the run: a session owns it and
    /// `delete_after` has been reached.
    pub fn purgeable_at(&self, now: DateTime<Utc>) -> bool {
        matches!(self.owner, Owner::Session { .. })
            && matches!(
                self.delete_after.as_deref().map(parse_time),
                Some(Ok(due)) if due <= now
            )
    }
}

/// What a run index record says happened to a run. A run's records come
/// in this order: `created` once, then any number of `checkpoint`s with
/// growing sequences, at most one `terminal`, and at most one `purged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexKind {
    /// The run's files were created. Its log is still empty, so it has no
    /// head; `meta_sha256` commits to its sidecar.
    Created,
    /// The run's head where it may rest: right after its task is
    /// recorded, and whenever it awaits a human decision. A later write
    /// to the run is refused unless its log still holds this head.
    Checkpoint,
    /// The run reached a terminal state; the head is its log's last
    /// record, and nothing may follow it.
    Terminal,
    /// Retention deleted the run's files; the head is the one they had,
    /// if the log could still be read and agreed with the index.
    Purged,
}

impl IndexKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IndexKind::Created => "created",
            IndexKind::Checkpoint => "checkpoint",
            IndexKind::Terminal => "terminal",
            IndexKind::Purged => "purged",
        }
    }
}

/// One line of `runs/index.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexRecord {
    /// Gap-free, from 1.
    pub seq: u64,
    pub run_id: RunId,
    pub kind: IndexKind,
    /// The sequence of the run log's record whose hash is `head_hash`:
    /// the run's last record at this point. None exactly when
    /// `head_hash` is None.
    pub head_sequence: Option<u64>,
    /// The run log's chain head at this point.
    pub head_hash: Option<String>,
    /// `created` records only: the SHA-256 of the sidecar's exact bytes,
    /// lowercase hex, so a later change to the run's owner or retention
    /// is detected.
    pub meta_sha256: Option<String>,
    /// RFC 3339, UTC.
    pub at: String,
    /// `hash` of the previous record; [`INDEX_GENESIS`] for the first.
    pub prev_hash: String,
    /// SHA-256 over (`prev_hash` || canonical JSON of this record with
    /// `hash` blanked), lowercase hex.
    pub hash: String,
}

/// The run index's head: its last record's sequence and hash, or 0 and
/// [`INDEX_GENESIS`] while it is empty. Publishing it (health endpoint,
/// receipts) lets whoever keeps it detect any later change to the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexHead {
    pub seq: u64,
    pub hash: String,
}

#[derive(Debug, Error)]
pub enum RunStoreError {
    #[error(transparent)]
    InvalidId(#[from] InvalidRunId),
    /// Never created here, or purged; for the owner-checked paths
    /// ([`RunStore::read_run`], [`RunStore::open_run_as`]) also a run that
    /// belongs to someone else or whose owner's access has ended. An API
    /// answers 404, as for [`InvalidId`](Self::InvalidId).
    #[error("unknown run {0}")]
    UnknownRun(RunId),
    /// Another handle, in this or another process, holds the run: an
    /// owner, for a second owner or a reader; a reader, for an owner,
    /// recovery or purge.
    #[error("run {0} is held by another handle or process")]
    InUse(RunId),
    /// The run's files disagree with each other or with the run index.
    #[error("run {run_id}: provenance inconsistent: {detail}")]
    Inconsistent { run_id: RunId, detail: String },
    /// The run index fails verification: a record edited, removed,
    /// reordered or added by hand. At [`RunStore::open`] this keeps the
    /// whole store closed (every run offline) until a copy of the index
    /// that verifies is restored: an index that fails cannot vouch for
    /// any run. Found later, it fails every operation that needs the index
    /// and shows in [`RunStore::health`].
    #[error("run index fails verification at record {line}: {detail}")]
    IndexTampered { line: usize, detail: String },
    /// `runs/` has no run index, yet a store has used it: the index was
    /// lost. A new, empty index would silently disown every run, so the
    /// store refuses to open unless [`MissingIndex::StartEmpty`] says to.
    #[error("the run index is missing, but {found}: restore the index, or start an empty one explicitly (MissingIndex::StartEmpty)")]
    IndexMissing { found: String },
    #[error("invalid retention: {0}")]
    InvalidRetention(&'static str),
    #[error("clock time is not RFC 3339: {0:?}")]
    Clock(String),
    #[error("{0} lock poisoned: a thread panicked while holding it")]
    Poisoned(&'static str),
    #[error("event log error: {0}")]
    Log(#[from] EventLogError),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
}

/// What [`RunStore::recover`] found and did. Problems with one run are
/// reported here, never raised, so a bad run cannot stop startup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Runs left mid-deliberation, now closed as
    /// `Failed{stage:"interrupted"}`.
    pub interrupted: Vec<RunId>,
    /// Terminal runs whose head was missing from the index, now recorded.
    pub terminal_recorded: Vec<RunId>,
    /// Runs awaiting a human decision whose checkpoint was missing from
    /// the index, now recorded.
    pub checkpoints_recorded: Vec<RunId>,
    /// Logs whose torn final record (a crash mid-append, after the run's
    /// latest recorded head) was removed.
    pub truncated_tails: Vec<RunId>,
    /// Logs that fail to open or replay. Left as they are, never rewritten.
    pub unreplayable: Vec<(RunId, String)>,
    /// Runs whose files disagree with each other or with the index: a bad
    /// sidecar, another run's records, or a log that does not hold the
    /// latest head the index recorded (records removed, cut, rewritten,
    /// or added after a terminal head). Such a log is left exactly as it
    /// is: it is read before anything is written, and never repaired,
    /// closed or re-indexed.
    pub inconsistent: Vec<(RunId, String)>,
    /// Indexed and never purged, but the log file is gone.
    pub missing: Vec<RunId>,
    /// Run ids with files the index never recorded: a creation that did
    /// not finish, or files placed by hand. Never opened or served, and
    /// left in place.
    pub unindexed: Vec<RunId>,
    /// Runs another live handle or process holds. Not touched.
    pub in_use: Vec<RunId>,
    /// Recorded purges whose files were still present, now deleted.
    pub purges_completed: Vec<RunId>,
    /// Per-run I/O failures during recovery.
    pub failed: Vec<(RunId, String)>,
    /// The run index ended in a torn record, which was removed on open.
    pub index_tail_repaired: bool,
}

/// What [`RunStore::purge_expired`] deleted, and what it could not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub purged: Vec<RunId>,
    /// Expired, but held by a live owner or reader; left for the next
    /// purge.
    pub in_use: Vec<RunId>,
    pub failed: Vec<(RunId, String)>,
}

/// What [`RunStore::open_with`] does when `runs/index.jsonl` is missing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MissingIndex {
    /// Start an index only in a directory no store has used. If `runs/`
    /// already holds the index's lock file or any run's files, the index
    /// was lost: refuse with [`RunStoreError::IndexMissing`].
    #[default]
    Refuse,
    /// Start a new, empty index all the same: an operator's decision once
    /// the loss is understood. Every run already there is then unindexed,
    /// never served, and reported by [`RunStore::recover`].
    StartEmpty,
}

/// The store's state, for a health endpoint. Never an error itself:
/// whatever fails is reported in it. A store that failed to open (for
/// example [`RunStoreError::IndexTampered`] or
/// [`RunStoreError::IndexMissing`]) has no health to ask; the caller
/// reports that error instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreHealth {
    /// The run index's head, after the whole index was verified again
    /// from its bytes. None when that fails.
    pub index_head: Option<IndexHead>,
    /// The index failed verification ([`RunStoreError::IndexTampered`]):
    /// a record was edited, removed or added since the store opened.
    /// Everything that needs the index fails until the store is reopened
    /// on an index that verifies.
    pub index_tampered: bool,
    /// Why the index could not be verified now, tampered or otherwise
    /// (I/O, a poisoned lock, an append whose rollback failed).
    pub index_error: Option<String>,
    /// The index ended in a torn record (a crash mid-append), which was
    /// cut off. Only a head obtained earlier tells whether that record
    /// was the only thing lost.
    pub index_tail_repaired: bool,
    /// The last failure to index a run's checkpoint or terminal head; see
    /// [`RunStore::index_write_error`].
    pub index_write_error: Option<String>,
}

/// Run storage under one root directory (see the module docs). Share it
/// between threads behind an `Arc`.
pub struct RunStore {
    shared: Arc<StoreShared>,
}

impl std::fmt::Debug for RunStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunStore")
            .field("dir", &self.shared.dir)
            .finish_non_exhaustive()
    }
}

impl RunStore {
    /// Opens the store under `root`, creating it if needed, and verifies
    /// its run index. Times come from the system clock.
    ///
    /// An index that fails verification is
    /// [`RunStoreError::IndexTampered`], and a lost one
    /// [`RunStoreError::IndexMissing`]: either keeps the whole store
    /// closed. That is deliberate (it is the one store-wide stop; a bad
    /// run file only takes its own run offline), because the index is
    /// what vouches for every run.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RunStoreError> {
        Self::open_with_clock(root, Box::new(SystemClock))
    }

    /// [`open`](Self::open) with the clock that stamps sidecars and index
    /// records.
    pub fn open_with_clock(
        root: impl AsRef<Path>,
        clock: Box<dyn Clock + Send + Sync>,
    ) -> Result<Self, RunStoreError> {
        Self::open_with(root, clock, MissingIndex::Refuse)
    }

    /// [`open_with_clock`](Self::open_with_clock), saying what to do when
    /// the run index is missing.
    pub fn open_with(
        root: impl AsRef<Path>,
        clock: Box<dyn Clock + Send + Sync>,
        missing_index: MissingIndex,
    ) -> Result<Self, RunStoreError> {
        let dir = root.as_ref().join(RUNS_DIR);
        std::fs::create_dir_all(&dir)?;
        let index = RunIndex::open(&dir, missing_index)?;
        Ok(Self {
            shared: Arc::new(StoreShared {
                dir,
                clock,
                index: Mutex::new(index),
                catalog: Mutex::default(),
                index_write_error: Mutex::default(),
            }),
        })
    }

    /// The directory that holds the run files.
    pub fn runs_dir(&self) -> &Path {
        &self.shared.dir
    }

    /// Creates a run with a fresh id: lock file (new, then locked),
    /// sidecar (atomic), empty event log, and last its `created` index
    /// record. A failure removes whatever this call created. The returned
    /// handle owns the run; append its `TaskCreated` through
    /// [`RunHandle::into_shared`].
    pub fn create_run(
        &self,
        owner: Owner,
        retention: Retention,
    ) -> Result<RunHandle, RunStoreError> {
        let now = self.shared.now()?;
        let run_id = parse_run_id(&UuidIdGen.next("run"))?;
        let meta = RunMeta::for_new_run(run_id.clone(), owner, now, retention)?;
        let sidecar = meta_bytes(&meta)?;
        let paths = RunPaths::new(&self.shared.dir, &run_id)?;
        // The lock file comes first and must be new, so an id that already
        // has files is never reused and nothing is written over them.
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&paths.lock)?;
        let mut undo = Undo(vec![paths.lock.clone()]);
        acquire_run_lock(&lock, &run_id)?;
        if exists(&paths.log)? || exists(&paths.meta)? {
            return Err(inconsistent(
                &run_id,
                "files already exist for a new run id",
            ));
        }
        // Both paths were just found empty, so whatever appears there from
        // now on is this call's to remove on failure.
        undo.0.push(paths.meta.clone());
        undo.0.push(paths.log.clone());
        write_atomic(&paths.meta, &paths.meta_tmp, &sidecar)?;
        let log = EventLog::open(&paths.log)?;
        sync_dir(Some(&self.shared.dir))?;
        // Indexed last, committing to the sidecar's bytes: a crash before
        // this point leaves files the index never mentions, which are
        // never served and which recovery reports as unindexed.
        self.shared.index()?.append(
            &run_id,
            IndexKind::Created,
            None,
            Some(sha256_hex(&sidecar)),
            meta.created_at.clone(),
        )?;
        undo.0.clear();
        if let Ok(mut catalog) = self.shared.catalog() {
            catalog.insert(run_id.clone(), meta.clone());
        }
        Ok(RunHandle {
            run_id: run_id.clone(),
            meta,
            log,
            lease: Arc::new(RunLease {
                run_id,
                _lock: lock,
                store: self.shared.clone(),
            }),
        })
    }

    /// Opens a live run and takes ownership of it: the runtime's own path
    /// (resuming or closing a run it manages, the operator). It checks no
    /// owner; a visitor's write goes through
    /// [`open_run_as`](Self::open_run_as), and every read through
    /// [`read_run`](Self::read_run).
    ///
    /// The id is parsed before anything else, and an id the run index
    /// does not list as live is [`RunStoreError::UnknownRun`] without any
    /// file access beyond the index. For a known run nothing is created:
    /// the lock file is opened, never made, and locked
    /// ([`RunStoreError::InUse`] if an owner or a reader holds it); then
    /// the sidecar must pass its checks, and the log, read before it is
    /// opened for writing, must hold the latest head the index recorded
    /// (nothing after it if that head is terminal). Only then is it opened
    /// as an [`EventLog`], which must hold only this run's records and
    /// replay. Opening may remove a torn final record after that head, as
    /// [`EventLog::open`] does after a crash mid-append; a log that
    /// contradicts the index is refused untouched.
    pub fn open_run(&self, id: &str) -> Result<RunHandle, RunStoreError> {
        let run_id = parse_run_id(id)?;
        self.live_run(&run_id)?;
        self.take_run(run_id)
    }

    /// [`open_run`](Self::open_run) for a visitor-facing write (a
    /// decision, a cancel): `owner` must own the run and still have
    /// access to it, checked from the sidecar before the run's lock or log
    /// is touched. Anyone else gets [`RunStoreError::UnknownRun`], as for a
    /// run that does not exist.
    pub fn open_run_as(&self, id: &str, owner: &Owner) -> Result<RunHandle, RunStoreError> {
        let run_id = parse_run_id(id)?;
        let indexed = self.live_run(&run_id)?;
        self.owned_meta(&run_id, &indexed, owner)?;
        self.take_run(run_id)
    }

    /// Reads a stored run for `owner`: every GET, SSE replay and receipt
    /// path (ABUSE-7).
    ///
    /// - The id is parsed and looked up in the index first; an unknown id
    ///   touches no run file.
    /// - The owner is checked from the sidecar before the run's lock or
    ///   log is touched. A run that belongs to someone else, or whose
    ///   owner's access has ended ([`RunMeta::access_open_at`]), is
    ///   [`RunStoreError::UnknownRun`].
    /// - The run's lock file is opened read-only, never created, and
    ///   locked *shared*: any number of readers at once, while owners,
    ///   recovery and purge, which need it exclusively, are refused
    ///   ([`RunStoreError::InUse`], or the run reported in use) until every
    ///   reader is gone. A reader likewise gets `InUse` while an owner
    ///   holds the run: a run some writer is appending to is read through
    ///   that writer's [`SharedRunLog::snapshot_from`].
    /// - The log is read and verified from its bytes, never opened for
    ///   writing: the chain, the sequences and ids, the latest head the
    ///   index recorded, only this run's records, and replay. A torn final
    ///   record is refused, not repaired. Nothing is created, repaired or
    ///   appended.
    ///
    /// The view holds the shared lock until it is dropped; drop it once
    /// the response is built, not after streaming it.
    pub fn read_run(&self, id: &str, owner: &Owner) -> Result<RunView, RunStoreError> {
        let run_id = parse_run_id(id)?;
        let indexed = self.live_run(&run_id)?;
        let meta = self.owned_meta(&run_id, &indexed, owner)?;
        let paths = RunPaths::new(&self.shared.dir, &run_id)?;
        let lock = match OpenOptions::new().read(true).open(&paths.lock) {
            Ok(lock) => lock,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(self.vanished(&run_id, "lock file missing"));
            }
            Err(e) => return Err(e.into()),
        };
        match lock.try_lock_shared() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(RunStoreError::InUse(run_id)),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        // Read again under the lock: a purge or a checkpoint may have
        // landed since.
        let indexed = self.live_run(&run_id)?;
        let bytes = self.read_log(&run_id, &paths)?;
        check_recorded_head(&bytes, &indexed).map_err(|detail| inconsistent(&run_id, detail))?;
        let events = verify_log_bytes(&bytes)?;
        let aggregate = check_run_events(&run_id, &events)?;
        Ok(RunView {
            run_id,
            meta,
            events,
            aggregate,
            _lock: lock,
        })
    }

    /// A live run's sidecar, without opening or locking the run. It does
    /// not check who asks: it is for the operator and the runtime, while
    /// visitor-facing paths use [`read_run`](Self::read_run) and
    /// [`open_run_as`](Self::open_run_as), which do. Unknown and malformed
    /// ids are refused as in [`open_run`](Self::open_run).
    pub fn meta(&self, id: &str) -> Result<RunMeta, RunStoreError> {
        let run_id = parse_run_id(id)?;
        let indexed = self.live_run(&run_id)?;
        self.load_meta(&run_id, &indexed)
    }

    /// The live runs (created, not purged) that `owner` owns, oldest
    /// first, whether or not the owner's access has ended
    /// ([`RunMeta::access_open_at`] tells). A run whose sidecar fails its
    /// checks is left out (recovery reports it), and so is one purged
    /// meanwhile by another process; any other failure (I/O, a poisoned
    /// lock) is an error, never a shorter list.
    pub fn list_for(&self, owner: &Owner) -> Result<Vec<RunMeta>, RunStoreError> {
        let mut runs = Vec::new();
        for (run_id, run) in self.index_view()? {
            if run.purged {
                continue;
            }
            match self.load_meta(&run_id, &run) {
                Ok(meta) if meta.owner == *owner => runs.push(meta),
                Ok(_)
                | Err(RunStoreError::Inconsistent { .. })
                | Err(RunStoreError::UnknownRun(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(runs)
    }

    /// The store's state for a health endpoint: the run index verified
    /// again from its bytes (so an edit made while the store is open
    /// shows, whatever its length), whether a torn index record was cut,
    /// and the last failure to index a run's head. It reads and hashes
    /// the whole index, a few hundred bytes per record: call it at health
    /// check rates, not per request.
    pub fn health(&self) -> StoreHealth {
        let mut health = StoreHealth {
            index_write_error: self.index_write_error(),
            ..StoreHealth::default()
        };
        let verified = self.shared.index().and_then(|mut index| {
            let head = index.reverify();
            health.index_tail_repaired = index.state.repaired_tail;
            head
        });
        match verified {
            Ok(head) => health.index_head = Some(head),
            Err(e) => {
                health.index_tampered = matches!(e, RunStoreError::IndexTampered { .. });
                health.index_error = Some(e.to_string());
            }
        }
        health
    }

    /// Startup recovery: call once after [`open`](Self::open), before
    /// serving. For every indexed run whose lock it can take, it first
    /// reads the run's log without opening it for writing and compares it
    /// with the latest head the index recorded for the run:
    ///
    /// - a log that does not hold that head (records removed or cut, the
    ///   run rewritten and re-hashed, records after a terminal head), that
    ///   does not open or replay, or that holds another run's records is
    ///   reported and left exactly as it is: never repaired, closed again
    ///   or re-indexed;
    /// - a run left mid-deliberation is closed by
    ///   [`mark_interrupted_runs`] on its own file, with event ids no
    ///   stored record has;
    /// - a terminal run whose head is not in the index gets it recorded,
    ///   and so does a run awaiting a decision whose checkpoint is not;
    /// - a bad sidecar is reported, though a run left mid-deliberation is
    ///   still closed;
    /// - a recorded purge whose files remain is finished.
    ///
    /// Runs another live owner or reader holds are skipped, indexed runs
    /// without a log are reported missing, and logs the index never
    /// recorded are reported, never adopted. Only store-level failures
    /// (the run index) are errors.
    pub fn recover(&self) -> Result<RecoveryReport, RunStoreError> {
        self.recover_with(&mut UuidIdGen)
    }

    /// [`recover`](Self::recover), with the generator whose ids recovery's
    /// own records start from. [`FreshIds`] keeps any of them that a
    /// stored record already has from reaching the log.
    fn recover_with<I: IdGen>(&self, ids: &mut I) -> Result<RecoveryReport, RunStoreError> {
        let now = stamp(self.shared.now()?);
        let indexed = self.index_view()?;
        let mut report = RecoveryReport {
            index_tail_repaired: self.shared.index()?.state.repaired_tail,
            ..RecoveryReport::default()
        };
        let (logs, files) = scan_run_files(&self.shared.dir)?;
        for (run_id, run) in &indexed {
            let paths = RunPaths::new(&self.shared.dir, run_id)?;
            if run.purged {
                if paths.any_exist()? {
                    match self.delete_purged(run_id, &paths) {
                        Ok(()) => report.purges_completed.push(run_id.clone()),
                        Err(RunStoreError::InUse(_)) => report.in_use.push(run_id.clone()),
                        Err(e) => report.failed.push((run_id.clone(), e.to_string())),
                    }
                }
                continue;
            }
            if !logs.contains(run_id) {
                report.missing.push(run_id.clone());
                continue;
            }
            self.recover_run(run_id, &paths, ids, &now, &mut report)?;
        }
        let known: BTreeSet<&RunId> = indexed.iter().map(|(run_id, _)| run_id).collect();
        report.unindexed = files
            .iter()
            .filter(|run_id| !known.contains(run_id))
            .cloned()
            .collect();
        Ok(report)
    }

    /// Deletes every expired session-owned run whole (event log, sidecar,
    /// lock file), each after recording its `purged` index record with the
    /// head its log had (read without opening it for writing). A run some
    /// owner or reader still holds is left for the next purge. Operator
    /// and showcase runs are never deleted, whatever their retention says.
    pub fn purge_expired(&self, now: DateTime<Utc>) -> Result<PurgeReport, RunStoreError> {
        let at = stamp(now);
        let mut report = PurgeReport::default();
        for (run_id, run) in self.index_view()? {
            let paths = RunPaths::new(&self.shared.dir, &run_id)?;
            if run.purged {
                // A recorded purge that did not finish deleting.
                if paths.any_exist()? {
                    match self.delete_purged(&run_id, &paths) {
                        Ok(()) => report.purged.push(run_id),
                        Err(RunStoreError::InUse(_)) => report.in_use.push(run_id),
                        Err(e) => report.failed.push((run_id, e.to_string())),
                    }
                }
                continue;
            }
            let meta = match self.load_meta(&run_id, &run) {
                Ok(meta) => meta,
                // Purged meanwhile by another process.
                Err(RunStoreError::UnknownRun(_)) => continue,
                Err(e) => {
                    report.failed.push((run_id, e.to_string()));
                    continue;
                }
            };
            if !meta.purgeable_at(now) {
                continue;
            }
            match self.purge_run(&run_id, &paths, &at) {
                Ok(()) => report.purged.push(run_id),
                Err(RunStoreError::InUse(_)) => report.in_use.push(run_id),
                Err(RunStoreError::UnknownRun(_)) => {}
                Err(e) => report.failed.push((run_id, e.to_string())),
            }
        }
        Ok(report)
    }

    /// The run index's head, after reading other processes' appends.
    pub fn index_head(&self) -> Result<IndexHead, RunStoreError> {
        let mut index = self.shared.index()?;
        index.refresh_if_changed()?;
        Ok(IndexHead {
            seq: index.state.records.len() as u64,
            hash: index.state.head().to_string(),
        })
    }

    /// Every run index record, verified, oldest first.
    pub fn index_records(&self) -> Result<Vec<IndexRecord>, RunStoreError> {
        let mut index = self.shared.index()?;
        index.refresh_if_changed()?;
        Ok(index.state.records.clone())
    }

    /// The last failure to index a run's checkpoint or terminal head while
    /// it was written, if any. The run's own log is unaffected, and
    /// [`recover`](Self::recover) records a missing terminal head, or the
    /// checkpoint of a run awaiting its decision, at the next start.
    pub fn index_write_error(&self) -> Option<String> {
        self.shared
            .index_write_error
            .lock()
            .ok()
            .and_then(|error| error.clone())
    }

    /// What the index says about `run_id`, after reading other processes'
    /// appends (a metadata call when there are none).
    fn indexed(&self, run_id: &RunId) -> Result<Option<IndexedRun>, RunStoreError> {
        let mut index = self.shared.index()?;
        index.refresh_if_changed()?;
        Ok(index.state.runs.get(run_id).cloned())
    }

    /// The index's entry for a live run (created, not purged); any other
    /// id is [`RunStoreError::UnknownRun`], found from the index alone.
    fn live_run(&self, run_id: &RunId) -> Result<IndexedRun, RunStoreError> {
        match self.indexed(run_id)? {
            Some(run) if !run.purged => Ok(run),
            _ => Err(RunStoreError::UnknownRun(run_id.clone())),
        }
    }

    /// The sidecar of a run `owner` owns and may still reach. Anyone else
    /// learns nothing more than for a run that does not exist.
    fn owned_meta(
        &self,
        run_id: &RunId,
        indexed: &IndexedRun,
        owner: &Owner,
    ) -> Result<RunMeta, RunStoreError> {
        let meta = self.load_meta(run_id, indexed)?;
        if meta.owner != *owner || !meta.access_open_at(self.shared.now()?) {
            return Err(RunStoreError::UnknownRun(run_id.clone()));
        }
        Ok(meta)
    }

    /// Takes ownership of a known run (see [`open_run`](Self::open_run)).
    fn take_run(&self, run_id: RunId) -> Result<RunHandle, RunStoreError> {
        let paths = RunPaths::new(&self.shared.dir, &run_id)?;
        let lock = match OpenOptions::new().read(true).write(true).open(&paths.lock) {
            Ok(lock) => lock,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(self.vanished(&run_id, "lock file missing"));
            }
            Err(e) => return Err(e.into()),
        };
        acquire_run_lock(&lock, &run_id)?;
        // From here the lock is held: nothing else appends to, purges or
        // recovers the run until this handle lets it go. What the index
        // says is read again, now that no one else can add to it for this
        // run.
        let indexed = self.live_run(&run_id)?;
        let meta = self.load_meta(&run_id, &indexed)?;
        // The bytes are checked before the log is opened for writing,
        // which could cut a torn record off a log that contradicts the
        // index.
        let bytes = self.read_log(&run_id, &paths)?;
        check_recorded_head(&bytes, &indexed).map_err(|detail| inconsistent(&run_id, detail))?;
        let log = EventLog::open(&paths.log)?;
        check_run_log(&run_id, &log)?;
        Ok(RunHandle {
            run_id: run_id.clone(),
            meta,
            log,
            lease: Arc::new(RunLease {
                run_id,
                _lock: lock,
                store: self.shared.clone(),
            }),
        })
    }

    /// A run log's bytes, read without opening it for writing. A log that
    /// is missing or not a regular file is the run's files vanishing.
    fn read_log(&self, run_id: &RunId, paths: &RunPaths) -> Result<Vec<u8>, RunStoreError> {
        if !is_regular_file(&paths.log)? {
            return Err(self.vanished(run_id, "event log missing or not a regular file"));
        }
        Ok(std::fs::read(&paths.log)?)
    }

    /// Every indexed run in creation order.
    fn index_view(&self) -> Result<Vec<(RunId, IndexedRun)>, RunStoreError> {
        let mut index = self.shared.index()?;
        index.refresh_if_changed()?;
        let mut runs: Vec<(RunId, IndexedRun)> = index
            .state
            .runs
            .iter()
            .map(|(run_id, run)| (run_id.clone(), run.clone()))
            .collect();
        runs.sort_by_key(|(_, run)| run.created_seq);
        Ok(runs)
    }

    /// The error for a known run whose files are gone: unknown when it
    /// was purged meanwhile (by another process), inconsistent otherwise.
    fn vanished(&self, run_id: &RunId, detail: &str) -> RunStoreError {
        match self.indexed(run_id) {
            Ok(Some(run)) if !run.purged => inconsistent(run_id, detail),
            Ok(_) => RunStoreError::UnknownRun(run_id.clone()),
            Err(e) => e,
        }
    }

    /// A live run's sidecar, read and checked once, then cached: it is
    /// never rewritten. Its bytes must be the ones the run's `created`
    /// index record committed to, so an edit of the owner or the retention
    /// is refused however it is formatted.
    fn load_meta(&self, run_id: &RunId, indexed: &IndexedRun) -> Result<RunMeta, RunStoreError> {
        if let Some(meta) = self.shared.catalog()?.get(run_id) {
            return Ok(meta.clone());
        }
        let paths = RunPaths::new(&self.shared.dir, run_id)?;
        if !is_regular_file(&paths.meta)? {
            return Err(self.vanished(run_id, "sidecar missing or not a regular file"));
        }
        let bytes = std::fs::read(&paths.meta)?;
        if sha256_hex(&bytes) != indexed.meta_sha256 {
            return Err(inconsistent(
                run_id,
                "the sidecar is not the one the run index recorded when the run was created",
            ));
        }
        let meta = read_meta(&bytes, run_id).map_err(|detail| inconsistent(run_id, detail))?;
        self.shared.catalog()?.insert(run_id.clone(), meta.clone());
        Ok(meta)
    }

    fn recover_run<I: IdGen>(
        &self,
        run_id: &RunId,
        paths: &RunPaths,
        ids: &mut I,
        now: &str,
        report: &mut RecoveryReport,
    ) -> Result<(), RunStoreError> {
        let lock = match open_lock_for_known_run(paths) {
            Ok(lock) => lock,
            Err(e) => {
                report.failed.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        match acquire_run_lock(&lock, run_id) {
            Ok(()) => {}
            Err(RunStoreError::InUse(_)) => {
                report.in_use.push(run_id.clone());
                return Ok(());
            }
            Err(e) => {
                report.failed.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        }
        // Read the index again now that the lock is held: the run may have
        // ended, or been purged, in another process since recovery began.
        let indexed = match self.indexed(run_id)? {
            Some(run) if !run.purged => run,
            _ => return Ok(()),
        };
        if let Err(e) = self.load_meta(run_id, &indexed) {
            // A bad sidecar keeps the run from being served, but a run left
            // mid-deliberation is still closed.
            report.inconsistent.push((run_id.clone(), e.to_string()));
        }
        // Read before anything is written: a log that does not hold the
        // head the index recorded is evidence, so it is never repaired
        // (EventLog::open would cut a torn record), closed again or
        // re-indexed.
        let bytes = match std::fs::read(&paths.log) {
            Ok(bytes) => bytes,
            Err(e) => {
                report.failed.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        if let Err(detail) = check_recorded_head(&bytes, &indexed) {
            report
                .inconsistent
                .push((run_id.clone(), detail.to_string()));
            return Ok(());
        }
        let mut log = match EventLog::open(&paths.log) {
            Ok(log) => log,
            Err(e) => {
                report.unreplayable.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        if log.recovered_truncated_tail {
            report.truncated_tails.push(run_id.clone());
        }
        match check_run_log(run_id, &log) {
            Ok(()) => {}
            Err(RunStoreError::Log(e)) => {
                report.unreplayable.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
            Err(e) => {
                report.inconsistent.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        }
        if indexed.terminal {
            // Terminal, indexed, and the log ends at that head: nothing to
            // write.
            return Ok(());
        }
        let mut fresh = FreshIds::new(ids, &log);
        let recovery = match mark_interrupted_runs(&mut log, &mut fresh, now.to_string()) {
            Ok(recovery) => recovery,
            Err(e) => {
                report.failed.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        report.interrupted.extend(recovery.interrupted);
        report.unreplayable.extend(recovery.unreplayable);
        let state = match log.rebuild(run_id) {
            Ok(agg) => agg.state,
            Err(e) => {
                report.unreplayable.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        let kind = if state.is_terminal() {
            IndexKind::Terminal
        } else if state == RunState::AwaitingHumanApproval {
            IndexKind::Checkpoint
        } else {
            // Still empty: created, its task never recorded. Nothing to
            // close and no head to record.
            return Ok(());
        };
        let Some(head) = log.all_events().last().and_then(Head::of) else {
            report
                .inconsistent
                .push((run_id.clone(), "a run at rest without a chain head".into()));
            return Ok(());
        };
        if self
            .shared
            .index()?
            .record_head(run_id, kind, head, now.to_string())?
        {
            match kind {
                IndexKind::Terminal => report.terminal_recorded.push(run_id.clone()),
                _ => report.checkpoints_recorded.push(run_id.clone()),
            }
        }
        Ok(())
    }

    fn purge_run(&self, run_id: &RunId, paths: &RunPaths, at: &str) -> Result<(), RunStoreError> {
        let lock = open_lock_for_known_run(paths)?;
        acquire_run_lock(&lock, run_id)?;
        let indexed = self.live_run(run_id)?;
        // The head the log had, read without opening it for writing. A log
        // that no longer verifies, or contradicts the index, is deleted all
        // the same (retention is a promise to the visitor), with no head
        // recorded.
        let head = is_regular_file(&paths.log)
            .ok()
            .filter(|regular| *regular)
            .and_then(|_| std::fs::read(&paths.log).ok())
            .filter(|bytes| check_recorded_head(bytes, &indexed).is_ok())
            .and_then(|bytes| verify_log_bytes(&bytes).ok())
            .and_then(|events| events.last().and_then(Head::of));
        // Recorded before anything is deleted: a crash in between leaves a
        // recorded purge, which the next purge or recovery finishes, never
        // a run that looks deleted by hand.
        self.shared
            .index()?
            .append(run_id, IndexKind::Purged, head, None, at.to_string())?;
        if let Ok(mut catalog) = self.shared.catalog() {
            catalog.remove(run_id);
        }
        remove_run_files(paths)?;
        Ok(())
    }

    fn delete_purged(&self, run_id: &RunId, paths: &RunPaths) -> Result<(), RunStoreError> {
        let lock = open_lock_for_known_run(paths)?;
        acquire_run_lock(&lock, run_id)?;
        if let Ok(mut catalog) = self.shared.catalog() {
            catalog.remove(run_id);
        }
        remove_run_files(paths)?;
        Ok(())
    }
}

/// A run this process owns: its sidecar, its open event log and its
/// exclusive lock. Read it here, or turn it into a [`SharedRunLog`] to
/// write. The lock is held until the handle, or the last clone of the
/// shared log made from it, is dropped; meanwhile every other owner and
/// every reader ([`RunStore::read_run`]) gets [`RunStoreError::InUse`].
pub struct RunHandle {
    run_id: RunId,
    meta: RunMeta,
    log: EventLog,
    lease: Arc<RunLease>,
}

impl std::fmt::Debug for RunHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunHandle")
            .field("run_id", &self.run_id)
            .field("meta", &self.meta)
            .field("events", &self.log.len())
            .finish_non_exhaustive()
    }
}

impl RunHandle {
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn meta(&self) -> &RunMeta {
        &self.meta
    }

    pub fn log(&self) -> &EventLog {
        &self.log
    }

    /// The run's validating, publishing writer. It keeps the run's lock
    /// and records the run's head in the run index when an append leaves
    /// the run at a checkpoint (its task recorded, awaiting a decision) or
    /// ends it.
    pub fn into_shared(self, publisher: Arc<dyn EventPublisher>) -> SharedRunLog {
        SharedRunLog::leased(self.run_id, self.log, publisher, self.lease)
    }
}

/// A stored run, read without taking ownership (see
/// [`RunStore::read_run`]): its sidecar and its verified records. The
/// run's lock is held shared while the view lives, so no owner, recovery
/// or purge changes the run meanwhile, and any number of views of one run
/// can live at once.
pub struct RunView {
    run_id: RunId,
    meta: RunMeta,
    events: Vec<ConsensusEvent>,
    aggregate: RunAggregate,
    /// `<run_id>.lock`, locked shared until the view is dropped.
    _lock: File,
}

impl std::fmt::Debug for RunView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunView")
            .field("run_id", &self.run_id)
            .field("meta", &self.meta)
            .field("events", &self.events.len())
            .field("state", &self.aggregate.state)
            .finish_non_exhaustive()
    }
}

impl RunView {
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn meta(&self) -> &RunMeta {
        &self.meta
    }

    /// Every record of the run, verified, in sequence order.
    pub fn events(&self) -> &[ConsensusEvent] {
        &self.events
    }

    /// The records with `sequence > after`, as
    /// [`SharedRunLog::snapshot_from`] gives them for a live run.
    pub fn snapshot_from(&self, after: u64) -> &[ConsensusEvent] {
        // Sequences run 1..=n with no gaps, so record `after + 1` is at
        // index `after`.
        let start = usize::try_from(after).unwrap_or(self.events.len());
        &self.events[start.min(self.events.len())..]
    }

    /// The run's aggregate, replayed from its records.
    pub fn aggregate(&self) -> &RunAggregate {
        &self.aggregate
    }

    /// The hash of the run's last record, or None while it has none.
    pub fn head_hash(&self) -> Option<&str> {
        self.events
            .last()
            .map(|e| e.hash.as_str())
            .filter(|h| !h.is_empty())
    }
}

/// Ownership of one run: its `<run_id>.lock` file, locked. The OS
/// releases the lock when the file is closed, so dropping the lease lets
/// the run go, and a process that dies never keeps it.
pub(crate) struct RunLease {
    run_id: RunId,
    _lock: File,
    store: Arc<StoreShared>,
}

impl RunLease {
    /// Records the run's head (`last`, already durable) in the run index
    /// as a `kind` record: [`IndexKind::Checkpoint`] or
    /// [`IndexKind::Terminal`]. The caller holds the run's lock, so a run's
    /// index records follow its sequence order. The record is acknowledged
    /// by then, so a failure here is not an append failure: it is kept for
    /// [`RunStore::index_write_error`], and recovery records a missing
    /// terminal head, or the checkpoint of a run awaiting its decision, at
    /// the next start.
    pub(crate) fn record_head(&self, kind: IndexKind, last: &ConsensusEvent) {
        let recorded = Head::of(last)
            .ok_or_else(|| inconsistent(&self.run_id, "the record has no chain hash to record"))
            .and_then(|head| {
                let now = self.store.now()?;
                self.store
                    .index()?
                    .record_head(&self.run_id, kind, head, stamp(now))
            });
        if let Err(e) = recorded {
            if let Ok(mut slot) = self.store.index_write_error.lock() {
                *slot = Some(format!(
                    "run {}: {} head not indexed: {e}",
                    self.run_id,
                    kind.as_str()
                ));
            }
        }
    }
}

/// State shared by a store and the leases of the runs it handed out.
struct StoreShared {
    dir: PathBuf,
    clock: Box<dyn Clock + Send + Sync>,
    index: Mutex<RunIndex>,
    /// Sidecars read so far. A sidecar is never rewritten, so an entry
    /// stays valid until its run is purged.
    catalog: Mutex<BTreeMap<RunId, RunMeta>>,
    index_write_error: Mutex<Option<String>>,
}

impl StoreShared {
    fn index(&self) -> Result<MutexGuard<'_, RunIndex>, RunStoreError> {
        self.index
            .lock()
            .map_err(|_| RunStoreError::Poisoned("run index"))
    }

    fn catalog(&self) -> Result<MutexGuard<'_, BTreeMap<RunId, RunMeta>>, RunStoreError> {
        self.catalog
            .lock()
            .map_err(|_| RunStoreError::Poisoned("run catalog"))
    }

    fn now(&self) -> Result<DateTime<Utc>, RunStoreError> {
        let now = self.clock.now();
        parse_time(&now).map_err(|_| RunStoreError::Clock(now))
    }
}

/// `runs/index.jsonl`, and the lock file that serializes its updates
/// across processes. Every read-modify cycle holds that lock and first
/// reads what other processes appended.
#[derive(Debug)]
struct RunIndex {
    path: PathBuf,
    lock: File,
    state: IndexState,
}

impl RunIndex {
    /// Opens `dir`'s index, starting an empty one only where `missing`
    /// allows it (see [`MissingIndex`]).
    fn open(dir: &Path, missing: MissingIndex) -> Result<Self, RunStoreError> {
        let path = dir.join(INDEX_FILE);
        if !exists(&path)? {
            if missing == MissingIndex::Refuse {
                if let Some(found) = store_traces(dir)? {
                    return Err(RunStoreError::IndexMissing { found });
                }
            }
            // Made before its lock file, so a crash in between never leaves
            // a lock file without an index, which the next open would take
            // for a lost index.
            OpenOptions::new().append(true).create(true).open(&path)?;
            sync_dir(Some(dir))?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(INDEX_LOCK_FILE))?;
        let mut index = Self {
            path,
            lock,
            state: IndexState::default(),
        };
        {
            let _held = FileLock::acquire(&index.lock)?;
            index.state.refresh(&index.path)?;
        }
        Ok(index)
    }

    /// Reads other processes' appends when the file's length moved: one
    /// metadata call when it did not. A file that is gone is an error: it
    /// is never made again behind the store's back.
    fn refresh_if_changed(&mut self) -> Result<(), RunStoreError> {
        if std::fs::metadata(&self.path)?.len() != self.state.verified_len {
            let _held = FileLock::acquire(&self.lock)?;
            self.state.refresh(&self.path)?;
        }
        Ok(())
    }

    fn append(
        &mut self,
        run_id: &RunId,
        kind: IndexKind,
        head: Option<Head>,
        meta_sha256: Option<String>,
        at: String,
    ) -> Result<IndexRecord, RunStoreError> {
        let _held = FileLock::acquire(&self.lock)?;
        self.state.refresh(&self.path)?;
        self.state
            .append(&self.path, run_id, kind, head, meta_sha256, at)
    }

    /// Records `head` as `run_id`'s latest `kind` record (checkpoint or
    /// terminal) unless it is already recorded as such. Returns whether a
    /// record was written. A head that goes back, or one after the
    /// terminal head, is refused.
    fn record_head(
        &mut self,
        run_id: &RunId,
        kind: IndexKind,
        head: Head,
        at: String,
    ) -> Result<bool, RunStoreError> {
        let _held = FileLock::acquire(&self.lock)?;
        self.state.refresh(&self.path)?;
        if let Some(run) = self.state.runs.get(run_id) {
            let same_kind = run.terminal == (kind == IndexKind::Terminal);
            if same_kind && run.head.as_ref() == Some(&head) {
                return Ok(false);
            }
        }
        self.state
            .append(&self.path, run_id, kind, Some(head), None, at)?;
        Ok(true)
    }

    /// Verifies the whole index again from its bytes, without writing,
    /// and checks that every record this view verified before is still
    /// there unchanged: an edit of any length, or a consistent rewrite of
    /// the chain, is [`RunStoreError::IndexTampered`]. Returns the head.
    fn reverify(&mut self) -> Result<IndexHead, RunStoreError> {
        self.refresh_if_changed()?;
        let _held = FileLock::acquire(&self.lock)?;
        let bytes = std::fs::read(&self.path)?;
        let mut fresh = IndexState::default();
        match fresh.absorb(&bytes)? {
            IndexTail::Unterminated(record, len) => fresh.commit(record, len),
            // A torn final record is what a crash leaves, not an edit; the
            // next refresh cuts it.
            IndexTail::Clean | IndexTail::Torn => {}
        }
        let known = &self.state.records;
        if let Some(line) = known
            .iter()
            .zip(&fresh.records)
            .position(|(known, now)| known != now)
            .or((fresh.records.len() < known.len()).then_some(fresh.records.len()))
        {
            return Err(RunStoreError::IndexTampered {
                line: line + 1,
                detail: "a record verified earlier is no longer the same on disk".into(),
            });
        }
        Ok(IndexHead {
            seq: fresh.records.len() as u64,
            hash: fresh.head().to_string(),
        })
    }
}

/// The verified in-memory view of `index.jsonl`.
#[derive(Debug, Default)]
struct IndexState {
    records: Vec<IndexRecord>,
    runs: BTreeMap<RunId, IndexedRun>,
    /// Bytes of the file that `records` cover, newlines included.
    verified_len: u64,
    /// A torn final record (a crash mid-append) was removed.
    repaired_tail: bool,
    /// Set when a failed append could not be rolled back: the file may
    /// end in a partial record, so this view appends nothing more.
    poisoned: Option<String>,
}

/// What the run index says about one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct IndexedRun {
    created_seq: u64,
    /// The SHA-256 of the sidecar, from the `created` record.
    meta_sha256: String,
    /// The latest head recorded for the run: its last checkpoint, or its
    /// terminal head once that is recorded.
    head: Option<Head>,
    terminal: bool,
    purged: bool,
}

/// A run log's head as the index records it: the last record's sequence
/// and hash.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Head {
    sequence: u64,
    hash: String,
}

impl Head {
    /// The head that `last`, a stored record, makes; None for a record
    /// without a chain hash (pre-1.1).
    fn of(last: &ConsensusEvent) -> Option<Self> {
        (!last.hash.is_empty()).then(|| Self {
            sequence: last.sequence,
            hash: last.hash.clone(),
        })
    }
}

/// How the index bytes [`IndexState::absorb`] was given end, after their
/// last complete record.
enum IndexTail {
    /// Nothing follows the last newline.
    Clean,
    /// A final record without its newline that parses and verifies, not
    /// yet committed, and its length.
    Unterminated(IndexRecord, u64),
    /// A final chunk without a newline that does not parse: a write cut
    /// short.
    Torn,
}

impl IndexState {
    fn head(&self) -> &str {
        self.records
            .last()
            .map_or(INDEX_GENESIS, |record| record.hash.as_str())
    }

    /// Reads and verifies what was appended since the last call,
    /// continuing the verified chain. The caller holds the index's file
    /// lock, so no writer is mid-append: an unterminated final record
    /// that does not parse is a torn write from a crash and is cut off
    /// (and reported in `repaired_tail`), and one that parses and verifies
    /// only lost its newline.
    fn refresh(&mut self, path: &Path) -> Result<(), RunStoreError> {
        if let Some(why) = &self.poisoned {
            return Err(index_unusable(why));
        }
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let len = file.metadata()?.len();
        if len < self.verified_len {
            return Err(RunStoreError::IndexTampered {
                line: self.records.len(),
                detail: "the index is shorter than the part already verified".into(),
            });
        }
        if len == self.verified_len {
            return Ok(());
        }
        file.seek(SeekFrom::Start(self.verified_len))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        match self.absorb(&bytes)? {
            IndexTail::Clean => {}
            IndexTail::Unterminated(record, len) => {
                file.seek(SeekFrom::End(0))?;
                file.write_all(b"\n")?;
                file.sync_all()?;
                self.commit(record, len + 1);
            }
            IndexTail::Torn => {
                file.set_len(self.verified_len)?;
                file.sync_all()?;
                self.repaired_tail = true;
            }
        }
        Ok(())
    }

    /// Verifies and commits every newline-terminated record in `bytes`,
    /// which continue the verified part of the file, and says how they
    /// end. Writes nothing.
    fn absorb(&mut self, bytes: &[u8]) -> Result<IndexTail, RunStoreError> {
        let mut rest = bytes;
        while !rest.is_empty() {
            let line = self.records.len() + 1;
            let tampered = move |detail: String| RunStoreError::IndexTampered { line, detail };
            let Some(end) = rest.iter().position(|b| *b == b'\n') else {
                return Ok(match parse_index_line(rest) {
                    Ok(record) => {
                        self.verify(&record, rest).map_err(tampered)?;
                        IndexTail::Unterminated(record, rest.len() as u64)
                    }
                    Err(_) => IndexTail::Torn,
                });
            };
            let record = parse_index_line(&rest[..end]).map_err(tampered)?;
            self.verify(&record, &rest[..end]).map_err(tampered)?;
            self.commit(record, end as u64 + 1);
            rest = &rest[end + 1..];
        }
        Ok(IndexTail::Clean)
    }

    fn verify(&self, record: &IndexRecord, raw: &[u8]) -> Result<(), String> {
        let expected = self.records.len() as u64 + 1;
        if record.seq != expected {
            return Err(format!(
                "sequence {} where {expected} was expected",
                record.seq
            ));
        }
        if record.prev_hash != self.head() {
            return Err("prev_hash does not link to the previous record".into());
        }
        if serde_json::to_vec(record).map_err(|e| e.to_string())? != raw {
            return Err("record is not in the canonical form this store writes".into());
        }
        if index_hash(record).map_err(|e| e.to_string())? != record.hash {
            return Err("hash mismatch".into());
        }
        if parse_run_id(record.run_id.as_str()).is_err() {
            return Err("invalid run id".into());
        }
        if parse_time(&record.at).is_err() {
            return Err("`at` is not an RFC 3339 time".into());
        }
        let head = match (record.head_sequence, &record.head_hash) {
            (None, None) => None,
            (Some(sequence), Some(hash)) => Some(Head {
                sequence,
                hash: hash.clone(),
            }),
            _ => return Err("head_sequence and head_hash come together".into()),
        };
        self.check(
            &record.run_id,
            record.kind,
            head.as_ref(),
            record.meta_sha256.as_deref(),
        )
        .map_err(String::from)
    }

    /// The order a run's records may come in: created once and first,
    /// with its sidecar's digest and no head; then checkpoints whose heads
    /// move forward; at most one terminal head, after them; purged once,
    /// last, with a head (if any) no older than the latest recorded one.
    fn check(
        &self,
        run_id: &RunId,
        kind: IndexKind,
        head: Option<&Head>,
        meta_sha256: Option<&str>,
    ) -> Result<(), &'static str> {
        if let Some(head) = head {
            if head.sequence == 0 {
                return Err("head_sequence starts at 1");
            }
            if !is_sha256_hex(&head.hash) {
                return Err("head_hash is not a SHA-256 hex digest");
            }
        }
        match (kind, meta_sha256) {
            (IndexKind::Created, Some(digest)) if !is_sha256_hex(digest) => {
                return Err("meta_sha256 is not a SHA-256 hex digest");
            }
            (IndexKind::Created, Some(_)) => {}
            (IndexKind::Created, None) => {
                return Err("a created record needs its sidecar's digest")
            }
            (_, Some(_)) => return Err("only a created record carries a sidecar digest"),
            (_, None) => {}
        }
        let recorded = |run: &IndexedRun| run.head.as_ref().map_or(0, |head| head.sequence);
        match (kind, self.runs.get(run_id)) {
            (IndexKind::Created, None) if head.is_some() => {
                Err("a created record has no head: the run's log is still empty")
            }
            (IndexKind::Created, None) => Ok(()),
            (IndexKind::Created, Some(_)) => Err("the run is already recorded as created"),
            (_, None) => Err("the run was never recorded as created"),
            (_, Some(run)) if run.purged => Err("the run is already recorded as purged"),
            (IndexKind::Checkpoint | IndexKind::Terminal, Some(run)) if run.terminal => {
                Err("the run's terminal head is already recorded")
            }
            (IndexKind::Checkpoint | IndexKind::Terminal, Some(run)) => match head {
                None => Err("a checkpoint or terminal record needs a head"),
                Some(head) if head.sequence <= recorded(run) => {
                    Err("the head goes back: it must follow the run's latest recorded head")
                }
                Some(_) => Ok(()),
            },
            (IndexKind::Purged, Some(run)) => match (head, &run.head) {
                (Some(head), Some(latest)) if head.sequence < latest.sequence => {
                    Err("the purged head is older than the run's latest recorded head")
                }
                (Some(head), Some(latest))
                    if head.sequence == latest.sequence && head.hash != latest.hash =>
                {
                    Err("the purged head contradicts the run's latest recorded head")
                }
                _ => Ok(()),
            },
        }
    }

    fn commit(&mut self, record: IndexRecord, bytes: u64) {
        let run = self.runs.entry(record.run_id.clone()).or_default();
        let head = record
            .head_sequence
            .zip(record.head_hash.clone())
            .map(|(sequence, hash)| Head { sequence, hash });
        match record.kind {
            IndexKind::Created => {
                run.created_seq = record.seq;
                run.meta_sha256 = record.meta_sha256.clone().unwrap_or_default();
            }
            IndexKind::Checkpoint => run.head = head,
            IndexKind::Terminal => {
                run.head = head;
                run.terminal = true;
            }
            IndexKind::Purged => run.purged = true,
        }
        self.verified_len += bytes;
        self.records.push(record);
    }

    /// Appends one record. The caller holds the file lock and has just
    /// refreshed, so the file ends where `verified_len` says.
    fn append(
        &mut self,
        path: &Path,
        run_id: &RunId,
        kind: IndexKind,
        head: Option<Head>,
        meta_sha256: Option<String>,
        at: String,
    ) -> Result<IndexRecord, RunStoreError> {
        if let Some(why) = &self.poisoned {
            return Err(index_unusable(why));
        }
        self.check(run_id, kind, head.as_ref(), meta_sha256.as_deref())
            .map_err(|detail| {
                inconsistent(
                    run_id,
                    format!("the run index refuses a {} record: {detail}", kind.as_str()),
                )
            })?;
        let (head_sequence, head_hash) = match head {
            Some(head) => (Some(head.sequence), Some(head.hash)),
            None => (None, None),
        };
        let mut record = IndexRecord {
            seq: self.records.len() as u64 + 1,
            run_id: run_id.clone(),
            kind,
            head_sequence,
            head_hash,
            meta_sha256,
            at,
            prev_hash: self.head().to_string(),
            hash: String::new(),
        };
        record.hash = index_hash(&record)?;
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        let mut file = OpenOptions::new().write(true).open(path)?;
        if file.seek(SeekFrom::End(0))? != self.verified_len {
            return Err(RunStoreError::IndexTampered {
                line: self.records.len() + 1,
                detail: "the index changed while its lock was held".into(),
            });
        }
        if let Err(e) = write_index_record(&mut file, &line) {
            // Roll the partial record back. If that fails too, the file
            // may end in one, so stop appending through this view.
            let rolled_back = file
                .set_len(self.verified_len)
                .and_then(|()| file.sync_all());
            if let Err(rollback) = rolled_back {
                self.poisoned = Some(format!(
                    "an append failed ({e}) and its rollback failed ({rollback})"
                ));
            }
            return Err(e.into());
        }
        self.commit(record.clone(), line.len() as u64);
        Ok(record)
    }
}

/// A run's file names, built only from an id [`parse_run_id`] accepts.
struct RunPaths {
    log: PathBuf,
    meta: PathBuf,
    meta_tmp: PathBuf,
    lock: PathBuf,
}

impl RunPaths {
    fn new(dir: &Path, run_id: &RunId) -> Result<Self, InvalidRunId> {
        let run_id = parse_run_id(run_id.as_str())?;
        let file = |suffix: &str| dir.join(format!("{run_id}{suffix}"));
        Ok(Self {
            log: file(".jsonl"),
            meta: file(".meta.json"),
            meta_tmp: file(".meta.json.tmp"),
            lock: file(".lock"),
        })
    }

    fn any_exist(&self) -> io::Result<bool> {
        for path in [&self.log, &self.meta, &self.meta_tmp, &self.lock] {
            if exists(path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Removes the files of a creation that did not finish.
struct Undo(Vec<PathBuf>);

impl Drop for Undo {
    fn drop(&mut self) {
        for path in self.0.iter().rev() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Holds a file's exclusive OS lock until dropped.
struct FileLock<'a>(&'a File);

impl<'a> FileLock<'a> {
    fn acquire(file: &'a File) -> io::Result<Self> {
        file.lock()?;
        Ok(Self(file))
    }
}

impl Drop for FileLock<'_> {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn acquire_run_lock(lock: &File, run_id: &RunId) -> Result<(), RunStoreError> {
    match lock.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(RunStoreError::InUse(run_id.clone())),
        Err(TryLockError::Error(e)) => Err(e.into()),
    }
}

/// Opens (creating if needed) the lock file of a run the index lists.
/// Only recovery and purge do this; lookups never create a lock file.
fn open_lock_for_known_run(paths: &RunPaths) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&paths.lock)
}

/// Deletes a run's files whole: log, sidecar, any temporary sidecar and,
/// last, the lock file, whose lock the caller holds.
fn remove_run_files(paths: &RunPaths) -> io::Result<()> {
    for path in [&paths.log, &paths.meta, &paths.meta_tmp, &paths.lock] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    sync_dir(paths.log.parent())
}

/// The cross-checks between a run's file name and its records: every
/// record and the task name this run, and the history replays.
fn check_run_log(run_id: &RunId, log: &EventLog) -> Result<(), RunStoreError> {
    check_run_events(run_id, log.all_events())?;
    Ok(())
}

/// [`check_run_log`] over records already read, returning the replayed
/// aggregate. A replay failure is [`RunStoreError::Log`], as
/// [`EventLog::rebuild`] reports it.
fn check_run_events(
    run_id: &RunId,
    events: &[ConsensusEvent],
) -> Result<RunAggregate, RunStoreError> {
    if events
        .iter()
        .any(|event| names_another_run(run_id, &event.run_id, &event.payload))
    {
        return Err(inconsistent(
            run_id,
            "the event log holds another run's records",
        ));
    }
    let mut aggregate = RunAggregate::new(run_id.clone());
    for event in events {
        aggregate
            .apply(event)
            .map_err(|e| RunStoreError::Log(e.into()))?;
    }
    Ok(aggregate)
}

/// Compares a run log's bytes, read without opening it for writing, with
/// the latest head the run index recorded for the run (LC-12): the record
/// at that sequence must be there whole (newline included) with that
/// hash, and a terminal head must be the log's last record. A run with no
/// recorded head yet passes. Nothing is parsed beyond that record; the
/// caller verifies the rest.
fn check_recorded_head(bytes: &[u8], indexed: &IndexedRun) -> Result<(), &'static str> {
    let Some(head) = &indexed.head else {
        return Ok(());
    };
    let mut start = 0usize;
    for sequence in 1..=head.sequence {
        let Some(len) = bytes[start..].iter().position(|b| *b == b'\n') else {
            return Err(HEAD_MISSING);
        };
        if sequence == head.sequence {
            let line = &bytes[start..start + len];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let Ok(record) = serde_json::from_slice::<ConsensusEvent>(line) else {
                return Err(HEAD_MISMATCH);
            };
            if record.sequence != head.sequence || record.hash != head.hash {
                return Err(HEAD_MISMATCH);
            }
            if indexed.terminal && start + len + 1 != bytes.len() {
                return Err(AFTER_TERMINAL);
            }
        }
        start += len + 1;
    }
    Ok(())
}

/// Verifies a run log from its bytes alone, never opening the file for
/// writing: the checks [`EventLog::open`] makes (readable schema
/// versions, sequences from 1 without gaps, unique event ids, the hash
/// chain from genesis, with its pre-1.1 legacy prefix rule), except that
/// a torn final record is refused instead of cut. A complete final record
/// without its newline is accepted, as `EventLog::open` accepts it.
///
/// This restates the event log's rules for a read that must not write;
/// a test holds it to `EventLog::open` record for record. Once the event
/// log offers a read-only open, that should replace it.
fn verify_log_bytes(bytes: &[u8]) -> Result<Vec<ConsensusEvent>, EventLogError> {
    let mut events: Vec<ConsensusEvent> = Vec::new();
    let mut ids: BTreeSet<EventId> = BTreeSet::new();
    if bytes.is_empty() {
        return Ok(events);
    }
    let ends_with_newline = bytes.last() == Some(&b'\n');
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.split('\n').collect();
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
        let ev = match serde_json::from_str::<ConsensusEvent>(trimmed) {
            Ok(ev) => ev,
            Err(e) if is_last_chunk && !ends_with_newline => {
                return Err(EventLogError::Corrupt {
                    line: line_no,
                    detail: format!(
                        "incomplete final record (a torn write), which a read never repairs: {e}"
                    ),
                });
            }
            Err(e) => {
                return Err(EventLogError::Corrupt {
                    line: line_no,
                    detail: e.to_string(),
                });
            }
        };
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
        if !ids.insert(ev.event_id.clone()) {
            return Err(EventLogError::DuplicateStoredId { line: line_no });
        }
        if let Some(detail) = chain_link_problem(events.last(), &ev)? {
            return Err(EventLogError::HashChain {
                line: line_no,
                detail,
            });
        }
        events.push(ev);
    }
    Ok(events)
}

/// Why `ev` breaks the event log's chain after `prev` (None: the log's
/// first record), or None when it links. Records without a hash are a
/// legacy (pre-1.1) prefix only: schema "1.0", no `prev_hash`, and no
/// chained record before them.
fn chain_link_problem(
    prev: Option<&ConsensusEvent>,
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
        if prev.is_some_and(|p| !p.hash.is_empty()) {
            return Ok(Some("unhashed record after hash-chain start".into()));
        }
        return Ok(None);
    }
    let expected = match prev {
        None => LOG_GENESIS.to_string(),
        Some(p) if p.hash.is_empty() => format!("pre-1.1:{}", p.sequence),
        Some(p) => p.hash.clone(),
    };
    if ev.prev_hash != expected {
        return Ok(Some(format!(
            "prev_hash mismatch: stored {:?}, expected {expected:?}",
            ev.prev_hash
        )));
    }
    let computed = event_hash(ev)?;
    if computed != ev.hash {
        return Ok(Some(format!(
            "hash mismatch: stored {}, computed {computed}",
            ev.hash
        )));
    }
    Ok(None)
}

/// SHA-256 over (`prev_hash` || canonical JSON of the record with `hash`
/// blanked), lowercase hex: how the event log hashes a record.
fn event_hash(event: &ConsensusEvent) -> Result<String, serde_json::Error> {
    let mut blanked = event.clone();
    blanked.hash.clear();
    let mut hasher = Sha256::new();
    hasher.update(event.prev_hash.as_bytes());
    hasher.update(serde_json::to_vec(&blanked)?);
    Ok(hex(&hasher.finalize()))
}

/// Event ids for the records recovery writes: `inner`'s ids, with a
/// counter appended to any that a record of the run's log already has.
/// [`EventLog::append`] takes an id it has stored for a retry and returns
/// the stored record instead of writing, so a repeated id would leave an
/// interrupted run unclosed (RT-8).
struct FreshIds<'a, I> {
    inner: &'a mut I,
    taken: BTreeSet<String>,
}

impl<'a, I: IdGen> FreshIds<'a, I> {
    fn new(inner: &'a mut I, log: &EventLog) -> Self {
        Self {
            inner,
            taken: log
                .all_events()
                .iter()
                .map(|event| event.event_id.as_str().to_string())
                .collect(),
        }
    }
}

impl<I: IdGen> IdGen for FreshIds<'_, I> {
    fn next(&mut self, prefix: &str) -> String {
        let id = self.inner.next(prefix);
        let mut fresh = id.clone();
        let mut n = 0u64;
        while self.taken.contains(&fresh) {
            n += 1;
            fresh = format!("{id}-{n}");
        }
        self.taken.insert(fresh.clone());
        fresh
    }
}

/// The sidecar's exact bytes: compact JSON and a newline.
fn meta_bytes(meta: &RunMeta) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(meta)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Parses a sidecar and checks it against the run it should describe.
/// Only the exact bytes [`meta_bytes`] writes are accepted, so an added
/// field, a reformatting or a hand edit of any kind is refused.
fn read_meta(bytes: &[u8], run_id: &RunId) -> Result<RunMeta, String> {
    let meta: RunMeta =
        serde_json::from_slice(bytes).map_err(|_| "the sidecar does not parse".to_string())?;
    if meta_bytes(&meta).map_err(|e| e.to_string())? != bytes {
        return Err("the sidecar is not in the canonical form this store writes".into());
    }
    if meta.version != META_VERSION {
        return Err(format!("unsupported sidecar version {}", meta.version));
    }
    if meta.run_id != *run_id {
        return Err("the sidecar names another run".into());
    }
    let created = parse_time(&meta.created_at)
        .map_err(|_| "created_at is not an RFC 3339 time".to_string())?;
    match (&meta.access_expires_at, &meta.delete_after) {
        (None, None) => {}
        (Some(access), Some(delete)) => {
            let (Ok(access), Ok(delete)) = (parse_time(access), parse_time(delete)) else {
                return Err("a retention time is not an RFC 3339 time".into());
            };
            if !(created <= access && access <= delete) {
                return Err("the retention times are out of order".into());
            }
        }
        _ => return Err("the retention sets only one of its two times".into()),
    }
    Ok(meta)
}

fn parse_index_line(line: &[u8]) -> Result<IndexRecord, String> {
    if line.is_empty() {
        return Err("empty line inside the index".into());
    }
    serde_json::from_slice(line).map_err(|_| "the record does not parse".to_string())
}

/// SHA-256 over (`prev_hash` || canonical JSON of the record with `hash`
/// blanked), lowercase hex: the event log's chain scheme.
fn index_hash(record: &IndexRecord) -> Result<String, serde_json::Error> {
    let mut blanked = record.clone();
    blanked.hash.clear();
    let mut hasher = Sha256::new();
    hasher.update(record.prev_hash.as_bytes());
    hasher.update(serde_json::to_vec(&blanked)?);
    Ok(hex(&hasher.finalize()))
}

fn write_index_record(file: &mut File, line: &[u8]) -> io::Result<()> {
    #[cfg(test)]
    if let Some(cut) = FAIL_INDEX_APPEND_AFTER.with(std::cell::Cell::take) {
        file.write_all(&line[..cut.min(line.len())])?;
        file.flush()?;
        return Err(io::Error::other("injected index append failure"));
    }
    file.write_all(line)?;
    file.flush()?;
    file.sync_all()
}

/// Writes `bytes` to `path` so that no partial file is ever visible
/// there: `tmp` (same directory, so the rename stays on one filesystem)
/// is written, flushed and fsync'd, renamed over `path`, and the
/// directory is fsync'd. On failure `tmp` is removed and `path` was
/// never touched.
fn write_atomic(path: &Path, tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let written = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(tmp)?;
        #[cfg(test)]
        if let Some(cut) = FAIL_SIDECAR_WRITE_AFTER.with(std::cell::Cell::take) {
            file.write_all(&bytes[..cut.min(bytes.len())])?;
            file.flush()?;
            return Err(io::Error::other("injected sidecar write failure"));
        }
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(tmp, path)?;
        sync_dir(path.parent())
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    written
}

/// Makes a create, rename or delete in `dir` durable. Unix only: std
/// offers no directory handle to fsync on Windows, where NTFS journals
/// its metadata.
#[cfg(unix)]
fn sync_dir(dir: Option<&Path>) -> io::Result<()> {
    match dir {
        Some(dir) if !dir.as_os_str().is_empty() => File::open(dir)?.sync_all(),
        _ => Ok(()),
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: Option<&Path>) -> io::Result<()> {
    Ok(())
}

fn exists(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// True when `path` is itself a regular file: not missing, and not a
/// symlink, which could lead into another run or the research archive.
fn is_regular_file(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_file()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Run ids with a regular `<run_id>.jsonl` file in `dir`, and run ids
/// with any run file at all (the debris of a creation that never
/// finished included). Other names, the index's among them, belong to no
/// run.
fn scan_run_files(dir: &Path) -> io::Result<(BTreeSet<RunId>, BTreeSet<RunId>)> {
    let (mut logs, mut any) = (BTreeSet::new(), BTreeSet::new());
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        for suffix in RUN_FILE_SUFFIXES {
            let Some(run_id) = name
                .strip_suffix(suffix)
                .and_then(|stem| parse_run_id(stem).ok())
            else {
                continue;
            };
            if suffix == ".jsonl" && entry.file_type()?.is_file() {
                logs.insert(run_id.clone());
            }
            any.insert(run_id);
        }
    }
    Ok((logs, any))
}

/// What in `dir` shows that a store has used it: the run index's lock
/// file, or any run's files. None for a directory no store has used.
fn store_traces(dir: &Path) -> io::Result<Option<String>> {
    if exists(&dir.join(INDEX_LOCK_FILE))? {
        return Ok(Some(format!("{RUNS_DIR}/{INDEX_LOCK_FILE} exists")));
    }
    let (_, runs) = scan_run_files(dir)?;
    Ok((!runs.is_empty())
        .then(|| format!("{RUNS_DIR}/ holds the files of runs ({} found)", runs.len())))
}

fn inconsistent(run_id: &RunId, detail: impl Into<String>) -> RunStoreError {
    RunStoreError::Inconsistent {
        run_id: run_id.clone(),
        detail: detail.into(),
    }
}

fn index_unusable(why: &str) -> RunStoreError {
    RunStoreError::Io(io::Error::other(format!(
        "run index unusable until the store is reopened: {why}"
    )))
}

fn parse_time(s: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(s).map(|t| t.with_timezone(&Utc))
}

/// RFC 3339, UTC, whole seconds: `2026-10-27T00:00:00Z`.
fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
thread_local! {
    /// Test-only fault injection: the next sidecar write on this thread
    /// writes this many bytes of its temporary file, then fails as a
    /// crash or a full disk would.
    static FAIL_SIDECAR_WRITE_AFTER: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
    /// Test-only: the next run index append on this thread writes this
    /// many bytes of its record, then fails.
    static FAIL_INDEX_APPEND_AFTER: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, SequentialIdGen};
    use crate::control::RunControl;
    use crate::orchestrator::{Orchestrator, RunLimits};
    use crate::scenarios::{proposal, retry_constraint_scenario};
    use crate::shared_log::NullPublisher;
    use crate::sink::EventSink;
    use crate::state_machine::TransitionError;
    use consensus_domain::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    const T0: &str = "2026-10-27T00:00:00Z";

    /// A fresh, empty directory for one test, removed again on drop.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "hyphae-consensus-run-store-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn runs(&self) -> PathBuf {
            self.0.join(RUNS_DIR)
        }

        fn file(&self, run_id: &RunId, suffix: &str) -> PathBuf {
            self.runs().join(format!("{run_id}{suffix}"))
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A clock the test moves.
    #[derive(Clone)]
    struct TestClock(Arc<Mutex<String>>);

    impl TestClock {
        fn at(ts: &str) -> Self {
            Self(Arc::new(Mutex::new(ts.into())))
        }

        fn set(&self, ts: &str) {
            *self.0.lock().unwrap() = ts.into();
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> String {
            self.0.lock().unwrap().clone()
        }
    }

    fn open_store(root: &TempRoot) -> RunStore {
        RunStore::open_with_clock(root.path(), Box::new(FixedClock::new(T0))).unwrap()
    }

    fn time(ts: &str) -> DateTime<Utc> {
        parse_time(ts).unwrap()
    }

    fn visitor(n: u8) -> Owner {
        Owner::session(&[n; 32])
    }

    /// Every directory and file under `dir`, recursively: a directory as
    /// such, a file as the SHA-256 of its bytes. Lock files hold no data,
    /// and Windows refuses reads of a locked one, so they are recorded by
    /// length.
    fn tree(dir: &Path) -> Vec<(PathBuf, String)> {
        let mut entries = Vec::new();
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    entries.push((path.clone(), "directory".to_string()));
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "lock") {
                    entries.push((path, format!("lock file, {} bytes", meta.len())));
                } else {
                    let digest = sha256_hex(&std::fs::read(&path).unwrap());
                    entries.push((path, digest));
                }
            }
        }
        entries.sort();
        entries
    }

    fn task_event(run: &RunId, id: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Human { id: "mario".into() },
            occurred_at: T0.into(),
            payload: EventPayload::TaskCreated {
                task: Task {
                    schema_version: SCHEMA_VERSION.into(),
                    id: TaskId::new("task-1"),
                    run_id: run.clone(),
                    project_id: ProjectId::new("p"),
                    title: "t".into(),
                    description: "d".into(),
                    created_at: T0.into(),
                },
            },
        }
    }

    fn proposal_event(run: &RunId, id: &str) -> NewEvent {
        let mut proposal = proposal("prop-1", "s", vec![]);
        proposal.run_id = run.clone();
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Agent {
                role: AgentRole::Proposer,
            },
            occurred_at: T0.into(),
            payload: EventPayload::ProposalSubmitted {
                proposal,
                invocation: ModelInvocation {
                    provider: "fake".into(),
                    model: "muse-wonder-1.0".into(),
                    request_id: None,
                    latency_ms: None,
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                },
            },
        }
    }

    fn fail_event(run: &RunId, id: &str) -> NewEvent {
        NewEvent {
            event_id: EventId::new(id),
            run_id: run.clone(),
            actor: Actor::Runtime,
            occurred_at: T0.into(),
            payload: EventPayload::RunFailed {
                reason: "boom".into(),
                stage: "propose".into(),
            },
        }
    }

    fn decision_event(run: &RunId, approved: bool) -> NewEvent {
        NewEvent {
            event_id: EventId::new(format!("human-decision:{run}:{approved}")),
            run_id: run.clone(),
            actor: Actor::Human { id: "mario".into() },
            occurred_at: T0.into(),
            payload: EventPayload::HumanDecisionRecorded {
                authorization: HumanAuthorization {
                    schema_version: SCHEMA_VERSION.into(),
                    run_id: run.clone(),
                    authorized_by: "mario".into(),
                    approved,
                    decided_at: T0.into(),
                    idempotency_key: approved.to_string(),
                    note: None,
                },
            },
        }
    }

    /// The ids of the runs a recovery found inconsistent, sorted.
    fn inconsistent_ids(report: &RecoveryReport) -> Vec<RunId> {
        let mut ids: Vec<RunId> = report
            .inconsistent
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    fn sorted(mut ids: Vec<RunId>) -> Vec<RunId> {
        ids.sort();
        ids
    }

    /// A run that ended: created, its task, then a failure. Returns its
    /// id and its log's head.
    fn ended_run(store: &RunStore, owner: Owner, retention: Retention) -> (RunId, String) {
        let run = store.create_run(owner, retention).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        log.append(task_event(&run_id, "e1")).unwrap();
        let last = log.append(fail_event(&run_id, "e2")).unwrap();
        (run_id, last.hash)
    }

    /// The reference scenario, deliberated to its human decision point.
    fn deliberate_to_awaiting(log: &mut SharedRunLog) {
        let s = retry_constraint_scenario();
        let mut orch = Orchestrator {
            model: s.model,
            memory: s.memory,
            verifier: s.verifier,
            clock: FixedClock::new(T0),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        let run_id = log.run_id().clone();
        let (agg, task) = orch.create_task(log, run_id, s.input).unwrap();
        let agg = orch
            .deliberate(log, agg, &task, &RunControl::new())
            .unwrap();
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
    }

    /// A store-owned run for `owner`, deliberated to its decision point
    /// and let go.
    fn resting_run(store: &RunStore, owner: Owner) -> RunId {
        let run = store.create_run(owner, Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        deliberate_to_awaiting(&mut run.into_shared(Arc::new(NullPublisher)));
        run_id
    }

    fn index_entries(store: &RunStore) -> Vec<(IndexKind, RunId, Option<String>)> {
        store
            .index_records()
            .unwrap()
            .into_iter()
            .map(|r| (r.kind, r.run_id, r.head_hash))
            .collect()
    }

    fn run_ids(metas: Vec<RunMeta>) -> Vec<RunId> {
        metas.into_iter().map(|meta| meta.run_id).collect()
    }

    #[test]
    fn parse_run_id_accepts_exactly_the_uuid_id_gen_format() {
        for _ in 0..256 {
            let id = UuidIdGen.next("run");
            assert_eq!(parse_run_id(&id).unwrap().as_str(), id);
        }
        let valid = "run-0f8fad5b-d9cb-469f-a165-70867728950e";
        assert_eq!(parse_run_id(valid).unwrap(), RunId::new(valid));

        let rejected = [
            String::new(),
            "run-".into(),
            "../x".into(),
            "run-../../x".into(),
            format!("run-{}", "../".repeat(12)),
            "/etc/passwd".into(),
            "C:\\x".into(),
            "C:x".into(),
            "\\\\server\\share\\x".into(),
            // Paths of exactly the accepted length.
            format!("C:\\{}", &valid[3..]),
            format!("/{}", &valid[1..]),
            format!("run-{}", "%2e%2e%2f".repeat(4)),
            format!("run-0f8fad5b-d9cb-469f-a165-7086772895{}", "/e"),
            valid.replacen('d', "\0", 1),
            valid.to_uppercase(),
            valid.replacen("d9cb", "D9CB", 1),
            // Version 1 and 5, and other variants.
            valid.replacen("-469f-", "-169f-", 1),
            valid.replacen("-469f-", "-569f-", 1),
            valid.replacen("-a165-", "-c165-", 1),
            valid.replacen("-a165-", "-7165-", 1),
            format!("{valid}0"),
            valid[..39].to_string(),
            format!("{valid}\n"),
            format!(" {}", &valid[..39]),
            valid.replace('-', "0"),
            valid.replacen("run-", "run_", 1),
            valid.replacen('0', "\u{e9}", 1),
        ];
        for s in &rejected {
            assert!(parse_run_id(s).is_err(), "{s:?} accepted");
        }
    }

    #[test]
    fn create_run_writes_its_sidecar_lock_log_and_created_record() {
        let root = TempRoot::new("create");
        let store = open_store(&root);
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        assert!(parse_run_id(run_id.as_str()).is_ok());
        for suffix in [".jsonl", ".meta.json", ".lock"] {
            assert!(root.file(&run_id, suffix).is_file(), "{suffix}");
        }
        assert!(!root.file(&run_id, ".meta.json.tmp").exists());
        assert!(run.log().is_empty());

        let meta = RunMeta {
            version: META_VERSION,
            run_id: run_id.clone(),
            owner: visitor(1),
            created_at: T0.into(),
            access_expires_at: Some("2026-10-28T00:00:00Z".into()),
            delete_after: Some("2026-11-03T00:00:00Z".into()),
        };
        assert_eq!(*run.meta(), meta);
        let sidecar = std::fs::read(root.file(&run_id, ".meta.json")).unwrap();
        assert_eq!(sidecar, meta_bytes(&meta).unwrap());
        // The session is stored as its hash, never as its id.
        let text = String::from_utf8(sidecar).unwrap();
        assert!(text.contains(&sha256_hex(&[1; 32])), "{text}");
        assert!(meta.access_open_at(time("2026-10-27T23:59:59Z")));
        assert!(!meta.access_open_at(time("2026-10-28T00:00:00Z")));

        let records = store.index_records().unwrap();
        assert_eq!(records.len(), 1);
        let created = &records[0];
        assert_eq!(
            (
                created.seq,
                created.kind,
                created.head_hash.as_deref(),
                created.at.as_str(),
                created.prev_hash.as_str()
            ),
            (1, IndexKind::Created, None, T0, INDEX_GENESIS)
        );
        assert_eq!(created.hash, index_hash(created).unwrap());
        // The record commits to the sidecar's exact bytes, and the empty
        // log has no head.
        assert_eq!(
            created.meta_sha256.as_deref(),
            Some(sha256_hex(&meta_bytes(&meta).unwrap()).as_str())
        );
        assert_eq!(created.head_sequence, None);
        assert_eq!(
            store.index_head().unwrap(),
            IndexHead {
                seq: 1,
                hash: created.hash.clone()
            }
        );
        assert_eq!(store.meta(run_id.as_str()).unwrap(), meta);
        assert_eq!(run_ids(store.list_for(&visitor(1)).unwrap()), [run_id]);
    }

    #[test]
    fn a_run_has_one_owner_at_a_time_across_handles_and_stores() {
        let root = TempRoot::new("owner");
        let store = open_store(&root);
        // A second store on the same root stands in for another process.
        let other = open_store(&root);
        let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let id = run.run_id().to_string();
        for attempt in [store.open_run(&id), other.open_run(&id)] {
            assert!(
                matches!(attempt, Err(RunStoreError::InUse(_))),
                "{attempt:?}"
            );
        }
        // The shared log keeps the lock, and its clones share it.
        let log = run.into_shared(Arc::new(NullPublisher));
        let clone = log.clone();
        drop(log);
        assert!(matches!(other.open_run(&id), Err(RunStoreError::InUse(_))));
        drop(clone);
        let reopened = other.open_run(&id).unwrap();
        assert!(matches!(store.open_run(&id), Err(RunStoreError::InUse(_))));
        // Recovery leaves a run that is owned elsewhere alone.
        let report = store.recover().unwrap();
        assert_eq!(report.in_use, [RunId::new(&id)]);
        drop(reopened);
        store.open_run(&id).unwrap();
    }

    #[test]
    fn unknown_and_malformed_ids_never_create_a_file() {
        let root = TempRoot::new("lookups");
        let store =
            RunStore::open_with_clock(root.path().join("store"), Box::new(FixedClock::new(T0)))
                .unwrap();
        let known = store
            .create_run(visitor(1), Retention::VISITOR)
            .unwrap()
            .run_id()
            .clone();
        let before = tree(root.path());

        let outside = root.path().join("outside");
        let mut malformed: Vec<String> = [
            "../x",
            "run-../../x",
            "..",
            "/",
            "C:\\x",
            "C:x",
            "run-\0",
            "index",
            "index.jsonl",
            "index.lock",
        ]
        .map(String::from)
        .to_vec();
        malformed.push(outside.join("evil").to_string_lossy().into_owned());
        malformed.push(format!("{known}/../../../outside/evil"));
        malformed.push(format!("../{known}"));
        malformed.push(format!("{known}.jsonl"));
        malformed.push(known.as_str().to_uppercase());
        for id in &malformed {
            assert!(
                matches!(store.open_run(id), Err(RunStoreError::InvalidId(_))),
                "{id:?}"
            );
            assert!(
                matches!(store.meta(id), Err(RunStoreError::InvalidId(_))),
                "{id:?}"
            );
            assert!(
                matches!(
                    store.read_run(id, &visitor(1)),
                    Err(RunStoreError::InvalidId(_))
                ),
                "{id:?}"
            );
            assert!(
                matches!(
                    store.open_run_as(id, &visitor(1)),
                    Err(RunStoreError::InvalidId(_))
                ),
                "{id:?}"
            );
        }
        for _ in 0..1000 {
            let id = UuidIdGen.next("run");
            assert!(
                matches!(store.open_run(&id), Err(RunStoreError::UnknownRun(_))),
                "{id}"
            );
            assert!(
                matches!(store.meta(&id), Err(RunStoreError::UnknownRun(_))),
                "{id}"
            );
            assert!(
                matches!(
                    store.read_run(&id, &visitor(1)),
                    Err(RunStoreError::UnknownRun(_))
                ),
                "{id}"
            );
            assert!(
                matches!(
                    store.open_run_as(&id, &visitor(1)),
                    Err(RunStoreError::UnknownRun(_))
                ),
                "{id}"
            );
        }
        assert_eq!(tree(root.path()), before, "nothing created, nothing grew");
        assert!(!outside.exists());
        store.open_run(known.as_str()).unwrap();
    }

    #[test]
    fn a_sidecar_write_that_fails_midway_leaves_no_sidecar_and_no_run() {
        let root = TempRoot::new("sidecar-fault");
        let store = open_store(&root);
        let before = tree(root.path());
        // Cut inside the record, and after all of it but before the
        // fsync and rename.
        for cut in [0, 1, 40, usize::MAX] {
            FAIL_SIDECAR_WRITE_AFTER.with(|fail| fail.set(Some(cut)));
            let err = store
                .create_run(visitor(2), Retention::VISITOR)
                .unwrap_err();
            assert!(
                err.to_string().contains("injected sidecar write failure"),
                "{err}"
            );
            // No sidecar, temporary file, lock file or log, and nothing
            // indexed.
            assert_eq!(tree(root.path()), before, "cut after {cut} bytes");
        }
        assert!(store.index_records().unwrap().is_empty());
        let run = store.create_run(visitor(2), Retention::VISITOR).unwrap();
        assert_eq!(store.meta(run.run_id().as_str()).unwrap(), *run.meta());
    }

    #[test]
    fn a_concurrent_reader_never_sees_a_partial_sidecar() {
        let root = TempRoot::new("sidecar-reader");
        let store = open_store(&root);
        let dir = root.runs();
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let done = done.clone();
            std::thread::spawn(move || {
                let mut whole = 0;
                loop {
                    let last_pass = done.load(Ordering::SeqCst);
                    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let Some(stem) = name.strip_suffix(".meta.json") else {
                            continue;
                        };
                        // A read may lose a race with the rename and find
                        // nothing; whatever it finds must be whole.
                        if let Ok(bytes) = std::fs::read(entry.path()) {
                            let run_id = parse_run_id(stem).unwrap();
                            if let Err(e) = read_meta(&bytes, &run_id) {
                                panic!("partial or bad sidecar visible: {e}: {bytes:?}");
                            }
                            whole += 1;
                        }
                    }
                    if last_pass {
                        return whole;
                    }
                }
            })
        };
        for _ in 0..40 {
            store.create_run(visitor(3), Retention::VISITOR).unwrap();
        }
        done.store(true, Ordering::SeqCst);
        assert!(reader.join().unwrap() >= 40);
    }

    #[test]
    fn a_sidecar_that_disagrees_with_its_run_is_provenance_inconsistent() {
        let root = TempRoot::new("sidecar-mismatch");
        let store = open_store(&root);
        let run_id = store
            .create_run(visitor(1), Retention::VISITOR)
            .unwrap()
            .run_id()
            .clone();
        let other = store
            .create_run(Owner::Operator, Retention::Keep)
            .unwrap()
            .run_id()
            .clone();
        drop(store);
        let path = root.file(&run_id, ".meta.json");
        let original = std::fs::read(&path).unwrap();
        let text = String::from_utf8(original.clone()).unwrap();
        let meta = read_meta(&original, &run_id).unwrap();
        let hash = sha256_hex(&[1; 32]);
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "another run's sidecar",
                std::fs::read(root.file(&other, ".meta.json")).unwrap(),
            ),
            (
                "an added field",
                text.replacen("{\"version\"", "{\"note\":1,\"version\"", 1)
                    .into_bytes(),
            ),
            (
                "an extra key on the owner",
                text.replacen("\"kind\":\"session\"", "\"kind\":\"session\",\"x\":1", 1)
                    .into_bytes(),
            ),
            ("reformatted", serde_json::to_vec_pretty(&meta).unwrap()),
            (
                "the raw session id",
                text.replacen(&hash, "the-session-cookie", 1).into_bytes(),
            ),
            (
                "retention out of order",
                text.replacen("2026-10-28", "2026-11-28", 1).into_bytes(),
            ),
            (
                "an unknown version",
                text.replacen("\"version\":1", "\"version\":2", 1)
                    .into_bytes(),
            ),
            ("cut short", original[..original.len() / 2].to_vec()),
        ];
        for (case, bytes) in cases {
            assert_ne!(bytes, original, "{case}");
            std::fs::write(&path, &bytes).unwrap();
            // A fresh store each time, so no sidecar is cached.
            let store = open_store(&root);
            let meta = store.meta(run_id.as_str());
            assert!(
                matches!(meta, Err(RunStoreError::Inconsistent { .. })),
                "{case}: {meta:?}"
            );
            let opened = store.open_run(run_id.as_str());
            assert!(
                matches!(opened, Err(RunStoreError::Inconsistent { .. })),
                "{case}: {opened:?}"
            );
        }
        std::fs::remove_file(&path).unwrap();
        let opened = open_store(&root).open_run(run_id.as_str());
        assert!(
            matches!(opened, Err(RunStoreError::Inconsistent { .. })),
            "missing: {opened:?}"
        );
        std::fs::write(&path, &original).unwrap();
        open_store(&root).open_run(run_id.as_str()).unwrap();
    }

    #[test]
    fn any_edit_to_the_run_index_is_detected_when_the_store_opens() {
        let root = TempRoot::new("index-tamper");
        let store = open_store(&root);
        for _ in 0..3 {
            ended_run(&store, visitor(1), Retention::VISITOR);
        }
        drop(store);
        let index_path = root.runs().join(INDEX_FILE);
        let original = std::fs::read_to_string(&index_path).unwrap();
        let lines: Vec<String> = original.lines().map(String::from).collect();
        assert_eq!(
            lines.len(),
            9,
            "created, task checkpoint and terminal for each run"
        );
        let record = |i: usize| -> IndexRecord { serde_json::from_str(&lines[i]).unwrap() };
        let json = |r: &IndexRecord| serde_json::to_string(r).unwrap();
        let text = |lines: &[String]| lines.iter().map(|l| format!("{l}\n")).collect::<String>();
        let edited = |i: usize, line: String| {
            let mut lines = lines.clone();
            lines[i] = line;
            text(&lines)
        };

        // Edited and re-hashed, as a forger would: the record checks out,
        // the next record's link does not.
        let mut rehashed = record(1);
        rehashed.head_hash = Some("0".repeat(64));
        rehashed.hash = index_hash(&rehashed).unwrap();
        let mut unhashed = record(1);
        unhashed.head_hash = Some("f".repeat(64));
        // Added by hand: a right sequence, no real link.
        let mut unlinked = record(8);
        unlinked.seq = 10;
        unlinked.kind = IndexKind::Purged;
        unlinked.prev_hash = "0".repeat(64);
        unlinked.hash = index_hash(&unlinked).unwrap();
        // Added by hand and correctly linked, but a second creation.
        let mut recreated = record(0);
        recreated.seq = 10;
        recreated.prev_hash = record(8).hash;
        recreated.hash = index_hash(&recreated).unwrap();
        // Added by hand and correctly linked, but after the terminal head.
        let mut late = record(7);
        late.seq = 10;
        late.prev_hash = record(8).hash;
        late.hash = index_hash(&late).unwrap();
        // Correctly linked, in place of the terminal record, but a second
        // checkpoint at the head already recorded.
        let mut standing = record(7);
        standing.seq = 9;
        standing.prev_hash = record(7).hash;
        standing.hash = index_hash(&standing).unwrap();
        let appended = |extra: &IndexRecord| {
            let mut lines = lines.clone();
            lines.push(json(extra));
            text(&lines)
        };

        let cases = [
            (
                "a kind changed",
                edited(2, lines[2].replacen("\"terminal\"", "\"purged\"", 1)),
            ),
            ("a head changed", edited(1, json(&unhashed))),
            ("a record re-hashed", edited(1, json(&rehashed))),
            ("a record deleted", {
                let mut lines = lines.clone();
                lines.remove(2);
                text(&lines)
            }),
            ("two records swapped", {
                let mut lines = lines.clone();
                lines.swap(2, 3);
                text(&lines)
            }),
            (
                "whitespace added",
                edited(0, lines[0].replacen(',', ", ", 1)),
            ),
            (
                "an unknown field",
                edited(0, lines[0].replacen('{', "{\"extra\":1,", 1)),
            ),
            ("an empty line inside", {
                let mut lines = lines.clone();
                lines.insert(3, String::new());
                text(&lines)
            }),
            ("an unlinked record added", appended(&unlinked)),
            ("a run created twice", appended(&recreated)),
            ("a checkpoint after the terminal head", appended(&late)),
            (
                "a checkpoint that does not move forward",
                edited(8, json(&standing)),
            ),
        ];
        for (case, bytes) in &cases {
            std::fs::write(&index_path, bytes).unwrap();
            let opened = RunStore::open_with_clock(root.path(), Box::new(FixedClock::new(T0)));
            assert!(
                matches!(opened, Err(RunStoreError::IndexTampered { .. })),
                "{case}: {:?}",
                opened.err()
            );
        }

        // Removing the newest records leaves a shorter valid chain: only a
        // head obtained earlier shows it.
        std::fs::write(&index_path, &original).unwrap();
        let published = open_store(&root).index_head().unwrap();
        assert_eq!(published.seq, 9);
        std::fs::write(&index_path, text(&lines[..8])).unwrap();
        assert_ne!(open_store(&root).index_head().unwrap(), published);

        // A torn final record, from a crash mid-append, is cut off.
        std::fs::write(&index_path, format!("{original}{}", &lines[0][..30])).unwrap();
        let store = open_store(&root);
        assert_eq!(std::fs::read_to_string(&index_path).unwrap(), original);
        assert_eq!(store.index_head().unwrap(), published);
        assert!(store.recover().unwrap().index_tail_repaired);
    }

    #[test]
    fn a_run_ending_through_its_shared_log_records_its_terminal_head() {
        let root = TempRoot::new("terminal-head");
        let store = open_store(&root);
        let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        let task = log.append(task_event(&run_id, "e1")).unwrap();
        assert_eq!(
            index_entries(&store),
            [
                (IndexKind::Created, run_id.clone(), None),
                (
                    IndexKind::Checkpoint,
                    run_id.clone(),
                    Some(task.hash.clone())
                )
            ]
        );
        let last = log.append(fail_event(&run_id, "e2")).unwrap();
        let expected = [
            (IndexKind::Created, run_id.clone(), None),
            (
                IndexKind::Checkpoint,
                run_id.clone(),
                Some(task.hash.clone()),
            ),
            (IndexKind::Terminal, run_id.clone(), Some(last.hash.clone())),
        ];
        assert_eq!(index_entries(&store), expected);
        // A retried terminal record writes nothing more.
        assert_eq!(log.append(fail_event(&run_id, "e2")).unwrap(), last);
        assert_eq!(index_entries(&store), expected);
        assert_eq!(store.index_write_error(), None);
        drop(log);
        // Reopening checks the log's head against the recorded one.
        let handle = store.open_run(run_id.as_str()).unwrap();
        assert_eq!(handle.log().head_hash(), Some(last.hash.as_str()));
    }

    #[test]
    fn a_failed_terminal_index_write_keeps_the_append_and_recovery_records_the_head() {
        let root = TempRoot::new("terminal-index-fault");
        let store = open_store(&root);
        let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        let task = log.append(task_event(&run_id, "e1")).unwrap();
        FAIL_INDEX_APPEND_AFTER.with(|fail| fail.set(Some(10)));
        let last = log.append(fail_event(&run_id, "e2")).unwrap();
        // The record is durable and acknowledged; only its index record
        // is missing, and the failure is kept for the health check.
        assert_eq!(log.aggregate().unwrap().state, RunState::Failed);
        let error = store.index_write_error().unwrap();
        assert!(error.contains("injected index append failure"), "{error}");
        assert_eq!(
            index_entries(&store),
            [
                (IndexKind::Created, run_id.clone(), None),
                (IndexKind::Checkpoint, run_id.clone(), Some(task.hash))
            ]
        );
        assert_eq!(store.health().index_write_error, Some(error));
        drop(log);
        let report = store.recover().unwrap();
        assert_eq!(report.terminal_recorded, vec![run_id.clone()]);
        assert_eq!(
            index_entries(&store).last(),
            Some(&(IndexKind::Terminal, run_id, Some(last.hash)))
        );
    }

    #[test]
    fn a_failed_index_append_rolls_back_and_leaves_no_run_files() {
        let root = TempRoot::new("index-fault");
        let store = open_store(&root);
        store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let before = tree(root.path());
        FAIL_INDEX_APPEND_AFTER.with(|fail| fail.set(Some(20)));
        let err = store
            .create_run(Owner::Operator, Retention::Keep)
            .unwrap_err();
        assert!(
            err.to_string().contains("injected index append failure"),
            "{err}"
        );
        assert_eq!(
            tree(root.path()),
            before,
            "partial record and run files removed"
        );
        // The index keeps working, and verifies from scratch.
        store.create_run(Owner::Operator, Retention::Keep).unwrap();
        assert_eq!(open_store(&root).index_records().unwrap().len(), 2);
    }

    #[test]
    fn recovery_closes_interrupted_runs_records_terminal_heads_and_reports_the_rest() {
        let root = TempRoot::new("recovery");
        let store = open_store(&root);
        // Left mid-deliberation by a process that died.
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let mid = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        log.append(task_event(&mid, "e1")).unwrap();
        log.append(proposal_event(&mid, "e2")).unwrap();
        drop(log);
        // Resting: awaiting its human decision.
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let resting = run.run_id().clone();
        deliberate_to_awaiting(&mut run.into_shared(Arc::new(NullPublisher)));
        // Ended, but its terminal head never reached the index, as when
        // that write fails.
        let unrecorded = store
            .create_run(Owner::Operator, Retention::Keep)
            .unwrap()
            .run_id()
            .clone();
        let mut raw = EventLog::open(root.file(&unrecorded, ".jsonl")).unwrap();
        EventSink::append(&mut raw, task_event(&unrecorded, "e1")).unwrap();
        let unrecorded_head = EventSink::append(&mut raw, fail_event(&unrecorded, "e2"))
            .unwrap()
            .hash;
        drop(raw);
        // Damaged: a record edited after it was written.
        let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let damaged = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        log.append(task_event(&damaged, "e1")).unwrap();
        log.append(proposal_event(&damaged, "e2")).unwrap();
        drop(log);
        let path = root.file(&damaged, ".jsonl");
        let forged = std::fs::read_to_string(&path).unwrap().replacen(
            "\"title\":\"t\"",
            "\"title\":\"forged\"",
            1,
        );
        std::fs::write(&path, forged).unwrap();
        // Mid-deliberation in a process that is still alive.
        let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let busy = run.run_id().clone();
        let mut busy_log = run.into_shared(Arc::new(NullPublisher));
        busy_log.append(task_event(&busy, "e1")).unwrap();
        drop(store);

        let logs_of = |runs: &[&RunId]| {
            runs.iter()
                .map(|run_id| std::fs::read(root.file(run_id, ".jsonl")).unwrap())
                .collect::<Vec<_>>()
        };
        let before = logs_of(&[&resting, &damaged, &busy]);
        // Restart.
        let store = open_store(&root);
        let report = store.recover().unwrap();
        assert_eq!(report.interrupted, vec![mid.clone()], "{report:?}");
        assert_eq!(report.terminal_recorded, [mid.clone(), unrecorded.clone()]);
        let unreplayable: Vec<&RunId> = report.unreplayable.iter().map(|(id, _)| id).collect();
        assert_eq!(unreplayable, [&damaged]);
        assert_eq!(report.in_use, vec![busy.clone()]);
        assert!(
            report.inconsistent.is_empty()
                && report.missing.is_empty()
                && report.unindexed.is_empty()
                && report.failed.is_empty()
                && report.purges_completed.is_empty(),
            "{report:?}"
        );
        assert_eq!(logs_of(&[&resting, &damaged, &busy]), before, "left alone");

        let handle = store.open_run(mid.as_str()).unwrap();
        let last = handle.log().all_events().last().unwrap().clone();
        assert!(
            matches!(&last.payload, EventPayload::RunFailed { stage, .. } if stage == "interrupted"),
            "{last:?}"
        );
        assert_eq!(handle.log().len(), 3);
        drop(handle);
        let entries = index_entries(&store);
        assert!(entries.contains(&(IndexKind::Terminal, mid.clone(), Some(last.hash))));
        assert!(entries.contains(&(IndexKind::Terminal, unrecorded, Some(unrecorded_head))));
        let resting_state = store
            .open_run(resting.as_str())
            .unwrap()
            .log()
            .rebuild(&resting)
            .unwrap()
            .state;
        assert_eq!(resting_state, RunState::AwaitingHumanApproval);

        // Once its process lets go, the busy run is closed too; no run is
        // ever interrupted twice.
        drop(busy_log);
        let report = store.recover().unwrap();
        assert_eq!(report.interrupted, vec![busy.clone()]);
        assert_eq!(report.terminal_recorded, [busy]);
        let report = store.recover().unwrap();
        assert!(
            report.interrupted.is_empty() && report.terminal_recorded.is_empty(),
            "{report:?}"
        );
        // Everything recovery wrote to the index verifies from scratch.
        assert_eq!(
            open_store(&root).index_records().unwrap(),
            store.index_records().unwrap()
        );
    }

    #[test]
    fn a_truncated_or_deleted_run_is_reported_and_left_alone() {
        let root = TempRoot::new("truncated");
        let store = open_store(&root);
        let (truncated, _) = ended_run(&store, Owner::Operator, Retention::Keep);
        let (deleted, _) = ended_run(&store, Owner::Showcase, Retention::Keep);
        drop(store);
        // Drop the terminal record, as if a rejection or a failure were
        // erased.
        let path = root.file(&truncated, ".jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, &text[..=text.find('\n').unwrap()]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        for suffix in [".jsonl", ".meta.json", ".lock"] {
            std::fs::remove_file(root.file(&deleted, suffix)).unwrap();
        }

        let store = open_store(&root);
        let report = store.recover().unwrap();
        let inconsistent: Vec<&RunId> = report.inconsistent.iter().map(|(id, _)| id).collect();
        assert_eq!(inconsistent, [&truncated], "{report:?}");
        assert_eq!(report.missing, vec![deleted.clone()]);
        assert!(
            report.interrupted.is_empty(),
            "not closed again: {report:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "left as found");
        for run_id in [&truncated, &deleted] {
            let opened = store.open_run(run_id.as_str());
            assert!(
                matches!(opened, Err(RunStoreError::Inconsistent { .. })),
                "{opened:?}"
            );
        }
    }

    #[test]
    fn run_files_the_index_never_recorded_are_never_served() {
        let root = TempRoot::new("unindexed");
        let store = open_store(&root);
        let (run_id, _) = ended_run(&store, Owner::Operator, Retention::Keep);
        // A copy of the run's files under a new, valid id, as anyone who
        // can write the directory could make.
        let planted = parse_run_id(&UuidIdGen.next("run")).unwrap();
        for suffix in [".jsonl", ".meta.json", ".lock"] {
            std::fs::copy(root.file(&run_id, suffix), root.file(&planted, suffix)).unwrap();
        }
        assert!(matches!(
            store.open_run(planted.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        assert!(matches!(
            store.meta(planted.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        assert_eq!(
            run_ids(store.list_for(&Owner::Operator).unwrap()),
            vec![run_id.clone()]
        );
        // The debris of a creation that died before its sidecar was
        // renamed into place: a lock file and a temporary sidecar.
        let debris = parse_run_id(&UuidIdGen.next("run")).unwrap();
        std::fs::write(root.file(&debris, ".lock"), b"").unwrap();
        std::fs::write(root.file(&debris, ".meta.json.tmp"), b"{\"vers").unwrap();
        let mut expected = vec![planted.clone(), debris.clone()];
        expected.sort();
        let report = store.recover().unwrap();
        assert_eq!(report.unindexed, expected);
        assert!(report.missing.is_empty(), "{report:?}");
        assert!(
            root.file(&debris, ".lock").exists(),
            "reported, left in place"
        );

        // Without its index a store serves no run at all, rather than
        // adopting whatever files it finds. It does not even open unless
        // told to start an empty index.
        drop(store);
        std::fs::remove_file(root.runs().join(INDEX_FILE)).unwrap();
        let refused = RunStore::open_with_clock(root.path(), Box::new(FixedClock::new(T0)));
        assert!(
            matches!(refused, Err(RunStoreError::IndexMissing { .. })),
            "{refused:?}"
        );
        let store = RunStore::open_with(
            root.path(),
            Box::new(FixedClock::new(T0)),
            MissingIndex::StartEmpty,
        )
        .unwrap();
        assert!(matches!(
            store.open_run(run_id.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        let mut expected = vec![run_id, planted, debris];
        expected.sort();
        assert_eq!(store.recover().unwrap().unindexed, expected);
    }

    #[test]
    fn open_run_refuses_a_log_holding_another_runs_records() {
        let root = TempRoot::new("foreign");
        let store = open_store(&root);
        let run_id = store
            .create_run(Owner::Operator, Retention::Keep)
            .unwrap()
            .run_id()
            .clone();
        let mut raw = EventLog::open(root.file(&run_id, ".jsonl")).unwrap();
        raw.append(task_event(&RunId::new("run-elsewhere"), "e1"))
            .unwrap();
        drop(raw);
        let opened = store.open_run(run_id.as_str());
        assert!(
            matches!(opened, Err(RunStoreError::Inconsistent { .. })),
            "{opened:?}"
        );
        let report = store.recover().unwrap();
        let inconsistent: Vec<&RunId> = report.inconsistent.iter().map(|(id, _)| id).collect();
        assert_eq!(inconsistent, [&run_id]);
    }

    #[test]
    fn purge_deletes_only_expired_session_runs_whole_and_records_each_purge() {
        let root = TempRoot::new("purge");
        let clock = TestClock::at(T0);
        let store = RunStore::open_with_clock(root.path(), Box::new(clock.clone())).unwrap();
        let (expired, expired_head) = ended_run(&store, visitor(1), Retention::VISITOR);
        // Operator and showcase runs are never purged, whatever their
        // retention says.
        let (operator, _) = ended_run(&store, Owner::Operator, Retention::VISITOR);
        let (showcase, _) = ended_run(&store, Owner::Showcase, Retention::Keep);
        let held = store.create_run(visitor(2), Retention::VISITOR).unwrap();
        let busy = held.run_id().clone();
        clock.set("2026-10-30T00:00:00Z");
        let (younger, _) = ended_run(&store, visitor(1), Retention::VISITOR);

        let early = store.purge_expired(time("2026-11-02T23:59:59Z")).unwrap();
        assert_eq!(early, PurgeReport::default());
        let report = store.purge_expired(time("2026-11-03T00:00:00Z")).unwrap();
        assert_eq!(
            report,
            PurgeReport {
                purged: vec![expired.clone()],
                in_use: vec![busy.clone()],
                failed: vec![],
            }
        );
        for suffix in [".jsonl", ".meta.json", ".lock"] {
            assert!(!root.file(&expired, suffix).exists(), "{suffix}");
        }
        for run_id in [&operator, &showcase, &busy, &younger] {
            assert!(root.file(run_id, ".jsonl").exists());
        }
        let last = store.index_records().unwrap().pop().unwrap();
        assert_eq!(
            (
                last.kind,
                &last.run_id,
                last.head_hash.as_deref(),
                last.at.as_str()
            ),
            (
                IndexKind::Purged,
                &expired,
                Some(expired_head.as_str()),
                "2026-11-03T00:00:00Z"
            )
        );
        assert!(matches!(
            store.open_run(expired.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        assert!(matches!(
            store.meta(expired.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        assert_eq!(
            run_ids(store.list_for(&visitor(1)).unwrap()),
            vec![younger.clone()]
        );

        // After a restart the index still verifies, and the purged run is
        // not reported missing.
        drop(held);
        let store = RunStore::open_with_clock(root.path(), Box::new(clock)).unwrap();
        let report = store.recover().unwrap();
        assert!(
            report.missing.is_empty() && report.unindexed.is_empty(),
            "{report:?}"
        );
        // Released, the held run goes at the next purge; the younger one
        // only once it is due.
        let now = time("2026-11-03T00:00:00Z");
        assert_eq!(store.purge_expired(now).unwrap().purged, [busy]);
        let later = time("2026-11-06T00:00:00Z");
        assert_eq!(store.purge_expired(later).unwrap().purged, [younger]);
        assert!(store.list_for(&visitor(1)).unwrap().is_empty());
        assert_eq!(store.list_for(&Owner::Operator).unwrap().len(), 1);
        assert_eq!(store.list_for(&Owner::Showcase).unwrap().len(), 1);
    }

    #[test]
    fn a_purge_interrupted_after_its_index_record_is_finished_later() {
        let root = TempRoot::new("purge-crash");
        let store = open_store(&root);
        let (run_id, head) = ended_run(&store, visitor(1), Retention::VISITOR);
        // The purge is recorded, then the process dies before deleting.
        let head = Head {
            sequence: 2,
            hash: head,
        };
        store
            .shared
            .index()
            .unwrap()
            .append(&run_id, IndexKind::Purged, Some(head), None, T0.into())
            .unwrap();
        drop(store);
        let store = open_store(&root);
        assert!(matches!(
            store.open_run(run_id.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        let report = store.recover().unwrap();
        assert_eq!(report.purges_completed, vec![run_id.clone()], "{report:?}");
        assert!(report.missing.is_empty(), "{report:?}");
        for suffix in [".jsonl", ".meta.json", ".lock"] {
            assert!(!root.file(&run_id, suffix).exists(), "{suffix}");
        }
    }

    #[test]
    fn list_for_returns_only_the_owners_live_runs_oldest_first() {
        let root = TempRoot::new("list");
        let store = open_store(&root);
        let mut mine = Vec::new();
        for i in 0..3 {
            let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
            mine.push(run.run_id().clone());
            store.create_run(visitor(2), Retention::VISITOR).unwrap();
            if i == 1 {
                store.create_run(Owner::Operator, Retention::Keep).unwrap();
            }
        }
        assert_eq!(run_ids(store.list_for(&visitor(1)).unwrap()), mine);
        assert_eq!(store.list_for(&visitor(2)).unwrap().len(), 3);
        assert_eq!(store.list_for(&Owner::Operator).unwrap().len(), 1);
        assert!(store.list_for(&Owner::Showcase).unwrap().is_empty());
        assert!(store.list_for(&visitor(9)).unwrap().is_empty());
        // A fresh store, with no sidecar cached, lists the same.
        assert_eq!(
            run_ids(open_store(&root).list_for(&visitor(1)).unwrap()),
            mine
        );
    }

    #[test]
    fn a_raw_session_id_or_an_impossible_retention_is_refused_before_any_file_exists() {
        let root = TempRoot::new("validation");
        let store = open_store(&root);
        let before = tree(root.path());
        // A session owner cannot be built from a string in code (see the
        // SessionHash doc tests); the one other way in, deserializing,
        // refuses anything but a digest.
        for hash in ["the-session-cookie-value".to_string(), "A".repeat(64)] {
            let json = serde_json::json!({ "kind": "session", "hash": hash });
            assert!(serde_json::from_value::<Owner>(json).is_err(), "{hash}");
        }
        let day = 24 * 3600;
        let inverted = Retention::Expiring {
            access_for: Duration::from_secs(8 * day),
            delete_after: Duration::from_secs(7 * day),
        };
        let unbounded = Retention::Expiring {
            access_for: Duration::from_secs(u64::MAX),
            delete_after: Duration::from_secs(u64::MAX),
        };
        for retention in [inverted, unbounded] {
            assert!(matches!(
                store.create_run(visitor(1), retention),
                Err(RunStoreError::InvalidRetention(_))
            ));
        }
        assert_eq!(tree(root.path()), before);
        // A session owner is the digest of the session id.
        assert_eq!(
            serde_json::to_value(Owner::session(b"abc")).unwrap(),
            serde_json::json!({
                "kind": "session",
                "hash": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            })
        );
    }

    #[test]
    fn a_raw_session_id_shaped_like_a_digest_never_reaches_a_sidecar() {
        let root = TempRoot::new("raw-session-id");
        let store = open_store(&root);
        // A hex-encoded 256-bit session id has exactly a digest's form.
        let cookie = "5e".repeat(32);
        assert!(is_sha256_hex(&cookie));
        let run = store
            .create_run(Owner::session(cookie.as_bytes()), Retention::VISITOR)
            .unwrap();
        let sidecar =
            String::from_utf8(std::fs::read(root.file(run.run_id(), ".meta.json")).unwrap())
                .unwrap();
        assert!(!sidecar.contains(&cookie), "{sidecar}");
        assert!(
            sidecar.contains(&sha256_hex(cookie.as_bytes())),
            "{sidecar}"
        );
        // The owner it stored is the hashed one, which is how a later
        // request with the same cookie finds the run again.
        let id = run.run_id().to_string();
        drop(run);
        assert_eq!(
            run_ids(store.list_for(&Owner::session(cookie.as_bytes())).unwrap()),
            [RunId::new(&id)]
        );
    }

    #[test]
    fn the_index_checkpoints_a_run_at_its_task_and_its_decision_point_then_ends_it() {
        let root = TempRoot::new("checkpoints");
        let store = open_store(&root);
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        deliberate_to_awaiting(&mut log);
        let stored = log.snapshot_from(0).unwrap();
        let decision = log.append(decision_event(&run_id, true)).unwrap();
        let heads: Vec<(IndexKind, Option<u64>, Option<String>)> = store
            .index_records()
            .unwrap()
            .into_iter()
            .map(|r| (r.kind, r.head_sequence, r.head_hash))
            .collect();
        let (task, awaiting) = (&stored[0], stored.last().unwrap());
        // Only where the run may rest; the deliberation in between is not
        // checkpointed.
        assert!(awaiting.sequence > 2);
        assert_eq!(
            heads,
            [
                (IndexKind::Created, None, None),
                (IndexKind::Checkpoint, Some(1), Some(task.hash.clone())),
                (
                    IndexKind::Checkpoint,
                    Some(awaiting.sequence),
                    Some(awaiting.hash.clone())
                ),
                (
                    IndexKind::Terminal,
                    Some(decision.sequence),
                    Some(decision.hash.clone())
                ),
            ]
        );
        assert_eq!(store.index_write_error(), None);
    }

    #[test]
    fn a_run_awaiting_its_decision_that_lost_records_is_reported_and_left_as_found() {
        let root = TempRoot::new("resting-truncated");
        let store = open_store(&root);
        let dropped = resting_run(&store, visitor(1));
        let cut = resting_run(&store, visitor(1));
        drop(store);
        // One loses its last record, the recommendation, so it looks as if
        // it were still deliberating; the other is cut inside that record,
        // as a torn write would leave it.
        let path = root.file(&dropped, ".jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        let last = text.strip_suffix('\n').unwrap().rfind('\n').unwrap() + 1;
        std::fs::write(&path, &text[..last]).unwrap();
        let path = root.file(&cut, ".jsonl");
        let text = std::fs::read(&path).unwrap();
        std::fs::write(&path, &text[..text.len() - 20]).unwrap();
        let before = tree(root.path());

        let store = open_store(&root);
        let report = store.recover().unwrap();
        assert_eq!(
            inconsistent_ids(&report),
            sorted(vec![dropped.clone(), cut.clone()]),
            "{report:?}"
        );
        assert!(
            report.interrupted.is_empty()
                && report.truncated_tails.is_empty()
                && report.terminal_recorded.is_empty(),
            "never closed, cut or re-indexed: {report:?}"
        );
        for run_id in [&dropped, &cut] {
            let opened = store.open_run(run_id.as_str());
            assert!(
                matches!(opened, Err(RunStoreError::Inconsistent { .. })),
                "{opened:?}"
            );
            let read = store.read_run(run_id.as_str(), &visitor(1));
            assert!(
                matches!(read, Err(RunStoreError::Inconsistent { .. })),
                "{read:?}"
            );
        }
        assert_eq!(tree(root.path()), before, "left byte for byte as found");
    }

    #[test]
    fn a_run_rewritten_and_rehashed_is_refused_and_left_as_found() {
        let root = TempRoot::new("rehashed");
        let store = open_store(&root);
        let run_id = resting_run(&store, visitor(1));
        drop(store);
        // A forger edits the task and re-hashes the whole chain: every
        // record verifies, and only the index knows the run's real head.
        let path = root.file(&run_id, ".jsonl");
        let original = verify_log_bytes(&std::fs::read(&path).unwrap()).unwrap();
        let forged_path = root.path().join("forged.jsonl");
        let mut forged = EventLog::open(&forged_path).unwrap();
        for event in &original {
            let mut payload = event.payload.clone();
            if let EventPayload::TaskCreated { task } = &mut payload {
                task.title = "forged".into();
            }
            forged
                .append(NewEvent {
                    event_id: event.event_id.clone(),
                    run_id: event.run_id.clone(),
                    actor: event.actor.clone(),
                    occurred_at: event.occurred_at.clone(),
                    payload,
                })
                .unwrap();
        }
        drop(forged);
        std::fs::rename(&forged_path, &path).unwrap();
        let rewritten = verify_log_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rewritten.len(), original.len());
        assert_eq!(
            check_run_events(&run_id, &rewritten).unwrap().state,
            RunState::AwaitingHumanApproval,
            "the forgery is internally consistent"
        );
        let before = tree(root.path());

        let store = open_store(&root);
        let opened = store.open_run(run_id.as_str());
        assert!(
            matches!(&opened, Err(RunStoreError::Inconsistent { detail, .. }) if detail == HEAD_MISMATCH),
            "{opened:?}"
        );
        let read = store.read_run(run_id.as_str(), &visitor(1));
        assert!(
            matches!(read, Err(RunStoreError::Inconsistent { .. })),
            "{read:?}"
        );
        let report = store.recover().unwrap();
        assert_eq!(inconsistent_ids(&report), [run_id], "{report:?}");
        assert!(report.interrupted.is_empty(), "{report:?}");
        assert_eq!(tree(root.path()), before);
    }

    #[test]
    fn recovery_records_the_checkpoint_a_run_awaiting_its_decision_is_missing() {
        let root = TempRoot::new("missing-checkpoint");
        let store = open_store(&root);
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        let s = retry_constraint_scenario();
        let mut orch = Orchestrator {
            model: s.model,
            memory: s.memory,
            verifier: s.verifier,
            clock: FixedClock::new(T0),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        let (agg, task) = orch.create_task(&mut log, run_id.clone(), s.input).unwrap();
        // The checkpoint at the decision point never reaches the index.
        FAIL_INDEX_APPEND_AFTER.with(|fail| fail.set(Some(5)));
        let agg = orch
            .deliberate(&mut log, agg, &task, &RunControl::new())
            .unwrap();
        assert_eq!(agg.state, RunState::AwaitingHumanApproval);
        let error = store.index_write_error().unwrap();
        assert!(error.contains("checkpoint head not indexed"), "{error}");
        let head = log.read(|l| l.head_hash().unwrap().to_owned()).unwrap();
        drop(log);

        let report = store.recover().unwrap();
        assert_eq!(
            report.checkpoints_recorded,
            vec![run_id.clone()],
            "{report:?}"
        );
        assert!(
            report.interrupted.is_empty(),
            "a resting run is not interrupted"
        );
        let last = store.index_records().unwrap().pop().unwrap();
        assert_eq!(
            (last.kind, last.run_id, last.head_hash),
            (IndexKind::Checkpoint, run_id, Some(head))
        );
        assert!(store.recover().unwrap().checkpoints_recorded.is_empty());
    }

    #[test]
    fn a_run_cut_inside_its_terminal_record_or_extended_after_it_is_left_as_found() {
        let root = TempRoot::new("terminal-cut");
        let store = open_store(&root);
        let (cut, _) = ended_run(&store, visitor(1), Retention::VISITOR);
        let (extended, _) = ended_run(&store, visitor(1), Retention::VISITOR);
        drop(store);
        // Cut inside the terminal record, which EventLog::open would take
        // for a torn write and cut off, and a fragment after the terminal
        // record, which it would cut too.
        let path = root.file(&cut, ".jsonl");
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 30]).unwrap();
        let path = root.file(&extended, ".jsonl");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"schema_version\":\"1.2\",\"seq");
        std::fs::write(&path, &bytes).unwrap();
        let before = tree(root.path());

        let store = open_store(&root);
        let report = store.recover().unwrap();
        assert_eq!(
            inconsistent_ids(&report),
            sorted(vec![cut.clone(), extended.clone()]),
            "{report:?}"
        );
        assert!(
            report.truncated_tails.is_empty() && report.interrupted.is_empty(),
            "{report:?}"
        );
        assert_eq!(tree(root.path()), before, "recovery wrote nothing");
        for run_id in [&cut, &extended] {
            let opened = store.open_run(run_id.as_str());
            assert!(
                matches!(opened, Err(RunStoreError::Inconsistent { .. })),
                "{opened:?}"
            );
            let read = store.read_run(run_id.as_str(), &visitor(1));
            assert!(
                matches!(read, Err(RunStoreError::Inconsistent { .. })),
                "{read:?}"
            );
        }
        assert_eq!(tree(root.path()), before, "neither did opening or reading");
    }

    #[test]
    fn owners_read_a_stored_run_at_once_and_nothing_changes_meanwhile() {
        let root = TempRoot::new("readers");
        let store = open_store(&root);
        let (run_id, head) = ended_run(&store, visitor(1), Retention::VISITOR);
        let id = run_id.as_str();
        let before = tree(root.path());
        let first = store.read_run(id, &visitor(1)).unwrap();
        let second = store.read_run(id, &visitor(1)).unwrap();
        assert_eq!(first.events(), second.events());
        assert_eq!(first.events().len(), 2);
        assert_eq!(first.head_hash(), Some(head.as_str()));
        assert_eq!(first.aggregate().state, RunState::Failed);
        assert_eq!(first.snapshot_from(1), &second.events()[1..]);
        assert!(first.snapshot_from(2).is_empty() && first.snapshot_from(9).is_empty());
        assert_eq!(*first.meta(), store.meta(id).unwrap());
        // While readers hold the run, no owner, recovery or purge touches
        // it.
        assert!(matches!(store.open_run(id), Err(RunStoreError::InUse(_))));
        assert_eq!(store.recover().unwrap().in_use, vec![run_id.clone()]);
        let due = time("2026-11-03T00:00:00Z");
        assert_eq!(
            store.purge_expired(due).unwrap().in_use,
            vec![run_id.clone()]
        );
        assert_eq!(tree(root.path()), before, "reads changed nothing");
        drop(first);
        assert!(
            matches!(store.open_run(id), Err(RunStoreError::InUse(_))),
            "one reader is enough"
        );
        drop(second);
        drop(store.open_run(id).unwrap());
        assert_eq!(store.purge_expired(due).unwrap().purged, [run_id]);
    }

    #[test]
    fn a_non_owner_is_told_nothing_and_never_touches_the_runs_lock_or_log() {
        let root = TempRoot::new("non-owner");
        let store = open_store(&root);
        let (run_id, _) = ended_run(&store, visitor(1), Retention::VISITOR);
        let id = run_id.as_str();
        let strangers = [visitor(2), Owner::Operator, Owner::Showcase];
        let before = tree(root.path());
        // While the owner holds the run, a stranger hears that it does not
        // exist, not that it is busy: the owner check comes first.
        let held = store.open_run(id).unwrap();
        for stranger in &strangers {
            assert!(matches!(
                store.read_run(id, stranger),
                Err(RunStoreError::UnknownRun(_))
            ));
            assert!(matches!(
                store.open_run_as(id, stranger),
                Err(RunStoreError::UnknownRun(_))
            ));
        }
        assert!(matches!(
            store.read_run(id, &visitor(1)),
            Err(RunStoreError::InUse(_))
        ));
        drop(held);
        assert_eq!(tree(root.path()), before);

        // With the log replaced by a directory and the lock file gone, a
        // stranger still learns nothing more: neither was looked at. The
        // owner hears that the run is inconsistent.
        let log = root.file(&run_id, ".jsonl");
        let lock = root.file(&run_id, ".lock");
        let saved = std::fs::read(&log).unwrap();
        std::fs::remove_file(&log).unwrap();
        std::fs::create_dir(&log).unwrap();
        for stranger in &strangers {
            assert!(matches!(
                store.read_run(id, stranger),
                Err(RunStoreError::UnknownRun(_))
            ));
        }
        let read = store.read_run(id, &visitor(1));
        assert!(
            matches!(read, Err(RunStoreError::Inconsistent { .. })),
            "{read:?}"
        );
        std::fs::remove_file(&lock).unwrap();
        for stranger in &strangers {
            assert!(matches!(
                store.read_run(id, stranger),
                Err(RunStoreError::UnknownRun(_))
            ));
            assert!(matches!(
                store.open_run_as(id, stranger),
                Err(RunStoreError::UnknownRun(_))
            ));
        }
        let read = store.read_run(id, &visitor(1));
        assert!(
            matches!(read, Err(RunStoreError::Inconsistent { .. })),
            "{read:?}"
        );
        assert!(!lock.exists(), "a read never makes a lock file");

        // Restored, it is the owner's again.
        std::fs::remove_dir(&log).unwrap();
        std::fs::write(&log, &saved).unwrap();
        std::fs::write(&lock, b"").unwrap();
        assert_eq!(store.read_run(id, &visitor(1)).unwrap().events().len(), 2);
        assert_eq!(
            store.open_run_as(id, &visitor(1)).unwrap().run_id(),
            &run_id
        );
    }

    #[test]
    fn owner_checked_paths_refuse_a_run_once_its_owners_access_has_ended() {
        let root = TempRoot::new("access-ended");
        let clock = TestClock::at(T0);
        let store = RunStore::open_with_clock(root.path(), Box::new(clock.clone())).unwrap();
        let (run_id, _) = ended_run(&store, visitor(1), Retention::VISITOR);
        let (kept, _) = ended_run(&store, Owner::Showcase, Retention::Keep);
        let id = run_id.as_str();
        clock.set("2026-10-27T23:59:59Z");
        drop(store.read_run(id, &visitor(1)).unwrap());
        drop(store.open_run_as(id, &visitor(1)).unwrap());
        clock.set("2026-10-28T00:00:00Z");
        assert!(matches!(
            store.read_run(id, &visitor(1)),
            Err(RunStoreError::UnknownRun(_))
        ));
        assert!(matches!(
            store.open_run_as(id, &visitor(1)),
            Err(RunStoreError::UnknownRun(_))
        ));
        // Stored until retention deletes it, and the runtime still opens
        // it.
        drop(store.open_run(id).unwrap());
        clock.set("2030-01-01T00:00:00Z");
        drop(store.read_run(kept.as_str(), &Owner::Showcase).unwrap());
    }

    #[test]
    fn a_read_refuses_a_torn_record_and_leaves_it_for_recovery() {
        let root = TempRoot::new("read-torn");
        let store = open_store(&root);
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        log.append(task_event(&run_id, "e1")).unwrap();
        log.append(proposal_event(&run_id, "e2")).unwrap();
        drop(log);
        // A crash mid-append, after the run's checkpoint.
        let path = root.file(&run_id, ".jsonl");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"schema_version\":\"1.2\",\"sequence\":3,\"ev");
        std::fs::write(&path, &bytes).unwrap();
        let read = store.read_run(run_id.as_str(), &visitor(1));
        assert!(
            matches!(
                read,
                Err(RunStoreError::Log(EventLogError::Corrupt { line: 3, .. }))
            ),
            "{read:?}"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "a read repairs nothing"
        );
        // Recovery, as the run's owner, cuts it and closes the run.
        let report = store.recover().unwrap();
        assert_eq!(report.truncated_tails, vec![run_id.clone()], "{report:?}");
        assert_eq!(report.interrupted, vec![run_id.clone()]);
        let view = store.read_run(run_id.as_str(), &visitor(1)).unwrap();
        assert_eq!(view.aggregate().state, RunState::Failed);
    }

    #[test]
    fn verify_log_bytes_accepts_and_refuses_exactly_what_event_log_open_does() {
        let root = TempRoot::new("verify-parity");
        let path = root.path().join("run.jsonl");
        {
            let log = EventLog::open(&path).unwrap();
            let run = RunId::new("run-parity");
            let mut log = SharedRunLog::new(run, log, Arc::new(NullPublisher)).unwrap();
            deliberate_to_awaiting(&mut log);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<String> = text.lines().map(String::from).collect();
        let join = |lines: &[String]| lines.iter().map(|l| format!("{l}\n")).collect::<String>();
        let with = |i: usize, line: String| {
            let mut lines = lines.clone();
            lines[i] = line;
            join(&lines)
        };
        let legacy = r#"{"schema_version":"1.0","sequence":1,"event_id":"e1","run_id":"run-1","actor":{"kind":"human","id":"mario"},"occurred_at":"2026-10-26T00:00:00Z","payload":{"type":"task_created","task":{"schema_version":"1.0","id":"task-1","run_id":"run-1","project_id":"p","title":"t","description":"d","created_at":"2026-10-26T00:00:00Z"}}}"#;
        let accepted = [
            ("a log the event log wrote", text.clone()),
            ("CRLF line ends", text.replace('\n', "\r\n")),
            (
                "no final newline",
                text.strip_suffix('\n').unwrap().to_string(),
            ),
            ("a pre-1.1 legacy record", format!("{legacy}\n")),
            ("empty", String::new()),
        ];
        for (case, bytes) in accepted {
            std::fs::write(&path, &bytes).unwrap();
            let read = verify_log_bytes(bytes.as_bytes()).unwrap();
            assert_eq!(read, EventLog::open(&path).unwrap().all_events(), "{case}");
        }
        let refused = [
            (
                "an edited record",
                with(1, lines[1].replacen(T0, "2026-10-27T00:00:01Z", 1)),
            ),
            (
                "a broken link",
                with(
                    1,
                    lines[1].replacen("\"prev_hash\":\"", "\"prev_hash\":\"0", 1),
                ),
            ),
            ("a sequence gap", {
                let mut lines = lines.clone();
                lines.remove(1);
                join(&lines)
            }),
            ("a duplicate record", {
                let mut lines = lines.clone();
                lines.insert(1, lines[0].clone());
                join(&lines)
            }),
            (
                "an unknown schema version",
                text.replacen(
                    "\"schema_version\":\"1.2\"",
                    "\"schema_version\":\"9.9\"",
                    1,
                ),
            ),
            ("a corrupt line inside", with(1, "not json".into())),
            ("an empty line inside", {
                let mut lines = lines.clone();
                lines.insert(2, String::new());
                join(&lines)
            }),
            (
                "an unchained record after chained ones",
                format!(
                    "{text}{}\n",
                    legacy.replacen("\"sequence\":1", "\"sequence\":99", 1)
                ),
            ),
        ];
        for (case, bytes) in refused {
            assert_ne!(bytes, text, "{case}");
            std::fs::write(&path, &bytes).unwrap();
            let opened = EventLog::open(&path).unwrap_err().to_string();
            let read = verify_log_bytes(bytes.as_bytes()).unwrap_err().to_string();
            assert_eq!(read, opened, "{case}");
        }
        // A torn final record: EventLog::open cuts it, a read refuses it.
        let whole = verify_log_bytes(text.as_bytes()).unwrap();
        let torn = format!("{text}{}", &lines[1][..40]);
        let read = verify_log_bytes(torn.as_bytes());
        assert!(
            matches!(read, Err(EventLogError::Corrupt { line, .. }) if line == lines.len() + 1),
            "{read:?}"
        );
        std::fs::write(&path, &torn).unwrap();
        let repaired = EventLog::open(&path).unwrap();
        assert!(repaired.recovered_truncated_tail);
        assert_eq!(repaired.all_events(), whole);
    }

    #[test]
    fn a_task_naming_another_run_never_reaches_a_stored_run() {
        let root = TempRoot::new("foreign-task");
        let store = open_store(&root);
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        let log = run.into_shared(Arc::new(NullPublisher));
        let mut foreign = task_event(&run_id, "e1");
        if let EventPayload::TaskCreated { task } = &mut foreign.payload {
            task.run_id = RunId::new("run-elsewhere");
        }
        let refused = log.append_validated(foreign);
        assert!(
            matches!(
                refused,
                Err(EventLogError::Replay(TransitionError::RunMismatch))
            ),
            "{refused:?}"
        );
        assert!(log.snapshot_from(0).unwrap().is_empty());
        drop(log);
        // The run is still whole: recovery has nothing to say about it, and
        // its owner can still give it its real task.
        let report = store.recover().unwrap();
        assert!(
            report.inconsistent.is_empty() && report.unreplayable.is_empty(),
            "{report:?}"
        );
        let log = store
            .open_run(run_id.as_str())
            .unwrap()
            .into_shared(Arc::new(NullPublisher));
        log.append_validated(task_event(&run_id, "e1")).unwrap();
    }

    #[test]
    fn a_sidecar_edited_in_canonical_form_is_provenance_inconsistent() {
        let root = TempRoot::new("sidecar-digest");
        let store = open_store(&root);
        let (run_id, _) = ended_run(&store, visitor(1), Retention::VISITOR);
        drop(store);
        let path = root.file(&run_id, ".meta.json");
        let original = std::fs::read(&path).unwrap();
        let meta = read_meta(&original, &run_id).unwrap();
        let edits = [
            (
                "another visitor's session",
                RunMeta {
                    owner: visitor(2),
                    ..meta.clone()
                },
            ),
            (
                "the operator",
                RunMeta {
                    owner: Owner::Operator,
                    ..meta.clone()
                },
            ),
            (
                "access until deletion",
                RunMeta {
                    access_expires_at: meta.delete_after.clone(),
                    ..meta.clone()
                },
            ),
            (
                "never deleted",
                RunMeta {
                    access_expires_at: None,
                    delete_after: None,
                    ..meta.clone()
                },
            ),
        ];
        for (case, edited) in edits {
            // Each is a sidecar this store could have written itself.
            let bytes = meta_bytes(&edited).unwrap();
            assert_eq!(read_meta(&bytes, &run_id).unwrap(), edited, "{case}");
            std::fs::write(&path, &bytes).unwrap();
            let store = open_store(&root);
            let found = store.meta(run_id.as_str());
            assert!(
                matches!(&found, Err(RunStoreError::Inconsistent { detail, .. }) if detail.contains("run index recorded")),
                "{case}: {found:?}"
            );
            let opened = store.open_run(run_id.as_str());
            assert!(
                matches!(opened, Err(RunStoreError::Inconsistent { .. })),
                "{case}: {opened:?}"
            );
            let read = store.read_run(run_id.as_str(), &edited.owner);
            assert!(
                matches!(read, Err(RunStoreError::Inconsistent { .. })),
                "{case}: {read:?}"
            );
            for owner in [&edited.owner, &visitor(1)] {
                assert!(store.list_for(owner).unwrap().is_empty(), "{case}");
            }
            let report = store.purge_expired(time("2030-01-01T00:00:00Z")).unwrap();
            assert!(report.purged.is_empty(), "{case}: {report:?}");
        }
        std::fs::write(&path, &original).unwrap();
        assert_eq!(
            run_ids(open_store(&root).list_for(&visitor(1)).unwrap()),
            [run_id]
        );
    }

    #[test]
    fn a_lost_run_index_keeps_the_store_closed_unless_an_empty_one_is_accepted() {
        let open = |root: &Path, missing| {
            RunStore::open_with(root, Box::new(FixedClock::new(T0)), missing)
        };
        // A directory no store has used gets a new index.
        let fresh = TempRoot::new("index-fresh");
        drop(open(fresh.path(), MissingIndex::Refuse).unwrap());
        assert!(fresh.runs().join(INDEX_FILE).is_file());

        // The index lost beside its lock file and a run: refused, and
        // nothing is created.
        let root = TempRoot::new("index-lost");
        let store = open_store(&root);
        let (run_id, _) = ended_run(&store, visitor(1), Retention::VISITOR);
        drop(store);
        std::fs::remove_file(root.runs().join(INDEX_FILE)).unwrap();
        let before = tree(root.path());
        let refused = open(root.path(), MissingIndex::Refuse);
        assert!(
            matches!(&refused, Err(RunStoreError::IndexMissing { found }) if found.contains(INDEX_LOCK_FILE)),
            "{refused:?}"
        );
        // The run's files alone show it too.
        std::fs::remove_file(root.runs().join(INDEX_LOCK_FILE)).unwrap();
        let refused = open(root.path(), MissingIndex::Refuse);
        assert!(
            matches!(&refused, Err(RunStoreError::IndexMissing { found }) if found.contains("1 found")),
            "{refused:?}"
        );
        let mut without_lock = before.clone();
        without_lock.retain(|(path, _)| !path.ends_with(INDEX_LOCK_FILE));
        assert_eq!(tree(root.path()), without_lock, "refusing created nothing");

        // Started empty on purpose, the old run is unindexed: never served,
        // reported.
        let store = open(root.path(), MissingIndex::StartEmpty).unwrap();
        assert!(store.index_records().unwrap().is_empty());
        assert_eq!(store.recover().unwrap().unindexed, vec![run_id.clone()]);
        assert!(matches!(
            store.read_run(run_id.as_str(), &visitor(1)),
            Err(RunStoreError::UnknownRun(_))
        ));

        // Only the index's lock file left: still refused.
        let lock_only = TempRoot::new("index-lock-only");
        std::fs::create_dir_all(lock_only.runs()).unwrap();
        std::fs::write(lock_only.runs().join(INDEX_LOCK_FILE), b"").unwrap();
        assert!(matches!(
            open(lock_only.path(), MissingIndex::Refuse),
            Err(RunStoreError::IndexMissing { .. })
        ));
    }

    #[test]
    fn health_reports_the_index_head_a_cut_tail_and_an_index_edited_while_open() {
        let root = TempRoot::new("health");
        let store = open_store(&root);
        ended_run(&store, visitor(1), Retention::VISITOR);
        assert_eq!(
            store.health(),
            StoreHealth {
                index_head: Some(store.index_head().unwrap()),
                ..StoreHealth::default()
            }
        );
        drop(store);
        // A torn final record is cut when the store opens, and health says
        // so.
        let index_path = root.runs().join(INDEX_FILE);
        let original = std::fs::read_to_string(&index_path).unwrap();
        std::fs::write(&index_path, format!("{original}{{\"seq\":4,\"ru")).unwrap();
        let store = open_store(&root);
        let health = store.health();
        assert!(
            health.index_tail_repaired && !health.index_tampered,
            "{health:?}"
        );
        assert_eq!(health.index_head.map(|head| head.seq), Some(3));

        // An edit of a verified record that keeps the file's length, made
        // while the store is open: reading appends alone would not see it,
        // health does.
        let edited = original.replacen(T0, "2026-10-27T00:00:01Z", 1);
        assert_eq!(edited.len(), original.len());
        std::fs::write(&index_path, &edited).unwrap();
        let health = store.health();
        assert!(health.index_tampered, "{health:?}");
        assert_eq!(health.index_head, None);
        assert!(
            health
                .index_error
                .as_deref()
                .is_some_and(|e| e.contains("fails verification")),
            "{health:?}"
        );
        // And the store does not open on it.
        let reopened = RunStore::open_with_clock(root.path(), Box::new(FixedClock::new(T0)));
        assert!(
            matches!(reopened, Err(RunStoreError::IndexTampered { .. })),
            "{reopened:?}"
        );
    }

    #[test]
    fn list_for_reports_a_failure_instead_of_a_shorter_list() {
        let root = TempRoot::new("list-errors");
        ended_run(&open_store(&root), visitor(1), Retention::VISITOR);
        // A fresh store has no sidecar cached, so listing has to read one.
        let store = open_store(&root);
        let shared = store.shared.clone();
        let poisoned = std::thread::spawn(move || {
            let _catalog = shared.catalog.lock().unwrap();
            panic!("a thread dies holding the sidecar catalog");
        })
        .join();
        assert!(poisoned.is_err());
        let listed = store.list_for(&visitor(1));
        assert!(
            matches!(listed, Err(RunStoreError::Poisoned("run catalog"))),
            "{listed:?}"
        );
    }

    /// Offers the same id every time.
    struct Repeating(&'static str);

    impl IdGen for Repeating {
        fn next(&mut self, _prefix: &str) -> String {
            self.0.to_string()
        }
    }

    #[test]
    fn a_generator_that_repeats_a_stored_id_still_closes_an_interrupted_run() {
        let root = TempRoot::new("repeated-ids");
        let store = open_store(&root);
        let run = store.create_run(visitor(1), Retention::VISITOR).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        log.append(task_event(&run_id, "e1")).unwrap();
        log.append(proposal_event(&run_id, "e2")).unwrap();
        drop(log);
        // Every id it offers is the task's. Appended as offered, the record
        // would be taken for a retry of the task and nothing would close
        // the run.
        let report = store.recover_with(&mut Repeating("e1")).unwrap();
        assert_eq!(report.interrupted, vec![run_id.clone()], "{report:?}");
        assert!(
            report.failed.is_empty() && report.unreplayable.is_empty(),
            "{report:?}"
        );
        let handle = store.open_run(run_id.as_str()).unwrap();
        let ids: Vec<&str> = handle
            .log()
            .all_events()
            .iter()
            .map(|e| e.event_id.as_str())
            .collect();
        assert_eq!(ids, ["e1", "e2", "e1-1"]);
        assert_eq!(
            handle.log().rebuild(&run_id).unwrap().state,
            RunState::Failed
        );
    }

    #[test]
    fn write_atomic_never_leaves_a_partial_file_at_its_path() {
        let root = TempRoot::new("write-atomic");
        let path = root.path().join("x.meta.json");
        let tmp = root.path().join("x.meta.json.tmp");
        let bytes = b"{\"whole\":true}\n";
        for cut in [0, 1, 7, bytes.len(), usize::MAX] {
            FAIL_SIDECAR_WRITE_AFTER.with(|fail| fail.set(Some(cut)));
            let err = write_atomic(&path, &tmp, bytes).unwrap_err();
            assert!(err.to_string().contains("injected"), "{err}");
            assert!(
                !exists(&path).unwrap(),
                "cut after {cut}: nothing at the path"
            );
            assert!(!exists(&tmp).unwrap(), "cut after {cut}: temporary removed");
        }
        // Over a file already there, a failed write leaves it as it was.
        std::fs::write(&path, b"old\n").unwrap();
        FAIL_SIDECAR_WRITE_AFTER.with(|fail| fail.set(Some(3)));
        write_atomic(&path, &tmp, bytes).unwrap_err();
        assert_eq!(std::fs::read(&path).unwrap(), b"old\n");
        assert!(!exists(&tmp).unwrap());
        // A write that completes replaces it whole.
        write_atomic(&path, &tmp, bytes).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!exists(&tmp).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_log_replaced_by_a_symlink_is_refused() {
        let root = TempRoot::new("symlink");
        let store = open_store(&root);
        let (run_id, _) = ended_run(&store, Owner::Operator, Retention::Keep);
        let log = root.file(&run_id, ".jsonl");
        let elsewhere = root.path().join("elsewhere.jsonl");
        std::fs::rename(&log, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &log).unwrap();
        let opened = store.open_run(run_id.as_str());
        assert!(
            matches!(opened, Err(RunStoreError::Inconsistent { .. })),
            "{opened:?}"
        );
    }
}
