// SPDX-License-Identifier: Apache-2.0
//! Hyphae Agent Memory adapter (pinned revision in README).
//!
//! Transport: MCP revision `2025-06-18`, newline-delimited JSON-RPC 2.0
//! over the stdio of `hyphae mcp --profile memory ...` — launched WITHOUT
//! `--allow-write`, so the server only advertises recall and status,
//! matching the `memory-reader` authority this project grants agents.
//! The credential reaches the child only as `--native-api-key-file <path>`
//! (its documented mechanism); no key value enters this process. Each
//! request has one deadline (a cancellation is sent when it passes), each
//! line is bounded, and the child's last 4 KiB of stderr are kept for the
//! operator ([`StdioTransport::stderr_tail`]) instead of discarded (HY-9).
//! Every Hyphae child process (the MCP adapter, the verifier) starts
//! without the Meta key variables in its environment (MA-4).
//!
//! Recalls (TC-4, HY-6, HY-12): every record keeps the project and scope
//! the backend reported (`backend_project`, `backend_scope`) and is marked
//! `retrieval: live_recall`. A record the backend attributed to neither
//! the requested project nor `_global` (or to no project at all) is never
//! evidence: it is returned apart ([`Recall::foreign`]) and the
//! orchestrator records its drop. A proven recall returns one proof for
//! its whole result; its records carry the proof's public reference and
//! the request it seals, and the proof itself is returned
//! ([`Recall::proof`]) even when the result is empty, as proof of absence.
//! When proof generation fails, the records are recalled unproven and the
//! failure travels typed ([`ProofFailure`]: Hyphae's category and message)
//! next to the persisted `proof_error` text (HY-4).
//!
//! Proof verification shells out to the documented offline verifier:
//! `hyphae proof verify --proof <FILE> --witness <FILE> --anchor <HEX>`.
//! Its exit status and report map to `IntegrityStatus` only; nothing here
//! judges truth. See [`HyphaeProofVerifier`] for what `Valid` requires.
//!
//! The exact JSON field names inside tool results are validated at
//! connect time (`tools/list`) and parsed defensively; final field-level
//! confirmation requires the live compatibility probe against an
//! installed Hyphae runtime (gated test in `tests/hyphae_adapter.rs`).

use crate::config::HyphaeConfig;
use consensus_domain::*;
use consensus_runtime::ports::{
    ForeignRecord, MemoryClient, MemoryError, MemoryStatus, ProofCheck, ProofFailure,
    ProofVerifier, Recall, RecallProof,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Longest line accepted from `hyphae mcp`; Hyphae itself bounds one MCP
/// message at 4 MiB.
pub const MAX_MCP_LINE_BYTES: usize = 8 * 1024 * 1024;
/// How much of the MCP child's stderr is kept for diagnostics.
pub const STDERR_TAIL_BYTES: usize = 4096;
/// Output kept from one verifier process, per stream; the rest is read
/// and dropped so the process never blocks on a full pipe.
pub const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
/// The project whose records every project's recall may see.
pub const GLOBAL_PROJECT: &str = "_global";
/// What a `hyphae proof verify` report of a memory proof says, as the
/// pinned 4.0.0 verifier printed it for the captured live recall proof
/// (2026-10-27): `"kind": "memory"`, `"scope": "semantic_reexecution"`.
pub const VERIFIED_KIND: &str = "memory";
pub const VERIFIED_SCOPE: &str = "semantic_reexecution";

pub trait McpTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, MemoryError>;
    fn notify(&mut self, method: &str, params: Value) -> Result<(), MemoryError>;
}

/// Scripted in-memory transport for tests: answers by method name from
/// per-method queues (the last scripted response repeats) and records
/// every request for assertions.
#[derive(Default)]
pub struct InMemoryTransport {
    pub responses: std::collections::HashMap<String, std::collections::VecDeque<Value>>,
    pub requests: Vec<(String, Value)>,
}

impl InMemoryTransport {
    pub fn script(&mut self, method: &str, response: Value) {
        self.responses
            .entry(method.to_string())
            .or_default()
            .push_back(response);
    }
}

impl McpTransport for InMemoryTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, MemoryError> {
        self.requests.push((method.to_string(), params));
        let queue = self.responses.get_mut(method).ok_or_else(|| {
            MemoryError::Backend(format!(
                "in-memory transport: no scripted response for {method}"
            ))
        })?;
        if queue.len() > 1 {
            Ok(queue.pop_front().expect("non-empty queue"))
        } else {
            Ok(queue.front().cloned().expect("non-empty queue"))
        }
    }
    fn notify(&mut self, method: &str, params: Value) -> Result<(), MemoryError> {
        self.requests.push((method.to_string(), params));
        Ok(())
    }
}

/// A command for a Hyphae child process, without the Meta key in its
/// environment (MA-4): Hyphae never needs it, and a wrapper such as
/// `docker exec` would carry it further.
fn hyphae_command(program: &str) -> Command {
    let mut command = Command::new(program);
    for name in crate::config::META_KEY_VARIABLES {
        command.env_remove(name);
    }
    command
}

/// The file name of a program path: errors are persisted, local paths
/// are not (SEC-9).
fn program_name(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string())
}

