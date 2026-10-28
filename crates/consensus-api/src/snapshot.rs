// SPDX-License-Identifier: Apache-2.0
//! Sealed Hyphae snapshot: the public live mode's evidence (ADR-0008).
//!
//! The public server never reaches a Hyphae daemon. One authorized capture
//! session (the integrator's `capture-snapshot.sh`) seeds a purpose-built
//! Agent Memory instance with the seven synthetic `demo-repo` facts,
//! captures PROVEN results through the released `hyphae agent ui` operator
//! surface, verifies each with `hyphae proof verify`, writes a manifest and
//! destroys the instance's credentials. [`SnapshotMemoryClient`] answers
//! every recall from those artifacts, re-verifying the captured proofs at
//! boot ([`SnapshotMemoryClient::open`]) and, through the orchestrator's
//! verifier, in every run, with the same `hyphae proof verify` the live
//! adapter uses ([`crate::hyphae::HyphaeProofVerifier`]).
//!
//! # Artifact layout (the contract with the capture script)
//!
//! ```text
//! <snapshot dir>/                       name == manifest.snapshot_id
//!   manifest.json                       hyphae-consensus-snapshot-manifest-v1
//!   SHA256SUMS                          "<64 hex>  <relative path>" for every
//!                                       other file, manifest.json included
//!   captures/<id>.response.json         the agent ui result object, verbatim
//!   captures/<id>.proof                 bytes of the response's proof_path
//!   captures/<id>.witness               bytes of witness_path (PRIVATE)
//!   captures/<id>.verify.json           stdout of hyphae proof verify, at capture
//! ```
//!
//! Two captures are required and are the only ones the runtime serves: the
//! proven `list` of the served project (`query ""`, `limit 64`) and the
//! proven EMPTY `recall` of another project (its isolation evidence). Other
//! captures may be present; they are checked like the rest (digests,
//! ids) but never served or boot-verified. [`Snapshot::load`] checks the
//! whole directory structurally; every failure is a [`SnapshotError`] with
//! a stable code ([`codes`]) and a message that names the offending file
//! by its relative path and never quotes its contents.
//!
//! # What a recall returns
//!
//! A proof seals ITS OWN request, not the claim query that led the runtime
//! to a record. The snapshot client is therefore query-independent: every
//! recall of the served project returns the complete proven list set, in
//! capture order, whatever the query or `limit` (the limit is accepted and
//! ignored: the proven set is indivisible, and the orchestrator's round
//! context holds at most 20 records). Each record is an `EvidenceReference`
//! with `retrieval: sealed_snapshot`, the snapshot id and capture time, the
//! list capture's id as `proof_ref`, the proof's SHA-256, and the proof's
//! real request (`list(project, query "", limit 64)`) as `proof_request`,
//! so a receipt says "proof covers: complete record list of `<project>` at
//! anchor `<prefix>`", never "recall of `<claim>`". The recall carries the
//! proof itself, so the orchestrator records `RecallProven` with the
//! capture's full id list. A recall of the isolation project returns no
//! records and the empty capture's proof (`RecallProven` with `empty:
//! true`); a recall of any other project is a [`MemoryError::Backend`]
//! whose reason the orchestrator records as `EvidenceUnavailable` (an empty
//! `Recall` would record nothing at all).
//!
//! # Paths the verifier sees
//!
//! `proof_file` and `witness_file` are the artifact paths AS THE VERIFIER
//! BINARY SEES THEM. When `CONSENSUS_HYPHAE_BIN` runs Hyphae elsewhere (the
//! Windows dev machine's `scripts/hyphae-docker.cmd` runs it inside the
//! container), the artifacts are copied there too and
//! `CONSENSUS_SNAPSHOT_VERIFIER_DIR` names that directory; its paths are
//! joined with `/`. Without it the local directory is used. The verifier's
//! content-digest cache (HY-13) reads the artifacts through these paths, so
//! it works only when they are local: with a mapped directory every check
//! runs the verifier.
//!
//! # Limits of the check
//!
//! Boot and per-run verification prove that the proof and witness bytes on
//! disk are the ones Hyphae sealed for the pinned anchor and that the
//! recall re-executes to the sealed result. The record TEXTS come from the
//! captured response file, which is bound to the proof only by the manifest
//! and `SHA256SUMS` (both checked here), not byte-compared with the proof:
//! the verifier's own detail says so, and the honest label of ADR-0008
//! applies ("this proves the records are the snapshot's records, not that
//! they are true or that a model used them").

