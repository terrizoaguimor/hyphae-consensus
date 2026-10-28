// SPDX-License-Identifier: Apache-2.0
//! Server-side Cloudflare Turnstile verification (Siteverify), fail
//! closed (ABUSE-9).
//!
//! A token is accepted only when Cloudflare says `success: true` **and**
//! the token was minted for the expected widget `action` **and** on the
//! expected `hostname` **and** its `challenge_ts` is recent
//! ([`DEFAULT_MAX_CHALLENGE_AGE`]). A Siteverify outage, a non-200 answer,
//! a timeout or an unparseable body all refuse the request: an outage
//! never admits anyone. The secret is read from a file, lives only inside
//! [`TurnstileSecret`] (whose `Debug` is redacted) and is sent nowhere but
//! the Siteverify form body.
//!
//! Cloudflare's documented always-pass and always-fail test secrets
//! ([`TEST_SECRETS`]) are refused at configuration load in public mode
//! (see [`super::config`]), so a local test configuration cannot reach
//! production by mistake.
//!
//! `idempotency_key` is Cloudflare's optional UUID under which the same
//! token may be validated again within its validity window; without a
//! repeated key a token is single-use (`timeout-or-duplicate`). That
//! idempotency is global and immediate, so a caller must never derive the
//! key from anything a client controls: the live gate passes a fresh UUID
//! v4 on every call, and one solved challenge admits at most one run. A
//! client's retry is the server's business, not Siteverify's: the create
//! route answers a repeated `Idempotency-Key` from its stored creation
//! before this module is asked (see `crate::live::gate`).
//!
//! The HTTP transport is `reqwest::blocking`; an async server must call it
//! from a blocking task.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

/// Cloudflare's Siteverify endpoint.
pub const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
/// Whole-request timeout for one Siteverify call.
pub const SITEVERIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// A `challenge_ts` older than this is refused.
pub const DEFAULT_MAX_CHALLENGE_AGE: Duration = Duration::from_secs(300);
/// A `challenge_ts` this far in the future is still accepted (clock skew).
pub const MAX_FUTURE_SKEW: Duration = Duration::from_secs(60);
/// Longest token accepted; Cloudflare tokens are far shorter.
pub const MAX_TOKEN_LEN: usize = 2048;
/// Default widget actions, matching the frontend widget configuration.
pub const DEFAULT_ACTION_LIVE: &str = "live-run";
pub const DEFAULT_ACTION_SESSION: &str = "session";

/// Cloudflare's documented testing secrets: always pass, always fail,
/// "token already spent". None may be used in public mode.
pub const TEST_SECRETS: [&str; 3] = [
    "1x0000000000000000000000000000000AA",
    "2x0000000000000000000000000000000AA",
    "3x0000000000000000000000000000000AA",
];

/// The widget secret. Never printed, never serialized.
#[derive(Clone)]
pub struct TurnstileSecret(String);

impl std::fmt::Debug for TurnstileSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TurnstileSecret(<redacted>)")
    }
}

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("cannot read the Turnstile secret file {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("the Turnstile secret is empty")]
    Empty,
    #[error("the Turnstile secret is a Cloudflare test secret; refused in public mode")]
    TestSecret,
}