/// Backend text as one bounded line: control characters become spaces.
fn one_line(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= max_chars {
        flat
    } else {
        let cut: String = flat.chars().take(max_chars).collect();
        format!("{cut}…[truncated]")
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a local file, when this process can read it (artifacts in
/// a sealed snapshot are local; a container's are not).
fn file_sha256(path: &str) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| hex(&Sha256::digest(&bytes)))
}

enum Line {
    Text(String),
    TooLong,
}

/// Forwards `reader`'s lines (without their line ending) until EOF; a line
/// longer than `max` bytes is reported once and ends the stream.
fn forward_lines<R: BufRead>(mut reader: R, max: usize, tx: Sender<Line>) {
    loop {
        let mut buf = Vec::new();
        match (&mut reader)
            .take(max as u64 + 1)
            .read_until(b'\n', &mut buf)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                } else if buf.len() > max {
                    let _ = tx.send(Line::TooLong);
                    return;
                }
                if tx
                    .send(Line::Text(String::from_utf8_lossy(&buf).into_owned()))
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

/// Keeps the last `cap` bytes `stream` writes.
fn keep_tail<R: Read>(mut stream: R, tail: &Mutex<VecDeque<u8>>, cap: usize) {
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let Ok(mut kept) = tail.lock() else { return };
                kept.extend(&chunk[..n]);
                let excess = kept.len().saturating_sub(cap);
                kept.drain(..excess);
            }
        }
    }
}

/// A JSON-RPC error object as bounded text: its code and message only.
fn rpc_error_text(err: &Value) -> String {
    let code = err
        .get("code")
        .and_then(Value::as_i64)
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".into());
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .map(|m| one_line(m, 200))
        .unwrap_or_else(|| "no message".into());
    format!("code {code}: {message}")
}

/// Stdio JSON-RPC transport talking to a `hyphae mcp` child process.
/// The adapter allows one in-flight call; this client is sequential.
pub struct StdioTransport {
    child: Child,
    lines: Receiver<Line>,
    next_id: u64,
    timeout: Duration,
    stderr: Arc<Mutex<VecDeque<u8>>>,
}

