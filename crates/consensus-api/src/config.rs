// SPDX-License-Identifier: Apache-2.0
//! Environment configuration. No secret is ever printed, logged, or
//! written to the event log; key material stays in memory only (Meta,
//! zeroized on drop) or in a restricted file referenced by path (Hyphae).
//!
//! Settings read here, all optional unless stated:
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `CONSENSUS_META_API_KEY_FILE` | unset | File whose trimmed content is the Meta key (systemd `LoadCredential` on the server); takes precedence over every key variable below. Empty or unreadable is an error naming the variable, never the file's content |
//! | `CONSENSUS_META_API_KEY`, else `MODEL_API_KEY`, else `META_API_KEY` | (required for Meta) | Meta Model API key |
//! | `CONSENSUS_META_BASE_URL` | `https://api.meta.ai/v1` | API base (https, or http on a loopback host) |
//! | `CONSENSUS_MODEL` | `muse-wonder-1.0` | model id |
//! | `CONSENSUS_MODEL_PROPOSER` | the resolved `CONSENSUS_MODEL` | optional model id of the Proposer's calls only (`DraftProposal` and its own revision, `DraftRevision`); Critic and Evaluator keep theirs. With the live ledger, a Proposer model other than `CONSENSUS_MODEL` requires both ledger prices set explicitly ([`crate::live::config::LiveConfig::require_prices_for_models`]) |
//! | `CONSENSUS_MODEL_CRITIC` | the resolved `CONSENSUS_MODEL` | optional model id of the Critic's calls only (`DraftChallenge`); Proposer, Revise and Evaluator keep theirs. With the live ledger, a Critic model other than `CONSENSUS_MODEL` requires both ledger prices set explicitly ([`crate::live::config::LiveConfig::require_prices_for_models`]) |
//! | `CONSENSUS_MODEL_TIMEOUT_SECS` | 120 | budget of one model call: every attempt, wait and body read |
//! | `CONSENSUS_META_MAX_COMPLETION_TOKENS` | 16000 | `max_completion_tokens` sent with every call |
//! | `CONSENSUS_META_REASONING_EFFORT` | not sent | `reasoning_effort` (`minimal` .. `max`) |
//! | `CONSENSUS_META_MAX_ATTEMPTS` | 3 | HTTP attempts per call, retries included |
//! | `CONSENSUS_META_RETRY_TRANSPORT_ONCE` | true | retry once when the connection breaks after the request was sent (can bill one duplicate call) |
//! | `CONSENSUS_META_DEBUG_DIR` | unset | opt-in copies of raw model output (refused in public mode) |
//! | `CONSENSUS_PUBLIC_MODE` | false | public deployment: refuses operator-only diagnostics |
//! | `CONSENSUS_MAX_RUN_TOKENS` | runtime default | per-run token cap |
//! | `HYPHAE_NATIVE_API_KEY_FILE` | (required for Hyphae) | key file path handed to `hyphae mcp` |
//! | `HYPHAE_NATIVE_ENDPOINT` or `HYPHAE_BASE_URL` | (one required) | Hyphae daemon |
//! | `CONSENSUS_HYPHAE_BIN` | `hyphae` | Hyphae binary (or wrapper) |
//! | `CONSENSUS_HYPHAE_{PERSONAL,WORK,JOURNAL}_COLLECTION` | unset | explicit collection ids |
//! | `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` | unset | pinned proof anchor, 64 hex digits |
//! | `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS` | 60 | deadline of one `hyphae proof verify` |
//! | `CONSENSUS_SNAPSHOT_DIR` | unset | sealed snapshot directory (`manifest.json` inside, ADR-0008); with it `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` is required and must equal the manifest's anchor |
//! | `CONSENSUS_SNAPSHOT_VERIFIER_DIR` | unset | the same directory as the verifier binary sees it (a container path when `CONSENSUS_HYPHAE_BIN` runs Hyphae in a container); unset when the binary is native |
//! | `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` | unset (manifest not pinned) | SHA-256 of the snapshot's `manifest.json`, 64 hex digits; when set, a manifest with another digest does not load (`manifest_pin_mismatch`) |
//! | `CONSENSUS_HYPHAE_BIN_SHA256` | unset (verifier binary not pinned) | SHA-256 of the verifier binary, 64 hex digits; when set, `CONSENSUS_HYPHAE_BIN` must be an absolute path, the file must hash to it at boot (`verifier_binary_mismatch`) and it must be the manifest's `hyphae.binary_sha256` (`capture_binary_mismatch`) |
//!
//! A `safety_identifier` is never read from the environment: the caller
//! sets it per run ([`MetaConfig::safety_identifier`]).

use consensus_runtime::RunLimits;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::Zeroizing;

/// Per-call model timeout when `CONSENSUS_MODEL_TIMEOUT_SECS` is unset.
/// 2 of 12 live runs on 2026-10-26 hit the earlier 60 s default.
pub const DEFAULT_MODEL_TIMEOUT_SECS: u64 = 120;

/// The variable [`parse_model_timeout`] reads.
pub const MODEL_TIMEOUT_VARIABLE: &str = "CONSENSUS_MODEL_TIMEOUT_SECS";
/// Bounds of `CONSENSUS_MODEL_TIMEOUT_SECS`, in seconds.
pub const MODEL_TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 1..=3600;
/// The slowest output rate a call budget is sized for, in tokens per
/// second: the slowest answered live call up to 2026-11-01 wrote about 81
/// tokens a second (the median about 124), so a budget below
/// `max_completion_tokens / 80` seconds can time out a call that would
/// have completed.
pub const MIN_OUTPUT_TOKENS_PER_SEC: u64 = 80;

/// `CONSENSUS_MODEL_TIMEOUT_SECS` in whole seconds, 1 to 3600; unset uses
/// the default. Anything else (`180s`, `0`, `soon`) is an error naming
/// the variable: a typo never silently becomes the 120 s default.
pub fn parse_model_timeout(value: Option<&str>) -> Result<Duration, ConfigError> {
    match value {
        None => Ok(Duration::from_secs(DEFAULT_MODEL_TIMEOUT_SECS)),
        Some(v) => {
            parse_bounded(MODEL_TIMEOUT_VARIABLE, v, MODEL_TIMEOUT_RANGE).map(Duration::from_secs)
        }
    }
}

/// True when a call budget of `timeout` can hold `max_completion_tokens`
/// of output at [`MIN_OUTPUT_TOKENS_PER_SEC`]. The public server warns at
/// boot when it cannot (it still starts).
pub fn timeout_holds_output(timeout: Duration, max_completion_tokens: u64) -> bool {
    timeout.as_secs().saturating_mul(MIN_OUTPUT_TOKENS_PER_SEC) >= max_completion_tokens
}

/// Per-run limits from `CONSENSUS_MAX_RUN_TOKENS` (positive integer).
/// Unset means the runtime default; an invalid value is an error, never
/// silently replaced by the default.
pub fn run_limits_from_env() -> Result<RunLimits, String> {
    match std::env::var("CONSENSUS_MAX_RUN_TOKENS") {
        Err(std::env::VarError::NotPresent) => parse_max_run_tokens(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err("CONSENSUS_MAX_RUN_TOKENS is not valid Unicode".into())
        }
        Ok(value) => parse_max_run_tokens(Some(&value)),
    }
}

pub fn parse_max_run_tokens(value: Option<&str>) -> Result<RunLimits, String> {
    let Some(value) = value else {
        return Ok(RunLimits::default());
    };
    value
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|tokens| *tokens > 0)
        .map(|max_run_tokens| RunLimits { max_run_tokens })
        .ok_or_else(|| {
            format!("CONSENSUS_MAX_RUN_TOKENS must be a positive integer, got {value:?}")
        })
}