impl TurnstileSecret {
    /// A secret from memory (tests and local tooling); refuses an empty one.
    pub fn new(secret: impl Into<String>) -> Result<Self, SecretError> {
        let secret = secret.into();
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            return Err(SecretError::Empty);
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Reads the secret from `path`, trimming surrounding whitespace and
    /// the trailing newline most editors add.
    pub fn from_file(path: &Path) -> Result<Self, SecretError> {
        let text = std::fs::read_to_string(path).map_err(|source| SecretError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::new(text)
    }

    /// True for any of [`TEST_SECRETS`].
    pub fn is_test_secret(&self) -> bool {
        TEST_SECRETS.contains(&self.0.as_str())
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

/// Turnstile settings from the environment (see [`super::config`]).
#[derive(Debug, Clone)]
pub struct TurnstileConfig {
    pub secret: TurnstileSecret,
    /// Expected `hostname` in every Siteverify answer (lowercase).
    pub hostname: String,
    /// Expected `action` of a live-run token.
    pub action_live: String,
    /// Expected `action` of a session token.
    pub action_session: String,
    pub max_challenge_age: Duration,
}

/// Cloudflare's Siteverify answer, as documented.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct SiteverifyResponse {
    pub success: bool,
    #[serde(default)]
    pub challenge_ts: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default, rename = "error-codes")]
    pub error_codes: Vec<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub cdata: Option<String>,
}

/// The raw Siteverify call: Cloudflare's answer, or why it could not be
/// obtained. Implemented by [`HttpSiteverify`] and [`FakeSiteverify`].
pub trait SiteverifyTransport: Send + Sync {
    fn siteverify(
        &self,
        token: &str,
        remote_ip: Option<IpAddr>,
        idempotency_key: &str,
    ) -> Result<SiteverifyResponse, TurnstileError>;
}

impl<T: SiteverifyTransport + ?Sized> SiteverifyTransport for Arc<T> {
    fn siteverify(
        &self,
        token: &str,
        remote_ip: Option<IpAddr>,
        idempotency_key: &str,
    ) -> Result<SiteverifyResponse, TurnstileError> {
        (**self).siteverify(token, remote_ip, idempotency_key)
    }
}

/// A verified token's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub action: String,
    pub hostname: String,
    pub challenge_ts: DateTime<Utc>,
    pub cdata: Option<String>,
}

/// Every way a token is refused. All of them answer the visitor with the
/// one public code [`TurnstileError::PUBLIC_CODE`]; the variant is for the
/// operator log.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TurnstileError {
    #[error("empty token")]
    EmptyToken,
    #[error("token longer than {MAX_TOKEN_LEN} bytes ({len})")]
    TokenTooLong { len: usize },
    #[error("Siteverify refused the token: {error_codes:?}")]
    Failed { error_codes: Vec<String> },
    #[error("token action {got:?} is not the expected {expected:?}")]
    WrongAction {
        expected: String,
        got: Option<String>,
    },
    #[error("token hostname {got:?} is not the expected {expected:?}")]
    WrongHostname {
        expected: String,
        got: Option<String>,
    },
    #[error("challenge_ts is {age_secs} s old or too far ahead; limit {max_secs} s")]
    StaleChallenge { age_secs: i64, max_secs: u64 },
    #[error("challenge_ts missing or unparseable: {0:?}")]
    BadChallengeTs(Option<String>),
    #[error("Siteverify unavailable: {0}")]
    Outage(String),
    #[error("Siteverify answer unreadable: {0}")]
    Malformed(String),
}

impl TurnstileError {
    /// The only code a visitor ever sees.
    pub const PUBLIC_CODE: &'static str = "turnstile_failed";

    pub fn public_code(&self) -> &'static str {
        Self::PUBLIC_CODE
    }

    /// True when Cloudflare could not be asked (the visitor may retry).
    pub fn is_outage(&self) -> bool {
        matches!(
            self,
            TurnstileError::Outage(_) | TurnstileError::Malformed(_)
        )
    }
}

/// The full check: transport plus the acceptance rules in the module docs.
///
/// `idempotency_key` goes to Cloudflare verbatim, which accepts the same
/// `token` again under the same key; the caller mints a fresh UUID per
/// call (never a client-supplied value) so that a token stays single-use.
pub trait Siteverify: Send + Sync {
    fn verify(
        &self,
        token: &str,
        remote_ip: Option<IpAddr>,
        idempotency_key: &str,
    ) -> Result<Verified, TurnstileError>;
}

type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Applies the acceptance rules to one expected action and hostname over
/// any transport. Clone it with [`TurnstileVerifier::for_action`] for a
/// second action sharing the same transport.
#[derive(Clone)]
pub struct TurnstileVerifier {
    transport: Arc<dyn SiteverifyTransport>,
    expected_hostname: String,
    expected_action: String,
    max_challenge_age: Duration,
    now: Clock,
}

impl std::fmt::Debug for TurnstileVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnstileVerifier")
            .field("expected_hostname", &self.expected_hostname)
            .field("expected_action", &self.expected_action)
            .field("max_challenge_age", &self.max_challenge_age)
            .finish_non_exhaustive()
    }
}

