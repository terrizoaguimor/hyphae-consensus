"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The run workspace: the header (title, state, outcome with its evidence
// axis, the four provenance axes with the latest frame's delivery, the
// flags), and the tabs Live (named Playback once the run is at rest),
// Explorer and Decision. The other tabs of the design (Evidence, Proof
// inspector, Audit trail, Meta API panel) belong to another lane and are not
// stubbed here.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { isRunId } from "@/lib/api/client";
import { evidenceLabel, RECOMMENDATION_VARIANTS } from "@/lib/provenance";
import { localePath } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { ProvenanceBadges } from "../ProvenanceBadges";
import { DecisionTab } from "./DecisionTab";
import { ExplorerTab } from "./ExplorerTab";
import { StageTab } from "./StageTab";
import { tabId, Tabs } from "./Tabs";
import { useRunData } from "./useRunData";

type RunWorkspaceProps = {
  locale: Locale;
};

type TabKey = "stage" | "explorer" | "decision";

export function RunWorkspace({ locale }: RunWorkspaceProps) {
  const t = useTranslations("Run");
  const params = useSearchParams();
  const requested = params.get("id");
  const runId = isRunId(requested) ? requested : null;

  if (runId === null) {
    return (
      <div className="hc-panel" data-run="no-id">
        <p className="hc-not-connected-message">
          {requested === null ? t("noId") : t("invalidId")}
        </p>
        <p>{t("noIdHint")}</p>
        <Link href={localePath(locale, "runs")} className="hc-link">
          {t("goToRuns")}
        </Link>
      </div>
    );
  }

  // Keyed by run id: another run starts from a fresh state and a fresh stream.
  return <RunView key={runId} locale={locale} runId={runId} />;
}

type RunViewProps = {
  locale: Locale;
  runId: string;
};

function RunView({ locale, runId }: RunViewProps) {
  const t = useTranslations("Run");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const data = useRunData(runId);
  const [tab, setTab] = useState<TabKey>("stage");
  const { detail, detailError, summary, frames } = data;

  if (summary === null) {
    if (detailError !== null) {
      return (
        <div className="hc-section" data-run="error">
          <ApiErrorNotice error={detailError} />
          <div className="hc-actions">
            <Button variant="secondary" label={t("goToRuns")} href={localePath(locale, "runs")} />
          </div>
        </div>
      );
    }
    return (
      <p role="status" className="hc-supporting" data-run="loading">
        {t("loading")}
      </p>
    );
  }

  const latest = frames.at(-1);
  const title = detail?.task?.title ?? summary.title ?? t("title");
  const stageLabel = summary.active ? t("tabs.live") : t("tabs.playback");

  return (
    <div className="hc-run" data-run-id={runId} data-run-state={summary.state}>
      <header className="hc-run-header hc-panel">
        <h2 className="hc-run-title">{title}</h2>
        <dl className="hc-kv">
          <div>
            <dt>{t("header.state")}</dt>
            <dd data-run-field="state">{t(`states.${summary.state}`)}</dd>
          </div>
          <div>
            <dt>{t("header.outcome")}</dt>
            <dd data-run-field="outcome">
              {summary.recommendation === undefined || summary.recommendation === null ? (
                t("header.noOutcome")
              ) : (
                <Badge
                  variant={RECOMMENDATION_VARIANTS[summary.recommendation]}
                  label={t("outcome", {
                    kind: t(`kinds.${summary.recommendation}`),
                    evidence: tProvenance(evidenceLabel(summary.provenance.evidence).key),
                  })}
                />
              )}
            </dd>
          </div>
          <div>
            <dt>{t("header.records")}</dt>
            <dd className="hc-tabular-nums">
              {t("header.records", { count: summary.event_count })}
            </dd>
          </div>
          <div>
            <dt>{t("header.revision", { round: summary.revision_round })}</dt>
            <dd>
              <code className="hc-code">{summary.run_id}</code>
            </dd>
          </div>
        </dl>
        {summary.failure === undefined || summary.failure === null ? null : (
          <p className="hc-error" role="status">
            {summary.failure.message}
          </p>
        )}
        <div>
          <p className="hc-supporting">{t("header.provenance")}</p>
          <ProvenanceBadges
            provenance={summary.provenance}
            delivery={latest?.delivery}
            recordedAt={latest?.event.occurred_at}
            withNote
          />
        </div>
      </header>

      <Tabs<TabKey>
        label={t("tabs.label")}
        value={tab}
        onChange={setTab}
        panelId="run-tabpanel"
        items={[
          { value: "stage", label: stageLabel },
          { value: "explorer", label: t("tabs.explorer") },
          { value: "decision", label: t("tabs.decision") },
        ]}
      />

      <div
        id="run-tabpanel"
        className="hc-tabpanel"
        role="tabpanel"
        aria-labelledby={tabId("run-tabpanel", tab)}
        data-tab={tab}
      >
        {tab === "stage" ? <StageTab data={data} /> : null}
        {tab === "explorer" ? <ExplorerTab detail={detail} frames={frames} /> : null}
        {tab === "decision" ? <DecisionTab runId={runId} data={data} /> : null}
      </div>
    </div>
  );
}
