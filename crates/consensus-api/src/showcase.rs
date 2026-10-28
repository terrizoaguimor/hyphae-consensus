// SPDX-License-Identifier: Apache-2.0
//! The research showcase (D1): the archived battery and console runs of
//! `research/`, pinned by `research/MANIFEST.json`, served read-only.
//!
//! Everything is loaded once at startup: every pinned file's SHA-256 is
//! checked against the manifest, every event log is verified from its
//! bytes ([`verify_bytes`], never a writable open), and each run's detail
//! is derived from its events on request. Run ids resolve through the
//! manifest's closed set, never through a path join (LC-15, ABUSE-7);
//! `/research/<path>` serves only paths the manifest names, with the
//! pinned SHA-256 as the `ETag`, re-checked on every read. Nothing here
//! writes to `research/`.

use crate::api::types::*;
use crate::public_view::{self, Surface};
use consensus_domain::*;
use consensus_runtime::event_log::verify_bytes;
use consensus_runtime::{build_receipt_v2, ReceiptError};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Archive files the manifest does not pin by digest, served with a
/// digest computed on read.
const UNPINNED: [&str; 3] = ["MANIFEST.json", "README.md", "SHA256SUMS"];

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    files: Vec<ManifestFile>,
    #[serde(default)]
    runs: Vec<ManifestRun>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    path: String,
    sha256: String,
    kind: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestRun {
    run_id: String,
    label: String,
    battery: String,
    task_title: String,
    outcome: String,
    authorization: String,
    schema_version: String,
    file: String,
    #[serde(default)]
    record: Option<String>,
}

struct ArchivedRun {
    index: usize,
    events: Vec<ConsensusEvent>,
}

/// A served archive file.
pub struct ResearchFile {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub content_type: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum ResearchError {
    #[error("not an archive file")]
    NotFound,
    #[error("the file does not match its pinned digest")]
    Mismatch,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The loaded archive.
pub struct Showcase {
    dir: PathBuf,
    entries: Vec<ShowcaseEntryV1>,
    runs: HashMap<RunId, ArchivedRun>,
    /// Served path -> pinned digest (None for unpinned files).
    files: BTreeMap<String, Option<String>>,
    manifest_sha256: String,
    errors: Vec<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn content_type_of(path: &str) -> &'static str {
    if path.ends_with(".jsonl") {
        "application/x-ndjson"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".md") {
        "text/markdown; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    }
}

impl Showcase {
    /// An empty showcase, for servers without an archive.
    pub fn disabled(reason: &str) -> Self {
        Self {
            dir: PathBuf::new(),
            entries: Vec::new(),
            runs: HashMap::new(),
            files: BTreeMap::new(),
            manifest_sha256: String::new(),
            errors: vec![reason.to_string()],
        }
    }

    /// Loads and verifies the archive under `dir`. Problems are collected
    /// in [`status`](Self::status), never raised: a bad file leaves its
    /// runs out and keeps the rest served.
    pub fn load(dir: &Path) -> Self {
        let manifest_path = dir.join("MANIFEST.json");
        let manifest_bytes = match std::fs::read(&manifest_path) {
            Ok(bytes) => bytes,
            Err(e) => return Self::disabled(&format!("research manifest unreadable: {e}")),
        };
        let manifest_sha256 = sha256_hex(&manifest_bytes);
        let manifest: Manifest = match serde_json::from_slice(&manifest_bytes) {
            Ok(m) => m,
            Err(e) => return Self::disabled(&format!("research manifest does not parse: {e}")),
        };
        let mut errors = Vec::new();
        let mut files: BTreeMap<String, Option<String>> = BTreeMap::new();
        for name in UNPINNED {
            files.insert(name.to_string(), None);
        }
        let mut logs: HashMap<String, Vec<ConsensusEvent>> = HashMap::new();
        for file in &manifest.files {
            if !safe_archive_path(&file.path) {
                errors.push(format!("{}: unsafe path in manifest", file.path));
                continue;
            }
            let full = dir.join(&file.path);
            let bytes = match std::fs::read(&full) {
                Ok(b) => b,
                Err(e) => {
                    errors.push(format!("{}: {e}", file.path));
                    continue;
                }
            };
            if sha256_hex(&bytes) != file.sha256 {
                errors.push(format!("{}: sha256 differs from MANIFEST.json", file.path));
                continue;
            }
            files.insert(file.path.clone(), Some(file.sha256.clone()));
            if file.kind == "event_log" {
                match verify_bytes(&bytes) {
                    Ok(events) => {
                        logs.insert(file.path.clone(), events);
                    }
                    Err(e) => errors.push(format!("{}: {e}", file.path)),
                }
            }
        }
        let mut entries = Vec::new();
        let mut runs = HashMap::new();
        for run in &manifest.runs {
            let Some(events) = logs.get(&run.file) else {
                errors.push(format!("{}: log {} not loaded", run.label, run.file));
                continue;
            };
            let run_id = RunId::new(&run.run_id);
            let run_events: Vec<ConsensusEvent> = events
                .iter()
                .filter(|e| e.run_id == run_id)
                .cloned()
                .collect();
            if run_events.is_empty() {
                errors.push(format!(
                    "{}: run {} absent from {}",
                    run.label, run.run_id, run.file
                ));
                continue;
            }
            let Some(sha256) = files.get(&run.file).cloned().flatten() else {
                continue;
            };
            let head_hash = run_events
                .last()
                .map(|e| e.hash.clone())
                .filter(|h| !h.is_empty());
            let entry = ShowcaseEntryV1 {
                run_id: run_id.clone(),
                label: run.label.clone(),
                battery: run.battery.clone(),
                title: run.task_title.clone(),
                outcome: run.outcome.clone(),
                authorization: run.authorization.clone(),
                schema_version: run.schema_version.clone(),
                provenance: public_view::provenance(&run_events, SourceProvenance::ResearchArchive),
                file: run.file.clone(),
                sha256,
                head_hash,
                record_file: run.record.clone(),
                event_count: run_events.len() as u64,
            };
            runs.insert(
                run_id,
                ArchivedRun {
                    index: entries.len(),
                    events: run_events,
                },
            );
            entries.push(entry);
        }
        Self {
            dir: dir.to_path_buf(),
            entries,
            runs,
            files,
            manifest_sha256,
            errors,
        }
    }

