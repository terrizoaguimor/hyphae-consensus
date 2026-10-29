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
//! proven EMPTY `recall` of another project (its isolation evidence). Each
//! is found by those semantics; when several captures have them, the one
//! named as the contract names it (`list-<project>`,
//! `recall-<project>-isolation`) is served and the others are extras. Other
//! captures may be present; they are checked like the rest (digests,
//! ids) but never served or boot-verified. [`Snapshot::load`] checks the
//! whole directory structurally; every failure is a [`SnapshotError`] with
//! a stable code ([`codes`]) and a message that names the offending file
//! by its relative path and never quotes its contents. The directory is
//! walked without following symbolic links (a link anywhere under it is
//! refused, so a loop cannot recurse and a link cannot stand for an
//! artifact) and no deeper than the flat layout ([`MAX_DEPTH`]). After
//! boot, [`SnapshotMemoryClient::check_artifacts`] re-hashes the served
//! artifacts (`manifest.json` and every file of the two served captures:
//! response, proof, witness, capture-time report) against the digests the
//! boot check accepted (and [`MemoryClient::status`] calls it on every
//! call), so the public live mode's evidence factory stops reporting the
//! evidence ok (`/ready`'s `evidence_ok`) and refuses admissions as soon as
//! a file on disk is not the one verified (`crate::live::snapshot_factory`).
//!
//! # Pins
//!
//! Two optional operator pins (production sets both; ADR-0008, "Binding")
//! tie the directory and the verifier to values published out of band:
//!
//! - `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` ([`SnapshotConfig::manifest_sha256`]):
//!   the loader refuses a `manifest.json` whose SHA-256 differs
//!   (`manifest_pin_mismatch`), before parsing it. The manifest holds the
//!   digest of every capture file, so with the pin every byte served is
//!   bound to the published digest, `SHA256SUMS` regenerated or not.
//! - `CONSENSUS_HYPHAE_BIN_SHA256` ([`SnapshotConfig::binary_sha256`]): at
//!   boot, before any verifier run, the file `CONSENSUS_HYPHAE_BIN` names
//!   (an absolute path, symbolic links followed as `exec` follows them)
//!   must hash to the pin (`verifier_binary_mismatch`) and the pin must be
//!   the manifest's `hyphae.binary_sha256` (`capture_binary_mismatch`: the
//!   snapshot was captured with another Hyphae build)
//!   ([`check_verifier_binary`]). The binary is hashed at boot only: one
//!   replaced while the server runs is not detected until the next start.
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
//! sealed request re-executes to the sealed result. What binds those bytes
//! to the RECORDS the runtime serves is weaker, and is exactly this:
//!
//! - The verifier's report at boot must name the same `proof_digest`,
//!   `result_digest` and `anchor_digest` as the capture-time
//!   `.verify.json` of the same capture, so the artifacts verified now are
//!   the ones verified when the response was captured
//!   (`boot_verification_failed` otherwise).
//! - The two served captures must carry distinct proof bytes and distinct
//!   response-reported `proof_blake3` values: a proof seals its own
//!   request, so one proof cannot stand for both the list and the empty
//!   recall. A copy step that writes one capture's proof and witness into
//!   the other's files is caught at load (`proof_shared`).
//! - The response file (the record texts) and the capture-time report are
//!   bound to their capture by the manifest's `response_sha256` and
//!   `verify_sha256`, checked against the very bytes the loader then
//!   parses (`digest_mismatch`), and by `SHA256SUMS`. The manifest builder
//!   writes both digests from the ones the capture session recorded the
//!   moment each file was written (`captures.index`). Nothing in the
//!   directory binds the manifest itself: a consistent rewrite of
//!   `manifest.json` and `SHA256SUMS` loads unless the manifest is pinned
//!   (`CONSENSUS_SNAPSHOT_MANIFEST_SHA256`, above). Nor is the text bound to
//!   the proof: the runtime cannot recompute the response's
//!   `proof_blake3` (no BLAKE3 in this workspace), and the verifier's
//!   `proof_digest` is NOT that value: the 2026-10-27 recovery probe (an
//!   MCP recall; its `proof` object has the same fields as `agent ui`'s)
//!   reported `proof_blake3 1ee5c45d…df79` for the proof file
//!   `recall-01a17bf3-…-361661d67761.proof`, while `hyphae proof verify`
//!   on that file reported `proof_digest 6a5f2fa6…edbb`
//!   (`demo-run/recovery-2026-10-27/`, local). Nor is `result_digest`
//!   recomputable from the response without Hyphae's canonical form. A
//!   swap of ALL files of one capture with another's is therefore caught
//!   by the manifest's digests, and with the manifest's entries rewritten
//!   to match, through the responses (record count, project attribution,
//!   memory ids), never through the proofs.
//! - Every proof of a session shares the anchor, so after boot the
//!   verifier alone cannot tell the list capture's artifacts from the
//!   isolation capture's either. A run's verifier is therefore bound to
//!   the boot check ([`BootBoundVerifier`]): its `Valid` stands only when
//!   the report's `proof_digest` and `result_digest` are the ones boot
//!   accepted for the record's `proof_ref`.
//!
//! The record texts are never byte-compared with the proof; the verifier's
//! own detail says so, and the honest label of ADR-0008 applies ("this
//! proves the records are the snapshot's records, not that they are true or
//! that a model used them").

