// SPDX-License-Identifier: Apache-2.0
//! The fixture snapshot shared by the test binaries (`tests/snapshot_client.rs`,
//! `tests/live_snapshot.rs`): a complete ADR-0008 snapshot directory
//! assembled in a temporary directory from `tests/fixtures/snapshot/` with
//! synthetic proof and witness bytes ([`SnapshotDir`], removed with
//! everything next to it when dropped), the configuration that points at
//! it, and a FAKE `hyphae` binary that logs every invocation and answers
//! `proof verify` with the capture-time report of the capture whose proof
//! it was given (digests follow content, as Hyphae's do). No Hyphae runs
//! here.

use consensus_api::config::SnapshotConfig;
use consensus_api::snapshot::MANIFEST_SCHEMA;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ops::Deref;
use std::path::{Path, PathBuf};

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/snapshot");
pub const SNAPSHOT_ID: &str = "snap-2026-10-29-demo-repo";
pub const ANCHOR: &str = "4e1f0c9a7b3d5e2f8a6c4b0d9e7f1a3c5b8d2e4f6a0c1b3d5e7f9a2c4e6b8d0f";
pub const PROJECT: &str = "demo-repo";
pub const OTHER: &str = "other-repo";
pub const LIST_ID: &str = "list-demo-repo";
pub const ISOLATION_ID: &str = "recall-other-repo-isolation";
pub const ISOLATION_QUERY: &str = "Fix payment client timeouts";
pub const CAPTURED_AT: &str = "2026-10-29T15:04:05Z";
pub const VERIFIED_AT: &str = "2026-10-29T15:04:09Z";
/// The seven demo-repo records, in capture order (see the fixture README).
pub const RECORD_IDS: [&str; 7] = [
    "186670067796945807561772405393844769735",
    "8864874238917673930644756936982232317",
    "287567405605917891851290763137356115373",
    "161896895843658017381255677281559699870",
    "237412834382378687445660534362297845298",
    "328114099256634798832222840773545327795",
    "295729039551322334710406856728557559035",
];

/// A fresh `hyphae-consensus-<name>-<pid>` directory under the system
/// temporary directory, emptied first (the same as `tests/support`). Not
/// removed afterwards: [`TempRoot`] is the guarded form.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hyphae-consensus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A [`temp_dir`] removed, with everything in it, when dropped.
pub struct TempRoot(PathBuf);

impl TempRoot {
    pub fn new(name: &str) -> Self {
        Self(temp_dir(name))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Deref for TempRoot {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fixture snapshot directory, `<root>/<SNAPSHOT_ID>`. It derefs to the
/// snapshot directory's path. Its root (a [`TempRoot`]) also holds whatever
/// a test put next to the snapshot (the fake binary, its log, a backup)
/// and is removed with the snapshot when this is dropped.
pub struct SnapshotDir {
    dir: PathBuf,
    root: TempRoot,
}

impl SnapshotDir {
    /// The snapshot directory itself.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The directory the snapshot sits in.
    pub fn root(&self) -> &Path {
        self.root.path()
    }
}

impl Deref for SnapshotDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.dir
    }
}

impl AsRef<Path> for SnapshotDir {
    fn as_ref(&self) -> &Path {
        &self.dir
    }
}

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).expect(name)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One capture to assemble: its manifest entry's fields and which fixture
/// response and capture-time verify report it carries.
#[derive(Clone)]
pub struct Spec {
    pub id: &'static str,
    pub operation: &'static str,
    pub project: &'static str,
    pub query: &'static str,
    pub limit: u32,
    pub response_fixture: &'static str,
    pub verify_fixture: &'static str,
    pub proof_blake3: &'static str,
}

pub fn list_spec() -> Spec {
    Spec {
        id: LIST_ID,
        operation: "list",
        project: PROJECT,
        query: "",
        limit: 64,
        response_fixture: "list-demo-repo.response.json",
        verify_fixture: "list-demo-repo.verify.json",
        proof_blake3: "a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4",
    }
}