impl StdioTransport {
    pub fn spawn(argv: &[String]) -> Result<Self, MemoryError> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| MemoryError::Unavailable("empty hyphae mcp argv".into()))?;
        let mut child = hyphae_command(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                MemoryError::Unavailable(format!("spawn {}: {}", program_name(program), e.kind()))
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MemoryError::Unavailable("mcp child missing stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| MemoryError::Unavailable("mcp child missing stderr".into()))?;
        let (tx, rx) = channel::<Line>();
        std::thread::spawn(move || forward_lines(BufReader::new(stdout), MAX_MCP_LINE_BYTES, tx));
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES)));
        let kept = tail.clone();
        std::thread::spawn(move || keep_tail(stderr, &kept, STDERR_TAIL_BYTES));
        Ok(Self {
            child,
            lines: rx,
            next_id: 0,
            timeout: Duration::from_secs(30),
            stderr: tail,
        })
    }

    /// The deadline of each request (30 s by default).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The last [`STDERR_TAIL_BYTES`] the child wrote to stderr, for the
    /// operator's diagnostics. Never put into errors or events: it can
    /// name local paths.
    pub fn stderr_tail(&self) -> String {
        self.stderr
            .lock()
            .map(|kept| {
                String::from_utf8_lossy(&kept.iter().copied().collect::<Vec<u8>>()).into_owned()
            })
            .unwrap_or_default()
    }

    /// The child stopped answering for good: says whether it exited.
    fn dead_child(&mut self, waiting_for: &str) -> MemoryError {
        // Its output closes as it exits; give the exit a moment to land.
        for _ in 0..10 {
            if let Ok(Some(status)) = self.child.try_wait() {
                return MemoryError::Unavailable(format!(
                    "hyphae mcp exited ({status}) before answering {waiting_for}"
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        MemoryError::Unavailable(format!(
            "hyphae mcp closed its output before answering {waiting_for}"
        ))
    }

    fn write_line(&mut self, value: &Value, what: &str) -> Result<(), MemoryError> {
        let mut line = serde_json::to_string(value)
            .map_err(|e| MemoryError::Backend(format!("encode mcp message: {e}")))?;
        line.push('\n');
        let written = match self.child.stdin.as_mut() {
            Some(stdin) => stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()),
            None => return Err(MemoryError::Unavailable("mcp child missing stdin".into())),
        };
        written.map_err(|_| self.dead_child(what))
    }
}

impl McpTransport for StdioTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, MemoryError> {
        self.next_id += 1;
        let id = self.next_id;
        // One deadline for the whole request, however many unrelated
        // lines (notifications) arrive first.
        let deadline = Instant::now() + self.timeout;
        self.write_line(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            method,
        )?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(remaining) {
                Ok(Line::Text(line)) => line,
                Ok(Line::TooLong) => {
                    return Err(MemoryError::Backend(format!(
                        "hyphae mcp sent a line longer than {MAX_MCP_LINE_BYTES} bytes while answering {method}"
                    )))
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Hyphae runs one tool call at a time and would answer
                    // the next request "busy" until this one ends.
                    let _ = self.notify(
                        "notifications/cancelled",
                        json!({"requestId": id, "reason": "client deadline passed"}),
                    );
                    return Err(MemoryError::Unavailable(format!(
                        "hyphae mcp did not answer {method} within {:?}",
                        self.timeout
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.dead_child(method)),
            };
            let msg: Value = serde_json::from_str(&line)
                .map_err(|e| MemoryError::Backend(format!("mcp response parse: {e}")))?;
            match msg.get("id").and_then(Value::as_u64) {
                Some(got) if got == id => {
                    if let Some(err) = msg.get("error") {
                        return Err(MemoryError::Backend(format!(
                            "mcp error for {method}: {}",
                            rpc_error_text(err)
                        )));
                    }
                    return msg.get("result").cloned().ok_or_else(|| {
                        MemoryError::Backend(format!("mcp response for {method} missing result"))
                    });
                }
                // Notifications or other traffic: keep waiting for our id.
                _ => continue,
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), MemoryError> {
        self.write_line(
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
            method,
        )
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A failed tool call, with what Hyphae said about it when it said it in
/// its structured tool-error envelope.
struct ToolFailure {
    category: Option<String>,
    message: Option<String>,
    error: MemoryError,
}

impl ToolFailure {
    fn plain(error: MemoryError) -> Self {
        Self {
            category: None,
            message: None,
            error,
        }
    }
}

/// One recalled record as mapped, before the attribution check.
struct Mapped {
    record: EvidenceReference,
    /// The backend attributed it to the requested project or `_global`.
    attributed: bool,
}

/// The public reference of a proof artifact: its file name without the
/// extension, never the path (e.g. `recall-01a17bf3-…-361661d67761`).
fn artifact_ref(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => file,
    };
    let cleaned: String = stem
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(128)
        .collect();
    if cleaned.is_empty() {
        "proof".into()
    } else {
        cleaned
    }
}

/// Read-only Agent Memory client over MCP.
pub struct HyphaeMemoryClient<T: McpTransport> {
    transport: T,
    pub tools: Vec<String>,
    now: Box<dyn Fn() -> String + Send>,
}

impl<T: McpTransport> HyphaeMemoryClient<T> {
    /// Performs the MCP handshake and verifies the memory tools exist.
    /// Refuses to operate against a surface missing `hyphae_memory_recall`.
    pub fn connect(mut transport: T) -> Result<Self, MemoryError> {
        transport.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "hyphae-consensus", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        transport.notify("notifications/initialized", json!({}))?;
        let list = transport.request("tools/list", json!({}))?;
        let tools: Vec<String> = list
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if !tools.iter().any(|t| t == "hyphae_memory_recall") {
            return Err(MemoryError::Backend(format!(
                "mcp surface lacks hyphae_memory_recall (advertised: {tools:?}); launch with --profile memory"
            )));
        }
        Ok(Self {
            transport,
            tools,
            now: Box::new(|| chrono::Utc::now().to_rfc3339()),
        })
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    /// The transport, e.g. for [`StdioTransport::stderr_tail`].
    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, ToolFailure> {
        if !self.tools.iter().any(|t| t == name) {
            return Err(ToolFailure::plain(MemoryError::Backend(format!(
                "tool {name} not advertised by server"
            ))));
        }
        let result = self
            .transport
            .request("tools/call", json!({"name": name, "arguments": arguments}))
            .map_err(ToolFailure::plain)?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(ToolFailure::plain(MemoryError::Backend(format!(
                "tool {name} returned an error result"
            ))));
        }
        if let Some(structured) = result.get("structuredContent") {
            // Hyphae may report tool failures inside structuredContent as
            // {"schema": "hyphae-native-mcp-tool-error-v2", "error": {...}}
            // with isError=false. That is an error, not data.
            let is_tool_error = structured.get("error").is_some()
                && structured
                    .get("schema")
                    .and_then(Value::as_str)
                    .map(|s| s.contains("tool-error"))
                    .unwrap_or(false);
            if is_tool_error {
                let error = structured.get("error");
                let field = |key: &str| {
                    error
                        .and_then(|e| e.get(key))
                        .and_then(Value::as_str)
                        .map(|v| one_line(v, 200))
                };
                let message = field("message");
                // Keep the category: the live corruption envelope says
                // "corruption" only there, never in `message`.
                let category = field("category").or_else(|| field("code"));
                let text = format!(
                    "tool {name} failed ({}): {}",
                    category.as_deref().unwrap_or("uncategorized"),
                    message.as_deref().unwrap_or("unknown tool error")
                );
                return Err(ToolFailure {
                    category,
                    message,
                    error: MemoryError::Backend(text),
                });
            }
            return Ok(structured.clone());
        }
        if let Some(text) = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
        {
            return serde_json::from_str(text).map_err(|e| {
                ToolFailure::plain(MemoryError::Backend(format!(
                    "tool {name} text content is not JSON: {e}"
                )))
            });
        }
        Err(ToolFailure::plain(MemoryError::Backend(format!(
            "tool {name} result has neither structuredContent nor text content"
        ))))
    }

    fn map_record(&self, project: &ProjectId, item: &Value) -> Mapped {
        let get = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| item.get(k).and_then(Value::as_str))
                .map(str::to_string)
        };
        let backend_id = get(&["id", "memory_id"]);
        let backend_project = get(&["project"]);
        let backend_scope = get(&["scope"]);
        let kind = get(&["kind"]).unwrap_or_else(|| "note".into());
        let content = get(&["text", "content"]).unwrap_or_default();
        // TC-4: a record counts for the run only when the backend itself
        // attributes it to the requested project or to `_global`; the
        // record keeps that attribution (a `_global` record stays labeled
        // `_global`).
        let attributed_to = match backend_project.as_deref() {
            Some(p) if p == project.as_str() => Some(project.clone()),
            Some(GLOBAL_PROJECT) => Some(ProjectId::new(GLOBAL_PROJECT)),
            _ => None,
        };
        // A record without an id gets one derived from what it is (HY-7),
        // so a second recall of it, in this round or the next, names it
        // the same way.
        let id = backend_id.clone().unwrap_or_else(|| {
            let mut hasher = Sha256::new();
            for part in [backend_project.as_deref().unwrap_or(""), &kind, &content] {
                hasher.update((part.len() as u64).to_be_bytes());
                hasher.update(part.as_bytes());
            }
            format!("hye-{}", &hex(&hasher.finalize())[..32])
        });
        Mapped {
            attributed: attributed_to.is_some(),
            record: EvidenceReference {
                schema_version: SCHEMA_VERSION.into(),
                id: EvidenceId::new(id),
                project_id: attributed_to.unwrap_or_else(|| project.clone()),
                backend_ref: backend_id.map(|b| format!("hyphae-memory:{b}")),
                kind,
                content,
                retrieved_at: (self.now)(),
                anchor: get(&["anchor", "anchor_digest", "anchor_hex"]),
                proof_file: get(&["proof_path", "proof_file", "proofOut", "proof_out"]),
                witness_file: get(&["witness_path", "witness_file", "witnessOut", "witness_out"]),
                proof_error: None,
                retrieval: Some(Retrieval::LiveRecall),
                snapshot_id: None,
                captured_at: None,
                proof_ref: None,
                proof_sha256: None,
                proof_request: None,
                backend_project,
                backend_scope,
            },
        }
    }
}

/// The record list of a recall result. An answer with no list at all is
/// an error, never "nothing in memory" (HY-7).
fn record_list(value: &Value) -> Result<Vec<Value>, MemoryError> {
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::Object(map) => ["memories", "results", "items", "records"]
            .iter()
            .find_map(|k| map.get(*k).and_then(Value::as_array).cloned())
            .ok_or_else(|| {
                MemoryError::Backend(
                    "recall result holds no record list (memories, results, items or records)"
                        .into(),
                )
            }),
        _ => Err(MemoryError::Backend(
            "recall result is neither a list nor an object".into(),
        )),
    }
}

impl<T: McpTransport> MemoryClient for HyphaeMemoryClient<T> {
    fn recall(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EvidenceReference>, MemoryError> {
        self.recall_with_proof(project, query, limit)
            .map(|recall| recall.records)
    }

    fn recall_with_proof(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Recall, MemoryError> {
        // Proofs are requested first. Proof generation can fail while
        // plain recall works (seen live on the pinned 4.0.0 build while
        // the daemon's socket sat inside its data directory: the witness
        // bundler rejects the special file and Hyphae reports its generic
        // `corruption` code). The unproven records are still returned, but
        // each carries the failure reason in `proof_error`, and the recall
        // the typed failure: the runtime treats a corruption-class failure
        // as `Invalid` (Blocked) and any other as `Unavailable`. It never
        // upgrades to verified.
        let arguments = |prove: bool| json!({"project": project.as_str(), "query": query, "limit": limit, "prove": prove});
        let (value, failure) = match self.call_tool("hyphae_memory_recall", arguments(true)) {
            Ok(v) => (v, None),
            Err(failure) => {
                let v = self
                    .call_tool("hyphae_memory_recall", arguments(false))
                    .map_err(|f| f.error)?;
                (v, Some(failure))
            }
        };
        let items = record_list(&value)?;
        // A proven recall carries ONE sealed proof for the whole ordered
        // result (`proof: {proof_path, witness_path, anchor_hex}`), not one
        // per record: every record of this recall is covered by it.
        let shared = value.get("proof").filter(|p| p.is_object());
        let shared_field = |keys: &[&str]| {
            shared.and_then(|p| {
                keys.iter()
                    .find_map(|k| p.get(k).and_then(Value::as_str))
                    .map(str::to_string)
            })
        };
        let shared_proof = shared_field(&["proof_path", "proof_file"]);
        let shared_witness = shared_field(&["witness_path", "witness_file"]);
        let shared_anchor = shared_field(&["anchor_hex", "anchor", "anchor_digest"]);
        let request = ProofRequest {
            operation: "recall".into(),
            project: project.as_str().to_string(),
            query: Some(query.to_string()),
            limit: u32::try_from(limit).ok(),
        };
        let shared_ref = shared_proof.as_deref().map(artifact_ref);
        let shared_sha256 = shared_proof.as_deref().and_then(file_sha256);
        let proof_error = failure.as_ref().map(|f| f.error.to_string());

        let mut recall = Recall {
            proof_failure: failure.map(|f| ProofFailure {
                category: f.category,
                message: f.message.unwrap_or_else(|| f.error.to_string()),
            }),
            ..Recall::default()
        };
        let mut sealed = Vec::with_capacity(items.len());
        for item in &items {
            let Mapped {
                mut record,
                attributed,
            } = self.map_record(project, item);
            sealed.push(record.id.clone());
            if record.proof_file.is_none() && shared_proof.is_some() {
                record.proof_file = shared_proof.clone();
                record.witness_file = shared_witness.clone();
                record.anchor = record.anchor.or_else(|| shared_anchor.clone());
                record.proof_ref = shared_ref.clone();
                record.proof_sha256 = shared_sha256.clone();
                record.proof_request = Some(request.clone());
            }
            if record.proof_file.is_none() {
                // Never silently unproven: either the proven call failed
                // (its reason travels on), or it "succeeded" without
                // artifacts, which is recorded as such.
                record.proof_error = proof_error
                    .clone()
                    .or_else(|| Some("recall(prove=true) returned no proof artifacts".into()));
            }
            if attributed {
                recall.records.push(record);
            } else {
                recall.foreign.push(ForeignRecord {
                    evidence_id: record.id,
                    backend_project: record.backend_project,
                });
            }
        }
        if let (Some(proof_ref), Some(anchor)) = (shared_ref, shared_anchor) {
            recall.proof = Some(RecallProof {
                request,
                evidence_ids: sealed,
                proof_ref,
                anchor,
            });
        }
        Ok(recall)
    }

    fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
        let value = self
            .call_tool("hyphae_memory_status", json!({}))
            .map_err(|f| f.error)?;
        Ok(MemoryStatus {
            healthy: true,
            detail: value.to_string().chars().take(300).collect(),
        })
    }
}

/// Exit status and captured output of one external command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Why an external command produced no output to judge.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("could not start {0}")]
    Spawn(String),
    #[error("did not finish within {} s and was killed", .0.as_secs())]
    TimedOut(Duration),
    #[error("could not wait for the process ({0})")]
    Wait(String),
}

/// Runs external verification commands; injectable for tests.
pub trait CommandRunner {
    /// Runs `argv` to completion, or kills it once `timeout` has passed
    /// ([`CommandError::TimedOut`]).
    fn run(&mut self, argv: &[String], timeout: Duration) -> Result<CommandOutput, CommandError>;
}

/// Reads all of `stream`, keeping its first `cap` bytes (and whether more
/// came): the rest is read and dropped, so the writer never blocks.
pub fn read_capped<R: Read>(stream: &mut R, cap: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return (kept, truncated),
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
                truncated |= n > room;
            }
        }
    }
}

