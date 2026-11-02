// SPDX-License-Identifier: Apache-2.0
//! Live-mode configuration from the environment.
//!
//! Every variable has either a documented default or is required when
//! live mode is enabled. An invalid value is always an error naming the
//! variable; nothing is silently replaced by a default. Secrets (the
//! Turnstile secret, the judge invite codes) are read from files named by
//! the environment, held in redacted types and never logged.
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `CONSENSUS_LIVE_ENABLED` | `false` | Public live mode on/off |
//! | `CONSENSUS_LIVE_DAILY_USD` | `10.00` | Daily cap (UTC day) |
//! | `CONSENSUS_LIVE_LIFETIME_USD` | `60.00` | Lifetime cap, judge slice included |
//! | `CONSENSUS_LIVE_JUDGE_USD` | `15.00` | Judge-lane slice inside the lifetime cap |
//! | `CONSENSUS_LIVE_JUDGE_DAILY_USD` | the judge slice | Judge reservations per UTC day (≤ the slice); a day's exposure is `daily + judge daily` |
//! | `CONSENSUS_LIVE_JUDGE_CODES_FILE` | unset (lane closed) | SHA-256 hex digests of invite codes, one per line |
//! | `CONSENSUS_LIVE_JUDGE_CODE_DAILY` | `5` | Judge-lane admissions per code per UTC day (process-local; over it the judge daily refusal) |
//! | `CONSENSUS_LIVE_LEDGER_DIR` | required when enabled | Ledger directory (`serve-operator` meters its `model=meta` runs on it too) |
//! | `CONSENSUS_LIVE_MAX_CONCURRENT` | `1` | Public live runs at once (in-process) |
//! | `CONSENSUS_LIVE_HOURLY_ADMISSIONS` | `6` | Reservations per rolling hour, per lane |
//! | `CONSENSUS_LIVE_CLIENT_DAILY` | `3` | Public-lane admissions per client key (IPv4 /32, IPv6 /64) per UTC day (process-local) |
//! | `CONSENSUS_LIVE_BURST_FRACTION` | `0.25` | Share of the daily cap available at 00:00 UTC |
//! | `CONSENSUS_LIVE_RUN_TOKEN_CAP` | `70000` | Public per-run token cap |
//! | `CONSENSUS_LIVE_MAX_COMPLETION_TOKENS` | `16000` | Output bound per call |
//! | `CONSENSUS_LIVE_INPUT_CAP_TOKENS` | `32000` | Input bound per call: a request whose `bytes + 1024` exceeds it is refused before sending (the Evaluator's request over the seven sealed-snapshot records needs well over 8,000); with the defaults a run's worst case is `(70,000 + 16,000 + 32,000) × $20/M = $2.36` |
//! | `CONSENSUS_META_PRICE_INPUT_PER_M` | `4.00` | USD per million input tokens |
//! | `CONSENSUS_META_PRICE_OUTPUT_PER_M` | `20.00` | USD per million output (and reasoning) tokens |
//! | `CONSENSUS_TURNSTILE_SECRET_FILE` | required when enabled | File holding the widget secret |
//! | `CONSENSUS_TURNSTILE_HOSTNAME` | required when enabled | Expected `hostname` in Siteverify answers |
//! | `CONSENSUS_TURNSTILE_ACTION_LIVE` | `live-run` | Expected widget action for live runs |
//! | `CONSENSUS_TURNSTILE_ACTION_SESSION` | `session` | Expected widget action for sessions |
//! | `CONSENSUS_GEO_BLOCKED_COUNTRIES` | 23 codes, see [`geo::DEFAULT_PORTAL_BLOCKED`] | Portal-wide block |
//! | `CONSENSUS_GEO_LIVE_ONLY_BLOCKED` | `KR` | Live-only block |
//! | `CONSENSUS_LIVE_KILL_SWITCH_FILE` | `<ledger dir>/kill-switch` | Its presence trips the ledger |
//! | `CONSENSUS_LIVE_EXPOSURE_DAYS` | unset | When set, `daily × days ≤ lifetime` is enforced |
//!
//! Cross-checks: `judge ≤ lifetime` and `judge daily ≤ judge`; the worst
//! case of one run must fit the daily cap and the public share of the
//! lifetime cap (`lifetime − judge`), and the judge slice and judge daily
//! cap when judge codes are configured, or nothing could ever be admitted;
//! `daily × exposure_days ≤ lifetime` when the exposure is declared. In
//! public mode a Cloudflare test secret is refused
//! ([`turnstile::TEST_SECRETS`]).
//!
//! The two prices are one table for every call (the defaults are Muse
//! Wonder's). With `CONSENSUS_MODEL_PROPOSER`, `CONSENSUS_MODEL_CRITIC` or
//! both naming another model than `CONSENSUS_MODEL`, both must be set
//! explicitly, to the highest price among the configured models;
//! `serve-public` (live mode enabled) and `serve-operator` (ledger
//! configured) refuse to start otherwise
//! ([`LiveConfig::require_prices_for_models`]).

