// SPDX-License-Identifier: Apache-2.0
//! Per-client limits (C3, ABUSE-4): the client key is an IPv4 /32 or an
//! IPv6 /64, every table is a bounded LRU, and a refusal says when to
//! retry. Keys come from `CF-Connecting-IP` only when the listener is a
//! unix socket behind cloudflared or the configuration trusts the proxy;
//! otherwise from the TCP peer address.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A client as the limits see it: one IPv4 address or one IPv6 /64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKey {
    V4([u8; 4]),
    /// The first 64 bits of the address.
    V6([u8; 8]),
    /// No peer address and no trusted header (in-process tests).
    Unknown,
}

impl ClientKey {
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => ClientKey::V4(v4.octets()),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => ClientKey::V4(v4.octets()),
                None => {
                    let o = v6.octets();
                    ClientKey::V6([o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7]])
                }
            },
        }
    }

    /// Short label for logs (never the full IPv6 address).
    pub fn label(&self) -> String {
        match self {
            ClientKey::V4(o) => format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3]),
            ClientKey::V6(o) => format!(
                "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}::/64",
                o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7]
            ),
            ClientKey::Unknown => "unknown".into(),
        }
    }
}

/// How the client key is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKeyPolicy {
    /// `CF-Connecting-IP` names the client; a request without it is
    /// refused.
    TrustedProxyHeader,
    /// The TCP peer address names the client.
    PeerAddress,
}

/// Why no client key could be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKeyError {
    /// The proxy header is required and missing or unparseable.
    MissingProxyHeader,
}

/// The client key of a request.
pub fn client_key(
    policy: ClientKeyPolicy,
    cf_connecting_ip: Option<&str>,
    peer: Option<SocketAddr>,
) -> Result<ClientKey, ClientKeyError> {
    match policy {
        ClientKeyPolicy::TrustedProxyHeader => cf_connecting_ip
            .and_then(|v| v.trim().parse::<IpAddr>().ok())
            .map(ClientKey::from_ip)
            .ok_or(ClientKeyError::MissingProxyHeader),
        ClientKeyPolicy::PeerAddress => Ok(peer
            .map(|p| ClientKey::from_ip(p.ip()))
            .unwrap_or(ClientKey::Unknown)),
    }
}

/// A bounded map with least-recently-used eviction.
pub struct LruMap<K, V> {
    map: HashMap<K, (V, u64)>,
    order: BTreeMap<u64, K>,
    next: u64,
    cap: usize,
    evictions: u64,
}

impl<K: Hash + Eq + Clone, V> LruMap<K, V> {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: BTreeMap::new(),
            next: 0,
            cap: cap.max(1),
            evictions: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    fn touch(&mut self, key: &K) {
        let tick = self.next;
        self.next += 1;
        if let Some((_, old)) = self.map.get_mut(key) {
            let previous = *old;
            *old = tick;
            self.order.remove(&previous);
            self.order.insert(tick, key.clone());
        }
    }

    /// The value for `key`, marking it recently used.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if !self.map.contains_key(key) {
            return None;
        }
        self.touch(key);
        self.map.get_mut(key).map(|(v, _)| v)
    }

    /// Inserts or replaces, evicting the least recently used entry when
    /// full.
    pub fn insert(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.touch(&key);
            if let Some((v, _)) = self.map.get_mut(&key) {
                *v = value;
            }
            return;
        }
        while self.map.len() >= self.cap {
            let Some((&tick, _)) = self.order.iter().next() else {
                break;
            };
            if let Some(victim) = self.order.remove(&tick) {
                self.map.remove(&victim);
                self.evictions += 1;
            }
        }
        let tick = self.next;
        self.next += 1;
        self.order.insert(tick, key.clone());
        self.map.insert(key, (value, tick));
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (value, tick) = self.map.remove(key)?;
        self.order.remove(&tick);
        Some(value)
    }

    /// Entry API: get or insert with `make`, marking it recently used.
    pub fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> &mut V {
        if !self.map.contains_key(&key) {
            self.insert(key.clone(), make());
        } else {
            self.touch(&key);
        }
        self.map
            .get_mut(&key)
            .map(|(v, _)| v)
            .expect("just inserted")
    }
}

