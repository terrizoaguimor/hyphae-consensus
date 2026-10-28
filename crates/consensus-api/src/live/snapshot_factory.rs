// SPDX-License-Identifier: Apache-2.0
//! The production [`DeliberationFactory`]: public live runs deliberate
//! over the sealed Hyphae snapshot (ADR-0008), never over a Hyphae daemon
//! and never over simulated evidence.
//!
//! [`SnapshotFactory::from_env`] is what `serve-public` calls at startup:
//! without `CONSENSUS_SNAPSHOT_DIR` there is no factory (and live mode
//! cannot be enabled, see [`crate::server::build_live_gate`]); with it the
//! snapshot is loaded, checked and boot-verified with `hyphae proof
//! verify` ([`SnapshotMemoryClient::open`]) before the server listens, and
//! a snapshot that does not verify stops startup with the snapshot error's
//! stable code. Every error text names variables and files relative to the
//! snapshot directory, never a secret, a local path or a file's contents.
//!
//! Each admitted run gets a copy of the client (plain data) and a FRESH
//! verifier over the configured binary, so the run's `EvidenceVerified`
//! records carry their own verification time and the verifier's
//! content-digest cache (HY-13) never outlives a run. The factory's
//! [`status`](DeliberationFactory::status) is the client's own
//! `MemoryClient::status`, read on a clone: when that check reports the
//! served artifacts changed, `/ready` says `evidence_ok: false` with a
//! stable code and nothing else.

use super::gate::{DeliberationFactory, FactoryError, FactoryStatus, SNAPSHOT_DIR_VAR};
use crate::config::SnapshotConfig;
use crate::hyphae::SystemCommandRunner;
use crate::meta::MetaModelClient;
use crate::runs::Deliberation;
use crate::snapshot::{self, SnapshotError, SnapshotMemoryClient, SnapshotStatus};
use consensus_domain::EvidenceMode;
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::ports::{MemoryClient, ProofVerifier};
use consensus_runtime::{Orchestrator, RunLimits};
use std::sync::Arc;

/// The stable `detail` codes of [`FactoryStatus`] as this factory reports
/// them.
pub mod status {
    /// The client's check passed.
    pub const OK: &str = "ok";
    /// The client reports its served artifacts unhealthy without a code of
    /// its own.
    pub const ARTIFACTS_CHANGED: &str = "artifacts_changed";
    /// The client's status could not be read at all.
    pub const STATUS_UNAVAILABLE: &str = "status_unavailable";
}

/// How many hex digits of the anchor the startup line shows.
const ANCHOR_PREFIX_LEN: usize = 12;

/// The sealed snapshot as the public live gate's evidence factory: one
/// boot-verified [`SnapshotMemoryClient`] shared by every run, and the
/// configuration a fresh per-run verifier is built from.
pub struct SnapshotFactory {
    client: SnapshotMemoryClient,
    cfg: SnapshotConfig,
}

impl std::fmt::Debug for SnapshotFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The configuration holds the local directory: not shown.
        f.debug_struct("SnapshotFactory")
            .field("status", self.snapshot_status())
            .finish_non_exhaustive()
    }
}

impl SnapshotFactory {
    /// The factory `serve-public` starts with: `Ok(None)` without
    /// `CONSENSUS_SNAPSHOT_DIR`; the configuration error (naming its
    /// variable) when a setting is missing or unusable; the boot failure
    /// (`CONSENSUS_SNAPSHOT_DIR: <code>: <message>`) when the snapshot does
    /// not load or verify. One line is logged for a snapshot that booted.
    pub fn from_env() -> Result<Option<Arc<dyn DeliberationFactory>>, String> {
        let cfg = SnapshotConfig::try_from_env().map_err(|e| e.to_string())?;
        Self::boot(cfg)
    }

