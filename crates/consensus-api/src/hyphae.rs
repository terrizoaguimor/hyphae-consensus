// SPDX-License-Identifier: Apache-2.0
//! Hyphae Agent Memory adapter (pinned revision in README).
//!
//! Transport: MCP revision `2025-06-18`, newline-delimited JSON-RPC 2.0
//! over the stdio of `hyphae mcp --profile memory ...` — launched WITHOUT
//! `--allow-write`, so the server only advertises recall and status,
//! matching the `memory-reader` authority this project grants agents.
//! The credential reaches the child only as `--native-api-key-file <path>`
//! (its documented mechanism); no key value enters this process.
//!
//! Proof verification shells out to the documented offline verifier:
//! `hyphae proof verify --proof <FILE> --witness <FILE> --anchor <HEX>`.
//! Its exit status and report map to `IntegrityStatus` only; nothing here
//! judges truth. A proven recall returns one proof for its whole result,
//! which this adapter attaches to every record of that recall.
//!
//! The exact JSON field names inside tool results are validated at
//! connect time (`tools/list`) and parsed defensively; final field-level
//! confirmation requires the live compatibility probe against an
//! installed Hyphae runtime (gated test in `tests/hyphae_adapter.rs`).

use crate::config::HyphaeConfig;
use consensus_domain::*;
use consensus_runtime::ports::{
    MemoryClient, MemoryError, MemoryStatus, ProofCheck, ProofVerifier,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

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

/// Stdio JSON-RPC transport talking to a `hyphae mcp` child process.
/// The adapter allows one in-flight call; this client is sequential.
pub struct StdioTransport {
    child: Child,
    lines: Receiver<String>,
    next_id: u64,
    timeout: Duration,
}

impl StdioTransport {
    pub fn spawn(argv: &[String]) -> Result<Self, MemoryError> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| MemoryError::Unavailable("empty hyphae mcp argv".into()))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| MemoryError::Unavailable(format!("spawn {program}: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MemoryError::Unavailable("mcp child missing stdout".into()))?;
        let (tx, rx) = channel::<String>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child,
            lines: rx,
            next_id: 0,
            timeout: Duration::from_secs(30),
        })
    }

    fn write_line(&mut self, value: &Value) -> Result<(), MemoryError> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| MemoryError::Unavailable("mcp child missing stdin".into()))?;
        let mut line = serde_json::to_string(value)
            .map_err(|e| MemoryError::Backend(format!("encode mcp message: {e}")))?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| MemoryError::Unavailable(format!("write to mcp child: {e}")))
    }
}