fn captured_text((bytes, truncated): (Vec<u8>, bool)) -> String {
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        format!("{text}…[truncated]")
    } else {
        text
    }
}

/// A stream read on its own thread, so a deadline can be kept.
fn capture<R: Read + Send + 'static>(stream: Option<R>) -> Receiver<(Vec<u8>, bool)> {
    let (tx, rx) = channel();
    match stream {
        Some(mut stream) => {
            std::thread::spawn(move || {
                let _ = tx.send(read_capped(&mut stream, MAX_COMMAND_OUTPUT_BYTES));
            });
        }
        None => {
            let _ = tx.send((Vec::new(), false));
        }
    }
    rx
}

/// Runs commands as child processes with a deadline (HY-2): past it the
/// child is killed and the run is [`CommandError::TimedOut`]. Each output
/// stream is capped at [`MAX_COMMAND_OUTPUT_BYTES`], and the Meta key
/// variables are removed from the child's environment. Only the direct
/// child is killed: a wrapper's own children (a `cmd.exe` running `docker
/// exec`, say) are not, so deployments run the verifier binary directly.
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, argv: &[String], timeout: Duration) -> Result<CommandOutput, CommandError> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| CommandError::Spawn("an empty command".into()))?;
        let mut child = hyphae_command(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                CommandError::Spawn(format!("{} ({})", program_name(program), e.kind()))
            })?;
        let stdout = capture(child.stdout.take());
        let stderr = capture(child.stderr.take());
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    let now = Instant::now();
                    if now >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(CommandError::TimedOut(timeout));
                    }
                    std::thread::sleep((deadline - now).min(Duration::from_millis(10)));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandError::Wait(e.kind().to_string()));
                }
            }
        };
        // The pipes close as the child exits, unless a process it started
        // still holds them: wait for them only within the deadline (or a
        // short grace past it).
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(200));
        let collect = |rx: Receiver<(Vec<u8>, bool)>| {
            rx.recv_timeout(wait)
                .map(captured_text)
                .unwrap_or_else(|_| "…[output still open when the process exited]".into())
        };
        Ok(CommandOutput {
            code: status.code().unwrap_or(-1),
            stdout: collect(stdout),
            stderr: collect(stderr),
        })
    }
}

