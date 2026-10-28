// SPDX-License-Identifier: Apache-2.0
//! HTTP-level test harness: builds the public or operator server in a
//! temporary store and drives its router through `tower::ServiceExt::
//! oneshot`, exactly as a connection would, including the Host, Origin
//! and Content-Type headers a browser sends.

#![allow(dead_code)]

pub mod doubles;
pub mod live;
pub mod snapshot;

use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, Request, Response, StatusCode};
use axum::Router;
use consensus_api::api::AppState;
use consensus_api::public_config::{OperatorConfig, PublicConfig};
use consensus_api::server::{build_operator, build_public, BuildOptions};
use consensus_api::static_files::csp_hash;
use http_body_util::BodyExt;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tower::ServiceExt;

/// The inline script of the test export's index page.
pub const INDEX_SCRIPT: &str = "document.documentElement.dataset.theme='dark'";

/// A fresh directory, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-api-{}-{tag}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `research/` at the repository root.
pub fn research_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("research")
}

/// A tiny static export with a CSP manifest, under `dir/web`.
pub fn write_web_root(dir: &Path) -> PathBuf {
    let web = dir.join("web");
    std::fs::create_dir_all(web.join("en")).unwrap();
    std::fs::write(
        web.join("index.html"),
        format!("<!doctype html><html><head><script>{INDEX_SCRIPT}</script></head><body>home</body></html>"),
    )
    .unwrap();
    std::fs::write(
        web.join("en/index.html"),
        "<!doctype html><html><body>en</body></html>",
    )
    .unwrap();
    std::fs::write(
        web.join("404.html"),
        "<!doctype html><html><body>missing</body></html>",
    )
    .unwrap();
    std::fs::write(
        web.join("csp-manifest.json"),
        serde_json::json!({
            "version": 1,
            "pages": {
                "/": {"scriptHashes": [csp_hash(INDEX_SCRIPT)], "styleHashes": []},
                "/en/": {"scriptHashes": [], "styleHashes": []},
                "/404.html": {"scriptHashes": [], "styleHashes": []}
            }
        })
        .to_string(),
    )
    .unwrap();
    web
}

pub const PUBLIC_HOST: &str = "localhost:8788";
pub const PUBLIC_ORIGIN: &str = "http://localhost:8788";
pub const OPERATOR_HOST: &str = "127.0.0.1:8787";
pub const OPERATOR_ORIGIN: &str = "http://127.0.0.1:8787";
pub const OPERATOR_TOKEN: &str = "test-operator-token-0123456789abcdef0123456789";
/// Cloudflare's country header, sent by default (see [`TestServer::send`]).
pub const COUNTRY_HEADER: &str = "cf-ipcountry";
/// The country the harness reports unless a request says otherwise: one
/// on neither block list.
pub const DEFAULT_COUNTRY: &str = "US";

/// A built server under test.
pub struct TestServer {
    pub router: Router,
    pub state: AppState,
    pub dir: TempDir,
    pub host: &'static str,
    pub origin: &'static str,
    pub token: Option<&'static str>,
    /// Other temporary directories that must outlive the server (a live
    /// harness's ledger).
    pub extra_dirs: Vec<TempDir>,
}

fn env_lookup(map: HashMap<String, String>) -> impl Fn(&str) -> Option<String> {
    move |name| map.get(name).cloned()
}

