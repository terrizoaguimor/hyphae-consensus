// SPDX-License-Identifier: Apache-2.0
//! `consensus-api ledger <status | reset | release>`: the operator's hands
//! on the live-mode USD ledger (ADR-0007), from a shell on the server.
//!
//!   consensus-api ledger status
//!   consensus-api ledger reset --by <operator>
//!   consensus-api ledger release <reservation-id> --by <operator>
//!
//! It opens the ledger the public server writes, configured from the
//! server's own environment (`CONSENSUS_LIVE_LEDGER_DIR`, the kill-switch
//! file and the budget variables) through the same [`LiveConfig::load`], so
//! its caps are the server's. It reads no credential: the live-mode switch,
//! the Turnstile secret file and the judge-codes file are hidden from that
//! load, and nothing here needs the Meta key. The ledger directory must
//! exist already: a mistyped path is an error, never a fresh, empty ledger.
//! The file lock is the server's own, so this is safe while the server
//! runs; the server reads what was appended at its next locked operation
//! (its next admission, say).
//!
//! - `status` prints the operator view as JSON: the ledger snapshot (trip
//!   reason included) and every unreleased reservation. Like every locked
//!   read it notices a kill-switch file, and trips.
//! - `reset` clears a trip, recorded with `--by`. It refuses while the
//!   kill-switch file is present (the next admission would trip again) and
//!   records nothing when there is no trip.
//! - `release` ends a reservation a crash left counting at its worst case,
//!   recorded with `--by`. Use it only for a run that is no longer active,
//!   after checking the provider dashboard: a run still drawing on a
//!   released reservation is refused its next Meta call.
//!
//! Exit codes: 0 done, 1 usage, 2 refused or failed (configuration, an
//! absent ledger directory, a ledger error, an unknown reservation).

use crate::live::budget::{Ledger, ReservationId};
use crate::live::config::{vars, EnvSource, LiveConfig, ProcessEnv};
use serde_json::json;
use std::fmt::Display;
use std::io::Write;
use std::path::Path;

pub const USAGE: &str = "usage: consensus-api ledger status
       consensus-api ledger reset --by <operator>
       consensus-api ledger release <reservation-id> --by <operator>";

/// Longest accepted `--by` value.
pub const MAX_BY_LEN: usize = 64;

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerCommand {
    Status,
    Reset { by: String },
    Release { id: ReservationId, by: String },
}

impl LedgerCommand {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let (verb, rest) = args.split_first().ok_or("a subcommand is required")?;
        let mut by = None;
        let mut positional = Vec::new();
        let mut i = 0;
        while i < rest.len() {
            match rest[i].as_str() {
                "--by" => {
                    let value = rest.get(i + 1).ok_or("--by needs the operator's name")?;
                    by = Some(parse_by(value)?);
                    i += 2;
                }
                other if other.starts_with("--") => return Err(format!("unknown option {other}")),
                other => {
                    positional.push(other.to_string());
                    i += 1;
                }
            }
        }
        match verb.as_str() {
            "status" => {
                if by.is_some() || !positional.is_empty() {
                    return Err("status takes no arguments".into());
                }
                Ok(Self::Status)
            }
            "reset" => {
                if !positional.is_empty() {
                    return Err("reset takes no positional argument".into());
                }
                let by = by.ok_or("reset needs --by <operator>")?;
                Ok(Self::Reset { by })
            }
            "release" => {
                let [id] = positional.as_slice() else {
                    return Err("release needs exactly one reservation id".into());
                };
                let id = ReservationId::parse(id)
                    .ok_or_else(|| format!("{id:?} is not a reservation id (a UUID)"))?;
                let by = by.ok_or("release needs --by <operator>")?;
                Ok(Self::Release { id, by })
            }
            other => Err(format!("unknown subcommand {other}")),
        }
    }
}

/// `--by` is written into the ledger: 1 to [`MAX_BY_LEN`] ASCII letters,
/// digits, `.`, `_`, `@` or `-`.
fn parse_by(value: &str) -> Result<String, String> {
    let ok = !value.is_empty()
        && value.len() <= MAX_BY_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'@' | b'-'));
    if ok {
        Ok(value.to_string())
    } else {
        Err(format!(
            "--by must be 1 to {MAX_BY_LEN} ASCII letters, digits, '.', '_', '@' or '-'"
        ))
    }
}

