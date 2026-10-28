"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The run workspace: the header (title, state, outcome with its evidence
// axis and, for a revised proposal, which proposal it is for, the record
// count, the revision round, the run id, the failure sentence of a failed
// run, the four provenance axes with the latest frame's delivery, the
// flags), the error of a failed detail request with a retry, and the tabs.
//
// Two data sources, one per run (`key`ed, so another run starts fresh):
// - a run of this server (`?id=<run_id>`): useRunData (GET /runs/{id} and
//   the stream) and the receipt (GET /runs/{id}/receipt); the tabs are Live
//   (named Playback unless the run deliberates and its latest frame is LIVE
//   by src/lib/provenance.ts isLive), Explorer, Evidence, Proof inspector,
//   Audit trail, Meta API and Decision;
// - a recorded run of the research archive (`?id=<run_id>&source=showcase`,
//   RECORDED mode): useRecordedRun (GET /showcase/{id}, the showcase entry
//   and the archive manifest's note) and its receipt (GET
//   /showcase/{id}/receipt); no stream and no stage, nothing runs, the
//   Decision tab is read-only, and the header says it is a recorded run with
//   the labels the archive data carries (schema 1.0: pre-1.1, unchained; a
//   run blocked because no proof could be generated upstream says so, with
//   the manifest's own note on its log).

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useFormatter, useTranslations } from "next-intl";
import { useMemo, useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { api, isRunId } from "@/lib/api/client";
import type { RunDetailV1, ShowcaseEntryV1 } from "@/lib/api/types";
import {
  evidenceLabel,
  isLive,
  ndjsonPublishable,
  RECOMMENDATION_VARIANTS,
  sourceLabel,
} from "@/lib/provenance";
import { evidenceView, generationFailures } from "@/lib/run/evidence";
import { baseName } from "@/lib/run/receipt";
import { supersededProposal } from "@/lib/run/view";
import { manifestNote } from "@/lib/showcase";
import { localePath, SHOWCASE_SOURCE } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { ProvenanceBadges } from "../ProvenanceBadges";
import { AuditTab } from "./AuditTab";
import { DecisionTab, type DecisionLinks } from "./DecisionTab";
import { EvidenceTab } from "./EvidenceTab";
import { ExplorerTab } from "./ExplorerTab";
import { FailureSentence } from "./FailureSentence";
import { MetaTab } from "./MetaTab";
import { ProofsTab } from "./ProofsTab";
import { StageTab } from "./StageTab";
import { tabId, Tabs } from "./Tabs";
import { UntrustedText } from "./UntrustedText";
import { type RunReceipt, useReceipt } from "./useReceipt";
import { type RecordedRunData, useRecordedRun } from "./useRecordedRun";
import { type RunData, useRunData } from "./useRunData";

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
  const recorded = params.get("source") === SHOWCASE_SOURCE;

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

  // Keyed by run id and source: another run starts from a fresh state and a fresh stream.
  return recorded ? (
    <RecordedRun key={`archive-${runId}`} locale={locale} runId={runId} />
  ) : (
    <OwnRun key={runId} locale={locale} runId={runId} />
  );
}

type RunViewProps = {
  locale: Locale;
  runId: string;
};

function OwnRun({ locale, runId }: RunViewProps) {
  const data = useRunData(runId);
  const receipt = useReceipt(runId, "this_server");
  return <Workspace locale={locale} runId={runId} receipt={receipt} mode={{ kind: "own", data }} />;
}

function RecordedRun({ locale, runId }: RunViewProps) {
  const data = useRecordedRun(runId);
  const receipt = useReceipt(runId, "research_archive");
  return <Workspace locale={locale} runId={runId} receipt={receipt} mode={{ kind: "recorded", data }} />;
}

type Mode = { kind: "own"; data: RunData } | { kind: "recorded"; data: RecordedRunData };

