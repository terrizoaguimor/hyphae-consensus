// SPDX-License-Identifier: Apache-2.0
//! The production [`DeliberationFactory`]: public live runs deliberate
//! over the sealed Hyphae snapshot (ADR-0008), never over a Hyphae daemon
//! and never over simulated evidence.
//!
//! **Startup.** `serve-public` reads the live configuration first and
//! calls [`SnapshotFactory::live_evidence_from_env`] only with live mode
//! enabled ([`crate::server::serve_public_with`]): with it disabled the
//! snapshot is never loaded or verified, and a set `CONSENSUS_SNAPSHOT_DIR`
//! earns one notice ([`UNUSED_NOTICE`]). With live mode enabled:
//! - a configuration error (`CONSENSUS_SNAPSHOT_DIR` unset, a missing or
//!   malformed `CONSENSUS_HYPHAE_EXPECTED_ANCHOR`, a digest pin
//!   `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` or `CONSENSUS_HYPHAE_BIN_SHA256`
//!   that is not 64 hex digits, a `CONSENSUS_HYPHAE_BIN` that is not an
//!   absolute path while the binary is pinned, an unusable verifier
//!   setting) stops startup naming its variable (the unset directory in
//!   [`crate::server::build_live_gate`]);
//! - a snapshot that does not load, verify, match its pins or serve the
//!   presets' project (`snapshot_dir_missing`, `checksums_mismatch`,
//!   `digest_mismatch`, `manifest_pin_mismatch`, `pinned_anchor_mismatch`,
//!   `verifier_binary_mismatch`, `capture_binary_mismatch`,
//!   `boot_verification_failed`, `project_mismatch`, ...) does not stop
//!   the portal: live mode runs on
//!   [`UnavailableEvidence`], whose status carries the code and which
//!   builds nothing, so guided mode, the showcase and the static site
//!   serve while every live admission is `503 live_unavailable`, `/ready`
//!   reports `live.evidence_ok: false` and `/capabilities` reports
//!   `live_enabled: false` (ADR-0008 decision 1: the mode is disabled and
//!   says why);
//! - a snapshot that boots is loaded (its manifest compared with the pin
//!   first, when set), its verifier binary checked against its pin
//!   ([`snapshot::check_verifier_binary`], when set), and verified with
//!   `hyphae proof verify` ([`SnapshotMemoryClient::from_snapshot`]) before
//!   the server listens, and one line without a path is printed
//!   ([`SnapshotFactory::startup_line`]): it says whether the manifest and
//!   the verifier binary are pinned, with a digest prefix only.
//!
//! **Admissions and runs.** Every admission re-checks the served files in
//! full ([`SnapshotMemoryClient::check_artifacts`]) and refuses with a
//! [`FactoryError`] when they changed since boot: the gate then releases
//! the reservation and answers `503 live_unavailable` before any model
//! call. An admitted run gets a copy of the client (plain data) and a
//! FRESH verifier over the configured binary, pinned anchor and deadline,
//! bound to the boot check ([`BootBoundVerifier`]: a `Valid` stands only
//! for the digests boot accepted for the record's capture). So the run's
//! `EvidenceVerified` records carry their own verification time, the
//! verifier's content-digest cache (HY-13) never outlives a run, and files
//! swapped while a run is under way verify `Invalid`.
//!
//! **Status.** [`DeliberationFactory::status`] is the same re-check,
//! cached for [`READY_CACHE_TTL`]: `/ready` is public and anonymous, and
//! must not make the server re-hash the served artifacts (about 6.6 MB
//! with the real witnesses) at will. An admission's own re-check refreshes
//! the cache. The detail is `ok` or the snapshot error's stable code. The
//! code is logged once at startup and once on every change between ok and
//! not ok (`[snapshot] live evidence unavailable: <code>`, `[snapshot] live
//! evidence ok again`), never with a path.
//!
//! **Messages.** Configuration and snapshot errors name variables, and
//! files relative to the snapshot directory, never a secret or a file's
//! contents. One exception: `boot_verification_failed` quotes the
//! verifier's own message or stderr (bounded, on one line), which may name
//! the artifact paths the verifier was given. Those messages reach only the
//! strict entry points' errors ([`SnapshotFactory::from_env`],
//! [`SnapshotFactory::open`]); `serve-public`'s log and every public
//! surface carry the code alone.

