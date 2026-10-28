"use client";
// SPDX-License-Identifier: Apache-2.0
//
// One API error, shown honestly: the translated sentence of its code (every
// ErrorCode of the contract has one; an unknown code gets the generic
// sentence), the Retry-After wait when the server gave one, and the code
// and HTTP status for support. The server's English message is not shown:
// clients localize from the code (contracts/api-v1/ApiError.schema.json).

import { useTranslations } from "next-intl";

import type { ApiError } from "@/lib/api/client";

/** The error codes with a sentence in the catalogs (`Api.errors.<code>`). */
export const KNOWN_ERROR_CODES = [
  "invalid_request",
  "payload_too_large",
  "unsupported_media_type",
  "csrf_rejected",
  "host_not_allowed",
  "client_unidentified",
  "session_required",
  "unauthorized",
  "not_found",
  "method_not_allowed",
  "rate_limited",
  "too_many_runs",
  "too_many_streams",
  "storage_full",
  "live_disabled",
  "shutting_down",
  "run_not_awaiting_decision",
  "decision_conflict",
  "idempotency_key_reused",
  "idempotency_key_required",
  "override_note_required",
  "run_not_cancellable",
  "run_busy",
  "model_not_configured",
  "memory_not_configured",
  "upstream_unavailable",
  "ndjson_unavailable",
  "request_timeout",
  "internal",
  "unreachable",
  "unexpected_status",
  "bad_response",
] as const;

type KnownErrorCode = (typeof KNOWN_ERROR_CODES)[number];

function knownCode(code: string): KnownErrorCode | "unknown" {
  return (KNOWN_ERROR_CODES as readonly string[]).includes(code)
    ? (code as KnownErrorCode)
    : "unknown";
}

type ApiErrorNoticeProps = {
  error: ApiError;
};

export function ApiErrorNotice({ error }: ApiErrorNoticeProps) {
  const t = useTranslations("Api");
  const code = knownCode(error.code);
  return (
    <div role="alert" className="hc-error" data-error-code={error.code}>
      <p className="hc-error-title">{t("errorTitle")}</p>
      <p>{t(`errors.${code}`)}</p>
      {error.retryAfterSecs === null ? null : (
        <p>{t("retryAfter", { seconds: error.retryAfterSecs })}</p>
      )}
      <p className="hc-supporting">
        {t("errorDetail", { code: error.code, status: error.status })}
      </p>
    </div>
  );
}