/// The ledger `cfg` names (`CONSENSUS_LIVE_LEDGER_DIR`), opened with its
/// caps: `Ok(None)` when the variable is unset; an error naming the
/// variable when the directory does not exist (a mistyped path is never a
/// fresh, empty ledger) or the ledger does not open. The subcommand and
/// `serve-operator` both open the server's ledger this way, from a
/// configuration read with [`LiveConfig::load_credential_free`].
pub fn open_configured(cfg: &LiveConfig) -> Result<Option<Ledger>, String> {
    let Some(dir) = cfg.ledger_dir.as_deref() else {
        return Ok(None);
    };
    if !dir.is_dir() {
        return Err(format!(
            "{}: no ledger directory at {}",
            vars::LEDGER_DIR,
            dir.display()
        ));
    }
    Ledger::open(dir, cfg.budget.clone())
        .map(Some)
        .map_err(|e| format!("{}: {e}", vars::LEDGER_DIR))
}

/// Runs the subcommand over the process environment; returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run_with(args, &ProcessEnv, out, err)
}

/// [`run`] over any variable source.
pub fn run_with(
    args: &[String],
    env: &dyn EnvSource,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let command = match LedgerCommand::parse(args) {
        Ok(command) => command,
        Err(e) => {
            let _ = writeln!(err, "{e}\n{USAGE}");
            return 1;
        }
    };
    match execute(&command, env, out) {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            2
        }
    }
}

fn text(e: impl Display) -> String {
    e.to_string()
}

fn execute(
    command: &LedgerCommand,
    env: &dyn EnvSource,
    out: &mut dyn Write,
) -> Result<(), String> {
    let cfg = LiveConfig::load_credential_free(env).map_err(text)?;
    let ledger =
        open_configured(&cfg)?.ok_or_else(|| format!("{} is not set", vars::LEDGER_DIR))?;
    let dir = ledger.dir().to_path_buf();
    let kill_switch = cfg.budget.kill_switch_file.clone();
    let view = match command {
        LedgerCommand::Status => {
            let snapshot = ledger.snapshot().map_err(text)?;
            let open = ledger.open_reservations().map_err(text)?;
            json!({
                "ledger_dir": dir.display().to_string(),
                "kill_switch_file": kill_switch.as_ref().map(|p| p.display().to_string()),
                "kill_switch_present": kill_switch.as_deref().is_some_and(Path::exists),
                "snapshot": snapshot,
                "open_reservations": open,
            })
        }
        LedgerCommand::Reset { by } => {
            if let Some(path) = kill_switch.as_deref().filter(|p| p.exists()) {
                return Err(format!(
                    "the kill-switch file {} is present: the next admission would trip again; remove it first",
                    path.display()
                ));
            }
            let before = ledger.snapshot().map_err(text)?;
            if before.tripped {
                ledger.reset(by).map_err(text)?;
            }
            let cleared = before.tripped.then(|| {
                json!({
                    "trip_code": before.trip_code,
                    "trip_reason": before.trip_reason,
                })
            });
            json!({
                "reset": before.tripped,
                "by": by,
                "cleared": cleared,
                "snapshot": ledger.snapshot().map_err(text)?,
            })
        }
        LedgerCommand::Release { id, by } => {
            let was_open = ledger
                .open_reservations()
                .map_err(text)?
                .iter()
                .any(|r| &r.id == id);
            let released = ledger.release_by_id(id, by).map_err(text)?;
            json!({
                "released": id,
                "by": by,
                "already_released": !was_open,
                "spent": released.spent,
                "snapshot": ledger.snapshot().map_err(text)?,
            })
        }
    };
    let rendered = serde_json::to_string_pretty(&view).map_err(text)?;
    writeln!(out, "{rendered}").map_err(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_by_value_is_bounded_and_plain() {
        assert_eq!(parse_by("mario.g@ops-1").unwrap(), "mario.g@ops-1");
        for bad in ["", "two words", "tab\there", "quote\"", "ünicode"] {
            assert!(parse_by(bad).is_err(), "{bad:?}");
        }
        assert!(parse_by(&"a".repeat(MAX_BY_LEN)).is_ok());
        assert!(parse_by(&"a".repeat(MAX_BY_LEN + 1)).is_err());
    }

    #[test]
    fn a_reservation_id_is_normalized_to_the_ledger_spelling() {
        let upper = "6F9619FF-8B86-4D11-B42D-00C04FC964FF";
        let Ok(LedgerCommand::Release { id, by }) =
            LedgerCommand::parse(&args(&["release", upper, "--by", "op"]))
        else {
            panic!("release should parse");
        };
        assert_eq!(id.as_str(), "6f9619ff-8b86-4d11-b42d-00c04fc964ff");
        assert_eq!(by, "op");
    }
}
