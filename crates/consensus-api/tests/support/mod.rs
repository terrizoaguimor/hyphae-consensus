// SPDX-License-Identifier: Apache-2.0
//! Shared test support: a loopback fake of the Chat Completions endpoint
//! with scripted replies (status, headers, body, delays, stalls, hang-ups)
//! that records every request, plus envelope builders and a test config.
//! No network beyond loopback, no credentials.
#![allow(dead_code)]

use consensus_api::config::{ApiKey, MetaConfig};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A key of the documented shape; not a secret.
pub const TEST_KEY: &str = "LLM_1000_test-key-not-a-secret";

/// One scripted answer.
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// Wait this long before answering at all.
    pub delay: Duration,
    /// Send the headers and the first half of the body, then stall.
    pub stall: Option<Duration>,
    /// Read the request, then close the connection without an answer.
    pub hang_up: bool,
}

impl Reply {
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: vec![],
            body: body.into(),
            delay: Duration::ZERO,
            stall: None,
            hang_up: false,
        }
    }

    pub fn ok(body: impl Into<String>) -> Self {
        Self::json(200, body)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn stalled(mut self, stall: Duration) -> Self {
        self.stall = Some(stall);
        self
    }

    pub fn hang_up() -> Self {
        let mut reply = Self::json(200, "");
        reply.hang_up = true;
        reply
    }
}

pub struct FakeServer {
    pub base_url: String,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeServer {
    /// Connections accepted (one per HTTP attempt).
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    /// Raw requests received, in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub fn last_request(&self) -> String {
        self.requests().last().cloned().expect("a request arrived")
    }

    /// The JSON body of the last request.
    pub fn last_body(&self) -> Value {
        body_of(&self.last_request())
    }

    /// The user-role prompt of the last request.
    pub fn last_prompt(&self) -> String {
        self.last_body()["messages"][1]["content"]
            .as_str()
            .expect("user message")
            .to_string()
    }

    /// The system-role prompt of the last request.
    pub fn last_system(&self) -> String {
        self.last_body()["messages"][0]["content"]
            .as_str()
            .expect("system message")
            .to_string()
    }
}

pub fn body_of(raw: &str) -> Value {
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .expect("request has a body");
    serde_json::from_str(body).expect("JSON request body")
}

/// The raw text of the strict schema named `name` inside a captured
/// request, exactly as sent (a parsed `Value` would re-sort its keys):
/// what `schema_sha256` digests.
pub fn raw_schema<'a>(request: &'a str, name: &str) -> &'a str {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .expect("a body");
    let marker = format!("\"json_schema\":{{\"name\":\"{name}\",\"schema\":");
    let start = body.find(&marker).expect("the named schema") + marker.len();
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (n, &b) in body.as_bytes()[start..].iter().enumerate() {
        match (in_string, b) {
            (true, b'\\') if !escaped => escaped = true,
            (true, b'"') if !escaped => in_string = false,
            (true, _) => escaped = false,
            (false, b'"') => in_string = true,
            (false, b'{') => depth += 1,
            (false, b'}') => {
                depth -= 1;
                if depth == 0 {
                    return &body[start..=start + n];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced schema in {body}")
}

/// Serves `replies` in order, repeating the last one; each connection is
/// answered on its own thread.
pub fn serve(replies: Vec<Reply>) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (hits2, requests2) = (hits.clone(), requests.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let n = hits2.fetch_add(1, Ordering::SeqCst);
            let reply = replies[n.min(replies.len() - 1)].clone();
            let requests = requests2.clone();
            std::thread::spawn(move || answer(stream, reply, &requests));
        }
    });
    FakeServer {
        base_url: format!("http://{addr}/v1"),
        hits,
        requests,
    }
}

fn answer(mut s: TcpStream, reply: Reply, requests: &Mutex<Vec<String>>) {
    let raw = read_request(&mut s);
    requests.lock().unwrap().push(raw);
    if reply.hang_up {
        let _ = s.shutdown(Shutdown::Both);
        return;
    }
    std::thread::sleep(reply.delay);
    let mut head = format!(
        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    match reply.stall {
        Some(stall) => {
            let half = reply.body.len() / 2;
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&reply.body.as_bytes()[..half]);
            let _ = s.flush();
            std::thread::sleep(stall);
        }
        None => {
            let _ = s.write_all(format!("{head}{}", reply.body).as_bytes());
        }
    }
}

/// Reads one full HTTP request (headers + `content-length` body).
fn read_request(s: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    while let Ok(read) = s.read(&mut buf) {
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buf[..read]);
        let text = String::from_utf8_lossy(&data);
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    if k.eq_ignore_ascii_case("content-length") {
                        v.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&data).to_string()
}

/// A chat completion answering `content`, with usage, no finish reason.
pub fn envelope(content: &str) -> String {
    json!({
        "id": "chatcmpl-test-1",
        "model": "muse-wonder-1.0",
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 50 }
    })
    .to_string()
}

/// A chat completion with the given message, finish reason and usage.
pub fn completion(message: Value, finish_reason: Option<&str>, usage: Value) -> String {
    json!({
        "id": "chatcmpl-test-2",
        "model": "muse-wonder-1.0",
        "choices": [{ "message": message, "finish_reason": finish_reason }],
        "usage": usage
    })
    .to_string()
}

/// A config for the fake server: the documented defaults, a 5 s call
/// budget, two attempts, and a 1 ms backoff base so retries are quick.
pub fn config(base_url: &str) -> MetaConfig {
    let mut cfg =
        MetaConfig::from_parts(base_url, ApiKey::new(TEST_KEY), "muse-wonder-1.0").unwrap();
    cfg.timeout = Duration::from_secs(5);
    cfg.max_attempts = 2;
    cfg.retry_base_delay = Duration::from_millis(1);
    cfg
}

pub const DRAFT_PROPOSAL: &str = r#"{"summary":"Raise retries","plan_steps":["edit config"],"primary_claim":{"statement":"Timeouts come from too few retries"},"further_claims":[],"assumptions":["upstream is tolerant"],"uncertainties":[]}"#;

pub fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hyphae-consensus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
