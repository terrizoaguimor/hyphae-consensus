"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A failed run, explained by the translated sentences of its failure CODE and
// STAGE (src/lib/run/failure.ts), never by the server's English message or
// reason: what happened (naming the role whose call it was, for a refused
// model document), where it stopped (the stage's own name and its role),
// that nothing was decided, what that means for a new run (the code's
// class), whether the failure counts toward the day's live runs, and a link
// to start another run. The raw stage and code are kept for support under
// the technical details (collapsed at the "simple" explanation level). The
// same vocabulary serves the Explorer's rows and the My runs cards
// (useFailureText, useUnavailableText), whose sentences name no role.

import Link from "next/link";
import { useTranslations } from "next-intl";
import { useEffect, useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import { api } from "@/lib/api/client";
import { failurePanel, failureSentence, unavailableSentence } from "@/lib/run/failure";

import { TechnicalDetails } from "../TechnicalDetails";

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

/** Where a new run starts: the live form with the same preset, or the guided examples. */
export type AgainLink = { kind: "live" | "guided"; href: string };

/**
 * How long a visitor run stays in My runs (the capabilities' limits), read
 * once when `enabled`; null until it is known (or when it cannot be read).
 */
export function useVisitorAccessHours(enabled: boolean): number | null {
  const [hours, setHours] = useState<number | null>(null);
  useEffect(() => {
    if (!enabled) {
      return;
    }
    let cancelled = false;
    api
      .capabilities()
      .then((capabilities) => {
        if (!cancelled) {
          setHours(capabilities.limits.visitor_access_hours);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [enabled]);
  return hours;
}

type FailureSentenceProps = {
  failure: Failure;
  /**
   * A run of this server: the panel says what the failure means for a new
   * run. A recorded run of the archive says what happened and where only.
   */
  own?: boolean;
  /** A live run of the public surface (the daily count and the budget apply to it). */
  live?: boolean;
  /** Where to start another run; none for a run that is not the visitor's own. */
  again?: AgainLink | null;
  /** How long the failed run stays in My runs, when known. */
  accessHours?: number | null;
};

/** The failure panel of a run, for the run header. */
export function FailureSentence({
  failure,
  own = false,
  live = false,
  again = null,
  accessHours = null,
}: FailureSentenceProps) {
  const t = dynamicTranslator(useTranslations("Run.failure"));
  const tStage = dynamicTranslator(useTranslations("Run.stage"));
  const code = failure.code ?? null;
  const panel = failurePanel(failure, own && live);
  const next = own ? panel.next : null;
  // A role-naming sentence takes the role as its subject, with its article.
  const values = panel.sentence.values as Record<string, string>;
  const role = values.role;
  const sentence = t(
    panel.sentence.key,
    role === undefined ? values : { role: t(`subjects.${role}`) },
  );
  return (
    <div className="hc-error" role="status" data-failure-code={code ?? ""} data-failure-class={panel.next ?? ""}>
      <p className="hc-error-title">{t("title")}</p>
      <p data-failure-sentence>{sentence}</p>
      {panel.where === null ? null : (
        <p data-failure-where={panel.where.role}>
          {t("where", {
            stageName: panel.where.stageName === null ? panel.where.stage : t(panel.where.stageName),
            role: tStage(panel.where.role),
          })}
        </p>
      )}
      <p data-failure-nothing-decided>{t("nothingDecided")}</p>
      {next === null ? null : <p data-failure-next={next}>{t(`next.${next}`)}</p>}
      {panel.notCounted ? <p data-failure-not-counted>{t("quota.notCounted")}</p> : null}
      {again === null ? null : (
        <div className="hc-failure-again" data-failure-again={again.kind}>
          <p>
            <Link href={again.href} className="hc-link">
              {t(`again.${again.kind}`)}
            </Link>
          </p>
          <p className="hc-supporting">
            {accessHours === null ? t("again.noteShort") : t("again.note", { hours: accessHours })}
          </p>
        </div>
      )}
      <TechnicalDetails summary={t("technical")}>
        <p className="hc-supporting" data-failure-detail>
          {code === null || code === ""
            ? t("detailNoCode", { stage: failure.stage })
            : t("detail", { stage: failure.stage, code })}
        </p>
      </TechnicalDetails>
    </div>
  );
}