/// Output bound sent as `max_completion_tokens`. The largest output seen
/// in 153 live calls was 10,692 tokens (reasoning included); Meta bills
/// reasoning as output and counts it against this bound.
pub const DEFAULT_MAX_COMPLETION_TOKENS: u64 = 16_000;
/// HTTP attempts per model call, the first included.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Base of the full-jitter exponential backoff between attempts (Meta:
/// "start with a short delay such as 500 ms").
pub const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
pub const DEFAULT_META_BASE_URL: &str = "https://api.meta.ai/v1";
pub const DEFAULT_META_MODEL: &str = "muse-wonder-1.0";
/// The model id of every role's calls.
pub const MODEL_VARIABLE: &str = "CONSENSUS_MODEL";
/// The optional model id of the Critic's calls only (both overrides may be
/// set at once); unset, the Critic
/// uses the resolved [`MODEL_VARIABLE`].
pub const CRITIC_MODEL_VARIABLE: &str = "CONSENSUS_MODEL_CRITIC";
/// The optional model id of the Proposer's calls only (its proposal and
/// its revision of it); unset, the Proposer uses the resolved
/// [`MODEL_VARIABLE`].
pub const PROPOSER_MODEL_VARIABLE: &str = "CONSENSUS_MODEL_PROPOSER";
/// The key file read before any key variable: the public server gets its
/// dedicated key as a credential file (systemd `LoadCredential`), never
/// through the environment.
pub const META_KEY_FILE_VARIABLE: &str = "CONSENSUS_META_API_KEY_FILE";
/// Where the Meta key is read from when [`META_KEY_FILE_VARIABLE`] is
/// unset, first match wins. `MODEL_API_KEY` is the name Meta's docs use;
/// `META_API_KEY` comes last because Muse Code reads it too, and its
/// subscription credential is not an API key.
pub const META_KEY_VARIABLES: [&str; 3] =
    ["CONSENSUS_META_API_KEY", "MODEL_API_KEY", "META_API_KEY"];
/// Documented `reasoning_effort` values this adapter accepts. `none` is
/// documented as well but returns HTTP 400 on Muse Wonder and Muse Spark,
/// so it is refused here rather than on every call.
pub const REASONING_EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
/// Deadline of one `hyphae proof verify` when
/// `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS` is unset.
pub const DEFAULT_VERIFY_TIMEOUT_SECS: u64 = 60;

/// A setting that cannot be used, named by its environment variable (or
/// its field, for values given in code). The message never contains a
/// secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{setting}: {message}")]
pub struct ConfigError {
    pub setting: &'static str,
    pub message: String,
}

impl ConfigError {
    fn new(setting: &'static str, message: impl Into<String>) -> Self {
        Self {
            setting,
            message: message.into(),
        }
    }
}

/// A Meta Model API key. Its bytes are zeroized when it is dropped, and
/// it never prints: `Debug` shows `ApiKey(<redacted>)` and there is no
/// `Display`. Surrounding whitespace (a credential file's trailing
/// newline, say) is removed when it is made.
#[derive(Clone)]
pub struct ApiKey {
    secret: Zeroizing<String>,
    /// Where the key came from (a variable name, `credential file`,
    /// `api_key`), for configuration errors. Never the key.
    source: &'static str,
}

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_source(key, "api_key")
    }

    fn with_source(key: impl Into<String>, source: &'static str) -> Self {
        let raw = Zeroizing::new(key.into());
        Self {
            secret: Zeroizing::new(raw.trim().to_string()),
            source,
        }
    }

    /// Reads a key from a credential file (systemd `LoadCredential`, a
    /// secrets mount). The file's bytes are zeroized after the read.
    pub fn read_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        Self::read_file_as(path.as_ref(), "credential file")
    }

    /// [`read_file`](Self::read_file) whose errors (and whose later shape
    /// check) name `source`, the variable the path came from. The messages
    /// name the path and the I/O error kind, never the file's content.
    fn read_file_as(path: &Path, source: &'static str) -> Result<Self, ConfigError> {
        let bytes = Zeroizing::new(std::fs::read(path).map_err(|e| {
            ConfigError::new(
                source,
                format!("cannot read {}: {}", path.display(), e.kind()),
            )
        })?);
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            ConfigError::new(source, format!("{} is not UTF-8 text", path.display()))
        })?;
        let key = Self::with_source(text, source);
        if key.secret.is_empty() {
            return Err(ConfigError::new(
                source,
                format!("{} holds no key", path.display()),
            ));
        }
        Ok(key)
    }

    /// The key itself, for the one place that sends it.
    pub fn expose(&self) -> &str {
        &self.secret
    }

    /// Soft check against the documented shape `^LLM_\d+_\S+$`, only to
    /// turn a wrong credential into a helpful configuration error before
    /// any call; the secret's length and characters are undocumented and
    /// never checked.
    pub fn has_documented_shape(&self) -> bool {
        let Some(rest) = self.secret.strip_prefix("LLM_") else {
            return false;
        };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return false;
        }
        match rest[digits..].strip_prefix('_') {
            Some(secret) => !secret.is_empty() && !secret.chars().any(char::is_whitespace),
            None => false,
        }
    }

    fn check(&self) -> Result<(), ConfigError> {
        if self.has_documented_shape() {
            return Ok(());
        }
        Err(ConfigError::new(
            self.source,
            "this does not have the documented Meta Model API key shape LLM_<numeric id>_<secret> \
             (a Muse Code subscription credential or a consumer account sign-in is not an API key); \
             the value is not shown",
        ))
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

#[derive(Clone)]
pub struct MetaConfig {
    pub base_url: String,
    /// In-memory only, zeroized on drop; `Debug` redacts it.
    pub api_key: ApiKey,
    /// Exact Model API identity of the Evaluator's calls, and of the
    /// Proposer's (proposal and revision) and the Critic's unless
    /// [`proposer_model`](Self::proposer_model) or
    /// [`critic_model`](Self::critic_model) differs; every call records
    /// its own model in its invocation. The single-agent baseline's call
    /// always uses it.
    pub model: String,
    /// Model id of the Proposer's calls only, its proposal and its
    /// revision (`CONSENSUS_MODEL_PROPOSER`, falling back to
    /// [`model`](Self::model)). The prompts and schemas of the role do not
    /// depend on it, so neither do their digests.
    pub proposer_model: String,
    /// Model id of the Critic's calls only (`CONSENSUS_MODEL_CRITIC`,
    /// falling back to [`model`](Self::model)). The prompt and schema of
    /// the role do not depend on it, so neither do their digests.
    pub critic_model: String,
    /// Budget of one model call: every attempt, every wait between
    /// attempts and the response body read share it.
    pub timeout: Duration,
    /// HTTP attempts per call, the first included (1 = never retry).
    pub max_attempts: u32,
    /// `CONSENSUS_META_RETRY_TRANSPORT_ONCE`: retry exactly once, within
    /// `max_attempts` and the call's time budget, when the connection
    /// breaks after the request was sent (reset, EOF; no HTTP status). The
    /// lost attempt may already have been billed, so this retry can bill
    /// one duplicate generation that no invocation records (a successful
    /// retry shows as `attempts` = 2 with the answered call's usage only; a
    /// failed one surfaces only in the failure reason, "2 attempt(s)
    /// made"). The live ledger (ADR-0007, `live/budget.rs`) charges a
    /// post-send transport failure at the attempt's full worst case against
    /// the run's reservation, for public live runs and metered operator
    /// runs; an unmetered run (no ledger configured) accounts for nothing
    /// of the duplicate. Timeouts, 504 and non-retryable 4xx are never
    /// retried, whatever this is set to.
    pub retry_transport_once: bool,
    /// `max_completion_tokens` sent with every call.
    pub max_completion_tokens: u64,
    /// `reasoning_effort` sent with every call; not sent when `None`.
    pub reasoning_effort: Option<String>,
    /// `safety_identifier` sent with every call, set per run by the
    /// caller (a pseudonymous id, e.g. an HMAC of a visitor session; at
    /// most 64 printable ASCII characters). Never derived from a secret
    /// here, never read from the environment.
    pub safety_identifier: Option<String>,
    /// A public deployment: operator-only diagnostics are refused.
    pub public_mode: bool,
    /// `CONSENSUS_META_DEBUG_DIR`: keep each call's raw model output (and
    /// a failed call's response body, the key removed) for operator
    /// diagnostics. Refused in public mode.
    pub debug_dir: Option<PathBuf>,
    /// Base of the full-jitter exponential backoff between attempts.
    pub retry_base_delay: Duration,
}

impl std::fmt::Debug for MetaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("proposer_model", &self.proposer_model)
            .field("critic_model", &self.critic_model)
            .field("timeout", &self.timeout)
            .field("max_attempts", &self.max_attempts)
            .field("retry_transport_once", &self.retry_transport_once)
            .field("max_completion_tokens", &self.max_completion_tokens)
            .field("reasoning_effort", &self.reasoning_effort)
            .field(
                "safety_identifier",
                &self.safety_identifier.as_ref().map(|_| "<set>"),
            )
            .field("public_mode", &self.public_mode)
            .field("debug_dir", &self.debug_dir)
            .field("retry_base_delay", &self.retry_base_delay)
            .finish()
    }
}

