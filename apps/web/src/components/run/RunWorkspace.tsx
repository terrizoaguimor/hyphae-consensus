"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The run workspace: the header (title, state, outcome with its evidence
// axis and, for a revised proposal, which proposal it is for, the record
// count, the revision round, the run id, the failure sentence of a failed
// run, the four provenance axes with the latest frame's delivery, the
// flags), the error of a failed detail request with a retry, and the tabs
// Live (named Playback unless the run deliberates and its latest frame is
// LIVE by src/lib/provenance.ts isLive), Explorer and Decision. The other tabs of the
// design (Evidence, Proof inspector, Audit trail, Meta API panel) belong to
// another lane and are not stubbed here.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useFormatter, useTranslations } from "next-intl";
import { useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { isRunId } from "@/lib/api/client";
import { evidenceLabel, isLive, RECOMMENDATION_VARIANTS } from "@/lib/provenance";
import { supersededProposal } from "@/lib/run/view";
import { localePath } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { ProvenanceBadges } from "../ProvenanceBadges";
import { AuditTab } from "./AuditTab";
import { DecisionTab } from "./DecisionTab";
import { EvidenceTab } from "./EvidenceTab";
import { ExplorerTab } from "./ExplorerTab";
import { FailureSentence } from "./FailureSentence";
import { MetaTab } from "./MetaTab";
import { ProofsTab } from "./ProofsTab";
import { StageTab } from "./StageTab";
import { tabId, Tabs } from "./Tabs";
import { useReceipt } from "./useReceipt";
import { useRunData } from "./useRunData";

type RunWorkspaceProps = {
  locale: Locale;
};

type TabKey = "stage" | "explorer" | "evidence" | "proofs" | "audit" | "meta" | "decision";

/** Tabs that show the receipt. */
const RECEIPT_TABS: ReadonlySet<TabKey> = new Set<TabKey>(["proofs", "audit"]);

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
  const format = useFormatter();
  const data = useRunData(runId);
  const receipt = useReceipt(runId, "this_server");
  const [tab, setTab] = useState<TabKey>("stage");
  const { detail, detailError, summary, frames, reloadDetail } = data;
  // The receipt is read when a tab that shows it is first opened.
  const selectTab = (next: TabKey) => {
    setTab(next);
    if (RECEIPT_TABS.has(next)) {
      receipt.ensure();
    }
  };

  if (summary === null) {
    if (detailError !== null) {
      return (
        <div className="hc-section" data-run="error">
          <ApiErrorNotice error={detailError} onRetry={reloadDetail} />
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
  // The stage tab says Live only while the run deliberates and its newest
  // frame is LIVE (isLive: a live frame of a live-model run without a
  // blocking flag); a replayed, flagged, scripted, still undetermined or
  // finished run is a playback.
  const liveNow =
    summary.active && latest !== undefined && isLive(latest.delivery, summary.provenance);
  const stageLabel = liveNow ? t("tabs.live") : t("tabs.playback");
  // When the recommendation is for a revision, the header says which one.
  const revisedId =
    detail !== null && supersededProposal(detail) !== null
      ? (detail.current_proposal?.id ?? null)
      : null;

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
                summary.state === "failed" ? (
                  t("header.noOutcomeFailed")
                ) : (
                  t("header.noOutcome")
                )
              ) : (
                <>
                  <Badge
                    variant={RECOMMENDATION_VARIANTS[summary.recommendation]}
                    label={t("outcome", {
                      kind: t(`kinds.${summary.recommendation}`),
                      evidence: tProvenance(evidenceLabel(summary.provenance.evidence).key),
                    })}
                  />
                  {revisedId === null ? null : (
                    <span className="hc-supporting" data-outcome-revision>
                      {t("header.forRevision", { id: revisedId })}
                    </span>
                  )}
                </>
              )}
            </dd>
          </div>
          <div>
            <dt>{t("header.recordsLabel")}</dt>
            <dd className="hc-tabular-nums" data-run-field="records">
              {t("header.records", { count: summary.event_count })}
            </dd>
          </div>
          <div>
            <dt>{t("header.revision")}</dt>
            <dd className="hc-tabular-nums" data-run-field="revision">
              {format.number(summary.revision_round)}
            </dd>
          </div>
          <div>
            <dt>{t("header.runId")}</dt>
            <dd>
              <code className="hc-code">{summary.run_id}</code>
            </dd>
          </div>
        </dl>
        {summary.failure === undefined || summary.failure === null ? null : (
          <FailureSentence failure={summary.failure} />
        )}
        <div>
          <p className="hc-supporting">{t("header.provenance")}</p>
          <ProvenanceBadges
            provenance={summary.provenance}
            delivery={latest?.delivery}
            recordedAt={latest?.event.occurred_at ?? summary.updated_at}
            withNote
          />
        </div>
      </header>

      {detailError === null ? null : (
        <div className="hc-section" data-run="detail-error">
          <p className="hc-supporting">{t("detailError")}</p>
          <ApiErrorNotice error={detailError} onRetry={reloadDetail} />
        </div>
      )}

      <Tabs<TabKey>
        label={t("tabs.label")}
        value={tab}
        onChange={selectTab}
        panelId="run-tabpanel"
        items={[
          { value: "stage", label: stageLabel },
          { value: "explorer", label: t("tabs.explorer") },
          { value: "evidence", label: t("tabs.evidence") },
          { value: "proofs", label: t("tabs.proofs") },
          { value: "audit", label: t("tabs.audit") },
          { value: "meta", label: t("tabs.meta") },
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
        {tab === "explorer" ? (
          <ExplorerTab detail={detail} frames={frames} detailError={detailError} />
        ) : null}
        {tab === "evidence" ? (
          <EvidenceTab detail={detail} frames={frames} detailError={detailError} summary={summary} />
        ) : null}
        {tab === "proofs" ? (
          <ProofsTab
            detail={detail}
            frames={frames}
            detailError={detailError}
            summary={summary}
            receipt={receipt}
          />
        ) : null}
        {tab === "audit" ? (
          <AuditTab
            runId={runId}
            detail={detail}
            frames={frames}
            detailError={detailError}
            summary={summary}
            receipt={receipt}
            archive={null}
          />
        ) : null}
        {tab === "meta" ? <MetaTab detail={detail} frames={frames} detailError={detailError} /> : null}
        {tab === "decision" ? <DecisionTab runId={runId} data={data} /> : null}
      </div>
    </div>
  );
}
