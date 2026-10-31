// SPDX-License-Identifier: Apache-2.0
//
// The error codes the UI has a sentence for (`Api.errors.<code>` in the four
// catalogs): every `ErrorCode` of the generated contract types, plus the
// client's own codes. The server's English `message` is never shown; clients
// localize from `code` (contracts/api-v1/ApiError.schema.json).
//
// `SERVER_CODES` is checked against the generated union with `satisfies
// Record<ErrorCode, true>`: a code the contract adds without an entry here
// fails `tsc`, and tests/api-errors.test.ts reads the union from
// src/lib/api/types.ts and requires a sentence for each member in every
// catalog, so a new code cannot reach the UI as the generic sentence.

import type { ClientErrorCode } from "./client";
import type { ErrorCode } from "./types";

const SERVER_CODES = {
  invalid_request: true,
  payload_too_large: true,
  unsupported_media_type: true,
  csrf_rejected: true,
  host_not_allowed: true,
  client_unidentified: true,
  session_required: true,
  unauthorized: true,
  not_found: true,
  method_not_allowed: true,
  rate_limited: true,
  too_many_runs: true,
  too_many_streams: true,
  storage_full: true,
  live_disabled: true,
  shutting_down: true,
  run_not_awaiting_decision: true,
  decision_conflict: true,
  idempotency_key_reused: true,
  idempotency_key_required: true,
  override_note_required: true,
  run_not_cancellable: true,
  run_busy: true,
  model_not_configured: true,
  memory_not_configured: true,
  upstream_unavailable: true,
  ndjson_unavailable: true,
  request_timeout: true,
  live_budget_daily: true,
  live_budget_lifetime: true,
  live_budget_judge_daily: true,
  live_budget_judge_slice: true,
  live_concurrency: true,
  live_hourly_admissions: true,
  live_client_quota: true,
  live_tripped: true,
  live_unavailable: true,
  turnstile_required: true,
  turnstile_failed: true,
  turnstile_unavailable: true,
  attestation_required: true,
  geo_blocked_portal: true,
  geo_blocked_live: true,
  geo_unknown: true,
  preset_unknown: true,
  internal: true,
} as const satisfies Record<ErrorCode, true>;

const CLIENT_CODES = {
  unreachable: true,
  unexpected_status: true,
  bad_response: true,
} as const satisfies Record<ClientErrorCode, true>;

/** Every `ErrorCode` of the contract this build was generated from. */
export const SERVER_ERROR_CODES = Object.keys(SERVER_CODES) as ErrorCode[];

/** The client's own codes (see `ClientErrorCode` in client.ts). */
export const CLIENT_ERROR_CODES = Object.keys(CLIENT_CODES) as ClientErrorCode[];

export type KnownErrorCode = ErrorCode | ClientErrorCode;

/** The codes with a sentence in the catalogs (`Api.errors.<code>`). */
export const KNOWN_ERROR_CODES: readonly KnownErrorCode[] = [
  ...SERVER_ERROR_CODES,
  ...CLIENT_ERROR_CODES,
];

/**
 * The catalog key of a code's sentence: its own, or `unknown` for a code
 * this build does not know (a newer server); the code itself is still shown
 * next to the sentence.
 */
export function errorSentenceKey(code: string): KnownErrorCode | "unknown" {
  return (KNOWN_ERROR_CODES as readonly string[]).includes(code)
    ? (code as KnownErrorCode)
    : "unknown";
}

/**
 * Where an error happened, which decides what its headline says first:
 * starting a run (`create`), recording a human decision (`decision`), or
 * reading something (`read`, every other request).
 */
export type ErrorContext = "create" | "decision" | "read";

/**
 * The headline of an error (`Api.headlines.<key>`): the consequence for the
 * visitor, chosen by the code's class, so a 500, a 503 and a 429 never share
 * one (design-direction 2.3, classes A to L). The code's own sentence stays
 * as the body.
 */