/// An environment lookup: `None` for an unset or empty variable.
type Lookup<'a> = &'a dyn Fn(&'static str) -> Result<Option<String>, ConfigError>;

fn process_env(name: &'static str) -> Result<Option<String>, ConfigError> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(ConfigError::new(name, "is not valid Unicode"))
        }
    }
}

fn parse_bounded<T>(
    name: &'static str,
    value: &str,
    range: std::ops::RangeInclusive<T>,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy,
{
    value
        .trim()
        .parse::<T>()
        .ok()
        .filter(|v| range.contains(v))
        .ok_or_else(|| {
            ConfigError::new(
                name,
                format!(
                    "must be an integer from {} to {}, got {value:?}",
                    range.start(),
                    range.end()
                ),
            )
        })
}

fn parse_flag(name: &'static str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ConfigError::new(
            name,
            format!("must be true or false, got {value:?}"),
        )),
    }
}

/// True for an http(s) URL host that names this machine.
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl MetaConfig {
    /// A config from its three essential parts, every other setting at
    /// its default (adjust the public fields afterwards): for a server
    /// that reads the key from a credential file and builds one config per
    /// run. The key's shape, the base URL and the model id are checked.
    pub fn from_parts(
        base_url: impl Into<String>,
        api_key: ApiKey,
        model: impl Into<String>,
    ) -> Result<Self, ConfigError> {
        let model = model.into().trim().to_string();
        let cfg = Self {
            base_url: base_url.into().trim().trim_end_matches('/').to_string(),
            api_key,
            proposer_model: model.clone(),
            critic_model: model.clone(),
            model,
            timeout: Duration::from_secs(DEFAULT_MODEL_TIMEOUT_SECS),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            retry_transport_once: true,
            max_completion_tokens: DEFAULT_MAX_COMPLETION_TOKENS,
            reasoning_effort: None,
            safety_identifier: None,
            public_mode: false,
            debug_dir: None,
            retry_base_delay: DEFAULT_RETRY_BASE_DELAY,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// The configuration in the environment, or `None` when no key
    /// variable is set or any setting is unusable (fail closed);
    /// [`try_from_env`](Self::try_from_env) says which.
    pub fn from_env() -> Option<Self> {
        Self::try_from_env().ok().flatten()
    }

    /// The configuration in the environment: `Ok(None)` when neither the
    /// key file ([`META_KEY_FILE_VARIABLE`]) nor a key variable
    /// ([`META_KEY_VARIABLES`], first set one wins) is set, an error
    /// naming the variable when a setting is unusable (a key file that is
    /// empty or cannot be read included).
    pub fn try_from_env() -> Result<Option<Self>, ConfigError> {
        Self::from_lookup(&process_env)
    }

    /// The non-secret settings from the environment, with a key obtained
    /// elsewhere (e.g. [`ApiKey::read_file`]).
    pub fn from_env_with_key(api_key: ApiKey) -> Result<Self, ConfigError> {
        Self::settings_from(&process_env, api_key)
    }

    /// [`try_from_env`](Self::try_from_env) over any variable source
    /// (tests, a config file); an empty value counts as unset.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ConfigError> {
        let lookup =
            |name: &'static str| Ok::<_, ConfigError>(get(name).filter(|v| !v.trim().is_empty()));
        Self::from_lookup(&lookup)
    }

    fn from_lookup(get: Lookup<'_>) -> Result<Option<Self>, ConfigError> {
        // The credential file first: on the server the key never travels
        // through the environment, and a file that is set but unusable is
        // an error, never a fall-through to a key variable.
        if let Some(path) = get(META_KEY_FILE_VARIABLE)? {
            let key = ApiKey::read_file_as(Path::new(path.trim()), META_KEY_FILE_VARIABLE)?;
            return Self::settings_from(get, key).map(Some);
        }
        for name in META_KEY_VARIABLES {
            if let Some(value) = get(name)? {
                let key = ApiKey::with_source(value, name);
                return Self::settings_from(get, key).map(Some);
            }
        }
        Ok(None)
    }

    fn settings_from(get: Lookup<'_>, api_key: ApiKey) -> Result<Self, ConfigError> {
        let base_url =
            get("CONSENSUS_META_BASE_URL")?.unwrap_or_else(|| DEFAULT_META_BASE_URL.into());
        let model = get(MODEL_VARIABLE)?
            .unwrap_or_else(|| DEFAULT_META_MODEL.into())
            .trim()
            .to_string();
        // The Proposer and the Critic fall back to the resolved model id,
        // never straight to the default: without the overrides every role
        // runs on one model.
        let role_model = |variable: &'static str| -> Result<String, ConfigError> {
            Ok(match get(variable)? {
                Some(value) => value.trim().to_string(),
                None => model.clone(),
            })
        };
        let proposer_model = role_model(PROPOSER_MODEL_VARIABLE)?;
        let critic_model = role_model(CRITIC_MODEL_VARIABLE)?;
        let mut cfg = Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key,
            model,
            proposer_model,
            critic_model,
            timeout: parse_model_timeout(get("CONSENSUS_MODEL_TIMEOUT_SECS")?.as_deref())?,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            retry_transport_once: true,
            max_completion_tokens: DEFAULT_MAX_COMPLETION_TOKENS,
            reasoning_effort: None,
            safety_identifier: None,
            public_mode: false,
            debug_dir: None,
            retry_base_delay: DEFAULT_RETRY_BASE_DELAY,
        };
        if let Some(v) = get("CONSENSUS_META_MAX_COMPLETION_TOKENS")? {
            cfg.max_completion_tokens =
                parse_bounded("CONSENSUS_META_MAX_COMPLETION_TOKENS", &v, 1..=1_048_576)?;
        }
        if let Some(v) = get("CONSENSUS_META_MAX_ATTEMPTS")? {
            cfg.max_attempts = parse_bounded("CONSENSUS_META_MAX_ATTEMPTS", &v, 1..=10)?;
        }
        if let Some(v) = get("CONSENSUS_META_RETRY_TRANSPORT_ONCE")? {
            cfg.retry_transport_once = parse_flag("CONSENSUS_META_RETRY_TRANSPORT_ONCE", &v)?;
        }
        if let Some(v) = get("CONSENSUS_META_REASONING_EFFORT")? {
            cfg.reasoning_effort = Some(v.trim().to_ascii_lowercase());
        }
        if let Some(v) = get("CONSENSUS_PUBLIC_MODE")? {
            cfg.public_mode = parse_flag("CONSENSUS_PUBLIC_MODE", &v)?;
        }
        if let Some(v) = get("CONSENSUS_META_DEBUG_DIR")? {
            cfg.debug_dir = Some(PathBuf::from(v.trim()));
        }
        cfg.validate().map_err(|e| match e.setting {
            // Name the variable a field-level check came from.
            "base_url" => ConfigError::new("CONSENSUS_META_BASE_URL", e.message),
            "model" => ConfigError::new(MODEL_VARIABLE, e.message),
            "proposer_model" => ConfigError::new(PROPOSER_MODEL_VARIABLE, e.message),
            "critic_model" => ConfigError::new(CRITIC_MODEL_VARIABLE, e.message),
            "timeout" => ConfigError::new("CONSENSUS_MODEL_TIMEOUT_SECS", e.message),
            "reasoning_effort" => ConfigError::new("CONSENSUS_META_REASONING_EFFORT", e.message),
            "debug_dir" => ConfigError::new("CONSENSUS_META_DEBUG_DIR", e.message),
            _ => e,
        })?;
        Ok(cfg)
    }

    /// Every check a usable configuration passes. [`MetaModelClient::new`]
    /// runs it too, so a config edited after it was built is checked
    /// before any call.
    ///
    /// [`MetaModelClient::new`]: crate::meta::MetaModelClient::new
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.api_key.check()?;
        let url = reqwest::Url::parse(&self.base_url).map_err(|_| {
            ConfigError::new("base_url", format!("{:?} is not a URL", self.base_url))
        })?;
        let host = url.host_str().unwrap_or_default();
        let secure = url.scheme() == "https" || (url.scheme() == "http" && is_loopback_host(host));
        if !secure || host.is_empty() {
            return Err(ConfigError::new(
                "base_url",
                "must be an https URL (plain http only to a loopback host): the API key travels in every request",
            ));
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ConfigError::new(
                "base_url",
                "must not carry credentials, a query or a fragment",
            ));
        }
        for (setting, model) in [
            ("model", &self.model),
            ("proposer_model", &self.proposer_model),
            ("critic_model", &self.critic_model),
        ] {
            if model.is_empty() || model.len() > 128 || !model.chars().all(|c| c.is_ascii_graphic())
            {
                return Err(ConfigError::new(
                    setting,
                    "must be 1 to 128 printable ASCII characters",
                ));
            }
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_secs(3600) {
            return Err(ConfigError::new(
                "timeout",
                "must be from 1 to 3600 seconds",
            ));
        }
        if !(1..=10).contains(&self.max_attempts) {
            return Err(ConfigError::new("max_attempts", "must be from 1 to 10"));
        }
        if !(1..=1_048_576).contains(&self.max_completion_tokens) {
            return Err(ConfigError::new(
                "max_completion_tokens",
                "must be from 1 to 1048576",
            ));
        }
        if let Some(effort) = &self.reasoning_effort {
            if !REASONING_EFFORTS.contains(&effort.as_str()) {
                return Err(ConfigError::new(
                    "reasoning_effort",
                    format!(
                        "must be one of {} (none returns HTTP 400 on Muse Wonder and Muse Spark), got {effort:?}",
                        REASONING_EFFORTS.join(", ")
                    ),
                ));
            }
        }
        if let Some(id) = &self.safety_identifier {
            if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_graphic()) {
                return Err(ConfigError::new(
                    "safety_identifier",
                    "must be 1 to 64 printable ASCII characters (e.g. a hex HMAC of the session)",
                ));
            }
        }
        if self.public_mode && self.debug_dir.is_some() {
            return Err(ConfigError::new(
                "debug_dir",
                "refused in public mode (CONSENSUS_PUBLIC_MODE): it would keep visitors' model output on disk",
            ));
        }
        if self.retry_base_delay > Duration::from_secs(30) {
            return Err(ConfigError::new(
                "retry_base_delay",
                "must be at most 30 seconds",
            ));
        }
        Ok(())
    }

    /// The model id `role`'s calls are sent to: the Proposer's
    /// [`proposer_model`](Self::proposer_model) for its proposal and its
    /// revision (revising is the Proposer correcting its own proposal),
    /// the Critic's [`critic_model`](Self::critic_model), the Evaluator's
    /// [`model`](Self::model).
    pub fn model_for(&self, role: crate::meta::Role) -> &str {
        use crate::meta::Role;
        match role {
            Role::Propose | Role::Revise => &self.proposer_model,
            Role::Critique => &self.critic_model,
            Role::Evaluate => &self.model,
        }
    }

    /// True when the roles do not all run on one model (the Proposer, the
    /// Critic or both on another model than `CONSENSUS_MODEL`): the live
    /// ledger, which has one price table, then needs its prices set
    /// explicitly
    /// ([`crate::live::config::LiveConfig::require_prices_for_models`]).
    pub fn mixes_models(&self) -> bool {
        !self.overridden_model_variables().is_empty()
    }

    /// The per-role override variables whose model differs from
    /// [`model`](Self::model), in a fixed order (Proposer, then Critic):
    /// the variables a mixed-model configuration is made of.
    pub fn overridden_model_variables(&self) -> Vec<&'static str> {
        let mut vars = Vec::new();
        if self.proposer_model != self.model {
            vars.push(PROPOSER_MODEL_VARIABLE);
        }
        if self.critic_model != self.model {
            vars.push(CRITIC_MODEL_VARIABLE);
        }
        vars
    }
}