use crate::config::SnapshotConfig;
use crate::hyphae::{CommandRunner, HyphaeProofVerifier, VERIFIED_KIND, VERIFIED_SCOPE};
use consensus_domain::*;
use consensus_runtime::contract::quote;
use consensus_runtime::ports::{
    MemoryClient, MemoryError, MemoryStatus, ProofVerifier, Recall, RecallProof,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `manifest.json`'s `schema`.
pub const MANIFEST_SCHEMA: &str = "hyphae-consensus-snapshot-manifest-v1";
/// The `hyphae agent ui` envelope schema the captures were made with.
pub const CONTROL_SCHEMA: &str = "hyphae-omarchy-control-v1";
/// The `scope` of a memory proof in an `agent ui` response.
pub const PROOF_SCOPE: &str = "memory-snapshot-v1";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const CHECKSUMS_FILE: &str = "SHA256SUMS";
/// The `limit` of the required list capture (the surface's maximum).
pub const LIST_LIMIT: u32 = 64;
/// Largest file read from a snapshot directory (a witness is a full copy
/// of the store's data directory, about 3.3 MB for the seven records).
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Stable codes of [`SnapshotError`].
pub mod codes {
    /// The configured directory does not exist or is not a directory.
    pub const SNAPSHOT_DIR_MISSING: &str = "snapshot_dir_missing";
    /// `manifest.json` cannot be read.
    pub const MANIFEST_UNREADABLE: &str = "manifest_unreadable";
    /// `manifest.json` is not the documented JSON document (a field is
    /// missing, mistyped or out of range).
    pub const MANIFEST_INVALID: &str = "manifest_invalid";
    /// `manifest.schema` or `hyphae.control_schema` is not the one known.
    pub const MANIFEST_SCHEMA: &str = "manifest_schema";
    /// `snapshot_id` breaks `^[a-z0-9][a-z0-9-]{0,63}$`.
    pub const SNAPSHOT_ID_INVALID: &str = "snapshot_id_invalid";
    /// `snapshot_id` differs from the directory name.
    pub const SNAPSHOT_ID_MISMATCH: &str = "snapshot_id_mismatch";
    /// A timestamp is not RFC 3339 UTC.
    pub const TIMESTAMP_INVALID: &str = "timestamp_invalid";
    /// `anchor_hex` is not 64 lowercase hex digits.
    pub const ANCHOR_INVALID: &str = "anchor_invalid";
    /// A capture (manifest, response or verify record) names an anchor
    /// other than the manifest's single anchor.
    pub const ANCHOR_MISMATCH: &str = "anchor_mismatch";
    /// The operator-pinned anchor is not the manifest's.
    pub const PINNED_ANCHOR_MISMATCH: &str = "pinned_anchor_mismatch";
    /// A capture entry is malformed (id, operation, arguments, file names,
    /// digests, memory ids).
    pub const CAPTURE_INVALID: &str = "capture_invalid";
    /// A required capture is absent.
    pub const CAPTURE_MISSING: &str = "capture_missing";
    /// More than one capture could be the required one.
    pub const CAPTURE_AMBIGUOUS: &str = "capture_ambiguous";
    /// A file the manifest or `SHA256SUMS` names cannot be read.
    pub const FILE_UNREADABLE: &str = "file_unreadable";
    /// A file exceeds [`super::MAX_FILE_BYTES`].
    pub const FILE_TOO_LARGE: &str = "file_too_large";
    /// `SHA256SUMS` is missing or not in `sha256sum` format.
    pub const CHECKSUMS_INVALID: &str = "checksums_invalid";
    /// A file under the directory is not listed, or a listed file is absent.
    pub const CHECKSUMS_INCOMPLETE: &str = "checksums_incomplete";
    /// A file's SHA-256 differs from its `SHA256SUMS` line.
    pub const CHECKSUMS_MISMATCH: &str = "checksums_mismatch";
    /// A proof or witness differs from the manifest's digest of it.
    pub const DIGEST_MISMATCH: &str = "digest_mismatch";
    /// A response file is not the documented result object.
    pub const RESPONSE_INVALID: &str = "response_invalid";
    /// A capture's `memory_ids` are not the ids of its response, in order.
    pub const MEMORY_IDS_MISMATCH: &str = "memory_ids_mismatch";
    /// The list capture holds a record of another project.
    pub const FOREIGN_RECORD: &str = "foreign_record";
    /// `store` disagrees with the list response (count or snapshot).
    pub const STORE_MISMATCH: &str = "store_mismatch";
    /// A capture-time verify record does not say verified/memory/semantic
    /// re-execution at the manifest anchor.
    pub const VERIFY_RECORD_INVALID: &str = "verify_record_invalid";
    /// The verifier did not accept a required capture at boot.
    pub const BOOT_VERIFICATION_FAILED: &str = "boot_verification_failed";
}

/// Why a snapshot cannot be served. `message` names files by their path
/// relative to the snapshot directory and never includes their contents.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct SnapshotError {
    pub code: &'static str,
    pub message: String,
}

impl SnapshotError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

type Result<T> = std::result::Result<T, SnapshotError>;

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: String,
    pub snapshot_id: String,
    /// RFC 3339 UTC, seconds.
    pub captured_at: String,
    /// The one project the snapshot serves.
    pub project: String,
    /// The single anchor every capture's proof is bound to (64 lowercase hex).
    pub anchor_hex: String,
    pub hyphae: HyphaeBuild,
    pub store: StoreSummary,
    pub captures: Vec<Capture>,
}

/// The Hyphae build that produced the captures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyphaeBuild {
    /// Text of `hyphae --version`.
    pub version: String,
    /// SHA-256 of the binary, 64 lowercase hex.
    pub binary_sha256: String,
    pub control_schema: String,
}

/// The sealed store as the list response described it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSummary {
    pub record_count: u64,
    pub root_digest: String,
    pub visible_csn: u64,
    pub logical_time_micros: i64,
}

/// One captured `agent ui` operation and its artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capture {
    /// `^[a-z0-9][a-z0-9-]{0,63}$`; the public `proof_ref` of its records.
    pub id: String,
    /// `list` or `recall`.
    pub operation: String,
    pub arguments: CaptureArguments,
    pub response_file: String,
    pub proof_file: String,
    pub witness_file: String,
    pub verify_file: String,
    pub proof_blake3: String,
    pub proof_sha256: String,
    pub witness_sha256: String,
    /// Exactly the ids in the response, in response order.
    pub memory_ids: Vec<String>,
    pub verification: CaptureVerification,
}

/// The arguments of the captured operation (the request its proof seals).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureArguments {
    pub project: String,
    pub query: String,
    pub limit: u32,
    pub prove: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// What `hyphae proof verify` said at capture time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureVerification {
    pub verified_at: String,
    pub status: String,
    pub kind: String,
    pub scope: String,
    pub anchor_hex: String,
}

/// One record of a captured response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedMemory {
    /// Hyphae's memory id, a decimal string.
    pub id: String,
    pub project: Option<String>,
    pub scope: Option<String>,
    pub kind: Option<String>,
    pub text: String,
}

/// `snapshot` of a response: the store state the result was read at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResponseSnapshot {
    root_digest: String,
    visible_csn: u64,
    logical_time_micros: i64,
}

struct ParsedResponse {
    memories: Vec<CapturedMemory>,
    snapshot: Option<ResponseSnapshot>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `^[a-z0-9][a-z0-9-]{0,63}$`.
fn is_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A project string as Hyphae accepts it: short, printable ASCII.
fn is_project(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic())
}

/// A memory id as the pinned Hyphae prints it: a decimal string.
fn is_memory_id(value: &str) -> bool {
    (1..=40).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}