use super::budget::BudgetConfig;
use super::geo::{self, parse_country_list, GeoPolicy};
use super::judge::JudgeCodes;
use super::money::{Ppm, Usd};
use super::turnstile::{self, TurnstileConfig, TurnstileSecret, DEFAULT_MAX_CHALLENGE_AGE};
use crate::config::MetaConfig;
use std::collections::BTreeMap;
use std::path::PathBuf;
use thiserror::Error;

/// Variable names, so callers and docs spell them once.
pub mod vars {
    pub const LIVE_ENABLED: &str = "CONSENSUS_LIVE_ENABLED";
    pub const DAILY_USD: &str = "CONSENSUS_LIVE_DAILY_USD";
    pub const LIFETIME_USD: &str = "CONSENSUS_LIVE_LIFETIME_USD";
    pub const JUDGE_USD: &str = "CONSENSUS_LIVE_JUDGE_USD";
    pub const JUDGE_DAILY_USD: &str = "CONSENSUS_LIVE_JUDGE_DAILY_USD";
    pub const JUDGE_CODES_FILE: &str = "CONSENSUS_LIVE_JUDGE_CODES_FILE";
    pub const LEDGER_DIR: &str = "CONSENSUS_LIVE_LEDGER_DIR";
    pub const MAX_CONCURRENT: &str = "CONSENSUS_LIVE_MAX_CONCURRENT";
    pub const HOURLY_ADMISSIONS: &str = "CONSENSUS_LIVE_HOURLY_ADMISSIONS";
    pub const BURST_FRACTION: &str = "CONSENSUS_LIVE_BURST_FRACTION";
    pub const RUN_TOKEN_CAP: &str = "CONSENSUS_LIVE_RUN_TOKEN_CAP";
    pub const MAX_COMPLETION_TOKENS: &str = "CONSENSUS_LIVE_MAX_COMPLETION_TOKENS";
    pub const INPUT_CAP_TOKENS: &str = "CONSENSUS_LIVE_INPUT_CAP_TOKENS";
    pub const PRICE_INPUT_PER_M: &str = "CONSENSUS_META_PRICE_INPUT_PER_M";
    pub const PRICE_OUTPUT_PER_M: &str = "CONSENSUS_META_PRICE_OUTPUT_PER_M";
    pub const TURNSTILE_SECRET_FILE: &str = "CONSENSUS_TURNSTILE_SECRET_FILE";
    pub const TURNSTILE_HOSTNAME: &str = "CONSENSUS_TURNSTILE_HOSTNAME";
    pub const TURNSTILE_ACTION_LIVE: &str = "CONSENSUS_TURNSTILE_ACTION_LIVE";
    pub const TURNSTILE_ACTION_SESSION: &str = "CONSENSUS_TURNSTILE_ACTION_SESSION";
    pub const GEO_BLOCKED_COUNTRIES: &str = "CONSENSUS_GEO_BLOCKED_COUNTRIES";
    pub const GEO_LIVE_ONLY_BLOCKED: &str = "CONSENSUS_GEO_LIVE_ONLY_BLOCKED";
    pub const KILL_SWITCH_FILE: &str = "CONSENSUS_LIVE_KILL_SWITCH_FILE";
    pub const EXPOSURE_DAYS: &str = "CONSENSUS_LIVE_EXPOSURE_DAYS";
    pub const CLIENT_DAILY: &str = "CONSENSUS_LIVE_CLIENT_DAILY";
    pub const JUDGE_CODE_DAILY: &str = "CONSENSUS_LIVE_JUDGE_CODE_DAILY";
}