/// A pinned proof anchor as the verifier needs it: surrounding whitespace
/// removed, lowercase, exactly 64 hex digits (audit finding HY-10).
/// Hyphae's CLI accepts only lowercase hex, and a stray CR from a shell
/// pipeline must not silently turn every proof Invalid.
pub fn normalize_anchor(value: &str) -> Result<String, String> {
    let anchor = value.trim().to_ascii_lowercase();
    if anchor.len() == 64 && anchor.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(anchor)
    } else {
        Err(format!(
            "must be 64 hex digits (a SHA-256 anchor), got {} characters",
            value.trim().chars().count()
        ))
    }
}

/// A pinned SHA-256 digest (of the snapshot's `manifest.json`, of the
/// verifier binary) as the snapshot boot compares it: surrounding
/// whitespace removed, lowercase, exactly 64 hex digits; `what` names the
/// digest in the error.
pub fn normalize_sha256(value: &str, what: &str) -> Result<String, String> {
    let digest = value.trim().to_ascii_lowercase();
    if digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(digest)
    } else {
        Err(format!(
            "must be 64 hex digits ({what}), got {} characters",
            value.trim().chars().count()
        ))
    }
}

/// How to reach a running Hyphae MCP memory adapter. Construction of the
/// `hyphae mcp` argv follows the pinned CLI reference exactly:
/// `hyphae mcp --profile memory (--endpoint X | --base-url Y)
/// --native-api-key-file P` (never `--allow-write`).
#[derive(Debug, Clone)]
pub struct HyphaeConfig {
    pub binary: String,
    pub endpoint: Option<String>,
    pub base_url: Option<String>,
    /// Path to a restricted key file; the key itself never enters this
    /// process's config structures.
    pub key_file: String,
    /// Optional explicit Agent Memory collection ids, for deployments
    /// provisioned manually instead of via `hyphae agent setup`.
    pub personal_collection: Option<String>,
    pub work_collection: Option<String>,
    pub journal_collection: Option<String>,
    /// Operator-pinned expected proof anchor (obtained out-of-band),
    /// normalized by [`normalize_anchor`]. When set, records anchored
    /// elsewhere verify as Invalid.
    pub expected_anchor: Option<String>,
    /// Deadline of one `hyphae proof verify`; the process is killed past it.
    pub verify_timeout: Duration,
}

impl HyphaeConfig {
    /// The configuration in the environment, or `None` when it is absent
    /// or unusable (fail closed: an invalid pinned anchor is never
    /// dropped); [`try_from_env`](Self::try_from_env) says which.
    pub fn from_env() -> Option<Self> {
        Self::try_from_env().ok().flatten()
    }

    /// `Ok(None)` without a key file or a daemon address; an error naming
    /// the variable when a setting is unusable.
    pub fn try_from_env() -> Result<Option<Self>, ConfigError> {
        Self::from_lookup(&process_env)
    }

    /// [`try_from_env`](Self::try_from_env) over any variable source; an
    /// empty value counts as unset.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ConfigError> {
        let lookup =
            |name: &'static str| Ok::<_, ConfigError>(get(name).filter(|v| !v.trim().is_empty()));
        Self::from_lookup(&lookup)
    }

    fn from_lookup(get: Lookup<'_>) -> Result<Option<Self>, ConfigError> {
        let Some(key_file) = get("HYPHAE_NATIVE_API_KEY_FILE")? else {
            return Ok(None);
        };
        let endpoint = get("HYPHAE_NATIVE_ENDPOINT")?;
        let base_url = get("HYPHAE_BASE_URL")?;
        if endpoint.is_none() && base_url.is_none() {
            return Ok(None);
        }
        let expected_anchor = match get("CONSENSUS_HYPHAE_EXPECTED_ANCHOR")? {
            Some(value) => Some(
                normalize_anchor(&value)
                    .map_err(|m| ConfigError::new("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", m))?,
            ),
            None => None,
        };
        let verify_timeout = match get("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS")? {
            Some(v) => Duration::from_secs(parse_bounded(
                "CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS",
                &v,
                1..=3600,
            )?),
            None => Duration::from_secs(DEFAULT_VERIFY_TIMEOUT_SECS),
        };
        Ok(Some(Self {
            binary: get("CONSENSUS_HYPHAE_BIN")?.unwrap_or_else(|| "hyphae".into()),
            endpoint,
            base_url,
            key_file,
            personal_collection: get("CONSENSUS_HYPHAE_PERSONAL_COLLECTION")?,
            work_collection: get("CONSENSUS_HYPHAE_WORK_COLLECTION")?,
            journal_collection: get("CONSENSUS_HYPHAE_JOURNAL_COLLECTION")?,
            expected_anchor,
            verify_timeout,
        }))
    }

    /// Full argv for the read-only memory-profile MCP adapter.
    pub fn mcp_argv(&self) -> Vec<String> {
        let mut argv = vec![
            self.binary.clone(),
            "mcp".into(),
            "--profile".into(),
            "memory".into(),
        ];
        if let Some(ep) = &self.endpoint {
            argv.push("--endpoint".into());
            argv.push(ep.clone());
        } else if let Some(url) = &self.base_url {
            argv.push("--base-url".into());
            argv.push(url.clone());
        }
        argv.push("--native-api-key-file".into());
        argv.push(self.key_file.clone());
        for (flag, value) in [
            ("--personal-memory-collection", &self.personal_collection),
            ("--work-memory-collection", &self.work_collection),
            ("--journal-memory-collection", &self.journal_collection),
        ] {
            if let Some(v) = value {
                argv.push(flag.into());
                argv.push(v.clone());
            }
        }
        argv
    }
}