impl TurnstileVerifier {
    pub fn new(
        transport: Arc<dyn SiteverifyTransport>,
        expected_hostname: impl Into<String>,
        expected_action: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            expected_hostname: expected_hostname.into().to_ascii_lowercase(),
            expected_action: expected_action.into(),
            max_challenge_age: DEFAULT_MAX_CHALLENGE_AGE,
            now: Arc::new(Utc::now),
        }
    }

    /// The live-run verifier for a configuration, over the HTTP transport.
    pub fn from_config(cfg: &TurnstileConfig) -> Self {
        let transport: Arc<dyn SiteverifyTransport> =
            Arc::new(HttpSiteverify::new(cfg.secret.clone()));
        Self::new(transport, cfg.hostname.clone(), cfg.action_live.clone())
            .with_max_challenge_age(cfg.max_challenge_age)
    }

    pub fn with_max_challenge_age(mut self, max: Duration) -> Self {
        self.max_challenge_age = max;
        self
    }

    pub fn with_clock(mut self, now: Clock) -> Self {
        self.now = now;
        self
    }

    /// The same transport, hostname and limits for another action.
    pub fn for_action(&self, action: impl Into<String>) -> Self {
        Self {
            expected_action: action.into(),
            ..self.clone()
        }
    }

    pub fn expected_action(&self) -> &str {
        &self.expected_action
    }

    pub fn expected_hostname(&self) -> &str {
        &self.expected_hostname
    }
}

impl Siteverify for TurnstileVerifier {
    fn verify(
        &self,
        token: &str,
        remote_ip: Option<IpAddr>,
        idempotency_key: &str,
    ) -> Result<Verified, TurnstileError> {
        if token.is_empty() {
            return Err(TurnstileError::EmptyToken);
        }
        if token.len() > MAX_TOKEN_LEN {
            return Err(TurnstileError::TokenTooLong { len: token.len() });
        }
        let answer = self
            .transport
            .siteverify(token, remote_ip, idempotency_key)?;
        if !answer.success {
            return Err(TurnstileError::Failed {
                error_codes: answer.error_codes,
            });
        }
        if answer.action.as_deref() != Some(self.expected_action.as_str()) {
            return Err(TurnstileError::WrongAction {
                expected: self.expected_action.clone(),
                got: answer.action,
            });
        }
        let hostname_ok = answer
            .hostname
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(&self.expected_hostname));
        if !hostname_ok {
            return Err(TurnstileError::WrongHostname {
                expected: self.expected_hostname.clone(),
                got: answer.hostname,
            });
        }
        let challenge_ts = answer
            .challenge_ts
            .as_deref()
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|ts| ts.with_timezone(&Utc))
            .ok_or_else(|| TurnstileError::BadChallengeTs(answer.challenge_ts.clone()))?;
        let age = (self.now)() - challenge_ts;
        let age_secs = age.num_seconds();
        let max_secs = self.max_challenge_age.as_secs();
        let too_old = age_secs > i64::try_from(max_secs).unwrap_or(i64::MAX);
        let too_new = age_secs < -i64::try_from(MAX_FUTURE_SKEW.as_secs()).unwrap_or(i64::MAX);
        if too_old || too_new {
            return Err(TurnstileError::StaleChallenge { age_secs, max_secs });
        }
        Ok(Verified {
            action: self.expected_action.clone(),
            hostname: self.expected_hostname.clone(),
            challenge_ts,
            cdata: answer.cdata,
        })
    }
}

/// The real transport: a form POST to Siteverify with a 10 s timeout.
pub struct HttpSiteverify {
    http: reqwest::blocking::Client,
    endpoint: String,
    secret: TurnstileSecret,
}

impl std::fmt::Debug for HttpSiteverify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpSiteverify")
            .field("endpoint", &self.endpoint)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

impl HttpSiteverify {
    pub fn new(secret: TurnstileSecret) -> Self {
        Self::with_endpoint(secret, SITEVERIFY_URL)
    }

    /// Another endpoint (a local fake in tests).
    pub fn with_endpoint(secret: TurnstileSecret, endpoint: impl Into<String>) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(SITEVERIFY_TIMEOUT)
            .build()
            .expect("reqwest client with a timeout builds");
        Self {
            http,
            endpoint: endpoint.into(),
            secret,
        }
    }
}