/// RFC 3339 in UTC, as the contract writes it: `YYYY-MM-DDTHH:MM:SSZ`
/// (chrono alone would also accept a space separator and an offset).
fn is_utc_timestamp(value: &str) -> bool {
    value.as_bytes().get(10) == Some(&b'T')
        && value.ends_with('Z')
        && chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

/// A path relative to the snapshot directory: `/`-separated components of
/// `[A-Za-z0-9._-]`, none empty, `.` or `..`.
fn is_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && component
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// Text as one bounded line: control characters become spaces.
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

/// The current time as the runtime records it.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// A loaded, structurally checked snapshot directory. Nothing here runs
/// the verifier: see [`SnapshotMemoryClient::open`].
#[derive(Debug, Clone)]
pub struct Snapshot {
    dir: PathBuf,
    verifier_dir: Option<String>,
    pub manifest: Manifest,
    list: usize,
    isolation: usize,
    records: Vec<CapturedMemory>,
}

impl Snapshot {
    /// Loads and checks `dir` (see the module doc for every check). With
    /// `verifier_dir`, artifact paths handed to the verifier are joined
    /// under it with `/` instead of under `dir`.
    pub fn load(dir: &Path, verifier_dir: Option<&str>) -> Result<Self> {
        if !dir.is_dir() {
            return Err(SnapshotError::new(
                codes::SNAPSHOT_DIR_MISSING,
                "the snapshot directory does not exist or is not a directory",
            ));
        }
        let verifier_dir = verifier_dir
            .map(|v| v.trim().trim_end_matches(['/', '\\']).to_string())
            .filter(|v| !v.is_empty());
        // The listing first: an altered file (the manifest included) is
        // reported as such, not as whatever parse error it then causes.
        check_checksums(dir)?;
        let manifest_bytes = read_file(dir, MANIFEST_FILE).map_err(|e| {
            SnapshotError::new(
                codes::MANIFEST_UNREADABLE,
                format!("{MANIFEST_FILE}: {}", e.message),
            )
        })?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| {
            SnapshotError::new(
                codes::MANIFEST_INVALID,
                format!("{MANIFEST_FILE}: {}", one_line(&e.to_string(), 200)),
            )
        })?;
        check_manifest(&manifest, dir)?;

        let mut responses = Vec::with_capacity(manifest.captures.len());
        for capture in &manifest.captures {
            responses.push(check_capture(dir, &manifest, capture)?);
        }

        // The required list capture of the served project.
        let list = single(
            &manifest,
            "the proven list of the served project (operation list, query \"\", limit 64)",
            |c| {
                c.operation == "list"
                    && c.arguments.project == manifest.project
                    && c.arguments.query.is_empty()
                    && c.arguments.limit == LIST_LIMIT
            },
        )?;
        let list_capture = &manifest.captures[list];
        let ParsedResponse { memories, snapshot } = &responses[list];
        if memories.is_empty() {
            return Err(SnapshotError::new(
                codes::CAPTURE_INVALID,
                format!(
                    "capture {}: the list of the served project holds no records",
                    list_capture.id
                ),
            ));
        }
        for memory in memories {
            let attributed = memory
                .project
                .as_deref()
                .is_some_and(|p| p == manifest.project || p == crate::hyphae::GLOBAL_PROJECT);
            if !attributed {
                return Err(SnapshotError::new(
                    codes::FOREIGN_RECORD,
                    format!(
                        "{}: record {} is attributed to {}, neither {} nor _global",
                        list_capture.response_file,
                        quote(&memory.id),
                        memory
                            .project
                            .as_deref()
                            .map(quote)
                            .unwrap_or_else(|| "no project".into()),
                        quote(&manifest.project)
                    ),
                ));
            }
        }
        if memories.len() as u64 != manifest.store.record_count {
            return Err(SnapshotError::new(
                codes::STORE_MISMATCH,
                format!(
                    "store.record_count is {} but {} holds {} records",
                    manifest.store.record_count,
                    list_capture.response_file,
                    memories.len()
                ),
            ));
        }
        let Some(snapshot) = snapshot else {
            return Err(SnapshotError::new(
                codes::RESPONSE_INVALID,
                format!(
                    "{}: no snapshot object (visible_csn, root_digest, logical_time_micros)",
                    list_capture.response_file
                ),
            ));
        };
        let expected = ResponseSnapshot {
            root_digest: manifest.store.root_digest.clone(),
            visible_csn: manifest.store.visible_csn,
            logical_time_micros: manifest.store.logical_time_micros,
        };
        if *snapshot != expected {
            return Err(SnapshotError::new(
                codes::STORE_MISMATCH,
                format!(
                    "store (root_digest, visible_csn, logical_time_micros) differs from the snapshot object of {}",
                    list_capture.response_file
                ),
            ));
        }

        // The required empty proven recall of another project.
        let isolation = single(
            &manifest,
            "the proven empty recall of another project (operation recall, no memory ids)",
            |c| {
                c.operation == "recall"
                    && c.arguments.project != manifest.project
                    && c.arguments.project != crate::hyphae::GLOBAL_PROJECT
                    && c.memory_ids.is_empty()
            },
        )?;

        let records = memories.clone();
        Ok(Self {
            dir: dir.to_path_buf(),
            verifier_dir,
            manifest,
            list,
            isolation,
            records,
        })
    }

    /// [`load`](Self::load) with the configured directories.
    pub fn load_from_config(cfg: &SnapshotConfig) -> Result<Self> {
        Self::load(&cfg.dir, cfg.verifier_dir.as_deref())
    }

    /// The proven list of the served project.
    pub fn list_capture(&self) -> &Capture {
        &self.manifest.captures[self.list]
    }

    /// The proven empty recall of another project.
    pub fn isolation_capture(&self) -> &Capture {
        &self.manifest.captures[self.isolation]
    }

    /// The list capture's records, in response order.
    pub fn records(&self) -> &[CapturedMemory] {
        &self.records
    }

    /// An artifact's path as the verifier binary sees it.
    pub fn artifact_path(&self, relative: &str) -> String {
        match &self.verifier_dir {
            Some(base) => format!("{base}/{relative}"),
            None => local_path(&self.dir, relative)
                .to_string_lossy()
                .into_owned(),
        }
    }

    /// The request a capture's proof seals.
    pub fn proof_request(capture: &Capture) -> ProofRequest {
        ProofRequest {
            operation: capture.operation.clone(),
            project: capture.arguments.project.clone(),
            query: Some(capture.arguments.query.clone()),
            limit: Some(capture.arguments.limit),
        }
    }
}

