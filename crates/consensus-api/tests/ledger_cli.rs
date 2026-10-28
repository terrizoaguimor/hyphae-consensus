// SPDX-License-Identifier: Apache-2.0
//! The operator's `ledger` subcommand (ADR-0007): status, reset and
//! release over a real ledger directory, next to a second handle standing
//! in for the running public server (same files, same lock). No Meta call,
//! no credential: the subcommand must work with the credential files
//! absent.

use consensus_api::ledger_cli::{run_with, LedgerCommand, MAX_BY_LEN, USAGE};
use consensus_api::live::budget::{Lane, Ledger};
use consensus_api::live::config::{vars, EnvSource, Exposure, LiveConfig};
use consensus_api::live::money::Usd;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A fresh directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-ledger-cli-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Env = BTreeMap<&'static str, String>;

fn env_for(dir: &Path) -> Env {
    BTreeMap::from([(vars::LEDGER_DIR, dir.display().to_string())])
}

/// The ledger handle the public server would hold, configured from the
/// same variables.
fn server_ledger(env: &Env) -> Ledger {
    let cfg = LiveConfig::load(Exposure::Public, env).unwrap();
    Ledger::open(cfg.ledger_dir.unwrap(), cfg.budget).unwrap()
}

struct Outcome {
    code: i32,
    out: String,
    err: String,
}

impl Outcome {
    fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("{e}: {}", self.out))
    }
}

fn cli(args: &[&str], env: &dyn EnvSource) -> Outcome {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args, env, &mut out, &mut err);
    Outcome {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// Every record of `kind` in the ledger's day files.
fn records(dir: &Path, kind: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            if record["type"] == kind {
                found.push(record);
            }
        }
    }
    found
}

fn kill_switch(dir: &Path) -> PathBuf {
    dir.join("kill-switch")
}

#[test]
fn status_lists_the_reservation_a_crash_left_open() {
    let tmp = TempDir::new("status");
    let env = env_for(tmp.path());
    let server = server_ledger(&env);
    let reservation = server.reserve(Lane::Public).unwrap();
    let id = reservation.id().as_str().to_string();
    // A crash: the run never releases, nothing in the files says it ended.
    std::mem::forget(reservation);

    let status = cli(&["status"], &env);
    assert_eq!(status.code, 0, "{}", status.err);
    let view = status.json();
    assert_eq!(view["snapshot"]["tripped"], false);
    assert_eq!(view["snapshot"]["unsettled_reservations"], 1);
    assert_eq!(view["kill_switch_present"], false);
    let open = view["open_reservations"].as_array().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["id"], id.as_str());
    assert_eq!(open[0]["lane"], "public");
    assert_eq!(open[0]["worst_case"], view["snapshot"]["run_worst_case"]);
    assert_eq!(open[0]["attempts"], 0);
}

#[test]
fn reset_clears_a_trip_and_the_server_sees_it_at_its_next_read() {
    let tmp = TempDir::new("reset");
    let env = env_for(tmp.path());
    let server = server_ledger(&env);
    std::fs::write(kill_switch(tmp.path()), b"").unwrap();
    assert!(server.snapshot().unwrap().tripped, "the kill switch trips");
    std::fs::remove_file(kill_switch(tmp.path())).unwrap();

    let reset = cli(&["reset", "--by", "mario"], &env);
    assert_eq!(reset.code, 0, "{}", reset.err);
    let view = reset.json();
    assert_eq!(view["reset"], true);
    assert_eq!(view["by"], "mario");
    assert_eq!(view["cleared"]["trip_code"], "kill_switch");
    assert_eq!(view["snapshot"]["tripped"], false);

    let recorded = records(tmp.path(), "reset");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["by"], "mario");
    // The server's own handle catches up under the lock.
    assert!(!server.snapshot().unwrap().tripped);
    assert!(server.reserve(Lane::Public).is_ok());
}

#[test]
fn reset_refuses_while_the_kill_switch_file_is_present() {
    let tmp = TempDir::new("reset-killswitch");
    let env = env_for(tmp.path());
    let server = server_ledger(&env);
    std::fs::write(kill_switch(tmp.path()), b"").unwrap();
    assert!(server.snapshot().unwrap().tripped);

    let reset = cli(&["reset", "--by", "mario"], &env);
    assert_eq!(reset.code, 2);
    assert!(reset.err.contains("kill-switch"), "{}", reset.err);
    assert!(reset.out.is_empty());
    assert!(records(tmp.path(), "reset").is_empty());
    assert!(server.snapshot().unwrap().tripped);
}

#[test]
fn reset_without_a_trip_records_nothing() {
    let tmp = TempDir::new("reset-untripped");
    let env = env_for(tmp.path());
    let _server = server_ledger(&env);

    let reset = cli(&["reset", "--by", "mario"], &env);
    assert_eq!(reset.code, 0, "{}", reset.err);
    let view = reset.json();
    assert_eq!(view["reset"], false);
    assert_eq!(view["cleared"], Value::Null);
    assert!(records(tmp.path(), "reset").is_empty());
}