/// Documented defaults, as strings parsed by the same code as the
/// environment, so the defaults are validated too.
pub mod defaults {
    pub const DAILY_USD: &str = "10.00";
    pub const LIFETIME_USD: &str = "60.00";
    pub const JUDGE_USD: &str = "15.00";
    pub const MAX_CONCURRENT: &str = "1";
    pub const HOURLY_ADMISSIONS: &str = "6";
    pub const BURST_FRACTION: &str = "0.25";
    pub const RUN_TOKEN_CAP: &str = "70000";
    pub const MAX_COMPLETION_TOKENS: &str = "16000";
    /// Room for the Evaluator's request over the sealed snapshot's seven
    /// records under the provable `bytes + 1024` bound in every round
    /// (8,000 refused every snapshot-backed run at its first evaluation).
    pub const INPUT_CAP_TOKENS: &str = "32000";
    pub const PRICE_INPUT_PER_M: &str = "4.00";
    pub const PRICE_OUTPUT_PER_M: &str = "20.00";
    pub const KILL_SWITCH_FILE_NAME: &str = "kill-switch";
    /// Public-lane admissions per client key per UTC day.
    pub const CLIENT_DAILY: &str = "3";
    /// Judge-lane admissions per code per UTC day.
    pub const JUDGE_CODE_DAILY: &str = "5";
}

/// Who can reach the server. Public mode refuses test credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    /// The public portal: test secrets are refused.
    Public,
    /// A developer's machine or a CI job: Cloudflare's test secrets are
    /// allowed so the widget can be exercised without a real site key.
    Local,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{var}: {reason}")]
    Invalid { var: &'static str, reason: String },
    #[error("{var} is required when {} is true", vars::LIVE_ENABLED)]
    Missing { var: &'static str },
    #[error("{var}: cannot read {path}: {source}")]
    Unreadable {
        var: &'static str,
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{vars:?}: {reason}")]
    Inconsistent {
        vars: Vec<&'static str>,
        reason: String,
    },
}

impl ConfigError {
    /// The variable(s) the error is about.
    pub fn vars(&self) -> Vec<&'static str> {
        match self {
            ConfigError::Invalid { var, .. }
            | ConfigError::Missing { var }
            | ConfigError::Unreadable { var, .. } => vec![var],
            ConfigError::Inconsistent { vars, .. } => vars.clone(),
        }
    }
}

/// Where variables come from: the process environment or a test map.
pub trait EnvSource {
    /// `Ok(None)` when unset; an error when the value is not Unicode.
    fn get(&self, name: &'static str) -> Result<Option<String>, ConfigError>;
}

/// The process environment.
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, name: &'static str) -> Result<Option<String>, ConfigError> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::Invalid {
                var: name,
                reason: "not valid Unicode".into(),
            }),
        }
    }
}

impl EnvSource for BTreeMap<&'static str, String> {
    fn get(&self, name: &'static str) -> Result<Option<String>, ConfigError> {
        Ok(self.get(name).cloned())
    }
}

/// The server's environment without what the ledger never needs
/// ([`LiveConfig::load_credential_free`]): the live-mode switch (so nothing
/// else becomes required), the judge-codes file, and every Turnstile
/// setting: the secret file holds a secret, and the hostname or actions
/// without it would read as an inconsistent Turnstile configuration (the
/// server's own public.env sets the hostname).
pub struct LedgerEnv<'a>(pub &'a dyn EnvSource);