/// Artifacts identified by their content and the anchor they were
/// checked against (HY-13): proof SHA-256, witness SHA-256, anchor.
type ArtifactKey = (String, String, String);

/// Integrity-only verifier backed by `hyphae proof verify`.
///
/// `Valid` requires exit 0 AND the verifier's own report saying
/// `status: verified` with semantic re-execution (`scope:
/// semantic_reexecution`) of a memory proof (`kind: memory`) whose
/// `anchor_digest` is the anchor it was given: the recall was re-run
/// against the witness and reproduced the sealed result (HY-1). A verified
/// proof of another kind, or bound to another anchor, is `Invalid`: it does
/// not prove this record. A report that falls short in any other way (no
/// re-execution, another scope, no kind, no anchor digest) is a
/// `VerificationError`: nothing is inferred from an absent field. The
/// report's fields are kept in `ProofCheck::report`, and the detail says
/// whether the anchor was pinned by the operator or reported by the
/// backend itself.
///
/// Failures are classified from the structured error's `category`:
/// `corruption` (tampered or inconsistent artifacts) and `invalid-request`
/// (e.g. an anchor the proof does not match) are `Invalid`; without a
/// structured error the stderr keywords decide as before; everything else
/// is a `VerificationError`. The persisted detail carries the structured
/// error's category and message (one bounded line each) or, without one,
/// the first 300 characters of stderr on one line (SEC-9). A verifier that
/// outlives its deadline ([`timeout`](Self::timeout), 60 s by default) is
/// killed and the record is a `VerificationError` whose detail starts with
/// `verifier_timeout`.
///
/// Results are reused for the same artifact bytes and anchor (SHA-256 of
/// the proof and of the witness, read here), never by path, and only when
/// both files are readable by this process; a `VerificationError` is
/// never reused (HY-13).
///
/// Trust note: without a pinned anchor the anchor is taken from the same
/// recall response that delivered the proof, so verification then proves
/// internal consistency against that server-supplied anchor. Operators
/// can pin `expected_anchor` (obtained out-of-band); records anchored
/// elsewhere are then reported `Invalid` without invoking the verifier,
/// and the pinned value is what the verifier is given (HY-10). The record
/// text handed to models is the MCP response's rendering of the proven
/// result; the proof seals that text, but this verifier does not
/// byte-compare it (the artifacts stay inside the Hyphae host).
pub struct HyphaeProofVerifier<R: CommandRunner> {
    pub binary: String,
    pub runner: R,
    pub expected_anchor: Option<String>,
    /// Deadline of one verifier process.
    pub timeout: Duration,
    verified: std::collections::HashMap<ArtifactKey, ProofCheck>,
}

