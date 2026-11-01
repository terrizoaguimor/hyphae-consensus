// SPDX-License-Identifier: Apache-2.0
//! Visitor sessions (B5): a 256-bit random id in an `HttpOnly; Secure;
//! SameSite=Strict; Path=/` cookie named `__Host-hc_session`, held in a
//! bounded server-side table with a TTL and LRU eviction.
//!
//! A session is minted only by `POST /api/v1/session`, never by a GET or
//! a static response (ABUSE-6). The store never writes the raw id to
//! disk: a run's owner is `Owner::session(id)`, the SHA-256 of the id,
//! and decisions are signed `visitor:<pseudonym>`, a second, truncated
//! digest, so a receipt never reveals a session's owner hash either.
//!
//! The `__Host-` prefix is only valid with `Secure` (RFC 6265bis): a
//! browser silently drops a `__Host-` cookie set without it. When the
//! server is configured for plain-HTTP development
//! (`CONSENSUS_COOKIE_SECURE=false`) the cookie is therefore named
//! `hc_session`, without the prefix; a `__Host-` cookie is never emitted
//! without `Secure`.
//!
//! **Persistence (public listener).** A table built with
//! [`SessionStore::persistent`] survives a restart: it keeps, in one JSON
//! file under the run store's root ([`SESSIONS_FILE`]), only
//! `{sha256(id), created_at, expires_at}` per session, rewritten whole
//! (compact JSON) through a temporary file and an atomic rename. A
//! presented cookie is looked up by hashing it, so the raw id never touches
//! the disk (the hash is the run owner's hash, which the run sidecars
//! already store). Expired entries are not reloaded; a file that cannot be
//! read starts an empty table with a warning (every visitor then starts a
//! new session, as before persistence); a write that fails is logged and
//! the session stays valid in memory.
//!
//! The rewrite never runs on the request path: a creation, a removal or an
//! expiry only marks the table changed, and one writer thread rewrites the
//! file at most once per [`WRITE_DEBOUNCE`], however many sessions changed
//! meanwhile (the public server runs on one vCPU, where an fsync inside a
//! handler would stall every other request). The writer also sweeps expired
//! sessions out of the table and the file every [`SWEEP_INTERVAL`], so an
//! ended session's hash leaves the disk within that interval even when no
//! visitor comes. [`SessionStore::flush`] writes pending changes at once
//! (the server calls it when it stops; dropping the store does too). A
//! crash loses at most the last [`WRITE_DEBOUNCE`] of changes: those
//! visitors start a new session.

use consensus_runtime::Owner;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// The cookie's name with `Secure`. The `__Host-` prefix pins it to this
/// host, with no `Domain` and `Path=/`, so a sibling subdomain cannot
/// plant or fixate one.
pub const COOKIE_NAME: &str = "__Host-hc_session";
/// The cookie's name without `Secure` (plain-HTTP development only): the
/// prefix would make a browser refuse the cookie.
pub const PLAIN_COOKIE_NAME: &str = "hc_session";
/// Hex length of a session id: 32 random bytes.
pub const ID_HEX_LEN: usize = 64;
/// The file a persistent table keeps its sessions in, under the run
/// store's root (`/var/lib/consensus/runs` in production).
pub const SESSIONS_FILE: &str = "sessions.json";
/// The file format's version.
const SESSIONS_FILE_VERSION: u32 = 1;
/// How long the writer waits after a change for others before it rewrites
/// the sessions file: every change in that window costs one rewrite.
pub const WRITE_DEBOUNCE: Duration = Duration::from_millis(500);
/// How often the writer sweeps expired sessions out of the table and the
/// sessions file.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(300);