/// The index of the one capture matching `pick`.
fn single(manifest: &Manifest, what: &str, pick: impl Fn(&Capture) -> bool) -> Result<usize> {
    let mut found: Vec<usize> = manifest
        .captures
        .iter()
        .enumerate()
        .filter(|(_, c)| pick(c))
        .map(|(i, _)| i)
        .collect();
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(SnapshotError::new(
            codes::CAPTURE_MISSING,
            format!("no capture is {what}"),
        )),
        _ => Err(SnapshotError::new(
            codes::CAPTURE_AMBIGUOUS,
            format!(
                "{} captures are {what}: {}",
                found.len(),
                found
                    .iter()
                    .map(|i| manifest.captures[*i].id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn local_path(dir: &Path, relative: &str) -> PathBuf {
    let mut path = dir.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
    }
    path
}

/// The bytes of a file under the snapshot directory, bounded.
fn read_file(dir: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = local_path(dir, relative);
    let size = std::fs::metadata(&path)
        .map_err(|e| {
            SnapshotError::new(codes::FILE_UNREADABLE, format!("{relative}: {}", e.kind()))
        })?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(SnapshotError::new(
            codes::FILE_TOO_LARGE,
            format!("{relative}: {size} bytes, more than {MAX_FILE_BYTES}"),
        ));
    }
    std::fs::read(&path).map_err(|e| {
        SnapshotError::new(codes::FILE_UNREADABLE, format!("{relative}: {}", e.kind()))
    })
}

fn check_timestamp(value: &str, what: &str) -> Result<()> {
    if is_utc_timestamp(value) {
        Ok(())
    } else {
        Err(SnapshotError::new(
            codes::TIMESTAMP_INVALID,
            format!("{what} is not an RFC 3339 UTC timestamp"),
        ))
    }
}

fn check_hex64(value: &str, code: &'static str, what: &str) -> Result<()> {
    if is_hex64(value) {
        Ok(())
    } else {
        Err(SnapshotError::new(
            code,
            format!("{what} is not 64 lowercase hex digits"),
        ))
    }
}

fn check_manifest(manifest: &Manifest, dir: &Path) -> Result<()> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(SnapshotError::new(
            codes::MANIFEST_SCHEMA,
            format!("{MANIFEST_FILE}: schema is not {MANIFEST_SCHEMA}"),
        ));
    }
    if !is_identifier(&manifest.snapshot_id) {
        return Err(SnapshotError::new(
            codes::SNAPSHOT_ID_INVALID,
            "snapshot_id must match ^[a-z0-9][a-z0-9-]{0,63}$",
        ));
    }
    let dir_name = std::fs::canonicalize(dir)
        .unwrap_or_else(|_| dir.to_path_buf())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if dir_name != manifest.snapshot_id {
        return Err(SnapshotError::new(
            codes::SNAPSHOT_ID_MISMATCH,
            format!(
                "snapshot_id {} differs from the directory name {}",
                quote(&manifest.snapshot_id),
                quote(&dir_name)
            ),
        ));
    }
    check_timestamp(&manifest.captured_at, "captured_at")?;
    if !is_project(&manifest.project) {
        return Err(SnapshotError::new(
            codes::MANIFEST_INVALID,
            "project must be 1 to 128 printable ASCII characters",
        ));
    }
    check_hex64(&manifest.anchor_hex, codes::ANCHOR_INVALID, "anchor_hex")?;
    if manifest.hyphae.control_schema != CONTROL_SCHEMA {
        return Err(SnapshotError::new(
            codes::MANIFEST_SCHEMA,
            format!("hyphae.control_schema is not {CONTROL_SCHEMA}"),
        ));
    }
    check_hex64(
        &manifest.hyphae.binary_sha256,
        codes::MANIFEST_INVALID,
        "hyphae.binary_sha256",
    )?;
    let version = manifest.hyphae.version.trim();
    if version.is_empty() || version.len() > 200 {
        return Err(SnapshotError::new(
            codes::MANIFEST_INVALID,
            "hyphae.version must be 1 to 200 characters",
        ));
    }
    check_hex64(
        &manifest.store.root_digest,
        codes::MANIFEST_INVALID,
        "store.root_digest",
    )?;
    if manifest.captures.is_empty() {
        return Err(SnapshotError::new(
            codes::CAPTURE_MISSING,
            "captures is empty",
        ));
    }
    let mut ids = BTreeSet::new();
    for capture in &manifest.captures {
        check_capture_entry(manifest, capture)?;
        if !ids.insert(capture.id.as_str()) {
            return Err(SnapshotError::new(
                codes::CAPTURE_INVALID,
                format!("capture id {} is used twice", capture.id),
            ));
        }
    }
    Ok(())
}

/// The manifest entry of one capture, before any file is read.
fn check_capture_entry(manifest: &Manifest, capture: &Capture) -> Result<()> {
    let invalid = |message: String| SnapshotError::new(codes::CAPTURE_INVALID, message);
    if !is_identifier(&capture.id) {
        return Err(invalid(
            "a capture id does not match ^[a-z0-9][a-z0-9-]{0,63}$".into(),
        ));
    }
    let id = &capture.id;
    if capture.operation != "list" && capture.operation != "recall" {
        return Err(invalid(format!(
            "capture {id}: operation must be list or recall"
        )));
    }
    if !is_project(&capture.arguments.project) {
        return Err(invalid(format!(
            "capture {id}: arguments.project must be 1 to 128 printable ASCII characters"
        )));
    }
    if !(1..=LIST_LIMIT).contains(&capture.arguments.limit) {
        return Err(invalid(format!(
            "capture {id}: arguments.limit must be from 1 to {LIST_LIMIT}"
        )));
    }
    if !capture.arguments.prove {
        return Err(invalid(format!(
            "capture {id}: arguments.prove must be true (every capture is proven)"
        )));
    }
    for (field, value, suffix) in [
        ("response_file", &capture.response_file, ".response.json"),
        ("proof_file", &capture.proof_file, ".proof"),
        ("witness_file", &capture.witness_file, ".witness"),
        ("verify_file", &capture.verify_file, ".verify.json"),
    ] {
        let expected = format!("captures/{id}{suffix}");
        if *value != expected {
            return Err(invalid(format!("capture {id}: {field} must be {expected}")));
        }
    }
    for (field, value) in [
        ("proof_blake3", &capture.proof_blake3),
        ("proof_sha256", &capture.proof_sha256),
        ("witness_sha256", &capture.witness_sha256),
    ] {
        check_hex64(
            value,
            codes::CAPTURE_INVALID,
            &format!("capture {id}: {field}"),
        )?;
    }
    let mut seen = BTreeSet::new();
    for memory_id in &capture.memory_ids {
        if !is_memory_id(memory_id) {
            return Err(invalid(format!(
                "capture {id}: a memory id is not a decimal string"
            )));
        }
        if !seen.insert(memory_id.as_str()) {
            return Err(invalid(format!(
                "capture {id}: memory id {memory_id} is listed twice"
            )));
        }
    }
    let v = &capture.verification;
    check_timestamp(
        &v.verified_at,
        &format!("capture {id}: verification.verified_at"),
    )?;
    if v.status != "verified" || v.kind != VERIFIED_KIND || v.scope != VERIFIED_SCOPE {
        return Err(SnapshotError::new(
            codes::VERIFY_RECORD_INVALID,
            format!(
                "capture {id}: verification must say status verified, kind {VERIFIED_KIND}, scope {VERIFIED_SCOPE}"
            ),
        ));
    }
    if v.anchor_hex != manifest.anchor_hex {
        return Err(SnapshotError::new(
            codes::ANCHOR_MISMATCH,
            format!("capture {id}: verification.anchor_hex is not the manifest anchor"),
        ));
    }
    Ok(())
}

/// Every file under `dir`, as `/`-separated paths relative to it, sorted.
fn walk(dir: &Path) -> Result<Vec<String>> {
    fn visit(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
        let entries = std::fs::read_dir(dir).map_err(|e| {
            SnapshotError::new(
                codes::FILE_UNREADABLE,
                format!(
                    "{}: {}",
                    if prefix.is_empty() { "." } else { prefix },
                    e.kind()
                ),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                SnapshotError::new(codes::FILE_UNREADABLE, format!("{prefix}: {}", e.kind()))
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            let meta = std::fs::metadata(&path).map_err(|e| {
                SnapshotError::new(codes::FILE_UNREADABLE, format!("{relative}: {}", e.kind()))
            })?;
            if meta.is_dir() {
                visit(&path, &relative, out)?;
            } else {
                out.push(relative);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(dir, "", &mut out)?;
    out.sort();
    Ok(out)
}

/// `SHA256SUMS` lines as `(digest, relative path)`.
fn parse_checksums(text: &str) -> Result<Vec<(String, String)>> {
    let invalid = |message: String| SnapshotError::new(codes::CHECKSUMS_INVALID, message);
    let mut lines = Vec::new();
    let mut listed = BTreeSet::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let number = number + 1;
        let (digest, rest) = match line.len() > 66 {
            true => line.split_at(64),
            false => {
                return Err(invalid(format!(
                    "{CHECKSUMS_FILE} line {number}: not \"<64 hex>  <relative path>\""
                )))
            }
        };
        let path = match rest.strip_prefix("  ").or_else(|| rest.strip_prefix(" *")) {
            Some(path) if is_hex64(digest) && is_relative_path(path) => path,
            _ => {
                return Err(invalid(format!(
                    "{CHECKSUMS_FILE} line {number}: not \"<64 hex>  <relative path>\""
                )))
            }
        };
        if path == CHECKSUMS_FILE {
            return Err(invalid(format!(
                "{CHECKSUMS_FILE} line {number}: lists {CHECKSUMS_FILE} itself"
            )));
        }
        if !listed.insert(path.to_string()) {
            return Err(invalid(format!(
                "{CHECKSUMS_FILE} line {number}: {path} is listed twice"
            )));
        }
        lines.push((digest.to_string(), path.to_string()));
    }
    if lines.is_empty() {
        return Err(invalid(format!("{CHECKSUMS_FILE} lists no file")));
    }
    Ok(lines)
}

/// `SHA256SUMS` covers every file under the directory, and every digest
/// matches.
fn check_checksums(dir: &Path) -> Result<()> {
    let bytes = read_file(dir, CHECKSUMS_FILE)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        SnapshotError::new(
            codes::CHECKSUMS_INVALID,
            format!("{CHECKSUMS_FILE} is not UTF-8 text"),
        )
    })?;
    let lines = parse_checksums(text)?;
    let listed: BTreeSet<&str> = lines.iter().map(|(_, p)| p.as_str()).collect();
    let present: Vec<String> = walk(dir)?;
    for file in &present {
        if file != CHECKSUMS_FILE && !listed.contains(file.as_str()) {
            return Err(SnapshotError::new(
                codes::CHECKSUMS_INCOMPLETE,
                format!("{file} is not listed in {CHECKSUMS_FILE}"),
            ));
        }
    }
    for (digest, path) in &lines {
        let bytes = read_file(dir, path).map_err(|e| match e.code {
            codes::FILE_UNREADABLE => SnapshotError::new(
                codes::CHECKSUMS_INCOMPLETE,
                format!("{path} is listed in {CHECKSUMS_FILE} but cannot be read"),
            ),
            _ => e,
        })?;
        if sha256_hex(&bytes) != *digest {
            return Err(SnapshotError::new(
                codes::CHECKSUMS_MISMATCH,
                format!("{path}: SHA-256 differs from its {CHECKSUMS_FILE} line"),
            ));
        }
    }
    Ok(())
}

fn string_field(object: &Value, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

/// One capture's files against its manifest entry.
fn check_capture(dir: &Path, manifest: &Manifest, capture: &Capture) -> Result<ParsedResponse> {
    let id = &capture.id;
    for (file, expected) in [
        (&capture.proof_file, &capture.proof_sha256),
        (&capture.witness_file, &capture.witness_sha256),
    ] {
        let bytes = read_file(dir, file)?;
        if sha256_hex(&bytes) != *expected {
            return Err(SnapshotError::new(
                codes::DIGEST_MISMATCH,
                format!("{file}: SHA-256 differs from the manifest entry of capture {id}"),
            ));
        }
    }

    let response_file = &capture.response_file;
    let response: Value = serde_json::from_slice(&read_file(dir, response_file)?).map_err(|e| {
        SnapshotError::new(
            codes::RESPONSE_INVALID,
            format!("{response_file}: {}", one_line(&e.to_string(), 200)),
        )
    })?;
    let invalid = |message: &str| {
        SnapshotError::new(
            codes::RESPONSE_INVALID,
            format!("{response_file}: {message}"),
        )
    };
    if !response.is_object() {
        return Err(invalid("not a JSON object"));
    }
    let items = response
        .get("memories")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("no memories array"))?;
    let mut memories = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if !item.is_object() {
            return Err(invalid(&format!("memories[{index}] is not an object")));
        }
        let memory_id = string_field(item, "id")
            .filter(|v| is_memory_id(v))
            .ok_or_else(|| invalid(&format!("memories[{index}].id is not a decimal string")))?;
        let text = string_field(item, "text")
            .ok_or_else(|| invalid(&format!("memories[{index}].text is not a string")))?;
        memories.push(CapturedMemory {
            id: memory_id,
            project: string_field(item, "project"),
            scope: string_field(item, "scope"),
            kind: string_field(item, "kind"),
            text,
        });
    }
    let ids: Vec<&str> = memories.iter().map(|m| m.id.as_str()).collect();
    let listed: Vec<&str> = capture.memory_ids.iter().map(String::as_str).collect();
    if ids != listed {
        return Err(SnapshotError::new(
            codes::MEMORY_IDS_MISMATCH,
            format!(
                "capture {id}: memory_ids ({} ids) are not the ids of {response_file} ({} records) in order",
                listed.len(),
                ids.len()
            ),
        ));
    }

    let proof = response
        .get("proof")
        .filter(|p| p.is_object())
        .ok_or_else(|| invalid("no proof object"))?;
    if string_field(proof, "anchor_hex").as_deref() != Some(manifest.anchor_hex.as_str()) {
        return Err(SnapshotError::new(
            codes::ANCHOR_MISMATCH,
            format!("{response_file}: proof.anchor_hex is not the manifest anchor"),
        ));
    }
    if string_field(proof, "proof_blake3").as_deref() != Some(capture.proof_blake3.as_str()) {
        return Err(invalid(&format!(
            "proof.proof_blake3 differs from the manifest entry of capture {id}"
        )));
    }
    if string_field(proof, "scope").as_deref() != Some(PROOF_SCOPE) {
        return Err(invalid(&format!("proof.scope is not {PROOF_SCOPE}")));
    }
    let snapshot = match response.get("snapshot") {
        None | Some(Value::Null) => None,
        Some(snapshot) => Some(ResponseSnapshot {
            root_digest: string_field(snapshot, "root_digest")
                .filter(|d| is_hex64(d))
                .ok_or_else(|| invalid("snapshot.root_digest is not 64 lowercase hex digits"))?,
            visible_csn: snapshot
                .get("visible_csn")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("snapshot.visible_csn is not an integer"))?,
            logical_time_micros: snapshot
                .get("logical_time_micros")
                .and_then(Value::as_i64)
                .ok_or_else(|| invalid("snapshot.logical_time_micros is not an integer"))?,
        }),
    };

    let verify_file = &capture.verify_file;
    let verify: Value = serde_json::from_slice(&read_file(dir, verify_file)?).map_err(|e| {
        SnapshotError::new(
            codes::VERIFY_RECORD_INVALID,
            format!("{verify_file}: {}", one_line(&e.to_string(), 200)),
        )
    })?;
    let said = |key: &str| string_field(&verify, key);
    let reexecuted = verify
        .get("semantic_reexecution_performed")
        .and_then(Value::as_bool)
        == Some(true);
    if said("status").as_deref() != Some("verified")
        || said("kind").as_deref() != Some(VERIFIED_KIND)
        || said("scope").as_deref() != Some(VERIFIED_SCOPE)
        || !reexecuted
    {
        return Err(SnapshotError::new(
            codes::VERIFY_RECORD_INVALID,
            format!(
                "{verify_file}: not a verified {VERIFIED_KIND} proof report with {VERIFIED_SCOPE} performed"
            ),
        ));
    }
    if said("anchor_digest").as_deref() != Some(manifest.anchor_hex.as_str()) {
        return Err(SnapshotError::new(
            codes::ANCHOR_MISMATCH,
            format!("{verify_file}: anchor_digest is not the manifest anchor"),
        ));
    }
    Ok(ParsedResponse { memories, snapshot })
}

/// A `hyphae proof verify` verifier as the snapshot configuration sets it:
/// binary, pinned anchor (the manifest's, checked by
/// [`SnapshotMemoryClient::open`]) and deadline.
pub fn verifier_from_config<R: CommandRunner>(
    cfg: &SnapshotConfig,
    runner: R,
) -> HyphaeProofVerifier<R> {
    let mut verifier = HyphaeProofVerifier::new(cfg.binary.clone(), runner);
    verifier.expected_anchor = Some(cfg.expected_anchor.clone());
    verifier.timeout = cfg.verify_timeout;
    verifier
}

/// What `/health` and `/ready` may say about the served snapshot: plain
/// data, no path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotStatus {
    pub snapshot_id: String,
    /// The one project recalls are served for.
    pub project: String,
    /// The project whose captured empty recall is the isolation evidence.
    pub isolation_project: String,
    pub captured_at: String,
    pub anchor_hex: String,
    pub record_count: usize,
    pub hyphae_version: String,
    /// SHA-256 of the Hyphae binary that produced the captures (published
    /// out of band; not compared with the verifier binary here).
    pub hyphae_binary_sha256: String,
    /// When this process verified the required captures (RFC 3339 UTC).
    pub verified_at: String,
    pub captures: Vec<CaptureStatus>,
}

