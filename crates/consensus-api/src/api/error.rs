// SPDX-License-Identifier: Apache-2.0
//! The one error envelope of the API: a stable code, a short English
//! message that never carries internal detail, and `Retry-After` where
//! the client should wait. Internal errors are logged on the server with
//! their detail and answered with `internal` only.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable error codes. A code never changes meaning; a new condition gets
/// a new code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The body is not the expected JSON: a syntax error, an unknown
    /// field, a bad enum value, a missing field, a value out of bounds.
    InvalidRequest,
    /// The body exceeds the request body limit.
    PayloadTooLarge,
    /// A state-changing request without `Content-Type: application/json`.
    UnsupportedMediaType,
    /// The `Origin` or `Sec-Fetch-Site` header does not allow the request.
    CsrfRejected,
    /// The `Host` header is not one this server answers for.
    HostNotAllowed,
    /// The listener trusts `CF-Connecting-IP` and the request has none.
    ClientUnidentified,
    /// The public route needs a session cookie (`POST /api/v1/session`).
    SessionRequired,
    /// The operator route needs a valid bearer token.
    Unauthorized,
    /// No such route, run or file, or a run the caller does not own.
    NotFound,
    /// Method not allowed on this route.
    MethodNotAllowed,
    /// The client key exceeded a request rate; see `retry_after_secs`.
    RateLimited,
    /// Every deliberation slot is busy; see `retry_after_secs`.
    TooManyRuns,
    /// A stream cap (per run, per client or global) is reached.
    TooManyStreams,
    /// The store holds its configured maximum of visitor runs.
    StorageFull,
    /// The public live mode is not wired on this server.
    LiveDisabled,
    /// The server is shutting down; no new runs are admitted.
    ShuttingDown,
    /// A decision needs the run to await a human decision.
    RunNotAwaitingDecision,
    /// The decision's `expected` block does not match the run.
    DecisionConflict,
    /// The `Idempotency-Key` was already used with a different body.
    IdempotencyKeyReused,
    /// The `Idempotency-Key` header is missing or malformed.
    IdempotencyKeyRequired,
    /// Approving a blocked or abstained run needs a note.
    OverrideNoteRequired,
    /// Only a run still deliberating can be cancelled.
    RunNotCancellable,
    /// The run is held by another operation; retry shortly.
    RunBusy,
    /// Operator surface: `model=meta` without credentials.
    ModelNotConfigured,
    /// Operator surface: `memory=hyphae` without configuration.
    MemoryNotConfigured,
    /// Operator surface: an upstream client could not be initialised.
    UpstreamUnavailable,
    /// Public surface: the verbatim event log of this run is not
    /// published, because its chain may carry upstream text (a live
    /// model's failure reason); the detail and the receipt remain.
    NdjsonUnavailable,
    /// The request was not handled within the server's request timeout.
    RequestTimeout,
    /// Public live mode: the daily USD budget cannot hold one more run;
    /// `retry_after_secs` says when the paced budget admits again.
    LiveBudgetDaily,
    /// Public live mode: the lifetime USD budget is exhausted.
    LiveBudgetLifetime,
    /// Judge lane: its daily budget is exhausted until the next UTC day.
    LiveBudgetJudgeDaily,
    /// Judge lane: its lifetime slice is exhausted.
    LiveBudgetJudgeSlice,
    /// Public live mode: every live slot is busy; see `retry_after_secs`.
    LiveConcurrency,
    /// Public live mode: the rolling-hour admission cap is reached.
    LiveHourlyAdmissions,
    /// Public live mode: the ledger is tripped (kill switch, a cap, a
    /// provider refusal of the key); an operator must reset it.
    LiveTripped,
    /// Public live mode: the ledger could not be consulted or the run
    /// could not be assembled; nothing was charged.
    LiveUnavailable,
    /// Public live mode: the request carries no Turnstile token
    /// (`cf-turnstile-response` header).
    TurnstileRequired,
    /// Public live mode: Cloudflare did not accept the Turnstile token.
    TurnstileFailed,
    /// Public live mode: Turnstile could not be asked; retry shortly.
    TurnstileUnavailable,
    /// Public live mode: `attest_adult` must be true.
    AttestationRequired,
    /// The whole portal is unavailable from the visitor's country.
    GeoBlockedPortal,
    /// Live mode is unavailable from the visitor's country.
    GeoBlockedLive,
    /// The visitor's country could not be determined (no `CF-IPCountry`);
    /// a listener behind Cloudflare refuses rather than guesses.
    GeoUnknown,
    /// Public live mode: the preset id names no curated preset.
    PresetUnknown,
    /// Something failed on the server; the detail is in its log.
    Internal,
}