impl EnvSource for LedgerEnv<'_> {
    fn get(&self, name: &'static str) -> Result<Option<String>, ConfigError> {
        match name {
            vars::LIVE_ENABLED
            | vars::JUDGE_CODES_FILE
            | vars::TURNSTILE_SECRET_FILE
            | vars::TURNSTILE_HOSTNAME
            | vars::TURNSTILE_ACTION_LIVE
            | vars::TURNSTILE_ACTION_SESSION => Ok(None),
            _ => self.0.get(name),
        }
    }
}

/// The validated live-mode configuration.
#[derive(Debug, Clone)]
pub struct LiveConfig {
    pub enabled: bool,
    pub budget: BudgetConfig,
    /// Required when enabled.
    pub ledger_dir: Option<PathBuf>,
    /// Empty when no codes file is configured (judge lane closed).
    pub judge_codes: JudgeCodes,
    /// Required when enabled.
    pub turnstile: Option<TurnstileConfig>,
    pub geo: GeoPolicy,
    pub exposure_days: Option<u32>,
    /// Public-lane admissions the gate grants one client key (IPv4 /32,
    /// IPv6 /64) per UTC day; the judge lane is exempt. Process-local.
    pub client_daily: u32,
    /// Judge-lane admissions the gate grants one judge code per UTC day.
    /// Process-local.
    pub judge_code_daily: u32,
    /// Both ledger prices were set in the environment rather than left at
    /// their defaults ([`LiveConfig::require_prices_for_models`]).
    pub prices_explicit: bool,
}