/// The boot verification of one served capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStatus {
    pub id: String,
    pub operation: String,
    pub project: String,
    pub record_count: usize,
    pub integrity: IntegrityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
    /// The verifier's structured report (status, scope, kind, digests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<VerifierReport>,
}

impl SnapshotStatus {
    /// One line for `MemoryStatus::detail`.
    pub fn summary(&self) -> String {
        format!(
            "sealed snapshot {}: project {}, {} records, anchor {}, captured {}, proofs re-verified at boot {} ({} captures valid)",
            self.snapshot_id,
            quote(&self.project),
            self.record_count,
            self.anchor_hex,
            self.captured_at,
            self.verified_at,
            self.captures
                .iter()
                .filter(|c| c.integrity == IntegrityStatus::Valid)
                .count()
        )
    }
}

/// Read-only memory client over a sealed snapshot. Plain data: `Clone`,
/// `Send` and `Sync`, so a server can load one and hand a copy to each
/// run.
#[derive(Clone)]
pub struct SnapshotMemoryClient {
    status: SnapshotStatus,
    project: ProjectId,
    isolation_project: ProjectId,
    records: Vec<EvidenceReference>,
    list_proof: RecallProof,
    isolation_proof: RecallProof,
    now: Arc<dyn Fn() -> String + Send + Sync>,
}