impl From<crate::live::geo::GeoBlockCode> for ErrorCode {
    /// The wire code of a geographic block is the geo module's fixed
    /// public code (`geo_blocked_portal`, `geo_blocked_live`).
    fn from(code: crate::live::geo::GeoBlockCode) -> Self {
        use crate::live::geo::GeoBlockCode;
        match code {
            GeoBlockCode::GeoBlockedPortal => ErrorCode::GeoBlockedPortal,
            GeoBlockCode::GeoBlockedLive => ErrorCode::GeoBlockedLive,
        }
    }
}

impl ErrorCode {
    pub fn status(self) -> StatusCode {
        match self {
            ErrorCode::InvalidRequest => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ErrorCode::CsrfRejected => StatusCode::FORBIDDEN,
            ErrorCode::HostNotAllowed => StatusCode::MISDIRECTED_REQUEST,
            ErrorCode::SessionRequired | ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::NotFound | ErrorCode::NdjsonUnavailable => StatusCode::NOT_FOUND,
            ErrorCode::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            ErrorCode::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            ErrorCode::RateLimited
            | ErrorCode::TooManyRuns
            | ErrorCode::TooManyStreams
            | ErrorCode::LiveBudgetDaily
            | ErrorCode::LiveBudgetJudgeDaily
            | ErrorCode::LiveConcurrency
            | ErrorCode::LiveHourlyAdmissions => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::StorageFull
            | ErrorCode::LiveDisabled
            | ErrorCode::ShuttingDown
            | ErrorCode::LiveBudgetLifetime
            | ErrorCode::LiveBudgetJudgeSlice
            | ErrorCode::LiveTripped
            | ErrorCode::LiveUnavailable
            | ErrorCode::TurnstileUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::TurnstileRequired | ErrorCode::TurnstileFailed => StatusCode::FORBIDDEN,
            ErrorCode::AttestationRequired => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::GeoBlockedPortal | ErrorCode::GeoBlockedLive | ErrorCode::GeoUnknown => {
                StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
            }
            ErrorCode::PresetUnknown => StatusCode::NOT_FOUND,
            ErrorCode::RunNotAwaitingDecision
            | ErrorCode::DecisionConflict
            | ErrorCode::IdempotencyKeyReused
            | ErrorCode::RunNotCancellable
            | ErrorCode::RunBusy => StatusCode::CONFLICT,
            ErrorCode::IdempotencyKeyRequired
            | ErrorCode::ClientUnidentified
            | ErrorCode::OverrideNoteRequired
            | ErrorCode::ModelNotConfigured
            | ErrorCode::MemoryNotConfigured => StatusCode::BAD_REQUEST,
            ErrorCode::UpstreamUnavailable => StatusCode::BAD_GATEWAY,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// The JSON body of every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    pub code: ErrorCode,
    /// Short English statement for humans and logs; clients localize from
    /// `code`. Never carries paths, upstream bodies or panic text.
    pub message: String,
    /// Seconds to wait before retrying, when the status is 429 or 503.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_secs: None,
        }
    }

    pub fn with_retry_after(mut self, secs: u64) -> Self {
        self.retry_after_secs = Some(secs.max(1));
        self
    }

    pub fn status(&self) -> StatusCode {
        self.code.status()
    }

    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound, "not found")
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }

    /// An internal failure: `detail` goes to the server log only.
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        eprintln!("[api] internal error: {detail}");
        Self::new(ErrorCode::Internal, "internal error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut response = (status, Json(&self)).into_response();
        if let Some(secs) = self.retry_after_secs {
            if let Ok(value) = header::HeaderValue::from_str(&secs.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_serialize_snake_case_and_carry_their_status() {
        let err = ApiError::new(ErrorCode::CsrfRejected, "x");
        assert_eq!(serde_json::to_value(&err).unwrap()["code"], "csrf_rejected");
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
        let limited = ApiError::new(ErrorCode::RateLimited, "slow down").with_retry_after(0);
        assert_eq!(limited.retry_after_secs, Some(1));
        let response = limited.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    }

    #[test]
    fn geo_codes_are_the_geo_modules_fixed_public_codes() {
        use crate::live::geo::GeoBlockCode;
        for code in [GeoBlockCode::GeoBlockedPortal, GeoBlockCode::GeoBlockedLive] {
            let api: ErrorCode = code.into();
            assert_eq!(serde_json::to_value(api).unwrap(), code.as_str());
            assert_eq!(api.status(), StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS);
        }
        assert_eq!(
            ErrorCode::GeoUnknown.status(),
            StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
        );
        assert_eq!(
            serde_json::to_value(ErrorCode::GeoUnknown).unwrap(),
            "geo_unknown"
        );
    }
}