struct Window {
    started: Instant,
    count: u32,
}

/// A fixed-window rate limit per client key, bounded in size.
pub struct RateTable {
    inner: Mutex<LruMap<ClientKey, Window>>,
    window: Duration,
    max: u32,
}

impl RateTable {
    pub fn new(max_per_window: u32, window: Duration, table_max: usize) -> Self {
        Self {
            inner: Mutex::new(LruMap::new(table_max)),
            window,
            max: max_per_window.max(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LruMap<ClientKey, Window>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Counts one request for `key`. `Err(retry_after)` when the window is
    /// full.
    pub fn check(&self, key: ClientKey) -> Result<(), Duration> {
        self.check_at(key, Instant::now())
    }

    pub fn check_at(&self, key: ClientKey, now: Instant) -> Result<(), Duration> {
        let mut table = self.lock();
        let window = self.window;
        let entry = table.get_or_insert_with(key, || Window {
            started: now,
            count: 0,
        });
        if now.duration_since(entry.started) >= window {
            entry.started = now;
            entry.count = 0;
        }
        if entry.count >= self.max {
            let elapsed = now.duration_since(entry.started);
            return Err(window.saturating_sub(elapsed).max(Duration::from_secs(1)));
        }
        entry.count += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn evictions(&self) -> u64 {
        self.lock().evictions()
    }
}

/// Concurrent stream counters per run, per client and in total. A
/// [`StreamPermit`] releases its counts on drop.
pub struct StreamCaps {
    inner: Mutex<StreamCounts>,
    max_per_run: usize,
    max_per_client: usize,
    max_global: usize,
}

struct StreamCounts {
    per_run: HashMap<String, usize>,
    per_client: LruMap<ClientKey, usize>,
    global: usize,
}

/// Which cap refused a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCap {
    PerRun,
    PerClient,
    Global,
}

/// A counted, open stream; dropping it frees its slots.
pub struct StreamPermit {
    caps: Arc<StreamCaps>,
    run_id: String,
    client: ClientKey,
}

impl std::fmt::Debug for StreamPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamPermit")
            .field("run_id", &self.run_id)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        let mut counts = self.caps.lock();
        let run_left = counts.per_run.get_mut(&self.run_id).map(|n| {
            *n = n.saturating_sub(1);
            *n
        });
        if run_left == Some(0) {
            counts.per_run.remove(&self.run_id);
        }
        let client_left = counts.per_client.get_mut(&self.client).map(|n| {
            *n = n.saturating_sub(1);
            *n
        });
        if client_left == Some(0) {
            counts.per_client.remove(&self.client);
        }
        counts.global = counts.global.saturating_sub(1);
    }
}

impl StreamCaps {
    pub fn new(
        max_per_run: usize,
        max_per_client: usize,
        max_global: usize,
        table_max: usize,
    ) -> Self {
        Self {
            inner: Mutex::new(StreamCounts {
                per_run: HashMap::new(),
                per_client: LruMap::new(table_max),
                global: 0,
            }),
            max_per_run: max_per_run.max(1),
            max_per_client: max_per_client.max(1),
            max_global: max_global.max(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StreamCounts> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes a slot for a stream of `run_id` by `client`.
    pub fn acquire(
        self: &Arc<Self>,
        run_id: &str,
        client: ClientKey,
    ) -> Result<StreamPermit, StreamCap> {
        let mut counts = self.lock();
        if counts.global >= self.max_global {
            return Err(StreamCap::Global);
        }
        let run_count = counts.per_run.get(run_id).copied().unwrap_or(0);
        if run_count >= self.max_per_run {
            return Err(StreamCap::PerRun);
        }
        let client_count = counts.per_client.get_mut(&client).copied().unwrap_or(0);
        if client_count >= self.max_per_client {
            return Err(StreamCap::PerClient);
        }
        counts.per_run.insert(run_id.to_string(), run_count + 1);
        counts.per_client.insert(client, client_count + 1);
        counts.global += 1;
        Ok(StreamPermit {
            caps: self.clone(),
            run_id: run_id.to_string(),
            client,
        })
    }

    pub fn open_streams(&self) -> usize {
        self.lock().global
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_addresses_in_one_slash_64_share_a_key() {
        let a: IpAddr = "2001:db8:1:2:3:4:5:6".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff:ffff:ffff:ffff".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(ClientKey::from_ip(a), ClientKey::from_ip(b));
        assert_ne!(ClientKey::from_ip(a), ClientKey::from_ip(c));
        let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
        assert_eq!(ClientKey::from_ip(mapped), ClientKey::V4([192, 0, 2, 1]));
        assert_eq!(ClientKey::from_ip(a).label(), "2001:0db8:0001:0002::/64");
    }

    #[test]
    fn trusted_proxy_policy_requires_the_header() {
        let peer: SocketAddr = "10.0.0.9:4000".parse().unwrap();
        assert_eq!(
            client_key(
                ClientKeyPolicy::TrustedProxyHeader,
                Some("198.51.100.7"),
                Some(peer)
            ),
            Ok(ClientKey::V4([198, 51, 100, 7]))
        );
        assert_eq!(
            client_key(ClientKeyPolicy::TrustedProxyHeader, None, Some(peer)),
            Err(ClientKeyError::MissingProxyHeader)
        );
        assert_eq!(
            client_key(
                ClientKeyPolicy::PeerAddress,
                Some("198.51.100.7"),
                Some(peer)
            ),
            Ok(ClientKey::V4([10, 0, 0, 9])),
            "a header is ignored when the proxy is not trusted"
        );
        assert_eq!(
            client_key(ClientKeyPolicy::PeerAddress, None, None),
            Ok(ClientKey::Unknown)
        );
    }

    #[test]
    fn rate_table_refuses_past_the_window_and_stays_bounded() {
        let table = RateTable::new(2, Duration::from_secs(60), 4);
        let key = ClientKey::V4([1, 2, 3, 4]);
        let t0 = Instant::now();
        assert!(table.check_at(key, t0).is_ok());
        assert!(table.check_at(key, t0).is_ok());
        let retry = table.check_at(key, t0).unwrap_err();
        assert!(retry >= Duration::from_secs(1) && retry <= Duration::from_secs(60));
        assert!(table.check_at(key, t0 + Duration::from_secs(61)).is_ok());
        for i in 0..100u8 {
            table.check_at(ClientKey::V4([9, 9, 9, i]), t0).unwrap();
        }
        assert_eq!(table.len(), 4);
        assert!(table.evictions() >= 96);
    }

    #[test]
    fn stream_caps_count_and_release() {
        let caps = Arc::new(StreamCaps::new(2, 3, 4, 16));
        let a = ClientKey::V4([1, 1, 1, 1]);
        let b = ClientKey::V4([2, 2, 2, 2]);
        let p1 = caps.acquire("run-a", a).unwrap();
        let p2 = caps.acquire("run-a", a).unwrap();
        assert_eq!(caps.acquire("run-a", a).unwrap_err(), StreamCap::PerRun);
        let p3 = caps.acquire("run-b", a).unwrap();
        assert_eq!(caps.acquire("run-c", a).unwrap_err(), StreamCap::PerClient);
        let p4 = caps.acquire("run-c", b).unwrap();
        assert_eq!(
            caps.acquire("run-d", ClientKey::V4([3, 3, 3, 3]))
                .unwrap_err(),
            StreamCap::Global
        );
        drop(p3);
        assert!(caps.acquire("run-c", a).is_ok(), "a slot came back");
        drop(p1);
        drop(p2);
        drop(p4);
        assert_eq!(caps.open_streams(), 0);
    }
}