impl std::fmt::Debug for SnapshotMemoryClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotMemoryClient")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl SnapshotMemoryClient {
    /// Loads the configured snapshot and verifies its two required captures
    /// with `verifier` (the same `hyphae proof verify` the runs use). Boot
    /// fails when the pinned anchor is not the manifest's, or when either
    /// capture is not `Valid` as a `memory` proof re-executed at scope
    /// `semantic_reexecution` against that anchor.
    pub fn open<V: ProofVerifier + ?Sized>(cfg: &SnapshotConfig, verifier: &mut V) -> Result<Self> {
        let snapshot = Snapshot::load_from_config(cfg)?;
        Self::from_snapshot(snapshot, &cfg.expected_anchor, verifier)
    }

    /// [`open`](Self::open) over an already loaded snapshot.
    pub fn from_snapshot<V: ProofVerifier + ?Sized>(
        snapshot: Snapshot,
        expected_anchor: &str,
        verifier: &mut V,
    ) -> Result<Self> {
        let anchor = snapshot.manifest.anchor_hex.clone();
        if expected_anchor.trim().to_ascii_lowercase() != anchor {
            return Err(SnapshotError::new(
                codes::PINNED_ANCHOR_MISMATCH,
                format!(
                    "CONSENSUS_HYPHAE_EXPECTED_ANCHOR is not the manifest anchor {anchor} of snapshot {}",
                    snapshot.manifest.snapshot_id
                ),
            ));
        }
        let mut captures = Vec::with_capacity(2);
        for capture in [snapshot.list_capture(), snapshot.isolation_capture()] {
            captures.push(verify_capture(&snapshot, capture, verifier)?);
        }
        let manifest = &snapshot.manifest;
        let list = snapshot.list_capture();
        let isolation = snapshot.isolation_capture();
        let list_request = Snapshot::proof_request(list);
        let project = ProjectId::new(manifest.project.clone());
        let records: Vec<EvidenceReference> = snapshot
            .records()
            .iter()
            .map(|memory| {
                let project_id = match memory.project.as_deref() {
                    Some(crate::hyphae::GLOBAL_PROJECT) => {
                        ProjectId::new(crate::hyphae::GLOBAL_PROJECT)
                    }
                    _ => project.clone(),
                };
                EvidenceReference {
                    schema_version: SCHEMA_VERSION.into(),
                    id: EvidenceId::new(memory.id.clone()),
                    project_id,
                    backend_ref: Some(format!("hyphae-memory:{}", memory.id)),
                    kind: memory.kind.clone().unwrap_or_else(|| "note".into()),
                    content: memory.text.clone(),
                    // Filled in per recall.
                    retrieved_at: String::new(),
                    anchor: Some(anchor.clone()),
                    proof_file: Some(snapshot.artifact_path(&list.proof_file)),
                    witness_file: Some(snapshot.artifact_path(&list.witness_file)),
                    proof_error: None,
                    retrieval: Some(Retrieval::SealedSnapshot),
                    snapshot_id: Some(manifest.snapshot_id.clone()),
                    captured_at: Some(manifest.captured_at.clone()),
                    proof_ref: Some(list.id.clone()),
                    proof_sha256: Some(list.proof_sha256.clone()),
                    proof_request: Some(list_request.clone()),
                    backend_project: memory.project.clone(),
                    backend_scope: memory.scope.clone(),
                }
            })
            .collect();
        let status = SnapshotStatus {
            snapshot_id: manifest.snapshot_id.clone(),
            project: manifest.project.clone(),
            isolation_project: isolation.arguments.project.clone(),
            captured_at: manifest.captured_at.clone(),
            anchor_hex: anchor.clone(),
            record_count: records.len(),
            hyphae_version: manifest.hyphae.version.trim().to_string(),
            hyphae_binary_sha256: manifest.hyphae.binary_sha256.clone(),
            verified_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            captures,
        };
        Ok(Self {
            isolation_project: ProjectId::new(isolation.arguments.project.clone()),
            list_proof: RecallProof {
                request: list_request,
                evidence_ids: records.iter().map(|r| r.id.clone()).collect(),
                proof_ref: list.id.clone(),
                anchor: anchor.clone(),
            },
            isolation_proof: RecallProof {
                request: Snapshot::proof_request(isolation),
                evidence_ids: vec![],
                proof_ref: isolation.id.clone(),
                anchor,
            },
            status,
            project,
            records,
            now: Arc::new(now_rfc3339),
        })
    }

    /// Uses `now` for each record's `retrieved_at` (tests).
    pub fn with_time_source(mut self, now: impl Fn() -> String + Send + Sync + 'static) -> Self {
        self.now = Arc::new(now);
        self
    }

    /// What the boot verification established.
    pub fn snapshot_status(&self) -> &SnapshotStatus {
        &self.status
    }

    /// The served records (templates: `retrieved_at` is set per recall).
    pub fn records(&self) -> &[EvidenceReference] {
        &self.records
    }

    pub fn project(&self) -> &ProjectId {
        &self.project
    }

    pub fn isolation_project(&self) -> &ProjectId {
        &self.isolation_project
    }
}