impl LiveConfig {
    /// Reads the process environment in public mode.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(Exposure::Public, &ProcessEnv)
    }

    /// Reads `env` with the given exposure; see the module docs.
    pub fn load(exposure: Exposure, env: &dyn EnvSource) -> Result<Self, ConfigError> {
        let enabled = parse_bool(vars::LIVE_ENABLED, env.get(vars::LIVE_ENABLED)?)?;
        let daily_cap = parse_usd(vars::DAILY_USD, env, defaults::DAILY_USD)?;
        let lifetime_cap = parse_usd(vars::LIFETIME_USD, env, defaults::LIFETIME_USD)?;
        let judge_slice = parse_usd(vars::JUDGE_USD, env, defaults::JUDGE_USD)?;
        // Defaults to the slice itself: judges stay admissible while the
        // public day is exhausted unless the owner bounds them per day.
        let judge_daily_cap = match env.get(vars::JUDGE_DAILY_USD)? {
            None => judge_slice,
            Some(raw) => Usd::parse(&raw).map_err(|e| ConfigError::Invalid {
                var: vars::JUDGE_DAILY_USD,
                reason: format!("must be a decimal USD amount: {e}"),
            })?,
        };
        let max_concurrent =
            parse_positive_u32(vars::MAX_CONCURRENT, env, defaults::MAX_CONCURRENT)?;
        let hourly_admissions =
            parse_positive_u32(vars::HOURLY_ADMISSIONS, env, defaults::HOURLY_ADMISSIONS)?;
        let client_daily = parse_positive_u32(vars::CLIENT_DAILY, env, defaults::CLIENT_DAILY)?;
        let judge_code_daily =
            parse_positive_u32(vars::JUDGE_CODE_DAILY, env, defaults::JUDGE_CODE_DAILY)?;
        let burst_fraction = {
            let raw = value_or(env, vars::BURST_FRACTION, defaults::BURST_FRACTION)?;
            Ppm::parse(&raw).map_err(|e| ConfigError::Invalid {
                var: vars::BURST_FRACTION,
                reason: format!("must be a decimal between 0 and 1: {e}"),
            })?
        };
        let run_token_cap = parse_positive_u64(vars::RUN_TOKEN_CAP, env, defaults::RUN_TOKEN_CAP)?;
        let max_completion_tokens = parse_positive_u64(
            vars::MAX_COMPLETION_TOKENS,
            env,
            defaults::MAX_COMPLETION_TOKENS,
        )?;
        let input_cap_tokens =
            parse_positive_u64(vars::INPUT_CAP_TOKENS, env, defaults::INPUT_CAP_TOKENS)?;
        let prices_explicit = env.get(vars::PRICE_INPUT_PER_M)?.is_some()
            && env.get(vars::PRICE_OUTPUT_PER_M)?.is_some();
        let price_input_per_m =
            parse_usd(vars::PRICE_INPUT_PER_M, env, defaults::PRICE_INPUT_PER_M)?;
        let price_output_per_m =
            parse_usd(vars::PRICE_OUTPUT_PER_M, env, defaults::PRICE_OUTPUT_PER_M)?;
        for (var, price) in [
            (vars::PRICE_INPUT_PER_M, price_input_per_m),
            (vars::PRICE_OUTPUT_PER_M, price_output_per_m),
        ] {
            if price.is_zero() {
                return Err(ConfigError::Invalid {
                    var,
                    reason: "must be greater than zero".into(),
                });
            }
        }

        let ledger_dir = parse_path(vars::LEDGER_DIR, env)?;
        if enabled && ledger_dir.is_none() {
            return Err(ConfigError::Missing {
                var: vars::LEDGER_DIR,
            });
        }
        let kill_switch_file = match parse_path(vars::KILL_SWITCH_FILE, env)? {
            Some(path) => Some(path),
            None => ledger_dir
                .as_ref()
                .map(|dir| dir.join(defaults::KILL_SWITCH_FILE_NAME)),
        };

        let budget = BudgetConfig {
            daily_cap,
            lifetime_cap,
            judge_slice,
            judge_daily_cap,
            max_concurrent,
            hourly_admissions,
            burst_fraction,
            run_token_cap,
            max_completion_tokens,
            input_cap_tokens,
            price_input_per_m,
            price_output_per_m,
            kill_switch_file,
        };

        if judge_slice > lifetime_cap {
            return Err(ConfigError::Inconsistent {
                vars: vec![vars::JUDGE_USD, vars::LIFETIME_USD],
                reason: format!(
                    "the judge slice ({judge_slice}) must not exceed the lifetime cap ({lifetime_cap})"
                ),
            });
        }
        if judge_daily_cap > judge_slice {
            return Err(ConfigError::Inconsistent {
                vars: vec![vars::JUDGE_DAILY_USD, vars::JUDGE_USD],
                reason: format!(
                    "the judge daily cap ({judge_daily_cap}) must not exceed the judge slice ({judge_slice})"
                ),
            });
        }
        let worst_case = budget.run_worst_case();
        if worst_case > daily_cap {
            return Err(ConfigError::Inconsistent {
                vars: vec![
                    vars::DAILY_USD,
                    vars::RUN_TOKEN_CAP,
                    vars::MAX_COMPLETION_TOKENS,
                    vars::INPUT_CAP_TOKENS,
                    vars::PRICE_OUTPUT_PER_M,
                ],
                reason: format!(
                    "one run's worst case ({worst_case}) exceeds the daily cap ({daily_cap}); no run could be admitted"
                ),
            });
        }
        let public_share = budget.public_share();
        if worst_case > public_share {
            return Err(ConfigError::Inconsistent {
                vars: vec![
                    vars::JUDGE_USD,
                    vars::LIFETIME_USD,
                    vars::RUN_TOKEN_CAP,
                    vars::MAX_COMPLETION_TOKENS,
                    vars::INPUT_CAP_TOKENS,
                    vars::PRICE_OUTPUT_PER_M,
                ],
                reason: format!(
                    "the public share of the lifetime cap ({lifetime_cap} − {judge_slice} judge slice = {public_share}) cannot hold one run's worst case ({worst_case}); no public run could be admitted"
                ),
            });
        }
        let exposure_days = match env.get(vars::EXPOSURE_DAYS)? {
            None => None,
            Some(raw) => Some(parse_positive_u32_value(vars::EXPOSURE_DAYS, &raw)?),
        };
        if let Some(days) = exposure_days {
            let needed = daily_cap.mul_div(u64::from(days), 1);
            if needed > lifetime_cap {
                return Err(ConfigError::Inconsistent {
                    vars: vec![vars::DAILY_USD, vars::EXPOSURE_DAYS, vars::LIFETIME_USD],
                    reason: format!(
                        "daily cap {daily_cap} × {days} days = {needed} exceeds the lifetime cap {lifetime_cap}"
                    ),
                });
            }
        }

        let judge_codes = match parse_path(vars::JUDGE_CODES_FILE, env)? {
            None => JudgeCodes::none(),
            Some(path) => JudgeCodes::load(&path).map_err(|e| ConfigError::Invalid {
                var: vars::JUDGE_CODES_FILE,
                reason: e.to_string(),
            })?,
        };
        if !judge_codes.is_empty() && worst_case > judge_slice {
            return Err(ConfigError::Inconsistent {
                vars: vec![vars::JUDGE_USD, vars::JUDGE_CODES_FILE],
                reason: format!(
                    "one run's worst case ({worst_case}) exceeds the judge slice ({judge_slice}); no judge run could be admitted"
                ),
            });
        }
        if !judge_codes.is_empty() && worst_case > judge_daily_cap {
            return Err(ConfigError::Inconsistent {
                vars: vec![vars::JUDGE_DAILY_USD, vars::JUDGE_CODES_FILE],
                reason: format!(
                    "one run's worst case ({worst_case}) exceeds the judge daily cap ({judge_daily_cap}); no judge run could be admitted"
                ),
            });
        }

        let turnstile = load_turnstile(exposure, enabled, env)?;

        let geo = GeoPolicy::new(
            parse_countries(
                vars::GEO_BLOCKED_COUNTRIES,
                env,
                geo::DEFAULT_PORTAL_BLOCKED,
            )?,
            parse_countries(
                vars::GEO_LIVE_ONLY_BLOCKED,
                env,
                geo::DEFAULT_LIVE_ONLY_BLOCKED,
            )?,
        );

        Ok(Self {
            enabled,
            budget,
            ledger_dir,
            judge_codes,
            turnstile,
            geo,
            exposure_days,
            client_daily,
            judge_code_daily,
            prices_explicit,
        })
    }

    /// The ledger has one price table, which defaults to Muse Wonder's
    /// (`4.00`/`20.00` per million tokens). When the roles do not all run
    /// on one model (`CONSENSUS_MODEL_PROPOSER`, `CONSENSUS_MODEL_CRITIC` or
    /// both, [`MetaConfig::mixes_models`]), those defaults no longer price
    /// a known model, so both [`vars::PRICE_INPUT_PER_M`] and
    /// [`vars::PRICE_OUTPUT_PER_M`] must be set explicitly (to the highest
    /// price among the configured models, so every call is over-reserved,
    /// never under-reserved). An error naming the override variables that
    /// make the mix and both price variables otherwise; one model for
    /// every role needs nothing.
    pub fn require_prices_for_models(&self, meta: &MetaConfig) -> Result<(), ConfigError> {
        let overrides = meta.overridden_model_variables();
        if overrides.is_empty() || self.prices_explicit {
            return Ok(());
        }
        let reason = format!(
            "the Proposer runs on {:?}, the Critic on {:?} and the Evaluator on {:?}, but the live ledger has one price table and its prices are the defaults; set both {} and {} explicitly to the highest price among the configured models, or unset {}",
            meta.proposer_model,
            meta.critic_model,
            meta.model,
            vars::PRICE_INPUT_PER_M,
            vars::PRICE_OUTPUT_PER_M,
            overrides.join(" and ")
        );
        let mut named = overrides;
        named.extend([vars::PRICE_INPUT_PER_M, vars::PRICE_OUTPUT_PER_M]);
        Err(ConfigError::Inconsistent {
            vars: named,
            reason,
        })
    }

    /// The configuration as the ledger's other users read it (the operator's
    /// `ledger` subcommand, `serve-operator`'s metering): through
    /// [`LedgerEnv`], in public mode. Live mode reads as disabled and no
    /// credential is read, so the server's caps apply where neither the
    /// Turnstile secret nor the judge codes are available.
    pub fn load_credential_free(env: &dyn EnvSource) -> Result<Self, ConfigError> {
        Self::load(Exposure::Public, &LedgerEnv(env))
    }

    /// True when the judge lane can admit anyone.
    pub fn judge_lane_open(&self) -> bool {
        !self.judge_codes.is_empty()
    }
}