function Workspace({
  locale,
  runId,
  receipt,
  mode,
}: RunViewProps & { receipt: RunReceipt; mode: Mode }) {
  const t = useTranslations("Run");
  const tShowcase = useTranslations("Showcase.run");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const recorded = mode.kind === "recorded";
  const [tab, setTab] = useState<TabKey>(recorded ? "decision" : "stage");
  const { detail, detailError, summary, frames, reloadDetail } = mode.data;
  const archive = mode.kind === "recorded" ? mode.data.entry : null;
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
            {recorded ? (
              <Button variant="secondary" label={tShowcase("back")} href={localePath(locale, "showcase")} />
            ) : (
              <Button variant="secondary" label={t("goToRuns")} href={localePath(locale, "runs")} />
            )}
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
  // finished run is a playback. A recorded run has no stage at all.
  const liveNow =
    summary.active && latest !== undefined && isLive(latest.delivery, summary.provenance);
  const stageLabel = liveNow ? t("tabs.live") : t("tabs.playback");
  // When the recommendation is for a revision, the header says which one.
  const revisedId =
    detail !== null && supersededProposal(detail) !== null
      ? (detail.current_proposal?.id ?? null)
      : null;
  // A recorded run is at rest, and nobody can decide it here.
  const stateText =
    recorded && summary.state === "awaiting_human_approval"
      ? tShowcase("awaiting")
      : t(`states.${summary.state}`);
  const items: Array<{ value: TabKey; label: string }> = [
    ...(recorded ? [] : [{ value: "stage" as const, label: stageLabel }]),
    { value: "explorer", label: t("tabs.explorer") },
    { value: "evidence", label: t("tabs.evidence") },
    { value: "proofs", label: t("tabs.proofs") },
    { value: "audit", label: t("tabs.audit") },
    { value: "meta", label: t("tabs.meta") },
    { value: "decision", label: t("tabs.decision") },
  ];
  const links: DecisionLinks =
    mode.kind === "recorded"
      ? {
          receipt: api.showcaseReceiptUrl(runId),
          log:
            archive === null
              ? null
              : { href: api.researchUrl(archive.file), name: baseName(archive.file), archive: true },
          logPending: archive === null,
        }
      : {
          receipt: api.receiptUrl(runId),
          log: ndjsonPublishable(summary.provenance)
            ? { href: api.ndjsonUrl(runId), name: null, archive: false }
            : null,
        };

  return (
    <div
      className="hc-run"
      data-run-id={runId}
      data-run-state={summary.state}
      data-run-mode={recorded ? "recorded" : "own"}
    >
      <header className="hc-run-header hc-panel">
        <h2 className="hc-run-title">{title}</h2>
        <dl className="hc-kv">
          <div>
            <dt>{t("header.state")}</dt>
            <dd data-run-field="state">{stateText}</dd>
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
        {mode.kind === "recorded" ? (
          <RecordedNotes locale={locale} detail={detail} data={mode.data} entry={archive} />
        ) : null}
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
        items={items}
      />

      <div
        id="run-tabpanel"
        className="hc-tabpanel"
        role="tabpanel"
        aria-labelledby={tabId("run-tabpanel", tab)}
        data-tab={tab}
      >
        {tab === "stage" && mode.kind === "own" ? <StageTab data={mode.data} /> : null}
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
            archive={archive}
          />
        ) : null}
        {tab === "meta" ? <MetaTab detail={detail} frames={frames} detailError={detailError} /> : null}
        {tab === "decision" ? (
          <DecisionTab
            runId={runId}
            data={mode.data}
            links={links}
            onDetail={mode.kind === "own" ? mode.data.setDetail : undefined}
          />
        ) : null}
      </div>
    </div>
  );
}

/**
 * What the archive data says about a recorded run: that it is recorded and
 * read-only, the pre-1.1 label of an unchained run, the reading of a run
 * blocked because no proof could be generated upstream, and the archive
 * manifest's own note on the log the run is a segment of.
 */
function RecordedNotes({
  locale,
  detail,
  data,
  entry,
}: {
  locale: Locale;
  detail: RunDetailV1 | null;
  data: RecordedRunData;
  entry: ShowcaseEntryV1 | null;
}) {
  const t = useTranslations("Showcase.run");
  const tShowcase = useTranslations("Showcase");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const failures = useMemo(() => (detail === null ? [] : generationFailures(evidenceView(detail, []))), [detail]);
  const summary = detail?.summary ?? null;
  // The source axis as provenance.ts labels it: RECORDED.
  const source = sourceLabel(summary?.provenance.source ?? "research_archive");
  const unchained =
    detail?.chain.scope === "unchained" || (summary?.provenance.flags ?? []).includes("unchained");
  const manifest =
    data.archive.status === "ready" &&
    data.archive.manifest.status === "ready" &&
    data.archive.manifest.matches
      ? data.archive.manifest.manifest
      : null;
  const note = entry === null ? null : manifestNote(manifest, entry.file);
  return (
    <section className="hc-recorded" aria-labelledby="recorded-heading" data-recorded-run>
      <h3 id="recorded-heading">
        <Badge variant={source.variant} label={tProvenance(source.key)} /> {t("heading")}
      </h3>
      <p>
        {entry === null
          ? t("bannerNoEntry")
          : t("banner", { label: entry.label, battery: entry.battery })}
      </p>
      {unchained ? (
        <p data-archive-label="unchained">
          <Badge variant="warning" label={tShowcase("unchained")} /> {t("unchainedNote")}
        </p>
      ) : null}
      {summary?.recommendation === "blocked" && failures.length > 0 ? (
        <p data-archive-label="generation-failed">{t("generationFailed", { count: failures.length })}</p>
      ) : null}
      {note === null ? null : (
        <div data-archive-note>
          <p className="hc-supporting">
            {manifest?.created === null || manifest?.created === undefined
              ? tShowcase("noteHeading")
              : tShowcase("noteHeadingDated", { created: manifest.created })}
          </p>
          <UntrustedText text={note} source="archive" />
        </div>
      )}
      <p>
        <Link href={localePath(locale, "showcase")} className="hc-link">
          {t("back")}
        </Link>
      </p>
    </section>
  );
}