use super::gate::{DeliberationFactory, FactoryError, FactoryStatus, SNAPSHOT_DIR_VAR};
use super::presets::PRESET_PROJECT;
use crate::api::READY_CACHE_TTL;
use crate::config::{SnapshotConfig, HYPHAE_BIN_SHA256_VAR, MANIFEST_SHA256_VAR};
use crate::hyphae::{HyphaeProofVerifier, SystemCommandRunner};
use crate::meta::MetaModelClient;
use crate::runs::Deliberation;
use crate::snapshot::{
    self, codes, BootBoundVerifier, Snapshot, SnapshotError, SnapshotMemoryClient, SnapshotStatus,
};
use consensus_domain::EvidenceMode;
use consensus_runtime::clock::{SystemClock, UuidIdGen};
use consensus_runtime::contract::quote;
use consensus_runtime::ports::ProofVerifier;
use consensus_runtime::{Orchestrator, RunLimits};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The stable `detail` codes of [`FactoryStatus`] this module adds to the
/// snapshot error codes ([`crate::snapshot::codes`]) it reports.
pub mod status {
    /// The served files are the ones the boot check accepted.
    pub const OK: &str = "ok";
    /// A snapshot error whose code is not an identifier (none is: a guard,
    /// so that nothing else can pass as a code).
    pub const ARTIFACTS_CHANGED: &str = "artifacts_changed";
}

/// Where the `[snapshot]` lines go: stderr in production, a test's
/// collector.
pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;

/// The production [`LogSink`]: one line on stderr (journald).
pub fn stderr_log() -> LogSink {
    Arc::new(|line: &str| eprintln!("{line}"))
}

/// The line logged when the live evidence becomes unavailable: the code,
/// never a path.
pub fn unavailable_line(code: &str) -> String {
    format!("[snapshot] live evidence unavailable: {code}")
}

/// The line logged when the served files are the boot-verified ones again.
pub const OK_AGAIN_LINE: &str = "[snapshot] live evidence ok again";

/// The one notice `serve-public` logs when `CONSENSUS_SNAPSHOT_DIR` is set
/// while live mode is disabled: nothing is loaded or verified then.
pub const UNUSED_NOTICE: &str = "[snapshot] CONSENSUS_SNAPSHOT_DIR is set, but public live mode is disabled (CONSENSUS_LIVE_ENABLED): the snapshot is not loaded or verified";

/// How many hex digits of the anchor, and of a pinned digest, the startup
/// line shows.
const ANCHOR_PREFIX_LEN: usize = 12;

/// The sealed snapshot as the public live gate's evidence factory: one
/// boot-verified [`SnapshotMemoryClient`] shared by every run, the
/// configuration a fresh per-run verifier is built from, and the cached
/// status of the served files.
pub struct SnapshotFactory {
    client: SnapshotMemoryClient,
    cfg: SnapshotConfig,
    /// How long a status read is reused ([`READY_CACHE_TTL`]).
    status_ttl: Duration,
    log: LogSink,
    state: Mutex<StatusState>,
}

/// The last re-check of the served files, and what was logged last.
struct StatusState {
    checked: Option<(Instant, FactoryStatus)>,
    /// Whether the evidence was ok when the last line was logged (the boot
    /// check counts: a factory exists only for a snapshot that booted).
    logged_ok: bool,
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
    /// The strict form: `Ok(None)` without `CONSENSUS_SNAPSHOT_DIR`; the
    /// configuration error (naming its variable) when a setting is missing
    /// or unusable; the boot failure (`CONSENSUS_SNAPSHOT_DIR: <code>:
    /// <message>`) when the snapshot does not load, verify or serve the
    /// presets' project. One line is printed for a snapshot that booted.
    /// `serve-public` uses [`live_evidence_from_env`](Self::live_evidence_from_env)
    /// instead, which keeps the portal up when the snapshot does not boot.
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