/// Runs the verifier on one capture's artifacts and requires `Valid` as a
/// memory proof re-executed against the manifest anchor.
fn verify_capture<V: ProofVerifier + ?Sized>(
    snapshot: &Snapshot,
    capture: &Capture,
    verifier: &mut V,
) -> Result<CaptureStatus> {
    let anchor = &snapshot.manifest.anchor_hex;
    let probe = EvidenceReference {
        schema_version: SCHEMA_VERSION.into(),
        id: EvidenceId::new(capture.id.clone()),
        project_id: ProjectId::new(capture.arguments.project.clone()),
        backend_ref: None,
        kind: "capture".into(),
        content: String::new(),
        retrieved_at: now_rfc3339(),
        anchor: Some(anchor.clone()),
        proof_file: Some(snapshot.artifact_path(&capture.proof_file)),
        witness_file: Some(snapshot.artifact_path(&capture.witness_file)),
        proof_error: None,
        retrieval: Some(Retrieval::SealedSnapshot),
        snapshot_id: Some(snapshot.manifest.snapshot_id.clone()),
        captured_at: Some(snapshot.manifest.captured_at.clone()),
        proof_ref: Some(capture.id.clone()),
        proof_sha256: Some(capture.proof_sha256.clone()),
        proof_request: Some(Snapshot::proof_request(capture)),
        backend_project: None,
        backend_scope: None,
    };
    let check = verifier.verify(&probe);
    let report = check.report.clone();
    let kind = report.as_ref().and_then(|r| r.kind.as_deref());
    let scope = report.as_ref().and_then(|r| r.scope.as_deref());
    let verified_anchor = check
        .verified_anchor
        .as_deref()
        .map(|a| a.trim().to_ascii_lowercase());
    let accepted = check.status == IntegrityStatus::Valid
        && kind == Some(VERIFIED_KIND)
        && scope == Some(VERIFIED_SCOPE)
        && verified_anchor.as_deref() == Some(anchor.as_str());
    if !accepted {
        return Err(SnapshotError::new(
            codes::BOOT_VERIFICATION_FAILED,
            format!(
                "capture {}: the verifier reported {:?} (kind {}, scope {}): {}",
                capture.id,
                check.status,
                kind.unwrap_or("?"),
                scope.unwrap_or("?"),
                one_line(check.detail.as_deref().unwrap_or("no detail"), 300)
            ),
        ));
    }
    Ok(CaptureStatus {
        id: capture.id.clone(),
        operation: capture.operation.clone(),
        project: capture.arguments.project.clone(),
        record_count: capture.memory_ids.len(),
        integrity: check.status,
        verifier: check.verifier,
        report,
    })
}