/// The variable pinning the SHA-256 of the snapshot's `manifest.json`.
pub const MANIFEST_SHA256_VAR: &str = "CONSENSUS_SNAPSHOT_MANIFEST_SHA256";
/// The variable pinning the SHA-256 of the verifier binary.
pub const HYPHAE_BIN_SHA256_VAR: &str = "CONSENSUS_HYPHAE_BIN_SHA256";

/// Where the public live mode's evidence comes from: a sealed snapshot
/// directory (ADR-0008, [`crate::snapshot`]). Present only when
/// `CONSENSUS_SNAPSHOT_DIR` is set; then the pinned anchor is mandatory,
/// since a snapshot is never served against an anchor it reported itself.
/// The two digest pins are optional here; `serve-public` requires both
/// with live mode enabled on the unix socket
/// ([`crate::server::require_snapshot_pins`]). Without them the manifest
/// and the verifier binary are whatever the directory and
/// `CONSENSUS_HYPHAE_BIN` hold, and the startup line says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotConfig {
    /// `CONSENSUS_SNAPSHOT_DIR`: the directory holding `manifest.json`.
    pub dir: PathBuf,
    /// `CONSENSUS_SNAPSHOT_VERIFIER_DIR`: the same directory as the
    /// verifier binary sees it, when that differs (e.g. a container path
    /// behind `scripts/hyphae-docker.cmd`); artifact paths are joined under
    /// it with `/`.
    pub verifier_dir: Option<String>,
    /// `CONSENSUS_HYPHAE_EXPECTED_ANCHOR`, normalized by
    /// [`normalize_anchor`]; boot fails unless it equals the manifest's.
    pub expected_anchor: String,
    /// `CONSENSUS_HYPHAE_BIN`: the verifier binary (or wrapper). An
    /// absolute path whenever [`binary_sha256`](Self::binary_sha256) is set.
    pub binary: String,
    /// `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS`: deadline of one verifier run.
    pub verify_timeout: Duration,
    /// `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`, normalized by
    /// [`normalize_sha256`]: when set, the loader refuses a `manifest.json`
    /// whose SHA-256 differs (`manifest_pin_mismatch`), before parsing it.
    pub manifest_sha256: Option<String>,
    /// `CONSENSUS_HYPHAE_BIN_SHA256`, normalized by [`normalize_sha256`]:
    /// when set, the file [`binary`](Self::binary) names must hash to it at
    /// boot (`verifier_binary_mismatch`) and it must be the manifest's
    /// `hyphae.binary_sha256` (`capture_binary_mismatch`), both before any
    /// verifier run ([`crate::snapshot::check_verifier_binary`]).
    pub binary_sha256: Option<String>,
}

impl SnapshotConfig {
    /// `Ok(None)` without `CONSENSUS_SNAPSHOT_DIR` (snapshot mode not
    /// configured); an error naming the variable when the pinned anchor is
    /// missing or malformed or any other setting is unusable (a digest pin
    /// that is not 64 hex digits, a `CONSENSUS_HYPHAE_BIN` that is not an
    /// absolute path while `CONSENSUS_HYPHAE_BIN_SHA256` is set). These two
    /// entry points (and [`from_vars`](Self::from_vars)) are the only ones
    /// on purpose: with the directory set, a missing or malformed
    /// `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` must abort boot, never read as
    /// "no snapshot configured" (ADR-0008 decision 1: a snapshot that fails
    /// its check disables the mode and says why; no silent fallback), so
    /// there is no accessor that folds the error into `None`.
    pub fn try_from_env() -> Result<Option<Self>, ConfigError> {
        Self::from_lookup(&process_env)
    }