    pub fn status(&self) -> ShowcaseStatusV1 {
        ShowcaseStatusV1 {
            ok: self.errors.is_empty() && !self.entries.is_empty(),
            entries: self.entries.len() as u64,
            errors: self.errors.clone(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn list(&self) -> ShowcaseV1 {
        ShowcaseV1 {
            entries: self.entries.clone(),
            manifest_file: "MANIFEST.json".into(),
            manifest_sha256: self.manifest_sha256.clone(),
        }
    }

    pub fn entry(&self, run_id: &str) -> Option<&ShowcaseEntryV1> {
        let run = self.runs.get(&RunId::new(run_id))?;
        self.entries.get(run.index)
    }

    /// The archived run's events (only ids the manifest lists).
    pub fn events(&self, run_id: &str) -> Option<&[ConsensusEvent]> {
        self.runs
            .get(&RunId::new(run_id))
            .map(|r| r.events.as_slice())
    }

    pub fn detail(&self, run_id: &str, surface: Surface) -> Option<RunDetailV1> {
        let events = self.events(run_id)?;
        let agg = public_view::aggregate_of(events)?;
        Some(public_view::detail(
            events,
            &agg,
            SourceProvenance::ResearchArchive,
            false,
            surface,
        ))
    }

    pub fn receipt(
        &self,
        run_id: &str,
        generated_at: String,
    ) -> Option<Result<RunReceiptV2, ReceiptError>> {
        let events = self.events(run_id)?;
        Some(build_receipt_v2(events, generated_at))
    }

    /// An archive file by its served path, verified against its pinned
    /// digest on every read.
    pub fn research_file(&self, path: &str) -> Result<ResearchFile, ResearchError> {
        let pinned = self.files.get(path).ok_or(ResearchError::NotFound)?;
        if !safe_archive_path(path) {
            return Err(ResearchError::NotFound);
        }
        let bytes = std::fs::read(self.dir.join(path))?;
        let sha256 = sha256_hex(&bytes);
        if let Some(expected) = pinned {
            if *expected != sha256 {
                return Err(ResearchError::Mismatch);
            }
        }
        Ok(ResearchFile {
            bytes,
            sha256,
            content_type: content_type_of(path),
        })
    }

    pub fn file_paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }
}

/// A relative path of plain segments: no traversal, no absolute paths, no
/// drive letters, no NUL, no backslashes.
pub fn safe_archive_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains(':')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn research_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("research")
    }

    #[test]
    fn the_archive_loads_verifies_and_resolves_only_manifest_ids() {
        let showcase = Showcase::load(&research_dir());
        let status = showcase.status();
        assert!(status.ok, "{:?}", status.errors);
        assert!(status.entries >= 33, "{}", status.entries);
        let list = showcase.list();
        let f03 = list.entries.iter().find(|e| e.label == "F03").expect("F03");
        assert_eq!(f03.provenance.source, SourceProvenance::ResearchArchive);
        assert_eq!(f03.provenance.model, ModelProvenance::LiveMeta);
        assert!(f03.head_hash.is_some());
        let detail = showcase
            .detail(f03.run_id.as_str(), Surface::Public)
            .unwrap();
        assert_eq!(detail.chain.scope, ChainScope::Log);
        // Public views carry no container paths.
        let text = serde_json::to_string(&detail).unwrap();
        assert!(!text.contains("/hyphae-home/"), "paths leaked");
        assert!(showcase
            .detail("run-00000000-0000-4000-8000-000000000000", Surface::Public)
            .is_none());
        assert!(showcase
            .detail("../MANIFEST.json", Surface::Public)
            .is_none());
        // The 1.0 console runs are served unchained and flagged.
        let c01 = list.entries.iter().find(|e| e.label == "C01").expect("C01");
        assert!(c01.head_hash.is_none());
        assert!(c01.provenance.flags.contains(&ProvenanceFlag::Unchained));
    }

    #[test]
    fn research_files_are_served_by_manifest_path_only() {
        let showcase = Showcase::load(&research_dir());
        let file = showcase.research_file("MANIFEST.json").unwrap();
        assert_eq!(file.content_type, "application/json");
        assert_eq!(file.sha256, showcase.list().manifest_sha256);
        let log = showcase
            .research_file("2026-10-27/live-battery-2026-10-27-final.jsonl")
            .unwrap();
        assert_eq!(log.content_type, "application/x-ndjson");
        for bad in [
            "../Cargo.toml",
            "/etc/passwd",
            "2026-10-27/../../Cargo.toml",
            "C:\\Windows\\win.ini",
            "2026-10-27",
            "nope.jsonl",
        ] {
            assert!(
                matches!(showcase.research_file(bad), Err(ResearchError::NotFound)),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_missing_archive_disables_the_showcase_without_failing() {
        let showcase = Showcase::load(Path::new("definitely/not/here"));
        assert!(!showcase.is_enabled());
        assert!(!showcase.status().ok);
        assert!(showcase.list().entries.is_empty());
    }
}
