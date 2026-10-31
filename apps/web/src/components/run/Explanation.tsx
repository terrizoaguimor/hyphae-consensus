"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Why the policy recommended what it did, from the recommendation's policy
// trace (src/lib/run/explain.ts): in plain words (the deciding rule with the
// kind it derives, the trace's counts, one sentence per failed check), and
// the technical view (the rule and the rule order, every failed check with
// the ids it names, the counts by field, the rationale code, the server's
// English rationale), which starts expanded at the "technical" explanation
// level and collapsed at the "simple" one (Settings; TechnicalDetails, as in
// Docs). What the records add to the trace is said apart from it: SIMULATED
// proof statuses, and for a run before protocol 1.3 that the policy read the
// final round only and how many earlier-round objections were never judged
// again (and how many of those their own round left unlisted too). When
// every record the invalid_proofs check names reports that no proof could
// be generated upstream, the plain text says nothing was checked, as the
// Proof inspector does. A recommendation without a trace (before schema 1.1) is said to be
// shown as recorded.

import { useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { PolicyTrace, RecommendationV1, RunDetailV1 } from "@/lib/api/types";
import {
  contextMessages,
  COUNT_FIELDS,
  type ExplainMessage,
  explainContext,
  plainExplanation,
  reasonIds,
  traceReading,
} from "@/lib/run/explain";

import { TechnicalDetails } from "../TechnicalDetails";
import { SimulatedRationaleNote } from "./StageTab";
import { UntrustedText } from "./UntrustedText";

type ExplanationProps = {
  recommendation: RecommendationV1;
  detail: RunDetailV1;
};

/** Renders an explanation message (`Run.explain.<key>`), its kind translated. */
function useExplainText(): (message: ExplainMessage) => string {
  const explain = dynamicTranslator(useTranslations("Run.explain"));
  const tKinds = useTranslations("Run.kinds");
  return (message) =>
    explain(message.key, {
      ...message.values,
      ...(message.kind === undefined ? {} : { kind: tKinds(message.kind) }),
    });
}

function PlainTrace({ trace, detail }: { trace: PolicyTrace; detail: RunDetailV1 }) {
  const t = useTranslations("Run.explain");
  const text = useExplainText();
  const plain = plainExplanation(trace, traceReading(trace, detail));
  return (
    <div data-explanation-plain data-trace-rule={trace.rule}>
      <p data-explain-lead>{text(plain.lead)}</p>
      <ul className="hc-list" data-explain-counts>
        {plain.counts.map((message) => (
          <li key={message.key}>{text(message)}</li>
        ))}
      </ul>
      {plain.reasons.length === 0 ? null : (
        <>
          <p>{t("reasonsIntro")}</p>
          <ul className="hc-list" data-explain-reasons>
            {plain.reasons.map((message, index) => (
              <li key={`${message.key}-${index}`}>{text(message)}</li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

function TechnicalTrace({
  trace,
  recommendation,
  simulated,
  mixed,
}: {
  trace: PolicyTrace;
  recommendation: RecommendationV1;
  /** Every proof of the run is SIMULATED: the counts by field (and the server's text) are scripted statuses. */
  simulated: boolean;
  /** Some proofs of the run are SIMULATED and some are not: the counts include scripted statuses. */
  mixed: boolean;
}) {
  const t = useTranslations("Run.explain");
  const explain = dynamicTranslator(t);
  return (
    <>
      <p data-technical-rule={trace.rule}>
        {t("technical.rule", {
          rule: explain(`ruleNames.${trace.rule}`),
          id: trace.rule,
          version: trace.version,
        })}
      </p>
      {recommendation.rationale_code === undefined || recommendation.rationale_code === null ? null : (
        <p>
          {t("technical.code")} <code className="hc-code">{recommendation.rationale_code}</code>
        </p>
      )}
      <p>{t("technical.reasonsHeading", { count: trace.reasons.length })}</p>
      {trace.reasons.length === 0 ? null : (
        <ul className="hc-list" data-technical-reasons>
          {trace.reasons.map((reason, index) => {
            const ids = reasonIds(reason);
            return (
              <li key={`${reason.reason}-${index}`} data-reason={reason.reason}>
                <span>
                  {explain(`reasonNames.${reason.reason}`)}
                  {" ("}
                  <code className="hc-code">{reason.reason}</code>
                  {")"}
                </span>
                {ids.length === 0 ? null : <span>{` ${t("technical.ids", { ids: ids.join(", ") })}`}</span>}
                {reason.reason === "evaluator_abstained" &&
                reason.abstain_reason !== undefined &&
                reason.abstain_reason !== null ? (
                  <UntrustedText text={reason.abstain_reason} source="model" />
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
      <dl className="hc-kv hc-kv--compact" data-technical-counts>
        {COUNT_FIELDS.map((field) => (
          <div key={field}>
            <dt>
              <code className="hc-code">{field}</code>
            </dt>
            <dd className="hc-tabular-nums" data-count-field={field}>
              {trace.counts[field]}
            </dd>
          </div>
        ))}
      </dl>
      {simulated || mixed ? (
        <p className="hc-supporting" data-counts-simulated={simulated ? "all" : "some"}>
          {t(simulated ? "technical.countsSimulated" : "technical.countsMixed")}
        </p>
      ) : null}
    </>
  );
}

export function Explanation({ recommendation, detail }: ExplanationProps) {
  const t = useTranslations("Run.explain");
  const text = useExplainText();
  const trace = recommendation.trace ?? null;
  const records = explainContext(detail);
  const context = contextMessages(records);

  return (
    <section className="hc-explanation" aria-labelledby="explanation-heading" data-explanation>
      <h4 id="explanation-heading">{t("heading")}</h4>
      {trace === null ? <p data-explanation-trace="none">{t("noTrace")}</p> : <PlainTrace trace={trace} detail={detail} />}
      {context.length === 0 ? null : (
        <ul className="hc-list" data-explain-context>
          {context.map((message) => (
            <li key={message.key} data-context={message.key}>
              {text(message)}
            </li>
          ))}
        </ul>
      )}
      <TechnicalDetails summary={t("technicalSummary")}>
        <div data-explanation-technical>
          {trace === null ? (
            <p>{t("technical.noTrace", { protocol: recommendation.protocol })}</p>
          ) : (
            <TechnicalTrace
              trace={trace}
              recommendation={recommendation}
              simulated={records.simulatedProofs}
              mixed={records.mixedProofs === true}
            />
          )}
          <p>{t("technical.rationale")}</p>
          <UntrustedText text={recommendation.rationale} source="server" />
          {/* Without a trace, the caption under the counts is absent: say it under the server's text. */}
          {(records.simulatedProofs || records.mixedProofs === true) && trace === null ? (
            <SimulatedRationaleNote mixed={records.mixedProofs === true} />
          ) : null}
          <p className="hc-supporting">{t("technical.protocol", { protocol: recommendation.protocol })}</p>
        </div>
      </TechnicalDetails>
    </section>
  );
}