pub fn isolation_spec() -> Spec {
    Spec {
        id: ISOLATION_ID,
        operation: "recall",
        project: OTHER,
        query: ISOLATION_QUERY,
        limit: 8,
        response_fixture: "recall-other-repo-isolation.response.json",
        verify_fixture: "recall-other-repo-isolation.verify.json",
        proof_blake3: "e5f60718e5f60718e5f60718e5f60718e5f60718e5f60718e5f60718e5f60718",
    }
}

pub const PROOF_PREFIX: &str = "synthetic proof bytes of ";

pub fn proof_bytes(id: &str) -> Vec<u8> {
    format!("{PROOF_PREFIX}{id}\n").into_bytes()
}

/// The capture whose synthetic proof bytes `path` holds; by file name
/// when the path is not readable here (a mapped container path).
pub fn capture_of_proof(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .trim()
            .strip_prefix(PROOF_PREFIX)
            .unwrap_or_else(|| panic!("not synthetic proof bytes: {path}"))
            .to_string(),
        Err(_) => Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".proof")
            .to_string(),
    }
}

pub fn witness_bytes(id: &str) -> Vec<u8> {
    format!("synthetic witness bytes of {id}: a private copy of the data directory\n").into_bytes()
}

/// Every file under `dir` as `/`-separated relative paths, sorted.
pub fn files_under(dir: &Path) -> Vec<String> {
    fn visit(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.path().is_dir() {
                visit(&entry.path(), &relative, out);
            } else {
                out.push(relative);
            }
        }
    }
    let mut out = Vec::new();
    visit(dir, "", &mut out);
    out.sort();
    out
}

/// (Re)writes `SHA256SUMS` for every other file under `dir`.
pub fn write_checksums(dir: &Path) {
    let mut text = String::new();
    for relative in files_under(dir) {
        if relative == "SHA256SUMS" {
            continue;
        }
        let bytes = std::fs::read(dir.join(&relative)).unwrap();
        text.push_str(&format!("{}  {relative}\n", sha256_hex(&bytes)));
    }
    std::fs::write(dir.join("SHA256SUMS"), text).unwrap();
}