fn load_turnstile(
    exposure: Exposure,
    enabled: bool,
    env: &dyn EnvSource,
) -> Result<Option<TurnstileConfig>, ConfigError> {
    let secret_file = parse_path(vars::TURNSTILE_SECRET_FILE, env)?;
    let hostname = env
        .get(vars::TURNSTILE_HOSTNAME)?
        .map(|h| parse_hostname(vars::TURNSTILE_HOSTNAME, &h))
        .transpose()?;
    let action_live = non_empty(
        vars::TURNSTILE_ACTION_LIVE,
        value_or(
            env,
            vars::TURNSTILE_ACTION_LIVE,
            turnstile::DEFAULT_ACTION_LIVE,
        )?,
    )?;
    let action_session = non_empty(
        vars::TURNSTILE_ACTION_SESSION,
        value_or(
            env,
            vars::TURNSTILE_ACTION_SESSION,
            turnstile::DEFAULT_ACTION_SESSION,
        )?,
    )?;
    if action_live == action_session {
        return Err(ConfigError::Inconsistent {
            vars: vec![vars::TURNSTILE_ACTION_LIVE, vars::TURNSTILE_ACTION_SESSION],
            reason: "the live and session actions must differ, or a session token could start a live run".into(),
        });
    }
    let secret = match secret_file {
        None => None,
        Some(path) => {
            let secret = TurnstileSecret::from_file(&path).map_err(|e| match e {
                turnstile::SecretError::Read { source, .. } => ConfigError::Unreadable {
                    var: vars::TURNSTILE_SECRET_FILE,
                    path: path.display().to_string(),
                    source,
                },
                other => ConfigError::Invalid {
                    var: vars::TURNSTILE_SECRET_FILE,
                    reason: other.to_string(),
                },
            })?;
            if exposure == Exposure::Public && secret.is_test_secret() {
                return Err(ConfigError::Invalid {
                    var: vars::TURNSTILE_SECRET_FILE,
                    reason: turnstile::SecretError::TestSecret.to_string(),
                });
            }
            Some(secret)
        }
    };
    match (secret, hostname) {
        (Some(secret), Some(hostname)) => Ok(Some(TurnstileConfig {
            secret,
            hostname,
            action_live,
            action_session,
            max_challenge_age: DEFAULT_MAX_CHALLENGE_AGE,
        })),
        (None, None) if !enabled => Ok(None),
        (None, _) if enabled => Err(ConfigError::Missing {
            var: vars::TURNSTILE_SECRET_FILE,
        }),
        (Some(_), None) if enabled => Err(ConfigError::Missing {
            var: vars::TURNSTILE_HOSTNAME,
        }),
        (secret, hostname) => Err(ConfigError::Inconsistent {
            vars: vec![vars::TURNSTILE_SECRET_FILE, vars::TURNSTILE_HOSTNAME],
            reason: format!(
                "set both or neither (secret file {}, hostname {})",
                if secret.is_some() { "set" } else { "unset" },
                if hostname.is_some() { "set" } else { "unset" }
            ),
        }),
    }
}

