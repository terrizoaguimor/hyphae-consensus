"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Overview's figures, computed from the research archive as GET
// /showcase lists it (src/lib/showcase.ts archiveFigures): the recorded runs
// and their records, the runs by outcome next to their evidence axis (an
// outcome is never shown without it), by battery with the schema versions
// and policy rules each was recorded under, by model axis (labels from
// src/lib/provenance.ts: META MODEL, never LIVE) and authorization, and the
// recorded Meta request ids. Every number is counted from the entries;
// nothing is typed into the page. Until the health probe answers it says it
// is checking; without the API it is the honest placeholder.

import Link from "next/link";
import { useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { useApiHealth } from "@/lib/api/useApiHealth";
import { evidenceLabel, modelLabel } from "@/lib/provenance";
import { digestPrefix } from "@/lib/run/evidence";
import { archiveFigures, type OutcomeKey, policyRulesOf } from "@/lib/showcase";
import { localePath } from "@/lib/site";

import { ApiErrorNotice } from "./ApiErrorNotice";
import { NotConnected } from "./NotConnected";
import { useAuthorizationText } from "./showcase/ShowcaseList";
import { useArchive } from "./showcase/useArchive";

type ImpactFiguresProps = {
  locale: Locale;
  /** What the figures are once connected (translated by the page). */
  planned: string[];
};

export function ImpactFigures({ locale, planned }: ImpactFiguresProps) {
  const t = useTranslations("ApiGate");
  const health = useApiHealth();
  if (health.status === "pending") {
    return (
      <p role="status" className="hc-supporting" data-figures="pending">
        {t("checking")}
      </p>
    );
  }
  if (health.status === "unreachable") {
    return (
      <div data-figures="unreachable">
        <NotConnected planned={planned} unreachableCode={health.error.code} onRetry={health.retry} />
      </div>
    );
  }
  return <Figures locale={locale} />;
}

function Figures({ locale }: { locale: Locale }) {
  const t = useTranslations("Overview.figures");
  const tRun = useTranslations("Run");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const authorizationText = useAuthorizationText();
  const { state, reload } = useArchive(false);

  if (state.status === "loading") {
    return (
      <p role="status" className="hc-supporting" data-figures="loading">
        {t("loading")}
      </p>
    );
  }
  if (state.status === "error") {
    return (
      <div data-figures="error">
        <ApiErrorNotice error={state.error} onRetry={reload} />
      </div>
    );
  }
  const figures = archiveFigures(state.showcase.entries);
  if (figures.runs === 0) {
    return (
      <p className="hc-panel" data-figures="empty">
        {t("empty")}
      </p>
    );
  }
  const outcomeText = (outcome: OutcomeKey) =>
    outcome === "failed" ? t("failed") : outcome === "other" ? t("other") : tRun(`kinds.${outcome}`);
  const rulesText = (schema: string) => {
    const rules = policyRulesOf(schema);
    return rules === null ? t("schemaNoRules", { schema }) : t("schemaRules", { schema, rules });
  };
  return (
    <div className="hc-figures" data-figures="ready">
      <p data-figures-total={figures.runs}>
        {t("source", {
          runs: figures.runs,
          events: figures.events,
          sha256: digestPrefix(state.showcase.manifest_sha256) ?? state.showcase.manifest_sha256,
        })}
      </p>

      <div className="hc-table-scroll" role="region" aria-labelledby="figures-outcomes" tabIndex={0}>
        <table className="hc-timeline" data-figures-outcomes>
          <caption id="figures-outcomes">{t("outcomesCaption")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("outcome")}</th>
              {figures.evidence.map((axis) => (
                <th scope="col" key={axis}>
                  {t("onEvidence", { evidence: tProvenance(evidenceLabel(axis).key) })}
                </th>
              ))}
              <th scope="col">{t("total")}</th>
            </tr>
          </thead>
          <tbody>
            {figures.outcomes.map((row) => (
              <tr key={row.outcome} data-figure-outcome={row.outcome}>
                <th scope="row">{outcomeText(row.outcome)}</th>
                {row.byEvidence.map((cell) => (
                  <td key={cell.evidence} className="hc-tabular-nums" data-figure-evidence={cell.evidence}>
                    {cell.count}
                  </td>
                ))}
                <td className="hc-tabular-nums" data-figure-total>
                  {row.total}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="hc-table-scroll" role="region" aria-labelledby="figures-batteries" tabIndex={0}>
        <table className="hc-timeline" data-figures-batteries>
          <caption id="figures-batteries">{t("batteriesCaption")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("battery")}</th>
              <th scope="col">{t("runs")}</th>
              <th scope="col">{t("schema")}</th>
              <th scope="col">{t("outcomes")}</th>
            </tr>
          </thead>
          <tbody>
            {figures.batteries.map((battery) => (
              <tr key={battery.battery} data-figure-battery={battery.battery}>
                <th scope="row">
                  <code className="hc-code">{battery.battery}</code>
                </th>
                <td className="hc-tabular-nums">{battery.runs}</td>
                <td>{battery.schemas.map(rulesText).join("; ")}</td>
                <td>
                  {battery.outcomes
                    .map((o) => t("outcomeCount", { outcome: outcomeText(o.outcome), count: o.count }))
                    .join(", ")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <ul className="hc-list" data-figures-axes>
        <li data-figures-models>
          {t("models", {
            list: figures.models
              .map((m) => t("modelCount", { label: tProvenance(modelLabel({ model: m.model }).key), count: m.count }))
              .join(", "),
          })}
        </li>
        <li data-figures-authorizations>
          {t("authorizations", {
            list: figures.authorizations
              .map((a) => t("authorizationCount", { authorization: authorizationText(a.authorization), count: a.count }))
              .join(", "),
          })}
        </li>
        <li data-figures-requests={figures.metaRequestIds}>{t("requests", { count: figures.metaRequestIds })}</li>
        <li data-figures-unchained={figures.unchained}>{t("unchained", { count: figures.unchained })}</li>
      </ul>
      <p className="hc-supporting">{t("note")}</p>
      <p>
        <Link href={localePath(locale, "showcase")} className="hc-link">
          {t("showcaseLink")}
        </Link>
      </p>
    </div>
  );
}