pub fn capture_entry(spec: &Spec, dir: &Path) -> Value {
    let response: Value = serde_json::from_str(&fixture(spec.response_fixture)).unwrap();
    let memory_ids: Vec<Value> = response["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    let id = spec.id;
    let digest = |suffix: &str| {
        sha256_hex(&std::fs::read(dir.join("captures").join(format!("{id}{suffix}"))).unwrap())
    };
    json!({
        "id": id,
        "operation": spec.operation,
        "arguments": {"project": spec.project, "query": spec.query, "limit": spec.limit, "prove": true},
        "response_file": format!("captures/{id}.response.json"),
        "proof_file": format!("captures/{id}.proof"),
        "witness_file": format!("captures/{id}.witness"),
        "verify_file": format!("captures/{id}.verify.json"),
        "proof_blake3": spec.proof_blake3,
        "proof_sha256": digest(".proof"),
        "witness_sha256": digest(".witness"),
        "response_sha256": digest(".response.json"),
        "verify_sha256": digest(".verify.json"),
        "memory_ids": memory_ids,
        "verification": {"verified_at": VERIFIED_AT, "status": "verified", "kind": "memory",
                         "scope": "semantic_reexecution", "anchor_hex": ANCHOR}
    })
}

/// Assembles a complete snapshot directory `<temp>/<SNAPSHOT_ID>` from the
/// fixtures and synthetic artifacts, lets `edit` change the manifest, then
/// writes `SHA256SUMS` over the result.
pub fn build_with(test: &str, specs: &[Spec], edit: impl FnOnce(&mut Value)) -> SnapshotDir {
    let root = TempRoot::new(&format!("snapshot-{test}"));
    let dir = root.join(SNAPSHOT_ID);
    std::fs::create_dir_all(dir.join("captures")).unwrap();
    for spec in specs {
        let captures = dir.join("captures");
        let id = spec.id;
        std::fs::write(
            captures.join(format!("{id}.response.json")),
            fixture(spec.response_fixture),
        )
        .unwrap();
        std::fs::write(captures.join(format!("{id}.proof")), proof_bytes(id)).unwrap();
        std::fs::write(captures.join(format!("{id}.witness")), witness_bytes(id)).unwrap();
        std::fs::write(
            captures.join(format!("{id}.verify.json")),
            fixture(spec.verify_fixture),
        )
        .unwrap();
    }
    let list: Value = serde_json::from_str(&fixture("list-demo-repo.response.json")).unwrap();
    let mut manifest = json!({
        "schema": MANIFEST_SCHEMA,
        "snapshot_id": SNAPSHOT_ID,
        "captured_at": CAPTURED_AT,
        "project": PROJECT,
        "anchor_hex": ANCHOR,
        "hyphae": {"version": "hyphae 4.0.0", "binary_sha256": "f0e1d2c3".repeat(8),
                   "control_schema": "hyphae-omarchy-control-v1"},
        "store": {"record_count": 7, "root_digest": list["snapshot"]["root_digest"],
                  "visible_csn": list["snapshot"]["visible_csn"],
                  "logical_time_micros": list["snapshot"]["logical_time_micros"]},
        "captures": specs.iter().map(|s| capture_entry(s, &dir)).collect::<Vec<_>>()
    });
    edit(&mut manifest);
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    write_checksums(&dir);
    SnapshotDir { dir, root }
}

/// The well-formed snapshot with the two required captures.
pub fn build(test: &str) -> SnapshotDir {
    build_with(test, &[list_spec(), isolation_spec()], |_| {})
}

/// Copies the directory tree `from` to `to` (which must not exist):
/// plain files and directories, as a fixture holds.
pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Replaces the snapshot directory `dir` with the tree `backup`, as an
/// operator restoring a copy would.
pub fn restore_dir(dir: &Path, backup: &Path) {
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    copy_dir(backup, dir);
}

/// Copies the isolation capture's proof and witness over the list
/// capture's: an operator re-copy gone wrong. Every proof of a session
/// shares the anchor, so the verifier alone accepts the pair.
pub fn swap_list_with_isolation(dir: &Path) {
    let captures = dir.join("captures");
    for ext in ["proof", "witness"] {
        std::fs::copy(
            captures.join(format!("{ISOLATION_ID}.{ext}")),
            captures.join(format!("{LIST_ID}.{ext}")),
        )
        .unwrap();
    }
}

/// The snapshot configuration for `dir` with the fixture anchor pinned and
/// `binary` as the verifier (no mapped verifier directory, no digest pin).
pub fn config(dir: &Path, binary: &str) -> SnapshotConfig {
    pinned_config(dir, binary, None, None)
}

/// [`config`] with the digest pins: `manifest` as
/// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`, `binary_sha256` as
/// `CONSENSUS_HYPHAE_BIN_SHA256` (then `binary` must be an absolute path).
pub fn pinned_config(
    dir: &Path,
    binary: &str,
    manifest: Option<&str>,
    binary_sha256: Option<&str>,
) -> SnapshotConfig {
    let vars = [
        (
            "CONSENSUS_SNAPSHOT_DIR",
            Some(dir.to_string_lossy().into_owned()),
        ),
        ("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", Some(ANCHOR.to_string())),
        ("CONSENSUS_HYPHAE_BIN", Some(binary.to_string())),
        (
            "CONSENSUS_SNAPSHOT_MANIFEST_SHA256",
            manifest.map(str::to_string),
        ),
        (
            "CONSENSUS_HYPHAE_BIN_SHA256",
            binary_sha256.map(str::to_string),
        ),
    ];
    SnapshotConfig::from_vars(|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .and_then(|(_, v)| v.clone())
    })
    .unwrap()
    .expect("a snapshot config")
}

