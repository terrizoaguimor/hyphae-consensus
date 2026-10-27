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
//! - **Sidecar.** `<run_id>.meta.json` holds the owner (a session only as
//!   the SHA-256 of its id), the creation time and the retention. It is
//!   written to a temporary file, flushed, fsync'd and renamed into place,
//!   so no partial sidecar is ever visible. It is read strictly (only the
//!   exact bytes this store writes) and checked against the run's file
//!   name and its events; any mismatch is [`RunStoreError::Inconsistent`]
//!   ("provenance inconsistent").
//! - **Run index (LC-12).** Per-run chains have no cross-run anchor, so a
//!   run rewritten and re-hashed, truncated or deleted would otherwise go
//!   unnoticed. `index.jsonl` records each run's creation, its terminal
//!   head and its purge. Records chain like event records: SHA-256 over
//!   `prev_hash` and the record's canonical JSON with `hash` blanked. The
//!   index is verified whenever the store opens (any edit is
//!   [`RunStoreError::IndexTampered`]), a run whose head differs from its
//!   recorded terminal head is refused, and recovery reports indexed runs
//!   whose files are gone. The index is unsigned as well: it detects
//!   changes made after someone obtained its head ([`RunStore::index_head`]),
//!   not a rewrite of the whole store at once.
//! - **Retention.** [`RunStore::purge_expired`] deletes an expired visitor
//!   run's files whole and records the purge. Operator and showcase runs
//!   are never purged.

use crate::clock::{Clock, IdGen, SystemClock, UuidIdGen};
use crate::event_log::{EventLog, EventLogError};
use crate::orchestrator::mark_interrupted_runs;
use crate::shared_log::{EventPublisher, SharedRunLog};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use consensus_domain::{ConsensusEvent, EventPayload, RunId};
use serde::{Deserialize, Serialize};
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

const HEAD_MISMATCH: &str = "the event log's head differs from the terminal head in the run index: the log was truncated or rewritten";

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
    /// A visitor's session, stored only as the lowercase hex SHA-256 of
    /// the session id: the sidecar never holds the id itself.
    Session { hash: String },
    /// The operator.
    Operator,
    /// A curated showcase run.
    Showcase,
}

impl Owner {
    /// The owner for the session whose secret id is `session_id`.
    pub fn session(session_id: &[u8]) -> Self {
        Owner::Session {
            hash: sha256_hex(session_id),
        }
    }

    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Owner::Session { hash } if !is_sha256_hex(hash) => Err(
                "a session owner is the SHA-256 hex digest of the session id, never the id itself",
            ),
            _ => Ok(()),
        }
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

/// What a run index record says happened to a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexKind {
    /// The run's files were created. Its log is still empty, so
    /// `head_hash` is None.
    Created,
    /// The run reached a terminal state; `head_hash` is its log's last
    /// record.
    Terminal,
    /// Retention deleted the run's files; `head_hash` is the head they
    /// had, if the log could still be read.
    Purged,
}