impl<R: CommandRunner> HyphaeProofVerifier<R> {
    pub fn new(binary: String, runner: R) -> Self {
        Self {
            binary,
            runner,
            expected_anchor: None,
            timeout: Duration::from_secs(crate::config::DEFAULT_VERIFY_TIMEOUT_SECS),
            verified: std::collections::HashMap::new(),
        }
    }

    /// A verifier as configured: binary, pinned anchor and deadline.
    pub fn from_config(cfg: &HyphaeConfig, runner: R) -> Self {
        let mut verifier = Self::new(cfg.binary.clone(), runner);
        verifier.expected_anchor = cfg.expected_anchor.clone();
        verifier.timeout = cfg.verify_timeout;
        verifier
    }

    /// `error.category` and `error.message` of the verifier's structured
    /// failure report, e.g. `{"error":{"category":"corruption","message":
    /// "native durable state is invalid",...},"exit_class":9}`; `(None,
    /// None)` when stderr holds no such line.
    fn failure_report(stderr: &str) -> (Option<String>, Option<String>) {
        stderr
            .lines()
            .find_map(|line| {
                let parsed = serde_json::from_str::<Value>(line.trim()).ok()?;
                let error = parsed.get("error")?;
                let field = |key: &str| {
                    error
                        .get(key)
                        .and_then(Value::as_str)
                        .map(|v| one_line(v, 200))
                };
                let category = field("category")?;
                Some((Some(category), field("message")))
            })
            .unwrap_or((None, None))
    }