/// The capture-time `hyphae proof verify` report of a capture.
pub fn verify_report(id: &str) -> String {
    fixture(&format!("{id}.verify.json"))
}

/// Rewrites every capture file digest of the manifest (`response_sha256`,
/// `proof_sha256`, `witness_sha256`, `verify_sha256`) from the files on
/// disk, as a capture script computing them after a copy step would (or a
/// consistent rewrite of the whole directory), then `SHA256SUMS`: only
/// what binds the manifest itself (its pin) and the checks beyond the
/// digests can still tell.
pub fn recompute_digests(dir: &Path) {
    edit_manifest(dir, |m| {
        for capture in m["captures"].as_array_mut().unwrap() {
            for (field, file) in [
                ("response_sha256", "response_file"),
                ("proof_sha256", "proof_file"),
                ("witness_sha256", "witness_file"),
                ("verify_sha256", "verify_file"),
            ] {
                let relative = capture[file].as_str().unwrap().to_string();
                let bytes = std::fs::read(dir.join(relative)).unwrap();
                capture[field] = json!(sha256_hex(&bytes));
            }
        }
    });
    write_checksums(dir);
}

pub fn flip_first_byte(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[0] ^= 0x01;
    std::fs::write(path, bytes).unwrap();
}

pub fn edit_manifest(dir: &Path, edit: impl FnOnce(&mut Value)) {
    let path = dir.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
}

/// The SHA-256 of the snapshot's `manifest.json` as it is now (the value
/// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` pins).
pub fn manifest_sha256(dir: &Path) -> String {
    sha256_hex(&std::fs::read(dir.join("manifest.json")).unwrap())
}

/// Records the file `binary` as the Hyphae build the snapshot was captured
/// with: its real SHA-256 goes into the manifest's `hyphae.binary_sha256`
/// (what a capture with that binary records), then `SHA256SUMS` is
/// rewritten. Returns the digest, the value `CONSENSUS_HYPHAE_BIN_SHA256`
/// pins, so a test can boot the fake binary through the pinned path.
pub fn record_capture_binary(dir: &Path, binary: &str) -> String {
    let digest = sha256_hex(&std::fs::read(binary).unwrap());
    edit_manifest(dir, |m| m["hyphae"]["binary_sha256"] = json!(digest));
    write_checksums(dir);
    digest
}

/// Replaces a record's text in a capture's response file (the attack R6
/// names: the proofs untouched, only the text a model reads changed);
/// nothing else is rewritten.
pub fn edit_record_text(dir: &Path, capture_id: &str, index: usize, text: &str) {
    let path = dir
        .join("captures")
        .join(format!("{capture_id}.response.json"));
    let mut response: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    response["memories"][index]["text"] = json!(text);
    std::fs::write(&path, serde_json::to_string_pretty(&response).unwrap()).unwrap();
}

