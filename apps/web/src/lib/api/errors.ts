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
