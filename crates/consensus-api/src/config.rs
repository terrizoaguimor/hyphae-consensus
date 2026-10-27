//! Environment configuration. No secret is ever printed, logged, or
//! written to the event log; key material stays in memory only (Meta) or
//! in a restricted file referenced by path (Hyphae).

use consensus_runtime::RunLimits;
use std::time::Duration;

/// Per-call model timeout when `CONSENSUS_MODEL_TIMEOUT_SECS` is unset.
/// 2 of 12 live runs on 2026-10-26 hit the earlier 60 s default.
pub const DEFAULT_MODEL_TIMEOUT_SECS: u64 = 120;

/// `CONSENSUS_MODEL_TIMEOUT_SECS` in whole seconds; unset or unparseable
/// values use the default.
pub fn parse_model_timeout(value: Option<&str>) -> Duration {
    Duration::from_secs(
        value
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_MODEL_TIMEOUT_SECS),
    )
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

#[derive(Clone)]
pub struct MetaConfig {
    pub base_url: String,
    /// In-memory only. `Debug` is implemented manually to avoid leaks.
    pub api_key: String,
    /// Exact Model API identity; recorded verbatim in receipts.
    pub model: String,
    pub timeout: Duration,
    pub max_retries: u32,
}

impl std::fmt::Debug for MetaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

impl MetaConfig {
    /// Reads `META_API_KEY` (falling back to `MODEL_API_KEY`, both names
    /// appear in Meta ecosystem docs), `CONSENSUS_MODEL`,
    /// `CONSENSUS_META_BASE_URL`, `CONSENSUS_MODEL_TIMEOUT_SECS`.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("META_API_KEY")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var("MODEL_API_KEY")
                    .ok()
                    .filter(|v| !v.is_empty())
            })?;
        Some(Self {
            base_url: std::env::var("CONSENSUS_META_BASE_URL")
                .unwrap_or_else(|_| "https://api.meta.ai/v1".into()),
            api_key,
            model: std::env::var("CONSENSUS_MODEL").unwrap_or_else(|_| "muse-wonder-1.0".into()),
            timeout: parse_model_timeout(
                std::env::var("CONSENSUS_MODEL_TIMEOUT_SECS")
                    .ok()
                    .as_deref(),
            ),
            max_retries: 2,
        })
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
    /// Operator-pinned expected proof anchor (obtained out-of-band).
    /// When set, records anchored elsewhere verify as Invalid.
    pub expected_anchor: Option<String>,
}

impl HyphaeConfig {
    pub fn from_env() -> Option<Self> {
        let key_file = std::env::var("HYPHAE_NATIVE_API_KEY_FILE")
            .ok()
            .filter(|v| !v.is_empty())?;
        let endpoint = std::env::var("HYPHAE_NATIVE_ENDPOINT")
            .ok()
            .filter(|v| !v.is_empty());
        let base_url = std::env::var("HYPHAE_BASE_URL")
            .ok()
            .filter(|v| !v.is_empty());
        if endpoint.is_none() && base_url.is_none() {
            return None;
        }
        let collection = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Some(Self {
            binary: std::env::var("CONSENSUS_HYPHAE_BIN").unwrap_or_else(|_| "hyphae".into()),
            endpoint,
            base_url,
            key_file,
            personal_collection: collection("CONSENSUS_HYPHAE_PERSONAL_COLLECTION"),
            work_collection: collection("CONSENSUS_HYPHAE_WORK_COLLECTION"),
            journal_collection: collection("CONSENSUS_HYPHAE_JOURNAL_COLLECTION"),
            expected_anchor: collection("CONSENSUS_HYPHAE_EXPECTED_ANCHOR"),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_timeout_defaults_to_120_seconds_and_keeps_the_override() {
        assert_eq!(parse_model_timeout(None), Duration::from_secs(120));
        assert_eq!(parse_model_timeout(Some("45")), Duration::from_secs(45));
        assert_eq!(parse_model_timeout(Some("soon")), Duration::from_secs(120));
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
}