    /// The evidence of an enabled live mode, as `serve-public` boots it:
    /// `Ok(None)` without `CONSENSUS_SNAPSHOT_DIR` (live mode then refuses
    /// to start, [`crate::server::build_live_gate`]); a configuration error
    /// naming its variable; otherwise the booted factory (one startup line
    /// printed) or, for a snapshot that does not boot, an
    /// [`UnavailableEvidence`] with its stable code (logged once). Never
    /// an error for the snapshot itself: the portal stays up.
    pub fn live_evidence_from_env() -> Result<Option<Arc<dyn DeliberationFactory>>, String> {
        let cfg = SnapshotConfig::try_from_env().map_err(|e| e.to_string())?;
        Ok(cfg.map(|cfg| Self::boot_or_unavailable(cfg, stderr_log())))
    }

    /// [`live_evidence_from_env`](Self::live_evidence_from_env) over any
    /// variable source, logging to `log` (tests).
    pub fn live_evidence_from_vars(
        get: impl Fn(&str) -> Option<String>,
        log: LogSink,
    ) -> Result<Option<Arc<dyn DeliberationFactory>>, String> {
        let cfg = SnapshotConfig::from_vars(get).map_err(|e| e.to_string())?;
        Ok(cfg.map(|cfg| Self::boot_or_unavailable(cfg, log)))
    }

    /// Boots `cfg` with the configured verifier binary: the factory, or the
    /// unavailable evidence of a snapshot that does not boot.
    pub fn boot_or_unavailable(cfg: SnapshotConfig, log: LogSink) -> Arc<dyn DeliberationFactory> {
        let mut verifier = snapshot::verifier_from_config(&cfg, SystemCommandRunner);
        match Self::open(cfg, &mut verifier) {
            Ok(factory) => {
                println!("{}", factory.startup_line());
                Arc::new(factory.with_log(log))
            }
            Err(e) => Arc::new(UnavailableEvidence::new(&e, &log)),
        }
    }

    /// Loads the configured snapshot (`manifest_pin_mismatch` when its
    /// manifest is not the pinned one), refuses one that serves a project
    /// other than the presets' (`project_mismatch`) and a verifier binary
    /// that is not the pinned build of the capture
    /// (`verifier_binary_mismatch`, `capture_binary_mismatch`), all before
    /// any verifier run, and boot-verifies it with `verifier`
    /// ([`SnapshotMemoryClient::from_snapshot`]); the factory then builds
    /// every run over a fresh verifier of its own.
    pub fn open<V: ProofVerifier + ?Sized>(
        cfg: SnapshotConfig,
        verifier: &mut V,
    ) -> Result<Self, SnapshotError> {
        let snapshot = Snapshot::load_from_config(&cfg)?;
        if snapshot.manifest.project != PRESET_PROJECT {
            return Err(SnapshotError::new(
                codes::PROJECT_MISMATCH,
                format!(
                    "the snapshot serves project {}, but every public live preset runs against {}",
                    quote(&snapshot.manifest.project),
                    quote(PRESET_PROJECT)
                ),
            ));
        }
        snapshot::check_verifier_binary(&cfg, &snapshot.manifest)?;
        let client = SnapshotMemoryClient::from_snapshot(snapshot, &cfg.expected_anchor, verifier)?;
        Ok(Self {
            client,
            cfg,
            status_ttl: READY_CACHE_TTL,
            log: stderr_log(),
            state: Mutex::new(StatusState {
                checked: None,
                logged_ok: true,
            }),
        })
    }

