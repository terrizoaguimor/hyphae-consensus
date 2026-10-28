"use client";
// SPDX-License-Identifier: Apache-2.0
//
// One API error, shown honestly: the translated sentence of its code (every
// ErrorCode of the contract has one, src/lib/api/errors.ts; an unknown code
// gets the generic sentence), the Retry-After wait when the server gave one,
// the code and HTTP status for support, and a retry button when the caller
// can repeat the request. The server's English message is not shown: clients
// localize from the code (contracts/api-v1/ApiError.schema.json).

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";

import type { ApiError } from "@/lib/api/client";
import { errorSentenceKey } from "@/lib/api/errors";

type ApiErrorNoticeProps = {
  error: ApiError;
  /** Repeats the failed request; shown as a "Try again" button when given. */
  onRetry?: () => void;
};

export function ApiErrorNotice({ error, onRetry }: ApiErrorNoticeProps) {
  const t = useTranslations("Api");
  const code = errorSentenceKey(error.code);
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
      {onRetry === undefined ? null : (
        <div className="hc-actions">
          <Button variant="secondary" size="sm" label={t("retry")} onClick={onRetry} />
        </div>
      )}
    </div>
  );
}