/// A fake `hyphae` binary under `root`: appends its arguments to the
/// returned log file and prints the capture-time verify report of the
/// capture whose synthetic proof bytes the `--proof` file holds, keyed by
/// the file's whole content (digests follow content, as Hyphae's do): a
/// proof with any byte changed has no report, whatever else was regenerated
/// around it, and the verifier fails. Windows: a `.cmd` script
/// (`std::process::Command` runs it through `cmd.exe`) that looks the
/// report up by the proof's SHA-256 (`certutil -hashfile`); elsewhere a
/// `sh` script that requires the proof to be exactly the one line
/// `synthetic proof bytes of <id>`. Returns the binary's path (for
/// `CONSENSUS_HYPHAE_BIN`) and the log file.
pub fn fake_hyphae(root: &Path) -> (String, PathBuf) {
    let log = root.join("hyphae-calls.log");
    let reports = root.join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    for id in [LIST_ID, ISOLATION_ID] {
        let key = if cfg!(windows) {
            sha256_hex(&proof_bytes(id))
        } else {
            id.to_string()
        };
        std::fs::write(
            reports.join(format!("{key}.verify.json")),
            verify_report(id),
        )
        .unwrap();
    }
    let (name, script) = if cfg!(windows) {
        (
            "hyphae.cmd",
            [
                "@echo off".to_string(),
                format!(">>\"{}\" echo %*", log.display()),
                "set \"proof=\"".into(),
                ":loop".into(),
                "if \"%~1\"==\"\" goto done".into(),
                "if \"%~1\"==\"--proof\" set \"proof=%~2\"".into(),
                "shift".into(),
                "goto loop".into(),
                ":done".into(),
                // The proof's SHA-256: the second line certutil prints
                // (older versions space the bytes apart).
                "set \"hash=\"".into(),
                "for /f \"skip=1 delims=\" %%h in ('certutil -hashfile \"%proof%\" SHA256') do if not defined hash set \"hash=%%h\"".into(),
                "set \"hash=%hash: =%\"".into(),
                format!("type \"{}\\%hash%.verify.json\"", reports.display()),
                String::new(),
            ]
            .join("\r\n"),
        )
    } else {
        (
            "hyphae",
            [
                "#!/bin/sh".to_string(),
                format!("printf '%s\\n' \"$*\" >> \"{}\"", log.display()),
                "proof=".into(),
                "while [ $# -gt 0 ]; do".into(),
                "  if [ \"$1\" = \"--proof\" ]; then proof=$2; fi".into(),
                "  shift".into(),
                "done".into(),
                format!("id=$(sed -n 's/^{PROOF_PREFIX}\\(.*\\)$/\\1/p' \"$proof\")"),
                format!("cat \"{}/$id.verify.json\"", reports.display()),
                String::new(),
            ]
            .join("\n"),
        )
    };
    let path = root.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (path.to_string_lossy().into_owned(), log)
}

/// A stand-in for a REPLACED `CONSENSUS_HYPHAE_BIN` (review R6): it
/// verifies nothing and prints the capture-time report stored next to the
/// proof it is given (`captures/<id>.verify.json` for
/// `captures/<id>.proof`), so every check it answers reproduces the
/// digests the boot check expects, whatever the proof's bytes. Logs its
/// invocations like [`fake_hyphae`]. Returns its path and the log file.
pub fn report_printing_wrapper(root: &Path) -> (String, PathBuf) {
    let log = root.join("wrapper-calls.log");
    let (name, script) = if cfg!(windows) {
        (
            "lying-wrapper.cmd",
            [
                "@echo off".to_string(),
                format!(">>\"{}\" echo %*", log.display()),
                "set \"proof=\"".into(),
                ":loop".into(),
                "if \"%~1\"==\"\" goto done".into(),
                "if \"%~1\"==\"--proof\" set \"proof=%~2\"".into(),
                "shift".into(),
                "goto loop".into(),
                ":done".into(),
                "type \"%proof:.proof=.verify.json%\"".into(),
                String::new(),
            ]
            .join("\r\n"),
        )
    } else {
        (
            "lying-wrapper",
            [
                "#!/bin/sh".to_string(),
                format!("printf '%s\\n' \"$*\" >> \"{}\"", log.display()),
                "proof=".into(),
                "while [ $# -gt 0 ]; do".into(),
                "  if [ \"$1\" = \"--proof\" ]; then proof=$2; fi".into(),
                "  shift".into(),
                "done".into(),
                "cat \"${proof%.proof}.verify.json\"".into(),
                String::new(),
            ]
            .join("\n"),
        )
    };
    let path = root.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (path.to_string_lossy().into_owned(), log)
}

/// The fake binary's invocations so far, one argument line each.
pub fn logged_calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
