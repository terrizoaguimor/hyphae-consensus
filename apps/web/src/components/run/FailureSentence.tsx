"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A failed run, explained by the translated sentence of its failure CODE
// (src/lib/run/failure.ts), never by the server's English message; a code
// without a sentence of its own gets the generic sentence, which names the
// code, and the stage and code are always shown for support. The same
// vocabulary serves the Explorer's rows and the My runs cards
// (useFailureText, useUnavailableText).

import { useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import { failureSentence, unavailableSentence } from "@/lib/run/failure";

type Failure = { code?: string | null; stage: string };

/** The sentence of a failure, as text. */
export function useFailureText(): (failure: Failure) => string {
  const t = dynamicTranslator(useTranslations("Run.failure"));
  return (failure) => {
    const sentence = failureSentence(failure);
    return t(sentence.key, sentence.values);
  };
}

/** The sentence of an `evidence_unavailable` record's code, as text. */
export function useUnavailableText(): (code: string | null | undefined) => string {
  const t = dynamicTranslator(useTranslations("Run.unavailable"));
  return (code) => {
    const sentence = unavailableSentence(code);
    return t(sentence.key, sentence.values);
  };
}

/** The failure of a run, for the run header. */
export function FailureSentence({ failure }: { failure: Failure }) {
  const t = useTranslations("Run.failure");
  const text = useFailureText();
  const code = failure.code ?? null;
  return (
    <div className="hc-error" role="status" data-failure-code={code ?? ""}>
      <p className="hc-error-title">{t("title")}</p>
      <p>{text(failure)}</p>
      <p className="hc-supporting">
        {code === null || code === ""
          ? t("detailNoCode", { stage: failure.stage })
          : t("detail", { stage: failure.stage, code })}
      </p>
    </div>
  );
}