/// The SHA-256 of a session id, lowercase hex: the table's key, the only
/// form a session takes on disk, and the run owner's hash
/// ([`Owner::session`]).
pub fn id_hash(id: &str) -> String {
    Sha256::digest(id.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One session as the sessions file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSession {
    sha256: String,
    created_at: String,
    expires_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSessions {
    version: u32,
    sessions: Vec<StoredSession>,
}

/// RFC 3339 with the clock's own sub-second digits, so the LRU order of
/// sessions minted in the same second survives a reload.
fn rfc3339(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| SystemTime::from(t.with_timezone(&chrono::Utc)))
}

/// A session the table knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The id as the cookie carries it (lowercase hex).
    id: String,
    pub expires_at: SystemTime,
}

impl Session {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The owner stored on this session's runs.
    pub fn owner(&self) -> Owner {
        Owner::session(self.id.as_bytes())
    }

    /// `visitor:<16 hex>`: recorded as `authorized_by` on this session's
    /// decisions. Derived from the id with its own prefix, so it matches
    /// neither the id nor the owner hash.
    pub fn pseudonym(&self) -> String {
        pseudonym_of(&self.id)
    }

    /// RFC 3339 expiry, whole seconds.
    pub fn expires_at_rfc3339(&self) -> String {
        chrono::DateTime::<chrono::Utc>::from(self.expires_at)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}

pub fn pseudonym_of(id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"hyphae-consensus:visitor:");
    hasher.update(id.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("visitor:{hex}")
}

struct Entry {
    created_at: SystemTime,
    expires_at: SystemTime,
    /// Position in the LRU order.
    tick: u64,
}

/// Sessions by the hash of their id ([`id_hash`]): the raw id is never
/// kept, in memory or on disk.
struct Table {
    by_hash: HashMap<String, Entry>,
    /// LRU order: oldest tick first; the values are id hashes.
    order: BTreeMap<u64, String>,
    next_tick: u64,
    evictions: u64,
}

impl Table {
    fn insert(&mut self, hash: String, created_at: SystemTime, expires_at: SystemTime) {
        let tick = self.next_tick;
        self.next_tick += 1;
        self.order.insert(tick, hash.clone());
        self.by_hash.insert(
            hash,
            Entry {
                created_at,
                expires_at,
                tick,
            },
        );
    }

    /// Drops every session expired at `now`; how many it dropped.
    fn remove_expired(&mut self, now: SystemTime) -> usize {
        let expired: Vec<String> = self
            .by_hash
            .iter()
            .filter(|(_, e)| e.expires_at <= now)
            .map(|(hash, _)| hash.clone())
            .collect();
        for stale in &expired {
            if let Some(entry) = self.by_hash.remove(stale) {
                self.order.remove(&entry.tick);
            }
        }
        expired.len()
    }
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock holds plain data; keep serving.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What the writer thread is asked to do.
#[derive(Default)]
struct WriterSignal {
    /// The table changed since the file was last written.
    dirty: bool,
    /// The store is being dropped: write what is pending and end.
    stop: bool,
}

/// How often the writer writes and sweeps (the constants, except in tests).
#[derive(Debug, Clone, Copy)]
struct WriterTiming {
    debounce: Duration,
    sweep: Duration,
}

/// The sessions file of a persistent table and the writer's state.
struct Persister {
    file: PathBuf,
    table: Arc<Mutex<Table>>,
    /// One rewrite of the file at a time, each from the table as it then is.
    write_lock: Mutex<()>,
    signal: Mutex<WriterSignal>,
    wake: Condvar,
    /// Rewrites done (for the tests' bound on them).
    writes: AtomicU64,
}

impl Persister {
    /// The table changed: the writer rewrites the file soon. Never blocks on
    /// the disk.
    fn mark_dirty(&self) {
        lock_or_recover(&self.signal).dirty = true;
        self.wake.notify_all();
    }

    /// Rewrites the file from the table as it is now: a temporary file in
    /// the same directory, flushed to disk, then renamed over the old one,
    /// so the file is always whole. With `only_if_dirty`, nothing is written
    /// when no change is pending (another write already took it). A failure
    /// is logged (with the file, never an id) and changes nothing in memory.
    fn write_now(&self, only_if_dirty: bool) {
        let _one_at_a_time = lock_or_recover(&self.write_lock);
        {
            let mut signal = lock_or_recover(&self.signal);
            if only_if_dirty && !signal.dirty {
                return;
            }
            // A change after this point marks the table again, for a later write.
            signal.dirty = false;
        }
        // Copied under the table's lock, formatted and written outside it.
        let mut sessions: Vec<(String, SystemTime, SystemTime)> = lock_or_recover(&self.table)
            .by_hash
            .iter()
            .map(|(hash, entry)| (hash.clone(), entry.created_at, entry.expires_at))
            .collect();
        sessions.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let stored = StoredSessions {
            version: SESSIONS_FILE_VERSION,
            sessions: sessions
                .into_iter()
                .map(|(sha256, created_at, expires_at)| StoredSession {
                    sha256,
                    created_at: rfc3339(created_at),
                    expires_at: rfc3339(expires_at),
                })
                .collect(),
        };
        match write_atomically(&self.file, &stored) {
            Ok(()) => {
                self.writes.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => eprintln!(
                "[session] the sessions file {} could not be written: {e}; the sessions stay valid until a restart",
                self.file.display()
            ),
        }
    }

    /// The writer thread: waits for a change (or the next sweep), lets the
    /// changes of one `debounce` gather, and writes once; on `stop` it
    /// writes what is pending and ends.
    fn run(self: Arc<Self>, timing: WriterTiming) {
        let mut next_sweep = Instant::now() + timing.sweep;
        loop {
            let (dirty, stop) = {
                let mut signal = lock_or_recover(&self.signal);
                while !signal.dirty && !signal.stop {
                    let now = Instant::now();
                    if now >= next_sweep {
                        break;
                    }
                    signal = self
                        .wake
                        .wait_timeout(signal, next_sweep - now)
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .0;
                }
                (signal.dirty, signal.stop)
            };
            if stop {
                if dirty {
                    self.write_now(true);
                }
                return;
            }
            if !dirty {
                // The sweep: an ended session's hash leaves the disk too.
                next_sweep = Instant::now() + timing.sweep;
                let swept = lock_or_recover(&self.table).remove_expired(SystemTime::now());
                if swept > 0 {
                    self.write_now(false);
                }
                continue;
            }
            // Let the changes of one debounce gather (a stop ends the wait).
            {
                let signal = lock_or_recover(&self.signal);
                let _ = self
                    .wake
                    .wait_timeout_while(signal, timing.debounce, |s| !s.stop)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            self.write_now(true);
        }
    }
}

/// The bounded session table.
pub struct SessionStore {
    inner: Arc<Mutex<Table>>,
    ttl: Duration,
    max: usize,
    cookie_secure: bool,
    /// The sessions file of a persistent table ([`SESSIONS_FILE`]).
    persister: Option<Arc<Persister>>,
    /// The writer thread of a persistent table.
    writer: Option<JoinHandle<()>>,
}

impl Drop for SessionStore {
    fn drop(&mut self) {
        let Some(persister) = &self.persister else {
            return;
        };
        lock_or_recover(&persister.signal).stop = true;
        persister.wake.notify_all();
        match self.writer.take() {
            Some(writer) => {
                let _ = writer.join();
            }
            // No writer thread: what is pending is written here.
            None => persister.write_now(true),
        }
    }
}

/// Why a cookie did not name a live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    Missing,
    Malformed,
    Unknown,
    Expired,
}

/// 32 random bytes from the OS as lowercase hex.
pub fn random_hex_32() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("os random source unavailable: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn is_hex_id(s: &str) -> bool {
    s.len() == ID_HEX_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl SessionStore {
    /// A table held in memory only (a restart ends every session).
    pub fn new(ttl: Duration, max: usize, cookie_secure: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Table {
                by_hash: HashMap::new(),
                order: BTreeMap::new(),
                next_tick: 0,
                evictions: 0,
            })),
            ttl,
            max: max.max(1),
            cookie_secure,
            persister: None,
            writer: None,
        }
    }

    /// A table kept in `file` across restarts (see the module doc): the
    /// sessions it holds that have not expired are reloaded, newest
    /// `max` at most, none later than `ttl` from now. A file that cannot
    /// be read or parsed is left as it is and the table starts empty; the
    /// returned warning says why (never an id or a hash), and the next
    /// creation rewrites the file. Entries it did not reload (expired or
    /// damaged) leave the file with the writer's first rewrite.
    pub fn persistent(
        ttl: Duration,
        max: usize,
        cookie_secure: bool,
        file: PathBuf,
    ) -> (Self, Option<String>) {
        Self::persistent_with(
            ttl,
            max,
            cookie_secure,
            file,
            WriterTiming {
                debounce: WRITE_DEBOUNCE,
                sweep: SWEEP_INTERVAL,
            },
        )
    }

    fn persistent_with(
        ttl: Duration,
        max: usize,
        cookie_secure: bool,
        file: PathBuf,
        timing: WriterTiming,
    ) -> (Self, Option<String>) {
        let mut store = Self::new(ttl, max, cookie_secure);
        let mut dropped_on_load = false;
        let mut warning = match load_sessions(&file) {
            Ok(stored) => {
                let now = SystemTime::now();
                let latest = now + ttl;
                let stored_len = stored.len();
                let mut live: Vec<(String, SystemTime, SystemTime)> = stored
                    .into_iter()
                    .filter_map(|s| {
                        let created_at = parse_rfc3339(&s.created_at)?;
                        let expires_at = parse_rfc3339(&s.expires_at)?;
                        let well_formed = s.sha256.len() == 64
                            && s.sha256
                                .bytes()
                                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
                        (well_formed && expires_at > now)
                            .then(|| (s.sha256, created_at, expires_at.min(latest)))
                    })
                    .collect();
                // The newest `max`, inserted oldest first (the LRU order).
                live.sort_by_key(|(_, created_at, _)| *created_at);
                let skip = live.len().saturating_sub(store.max);
                dropped_on_load = live.len() - skip < stored_len;
                let mut table = lock_or_recover(&store.inner);
                for (hash, created_at, expires_at) in live.into_iter().skip(skip) {
                    table.insert(hash, created_at, expires_at);
                }
                None
            }
            Err(e) => Some(format!(
                "the sessions file {} could not be read ({e}); every session starts anew",
                file.display()
            )),
        };
        let persister = Arc::new(Persister {
            file,
            table: store.inner.clone(),
            write_lock: Mutex::new(()),
            signal: Mutex::new(WriterSignal::default()),
            wake: Condvar::new(),
            writes: AtomicU64::new(0),
        });
        let writer = {
            let persister = persister.clone();
            std::thread::Builder::new()
                .name("session-writer".into())
                .spawn(move || persister.run(timing))
        };
        match writer {
            Ok(handle) => store.writer = Some(handle),
            Err(e) => {
                let note = format!(
                    "the sessions writer could not start ({e}); sessions are written when the server stops"
                );
                warning = Some(match warning {
                    Some(w) => format!("{w}; {note}"),
                    None => note,
                });
            }
        }
        if dropped_on_load {
            // Expired or damaged entries leave the file too.
            persister.mark_dirty();
        }
        store.persister = Some(persister);
        (store, warning)
    }

    /// Marks a persistent table changed, for the writer (never blocks on
    /// the disk).
    fn persist(&self) {
        if let Some(persister) = &self.persister {
            persister.mark_dirty();
        }
    }

    /// Writes the pending changes of a persistent table now, on the calling
    /// thread (it blocks on the disk: call it from a blocking context). The
    /// server calls it when it stops; dropping the store does as well.
    pub fn flush(&self) {
        if let Some(persister) = &self.persister {
            persister.write_now(true);
        }
    }

    /// How many times the sessions file was rewritten (0 for a table held
    /// in memory only).
    pub fn file_writes(&self) -> u64 {
        self.persister
            .as_ref()
            .map_or(0, |p| p.writes.load(Ordering::Relaxed))
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// The name this store sets and reads: [`COOKIE_NAME`] with `Secure`,
    /// [`PLAIN_COOKIE_NAME`] without.
    pub fn cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            COOKIE_NAME
        } else {
            PLAIN_COOKIE_NAME
        }
    }

    fn lock(&self) -> MutexGuard<'_, Table> {
        lock_or_recover(&self.inner)
    }

    /// Mints a session. Evicts the least recently used one when the table
    /// is full, and drops expired ones first.
    pub fn create(&self) -> Result<Session, String> {
        let id = random_hex_32()?;
        let now = SystemTime::now();
        let expires_at = now + self.ttl;
        {
            let mut table = self.lock();
            // Expired entries go first, then the LRU, so the bound holds.
            table.remove_expired(now);
            while table.by_hash.len() >= self.max {
                let Some((&tick, _)) = table.order.iter().next() else {
                    break;
                };
                if let Some(victim) = table.order.remove(&tick) {
                    table.by_hash.remove(&victim);
                    table.evictions += 1;
                }
            }
            table.insert(id_hash(&id), now, expires_at);
        }
        self.persist();
        Ok(Session { id, expires_at })
    }

    /// Looks a cookie value up (by its hash), touching the LRU position.
    pub fn lookup(&self, id: &str) -> Result<Session, SessionError> {
        if !is_hex_id(id) {
            return Err(SessionError::Malformed);
        }
        let hash = id_hash(id);
        let now = SystemTime::now();
        let mut table = self.lock();
        let tick = table.next_tick;
        let Some(entry) = table.by_hash.get_mut(&hash) else {
            return Err(SessionError::Unknown);
        };
        if entry.expires_at <= now {
            let old = entry.tick;
            table.by_hash.remove(&hash);
            table.order.remove(&old);
            drop(table);
            // The ended session's hash leaves the file with the next write.
            self.persist();
            return Err(SessionError::Expired);
        }
        let old = entry.tick;
        entry.tick = tick;
        let expires_at = entry.expires_at;
        table.next_tick += 1;
        table.order.remove(&old);
        table.order.insert(tick, hash);
        Ok(Session {
            id: id.to_string(),
            expires_at,
        })
    }

    /// Forgets a session.
    pub fn remove(&self, id: &str) -> bool {
        let removed = {
            let mut table = self.lock();
            match table.by_hash.remove(&id_hash(id)) {
                Some(entry) => {
                    table.order.remove(&entry.tick);
                    true
                }
                None => false,
            }
        };
        if removed {
            self.persist();
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.lock().by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn evictions(&self) -> u64 {
        self.lock().evictions
    }

    /// The session named by a `Cookie` header.
    pub fn from_cookie_header(&self, header: Option<&str>) -> Result<Session, SessionError> {
        let header = header.ok_or(SessionError::Missing)?;
        let value = cookie_value(header, self.cookie_name()).ok_or(SessionError::Missing)?;
        self.lookup(value)
    }

    /// `Set-Cookie` value establishing `session`.
    pub fn set_cookie(&self, session: &Session) -> String {
        let max_age = session
            .expires_at
            .duration_since(SystemTime::now())
            .unwrap_or_default()
            .as_secs();
        format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
            self.cookie_name(),
            session.id,
            if self.cookie_secure { "; Secure" } else { "" }
        )
    }

    /// `Set-Cookie` value removing the cookie.
    pub fn clear_cookie(&self) -> String {
        format!(
            "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
            self.cookie_name(),
            if self.cookie_secure { "; Secure" } else { "" }
        )
    }
}

/// The sessions a file holds; an absent file holds none.
fn load_sessions(file: &Path) -> Result<Vec<StoredSession>, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.kind().to_string()),
    };
    let stored: StoredSessions =
        serde_json::from_str(&text).map_err(|e| format!("not a sessions file: {e}"))?;
    if stored.version != SESSIONS_FILE_VERSION {
        return Err(format!("unknown version {}", stored.version));
    }
    Ok(stored.sessions)
}