export type ErrorHeadline =
  | "notAccepted"
  | "rateLimited"
  | "busy"
  | "liveInProgress"
  | "liveLimit"
  | "budgetToday"
  | "budgetSpent"
  | "liveClosed"
  | "humanCheck"
  | "location"
  | "session"
  | "notAvailable"
  | "decision"
  | "serverProblem"
  | "serverUnavailable"
  | "unreachable"
  | "unreadable";

const HEADLINES = {
  // A. The request was not accepted as sent.
  invalid_request: "notAccepted",
  payload_too_large: "notAccepted",
  unsupported_media_type: "notAccepted",
  csrf_rejected: "notAccepted",
  host_not_allowed: "notAccepted",
  client_unidentified: "notAccepted",
  method_not_allowed: "notAccepted",
  idempotency_key_reused: "notAccepted",
  idempotency_key_required: "notAccepted",
  attestation_required: "notAccepted",
  preset_unknown: "notAccepted",
  run_not_cancellable: "notAccepted",
  // B. This connection sent too many requests (429).
  rate_limited: "rateLimited",
  // C. The server is busy.
  too_many_runs: "busy",
  too_many_streams: "busy",
  storage_full: "busy",
  run_busy: "busy",
  // D. Live capacity.
  live_concurrency: "liveInProgress",
  live_hourly_admissions: "liveLimit",
  live_client_quota: "liveLimit",
  // E. Live budget.
  live_budget_daily: "budgetToday",
  live_budget_judge_daily: "budgetToday",
  live_budget_lifetime: "budgetSpent",
  live_budget_judge_slice: "budgetSpent",
  // F. Live mode off or unavailable.
  live_disabled: "liveClosed",
  live_tripped: "liveClosed",
  live_unavailable: "liveClosed",
  model_not_configured: "liveClosed",
  memory_not_configured: "liveClosed",
  // G. The human check.
  turnstile_required: "humanCheck",
  turnstile_failed: "humanCheck",
  turnstile_unavailable: "humanCheck",
  // H. Where the visitor is.
  geo_blocked_portal: "location",
  geo_blocked_live: "location",
  geo_unknown: "location",
  // I. Session and access.
  session_required: "session",
  unauthorized: "notAvailable",
  not_found: "notAvailable",
  ndjson_unavailable: "notAvailable",
  // J. A decision.
  run_not_awaiting_decision: "decision",
  decision_conflict: "decision",
  override_note_required: "decision",
  // K. A server problem: a failure (500, 504) or a server that is not
  // serving right now (503).
  internal: "serverProblem",
  request_timeout: "serverProblem",
  shutting_down: "serverUnavailable",
  upstream_unavailable: "serverUnavailable",
  // L. Not reachable, or an answer that could not be read.
  unreachable: "unreachable",
  unexpected_status: "unreadable",
  bad_response: "unreadable",
} as const satisfies Record<KnownErrorCode, ErrorHeadline>;

/**
 * The headline key of an error: in the decision context every failure says
 * first that the decision was not recorded; elsewhere the code's class
 * decides. A code this build does not know reads as an answer it could not
 * read (class L, as its sentence `unknown` does).
 */
export function errorHeadlineKey(code: string, context: ErrorContext = "read"): ErrorHeadline {
  if (context === "decision") {
    return "decision";
  }
  const key = errorSentenceKey(code);
  return key === "unknown" ? "unreadable" : HEADLINES[key];
}

/**
 * The codes whose generic sentence (`Api.errors.<code>`) would not fit a
 * run's creation, which gets its own (`Api.createErrors.<code>`): the
 * run-creation sentence ("admits no new run") must never appear under a
 * decision or a read.
 */
export const CREATE_SENTENCE_CODES = ["shutting_down"] as const;

/** The catalog key of an error's body sentence in a context. */
export function errorSentencePath(code: string, context: ErrorContext = "read"): string {
  const key = errorSentenceKey(code);
  return context === "create" && (CREATE_SENTENCE_CODES as readonly string[]).includes(key)
    ? `createErrors.${key}`
    : `errors.${key}`;
}