impl SiteverifyTransport for HttpSiteverify {
    fn siteverify(
        &self,
        token: &str,
        remote_ip: Option<IpAddr>,
        idempotency_key: &str,
    ) -> Result<SiteverifyResponse, TurnstileError> {
        let mut form: Vec<(&str, String)> = vec![
            ("secret", self.secret.expose().to_string()),
            ("response", token.to_string()),
            ("idempotency_key", idempotency_key.to_string()),
        ];
        if let Some(ip) = remote_ip {
            form.push(("remoteip", ip.to_string()));
        }
        let response = self
            .http
            .post(&self.endpoint)
            .form(&form)
            .send()
            .map_err(|e| {
                // reqwest's Display never includes the request body.
                let kind = if e.is_timeout() {
                    "timeout"
                } else {
                    "transport"
                };
                TurnstileError::Outage(format!("{kind}: {e}"))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(TurnstileError::Outage(format!("siteverify http {status}")));
        }
        let body = response
            .bytes()
            .map_err(|e| TurnstileError::Outage(format!("body: {e}")))?;
        serde_json::from_slice::<SiteverifyResponse>(&body)
            .map_err(|e| TurnstileError::Malformed(e.to_string()))
    }
}

/// One recorded call to a [`FakeSiteverify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeCall {
    pub token: String,
    pub remote_ip: Option<IpAddr>,
    pub idempotency_key: String,
}

type FakeHandler =
    dyn Fn(&FakeCall, &[FakeCall]) -> Result<SiteverifyResponse, TurnstileError> + Send + Sync;

/// A scripted transport for tests. The handler sees the current call and
/// every earlier call, so it can model Cloudflare's single-use tokens
/// (`timeout-or-duplicate` for a repeated token under a new key).
pub struct FakeSiteverify {
    handler: Box<FakeHandler>,
    calls: Mutex<Vec<FakeCall>>,
}

impl FakeSiteverify {
    pub fn new(
        handler: impl Fn(&FakeCall, &[FakeCall]) -> Result<SiteverifyResponse, TurnstileError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            handler: Box::new(handler),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Always the same answer.
    pub fn always(answer: Result<SiteverifyResponse, TurnstileError>) -> Self {
        Self::new(move |_, _| answer.clone())
    }

    pub fn calls(&self) -> Vec<FakeCall> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl SiteverifyTransport for FakeSiteverify {
    fn siteverify(
        &self,
        token: &str,
        remote_ip: Option<IpAddr>,
        idempotency_key: &str,
    ) -> Result<SiteverifyResponse, TurnstileError> {
        let call = FakeCall {
            token: token.to_string(),
            remote_ip,
            idempotency_key: idempotency_key.to_string(),
        };
        let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let answer = (self.handler)(&call, &calls);
        calls.push(call);
        answer
    }
}

/// A passing answer for `hostname`/`action` minted at `challenge_ts`
/// (test helper).
pub fn passing_answer(
    hostname: &str,
    action: &str,
    challenge_ts: DateTime<Utc>,
) -> SiteverifyResponse {
    SiteverifyResponse {
        success: true,
        challenge_ts: Some(challenge_ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        hostname: Some(hostname.to_string()),
        error_codes: Vec::new(),
        action: Some(action.to_string()),
        cdata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_is_redacted_and_test_secrets_are_recognized() {
        let s = TurnstileSecret::new("1x0000000000000000000000000000000AA\n").unwrap();
        assert!(s.is_test_secret());
        assert_eq!(format!("{s:?}"), "TurnstileSecret(<redacted>)");
        let real = TurnstileSecret::new("0x4AAAAAAAAzzzzzzzzzzzzzzzzzzzzzzzz").unwrap();
        assert!(!real.is_test_secret());
        assert!(!format!("{real:?}").contains("4AAAA"));
        assert!(matches!(
            TurnstileSecret::new("  \n"),
            Err(SecretError::Empty)
        ));
    }

    #[test]
    fn response_parses_cloudflares_documented_shape() {
        let body = r#"{"success":false,"error-codes":["invalid-input-response"],"messages":[]}"#;
        let r: SiteverifyResponse = serde_json::from_str(body).unwrap();
        assert!(!r.success);
        assert_eq!(r.error_codes, vec!["invalid-input-response"]);
        let body = r#"{"success":true,"challenge_ts":"2026-10-27T15:14:30.096Z","hostname":"demo.example.org","error-codes":[],"action":"live-run","cdata":"x"}"#;
        let r: SiteverifyResponse = serde_json::from_str(body).unwrap();
        assert!(r.success);
        assert_eq!(r.action.as_deref(), Some("live-run"));
    }
}
