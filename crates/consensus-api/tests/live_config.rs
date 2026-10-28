// SPDX-License-Identifier: Apache-2.0
//! Live configuration tests: defaults, every invalid value names its
//! variable, required-when-enabled, cross-checks, secrets and test keys.

use consensus_api::live::config::{vars, ConfigError, Exposure, LiveConfig};
use consensus_api::live::judge::JudgeCodes;
use consensus_api::live::money::Usd;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Env = BTreeMap<&'static str, String>;

fn env(pairs: &[(&'static str, &str)]) -> Env {
    pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

fn load(exposure: Exposure, env: &Env) -> Result<LiveConfig, ConfigError> {
    LiveConfig::load(exposure, env)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-live-config-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, content: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
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

const REAL_SECRET: &str = "0x4AAAAAAAzzzzzzzzzzzzzzzzzzzzzzzzzz\n";

/// A complete, valid, enabled public configuration.
fn enabled_env(dir: &TempDir) -> Env {
    let secret = dir.file("turnstile.secret", REAL_SECRET);
    let ledger = dir.path().join("ledger").to_string_lossy().into_owned();
    let mut e = env(&[
        (vars::LIVE_ENABLED, "true"),
        (vars::TURNSTILE_HOSTNAME, "Demo.Example.ORG"),
    ]);
    e.insert(vars::TURNSTILE_SECRET_FILE, secret);
    e.insert(vars::LEDGER_DIR, ledger);
    e
}

#[test]
fn defaults_are_the_owner_decisions() {
    let cfg = load(Exposure::Public, &Env::new()).unwrap();
    assert!(!cfg.enabled);
    let b = &cfg.budget;
    assert_eq!(b.daily_cap, Usd::parse("10.00").unwrap());
    assert_eq!(b.lifetime_cap, Usd::parse("60.00").unwrap());
    assert_eq!(b.judge_slice, Usd::parse("15.00").unwrap());
    assert_eq!(b.max_concurrent, 1);
    assert_eq!(b.hourly_admissions, 6);
    assert_eq!(b.burst_fraction.parts_per_million(), 250_000);
    assert_eq!(b.run_token_cap, 70_000);
    assert_eq!(b.max_completion_tokens, 16_000);
    assert_eq!(b.input_cap_tokens, 8_000);
    assert_eq!(b.price_input_per_m, Usd::parse("4.00").unwrap());
    assert_eq!(b.price_output_per_m, Usd::parse("20.00").unwrap());
    assert_eq!(b.run_worst_case(), Usd::parse("1.88").unwrap());
    assert!(
        b.kill_switch_file.is_none(),
        "no ledger dir, no default kill switch"
    );
    assert!(cfg.ledger_dir.is_none());
    assert!(cfg.turnstile.is_none());
    assert!(!cfg.judge_lane_open());
    assert_eq!(cfg.exposure_days, None);
    assert_eq!(cfg.geo.portal_blocked().len(), 23);
    assert_eq!(cfg.geo.live_only_blocked().len(), 1);
}

#[test]
fn every_invalid_value_is_an_error_naming_its_variable() {
    let cases: &[(&'static str, &str)] = &[
        (vars::LIVE_ENABLED, "maybe"),
        (vars::DAILY_USD, "ten"),
        (vars::DAILY_USD, "-5"),
        (vars::DAILY_USD, "1e3"),
        (vars::LIFETIME_USD, "60,00"),
        (vars::LIFETIME_USD, "16.00"),
        (vars::JUDGE_USD, "15.0000001"),
        (vars::JUDGE_USD, "60.00"),
        (vars::JUDGE_DAILY_USD, "abc"),
        (vars::JUDGE_DAILY_USD, "15.01"),
        (vars::MAX_CONCURRENT, "0"),
        (vars::HOURLY_ADMISSIONS, "six"),
        (vars::BURST_FRACTION, "1.5"),
        (vars::BURST_FRACTION, "25%"),
        (vars::RUN_TOKEN_CAP, "0"),
        (vars::MAX_COMPLETION_TOKENS, "-1"),
        (vars::INPUT_CAP_TOKENS, "8k"),
        (vars::PRICE_INPUT_PER_M, "0"),
        (vars::PRICE_OUTPUT_PER_M, "abc"),
        (vars::TURNSTILE_HOSTNAME, "https://demo.example.org"),
        (vars::TURNSTILE_HOSTNAME, "demo.example.org/path"),
        (vars::TURNSTILE_ACTION_LIVE, "  "),
        (vars::TURNSTILE_ACTION_SESSION, ""),
        (vars::GEO_BLOCKED_COUNTRIES, "USA"),
        (vars::GEO_BLOCKED_COUNTRIES, ""),
        (vars::GEO_LIVE_ONLY_BLOCKED, "XX"),
        (vars::EXPOSURE_DAYS, "0"),
        (vars::EXPOSURE_DAYS, "seven"),
        (vars::LEDGER_DIR, " "),
        (vars::KILL_SWITCH_FILE, ""),
        (vars::JUDGE_CODES_FILE, ""),
        (vars::TURNSTILE_SECRET_FILE, ""),
    ];
    for (var, value) in cases {
        let err = load(Exposure::Public, &env(&[(var, value)]))
            .err()
            .unwrap_or_else(|| panic!("{var}={value:?} must be refused"));
        assert!(
            err.vars().contains(var),
            "{var}={value:?}: error does not name the variable: {err}"
        );
        assert!(err.to_string().contains(var), "{err}");
    }
}

#[test]
fn enabling_live_mode_requires_the_ledger_dir_secret_and_hostname() {
    let dir = TempDir::new("required");
    let full = enabled_env(&dir);
    for missing in [
        vars::LEDGER_DIR,
        vars::TURNSTILE_SECRET_FILE,
        vars::TURNSTILE_HOSTNAME,
    ] {
        let mut e = full.clone();
        e.remove(missing);
        match load(Exposure::Public, &e) {
            Err(ConfigError::Missing { var }) => assert_eq!(var, missing),
            Err(ConfigError::Inconsistent { vars, .. }) if missing == vars::TURNSTILE_HOSTNAME => {
                assert!(vars.contains(&vars::TURNSTILE_HOSTNAME));
            }
            other => panic!("without {missing}: {:?}", other.map(|_| ())),
        }
    }
    let cfg = load(Exposure::Public, &full).unwrap();
    assert!(cfg.enabled);
    let turnstile = cfg.turnstile.as_ref().expect("turnstile configured");
    assert_eq!(turnstile.hostname, "demo.example.org", "lowercased");
    assert_eq!(turnstile.action_live, "live-run");
    assert_eq!(turnstile.action_session, "session");
    assert!(!turnstile.secret.is_test_secret());
    assert_eq!(
        cfg.budget.kill_switch_file.as_deref(),
        Some(dir.path().join("ledger").join("kill-switch").as_path()),
        "the kill switch defaults to <ledger dir>/kill-switch"
    );
    assert_eq!(
        cfg.ledger_dir.as_deref(),
        Some(dir.path().join("ledger").as_path())
    );
    assert!(
        !format!("{cfg:?}").contains("0x4AAAAAAAzzzz"),
        "secrets never print"
    );
}

#[test]
fn cloudflare_test_secrets_are_refused_in_public_mode_only() {
    let dir = TempDir::new("test-secret");
    for test_secret in [
        "1x0000000000000000000000000000000AA",
        "2x0000000000000000000000000000000AA",
        "3x0000000000000000000000000000000AA",
    ] {
        let mut e = enabled_env(&dir);
        e.insert(
            vars::TURNSTILE_SECRET_FILE,
            dir.file("test.secret", &format!("{test_secret}\n")),
        );
        match load(Exposure::Public, &e) {
            Err(ConfigError::Invalid { var, reason }) => {
                assert_eq!(var, vars::TURNSTILE_SECRET_FILE);
                assert!(reason.contains("test secret"), "{reason}");
                assert!(!reason.contains(test_secret), "the value is not echoed");
            }
            other => panic!("{:?}", other.map(|_| ())),
        }
        let local = load(Exposure::Local, &e).unwrap();
        assert!(local.turnstile.unwrap().secret.is_test_secret());
    }
    // An empty secret file is refused everywhere.
    let mut e = enabled_env(&dir);
    e.insert(vars::TURNSTILE_SECRET_FILE, dir.file("empty.secret", "\n"));
    assert!(matches!(
        load(Exposure::Local, &e),
        Err(ConfigError::Invalid {
            var: vars::TURNSTILE_SECRET_FILE,
            ..
        })
    ));
    // A missing file names the variable and the path.
    let mut e = enabled_env(&dir);
    e.insert(
        vars::TURNSTILE_SECRET_FILE,
        dir.path()
            .join("nope.secret")
            .to_string_lossy()
            .into_owned(),
    );
    match load(Exposure::Public, &e) {
        Err(ConfigError::Unreadable { var, path, .. }) => {
            assert_eq!(var, vars::TURNSTILE_SECRET_FILE);
            assert!(path.ends_with("nope.secret"));
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn cross_checks_hold_the_caps_together() {
    // daily × exposure days must fit the lifetime cap.
    let err = load(Exposure::Public, &env(&[(vars::EXPOSURE_DAYS, "7")])).unwrap_err();
    match &err {
        ConfigError::Inconsistent { vars, reason } => {
            assert_eq!(
                *vars,
                vec![vars::DAILY_USD, vars::EXPOSURE_DAYS, vars::LIFETIME_USD]
            );
            assert!(reason.contains("70.00"), "{reason}");
        }
        other => panic!("{other}"),
    }
    let ok = load(Exposure::Public, &env(&[(vars::EXPOSURE_DAYS, "6")])).unwrap();
    assert_eq!(ok.exposure_days, Some(6));

    // The judge slice lives inside the lifetime cap.
    let err = load(Exposure::Public, &env(&[(vars::JUDGE_USD, "60.01")])).unwrap_err();
    assert_eq!(err.vars(), vec![vars::JUDGE_USD, vars::LIFETIME_USD]);

    // A run must fit the daily cap, or nothing could ever be admitted.
    let err = load(Exposure::Public, &env(&[(vars::DAILY_USD, "1.00")])).unwrap_err();
    assert!(err.vars().contains(&vars::DAILY_USD));
    assert!(err.to_string().contains("1.88"), "{err}");

    // ...and the public share of the lifetime cap: $60 − $58.50 = $1.50
    // would leave a public lane that refuses its first request forever.
    let err = load(Exposure::Public, &env(&[(vars::JUDGE_USD, "58.50")])).unwrap_err();
    match &err {
        ConfigError::Inconsistent {
            vars: named,
            reason,
        } => {
            assert!(named.contains(&vars::JUDGE_USD), "{named:?}");
            assert!(named.contains(&vars::LIFETIME_USD), "{named:?}");
            assert!(reason.contains("public share"), "{reason}");
            assert!(reason.contains("1.50"), "{reason}");
        }
        other => panic!("{other}"),
    }
    assert!(load(Exposure::Public, &env(&[(vars::JUDGE_USD, "58.00")])).is_ok());

    // The two widget actions must differ.
    let err = load(
        Exposure::Public,
        &env(&[
            (vars::TURNSTILE_ACTION_LIVE, "same"),
            (vars::TURNSTILE_ACTION_SESSION, "same"),
        ]),
    )
    .unwrap_err();
    assert_eq!(
        err.vars(),
        vec![vars::TURNSTILE_ACTION_LIVE, vars::TURNSTILE_ACTION_SESSION]
    );

    // A secret without a hostname (or the reverse) is refused even when
    // live mode is off: half a Turnstile configuration is a mistake.
    let dir = TempDir::new("half");
    let mut e = Env::new();
    e.insert(vars::TURNSTILE_SECRET_FILE, dir.file("s", REAL_SECRET));
    let err = load(Exposure::Public, &e).unwrap_err();
    assert!(err.vars().contains(&vars::TURNSTILE_HOSTNAME), "{err}");
    let err = load(
        Exposure::Public,
        &env(&[(vars::TURNSTILE_HOSTNAME, "demo.example.org")]),
    )
    .unwrap_err();
    assert!(err.vars().contains(&vars::TURNSTILE_SECRET_FILE), "{err}");
}

#[test]
fn judge_codes_load_as_digests_and_malformed_lines_are_never_echoed() {
    let dir = TempDir::new("judge");
    let good = dir.file(
        "judges.sha256",
        &format!(
            "# judge lane\n{}\n{}\n",
            JudgeCodes::digest_hex("judge-alpha-1234"),
            JudgeCodes::digest_hex("judge-bravo-5678")
        ),
    );
    let mut e = Env::new();
    e.insert(vars::JUDGE_CODES_FILE, good);
    let cfg = load(Exposure::Public, &e).unwrap();
    assert!(cfg.judge_lane_open());
    assert_eq!(cfg.judge_codes.len(), 2);
    assert!(cfg.judge_codes.verify("judge-alpha-1234"));
    assert!(!cfg.judge_codes.verify("judge-charlie-0000"));
    assert!(
        !format!("{cfg:?}").contains("judge-alpha"),
        "codes never print"
    );

    let bad = dir.file("judges-bad.sha256", "judge-plaintext-code-leaked\n");
    let mut e = Env::new();
    e.insert(vars::JUDGE_CODES_FILE, bad);
    let err = load(Exposure::Public, &e).unwrap_err();
    assert_eq!(err.vars(), vec![vars::JUDGE_CODES_FILE]);
    let text = err.to_string();
    assert!(text.contains("line 1"), "{text}");
    assert!(
        !text.contains("plaintext-code"),
        "the line content is not echoed: {text}"
    );

    let mut e = Env::new();
    e.insert(
        vars::JUDGE_CODES_FILE,
        dir.path()
            .join("missing.sha256")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        load(Exposure::Public, &e).unwrap_err().vars(),
        vec![vars::JUDGE_CODES_FILE]
    );

    // With codes configured, the slice must hold at least one run.
    let mut e = Env::new();
    e.insert(
        vars::JUDGE_CODES_FILE,
        dir.file("j", &JudgeCodes::digest_hex("x")),
    );
    e.insert(vars::JUDGE_USD, "1.00".into());
    let err = load(Exposure::Public, &e).unwrap_err();
    assert!(err.vars().contains(&vars::JUDGE_USD), "{err}");
    // Without codes a small slice is fine: the lane is closed anyway.
    assert!(load(Exposure::Public, &env(&[(vars::JUDGE_USD, "1.00")])).is_ok());
}

#[test]
fn the_judge_daily_cap_defaults_to_the_slice_and_stays_within_it() {
    let cfg = load(Exposure::Public, &Env::new()).unwrap();
    assert_eq!(cfg.budget.judge_daily_cap, Usd::parse("15.00").unwrap());
    let cfg = load(Exposure::Public, &env(&[(vars::JUDGE_DAILY_USD, "3.76")])).unwrap();
    assert_eq!(cfg.budget.judge_daily_cap, Usd::parse("3.76").unwrap());
    // Lowering the slice lowers the default with it.
    let cfg = load(Exposure::Public, &env(&[(vars::JUDGE_USD, "4.00")])).unwrap();
    assert_eq!(cfg.budget.judge_daily_cap, Usd::parse("4.00").unwrap());
    // Above the slice: an error naming both.
    let err = load(Exposure::Public, &env(&[(vars::JUDGE_DAILY_USD, "15.01")])).unwrap_err();
    assert_eq!(err.vars(), vec![vars::JUDGE_DAILY_USD, vars::JUDGE_USD]);
    // With codes configured, the daily cap must hold one run.
    let dir = TempDir::new("judge-daily");
    let mut e = Env::new();
    e.insert(
        vars::JUDGE_CODES_FILE,
        dir.file("j", &JudgeCodes::digest_hex("x")),
    );
    e.insert(vars::JUDGE_DAILY_USD, "1.00".into());
    let err = load(Exposure::Public, &e).unwrap_err();
    assert_eq!(
        err.vars(),
        vec![vars::JUDGE_DAILY_USD, vars::JUDGE_CODES_FILE]
    );
    e.insert(vars::JUDGE_DAILY_USD, "1.88".into());
    assert!(load(Exposure::Public, &e).is_ok());
}

#[test]
fn geo_lists_and_booleans_accept_the_documented_forms() {
    let cfg = load(
        Exposure::Public,
        &env(&[
            (vars::LIVE_ENABLED, "0"),
            (vars::GEO_BLOCKED_COUNTRIES, "ua, ru cn"),
            (vars::GEO_LIVE_ONLY_BLOCKED, "kr"),
            (vars::BURST_FRACTION, "1"),
            (vars::MAX_CONCURRENT, "2"),
        ]),
    )
    .unwrap();
    assert!(!cfg.enabled);
    let portal: Vec<&str> = cfg
        .geo
        .portal_blocked()
        .iter()
        .map(|c| c.as_str())
        .collect();
    assert_eq!(portal, vec!["CN", "RU", "UA"]);
    assert_eq!(cfg.budget.burst_fraction.parts_per_million(), 1_000_000);
    assert_eq!(cfg.budget.max_concurrent, 2);
    let on = load(Exposure::Local, &env(&[(vars::LIVE_ENABLED, "TRUE")]));
    assert!(matches!(
        on,
        Err(ConfigError::Missing {
            var: vars::LEDGER_DIR
        })
    ));
}