use crate::config::SnapshotConfig;
use crate::hyphae::{CommandRunner, HyphaeProofVerifier, VERIFIED_KIND, VERIFIED_SCOPE};
use consensus_domain::*;
use consensus_runtime::contract::quote;
use consensus_runtime::ports::{
    MemoryClient, MemoryError, MemoryStatus, ProofCheck, ProofVerifier, Recall, RecallProof,
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
/// Deepest path under the snapshot directory, in `/`-separated components:
/// the layout is flat (`manifest.json`, `captures/<file>`), so a directory
/// inside `captures/` (or inside any other first-level directory) is
/// refused rather than walked.
pub const MAX_DEPTH: usize = 2;

/// Stable codes of [`SnapshotError`].
pub mod codes {
    /// The configured directory does not exist or is not a directory.
    pub const SNAPSHOT_DIR_MISSING: &str = "snapshot_dir_missing";
    /// `manifest.json` cannot be read.
    pub const MANIFEST_UNREADABLE: &str = "manifest_unreadable";
    /// `manifest.json` is not the documented JSON document (a field is
    /// missing, mistyped or out of range).
    pub const MANIFEST_INVALID: &str = "manifest_invalid";
    /// The SHA-256 of `manifest.json` is not the operator-pinned
    /// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`: the directory is not the
    /// snapshot whose digest was published.
    pub const MANIFEST_PIN_MISMATCH: &str = "manifest_pin_mismatch";
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
    /// A symbolic link under the snapshot directory. Artifacts are plain
    /// files: a link could point outside the directory (its target would be
    /// hashed and served as if it were the artifact) or loop.
    pub const SYMLINK_REFUSED: &str = "symlink_refused";
    /// A directory nested deeper than the flat layout allows
    /// ([`super::MAX_DEPTH`] components).
    pub const LAYOUT_TOO_DEEP: &str = "layout_too_deep";
    /// `SHA256SUMS` is missing or not in `sha256sum` format.
    pub const CHECKSUMS_INVALID: &str = "checksums_invalid";
    /// A file under the directory is not listed, or a listed file is absent.
    pub const CHECKSUMS_INCOMPLETE: &str = "checksums_incomplete";
    /// A file's SHA-256 differs from its `SHA256SUMS` line.
    pub const CHECKSUMS_MISMATCH: &str = "checksums_mismatch";
    /// A capture file (response, proof, witness or capture-time report)
    /// differs from the manifest's digest of it, or a served file differs
    /// from the digest the boot check accepted.
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
    /// re-execution at the manifest anchor, or names no proof and result
    /// digests.
    pub const VERIFY_RECORD_INVALID: &str = "verify_record_invalid";
    /// The two served captures name the same proof bytes (or the same
    /// response-reported `proof_blake3`): one proof cannot seal both the
    /// list and the empty recall.
    pub const PROOF_SHARED: &str = "proof_shared";
    /// The verifier did not accept a required capture at boot, or its
    /// report's digests are not the capture-time record's.
    pub const BOOT_VERIFICATION_FAILED: &str = "boot_verification_failed";
    /// The snapshot serves a project other than the one its user requires
    /// (the public live presets all run against `demo-repo`,
    /// `crate::live::presets::PRESET_PROJECT`); raised by the live mode's
    /// evidence factory before anything is verified.
    pub const PROJECT_MISMATCH: &str = "project_mismatch";
    /// The file `CONSENSUS_HYPHAE_BIN` names is missing, unreadable, not a
    /// regular file, or does not hash to the pinned
    /// `CONSENSUS_HYPHAE_BIN_SHA256`; raised at boot before any verifier
    /// run.
    pub const VERIFIER_BINARY_MISMATCH: &str = "verifier_binary_mismatch";
    /// The pinned `CONSENSUS_HYPHAE_BIN_SHA256` is not the manifest's
    /// `hyphae.binary_sha256`: the snapshot was captured with another
    /// Hyphae build; raised at boot before any verifier run.
    pub const CAPTURE_BINARY_MISMATCH: &str = "capture_binary_mismatch";
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
    /// An error with one of the [`codes`]; `message` must follow the rule
    /// above (relative file names, no contents).
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
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
    /// SHA-256 of the response file (the record texts), as the capture
    /// session recorded it the moment the file was written. Required: the
    /// one binding of the texts that the manifest pin covers.
    pub response_sha256: String,
    /// SHA-256 of the capture-time verifier report, recorded the same way.
    pub verify_sha256: String,
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

/// What the capture-time `.verify.json` recorded about the artifacts:
/// the digests the boot verification must reproduce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedReport {
    pub proof_digest: String,
    pub result_digest: String,
}

struct ParsedCapture {
    memories: Vec<CapturedMemory>,
    snapshot: Option<ResponseSnapshot>,
    report: RecordedReport,
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
    /// SHA-256 of the `manifest.json` bytes that were loaded.
    manifest_sha256: String,
    list: usize,
    isolation: usize,
    records: Vec<CapturedMemory>,
    /// One per capture, in manifest order.
    reports: Vec<RecordedReport>,
}

impl Snapshot {
    /// Loads and checks `dir` (see the module doc for every check), its
    /// manifest not pinned. With `verifier_dir`, artifact paths handed to
    /// the verifier are joined under it with `/` instead of under `dir`.
    pub fn load(dir: &Path, verifier_dir: Option<&str>) -> Result<Self> {
        Self::load_pinned(dir, verifier_dir, None)
    }

    /// [`load`](Self::load) with the manifest pinned to `manifest_pin`
    /// (64 hex digits, `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`): a
    /// `manifest.json` whose SHA-256 differs is refused with
    /// `manifest_pin_mismatch` before it is parsed. The listing is checked
    /// first, as without a pin.
    pub fn load_pinned(
        dir: &Path,
        verifier_dir: Option<&str>,
        manifest_pin: Option<&str>,
    ) -> Result<Self> {
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
        let manifest_sha256 = sha256_hex(&manifest_bytes);
        // The pin is compared with the very bytes parsed below, before they
        // are parsed: a manifest that is not the published one is reported
        // as such, whatever else it says.
        if let Some(pin) = manifest_pin {
            let pin = pin.trim().to_ascii_lowercase();
            if pin != manifest_sha256 {
                return Err(SnapshotError::new(
                    codes::MANIFEST_PIN_MISMATCH,
                    format!(
                        "{MANIFEST_FILE}: its SHA-256 is {manifest_sha256}, not the pinned {} {pin}: this is not the snapshot whose manifest digest was published",
                        crate::config::MANIFEST_SHA256_VAR
                    ),
                ));
            }
        }
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| {
            SnapshotError::new(
                codes::MANIFEST_INVALID,
                format!("{MANIFEST_FILE}: {}", one_line(&e.to_string(), 200)),
            )
        })?;
        check_manifest(&manifest, dir)?;

        let mut parsed = Vec::with_capacity(manifest.captures.len());
        for capture in &manifest.captures {
            parsed.push(check_capture(dir, &manifest, capture)?);
        }

        // The required list capture of the served project.
        let list = required(
            &manifest,
            "the proven list of the served project (operation list, query \"\", limit 64)",
            |c| {
                c.operation == "list"
                    && c.arguments.project == manifest.project
                    && c.arguments.query.is_empty()
                    && c.arguments.limit == LIST_LIMIT
            },
            |_| format!("list-{}", manifest.project),
        )?;
        let list_capture = &manifest.captures[list];
        let ParsedCapture {
            memories, snapshot, ..
        } = &parsed[list];
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
        let isolation = required(
            &manifest,
            "the proven empty recall of another project (operation recall, no memory ids)",
            |c| {
                c.operation == "recall"
                    && c.arguments.project != manifest.project
                    && c.arguments.project != crate::hyphae::GLOBAL_PROJECT
                    && c.memory_ids.is_empty()
            },
            |c| format!("recall-{}-isolation", c.arguments.project),
        )?;

        // A proof seals its own request: the list and the empty recall
        // cannot be sealed by one proof. Equal digests mean a copy step
        // wrote one capture's artifacts (or response) into the other's.
        let isolation_capture = &manifest.captures[isolation];
        if list_capture.proof_sha256 == isolation_capture.proof_sha256
            || list_capture.proof_blake3 == isolation_capture.proof_blake3
        {
            return Err(SnapshotError::new(
                codes::PROOF_SHARED,
                format!(
                    "captures {} and {} name the same proof: one proof cannot seal both the list and the empty recall",
                    list_capture.id, isolation_capture.id
                ),
            ));
        }

        let records = memories.clone();
        let reports = parsed.into_iter().map(|p| p.report).collect();
        Ok(Self {
            dir: dir.to_path_buf(),
            verifier_dir,
            manifest,
            manifest_sha256,
            list,
            isolation,
            records,
            reports,
        })
    }

    /// [`load_pinned`](Self::load_pinned) with the configured directories
    /// and manifest pin (none when `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` is
    /// unset).
    pub fn load_from_config(cfg: &SnapshotConfig) -> Result<Self> {
        Self::load_pinned(
            &cfg.dir,
            cfg.verifier_dir.as_deref(),
            cfg.manifest_sha256.as_deref(),
        )
    }

    /// SHA-256 of the `manifest.json` bytes that were loaded (the value
    /// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` pins).
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// The proven list of the served project.
    pub fn list_capture(&self) -> &Capture {
        &self.manifest.captures[self.list]
    }

    /// The proven empty recall of another project.
    pub fn isolation_capture(&self) -> &Capture {
        &self.manifest.captures[self.isolation]
    }

    /// What the capture-time verify record of `capture` (a capture of this
    /// snapshot's manifest) says the verifier must reproduce.
    pub fn recorded_report(&self, capture: &Capture) -> Option<&RecordedReport> {
        self.manifest
            .captures
            .iter()
            .position(|c| c.id == capture.id)
            .map(|i| &self.reports[i])
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

/// The index of the required capture matching `pick`: the only one with
/// those semantics, or, when several have them (additional captures are
/// allowed), the one named as the contract names it (`contract_id`).
fn required(
    manifest: &Manifest,
    what: &str,
    pick: impl Fn(&Capture) -> bool,
    contract_id: impl Fn(&Capture) -> String,
) -> Result<usize> {
    let found: Vec<usize> = manifest
        .captures
        .iter()
        .enumerate()
        .filter(|(_, c)| pick(c))
        .map(|(i, _)| i)
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(SnapshotError::new(
            codes::CAPTURE_MISSING,
            format!("no capture is {what}"),
        )),
        several => {
            let named: Vec<usize> = several
                .iter()
                .copied()
                .filter(|i| {
                    let capture = &manifest.captures[*i];
                    capture.id == contract_id(capture)
                })
                .collect();
            if let [one] = named.as_slice() {
                return Ok(*one);
            }
            Err(SnapshotError::new(
                codes::CAPTURE_AMBIGUOUS,
                format!(
                    "{} captures are {what} and none is named as the contract names it: {}",
                    several.len(),
                    several
                        .iter()
                        .map(|i| manifest.captures[*i].id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ))
        }
    }
}

fn local_path(dir: &Path, relative: &str) -> PathBuf {
    let mut path = dir.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
    }
    path
}

/// The bytes of a file under the snapshot directory, bounded. A symbolic
/// link is refused, not followed (`symlink_refused`).
fn read_file(dir: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = local_path(dir, relative);
    let meta = std::fs::symlink_metadata(&path).map_err(|e| {
        SnapshotError::new(codes::FILE_UNREADABLE, format!("{relative}: {}", e.kind()))
    })?;
    if meta.file_type().is_symlink() {
        return Err(symlink_refused(relative));
    }
    let size = meta.len();
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
        ("response_sha256", &capture.response_sha256),
        ("verify_sha256", &capture.verify_sha256),
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
/// Symbolic links are refused wherever they appear (`symlink_refused`: a
/// link is never followed, so a loop cannot recurse and a link to a file
/// outside the directory is not taken for the artifact), and so is a
/// directory nested deeper than the flat layout (`layout_too_deep`,
/// [`MAX_DEPTH`]).
fn walk(dir: &Path) -> Result<Vec<String>> {
    /// `depth` is the number of components of the entries of `dir`.
    fn visit(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) -> Result<()> {
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
            // Not followed: a link is reported as what it is.
            let meta = std::fs::symlink_metadata(&path).map_err(|e| {
                SnapshotError::new(codes::FILE_UNREADABLE, format!("{relative}: {}", e.kind()))
            })?;
            if meta.file_type().is_symlink() {
                return Err(symlink_refused(&relative));
            }
            if meta.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    return Err(SnapshotError::new(
                        codes::LAYOUT_TOO_DEEP,
                        format!(
                            "{relative}: a directory nested deeper than the flat layout allows ({MAX_DEPTH} components)"
                        ),
                    ));
                }
                visit(&path, &relative, depth + 1, out)?;
            } else {
                out.push(relative);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(dir, "", 1, &mut out)?;
    out.sort();
    Ok(out)
}

fn symlink_refused(relative: &str) -> SnapshotError {
    SnapshotError::new(
        codes::SYMLINK_REFUSED,
        format!("{relative}: a symbolic link; a snapshot holds plain files only"),
    )
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
        // `split_at_checked`: a non-ASCII byte in the first 64 is a format
        // error, never a panic on a char boundary.
        let (digest, rest) = match line.split_at_checked(64) {
            Some(parts) if line.len() > 66 => parts,
            _ => {
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

/// The bytes of the capture file `relative`, which must hash to `expected`
/// (the manifest entry's digest of it): what a caller parses next is what
/// was checked here, not a later read of the file.
fn read_digested(dir: &Path, relative: &str, expected: &str, capture_id: &str) -> Result<Vec<u8>> {
    let bytes = read_file(dir, relative)?;
    if sha256_hex(&bytes) != expected {
        return Err(SnapshotError::new(
            codes::DIGEST_MISMATCH,
            format!("{relative}: SHA-256 differs from the manifest entry of capture {capture_id}"),
        ));
    }
    Ok(bytes)
}

/// The list/recall result of a captured response (a JSON object): the
/// response itself when `memories` is at its top level, or its `result`
/// member when `hyphae agent ui` wrapped it. The 4.0.0 `agent ui` prints the
/// envelope `{"id":…,"ok":true,"result":{…},"schema":…}` (observed in the
/// capture session of 2026-10-29); `scripts/snapshot_manifest.py`
/// (`result_object`) accepts the same two shapes. An envelope that reports
/// an error, says `ok` is not true, or carries no result object is refused.
fn result_object(response: &Value) -> std::result::Result<&Value, &'static str> {
    if response.get("memories").is_some() {
        return Ok(response);
    }
    if response.get("error").is_some_and(|e| !e.is_null()) {
        return Err("the response is an error, not a result");
    }
    match response.get("result") {
        Some(result) if result.is_object() => {
            if response.get("ok").and_then(Value::as_bool) != Some(true) {
                return Err("the response envelope does not say ok");
            }
            if result.get("memories").is_none() {
                return Err("no memories array (neither at the top level nor under result)");
            }
            Ok(result)
        }
        Some(_) => Err("result is not an object"),
        None => Err("no memories array (neither at the top level nor under result)"),
    }
}

/// One capture's files against its manifest entry.
fn check_capture(dir: &Path, manifest: &Manifest, capture: &Capture) -> Result<ParsedCapture> {
    let id = &capture.id;
    // Every file against the manifest's digest first, in file order: an
    // altered file (an edited record text, say) is reported as such, never
    // as whatever parse error it then causes.
    let response_bytes = read_digested(dir, &capture.response_file, &capture.response_sha256, id)?;
    read_digested(dir, &capture.proof_file, &capture.proof_sha256, id)?;
    read_digested(dir, &capture.witness_file, &capture.witness_sha256, id)?;
    let verify_bytes = read_digested(dir, &capture.verify_file, &capture.verify_sha256, id)?;

    let response_file = &capture.response_file;
    let response: Value = serde_json::from_slice(&response_bytes).map_err(|e| {
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
    let response = result_object(&response).map_err(invalid)?;
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
    let verify: Value = serde_json::from_slice(&verify_bytes).map_err(|e| {
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
    // The digests the boot verification must reproduce (the pinned 4.0.0
    // verifier prints both).
    let digest = |key: &str| {
        said(key).filter(|d| is_hex64(d)).ok_or_else(|| {
            SnapshotError::new(
                codes::VERIFY_RECORD_INVALID,
                format!("{verify_file}: {key} is not 64 lowercase hex digits"),
            )
        })
    };
    let report = RecordedReport {
        proof_digest: digest("proof_digest")?,
        result_digest: digest("result_digest")?,
    };
    Ok(ParsedCapture {
        memories,
        snapshot,
        report,
    })
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

/// Largest verifier binary [`check_verifier_binary`] hashes. The released
/// Linux `hyphae` is far smaller; the bound only keeps a misconfigured
/// path from stalling boot.
pub const MAX_BINARY_BYTES: u64 = 1024 * 1024 * 1024;

/// The pinned verifier binary, checked at boot before any verifier run; a
/// no-op when `CONSENSUS_HYPHAE_BIN_SHA256` is unset. The file
/// `CONSENSUS_HYPHAE_BIN` names (an absolute path: [`SnapshotConfig`]
/// refuses any other with the pin set, and so does this check for a
/// configuration built in code; symbolic links followed, as `exec` follows
/// them) must be a regular file whose SHA-256 is the pin
/// (`verifier_binary_mismatch`: a replaced binary, or a wrapper that prints
/// a capture-time report, never runs), and the pin must be the manifest's
/// `hyphae.binary_sha256` (`capture_binary_mismatch`: the snapshot was
/// captured with another Hyphae build). Messages name the variables and
/// the digests, never the binary's path. The file is hashed once, here:
/// one replaced after boot is not detected until the next start.
/// [`SnapshotMemoryClient::open`] and the live mode's evidence factory
/// (`crate::live::snapshot_factory`) call it.
pub fn check_verifier_binary(cfg: &SnapshotConfig, manifest: &Manifest) -> Result<()> {
    let Some(pin) = cfg.binary_sha256.as_deref() else {
        return Ok(());
    };
    let pin = pin.trim().to_ascii_lowercase();
    let var = crate::config::HYPHAE_BIN_SHA256_VAR;
    let mismatch = |message: String| SnapshotError::new(codes::VERIFIER_BINARY_MISMATCH, message);
    // The configuration refuses this already; a configuration built in
    // code is held to it here: a relative path or a bare name is resolved
    // differently when hashed (the working directory) and when run (PATH).
    if !Path::new(&cfg.binary).is_absolute() {
        return Err(mismatch(format!(
            "CONSENSUS_HYPHAE_BIN is not an absolute path, so the file hashed could differ from the one run: it is not checked against the pinned {var} ({pin})"
        )));
    }
    let actual = file_sha256(Path::new(&cfg.binary)).map_err(|why| {
        mismatch(format!(
            "CONSENSUS_HYPHAE_BIN {why}, so it is not the binary pinned by {var} ({pin})"
        ))
    })?;
    if actual != pin {
        return Err(mismatch(format!(
            "CONSENSUS_HYPHAE_BIN hashes to {actual}, not to the pinned {var} {pin}: it is not the verifier build that was pinned"
        )));
    }
    if manifest.hyphae.binary_sha256 != pin {
        return Err(SnapshotError::new(
            codes::CAPTURE_BINARY_MISMATCH,
            format!(
                "snapshot {} was captured with the Hyphae build {} (hyphae.binary_sha256), not the pinned {var} {pin}",
                manifest.snapshot_id, manifest.hyphae.binary_sha256
            ),
        ));
    }
    Ok(())
}

/// SHA-256 of a regular file of at most [`MAX_BINARY_BYTES`], read in
/// chunks; `Err` says why not (an I/O error kind, not a regular file, too
/// large), without the path.
fn file_sha256(path: &Path) -> std::result::Result<String, String> {
    use std::io::Read;
    let unreadable = |e: std::io::Error| format!("cannot be read ({})", e.kind());
    let too_large = |size: u64| format!("is {size} bytes, more than {MAX_BINARY_BYTES}");
    let meta = std::fs::metadata(path).map_err(unreadable)?;
    if !meta.is_file() {
        return Err("is not a regular file".into());
    }
    if meta.len() > MAX_BINARY_BYTES {
        return Err(too_large(meta.len()));
    }
    let mut file = std::fs::File::open(path).map_err(unreadable)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total: u64 = 0;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(unreadable(e)),
        };
        total += n as u64;
        if total > MAX_BINARY_BYTES {
            return Err(too_large(total));
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// What the boot verification established about the served snapshot, for
/// the operator's startup line and the live mode's evidence factory: plain
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
    /// SHA-256 of the Hyphae binary that produced the captures (the
    /// manifest's `hyphae.binary_sha256`, published out of band). With
    /// `CONSENSUS_HYPHAE_BIN_SHA256` set, the boot check required the
    /// verifier binary to be this build ([`check_verifier_binary`]);
    /// without it, nothing compared them.
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
    /// The local snapshot directory, for [`check_artifacts`](Self::check_artifacts).
    dir: PathBuf,
    /// `(relative path, SHA-256)` of `manifest.json` and of every file of
    /// the two served captures (response, proof, witness, capture-time
    /// report), as the boot check accepted them.
    artifacts: Vec<(String, String)>,
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
    /// Loads the configured snapshot (its manifest pinned when
    /// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` is set), checks the pinned
    /// verifier binary ([`check_verifier_binary`]) and verifies its two
    /// required captures with `verifier` (the same `hyphae proof verify` the
    /// runs use). Boot fails when a pin does not hold, when the pinned anchor
    /// is not the manifest's, or when either capture is not `Valid` as a
    /// `memory` proof re-executed at scope `semantic_reexecution` against
    /// that anchor.
    pub fn open<V: ProofVerifier + ?Sized>(cfg: &SnapshotConfig, verifier: &mut V) -> Result<Self> {
        let snapshot = Snapshot::load_from_config(cfg)?;
        check_verifier_binary(cfg, &snapshot.manifest)?;
        Self::from_snapshot(snapshot, &cfg.expected_anchor, verifier)
    }

    /// [`open`](Self::open) over an already loaded snapshot, with
    /// `verifier` as given: no binary pin applies here (the configuration
    /// holds it; [`open`](Self::open) and the live mode's evidence factory
    /// check it before calling this).
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
        for index in [snapshot.list, snapshot.isolation] {
            captures.push(verify_capture(&snapshot, index, verifier)?);
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
        let mut artifacts = vec![(MANIFEST_FILE.to_string(), snapshot.manifest_sha256.clone())];
        for capture in [list, isolation] {
            artifacts.extend([
                (
                    capture.response_file.clone(),
                    capture.response_sha256.clone(),
                ),
                (capture.proof_file.clone(), capture.proof_sha256.clone()),
                (capture.witness_file.clone(), capture.witness_sha256.clone()),
                (capture.verify_file.clone(), capture.verify_sha256.clone()),
            ]);
        }
        Ok(Self {
            dir: snapshot.dir.clone(),
            artifacts,
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

    /// The served artifacts as they are on disk NOW: `manifest.json` and
    /// every file of the two served captures (response, proof, witness,
    /// capture-time report) are re-read (bounded, no symlink) and re-hashed
    /// against the digests the boot check accepted. A file deleted,
    /// truncated or replaced after boot (an edited record text included) is
    /// a [`SnapshotError`] (`file_unreadable`, `file_too_large`,
    /// `symlink_refused` or `digest_mismatch`) naming the artifact by its
    /// relative path, never by a filesystem path or its contents. The
    /// records themselves are served from memory, as boot read them; the
    /// re-check keeps the disk (what the next start would load) honest.
    pub fn check_artifacts(&self) -> Result<()> {
        for (relative, expected) in &self.artifacts {
            let bytes = read_file(&self.dir, relative)?;
            if sha256_hex(&bytes) != *expected {
                return Err(SnapshotError::new(
                    codes::DIGEST_MISMATCH,
                    format!("{relative}: SHA-256 differs from the digest accepted at boot"),
                ));
            }
        }
        Ok(())
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

    /// `inner` (a run's own `hyphae proof verify` verifier) bound to what
    /// this client's boot check accepted: see [`BootBoundVerifier`].
    pub fn boot_bound_verifier<V: ProofVerifier>(&self, inner: V) -> BootBoundVerifier<V> {
        BootBoundVerifier {
            inner,
            accepted: self
                .status
                .captures
                .iter()
                .filter_map(|c| c.report.clone().map(|report| (c.id.clone(), report)))
                .collect(),
        }
    }
}

/// The fixed `detail` of a record [`BootBoundVerifier`] refuses.
pub const NOT_THE_BOOT_ARTIFACTS: &str = "the verifier accepted proof artifacts whose proof or result digest is not the one the boot check accepted for this record's proof_ref: the snapshot's files changed after boot, so the record is not proven by the sealed snapshot";

/// A run's proof verifier bound to the boot check of its snapshot.
///
/// Every proof of a capture session shares the anchor, so `hyphae proof
/// verify` accepts the isolation capture's proof and witness as readily
/// as the list capture's: a pair copied over the served files after boot
/// would verify `Valid` (kind, scope and pinned anchor all hold). This
/// wrapper lets a `Valid` result stand only when the verifier's report
/// names the `proof_digest` and `result_digest` of the boot-accepted report
/// of the record's `proof_ref` (the verifier's parsed report carries no
/// request digest to compare). Anything else (another digest, a record
/// without a `proof_ref`, a report without digests) is `Invalid` with
/// [`NOT_THE_BOOT_ARTIFACTS`] as its detail; the verifier's own report is
/// kept on the record. Results other than `Valid` pass through unchanged.
pub struct BootBoundVerifier<V> {
    inner: V,
    /// `(capture id, report)` of every capture boot verified.
    accepted: Vec<(String, VerifierReport)>,
}

impl<V> BootBoundVerifier<V> {
    /// The wrapped verifier (tests read its configuration).
    pub fn inner(&self) -> &V {
        &self.inner
    }
}

impl<V: ProofVerifier> ProofVerifier for BootBoundVerifier<V> {
    fn verify(&mut self, evidence: &EvidenceReference) -> ProofCheck {
        let check = self.inner.verify(evidence);
        if check.status != IntegrityStatus::Valid {
            return check;
        }
        let same = |now: &Option<String>, then: &Option<String>| match (now, then) {
            (Some(now), Some(then)) => now.trim().eq_ignore_ascii_case(then.trim()),
            _ => false,
        };
        let boot = evidence
            .proof_ref
            .as_deref()
            .and_then(|r| self.accepted.iter().find(|(id, _)| id == r))
            .map(|(_, report)| report);
        let bound = match (boot, check.report.as_ref()) {
            (Some(then), Some(now)) => {
                same(&now.proof_digest, &then.proof_digest)
                    && same(&now.result_digest, &then.result_digest)
            }
            _ => false,
        };
        if bound {
            return check;
        }
        ProofCheck {
            status: IntegrityStatus::Invalid,
            detail: Some(NOT_THE_BOOT_ARTIFACTS.into()),
            ..check
        }
    }
}

/// Runs the verifier on one capture's artifacts and requires `Valid` as a
/// memory proof re-executed against the manifest anchor, with the proof
/// and result digests the capture-time verify record holds.
fn verify_capture<V: ProofVerifier + ?Sized>(
    snapshot: &Snapshot,
    index: usize,
    verifier: &mut V,
) -> Result<CaptureStatus> {
    let capture = &snapshot.manifest.captures[index];
    let recorded = &snapshot.reports[index];
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
    let normalized = |value: Option<&str>| value.map(|a| a.trim().to_ascii_lowercase());
    let verified_anchor = normalized(check.verified_anchor.as_deref());
    let report_anchor = normalized(report.as_ref().and_then(|r| r.anchor_digest.as_deref()));
    let accepted = check.status == IntegrityStatus::Valid
        && kind == Some(VERIFIED_KIND)
        && scope == Some(VERIFIED_SCOPE)
        && verified_anchor.as_deref() == Some(anchor.as_str())
        && report_anchor.as_deref() == Some(anchor.as_str());
    if !accepted {
        return Err(SnapshotError::new(
            codes::BOOT_VERIFICATION_FAILED,
            format!(
                "capture {}: the verifier reported {:?} (kind {}, scope {}, anchor {}): {}",
                capture.id,
                check.status,
                kind.unwrap_or("?"),
                scope.unwrap_or("?"),
                report_anchor.as_deref().unwrap_or("?"),
                one_line(check.detail.as_deref().unwrap_or("no detail"), 300)
            ),
        ));
    }
    // The artifacts verified now are the ones verified at capture time:
    // the verifier's digests must be the capture-time record's.
    for (what, now, then) in [
        (
            "proof digest",
            report.as_ref().and_then(|r| r.proof_digest.as_deref()),
            recorded.proof_digest.as_str(),
        ),
        (
            "result digest",
            report.as_ref().and_then(|r| r.result_digest.as_deref()),
            recorded.result_digest.as_str(),
        ),
    ] {
        if normalized(now).as_deref() != Some(then) {
            return Err(SnapshotError::new(
                codes::BOOT_VERIFICATION_FAILED,
                format!(
                    "capture {}: the verifier reported {what} {}, but {} recorded {then} at capture time",
                    capture.id,
                    now.map(|d| one_line(d, 80)).unwrap_or_else(|| "(none)".into()),
                    capture.verify_file
                ),
            ));
        }
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

    /// `healthy` is established on every call, not at boot: the served
    /// artifacts are re-hashed ([`check_artifacts`](Self::check_artifacts)),
    /// so a response, proof, witness, report or manifest deleted, truncated
    /// or replaced on disk reads as unhealthy before the next run would
    /// notice. The detail then starts with the error (its code and the
    /// artifact's relative path) and keeps the boot summary after it. (The
    /// public live mode reads
    /// [`check_artifacts`](Self::check_artifacts) itself, without a clone:
    /// `crate::live::snapshot_factory`.)
    fn status(&mut self) -> std::result::Result<MemoryStatus, MemoryError> {
        let summary = self.status.summary();
        Ok(match self.check_artifacts() {
            Ok(()) => MemoryStatus {
                healthy: true,
                detail: summary,
            },
            Err(e) => MemoryStatus {
                healthy: false,
                detail: format!("{e}; at boot: {summary}"),
            },
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
            // Multi-byte characters across the 64th byte: a format error,
            // not a panic on a char boundary.
            format!("{}  manifest.json", "é".repeat(33)),
            format!("{}  manifest.json", "€".repeat(22)),
            "é".repeat(40),
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