    /// Runs the verifier on the artifacts against `anchor` (`pinned`: the
    /// operator pinned it; else the backend reported it with the proof).
    fn check_artifacts(
        &mut self,
        proof: &str,
        witness: &str,
        anchor: &str,
        pinned: bool,
    ) -> ProofCheck {
        let argv: Vec<String> = vec![
            self.binary.clone(),
            "proof".into(),
            "verify".into(),
            "--proof".into(),
            proof.to_string(),
            "--witness".into(),
            witness.to_string(),
            "--anchor".into(),
            anchor.to_string(),
        ];
        let checked = |status, detail: String, report: Option<VerifierReport>| ProofCheck {
            status,
            verifier: Some("hyphae proof verify".into()),
            verified_anchor: Some(anchor.to_string()),
            detail: Some(detail),
            report,
        };
        let out = match self.runner.run(&argv, self.timeout) {
            Ok(out) => out,
            Err(CommandError::TimedOut(after)) => {
                return checked(
                    IntegrityStatus::VerificationError,
                    format!(
                        "verifier_timeout: hyphae proof verify did not finish within {} s and was killed",
                        after.as_secs()
                    ),
                    None,
                )
            }
            Err(e) => {
                return checked(
                    IntegrityStatus::VerificationError,
                    format!("verifier could not run: {e}"),
                    None,
                )
            }
        };
        if out.code == 0 {
            let report = serde_json::from_str::<Value>(out.stdout.trim())
                .ok()
                .filter(Value::is_object);
            let unverified = || {
                format!(
                    "verifier exited 0 without a verified semantic re-execution report: {}",
                    one_line(&out.stdout, 300)
                )
            };
            let Some(report) = report else {
                return checked(IntegrityStatus::VerificationError, unverified(), None);
            };
            let field = |key: &str| report.get(key).and_then(Value::as_str).map(str::to_string);
            let structured = VerifierReport {
                status: field("status"),
                scope: field("scope"),
                kind: field("kind"),
                anchor_digest: field("anchor_digest"),
                result_digest: field("result_digest"),
                proof_digest: field("proof_digest"),
            };
            let reexecuted = report
                .get("semantic_reexecution_performed")
                .and_then(Value::as_bool)
                == Some(true);
            let shown = |v: &Option<String>| {
                v.as_deref()
                    .map(|s| one_line(s, 80))
                    .unwrap_or_else(|| "?".into())
            };
            // Exit 0 without a verified re-execution report proves nothing.
            if structured.status.as_deref() != Some("verified") || !reexecuted {
                return checked(
                    IntegrityStatus::VerificationError,
                    unverified(),
                    Some(structured),
                );
            }
            if structured.scope.as_deref() != Some(VERIFIED_SCOPE) {
                let detail = format!(
                    "the verifier checked the proof at scope {}, not {VERIFIED_SCOPE}",
                    shown(&structured.scope)
                );
                return checked(IntegrityStatus::VerificationError, detail, Some(structured));
            }
            match structured.kind.as_deref() {
                Some(VERIFIED_KIND) => {}
                Some(_) => {
                    let detail = format!(
                        "the proof is a {} proof, not a {VERIFIED_KIND} proof: it does not prove this record",
                        shown(&structured.kind)
                    );
                    return checked(IntegrityStatus::Invalid, detail, Some(structured));
                }
                None => {
                    let detail = format!(
                        "the verifier report names no proof kind, so it does not show a {VERIFIED_KIND} proof"
                    );
                    return checked(IntegrityStatus::VerificationError, detail, Some(structured));
                }
            }
            // The report must name the anchor it verified against, and it
            // must be the one given: an absent digest binds the proof to
            // nothing this record claims.
            match &structured.anchor_digest {
                Some(digest) if digest.trim().eq_ignore_ascii_case(anchor) => {}
                Some(_) => {
                    let detail = format!(
                        "the verifier checked anchor {}, not the anchor it was given ({anchor})",
                        shown(&structured.anchor_digest)
                    );
                    return checked(IntegrityStatus::Invalid, detail, Some(structured));
                }
                None => {
                    let detail = format!(
                        "the verifier report names no anchor digest, so it does not show the proof bound to anchor {anchor}"
                    );
                    return checked(IntegrityStatus::VerificationError, detail, Some(structured));
                }
            }
            let provenance = if pinned {
                "the anchor is the operator-pinned one"
            } else {
                "the anchor is the one the backend reported with the proof (none pinned: this shows self-consistency, not independent trust)"
            };
            let detail = format!(
                "offline verifier re-executed the recall against the witness and accepted the proof (scope {}, kind {}, anchor {anchor}, result digest {}); {provenance}; the proof covers the whole recall result this record belongs to; the record's text is not byte-compared with the proof",
                shown(&structured.scope),
                shown(&structured.kind),
                shown(&structured.result_digest)
            );
            return checked(IntegrityStatus::Valid, detail, Some(structured));
        }
        // Only a verifier rejection is Invalid. Transport/usage failures
        // (docker 125-127, missing files, I/O) are infrastructure errors,
        // never proof that a record was tampered with. Without a structured
        // error the keyword rule stays as wide as it always was (LC-10).
        let (category, message) = Self::failure_report(&out.stderr);
        let rejection = match category.as_deref() {
            Some(c) => c == "corruption" || c == "invalid-request",
            None => {
                let lower = out.stderr.to_lowercase();
                lower.contains("mismatch")
                    || lower.contains("invalid")
                    || lower.contains("corrupt")
                    || lower.contains("digest")
            }
        };
        let status = if rejection && !(125..=127).contains(&out.code) {
            IntegrityStatus::Invalid
        } else {
            IntegrityStatus::VerificationError
        };
        // Hyphae's own message when it gave one; otherwise the text came
        // from a wrapper or the OS, kept bounded on one line.
        let said = message.unwrap_or_else(|| one_line(&out.stderr, 300));
        checked(
            status,
            format!(
                "verifier exited {} (category {}): {said}",
                out.code,
                category.as_deref().unwrap_or("unknown"),
            ),
            None,
        )
    }
}