    /// [`try_from_env`](Self::try_from_env) over any variable source; an
    /// empty value counts as unset.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ConfigError> {
        let lookup =
            |name: &'static str| Ok::<_, ConfigError>(get(name).filter(|v| !v.trim().is_empty()));
        Self::from_lookup(&lookup)
    }

    fn from_lookup(get: Lookup<'_>) -> Result<Option<Self>, ConfigError> {
        let Some(dir) = get("CONSENSUS_SNAPSHOT_DIR")? else {
            return Ok(None);
        };
        let expected_anchor = match get("CONSENSUS_HYPHAE_EXPECTED_ANCHOR")? {
            Some(value) => normalize_anchor(&value)
                .map_err(|m| ConfigError::new("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", m))?,
            None => {
                return Err(ConfigError::new(
                    "CONSENSUS_HYPHAE_EXPECTED_ANCHOR",
                    "is required with CONSENSUS_SNAPSHOT_DIR: a sealed snapshot is only served against the operator-pinned anchor",
                ))
            }
        };
        let verify_timeout = match get("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS")? {
            Some(v) => Duration::from_secs(parse_bounded(
                "CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS",
                &v,
                1..=3600,
            )?),
            None => Duration::from_secs(DEFAULT_VERIFY_TIMEOUT_SECS),
        };
        let pinned = |name: &'static str, what: &str| -> Result<Option<String>, ConfigError> {
            get(name)?
                .map(|value| normalize_sha256(&value, what).map_err(|m| ConfigError::new(name, m)))
                .transpose()
        };
        let manifest_sha256 = pinned(
            MANIFEST_SHA256_VAR,
            "the SHA-256 of the snapshot's manifest.json",
        )?;
        let binary_sha256 = pinned(HYPHAE_BIN_SHA256_VAR, "the SHA-256 of the verifier binary")?;
        let binary = get("CONSENSUS_HYPHAE_BIN")?.unwrap_or_else(|| "hyphae".into());
        // The pinned digest is checked against one file: a name looked up on
        // PATH (or a path relative to the working directory) could run
        // another one than the file hashed.
        if binary_sha256.is_some() && !Path::new(&binary).is_absolute() {
            return Err(ConfigError::new(
                "CONSENSUS_HYPHAE_BIN",
                format!(
                    "must be an absolute path when {HYPHAE_BIN_SHA256_VAR} is set: the pinned digest is checked against that one file, and a name looked up on PATH or a relative path could run another one"
                ),
            ));
        }
        Ok(Some(Self {
            dir: PathBuf::from(dir.trim()),
            verifier_dir: get("CONSENSUS_SNAPSHOT_VERIFIER_DIR")?.map(|v| v.trim().to_string()),
            expected_anchor,
            binary,
            verify_timeout,
            manifest_sha256,
            binary_sha256,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn model_timeout_defaults_to_120_seconds_and_keeps_the_override() {
        assert_eq!(parse_model_timeout(None), Ok(Duration::from_secs(120)));
        assert_eq!(parse_model_timeout(Some("45")), Ok(Duration::from_secs(45)));
        assert_eq!(
            parse_model_timeout(Some(" 240 ")),
            Ok(Duration::from_secs(240))
        );
        assert_eq!(
            parse_model_timeout(Some("3600")),
            Ok(Duration::from_secs(3600))
        );
    }

    #[test]
    fn an_unusable_model_timeout_is_an_error_naming_the_variable() {
        // Before: "180s" silently fell back to the 120 s default.
        for bad in ["180s", "0", "-5", "soon", "3601", "2.5"] {
            let err = parse_model_timeout(Some(bad)).unwrap_err();
            assert_eq!(err.setting, "CONSENSUS_MODEL_TIMEOUT_SECS", "{bad}");
            assert!(err.message.contains("1 to 3600"), "{bad}: {err}");
        }
        // Through the whole configuration: the server refuses to boot.
        let err = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL_TIMEOUT_SECS", "180s"),
        ])
        .unwrap_err();
        assert_eq!(err.setting, "CONSENSUS_MODEL_TIMEOUT_SECS");
        assert!(!err.to_string().contains("nx9-test-secret"), "{err}");
        let cfg = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL_TIMEOUT_SECS", "240"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cfg.timeout, Duration::from_secs(240));
        let cfg = meta(&[("MODEL_API_KEY", KEY)]).unwrap().unwrap();
        assert_eq!(cfg.timeout, Duration::from_secs(120));
    }

    #[test]
    fn the_timeout_must_hold_the_output_bound_at_80_tokens_a_second() {
        // 16,000 tokens at 80/s need 200 s.
        assert!(!timeout_holds_output(Duration::from_secs(120), 16_000));
        assert!(!timeout_holds_output(Duration::from_secs(199), 16_000));
        assert!(timeout_holds_output(Duration::from_secs(200), 16_000));
        assert!(timeout_holds_output(Duration::from_secs(240), 16_000));
    }

    #[test]
    fn run_token_cap_is_configurable_and_never_silently_defaulted() {
        assert_eq!(parse_max_run_tokens(None), Ok(RunLimits::default()));
        assert_eq!(
            parse_max_run_tokens(Some(" 5000 ")).map(|l| l.max_run_tokens),
            Ok(5000)
        );
        for bad in ["0", "-1", "lots", ""] {
            let err = parse_max_run_tokens(Some(bad)).unwrap_err();
            assert!(err.contains("CONSENSUS_MAX_RUN_TOKENS"), "{err}");
        }
    }

    const KEY: &str = "LLM_1607358788850350_nx9-test-secret-LJY";

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn meta(pairs: &[(&str, &str)]) -> Result<Option<MetaConfig>, ConfigError> {
        let map = vars(pairs);
        MetaConfig::from_vars(|name| map.get(name).cloned())
    }

    #[test]
    fn meta_key_precedence_is_consensus_then_model_then_meta() {
        assert!(meta(&[]).unwrap().is_none(), "no key, no config");
        let key =
            |pairs: &[(&str, &str)]| meta(pairs).unwrap().unwrap().api_key.expose().to_string();
        assert_eq!(
            key(&[
                ("META_API_KEY", "LLM_3_meta"),
                ("MODEL_API_KEY", "LLM_2_model"),
                ("CONSENSUS_META_API_KEY", "LLM_1_consensus"),
            ]),
            "LLM_1_consensus"
        );
        assert_eq!(
            key(&[
                ("META_API_KEY", "LLM_3_meta"),
                ("MODEL_API_KEY", "LLM_2_model")
            ]),
            "LLM_2_model"
        );
        assert_eq!(key(&[("META_API_KEY", "LLM_3_meta")]), "LLM_3_meta");
        // An empty variable is unset: the next one is used.
        assert_eq!(
            key(&[
                ("CONSENSUS_META_API_KEY", "  "),
                ("META_API_KEY", "LLM_3_meta")
            ]),
            "LLM_3_meta"
        );
    }

    #[test]
    fn meta_defaults_follow_the_documented_request_parameters() {
        let cfg = meta(&[("MODEL_API_KEY", KEY)]).unwrap().unwrap();
        assert_eq!(cfg.base_url, "https://api.meta.ai/v1");
        assert_eq!(cfg.model, "muse-wonder-1.0");
        assert_eq!(cfg.timeout, Duration::from_secs(120));
        assert_eq!(cfg.max_attempts, 3);
        assert!(
            cfg.retry_transport_once,
            "one post-send transport retry by default (live R06)"
        );
        assert_eq!(cfg.max_completion_tokens, 16_000);
        assert_eq!(cfg.reasoning_effort, None, "not sent by default");
        assert_eq!(
            cfg.safety_identifier, None,
            "never read from the environment"
        );
        assert!(!cfg.public_mode);
        assert_eq!(cfg.debug_dir, None);
        assert_eq!(cfg.retry_base_delay, Duration::from_millis(500));

        let cfg = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_META_MAX_COMPLETION_TOKENS", "32"),
            ("CONSENSUS_META_MAX_ATTEMPTS", "1"),
            ("CONSENSUS_META_RETRY_TRANSPORT_ONCE", " off "),
            ("CONSENSUS_META_REASONING_EFFORT", " High "),
            ("CONSENSUS_META_BASE_URL", "https://gateway.example/v1/"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cfg.max_completion_tokens, 32);
        assert_eq!(cfg.max_attempts, 1);
        assert!(!cfg.retry_transport_once);
        assert_eq!(cfg.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(cfg.base_url, "https://gateway.example/v1");
        for on in ["1", "true", "yes", "on"] {
            let cfg = meta(&[
                ("MODEL_API_KEY", KEY),
                ("CONSENSUS_META_RETRY_TRANSPORT_ONCE", on),
            ])
            .unwrap()
            .unwrap();
            assert!(cfg.retry_transport_once, "{on}");
        }
    }

    #[test]
    fn unusable_meta_settings_are_errors_naming_the_variable() {
        for (pairs, variable) in [
            (
                vec![("MODEL_API_KEY", "sk-not-a-meta-key")],
                "MODEL_API_KEY",
            ),
            (
                vec![("CONSENSUS_META_API_KEY", "LLM_12_has space")],
                "CONSENSUS_META_API_KEY",
            ),
            (vec![("META_API_KEY", "LLM__nodigits")], "META_API_KEY"),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_META_MAX_COMPLETION_TOKENS", "0"),
                ],
                "CONSENSUS_META_MAX_COMPLETION_TOKENS",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_META_MAX_ATTEMPTS", "lots"),
                ],
                "CONSENSUS_META_MAX_ATTEMPTS",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_META_REASONING_EFFORT", "none"),
                ],
                "CONSENSUS_META_REASONING_EFFORT",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_META_BASE_URL", "http://api.meta.ai/v1"),
                ],
                "CONSENSUS_META_BASE_URL",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_META_BASE_URL", "https://user:pw@api.meta.ai/v1"),
                ],
                "CONSENSUS_META_BASE_URL",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_MODEL_TIMEOUT_SECS", "0"),
                ],
                "CONSENSUS_MODEL_TIMEOUT_SECS",
            ),
            (
                vec![("MODEL_API_KEY", KEY), ("CONSENSUS_PUBLIC_MODE", "maybe")],
                "CONSENSUS_PUBLIC_MODE",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_META_RETRY_TRANSPORT_ONCE", "once"),
                ],
                "CONSENSUS_META_RETRY_TRANSPORT_ONCE",
            ),
        ] {
            let err = meta(&pairs).unwrap_err();
            assert_eq!(err.setting, variable, "{err}");
            for (_, value) in &pairs {
                if value.starts_with("LLM_") || value.starts_with("sk-") {
                    assert!(!err.to_string().contains(value), "a key leaked: {err}");
                }
            }
        }
        // The key-shape error is a helpful one.
        let err = meta(&[("MODEL_API_KEY", "sk-not-a-meta-key")]).unwrap_err();
        assert!(err.message.contains("LLM_<numeric id>_<secret>"), "{err}");
        // A plain-http base is fine only on this machine.
        for local in [
            "http://127.0.0.1:8080/v1",
            "http://localhost/v1",
            "http://[::1]:9/v1",
        ] {
            assert!(
                meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_META_BASE_URL", local)]).is_ok(),
                "{local}"
            );
        }
    }

    #[test]
    fn the_critic_model_falls_back_to_the_model_then_to_the_default() {
        use crate::meta::Role;
        // Neither set: every role on muse-wonder-1.0.
        let cfg = meta(&[("MODEL_API_KEY", KEY)]).unwrap().unwrap();
        assert_eq!(cfg.critic_model, "muse-wonder-1.0");
        assert!(!cfg.mixes_models());
        for role in Role::ALL {
            assert_eq!(cfg.model_for(role), "muse-wonder-1.0", "{role:?}");
        }
        // CONSENSUS_MODEL only: every role on it, the Critic included.
        let cfg = meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL", " muse-x-2 ")])
            .unwrap()
            .unwrap();
        assert_eq!(cfg.critic_model, "muse-x-2");
        assert!(!cfg.mixes_models());
        for role in Role::ALL {
            assert_eq!(cfg.model_for(role), "muse-x-2", "{role:?}");
        }
        // CONSENSUS_MODEL_CRITIC changes the Critic's calls only, with or
        // without CONSENSUS_MODEL.
        for (pairs, others) in [
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_MODEL_CRITIC", " muse-spark-1.3\n"),
                ],
                "muse-wonder-1.0",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_MODEL", "muse-x-2"),
                    ("CONSENSUS_MODEL_CRITIC", "muse-spark-1.3"),
                ],
                "muse-x-2",
            ),
        ] {
            let cfg = meta(&pairs).unwrap().unwrap();
            assert_eq!(cfg.model, others);
            assert_eq!(cfg.critic_model, "muse-spark-1.3");
            assert!(cfg.mixes_models());
            assert_eq!(cfg.model_for(Role::Critique), "muse-spark-1.3");
            for role in [Role::Propose, Role::Evaluate, Role::Revise] {
                assert_eq!(cfg.model_for(role), others, "{role:?}");
            }
        }
        // An empty override is unset, like every other variable.
        let cfg = meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL_CRITIC", "  ")])
            .unwrap()
            .unwrap();
        assert_eq!(cfg.critic_model, "muse-wonder-1.0");
        // from_parts: one model for every role.
        let cfg = MetaConfig::from_parts(DEFAULT_META_BASE_URL, ApiKey::new(KEY), " m-1 ").unwrap();
        assert_eq!(
            (cfg.model.as_str(), cfg.critic_model.as_str()),
            ("m-1", "m-1")
        );
    }

    #[test]
    fn the_proposer_model_covers_propose_and_revise_and_falls_back_like_the_critics() {
        use crate::meta::Role;
        // Unset: the Proposer follows the resolved CONSENSUS_MODEL.
        let cfg = meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL", "muse-x-2")])
            .unwrap()
            .unwrap();
        assert_eq!(cfg.proposer_model, "muse-x-2");
        assert!(cfg.overridden_model_variables().is_empty());
        let cfg = meta(&[("MODEL_API_KEY", KEY)]).unwrap().unwrap();
        assert_eq!(cfg.proposer_model, "muse-wonder-1.0");
        // An empty override is unset.
        let cfg = meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL_PROPOSER", " \t")])
            .unwrap()
            .unwrap();
        assert_eq!(cfg.proposer_model, "muse-wonder-1.0");
        assert!(!cfg.mixes_models());

        // CONSENSUS_MODEL_PROPOSER moves the proposal and the revision only
        // (the owner's 2026-11-01 split: Proposer on Spark, Critic and
        // Evaluator on Wonder), with or without CONSENSUS_MODEL.
        for (pairs, others) in [
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_MODEL_PROPOSER", " muse-spark-1.3\n"),
                ],
                "muse-wonder-1.0",
            ),
            (
                vec![
                    ("MODEL_API_KEY", KEY),
                    ("CONSENSUS_MODEL", "muse-x-2"),
                    ("CONSENSUS_MODEL_PROPOSER", "muse-spark-1.3"),
                ],
                "muse-x-2",
            ),
        ] {
            let cfg = meta(&pairs).unwrap().unwrap();
            assert_eq!(cfg.model, others);
            assert_eq!(cfg.proposer_model, "muse-spark-1.3");
            assert_eq!(cfg.critic_model, others);
            assert!(cfg.mixes_models());
            assert_eq!(
                cfg.overridden_model_variables(),
                ["CONSENSUS_MODEL_PROPOSER"]
            );
            for role in [Role::Propose, Role::Revise] {
                assert_eq!(cfg.model_for(role), "muse-spark-1.3", "{role:?}");
            }
            for role in [Role::Critique, Role::Evaluate] {
                assert_eq!(cfg.model_for(role), others, "{role:?}");
            }
        }

        // Both overrides at once: each role on its own model.
        let cfg = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL", "muse-x-2"),
            ("CONSENSUS_MODEL_PROPOSER", "muse-spark-1.3"),
            ("CONSENSUS_MODEL_CRITIC", "muse-wonder-1.0"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cfg.model_for(Role::Propose), "muse-spark-1.3");
        assert_eq!(cfg.model_for(Role::Revise), "muse-spark-1.3");
        assert_eq!(cfg.model_for(Role::Critique), "muse-wonder-1.0");
        assert_eq!(cfg.model_for(Role::Evaluate), "muse-x-2");
        assert_eq!(
            cfg.overridden_model_variables(),
            ["CONSENSUS_MODEL_PROPOSER", "CONSENSUS_MODEL_CRITIC"]
        );
        // Overrides equal to CONSENSUS_MODEL are one model.
        let cfg = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL_PROPOSER", "muse-wonder-1.0"),
            ("CONSENSUS_MODEL_CRITIC", "muse-wonder-1.0"),
        ])
        .unwrap()
        .unwrap();
        assert!(!cfg.mixes_models());
        // from_parts: one model for every role, the Proposer included.
        let cfg = MetaConfig::from_parts(DEFAULT_META_BASE_URL, ApiKey::new(KEY), " m-1 ").unwrap();
        assert_eq!(cfg.proposer_model, "m-1");
    }

    #[test]
    fn an_unusable_proposer_model_id_names_its_variable() {
        let long = "m".repeat(129);
        for value in ["has space", "\u{e9}t\u{e9}", long.as_str()] {
            let err =
                meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL_PROPOSER", value)]).unwrap_err();
            assert_eq!(err.setting, "CONSENSUS_MODEL_PROPOSER", "{value:?}: {err}");
            assert!(err.message.contains("printable ASCII"), "{err}");
            // A good Critic override does not hide it.
            let err = meta(&[
                ("MODEL_API_KEY", KEY),
                ("CONSENSUS_MODEL_PROPOSER", value),
                ("CONSENSUS_MODEL_CRITIC", "muse-wonder-1.0"),
            ])
            .unwrap_err();
            assert_eq!(err.setting, "CONSENSUS_MODEL_PROPOSER", "{value:?}: {err}");
        }
        let mut cfg =
            MetaConfig::from_parts(DEFAULT_META_BASE_URL, ApiKey::new(KEY), DEFAULT_META_MODEL)
                .unwrap();
        cfg.proposer_model = String::new();
        assert_eq!(cfg.validate().unwrap_err().setting, "proposer_model");
    }

    #[test]
    fn an_unusable_model_id_names_its_own_variable() {
        let long = "m".repeat(129);
        for value in ["has space", "\u{e9}t\u{e9}", long.as_str()] {
            let err =
                meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL_CRITIC", value)]).unwrap_err();
            assert_eq!(err.setting, "CONSENSUS_MODEL_CRITIC", "{value:?}: {err}");
            assert!(err.message.contains("printable ASCII"), "{err}");
            let err = meta(&[("MODEL_API_KEY", KEY), ("CONSENSUS_MODEL", value)]).unwrap_err();
            assert_eq!(err.setting, "CONSENSUS_MODEL", "{value:?}: {err}");
        }
        // A good CONSENSUS_MODEL does not hide a bad Critic override, nor
        // the reverse.
        let err = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL", "has space"),
            ("CONSENSUS_MODEL_CRITIC", "muse-spark-1.3"),
        ])
        .unwrap_err();
        assert_eq!(err.setting, "CONSENSUS_MODEL");
        assert!(meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL_CRITIC", "muse-spark-1.3")
        ])
        .is_ok());
        // A field edited in code after the build is checked too.
        let mut cfg =
            MetaConfig::from_parts(DEFAULT_META_BASE_URL, ApiKey::new(KEY), DEFAULT_META_MODEL)
                .unwrap();
        cfg.critic_model = String::new();
        assert_eq!(cfg.validate().unwrap_err().setting, "critic_model");
    }

    #[test]
    fn debug_lists_both_models_and_still_redacts_the_key() {
        let cfg = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL_CRITIC", "muse-spark-1.3"),
        ])
        .unwrap()
        .unwrap();
        let printed = format!("{cfg:?}");
        assert!(printed.contains(r#"model: "muse-wonder-1.0""#), "{printed}");
        assert!(
            printed.contains(r#"critic_model: "muse-spark-1.3""#),
            "{printed}"
        );
        assert!(!printed.contains("nx9-test-secret"), "{printed}");
        assert!(printed.contains("<redacted>"), "{printed}");
        let cfg = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_MODEL_PROPOSER", "muse-spark-1.3"),
        ])
        .unwrap()
        .unwrap();
        let printed = format!("{cfg:?}");
        assert!(
            printed.contains(r#"proposer_model: "muse-spark-1.3""#),
            "{printed}"
        );
        assert!(!printed.contains("nx9-test-secret"), "{printed}");
    }

    #[test]
    fn the_debug_dir_is_refused_in_public_mode() {
        let ok = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_META_DEBUG_DIR", "target/debug-drafts"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(ok.debug_dir, Some(PathBuf::from("target/debug-drafts")));
        let err = meta(&[
            ("MODEL_API_KEY", KEY),
            ("CONSENSUS_PUBLIC_MODE", "true"),
            ("CONSENSUS_META_DEBUG_DIR", "target/debug-drafts"),
        ])
        .unwrap_err();
        assert_eq!(err.setting, "CONSENSUS_META_DEBUG_DIR");
        assert!(err.message.contains("public mode"), "{err}");
        // Also when the flag is set in code after the config was built.
        let mut cfg = ok;
        cfg.public_mode = true;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn the_key_never_prints_and_is_trimmed() {
        let key = ApiKey::new(format!("  {KEY}\r\n"));
        assert_eq!(key.expose(), KEY);
        assert!(key.has_documented_shape());
        let cfg =
            MetaConfig::from_parts("https://api.meta.ai/v1/", key, "muse-wonder-1.0").unwrap();
        assert_eq!(cfg.base_url, "https://api.meta.ai/v1");
        let printed = format!("{cfg:?} {:?}", cfg.api_key);
        assert!(!printed.contains("nx9-test-secret"), "{printed}");
        assert!(printed.contains("<redacted>"));
        // from_parts checks the key shape too.
        let err = MetaConfig::from_parts(DEFAULT_META_BASE_URL, ApiKey::new("not-a-key"), "m")
            .unwrap_err();
        assert_eq!(err.setting, "api_key");
        assert!(!err.to_string().contains("not-a-key"), "{err}");
    }

    #[test]
    fn a_credential_file_key_is_read_trimmed_and_checked() {
        let dir =
            std::env::temp_dir().join(format!("hyphae-consensus-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("meta.key");
        std::fs::write(&path, format!("{KEY}\n")).unwrap();
        let key = ApiKey::read_file(&path).unwrap();
        assert_eq!(key.expose(), KEY);
        std::fs::write(&path, "\n").unwrap();
        assert!(
            ApiKey::read_file(&path).is_err(),
            "an empty file holds no key"
        );
        assert!(ApiKey::read_file(dir.join("missing.key")).is_err());
        std::fs::write(&path, "sk-wrong\n").unwrap();
        let err = MetaConfig::from_parts(
            DEFAULT_META_BASE_URL,
            ApiKey::read_file(&path).unwrap(),
            "m",
        )
        .unwrap_err();
        assert_eq!(err.setting, "credential file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_key_file_variable_comes_first_and_its_errors_name_it() {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-config-keyfile-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("meta.key");
        std::fs::write(&path, format!("  {KEY}\r\n")).unwrap();
        let path_text = path.display().to_string();
        // The file wins over every key variable, and is trimmed.
        let cfg = meta(&[
            ("CONSENSUS_META_API_KEY_FILE", &path_text),
            ("CONSENSUS_META_API_KEY", "LLM_1_consensus"),
            ("MODEL_API_KEY", "LLM_2_model"),
            ("META_API_KEY", "LLM_3_meta"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cfg.api_key.expose(), KEY);
        assert!(!format!("{cfg:?}").contains("nx9-test-secret"));
        // Empty, missing, or not a key: an error naming the variable, never
        // a fall-through to the variables and never the file's content.
        std::fs::write(&path, "\n").unwrap();
        let err = meta(&[
            ("CONSENSUS_META_API_KEY_FILE", &path_text),
            ("MODEL_API_KEY", KEY),
        ])
        .unwrap_err();
        assert_eq!(err.setting, "CONSENSUS_META_API_KEY_FILE");
        assert!(err.message.contains("holds no key"), "{err}");
        let missing = dir.join("missing.key").display().to_string();
        let err = meta(&[
            ("CONSENSUS_META_API_KEY_FILE", &missing),
            ("MODEL_API_KEY", KEY),
        ])
        .unwrap_err();
        assert_eq!(err.setting, "CONSENSUS_META_API_KEY_FILE");
        assert!(err.message.contains("cannot read"), "{err}");
        std::fs::write(&path, "sk-not-a-meta-key\n").unwrap();
        let err = meta(&[("CONSENSUS_META_API_KEY_FILE", &path_text)]).unwrap_err();
        assert_eq!(err.setting, "CONSENSUS_META_API_KEY_FILE");
        assert!(!err.to_string().contains("sk-not-a-meta-key"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safety_identifiers_are_bounded_printable_ascii() {
        let mut cfg =
            MetaConfig::from_parts(DEFAULT_META_BASE_URL, ApiKey::new(KEY), DEFAULT_META_MODEL)
                .unwrap();
        cfg.safety_identifier = Some("a".repeat(64));
        assert!(cfg.validate().is_ok());
        for bad in ["", "has space", "\u{e9}t\u{e9}"] {
            cfg.safety_identifier = Some(bad.into());
            assert!(cfg.validate().is_err(), "{bad:?}");
        }
        cfg.safety_identifier = Some("a".repeat(65));
        assert!(cfg.validate().is_err());
    }

    fn hyphae(pairs: &[(&str, &str)]) -> Result<Option<HyphaeConfig>, ConfigError> {
        let map = vars(pairs);
        HyphaeConfig::from_vars(|name| map.get(name).cloned())
    }

    const BASE: [(&str, &str); 2] = [
        ("HYPHAE_NATIVE_API_KEY_FILE", "/run/keys/hyphae.key"),
        ("HYPHAE_NATIVE_ENDPOINT", "unix:/run/hyphae.sock"),
    ];

    #[test]
    fn the_pinned_anchor_is_normalized_and_validated_at_load() {
        let anchor = "E9CF8C51AD12358397BCD1A387D73B0778482070B1A6133E3F5D9351BBC22621";
        let mut pairs = BASE.to_vec();
        let padded = format!(" {anchor}\r\n");
        pairs.push(("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", &padded));
        let cfg = hyphae(&pairs).unwrap().unwrap();
        assert_eq!(
            cfg.expected_anchor.as_deref(),
            Some(anchor.to_ascii_lowercase().as_str())
        );
        assert_eq!(cfg.verify_timeout, Duration::from_secs(60));

        let not_hex = "zz".repeat(32);
        for bad in [&anchor[1..], not_hex.as_str(), "e9cf"] {
            let mut pairs: Vec<(&str, &str)> = BASE.to_vec();
            pairs.push(("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", bad));
            let err = hyphae(&pairs).unwrap_err();
            assert_eq!(err.setting, "CONSENSUS_HYPHAE_EXPECTED_ANCHOR", "{bad}");
        }
        // Fail closed: an unusable pin never leaves the verifier unpinned.
        let mut pairs = BASE.to_vec();
        pairs.push(("CONSENSUS_HYPHAE_EXPECTED_ANCHOR", "e9cf"));
        assert!(hyphae(&pairs).is_err());
    }

    #[test]
    fn hyphae_needs_a_key_file_and_a_daemon_and_bounds_the_verifier() {
        assert!(hyphae(&[]).unwrap().is_none());
        assert!(hyphae(&BASE[..1]).unwrap().is_none(), "no daemon address");
        let mut pairs = BASE.to_vec();
        pairs.push(("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS", "5"));
        let cfg = hyphae(&pairs).unwrap().unwrap();
        assert_eq!(cfg.verify_timeout, Duration::from_secs(5));
        assert_eq!(cfg.binary, "hyphae");
        assert!(!cfg.mcp_argv().iter().any(|a| a == "--allow-write"));
        let mut pairs = BASE.to_vec();
        pairs.push(("CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS", "0"));
        assert_eq!(
            hyphae(&pairs).unwrap_err().setting,
            "CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS"
        );
    }
}