fn value_or(env: &dyn EnvSource, var: &'static str, default: &str) -> Result<String, ConfigError> {
    Ok(env.get(var)?.unwrap_or_else(|| default.to_string()))
}

fn non_empty(var: &'static str, value: String) -> Result<String, ConfigError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::Invalid {
            var,
            reason: "must not be empty".into(),
        });
    }
    Ok(trimmed.to_string())
}

fn parse_bool(var: &'static str, value: Option<String>) -> Result<bool, ConfigError> {
    match value.as_deref().map(|v| v.trim().to_ascii_lowercase()) {
        None => Ok(false),
        Some(v) if v == "true" || v == "1" => Ok(true),
        Some(v) if v == "false" || v == "0" => Ok(false),
        Some(v) => Err(ConfigError::Invalid {
            var,
            reason: format!("must be true or false, got {v:?}"),
        }),
    }
}

fn parse_usd(var: &'static str, env: &dyn EnvSource, default: &str) -> Result<Usd, ConfigError> {
    let raw = value_or(env, var, default)?;
    Usd::parse(&raw).map_err(|e| ConfigError::Invalid {
        var,
        reason: format!("must be a decimal USD amount: {e}"),
    })
}

fn parse_positive_u64(
    var: &'static str,
    env: &dyn EnvSource,
    default: &str,
) -> Result<u64, ConfigError> {
    let raw = value_or(env, var, default)?;
    raw.trim()
        .parse::<u64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| ConfigError::Invalid {
            var,
            reason: format!("must be a positive integer, got {raw:?}"),
        })
}