#[test]
fn release_ends_a_reservation_left_by_a_crash_once() {
    let tmp = TempDir::new("release");
    let env = env_for(tmp.path());
    let server = server_ledger(&env);
    let reservation = server.reserve(Lane::Public).unwrap();
    let id = reservation.id().as_str().to_string();
    std::mem::forget(reservation);
    assert_eq!(server.snapshot().unwrap().unsettled_reservations, 1);

    // The operator may paste the id in capitals.
    let release = cli(&["release", &id.to_uppercase(), "--by", "mario"], &env);
    assert_eq!(release.code, 0, "{}", release.err);
    let view = release.json();
    assert_eq!(view["released"], id.as_str());
    assert_eq!(view["already_released"], false);
    assert_eq!(view["spent"], serde_json::to_value(Usd::ZERO).unwrap());
    assert_eq!(view["snapshot"]["unsettled_reservations"], 0);
    assert_eq!(server.snapshot().unwrap().unsettled_reservations, 0);

    let again = cli(&["release", &id, "--by", "mario"], &env);
    assert_eq!(again.code, 0, "{}", again.err);
    assert_eq!(again.json()["already_released"], true);
    let recorded = records(tmp.path(), "release");
    assert_eq!(recorded.len(), 1, "released once, recorded once");
    assert_eq!(recorded[0]["by"], "mario");
    assert_eq!(recorded[0]["reservation"], id.as_str());
}

#[test]
fn releasing_an_unknown_reservation_fails() {
    let tmp = TempDir::new("release-unknown");
    let env = env_for(tmp.path());
    let _server = server_ledger(&env);
    let release = cli(
        &[
            "release",
            "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
            "--by",
            "mario",
        ],
        &env,
    );
    assert_eq!(release.code, 2);
    assert!(
        release.err.contains("unknown reservation"),
        "{}",
        release.err
    );
    assert!(records(tmp.path(), "release").is_empty());
}

#[test]
fn arguments_are_checked_before_any_ledger_is_opened() {
    let tmp = TempDir::new("usage");
    // A directory that does not exist: a usage error must win, and nothing
    // may be created.
    let missing = tmp.path().join("absent");
    let env = env_for(&missing);
    let long = "a".repeat(MAX_BY_LEN + 1);
    for args in [
        vec![],
        vec!["bogus"],
        vec!["status", "--by", "mario"],
        vec!["status", "extra"],
        vec!["status", "--force"],
        vec!["reset"],
        vec!["reset", "--by"],
        vec!["reset", "--by", ""],
        vec!["reset", "--by", "two words"],
        vec!["reset", "--by", long.as_str()],
        vec!["reset", "now", "--by", "mario"],
        vec!["release", "--by", "mario"],
        vec!["release", "not-a-uuid", "--by", "mario"],
        vec!["release", "6f9619ff-8b86-4d11-b42d-00c04fc964ff"],
    ] {
        let outcome = cli(&args, &env);
        assert_eq!(outcome.code, 1, "{args:?}: {}", outcome.err);
        assert!(outcome.err.contains(USAGE), "{args:?}");
        assert!(outcome.out.is_empty(), "{args:?}");
    }
    assert!(!missing.exists());
    assert!(matches!(
        LedgerCommand::parse(&["status".to_string()]),
        Ok(LedgerCommand::Status)
    ));
}

#[test]
fn an_absent_ledger_directory_is_an_error_and_is_never_created() {
    let tmp = TempDir::new("absent");
    let missing = tmp.path().join("ledger").join("public");
    let outcome = cli(&["status"], &env_for(&missing));
    assert_eq!(outcome.code, 2);
    assert!(outcome.err.contains(vars::LEDGER_DIR), "{}", outcome.err);
    assert!(!missing.exists());

    let unset = cli(&["status"], &Env::new());
    assert_eq!(unset.code, 2);
    assert!(unset.err.contains(vars::LEDGER_DIR), "{}", unset.err);
}

#[test]
fn no_credential_is_read_and_the_server_caps_apply() {
    let tmp = TempDir::new("credentials");
    let mut env = env_for(tmp.path());
    // The server's environment with live mode on, but the credential files
    // are not there (they exist only inside the systemd unit): the
    // subcommand must not need them.
    env.insert(vars::LIVE_ENABLED, "true".into());
    env.insert(
        vars::TURNSTILE_SECRET_FILE,
        tmp.path()
            .join("absent-turnstile.secret")
            .display()
            .to_string(),
    );
    env.insert(
        vars::JUDGE_CODES_FILE,
        tmp.path().join("absent-judge-codes").display().to_string(),
    );
    // The rest of the deployed public.env's Turnstile settings: without the
    // hidden secret file, a hostname alone would read as inconsistent.
    env.insert(vars::TURNSTILE_HOSTNAME, "portal.example".into());
    env.insert(vars::TURNSTILE_ACTION_LIVE, "live-run".into());
    env.insert(vars::TURNSTILE_ACTION_SESSION, "session".into());
    env.insert(vars::DAILY_USD, "5.00".into());
    assert!(
        LiveConfig::load(Exposure::Public, &env).is_err(),
        "the server itself would need the files"
    );

    let status = cli(&["status"], &env);
    assert_eq!(status.code, 0, "{}", status.err);
    let daily_cap = serde_json::to_value(Usd::parse("5.00").unwrap()).unwrap();
    assert_eq!(status.json()["snapshot"]["daily_cap"], daily_cap);
}