impl<R: CommandRunner> ProofVerifier for HyphaeProofVerifier<R> {
    fn verify(&mut self, evidence: &EvidenceReference) -> ProofCheck {
        let (Some(proof), Some(witness), Some(anchor)) = (
            &evidence.proof_file,
            &evidence.witness_file,
            &evidence.anchor,
        ) else {
            return ProofCheck {
                status: IntegrityStatus::Unavailable,
                verifier: None,
                verified_anchor: evidence.anchor.clone(),
                detail: Some("no proof/witness/anchor artifacts available for this record".into()),
                report: None,
            };
        };
        let record_anchor = anchor.trim().to_ascii_lowercase();
        let pinned = self.expected_anchor.is_some();
        let anchor = match &self.expected_anchor {
            Some(expected) => {
                let expected = expected.trim().to_ascii_lowercase();
                if expected != record_anchor {
                    return ProofCheck {
                        status: IntegrityStatus::Invalid,
                        verifier: Some("operator-pinned anchor".into()),
                        verified_anchor: Some(anchor.clone()),
                        detail: Some(format!(
                            "record anchor does not match operator-pinned anchor {expected}"
                        )),
                        report: None,
                    };
                }
                // HY-10: the verifier is given the pinned value.
                expected
            }
            None => record_anchor,
        };
        let key = file_sha256(proof)
            .zip(file_sha256(witness))
            .map(|(p, w)| (p, w, anchor.clone()));
        if let Some(done) = key.as_ref().and_then(|k| self.verified.get(k)) {
            return done.clone();
        }
        let check = self.check_artifacts(proof, witness, &anchor, pinned);
        if let Some(key) = key {
            if check.status != IntegrityStatus::VerificationError {
                self.verified.insert(key, check.clone());
            }
        }
        check
    }
}

/// Spawns the read-only MCP adapter for a configured Hyphae deployment.
pub fn connect_from_config(
    cfg: &HyphaeConfig,
) -> Result<HyphaeMemoryClient<StdioTransport>, MemoryError> {
    let transport = StdioTransport::spawn(&cfg.mcp_argv())?;
    HyphaeMemoryClient::connect(transport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn lines(input: &[u8], max: usize) -> Vec<Option<String>> {
        let (tx, rx) = channel();
        forward_lines(Cursor::new(input.to_vec()), max, tx);
        rx.iter()
            .map(|l| match l {
                Line::Text(t) => Some(t),
                Line::TooLong => None,
            })
            .collect()
    }

    #[test]
    fn mcp_lines_are_bounded() {
        assert_eq!(
            lines(b"{\"a\":1}\r\n{\"b\":2}\nlast", 16),
            [
                Some("{\"a\":1}".into()),
                Some("{\"b\":2}".into()),
                Some("last".into())
            ]
        );
        // A line of exactly the bound passes; one byte more ends the
        // stream with a single TooLong.
        let exact = format!("{}\n", "x".repeat(16));
        assert_eq!(lines(exact.as_bytes(), 16), [Some("x".repeat(16))]);
        let over = format!("{}\nnext\n", "x".repeat(17));
        assert_eq!(lines(over.as_bytes(), 16), [None]);
    }

    #[test]
    fn the_stderr_tail_keeps_only_the_last_bytes() {
        let tail = Mutex::new(VecDeque::new());
        let text: Vec<u8> = (0..10_000u32).map(|i| b'a' + (i % 26) as u8).collect();
        keep_tail(Cursor::new(text.clone()), &tail, STDERR_TAIL_BYTES);
        let kept: Vec<u8> = tail.into_inner().unwrap().into_iter().collect();
        assert_eq!(kept.len(), STDERR_TAIL_BYTES);
        assert_eq!(kept, text[text.len() - STDERR_TAIL_BYTES..]);
    }

    #[test]
    fn command_output_is_capped_and_marked() {
        let big = vec![b'x'; 100];
        let (kept, truncated) = read_capped(&mut Cursor::new(big), 10);
        assert_eq!((kept.len(), truncated), (10, true));
        assert_eq!(
            captured_text((kept, truncated)),
            format!("{}…[truncated]", "x".repeat(10))
        );
        let (kept, truncated) = read_capped(&mut Cursor::new(b"ok".to_vec()), 10);
        assert_eq!((kept, truncated), (b"ok".to_vec(), false));
    }

    #[test]
    fn artifact_refs_never_carry_a_path() {
        assert_eq!(
            artifact_ref("/hyphae-home/.local/state/hyphae/proofs/recall-01a17bf3-879a-7833-94a4-361661d67761.proof"),
            "recall-01a17bf3-879a-7833-94a4-361661d67761"
        );
        assert_eq!(artifact_ref("C:\\proofs\\p.hyproof"), "p");
        assert_eq!(artifact_ref("/tmp/../"), "proof");
        assert!(!artifact_ref("a/b c$d.proof").contains(['/', ' ', '$']));
    }

    #[test]
    fn rpc_errors_keep_only_their_code_and_a_bounded_message() {
        let err = json!({"code": -32001, "message": "Server busy:\none tool call is already active", "data": {"path": "/hyphae-home/secret"}});
        let text = rpc_error_text(&err);
        assert_eq!(
            text,
            "code -32001: Server busy: one tool call is already active"
        );
        assert!(!text.contains("hyphae-home"));
    }
}