/// Builds the public server over a temporary store. `overrides` are
/// environment variables on top of the test defaults.
pub fn public_server(overrides: &[(&str, &str)], opts: BuildOptions) -> TestServer {
    let dir = TempDir::new("public");
    let web = write_web_root(dir.path());
    let mut env: HashMap<String, String> = [
        ("CONSENSUS_PUBLIC_BIND", "127.0.0.1:8788".to_string()),
        (
            "CONSENSUS_RUN_STORE_DIR",
            dir.path().join("store").display().to_string(),
        ),
        (
            "CONSENSUS_RESEARCH_DIR",
            research_dir().display().to_string(),
        ),
        ("CONSENSUS_WEB_ROOT", web.display().to_string()),
        ("CONSENSUS_SSE_HEARTBEAT_SECS", "1".to_string()),
        ("CONSENSUS_COOKIE_SECURE", "false".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    for (k, v) in overrides {
        env.insert(k.to_string(), v.to_string());
    }
    let cfg = PublicConfig::from_env(&env_lookup(env)).expect("public config");
    let built = build_public(&cfg, opts).expect("build public server");
    TestServer {
        router: built.router,
        state: built.state,
        dir,
        host: PUBLIC_HOST,
        origin: PUBLIC_ORIGIN,
        token: None,
        extra_dirs: Vec::new(),
    }
}

/// Builds the operator server over a temporary store.
pub fn operator_server(overrides: &[(&str, &str)], opts: BuildOptions) -> TestServer {
    let dir = TempDir::new("operator");
    let mut env: HashMap<String, String> = [
        (
            "CONSENSUS_RUN_STORE_DIR",
            dir.path().join("store").display().to_string(),
        ),
        (
            "CONSENSUS_RESEARCH_DIR",
            research_dir().display().to_string(),
        ),
        ("CONSENSUS_SSE_HEARTBEAT_SECS", "1".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    for (k, v) in overrides {
        env.insert(k.to_string(), v.to_string());
    }
    let cfg = OperatorConfig::from_env(&env_lookup(env)).expect("operator config");
    let built = build_operator(&cfg, OPERATOR_TOKEN, opts).expect("build operator server");
    TestServer {
        router: built.router,
        state: built.state,
        dir,
        host: OPERATOR_HOST,
        origin: OPERATOR_ORIGIN,
        token: Some(OPERATOR_TOKEN),
        extra_dirs: Vec::new(),
    }
}

/// A request under construction with the headers a same-origin browser
/// (or the operator console) would send.
pub struct Req {
    pub method: Method,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub peer: Option<SocketAddr>,
    /// The session cookie's value; the header is written with the name
    /// the server under test uses (`__Host-hc_session` with `Secure`,
    /// `hc_session` without).
    pub session: Option<String>,
}

impl Req {
    pub fn new(method: Method, path: &str) -> Self {
        Self {
            method,
            path: path.to_string(),
            headers: Vec::new(),
            body: None,
            peer: None,
            session: None,
        }
    }

    pub fn get(path: &str) -> Self {
        Self::new(Method::GET, path)
    }

    pub fn post(path: &str) -> Self {
        Self::new(Method::POST, path)
    }

    pub fn delete(path: &str) -> Self {
        Self::new(Method::DELETE, path)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Removes a header the harness would otherwise add by default.
    pub fn without(mut self, name: &str) -> Self {
        self.headers.push((format!("-{name}"), String::new()));
        self
    }

    pub fn json(mut self, body: &Value) -> Self {
        self.body = Some(body.to_string().into_bytes());
        self.header("content-type", "application/json")
    }

    pub fn raw_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    pub fn cookie(mut self, session: &str) -> Self {
        self.session = Some(session.to_string());
        self
    }

    pub fn peer(mut self, addr: &str) -> Self {
        self.peer = Some(addr.parse().unwrap());
        self
    }
}

impl TestServer {
    /// Sends a request. Adds `Host`, the `CF-IPCountry` cloudflared always
    /// sets ([`DEFAULT_COUNTRY`]; a listener that trusts its proxy headers
    /// refuses a request without one), and for state-changing methods the
    /// allowed `Origin` and `Sec-Fetch-Site: same-origin`, plus the bearer
    /// token on the operator surface, unless the request removed them with
    /// [`Req::without`] or set its own.
    pub async fn send(&self, req: Req) -> Response<Body> {
        let removed: Vec<String> = req
            .headers
            .iter()
            .filter_map(|(k, _)| k.strip_prefix('-').map(str::to_string))
            .collect();
        let mut builder = Request::builder()
            .method(req.method.clone())
            .uri(req.path.clone());
        let mut defaults: Vec<(&str, String)> = vec![
            ("host", self.host.to_string()),
            (COUNTRY_HEADER, DEFAULT_COUNTRY.to_string()),
        ];
        if !matches!(req.method, Method::GET | Method::HEAD | Method::OPTIONS) {
            defaults.push(("origin", self.origin.to_string()));
            defaults.push(("sec-fetch-site", "same-origin".to_string()));
        }
        if let Some(token) = self.token {
            defaults.push(("authorization", format!("Bearer {token}")));
        }
        let explicit: Vec<&str> = req
            .headers
            .iter()
            .filter(|(k, _)| !k.starts_with('-'))
            .map(|(k, _)| k.as_str())
            .collect();
        for (name, value) in defaults {
            if !removed.iter().any(|r| r == name)
                && !explicit.iter().any(|e| e.eq_ignore_ascii_case(name))
            {
                builder = builder.header(name, value);
            }
        }
        for (name, value) in &req.headers {
            if !name.starts_with('-') {
                builder = builder.header(name.as_str(), value.as_str());
            }
        }
        if let Some(session) = &req.session {
            builder = builder.header(
                "cookie",
                format!("{}={session}", self.state.sessions.cookie_name()),
            );
        }
        let mut request = builder
            .body(match req.body {
                Some(bytes) => Body::from(bytes),
                None => Body::empty(),
            })
            .unwrap();
        if let Some(peer) = req.peer {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// Sends and reads a JSON body.
    pub async fn json(&self, req: Req) -> (StatusCode, Value, HeaderMap) {
        let response = self.send(req).await;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&bytes)))
        };
        (status, value, headers)
    }

    /// Mints a session and returns the cookie value.
    pub async fn new_session(&self) -> String {
        let response = self.send(Req::post("/api/v1/session")).await;
        assert_eq!(response.status(), StatusCode::CREATED, "session");
        cookie_value(response.headers()).expect("Set-Cookie")
    }

    /// Mints a session for a client at `peer`.
    pub async fn new_session_from(&self, peer: &str) -> (StatusCode, Option<String>) {
        let response = self.send(Req::post("/api/v1/session").peer(peer)).await;
        let status = response.status();
        (status, cookie_value(response.headers()))
    }

    /// Mints a session for the client `CF-Connecting-IP` names (servers
    /// that trust the proxy header).
    pub async fn new_session_from_header(&self, ip: &str) -> (StatusCode, Option<String>) {
        let response = self
            .send(Req::post("/api/v1/session").header("cf-connecting-ip", ip))
            .await;
        let status = response.status();
        (status, cookie_value(response.headers()))
    }

    /// `POST /api/v1/runs` for a guided scenario.
    pub async fn create_guided(&self, cookie: &str, scenario: &str) -> (StatusCode, Value) {
        let (status, body, _) = self
            .json(
                Req::post("/api/v1/runs")
                    .cookie(cookie)
                    .json(&serde_json::json!({"scenario": scenario})),
            )
            .await;
        (status, body)
    }

    /// `GET /api/v1/runs/{id}` as the given session.
    pub async fn detail(&self, cookie: &str, run_id: &str) -> (StatusCode, Value) {
        let (status, body, _) = self
            .json(Req::get(&format!("/api/v1/runs/{run_id}")).cookie(cookie))
            .await;
        (status, body)
    }

    /// Polls the detail until the run awaits a decision or is terminal.
    pub async fn wait_resting(&self, cookie: &str, run_id: &str) -> Value {
        for _ in 0..600 {
            let (status, body) = self.detail(cookie, run_id).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            let state = body["summary"]["state"].as_str().unwrap_or("");
            if matches!(
                state,
                "awaiting_human_approval" | "approved" | "rejected" | "failed"
            ) {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("run {run_id} never came to rest");
    }

    /// Waits until the manager has let the run go (its writer dropped, the
    /// run's lock free): what a purge or a decision needs.
    pub async fn wait_released(&self, run_id: &str) {
        let id = consensus_domain::RunId::new(run_id);
        for _ in 0..600 {
            if !self.state.manager.is_active(&id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("run {run_id} is still active");
    }
}

impl TestServer {
    /// The run store's root directory.
    pub fn store_dir(&self) -> PathBuf {
        self.dir.path().join("store")
    }

    /// Edits one hex digit of the run index's first record hash, keeping
    /// the file's length, as a hand edit made while the server runs would:
    /// the next health check fails verification (`index_tampered`).
    pub fn tamper_index(&self) {
        let index = self.store_dir().join("runs").join("index.jsonl");
        let mut bytes = std::fs::read(&index).unwrap();
        let marker = b"\"hash\":\"";
        let at = bytes
            .windows(marker.len())
            .position(|w| w == marker)
            .expect("an index record")
            + marker.len();
        bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
        std::fs::write(&index, bytes).unwrap();
    }

    /// Every spelling of the store directory a JSON body could carry.
    pub fn store_dir_spellings(&self) -> Vec<String> {
        let local = self.store_dir().to_string_lossy().into_owned();
        vec![local.replace('\\', "\\\\"), local.replace('\\', "/"), local]
    }
}

/// The session cookie value of a response, if it set one, under either
/// name the server may use.
pub fn cookie_value(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::SET_COOKIE).iter().find_map(|v| {
        let text = v.to_str().ok()?;
        let (name, rest) = text.split_once('=')?;
        if name != "__Host-hc_session" && name != "hc_session" {
            return None;
        }
        let value = rest.split(';').next()?.to_string();
        (!value.is_empty()).then_some(value)
    })
}

pub async fn body_bytes(response: Response<Body>) -> Bytes {
    response.into_body().collect().await.unwrap().to_bytes()
}

pub async fn body_text(response: Response<Body>) -> String {
    String::from_utf8(body_bytes(response).await.to_vec()).unwrap()
}

/// One parsed SSE frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    /// A comment line (heartbeat, stream-start).
    pub comment: Option<String>,
    pub retry: Option<u64>,
}

impl SseFrame {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.data).unwrap_or(Value::Null)
    }
}

fn parse_frame(block: &str) -> SseFrame {
    let mut frame = SseFrame {
        event: "message".into(),
        data: String::new(),
        id: None,
        comment: None,
        retry: None,
    };
    for line in block.lines() {
        if let Some(c) = line.strip_prefix(':') {
            frame.comment = Some(c.trim().to_string());
        } else if let Some(v) = line.strip_prefix("event:") {
            frame.event = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            if !frame.data.is_empty() {
                frame.data.push('\n');
            }
            frame.data.push_str(v.trim_start());
        } else if let Some(v) = line.strip_prefix("id:") {
            frame.id = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("retry:") {
            frame.retry = v.trim().parse().ok();
        }
    }
    frame
}

/// Incremental SSE reader over a response body: frames can be read one
/// at a time, the reader can be left unpolled (to make the subscriber
/// lag), and dropped (to disconnect).
pub struct SseReader {
    body: Body,
    buffer: String,
    done: bool,
}

impl SseReader {
    pub fn new(body: Body) -> Self {
        Self {
            body,
            buffer: String::new(),
            done: false,
        }
    }

    fn take_frame(&mut self) -> Option<SseFrame> {
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();
            if block.trim().is_empty() {
                continue;
            }
            return Some(parse_frame(&block));
        }
        None
    }

    /// The next frame (comments included), or None once the body ends.
    /// Panics after ten seconds without one.
    pub async fn next(&mut self) -> Option<SseFrame> {
        loop {
            if let Some(frame) = self.take_frame() {
                return Some(frame);
            }
            if self.done {
                return None;
            }
            let next = tokio::time::timeout(Duration::from_secs(10), self.body.frame()).await;
            match next {
                Ok(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        self.buffer.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                Ok(Some(Err(e))) => panic!("body error: {e}"),
                Ok(None) => self.done = true,
                Err(_) => panic!("SSE read timed out"),
            }
        }
    }

    /// Skips to the next frame of `event`.
    pub async fn next_event(&mut self, event: &str) -> Option<SseFrame> {
        loop {
            let frame = self.next().await?;
            if frame.event == event && frame.comment.is_none() {
                return Some(frame);
            }
        }
    }

    /// Every remaining frame up to and including `stream_end` (or `max`).
    pub async fn rest(&mut self, max: usize) -> Vec<SseFrame> {
        let mut frames = Vec::new();
        while frames.len() < max {
            let Some(frame) = self.next().await else {
                break;
            };
            let end = frame.event == "stream_end";
            frames.push(frame);
            if end {
                break;
            }
        }
        frames
    }
}

/// Reads an SSE body frame by frame until `stream_end` (or `max`
/// frames), with a timeout per read so a stuck stream fails the test.
pub async fn read_sse(body: Body, max: usize) -> Vec<SseFrame> {
    SseReader::new(body).rest(max).await
}

/// Sequences of the `consensus` frames, in order.
pub fn sequences(frames: &[SseFrame]) -> Vec<u64> {
    frames
        .iter()
        .filter(|f| f.event == "consensus")
        .map(|f| f.json()["event"]["sequence"].as_u64().unwrap())
        .collect()
}