    /// How long a status read is reused; [`READY_CACHE_TTL`] by default
    /// (a test sets zero to see a change at once).
    pub fn with_status_ttl(mut self, ttl: Duration) -> Self {
        self.status_ttl = ttl;
        self
    }

    /// Where the `[snapshot]` lines go ([`stderr_log`] by default).
    pub fn with_log(mut self, log: LogSink) -> Self {
        self.log = log;
        self
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
    /// prefix, whether the manifest and the verifier binary are pinned (the
    /// pinned digest's prefix, or `not pinned` with the variable that would
    /// pin it), capture and verification times. No path.
    pub fn startup_line(&self) -> String {
        let s = self.snapshot_status();
        let prefix = |digest: &str| -> String { digest.chars().take(ANCHOR_PREFIX_LEN).collect() };
        let pin = |what: &str, digest: Option<&String>, var: &str| match digest {
            Some(digest) => format!("{what} sha256 pinned {}…", prefix(digest)),
            None => format!("{what} not pinned ({var} unset)"),
        };
        format!(
            "Sealed snapshot evidence: {} (project {}, {} records, anchor {}…, {}, {}, captured {}, proofs re-verified at boot {})",
            s.snapshot_id,
            s.project,
            s.record_count,
            prefix(&s.anchor_hex),
            pin(
                "manifest",
                self.cfg.manifest_sha256.as_ref(),
                MANIFEST_SHA256_VAR
            ),
            pin(
                "verifier binary",
                self.cfg.binary_sha256.as_ref(),
                HYPHAE_BIN_SHA256_VAR
            ),
            s.captured_at,
            s.verified_at
        )
    }

    /// The verifier one admitted run gets: fresh (its own verification
    /// times and HY-13 cache), over the configured binary, pinned anchor
    /// and deadline, bound to the boot check.
    pub fn run_verifier(&self) -> BootBoundVerifier<HyphaeProofVerifier<SystemCommandRunner>> {
        self.client
            .boot_bound_verifier(snapshot::verifier_from_config(
                &self.cfg,
                SystemCommandRunner,
            ))
    }

    fn lock(&self) -> MutexGuard<'_, StatusState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-hashes the served files now, keeps the result as the cached
    /// status and logs a change between ok and not ok.
    fn check_now(&self, state: &mut StatusState) -> FactoryStatus {
        let status = match self.client.check_artifacts() {
            Ok(()) => FactoryStatus {
                ok: true,
                kind: EvidenceMode::SealedSnapshot,
                detail: Some(status::OK.to_string()),
            },
            Err(e) => FactoryStatus {
                ok: false,
                kind: EvidenceMode::SealedSnapshot,
                detail: Some(unhealthy_code(e.code)),
            },
        };
        if status.ok != state.logged_ok {
            if status.ok {
                (self.log)(OK_AGAIN_LINE);
            } else {
                (self.log)(&unavailable_line(
                    status
                        .detail
                        .as_deref()
                        .unwrap_or(status::ARTIFACTS_CHANGED),
                ));
            }
            state.logged_ok = status.ok;
        }
        state.checked = Some((Instant::now(), status.clone()));
        status
    }
}

/// The stable code for an unhealthy state: the snapshot error's code (or
/// its detail's head, `checksums_mismatch: <file>` as [`SnapshotError`]
/// prints) when it is a code, [`status::ARTIFACTS_CHANGED`] otherwise. A
/// path can never pass as a code.
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

    /// Re-checks the served files in full (never the cached status), then
    /// builds the run over a copy of the client and [`Self::run_verifier`].
    fn build(
        &self,
        model: MetaModelClient,
        limits: RunLimits,
    ) -> Result<Box<dyn Deliberation>, FactoryError> {
        let status = {
            let mut state = self.lock();
            self.check_now(&mut state)
        };
        if !status.ok {
            // The re-check above logged the code (a change from ok); the
            // gate logs this text: it does not repeat it.
            return Err(FactoryError(
                "the sealed snapshot's served files changed since boot: no run is built over them (the [snapshot] line says why)".into(),
            ));
        }
        Ok(Box::new(Orchestrator {
            model,
            memory: self.client.clone(),
            verifier: self.run_verifier(),
            clock: SystemClock,
            ids: UuidIdGen,
            limits,
        }))
    }