    /// [`from_env`](Self::from_env) over any variable source (tests).
    pub fn from_vars(
        get: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Arc<dyn DeliberationFactory>>, String> {
        let cfg = SnapshotConfig::from_vars(get).map_err(|e| e.to_string())?;
        Self::boot(cfg)
    }

    fn boot(cfg: Option<SnapshotConfig>) -> Result<Option<Arc<dyn DeliberationFactory>>, String> {
        let Some(cfg) = cfg else {
            return Ok(None);
        };
        let mut verifier = snapshot::verifier_from_config(&cfg, SystemCommandRunner);
        let factory =
            Self::open(cfg, &mut verifier).map_err(|e| format!("{SNAPSHOT_DIR_VAR}: {e}"))?;
        println!("{}", factory.startup_line());
        Ok(Some(Arc::new(factory)))
    }

    /// Loads and boot-verifies the configured snapshot with `verifier`
    /// ([`SnapshotMemoryClient::open`]); the factory then builds every run
    /// over a fresh verifier of its own.
    pub fn open<V: ProofVerifier + ?Sized>(
        cfg: SnapshotConfig,
        verifier: &mut V,
    ) -> Result<Self, SnapshotError> {
        let client = SnapshotMemoryClient::open(&cfg, verifier)?;
        Ok(Self { client, cfg })
    }

    /// What the boot verification established.
    pub fn snapshot_status(&self) -> &SnapshotStatus {
        self.client.snapshot_status()
    }

    /// The configuration the per-run verifiers are built from.
    pub fn config(&self) -> &SnapshotConfig {
        &self.cfg
    }

    /// The one line logged at startup: id, project, record count, anchor
    /// prefix and verification time. No path.
    pub fn startup_line(&self) -> String {
        let s = self.snapshot_status();
        let prefix: String = s.anchor_hex.chars().take(ANCHOR_PREFIX_LEN).collect();
        format!(
            "Sealed snapshot evidence: {} (project {}, {} records, anchor {prefix}…, captured {}, proofs re-verified at boot {})",
            s.snapshot_id, s.project, s.record_count, s.captured_at, s.verified_at
        )
    }
}

/// The stable code for an unhealthy memory status: the client's detail
/// when it is a code (`checksums_mismatch`, or `checksums_mismatch: <file>`
/// as [`SnapshotError`] prints), [`status::ARTIFACTS_CHANGED`] otherwise.
/// A path can never pass as a code.
fn unhealthy_code(detail: &str) -> String {
    let head = detail.split(':').next().unwrap_or("").trim();
    let is_code = !head.is_empty()
        && head.len() <= 64
        && head
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if is_code {
        head.to_string()
    } else {
        status::ARTIFACTS_CHANGED.to_string()
    }
}

impl DeliberationFactory for SnapshotFactory {
    fn evidence_mode(&self) -> EvidenceMode {
        EvidenceMode::SealedSnapshot
    }

    fn build(
        &self,
        model: MetaModelClient,
        limits: RunLimits,
    ) -> Result<Box<dyn Deliberation>, FactoryError> {
        Ok(Box::new(Orchestrator {
            model,
            memory: self.client.clone(),
            // Fresh per run: its own verification times, its own cache.
            verifier: snapshot::verifier_from_config(&self.cfg, SystemCommandRunner),
            clock: SystemClock,
            ids: UuidIdGen,
            limits,
        }))
    }

    fn status(&self) -> FactoryStatus {
        let mut client = self.client.clone();
        let (ok, detail) = match client.status() {
            Ok(s) if s.healthy => (true, status::OK.to_string()),
            Ok(s) => (false, unhealthy_code(&s.detail)),
            Err(_) => (false, status::STATUS_UNAVAILABLE.to_string()),
        };
        FactoryStatus {
            ok,
            kind: EvidenceMode::SealedSnapshot,
            detail: Some(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unhealthy_detail_becomes_a_stable_code_and_never_a_path() {
        assert_eq!(unhealthy_code("checksums_mismatch"), "checksums_mismatch");
        assert_eq!(
            unhealthy_code("digest_mismatch: captures/list-demo-repo.proof: SHA-256 differs"),
            "digest_mismatch"
        );
        assert_eq!(unhealthy_code(" file_unreadable : x"), "file_unreadable");
        for not_a_code in [
            "",
            "C:\\snapshots\\snap\\captures\\x.proof differs",
            "/srv/snap/captures/x.proof: differs",
            "captures/x.proof: differs",
            "Checksums mismatch",
            "the artifacts changed since boot",
        ] {
            assert_eq!(
                unhealthy_code(not_a_code),
                status::ARTIFACTS_CHANGED,
                "{not_a_code:?}"
            );
        }
    }
}