fn parse_positive_u32(
    var: &'static str,
    env: &dyn EnvSource,
    default: &str,
) -> Result<u32, ConfigError> {
    let raw = value_or(env, var, default)?;
    parse_positive_u32_value(var, &raw)
}

fn parse_positive_u32_value(var: &'static str, raw: &str) -> Result<u32, ConfigError> {
    raw.trim()
        .parse::<u32>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| ConfigError::Invalid {
            var,
            reason: format!("must be a positive integer, got {raw:?}"),
        })
}

fn parse_path(var: &'static str, env: &dyn EnvSource) -> Result<Option<PathBuf>, ConfigError> {
    match env.get(var)? {
        None => Ok(None),
        Some(raw) if raw.trim().is_empty() => Err(ConfigError::Invalid {
            var,
            reason: "must not be empty when set".into(),
        }),
        Some(raw) => Ok(Some(PathBuf::from(raw.trim()))),
    }
}

fn parse_hostname(var: &'static str, raw: &str) -> Result<String, ConfigError> {
    let host = raw.trim().to_ascii_lowercase();
    let valid = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..");
    if !valid {
        return Err(ConfigError::Invalid {
            var,
            reason: format!("must be a bare host name (no scheme, port or path), got {raw:?}"),
        });
    }
    Ok(host)
}

fn parse_countries(
    var: &'static str,
    env: &dyn EnvSource,
    default: &str,
) -> Result<std::collections::BTreeSet<geo::CountryCode>, ConfigError> {
    let raw = value_or(env, var, default)?;
    parse_country_list(&raw).map_err(|e| ConfigError::Invalid {
        var,
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_load_with_live_disabled() {
        let cfg = LiveConfig::load(Exposure::Public, &BTreeMap::new()).unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.budget.daily_cap, Usd::parse("10").unwrap());
        assert_eq!(cfg.budget.lifetime_cap, Usd::parse("60").unwrap());
        assert_eq!(cfg.budget.judge_slice, Usd::parse("15").unwrap());
        // (70,000 + 16,000 + 32,000) tokens × $20/M.
        assert_eq!(cfg.budget.run_worst_case(), Usd::parse("2.36").unwrap());
        // It fits what the day offers at 00:00 UTC (25% of $10).
        assert!(cfg.budget.run_worst_case() <= cfg.budget.burst());
        assert!(cfg.turnstile.is_none());
        assert!(cfg.ledger_dir.is_none());
        assert!(!cfg.judge_lane_open());
        assert_eq!(cfg.geo, GeoPolicy::default_policy());
    }

    #[test]
    fn hostnames_are_bare_and_lowercased() {
        assert_eq!(
            parse_hostname(vars::TURNSTILE_HOSTNAME, " Demo.Example.ORG ").unwrap(),
            "demo.example.org"
        );
        for bad in [
            "",
            "https://demo.example.org",
            "demo.example.org/x",
            "demo:443",
            ".demo",
            "a..b",
        ] {
            assert!(
                parse_hostname(vars::TURNSTILE_HOSTNAME, bad).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn booleans_are_strict() {
        assert!(parse_bool(vars::LIVE_ENABLED, Some("TRUE".into())).unwrap());
        assert!(!parse_bool(vars::LIVE_ENABLED, Some("0".into())).unwrap());
        assert!(!parse_bool(vars::LIVE_ENABLED, None).unwrap());
        assert!(parse_bool(vars::LIVE_ENABLED, Some("yes".into())).is_err());
    }
}
