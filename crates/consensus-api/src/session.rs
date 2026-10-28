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

use consensus_runtime::Owner;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// The cookie's name. The `__Host-` prefix pins it to this host, with no
/// `Domain` and `Path=/`, so a sibling subdomain cannot plant or fixate
/// one.
pub const COOKIE_NAME: &str = "__Host-hc_session";
/// Hex length of a session id: 32 random bytes.
pub const ID_HEX_LEN: usize = 64;

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
    expires_at: SystemTime,
    /// Position in the LRU order.
    tick: u64,
}

struct Table {
    by_id: HashMap<String, Entry>,
    /// LRU order: oldest tick first.
    order: BTreeMap<u64, String>,
    next_tick: u64,
    evictions: u64,
}

/// The bounded session table.
pub struct SessionStore {
    inner: Mutex<Table>,
    ttl: Duration,
    max: usize,
    cookie_secure: bool,
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
    pub fn new(ttl: Duration, max: usize, cookie_secure: bool) -> Self {
        Self {
            inner: Mutex::new(Table {
                by_id: HashMap::new(),
                order: BTreeMap::new(),
                next_tick: 0,
                evictions: 0,
            }),
            ttl,
            max: max.max(1),
            cookie_secure,
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Table> {
        // A poisoned table holds plain data; keep serving.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Mints a session. Evicts the least recently used one when the table
    /// is full, and drops expired ones first.
    pub fn create(&self) -> Result<Session, String> {
        let id = random_hex_32()?;
        let now = SystemTime::now();
        let expires_at = now + self.ttl;
        let mut table = self.lock();
        // Expired entries go first, then the LRU, so the bound holds.
        let expired: Vec<String> = table
            .by_id
            .iter()
            .filter(|(_, e)| e.expires_at <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for stale in expired {
            if let Some(entry) = table.by_id.remove(&stale) {
                table.order.remove(&entry.tick);
            }
        }
        while table.by_id.len() >= self.max {
            let Some((&tick, _)) = table.order.iter().next() else {
                break;
            };
            if let Some(victim) = table.order.remove(&tick) {
                table.by_id.remove(&victim);
                table.evictions += 1;
            }
        }
        let tick = table.next_tick;
        table.next_tick += 1;
        table.order.insert(tick, id.clone());
        table.by_id.insert(id.clone(), Entry { expires_at, tick });
        Ok(Session { id, expires_at })
    }

    /// Looks a cookie value up, touching the LRU position.
    pub fn lookup(&self, id: &str) -> Result<Session, SessionError> {
        if !is_hex_id(id) {
            return Err(SessionError::Malformed);
        }
        let now = SystemTime::now();
        let mut table = self.lock();
        let tick = table.next_tick;
        let Some(entry) = table.by_id.get_mut(id) else {
            return Err(SessionError::Unknown);
        };
        if entry.expires_at <= now {
            let old = entry.tick;
            table.by_id.remove(id);
            table.order.remove(&old);
            return Err(SessionError::Expired);
        }
        let old = entry.tick;
        entry.tick = tick;
        let expires_at = entry.expires_at;
        table.next_tick += 1;
        table.order.remove(&old);
        table.order.insert(tick, id.to_string());
        Ok(Session {
            id: id.to_string(),
            expires_at,
        })
    }

    /// Forgets a session.
    pub fn remove(&self, id: &str) -> bool {
        let mut table = self.lock();
        match table.by_id.remove(id) {
            Some(entry) => {
                table.order.remove(&entry.tick);
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.lock().by_id.len()
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
        let value = cookie_value(header, COOKIE_NAME).ok_or(SessionError::Missing)?;
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
            "{COOKIE_NAME}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
            session.id,
            if self.cookie_secure { "; Secure" } else { "" }
        )
    }

    /// `Set-Cookie` value removing the cookie.
    pub fn clear_cookie(&self) -> String {
        format!(
            "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
            if self.cookie_secure { "; Secure" } else { "" }
        )
    }
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
}