impl MemoryClient for SnapshotMemoryClient {
    fn recall(
        &mut self,
        project: &ProjectId,
        query: &str,
        limit: usize,
    ) -> std::result::Result<Vec<EvidenceReference>, MemoryError> {
        self.recall_with_proof(project, query, limit)
            .map(|recall| recall.records)
    }

    /// Query-independent: the served project gets the complete proven list
    /// set in capture order (`limit` is accepted and ignored; the proven
    /// set is indivisible), the isolation project its proven empty result,
    /// any other project a `Backend` error with the reason.
    fn recall_with_proof(
        &mut self,
        project: &ProjectId,
        _query: &str,
        _limit: usize,
    ) -> std::result::Result<Recall, MemoryError> {
        if *project == self.project {
            let retrieved_at = (self.now)();
            let records = self
                .records
                .iter()
                .cloned()
                .map(|mut record| {
                    record.retrieved_at = retrieved_at.clone();
                    record
                })
                .collect();
            return Ok(Recall {
                records,
                proof: Some(self.list_proof.clone()),
                proof_failure: None,
                foreign: vec![],
            });
        }
        if *project == self.isolation_project {
            return Ok(Recall {
                proof: Some(self.isolation_proof.clone()),
                ..Recall::default()
            });
        }
        Err(MemoryError::Backend(format!(
            "sealed snapshot {} serves project {} only (its captured empty recall covers {}); project {} was not captured, so it has no records here",
            self.status.snapshot_id,
            quote(self.project.as_str()),
            quote(self.isolation_project.as_str()),
            quote(project.as_str())
        )))
    }

    fn status(&mut self) -> std::result::Result<MemoryStatus, MemoryError> {
        Ok(MemoryStatus {
            healthy: true,
            detail: self.status.summary(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_digests_and_paths_are_checked_strictly() {
        assert!(is_identifier("snap-2026-10-29-demo-repo"));
        assert!(is_identifier("a") && is_identifier(&"a".repeat(64)));
        for bad in ["", "-x", "Snap", "a_b", "a b", &"a".repeat(65)] {
            assert!(!is_identifier(bad), "{bad:?}");
        }
        assert!(is_hex64(&"ab".repeat(32)));
        assert!(!is_hex64(&"AB".repeat(32)), "lowercase only");
        assert!(!is_hex64(&"ab".repeat(31)));
        assert!(is_memory_id("186670067796945807561772405393844769735"));
        assert!(!is_memory_id("mem-1") && !is_memory_id(""));
        assert!(is_relative_path("captures/list-demo-repo.proof"));
        for bad in [
            "",
            "/abs",
            "a//b",
            "../x",
            "a/./b",
            "a\\b",
            "a b",
            "captures/",
        ] {
            assert!(!is_relative_path(bad), "{bad:?}");
        }
        assert!(is_utc_timestamp("2026-10-29T15:04:05Z"));
        assert!(
            !is_utc_timestamp("2026-10-29T15:04:05+00:00"),
            "UTC is written Z"
        );
        assert!(!is_utc_timestamp("2026-10-29 15:04:05Z"));
    }

    #[test]
    fn checksum_lines_follow_the_sha256sum_format() {
        let digest = "ab".repeat(32);
        let text = format!("{digest}  manifest.json\n{digest} *captures/a.proof\r\n\n");
        let lines = parse_checksums(&text).unwrap();
        assert_eq!(
            lines,
            [
                (digest.clone(), "manifest.json".to_string()),
                (digest.clone(), "captures/a.proof".to_string())
            ]
        );
        for bad in [
            String::new(),
            format!("{digest} manifest.json"),
            format!("{digest}  /etc/passwd"),
            format!("{digest}  SHA256SUMS"),
            format!("{digest}  a.json\n{digest}  a.json"),
            format!("{}  a.json", "zz".repeat(32)),
        ] {
            let err = parse_checksums(&bad).unwrap_err();
            assert_eq!(err.code, codes::CHECKSUMS_INVALID, "{bad:?}");
        }
    }

    #[test]
    fn errors_print_their_code_first() {
        let err = SnapshotError::new(codes::DIGEST_MISMATCH, "captures/x.proof: differs");
        assert_eq!(
            err.to_string(),
            "digest_mismatch: captures/x.proof: differs"
        );
    }
}