/// Writes `value` to `file` whole: a temporary sibling, flushed to disk
/// (owner-only on Unix), renamed into place, and the directory flushed.
fn write_atomically(file: &Path, value: &StoredSessions) -> std::io::Result<()> {
    let dir = file
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| SESSIONS_FILE.into());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    // Compact: about 150 bytes a session.
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = (|| {
        let mut out = options.open(&tmp)?;
        out.write_all(&bytes)?;
        out.write_all(b"\n")?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&tmp, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    #[cfg(unix)]
    {
        if let Ok(handle) = std::fs::File::open(dir) {
            let _ = handle.sync_all();
        }
    }
    Ok(())
}

/// The value of cookie `name` in a `Cookie` header.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k.trim() == name).then(|| v.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_256_bit_hex_and_owner_hashes_differ_from_ids() {
        let store = SessionStore::new(Duration::from_secs(60), 10, true);
        let s = store.create().unwrap();
        assert_eq!(s.id().len(), ID_HEX_LEN);
        let Owner::Session { hash } = s.owner() else {
            panic!("session owner")
        };
        assert_ne!(hash.as_str(), s.id());
        assert!(s.pseudonym().starts_with("visitor:"));
        assert!(!s.pseudonym().contains(hash.as_str()));
        let cookie = store.set_cookie(&s);
        assert!(cookie.starts_with("__Host-hc_session="));
        for attr in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
            assert!(cookie.contains(attr), "{cookie}");
        }
        let plain = SessionStore::new(Duration::from_secs(60), 10, false);
        assert!(!plain.set_cookie(&s).contains("Secure"));
    }

    #[test]
    fn a_host_prefixed_cookie_is_never_emitted_without_secure() {
        let plain = SessionStore::new(Duration::from_secs(60), 10, false);
        let s = plain.create().unwrap();
        for cookie in [plain.set_cookie(&s), plain.clear_cookie()] {
            assert!(!cookie.contains("__Host-"), "{cookie}");
            assert!(!cookie.contains("Secure"), "{cookie}");
            assert!(cookie.starts_with("hc_session="), "{cookie}");
        }
        assert_eq!(plain.cookie_name(), PLAIN_COOKIE_NAME);
        // The plain store reads the plain name and ignores the prefixed one.
        let header = format!("{PLAIN_COOKIE_NAME}={}", s.id());
        assert!(plain.from_cookie_header(Some(&header)).is_ok());
        let prefixed = format!("{COOKIE_NAME}={}", s.id());
        assert_eq!(
            plain.from_cookie_header(Some(&prefixed)),
            Err(SessionError::Missing)
        );
        let secure = SessionStore::new(Duration::from_secs(60), 10, true);
        assert_eq!(secure.cookie_name(), COOKIE_NAME);
        for cookie in [secure.set_cookie(&s), secure.clear_cookie()] {
            assert!(cookie.starts_with("__Host-hc_session=") && cookie.ends_with("; Secure"));
        }
    }

    #[test]
    fn lookup_accepts_only_live_well_formed_ids() {
        let store = SessionStore::new(Duration::from_secs(60), 10, true);
        let s = store.create().unwrap();
        assert_eq!(store.lookup(s.id()).unwrap().id(), s.id());
        assert_eq!(store.lookup("nope"), Err(SessionError::Malformed));
        assert_eq!(store.lookup(&"0".repeat(64)), Err(SessionError::Unknown));
        let header = format!("other=1; {COOKIE_NAME}={}; x=y", s.id());
        assert!(store.from_cookie_header(Some(&header)).is_ok());
        assert_eq!(
            store.from_cookie_header(Some("other=1")),
            Err(SessionError::Missing)
        );
        assert!(store.remove(s.id()));
        assert_eq!(store.lookup(s.id()), Err(SessionError::Unknown));
    }

    #[test]
    fn the_table_is_bounded_with_lru_eviction() {
        let store = SessionStore::new(Duration::from_secs(60), 3, true);
        let a = store.create().unwrap();
        let _b = store.create().unwrap();
        let _c = store.create().unwrap();
        // Touch `a` so it is the most recently used.
        store.lookup(a.id()).unwrap();
        let _d = store.create().unwrap();
        assert_eq!(store.len(), 3);
        assert_eq!(store.evictions(), 1);
        assert!(store.lookup(a.id()).is_ok(), "recently used survives");
        for _ in 0..100 {
            store.create().unwrap();
        }
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn expired_sessions_are_refused() {
        let store = SessionStore::new(Duration::from_secs(0), 3, true);
        let s = store.create().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(store.lookup(s.id()), Err(SessionError::Expired));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-sessions-{name}-{}-{}",
            std::process::id(),
            random_hex_32().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_rebuilt_store_keeps_the_sessions_and_their_owners() {
        let dir = temp_dir("rebuild");
        let file = dir.join(SESSIONS_FILE);
        let ttl = Duration::from_secs(3600);
        let (store, warning) = SessionStore::persistent(ttl, 10, true, file.clone());
        assert_eq!(warning, None, "no file yet is no error");
        let a = store.create().unwrap();
        let b = store.create().unwrap();
        let gone = store.create().unwrap();
        assert!(store.remove(gone.id()));
        drop(store);

        let (rebuilt, warning) = SessionStore::persistent(ttl, 10, true, file.clone());
        assert_eq!(warning, None);
        assert_eq!(rebuilt.len(), 2);
        for session in [&a, &b] {
            let header = format!("{COOKIE_NAME}={}", session.id());
            let found = rebuilt.from_cookie_header(Some(&header)).unwrap();
            assert_eq!(found.owner(), session.owner(), "the same owner");
            assert_eq!(found.pseudonym(), session.pseudonym());
            assert_eq!(found.expires_at_rfc3339(), session.expires_at_rfc3339());
        }
        assert_eq!(rebuilt.lookup(gone.id()), Err(SessionError::Unknown));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sessions_file_holds_no_raw_id() {
        let dir = temp_dir("no-raw-id");
        let file = dir.join(SESSIONS_FILE);
        // The writer stays idle, so no temporary file of its own can show
        // up while the directory is listed below.
        let hour = Duration::from_secs(3600);
        let (store, _) = SessionStore::persistent_with(
            Duration::from_secs(60),
            10,
            true,
            file.clone(),
            timing(hour, hour),
        );
        let ids: Vec<String> = (0..3)
            .map(|_| store.create().unwrap().id().to_string())
            .collect();
        store.flush();
        let text = std::fs::read_to_string(&file).unwrap();
        for id in &ids {
            assert!(!text.contains(id.as_str()), "a raw id on disk");
            assert!(text.contains(&id_hash(id)), "its hash is the key");
            let Owner::Session { hash } = Owner::session(id.as_bytes()) else {
                panic!("session owner")
            };
            assert_eq!(hash.as_str(), id_hash(id), "the owner's hash");
        }
        // Every 64-hex string in the file is a hash, none an id.
        let mut found = 0;
        for word in text.split(|c: char| !c.is_ascii_hexdigit()) {
            if word.len() == ID_HEX_LEN {
                found += 1;
                assert!(!ids.iter().any(|id| id == word));
            }
        }
        assert_eq!(found, 3);
        let stored: StoredSessions = serde_json::from_str(&text).unwrap();
        assert_eq!(stored.version, 1);
        assert_eq!(stored.sessions.len(), 3);
        // No temporary file is left behind.
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [SESSIONS_FILE]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_or_damaged_entries_are_not_reloaded() {
        let dir = temp_dir("expired");
        let file = dir.join(SESSIONS_FILE);
        let now = SystemTime::now();
        let live_id = random_hex_32().unwrap();
        let expired_id = random_hex_32().unwrap();
        let entry = |id: &str, created: SystemTime, expires: SystemTime| StoredSession {
            sha256: id_hash(id),
            created_at: rfc3339(created),
            expires_at: rfc3339(expires),
        };
        let stored = StoredSessions {
            version: 1,
            sessions: vec![
                entry(
                    &expired_id,
                    now - Duration::from_secs(7200),
                    now - Duration::from_secs(60),
                ),
                entry(&live_id, now, now + Duration::from_secs(600)),
                StoredSession {
                    sha256: "not-a-hash".into(),
                    created_at: rfc3339(now),
                    expires_at: rfc3339(now + Duration::from_secs(600)),
                },
            ],
        };
        write_atomically(&file, &stored).unwrap();
        let (store, warning) = SessionStore::persistent(Duration::from_secs(3600), 10, true, file);
        assert_eq!(warning, None);
        assert_eq!(store.len(), 1);
        assert!(store.lookup(&live_id).is_ok());
        assert_eq!(store.lookup(&expired_id), Err(SessionError::Unknown));
        // What was not reloaded leaves the file with the next write.
        store.flush();
        let text = std::fs::read_to_string(dir.join(SESSIONS_FILE)).unwrap();
        assert!(text.contains(&id_hash(&live_id)));
        assert!(
            !text.contains(&id_hash(&expired_id)),
            "an expired hash stays on disk"
        );
        assert!(!text.contains("not-a-hash"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Timing for the writer tests: a debounce no test waits out, unless it
    /// says otherwise.
    fn timing(debounce: Duration, sweep: Duration) -> WriterTiming {
        WriterTiming { debounce, sweep }
    }

    /// Waits (up to 10 s) until `done` holds.
    fn eventually(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn creating_and_removing_sessions_never_waits_for_the_disk() {
        let dir = temp_dir("off-path");
        let file = dir.join(SESSIONS_FILE);
        let hour = Duration::from_secs(3600);
        let (store, _) =
            SessionStore::persistent_with(hour, 1000, true, file.clone(), timing(hour, hour));
        let sessions: Vec<Session> = (0..200).map(|_| store.create().unwrap()).collect();
        assert!(store.remove(sessions[0].id()));
        // Nothing was written on the callers' thread: the writer is still
        // gathering changes.
        assert_eq!(store.file_writes(), 0);
        assert!(!file.exists());
        // One write for all of them, and none for nothing.
        store.flush();
        assert_eq!(store.file_writes(), 1);
        store.flush();
        assert_eq!(store.file_writes(), 1);
        let stored: StoredSessions =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(stored.sessions.len(), 199);
        // Compact JSON: no indentation.
        assert!(!std::fs::read_to_string(&file).unwrap().contains("\n  "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_writer_rewrites_the_file_a_bounded_number_of_times() {
        let dir = temp_dir("coalesce");
        let file = dir.join(SESSIONS_FILE);
        let (store, _) = SessionStore::persistent_with(
            Duration::from_secs(3600),
            1000,
            true,
            file.clone(),
            timing(Duration::from_millis(200), Duration::from_secs(3600)),
        );
        let ids: Vec<String> = (0..300)
            .map(|_| store.create().unwrap().id().to_string())
            .collect();
        eventually("the writer's rewrite", || store.file_writes() >= 1);
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            store.file_writes() <= 3,
            "{} rewrites for 300 sessions",
            store.file_writes()
        );
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(ids.iter().all(|id| text.contains(&id_hash(id))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ended_session_leaves_the_file_when_it_is_looked_up() {
        let dir = temp_dir("lookup-expired");
        let file = dir.join(SESSIONS_FILE);
        let (store, _) = SessionStore::persistent_with(
            Duration::from_millis(300),
            10,
            true,
            file.clone(),
            timing(Duration::from_millis(10), Duration::from_secs(3600)),
        );
        let s = store.create().unwrap();
        store.flush();
        let hash = id_hash(s.id());
        assert!(std::fs::read_to_string(&file).unwrap().contains(&hash));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(store.lookup(s.id()), Err(SessionError::Expired));
        eventually("the expired hash leaves the file", || {
            !std::fs::read_to_string(&file).unwrap().contains(&hash)
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sweep_removes_ended_sessions_from_the_file_with_no_visitor() {
        let dir = temp_dir("sweep");
        let file = dir.join(SESSIONS_FILE);
        let (store, _) = SessionStore::persistent_with(
            Duration::from_millis(300),
            10,
            true,
            file.clone(),
            timing(Duration::from_millis(10), Duration::from_millis(100)),
        );
        let s = store.create().unwrap();
        store.flush();
        let hash = id_hash(s.id());
        assert!(std::fs::read_to_string(&file).unwrap().contains(&hash));
        // No lookup, no creation: the writer's sweep alone.
        eventually("the sweep removes the ended session", || {
            !std::fs::read_to_string(&file).unwrap().contains(&hash)
        });
        assert!(store.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropping_the_store_writes_what_is_pending() {
        let dir = temp_dir("drop");
        let file = dir.join(SESSIONS_FILE);
        let hour = Duration::from_secs(3600);
        let (store, _) =
            SessionStore::persistent_with(hour, 10, true, file.clone(), timing(hour, hour));
        let s = store.create().unwrap();
        assert!(!file.exists());
        drop(store);
        let (rebuilt, _) = SessionStore::persistent(hour, 10, true, file);
        assert!(rebuilt.lookup(s.id()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_sessions_file_starts_an_empty_table_with_a_warning() {
        let dir = temp_dir("corrupt");
        let file = dir.join(SESSIONS_FILE);
        std::fs::write(&file, b"{ not json").unwrap();
        let (store, warning) =
            SessionStore::persistent(Duration::from_secs(60), 10, true, file.clone());
        let warning = warning.expect("a warning");
        assert!(warning.contains("could not be read"), "{warning}");
        assert!(store.is_empty());
        // The next creation rewrites it whole.
        let s = store.create().unwrap();
        store.flush();
        let (rebuilt, warning) = SessionStore::persistent(Duration::from_secs(60), 10, true, file);
        assert_eq!(warning, None);
        assert!(rebuilt.lookup(s.id()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuilt_store_keeps_its_bound_and_never_extends_a_session() {
        let dir = temp_dir("bound");
        let file = dir.join(SESSIONS_FILE);
        let (store, _) =
            SessionStore::persistent(Duration::from_secs(3600), 10, true, file.clone());
        let ids: Vec<Session> = (0..5).map(|_| store.create().unwrap()).collect();
        drop(store);
        // A smaller table keeps the newest; a shorter TTL caps the expiry.
        let (small, _) = SessionStore::persistent(Duration::from_secs(60), 2, true, file);
        assert_eq!(small.len(), 2);
        assert!(small.lookup(ids[3].id()).is_ok());
        let kept = small.lookup(ids[4].id()).unwrap();
        assert!(kept.expires_at <= SystemTime::now() + Duration::from_secs(60));
        assert_eq!(small.lookup(ids[0].id()), Err(SessionError::Unknown));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