impl IndexKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IndexKind::Created => "created",
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
    /// The run log's chain head at this point.
    pub head_hash: Option<String>,
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
    /// Never created here, or purged. An API answers 404, as for
    /// [`InvalidId`](Self::InvalidId).
    #[error("unknown run {0}")]
    UnknownRun(RunId),
    /// Another handle, in this or another process, owns the run.
    #[error("run {0} is owned by another handle or process")]
    InUse(RunId),
    /// The run's files disagree with each other or with the run index.
    #[error("run {run_id}: provenance inconsistent: {detail}")]
    Inconsistent { run_id: RunId, detail: String },
    /// The run index fails verification: a record edited, removed,
    /// reordered or added by hand.
    #[error("run index fails verification at record {line}: {detail}")]
    IndexTampered { line: usize, detail: String },
    #[error("invalid owner: {0}")]
    InvalidOwner(&'static str),
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
    /// Logs whose torn final record (a crash mid-append) was removed.
    pub truncated_tails: Vec<RunId>,
    /// Logs that fail to open or replay. Left as they are, never rewritten.
    pub unreplayable: Vec<(RunId, String)>,
    /// Runs whose files disagree with each other or with the index: a bad
    /// sidecar, another run's records, or a head other than the recorded
    /// terminal head. A log with a contradicted head is left as it is.
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
    /// Expired, but owned by a live handle; left for the next purge.
    pub in_use: Vec<RunId>,
    pub failed: Vec<(RunId, String)>,
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
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RunStoreError> {
        Self::open_with_clock(root, Box::new(SystemClock))
    }

    /// [`open`](Self::open) with the clock that stamps sidecars and index
    /// records.
    pub fn open_with_clock(
        root: impl AsRef<Path>,
        clock: Box<dyn Clock + Send + Sync>,
    ) -> Result<Self, RunStoreError> {
        let dir = root.as_ref().join(RUNS_DIR);
        std::fs::create_dir_all(&dir)?;
        let index = RunIndex::open(&dir)?;
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
        owner.validate().map_err(RunStoreError::InvalidOwner)?;
        let now = self.shared.now()?;
        let run_id = parse_run_id(&UuidIdGen.next("run"))?;
        let meta = RunMeta::for_new_run(run_id.clone(), owner, now, retention)?;
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
        write_atomic(&paths.meta, &paths.meta_tmp, &meta_bytes(&meta)?)?;
        let log = EventLog::open(&paths.log)?;
        sync_dir(Some(&self.shared.dir))?;
        // Indexed last: a crash before this point leaves files the index
        // never mentions, which are never served and which recovery
        // reports as unindexed.
        self.shared
            .index()?
            .append(&run_id, IndexKind::Created, None, meta.created_at.clone())?;
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

    /// Opens a live run and takes ownership of it.
    ///
    /// The id is parsed before anything else, and an id the run index
    /// does not list as live is [`RunStoreError::UnknownRun`] without any
    /// file access beyond the index. For a known run nothing is created:
    /// the lock file is opened, never made, and locked
    /// ([`RunStoreError::InUse`] if held); then the sidecar must pass its
    /// checks, the log must be a regular file that opens, holds only this
    /// run's records and replays, and a terminal head recorded in the
    /// index must be the log's head. Opening may remove a torn final
    /// record, as [`EventLog::open`] does; the caller is the run's owner.
    pub fn open_run(&self, id: &str) -> Result<RunHandle, RunStoreError> {
        let run_id = parse_run_id(id)?;
        if !self.indexed(&run_id)?.is_some_and(|run| !run.purged) {
            return Err(RunStoreError::UnknownRun(run_id));
        }
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
        // recovers the run until this handle lets it go.
        let meta = self.load_meta(&run_id)?;
        if !is_regular_file(&paths.log)? {
            return Err(self.vanished(&run_id, "event log missing or not a regular file"));
        }
        let log = EventLog::open(&paths.log)?;
        check_run_log(&run_id, &log)?;
        if let Some(recorded) = self.indexed(&run_id)?.and_then(|run| run.terminal_head) {
            if log.head_hash() != Some(recorded.as_str()) {
                return Err(inconsistent(&run_id, HEAD_MISMATCH));
            }
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

    /// A live run's sidecar, without opening or locking the run: the
    /// owner check an API makes before it touches the log. Unknown and
    /// malformed ids are refused as in [`open_run`](Self::open_run).
    pub fn meta(&self, id: &str) -> Result<RunMeta, RunStoreError> {
        let run_id = parse_run_id(id)?;
        match self.indexed(&run_id)? {
            Some(run) if !run.purged => self.load_meta(&run_id),
            _ => Err(RunStoreError::UnknownRun(run_id)),
        }
    }

    /// The live runs (created, not purged) that `owner` owns, oldest
    /// first. A run whose sidecar fails its checks is left out; recovery
    /// reports it.
    pub fn list_for(&self, owner: &Owner) -> Result<Vec<RunMeta>, RunStoreError> {
        let mut runs = Vec::new();
        for (run_id, run) in self.index_view()? {
            if run.purged {
                continue;
            }
            if let Ok(meta) = self.load_meta(&run_id) {
                if meta.owner == *owner {
                    runs.push(meta);
                }
            }
        }
        Ok(runs)
    }

    /// Startup recovery: call once after [`open`](Self::open), before
    /// serving. For every indexed run whose lock it can take:
    ///
    /// - a run left mid-deliberation is closed by
    ///   [`mark_interrupted_runs`] on its own file (pass [`UuidIdGen`]:
    ///   the new event id must not collide with the run's stored ids);
    /// - a terminal run whose head is not in the index gets it recorded;
    /// - a run whose head contradicts its recorded terminal head, whose
    ///   log does not open or replay, or whose sidecar is bad is reported
    ///   and its log left as it is;
    /// - a recorded purge whose files remain is finished.
    ///
    /// Runs another live owner holds are skipped, indexed runs without a
    /// log are reported missing, and logs the index never recorded are
    /// reported, never adopted. Only store-level failures (the run index)
    /// are errors.
    pub fn recover<I: IdGen>(&self, ids: &mut I) -> Result<RecoveryReport, RunStoreError> {
        let now = stamp(self.shared.now()?);
        let indexed = self.index_view()?;
        let mut report = RecoveryReport {
            index_tail_repaired: self.shared.index()?.state.repaired_tail,
            ..RecoveryReport::default()
        };
        let (logs, files) = self.scan_runs()?;
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
            self.recover_run(run_id, run, &paths, ids, &now, &mut report)?;
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
    /// head its log had. A run some handle still owns is left for the next
    /// purge. Operator and showcase runs are never deleted, whatever their
    /// retention says.
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
            let meta = match self.load_meta(&run_id) {
                Ok(meta) => meta,
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

    /// The last failure to index a terminal head while a run was ending,
    /// if any. The run's own log is unaffected, and
    /// [`recover`](Self::recover) records the missing head at the next
    /// start.
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
    /// never rewritten.
    fn load_meta(&self, run_id: &RunId) -> Result<RunMeta, RunStoreError> {
        if let Some(meta) = self.shared.catalog()?.get(run_id) {
            return Ok(meta.clone());
        }
        let paths = RunPaths::new(&self.shared.dir, run_id)?;
        if !is_regular_file(&paths.meta)? {
            return Err(self.vanished(run_id, "sidecar missing or not a regular file"));
        }
        let bytes = std::fs::read(&paths.meta)?;
        let meta = read_meta(&bytes, run_id).map_err(|detail| inconsistent(run_id, detail))?;
        self.shared.catalog()?.insert(run_id.clone(), meta.clone());
        Ok(meta)
    }

    /// Run ids with a regular `<run_id>.jsonl` file, and run ids with any
    /// run file at all (the debris of a creation that never finished
    /// included). Other names, the index's among them, belong to no run.
    fn scan_runs(&self) -> Result<(BTreeSet<RunId>, BTreeSet<RunId>), RunStoreError> {
        let (mut logs, mut any) = (BTreeSet::new(), BTreeSet::new());
        for entry in std::fs::read_dir(&self.shared.dir)? {
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

    fn recover_run<I: IdGen>(
        &self,
        run_id: &RunId,
        indexed: &IndexedRun,
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
        if let Err(e) = self.load_meta(run_id) {
            // A bad sidecar keeps the run from being served, but a run left
            // mid-deliberation is still closed.
            report.inconsistent.push((run_id.clone(), e.to_string()));
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
        if let Some(recorded) = &indexed.terminal_head {
            // Terminal and indexed: nothing to write. A log that disagrees
            // with its recorded head is evidence, so it is not "repaired"
            // by closing it again.
            if log.head_hash() != Some(recorded.as_str()) {
                report
                    .inconsistent
                    .push((run_id.clone(), HEAD_MISMATCH.to_string()));
            }
            return Ok(());
        }
        let recovery = match mark_interrupted_runs(&mut log, ids, now.to_string()) {
            Ok(recovery) => recovery,
            Err(e) => {
                report.failed.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        report.interrupted.extend(recovery.interrupted);
        report.unreplayable.extend(recovery.unreplayable);
        let terminal = match log.rebuild(run_id) {
            Ok(agg) => agg.state.is_terminal(),
            Err(e) => {
                report.unreplayable.push((run_id.clone(), e.to_string()));
                return Ok(());
            }
        };
        if terminal {
            match log.head_hash() {
                Some(head) => {
                    if self
                        .shared
                        .index()?
                        .record_terminal(run_id, head, now.to_string())?
                    {
                        report.terminal_recorded.push(run_id.clone());
                    }
                }
                None => report
                    .inconsistent
                    .push((run_id.clone(), "a terminal run without a chain head".into())),
            }
        }
        Ok(())
    }

    fn purge_run(&self, run_id: &RunId, paths: &RunPaths, at: &str) -> Result<(), RunStoreError> {
        let lock = open_lock_for_known_run(paths)?;
        acquire_run_lock(&lock, run_id)?;
        // A log that no longer opens is deleted all the same (retention is
        // a promise to the visitor), with no head recorded.
        let head = if is_regular_file(&paths.log)? {
            EventLog::open(&paths.log)
                .ok()
                .and_then(|log| log.head_hash().map(str::to_owned))
        } else {
            None
        };
        // Recorded before anything is deleted: a crash in between leaves a
        // recorded purge, which the next purge or recovery finishes, never
        // a run that looks deleted by hand.
        self.shared
            .index()?
            .append(run_id, IndexKind::Purged, head, at.to_string())?;
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
/// shared log made from it, is dropped.
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
    /// and records the run's terminal head in the run index when an
    /// append ends the run.
    pub fn into_shared(self, publisher: Arc<dyn EventPublisher>) -> SharedRunLog {
        SharedRunLog::leased(self.run_id, self.log, publisher, self.lease)
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
    /// Records the run's terminal head (`last`, already durable) in the
    /// run index. The record is acknowledged by then, so a failure here
    /// is not an append failure: it is kept for
    /// [`RunStore::index_write_error`], and recovery records the missing
    /// head at the next start.
    pub(crate) fn record_terminal(&self, last: &ConsensusEvent) {
        let recorded = self.store.now().and_then(|now| {
            self.store
                .index()?
                .record_terminal(&self.run_id, &last.hash, stamp(now))
        });
        if let Err(e) = recorded {
            if let Ok(mut slot) = self.store.index_write_error.lock() {
                *slot = Some(format!(
                    "run {}: terminal head not indexed: {e}",
                    self.run_id
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
    fn open(dir: &Path) -> Result<Self, RunStoreError> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(INDEX_LOCK_FILE))?;
        let mut index = Self {
            path: dir.join(INDEX_FILE),
            lock,
            state: IndexState::default(),
        };
        {
            let _held = FileLock::acquire(&index.lock)?;
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(&index.path)?;
            index.state.refresh(&index.path)?;
        }
        Ok(index)
    }

    /// Reads other processes' appends when the file's length moved: one
    /// metadata call when it did not.
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
        head_hash: Option<String>,
        at: String,
    ) -> Result<IndexRecord, RunStoreError> {
        let _held = FileLock::acquire(&self.lock)?;
        self.state.refresh(&self.path)?;
        self.state.append(&self.path, run_id, kind, head_hash, at)
    }

    /// Records `run_id`'s terminal head unless that head is already
    /// recorded. Returns whether a record was written.
    fn record_terminal(
        &mut self,
        run_id: &RunId,
        head: &str,
        at: String,
    ) -> Result<bool, RunStoreError> {
        let _held = FileLock::acquire(&self.lock)?;
        self.state.refresh(&self.path)?;
        let recorded = self
            .state
            .runs
            .get(run_id)
            .and_then(|run| run.terminal_head.as_deref());
        if recorded == Some(head) {
            return Ok(false);
        }
        self.state.append(
            &self.path,
            run_id,
            IndexKind::Terminal,
            Some(head.to_string()),
            at,
        )?;
        Ok(true)
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
    terminal_head: Option<String>,
    purged: bool,
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
    /// that does not parse is a torn write from a crash and is cut off,
    /// and one that parses and verifies only lost its newline.
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
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let line = self.records.len() + 1;
            let tampered = move |detail: String| RunStoreError::IndexTampered { line, detail };
            match rest.iter().position(|b| *b == b'\n') {
                Some(end) => {
                    let record = parse_index_line(&rest[..end]).map_err(tampered)?;
                    self.verify(&record, &rest[..end]).map_err(tampered)?;
                    self.commit(record, end as u64 + 1);
                    rest = &rest[end + 1..];
                }
                None => {
                    match parse_index_line(rest) {
                        Ok(record) => {
                            self.verify(&record, rest).map_err(tampered)?;
                            file.seek(SeekFrom::End(0))?;
                            file.write_all(b"\n")?;
                            file.sync_all()?;
                            self.commit(record, rest.len() as u64 + 1);
                        }
                        Err(_) => {
                            file.set_len(self.verified_len)?;
                            file.sync_all()?;
                            self.repaired_tail = true;
                        }
                    }
                    break;
                }
            }
        }
        Ok(())
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
        self.check(&record.run_id, record.kind, record.head_hash.as_deref())
            .map_err(String::from)
    }

    /// The order a run's records may come in: created once and first;
    /// at most one terminal head, before any purge; purged once.
    fn check(
        &self,
        run_id: &RunId,
        kind: IndexKind,
        head: Option<&str>,
    ) -> Result<(), &'static str> {
        if head.is_some_and(|h| !is_sha256_hex(h)) {
            return Err("head_hash is not a SHA-256 hex digest");
        }
        match (kind, self.runs.get(run_id)) {
            (IndexKind::Created, None) => Ok(()),
            (IndexKind::Created, Some(_)) => Err("the run is already recorded as created"),
            (_, None) => Err("the run was never recorded as created"),
            (_, Some(run)) if run.purged => Err("the run is already recorded as purged"),
            (IndexKind::Terminal, Some(run)) if run.terminal_head.is_some() => {
                Err("the run's terminal head is already recorded")
            }
            (IndexKind::Terminal, Some(_)) if head.is_none() => {
                Err("a terminal record needs a head hash")
            }
            (IndexKind::Terminal | IndexKind::Purged, Some(_)) => Ok(()),
        }
    }

    fn commit(&mut self, record: IndexRecord, bytes: u64) {
        let run = self.runs.entry(record.run_id.clone()).or_default();
        match record.kind {
            IndexKind::Created => run.created_seq = record.seq,
            IndexKind::Terminal => run.terminal_head = record.head_hash.clone(),
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
        head_hash: Option<String>,
        at: String,
    ) -> Result<IndexRecord, RunStoreError> {
        if let Some(why) = &self.poisoned {
            return Err(index_unusable(why));
        }
        self.check(run_id, kind, head_hash.as_deref())
            .map_err(|detail| {
                inconsistent(
                    run_id,
                    format!("the run index refuses a {} record: {detail}", kind.as_str()),
                )
            })?;
        let mut record = IndexRecord {
            seq: self.records.len() as u64 + 1,
            run_id: run_id.clone(),
            kind,
            head_hash,
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
    let foreign = log.all_events().iter().any(|event| {
        event.run_id != *run_id
            || matches!(&event.payload, EventPayload::TaskCreated { task } if task.run_id != *run_id)
    });
    if foreign {
        return Err(inconsistent(
            run_id,
            "the event log holds another run's records",
        ));
    }
    log.rebuild(run_id)?;
    Ok(())
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
    meta.owner.validate().map_err(String::from)?;
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

    /// Every file under `dir`, recursively, with its length.
    fn tree(dir: &Path) -> Vec<(PathBuf, u64)> {
        let mut files = Vec::new();
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    dirs.push(entry.path());
                } else {
                    files.push((entry.path(), meta.len()));
                }
            }
        }
        files.sort();
        files
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
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        assert_eq!(lines.len(), 6, "created and terminal for each run");
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
        let mut unlinked = record(5);
        unlinked.seq = 7;
        unlinked.kind = IndexKind::Purged;
        unlinked.prev_hash = "0".repeat(64);
        unlinked.hash = index_hash(&unlinked).unwrap();
        // Added by hand and correctly linked, but a second creation.
        let mut recreated = record(0);
        recreated.seq = 7;
        recreated.prev_hash = record(5).hash;
        recreated.hash = index_hash(&recreated).unwrap();
        let appended = |extra: &IndexRecord| {
            let mut lines = lines.clone();
            lines.push(json(extra));
            text(&lines)
        };

        let cases = [
            (
                "a kind changed",
                edited(1, lines[1].replacen("\"terminal\"", "\"purged\"", 1)),
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
        assert_eq!(published.seq, 6);
        std::fs::write(&index_path, text(&lines[..5])).unwrap();
        assert_ne!(open_store(&root).index_head().unwrap(), published);

        // A torn final record, from a crash mid-append, is cut off.
        std::fs::write(&index_path, format!("{original}{}", &lines[0][..30])).unwrap();
        let store = open_store(&root);
        assert_eq!(std::fs::read_to_string(&index_path).unwrap(), original);
        assert_eq!(store.index_head().unwrap(), published);
        assert!(store.recover(&mut UuidIdGen).unwrap().index_tail_repaired);
    }

    #[test]
    fn a_run_ending_through_its_shared_log_records_its_terminal_head() {
        let root = TempRoot::new("terminal-head");
        let store = open_store(&root);
        let run = store.create_run(Owner::Operator, Retention::Keep).unwrap();
        let run_id = run.run_id().clone();
        let mut log = run.into_shared(Arc::new(NullPublisher));
        log.append(task_event(&run_id, "e1")).unwrap();
        assert_eq!(
            index_entries(&store),
            [(IndexKind::Created, run_id.clone(), None)]
        );
        let last = log.append(fail_event(&run_id, "e2")).unwrap();
        let expected = [
            (IndexKind::Created, run_id.clone(), None),
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
        log.append(task_event(&run_id, "e1")).unwrap();
        FAIL_INDEX_APPEND_AFTER.with(|fail| fail.set(Some(10)));
        let last = log.append(fail_event(&run_id, "e2")).unwrap();
        // The record is durable and acknowledged; only its index record
        // is missing, and the failure is kept for the health check.
        assert_eq!(log.aggregate().unwrap().state, RunState::Failed);
        let error = store.index_write_error().unwrap();
        assert!(error.contains("injected index append failure"), "{error}");
        assert_eq!(
            index_entries(&store),
            [(IndexKind::Created, run_id.clone(), None)]
        );
        drop(log);
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        let report = store.recover(&mut UuidIdGen).unwrap();
        assert_eq!(report.interrupted, vec![busy.clone()]);
        assert_eq!(report.terminal_recorded, [busy]);
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        let report = store.recover(&mut UuidIdGen).unwrap();
        assert_eq!(report.unindexed, expected);
        assert!(report.missing.is_empty(), "{report:?}");
        assert!(
            root.file(&debris, ".lock").exists(),
            "reported, left in place"
        );

        // Without its index a store serves no run at all, rather than
        // adopting whatever files it finds.
        drop(store);
        std::fs::remove_file(root.runs().join(INDEX_FILE)).unwrap();
        let store = open_store(&root);
        assert!(matches!(
            store.open_run(run_id.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        let mut expected = vec![run_id, planted, debris];
        expected.sort();
        assert_eq!(store.recover(&mut UuidIdGen).unwrap().unindexed, expected);
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
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        store
            .shared
            .index()
            .unwrap()
            .append(&run_id, IndexKind::Purged, Some(head), T0.into())
            .unwrap();
        drop(store);
        let store = open_store(&root);
        assert!(matches!(
            store.open_run(run_id.as_str()),
            Err(RunStoreError::UnknownRun(_))
        ));
        let report = store.recover(&mut UuidIdGen).unwrap();
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
        for hash in ["the-session-cookie-value".to_string(), "A".repeat(64)] {
            let owner = Owner::Session { hash };
            assert!(matches!(
                store.create_run(owner, Retention::Keep),
                Err(RunStoreError::InvalidOwner(_))
            ));
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
            Owner::session(b"abc"),
            Owner::Session {
                hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()
            }
        );
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
