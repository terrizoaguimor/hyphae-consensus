"use client";
// SPDX-License-Identifier: Apache-2.0
//
// One API error, shown honestly: a headline that says the consequence first,
// chosen by the code's class and the context of the request (a 500, a 503 and
// a 429 never share one; a refused decision says the decision was not
// recorded; src/lib/api/errors.ts), the translated sentence of its code
// (every ErrorCode of the contract has one; an unknown code gets the generic
// sentence; a run-creation sentence never appears under a decision or a
// read), the Retry-After wait when the server gave one, the code and HTTP
// status for support, a retry button when the caller can repeat the request,
// and, for a run's creation refused with a 429, the way to a recorded run
// instead. The server's English message is not shown: clients localize from
// the code (contracts/api-v1/ApiError.schema.json).

import { Button } from "@astryxdesign/core/Button";
import Link from "next/link";
import { useLocale, useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { ApiError } from "@/lib/api/client";
import { type ErrorContext, errorHeadlineKey, errorSentencePath } from "@/lib/api/errors";
import { localePath } from "@/lib/site";

type ApiErrorNoticeProps = {
  error: ApiError;
  /** Where the request was made: what the headline says first. */
  context?: ErrorContext;
  /** Repeats the failed request; shown as a "Try again" button when given. */
  onRetry?: () => void;
};

export function ApiErrorNotice({ error, context = "read", onRetry }: ApiErrorNoticeProps) {
  // Headline and sentence keys are chosen at run time from the code
  // (src/lib/api/errors.ts); tests/api-errors.test.ts checks every key exists.
  const t = dynamicTranslator(useTranslations("Api"));
  const locale = useLocale();
  const headline = errorHeadlineKey(error.code, context);
  const wait = error.retryAfterSecs;
  const headlineText =
    headline === "rateLimited"
      ? wait === null
        ? t("headlines.rateLimitedNoWait")
        : t("headlines.rateLimited", { seconds: wait })
      : t(`headlines.${headline}`);
  // A refused creation that asks to wait: a recorded run needs no slot.
  const fallback = context === "create" && error.status === 429;
  return (
    <div
      role="alert"
      className="hc-error"
      data-error-code={error.code}
      data-error-headline={headline}
      data-error-context={context}
    >
      <p className="hc-error-title">{headlineText}</p>
      <p>{t(errorSentencePath(error.code, context))}</p>
      {wait === null ? null : <p>{t("retryAfter", { seconds: wait })}</p>}
      <p className="hc-supporting">
        {t("errorDetail", { code: error.code, status: error.status })}
      </p>
      {onRetry === undefined && !fallback ? null : (
        <div className="hc-actions">
          {onRetry === undefined ? null : (
            <Button variant="secondary" size="sm" label={t("retry")} onClick={onRetry} />
          )}
          {fallback ? (
            <Link href={localePath(locale, "showcase")} className="hc-link" data-error-fallback>
              {t("fallback")}
            </Link>
          ) : null}
        </div>
      )}
    </div>
  );
}