    /// The cached re-check of the served files ([`READY_CACHE_TTL`]).
    fn status(&self) -> FactoryStatus {
        let mut state = self.lock();
        if let Some((at, status)) = &state.checked {
            if at.elapsed() < self.status_ttl {
                return status.clone();
            }
        }
        self.check_now(&mut state)
    }
}

/// The evidence of an enabled live mode whose sealed snapshot did not boot
/// (ADR-0008 decision 1: the mode is disabled and says why, the portal
/// stays up). Its status is not ok with the boot error's stable code, it
/// builds nothing (the gate refuses every admission before Turnstile, and
/// would release the reservation if one were ever made), and it declares
/// the sealed snapshot, the only evidence a public live run may have. It
/// does not retry: the operator fixes the snapshot and restarts
/// `serve-public`.
pub struct UnavailableEvidence {
    code: String,
}

impl std::fmt::Debug for UnavailableEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnavailableEvidence")
            .field("code", &self.code)
            .finish()
    }
}

impl UnavailableEvidence {
    /// The unavailable evidence of a snapshot that failed to boot with
    /// `error`; logs its code once ([`unavailable_line`]), never the
    /// error's message.
    pub fn new(error: &SnapshotError, log: &LogSink) -> Self {
        let code = unhealthy_code(error.code);
        log(&unavailable_line(&code));
        Self { code }
    }

    /// The boot error's stable code.
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl DeliberationFactory for UnavailableEvidence {
    fn evidence_mode(&self) -> EvidenceMode {
        EvidenceMode::SealedSnapshot
    }

    fn build(
        &self,
        _model: MetaModelClient,
        _limits: RunLimits,
    ) -> Result<Box<dyn Deliberation>, FactoryError> {
        Err(FactoryError(
            "the sealed snapshot did not boot (the [snapshot] line says why): live mode is unavailable until it is fixed and serve-public restarted".into(),
        ))
    }

    fn status(&self) -> FactoryStatus {
        FactoryStatus {
            ok: false,
            kind: EvidenceMode::SealedSnapshot,
            detail: Some(self.code.clone()),
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

    #[test]
    fn a_snapshot_that_did_not_boot_is_unavailable_with_its_code_logged_once() {
        let lines: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = lines.clone();
        let log: LogSink = Arc::new(move |line: &str| sink.lock().unwrap().push(line.into()));
        let error = SnapshotError::new(
            codes::BOOT_VERIFICATION_FAILED,
            "capture list-demo-repo: the verifier reported Invalid: /srv/snap/captures/x.proof",
        );
        let evidence = UnavailableEvidence::new(&error, &log);
        assert_eq!(evidence.code(), codes::BOOT_VERIFICATION_FAILED);
        assert_eq!(evidence.evidence_mode(), EvidenceMode::SealedSnapshot);
        let status = evidence.status();
        assert!(!status.ok);
        assert_eq!(status.kind, EvidenceMode::SealedSnapshot);
        assert_eq!(
            status.detail.as_deref(),
            Some(codes::BOOT_VERIFICATION_FAILED)
        );
        // Asked again: the same answer, nothing logged again.
        assert_eq!(evidence.status(), status);
        let logged = lines.lock().unwrap().clone();
        assert_eq!(
            logged,
            ["[snapshot] live evidence unavailable: boot_verification_failed"]
        );
        assert!(!format!("{evidence:?}").contains("/srv"));
        assert_eq!(
            unavailable_line("digest_mismatch"),
            "[snapshot] live evidence unavailable: digest_mismatch"
        );
    }
}