impl McpTransport for StdioTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, MemoryError> {
        self.next_id += 1;
        let id = self.next_id;
        self.write_line(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            let line = self
                .lines
                .recv_timeout(self.timeout)
                .map_err(|e| MemoryError::Unavailable(format!("mcp response wait: {e}")))?;
            let msg: Value = serde_json::from_str(&line)
                .map_err(|e| MemoryError::Backend(format!("mcp response parse: {e}")))?;
            match msg.get("id").and_then(Value::as_u64) {
                Some(got) if got == id => {
                    if let Some(err) = msg.get("error") {
                        return Err(MemoryError::Backend(format!(
                            "mcp error for {method}: {err}"
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
        self.write_line(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Read-only Agent Memory client over MCP.
pub struct HyphaeMemoryClient<T: McpTransport> {
    transport: T,
    pub tools: Vec<String>,
    now: Box<dyn Fn() -> String + Send>,
    recall_counter: u64,
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
            recall_counter: 0,
        })
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, MemoryError> {
        if !self.tools.iter().any(|t| t == name) {
            return Err(MemoryError::Backend(format!(
                "tool {name} not advertised by server"
            )));
        }
        let result = self
            .transport
            .request("tools/call", json!({"name": name, "arguments": arguments}))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(MemoryError::Backend(format!(
                "tool {name} returned an error result"
            )));
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
                        .map(str::to_string)
                };
                let message = field("message").unwrap_or_else(|| "unknown tool error".into());
                // Keep the category: the live corruption envelope says
                // "corruption" only there, never in `message`.
                let category = field("category")
                    .or_else(|| field("code"))
                    .unwrap_or_else(|| "uncategorized".into());
                return Err(MemoryError::Backend(format!(
                    "tool {name} failed ({category}): {message}"
                )));
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
                MemoryError::Backend(format!("tool {name} text content is not JSON: {e}"))
            });
        }
        Err(MemoryError::Backend(format!(
            "tool {name} result has neither structuredContent nor text content"
        )))
    }

    fn map_record(&mut self, project: &ProjectId, item: &Value) -> EvidenceReference {
        self.recall_counter += 1;
        let get = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| item.get(k).and_then(Value::as_str))
                .map(str::to_string)
        };
        let backend_id = get(&["id", "memory_id"]);
        EvidenceReference {
            schema_version: SCHEMA_VERSION.into(),
            id: EvidenceId::new(
                backend_id
                    .clone()
                    .unwrap_or_else(|| format!("hye-recall-{}", self.recall_counter)),
            ),
            project_id: project.clone(),
            backend_ref: backend_id.map(|b| format!("hyphae-memory:{b}")),
            kind: get(&["kind"]).unwrap_or_else(|| "note".into()),
            content: get(&["text", "content"]).unwrap_or_default(),
            retrieved_at: (self.now)(),
            anchor: get(&["anchor", "anchor_digest", "anchor_hex"]),
            proof_file: get(&["proof_path", "proof_file", "proofOut", "proof_out"]),
            witness_file: get(&["witness_path", "witness_file", "witnessOut", "witness_out"]),
            proof_error: None,
        }
    }
}

impl<T: McpTransport> MemoryClient for HyphaeMemoryClient<T> {
    fn recall(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EvidenceReference>, MemoryError> {
        // Proofs are requested first. Proof generation can fail while
        // plain recall works (seen live on the pinned 4.0.0 build while
        // the daemon's socket sat inside its data directory: the witness
        // bundler rejects the special file and Hyphae reports its generic
        // `corruption` code). The unproven records are still returned, but
        // each carries the failure reason in `proof_error`: the runtime
        // treats a corruption-class reason as `Invalid` (Blocked) and any
        // other as `Unavailable`. It never upgrades to verified.
        let proven = self.call_tool(
            "hyphae_memory_recall",
            json!({"project": project.as_str(), "query": query, "limit": limit, "prove": true}),
        );
        let (value, proof_error) = match proven {
            Ok(v) => (v, None),
            Err(e) => {
                let reason = e.to_string();
                let v = self.call_tool(
                    "hyphae_memory_recall",
                    json!({"project": project.as_str(), "query": query, "limit": limit, "prove": false}),
                )?;
                (v, Some(reason))
            }
        };
        let items: Vec<Value> = match &value {
            Value::Array(a) => a.clone(),
            Value::Object(map) => ["memories", "results", "items", "records"]
                .iter()
                .find_map(|k| map.get(*k).and_then(Value::as_array).cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
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
        Ok(items
            .iter()
            .map(|i| {
                let mut rec = self.map_record(project, i);
                if rec.proof_file.is_none() && shared_proof.is_some() {
                    rec.proof_file = shared_proof.clone();
                    rec.witness_file = shared_witness.clone();
                    rec.anchor = rec.anchor.or_else(|| shared_anchor.clone());
                }
                if rec.proof_file.is_none() {
                    // Never silently unproven: either the proven call failed
                    // (its reason travels on), or it "succeeded" without
                    // artifacts, which is recorded as such.
                    rec.proof_error = proof_error
                        .clone()
                        .or_else(|| Some("recall(prove=true) returned no proof artifacts".into()));
                }
                rec
            })
            .collect())
    }

    fn status(&mut self) -> Result<MemoryStatus, MemoryError> {
        let value = self.call_tool("hyphae_memory_status", json!({}))?;
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

/// Runs external verification commands; injectable for tests.
pub trait CommandRunner {
    /// Returns the command's output, or a spawn/transport error string.
    fn run(&mut self, argv: &[String]) -> Result<CommandOutput, String>;
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, argv: &[String]) -> Result<CommandOutput, String> {
        let (program, args) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;
        let out = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("spawn {program}: {e}"))?;
        Ok(CommandOutput {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }
}

/// Integrity-only verifier backed by `hyphae proof verify`.
///
/// `Valid` requires exit 0 AND the verifier's own report saying
/// `status: verified` with semantic re-execution: the recall was re-run
/// against the witness and reproduced the sealed result. That one proof
/// covers every record of the recall, so each distinct proof is verified
/// once per run and the result reused. Failures are classified from the
/// structured error's `category`: `corruption` (tampered or inconsistent
/// artifacts) and `invalid-request` (e.g. an anchor the proof does not
/// match) are `Invalid`; everything else is a `VerificationError`.
///
/// Trust note: by default the anchor is taken from the same recall
/// response that delivered the proof, so verification then proves
/// internal consistency against that server-supplied anchor. Operators
/// can pin `expected_anchor` (obtained out-of-band); records anchored
/// elsewhere are then reported `Invalid` without invoking the verifier.
/// The record text handed to models is the MCP response's rendering of
/// the proven result; the proof seals that text, but this verifier does
/// not byte-compare it (the artifacts stay inside the Hyphae host).
pub struct HyphaeProofVerifier<R: CommandRunner> {
    pub binary: String,
    pub runner: R,
    pub expected_anchor: Option<String>,
    /// Results by (proof, witness, anchor): records of one recall share
    /// one proof, which needs checking only once.
    verified: std::collections::HashMap<(String, String, String), ProofCheck>,
}

impl<R: CommandRunner> HyphaeProofVerifier<R> {
    pub fn new(binary: String, runner: R) -> Self {
        Self {
            binary,
            runner,
            expected_anchor: None,
            verified: std::collections::HashMap::new(),
        }
    }

    /// `error.category` of the verifier's structured failure report,
    /// e.g. `{"error":{"category":"corruption",...},"exit_class":9}`.
    fn failure_category(stderr: &str) -> Option<String> {
        stderr.lines().find_map(|line| {
            serde_json::from_str::<Value>(line.trim())
                .ok()?
                .get("error")?
                .get("category")?
                .as_str()
                .map(str::to_string)
        })
    }

    fn check_artifacts(&mut self, proof: &str, witness: &str, anchor: &str) -> ProofCheck {
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
        let checked = |status, detail: String| ProofCheck {
            status,
            verifier: Some("hyphae proof verify".into()),
            verified_anchor: Some(anchor.to_string()),
            detail: Some(detail),
        };
        let out = match self.runner.run(&argv) {
            Ok(out) => out,
            Err(e) => return checked(IntegrityStatus::VerificationError, e),
        };
        if out.code == 0 {
            let report: Option<Value> = serde_json::from_str(out.stdout.trim()).ok();
            let field = |key: &str| {
                report
                    .as_ref()
                    .and_then(|r| r.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string()
            };
            let reexecuted = report
                .as_ref()
                .and_then(|r| r.get("semantic_reexecution_performed"))
                .and_then(Value::as_bool)
                == Some(true);
            if field("status") == "verified" && reexecuted {
                return checked(
                    IntegrityStatus::Valid,
                    format!(
                        "offline verifier re-executed the recall against the witness and accepted the proof (scope {}, kind {}, result digest {}); the proof covers the whole recall result this record belongs to",
                        field("scope"),
                        field("kind"),
                        field("result_digest")
                    ),
                );
            }
            // Exit 0 without a verified re-execution report proves nothing.
            return checked(
                IntegrityStatus::VerificationError,
                format!(
                    "verifier exited 0 without a verified semantic re-execution report: {}",
                    out.stdout.chars().take(300).collect::<String>()
                ),
            );
        }
        // Only a verifier rejection is Invalid. Transport/usage failures
        // (docker 125-127, missing files, I/O) are infrastructure errors,
        // never proof that a record was tampered with.
        let category = Self::failure_category(&out.stderr);
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
        checked(
            status,
            format!(
                "verifier exited {} (category {}): {}",
                out.code,
                category.as_deref().unwrap_or("unknown"),
                out.stderr.chars().take(300).collect::<String>()
            ),
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
            };
        };
        if let Some(expected) = &self.expected_anchor {
            if !expected.eq_ignore_ascii_case(anchor) {
                return ProofCheck {
                    status: IntegrityStatus::Invalid,
                    verifier: Some("operator-pinned anchor".into()),
                    verified_anchor: Some(anchor.clone()),
                    detail: Some(format!(
                        "record anchor does not match operator-pinned anchor {expected}"
                    )),
                };
            }
        }
        let key = (proof.clone(), witness.clone(), anchor.clone());
        if let Some(done) = self.verified.get(&key) {
            return done.clone();
        }
        let check = self.check_artifacts(proof, witness, anchor);
        self.verified.insert(key, check.clone());
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
