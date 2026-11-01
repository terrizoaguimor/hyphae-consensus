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
//   by src/lib/provenance.ts isLive), Decision, Explorer, Evidence, Proof
//   inspector, Audit trail and Meta API (src/lib/run/decision.ts
//   runTabOrder); while the run awaits a human decision the header says so
//   and offers "Go to your decision" on every tab;
// - a recorded run of the research archive (`?id=<run_id>&source=showcase`,
//   RECORDED mode): useRecordedRun (GET /showcase/{id}, the showcase entry
//   and the archive manifest's note) and its receipt (GET
//   /showcase/{id}/receipt); no stream and no stage, nothing runs, the
//   Decision tab comes first and is read-only, and the header says it is a recorded run with
//   the labels the archive data carries (schema 1.0: pre-1.1, unchained; a
//   run blocked because no proof could be generated upstream says so, with
//   the manifest's own note on its log).

import { Button } from "@astryxdesign/core/Button";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useFormatter, useTranslations } from "next-intl";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { api, isRunId } from "@/lib/api/client";
import type { DecisionV1, ProvenanceFlag, RunDetailV1, ShowcaseEntryV1 } from "@/lib/api/types";
import {
  DECISION_ICONS,
  isLive,
  isScriptedRun,
  ndjsonPublishable,
  OUTCOME_ICONS,
  RECOMMENDATION_VARIANTS,
  sourceLabel,
} from "@/lib/provenance";
import { labelDisagreements } from "@/lib/run/consistency";
import { awaitsDecision, runTabOrder, type RunTabKey } from "@/lib/run/decision";
import { evidenceView, generationFailures } from "@/lib/run/evidence";
import { againPath, visitorRunKind } from "@/lib/run/failure";
import { baseName } from "@/lib/run/receipt";
import {
  awaitsFirstFrame,
  decisionOf,
  followsPlayback,
  headerSummary,
  mergeEvents,
  type PlaybackPosition,
  streamSettled,
  supersededProposal,
} from "@/lib/run/view";
import { manifestNote } from "@/lib/showcase";
import { localePath, SHOWCASE_SOURCE } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { Callout } from "../Callout";
import { OutcomeTags } from "../OutcomeTags";
import { ProvenanceBadges } from "../ProvenanceBadges";
import { Tag } from "../Tag";
import { useManifestDate } from "../showcase/ShowcaseList";
import { archivePin, checkedManifest } from "../showcase/useArchive";
import { AuditTab } from "./AuditTab";
import { type ChainBreak, ChainBroken } from "./ChainBroken";
import { DecisionTab, type DecisionLinks } from "./DecisionTab";
import { EvidenceTab } from "./EvidenceTab";
import { ExplorerTab } from "./ExplorerTab";
import { FailureSentence, useVisitorAccessHours } from "./FailureSentence";
import { MetaTab } from "./MetaTab";
import { ProofsTab } from "./ProofsTab";
import { StageTab } from "./StageTab";
import { tabId, Tabs } from "./Tabs";
import { UntrustedText } from "./UntrustedText";
import { useRunTimeReceipt } from "./RunTimeReceipt";
import { type RunReceipt, useReceipt } from "./useReceipt";
import { missingEntry, type RecordedRunData, useRecordedRun } from "./useRecordedRun";
import { type RunData, useRunData } from "./useRunData";

type RunWorkspaceProps = {
  locale: Locale;
};

type TabKey = RunTabKey;

/** The flag whose note the header shows in its own full-width callout, not under the labels. */
const INCONSISTENT: readonly ProvenanceFlag[] = ["provenance_inconsistent"];

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
  const tConsistency = dynamicTranslator(useTranslations("Run.consistency"));
  const format = useFormatter();
  const recorded = mode.kind === "recorded";
  const [tab, setTab] = useState<TabKey>(recorded ? "decision" : "stage");
  // The decision this session recorded from the Decision tab (the server's answer).
  const [decided, setDecided] = useState<DecisionV1 | null>(null);
  const { detail, detailError, summary, frames, reloadDetail } = mode.data;
  const archive = mode.kind === "recorded" ? mode.data.entry : null;
  // The entry's log digest, cross-checked against the manifest this browser checked.
  const pin =
    mode.kind === "recorded" && archive !== null
      ? archivePin(mode.data.archive, archive.file, archive.sha256)
      : null;
  // A recorded run's record (the receipt as downloaded at run time), read on request.
  const runTime = useRunTimeReceipt(runId, archive?.record_file ?? null);
  // The receipt is read when a tab that shows it is first opened.
  const selectTab = (next: TabKey) => {
    setTab(next);
    if (RECEIPT_TABS.has(next)) {
      receipt.ensure();
    }
  };
  // "Go to your decision" opens the Decision tab and moves the keyboard
  // focus (and the view) to the decision form's heading once it is drawn.
  const focusDecision = useRef(false);
  const goToDecision = () => {
    focusDecision.current = true;
    selectTab("decision");
  };
  useEffect(() => {
    if (!focusDecision.current || tab !== "decision") {
      return;
    }
    const heading = document.getElementById("decision-form");
    if (heading !== null) {
      focusDecision.current = false;
      heading.focus({ preventScroll: true });
      // The card's top at the top of the view (under the sticky top bar,
      // through scroll-padding-top), so its buttons show with it.
      heading.scrollIntoView({ block: "start" });
    }
  });
  // Once read, the receipt is read again whenever the detail moves on (a
  // decision, the end of the stream, a re-read after a gap): the server
  // rebuilds it from the run's current records, so the copy held would be
  // stale (an authorization still pending, an older head and count).
  // A layout effect: the newer receipt is asked for (and marked pending)
  // before the moved-on detail is painted, so the page never compares it
  // with the stale receipt (the "receipt disagrees" callout).
  const detailSequence = detail?.summary.last_sequence ?? null;
  const { refresh: refreshReceipt } = receipt;
  useLayoutEffect(() => {
    if (detailSequence !== null) {
      refreshReceipt();
    }
  }, [detailSequence, refreshReceipt]);
  // Where a chain check run on this page failed: a state of this page (not
  // of the run), shown on every tab until a later check does not fail.
  const [chainBreak, setChainBreak] = useState<ChainBreak | null>(null);
  // The page's own check: provenance labels that its records contradict.
  const provenance = summary?.provenance ?? null;
  const disagreements = useMemo(
    () => (provenance === null ? [] : labelDisagreements(provenance, mergeEvents(detail, frames))),
    [provenance, detail, frames],
  );

  // One clock: during a paced playback the header shows the run as of the
  // newest record on the stage, so no outcome (and no decision) appears
  // before the playback reaches it (view.ts headerSummary).
  const position: PlaybackPosition | null =
    mode.kind === "own"
      ? {
          pacedFrames: mode.data.pacedFrames,
          pending: mode.data.pending,
          settled: streamSettled(mode.data.stream),
        }
      : null;
  // A scripted run about to be paced whose first frame has not arrived yet:
  // its stored summary is the ending, so the header shows none of it (no
  // state, outcome or decision) until the playback starts. The workspace,
  // its tabs and the stream's status line (with its reconnect control) stay
  // visible; a failed or closed stream settles it, and the stored run shows.
  const beforePlayback =
    mode.kind === "own" &&
    summary !== null &&
    awaitsFirstFrame({
      scripted: isScriptedRun(summary.provenance),
      pacing: mode.data.pacing,
      framesShown: frames.length,
      settled: streamSettled(mode.data.stream),
    });
  const following = beforePlayback || (position !== null && followsPlayback(position));
  const shown = position === null ? summary : headerSummary(detail, frames, position);
  // A failed run of this browser offers another one; the note under the
  // link says how long this one stays in My runs (the capabilities' limits).
  const failedOwnRun =
    mode.kind === "own" && shown !== null && !beforePlayback && (shown.failure ?? null) !== null;
  const accessHours = useVisitorAccessHours(failedOwnRun);

  if (summary === null || shown === null) {
    if (detailError !== null && summary === null) {
      return (
        <div className="hc-section" data-run="error">
          <ApiErrorNotice error={detailError} onRetry={reloadDetail} />
          {/* A session a restart ended (a server that keeps its sessions in
              memory only) makes this browser's own run read as another
              session's: the hint says so, without guessing which case it is. */}
          {!recorded && detailError.code === "not_found" ? (
            <p className="hc-supporting" data-run-restart-hint>
              {t("restartHint")}
            </p>
          ) : null}
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
  const recordedTitle = detail?.task?.title ?? summary.title ?? null;
  const title = recordedTitle ?? t("title");
  // A recorded run's title is the archive's English task text; an own run's
  // title is the requester's text in any language, and the fallback is
  // localized, so only the former is marked lang="en".
  const titleLang = recorded && recordedTitle !== null ? "en" : undefined;
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
  const stateText = beforePlayback
    ? t("header.notShown")
    : recorded && shown.state === "awaiting_human_approval"
      ? tShowcase("awaiting")
      : t(`states.${shown.state}`);
  // The person's decision, as its own tag next to the policy's outcome.
  const decision = beforePlayback ? null : decisionOf(shown);
  // Before the playback starts the header shows no outcome of the ending.
  const outcome = beforePlayback ? null : (shown.recommendation ?? null);
  const failure = beforePlayback ? null : (shown.failure ?? null);
  // A live run of the public surface: its failures may not count toward the
  // day's live runs, and its cost is counted in the public live budget.
  const publicLive = !recorded && visitorRunKind(summary.provenance) === "live";
  const again = failedOwnRun ? againPath(localePath(locale, "new"), summary.provenance) : null;
  // Decision comes right after the first tab (first for a recorded run, whose default tab it is).
  const items: Array<{ value: TabKey; label: string }> = runTabOrder(recorded).map((value) => ({
    value,
    label: value === "stage" ? stageLabel : t(`tabs.${value}`),
  }));
  // A decision can still be recorded on this run here (the chain-broken
  // callouts then say "before you decide"). A decision sent from the
  // Decision tab counts at once, even when the detail could not be read
  // again after it (the tab shows it as recorded).
  const deciding = !recorded && awaitsDecision(summary, detail?.decision ?? decided);
  // The header offers "Go to your decision" on every tab, on the header's
  // one clock: only once the playback shows the run at its decision point
  // (never before the stage reaches the recommendation, HE #6).
  const awaiting = deciding && !beforePlayback && shown.state === "awaiting_human_approval";
  const inconsistent = (summary.provenance.flags ?? []).includes("provenance_inconsistent");
  const openAudit = () => selectTab("audit");
  const links: DecisionLinks =
    mode.kind === "recorded"
      ? {
          receipt: api.showcaseReceiptUrl(runId),
          log:
            archive === null
              ? null
              : { href: api.researchUrl(archive.file), name: baseName(archive.file), archive: true },
          logMissing: missingEntry(mode.data),
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
      <header
        className="hc-run-header hc-panel hc-glass hc-glass--static"
        data-header-clock={following ? "playback" : "run"}
        data-header-state={beforePlayback ? "not_shown" : shown.state}
      >
        <h2 className="hc-run-title" lang={titleLang}>
          {title}
        </h2>
        {following ? (
          <p className="hc-supporting hc-tabular-nums" data-header-playback>
            {t("header.playback", {
              shown: beforePlayback ? 0 : shown.event_count,
              total: summary.event_count,
            })}
          </p>
        ) : null}
        {awaiting ? (
          <div className="hc-go-to-decision" data-go-to-decision>
            <p>{t("header.awaitingDecision")}</p>
            <Button
              variant="primary"
              className="hc-decide-button"
              label={t("header.goToDecision")}
              onClick={goToDecision}
            />
          </div>
        ) : null}
        <dl className="hc-kv">
          <div>
            <dt>{t("header.state")}</dt>
            <dd data-run-field="state">{stateText}</dd>
          </div>
          <div>
            <dt>{t("header.outcome")}</dt>
            <dd data-run-field="outcome">
              {outcome === null ? (
                !beforePlayback && shown.state === "failed" ? (
                  t("header.noOutcomeFailed")
                ) : (
                  t("header.noOutcome")
                )
              ) : (
                <>
                  <OutcomeTags
                    outcome={t(`kinds.${outcome}`)}
                    variant={RECOMMENDATION_VARIANTS[outcome]}
                    icon={OUTCOME_ICONS[outcome]}
                    evidence={shown.provenance.evidence}
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
          {decision === null ? null : (
            <div>
              <dt>{recorded ? t("header.decisionRecorded") : t("header.decision")}</dt>
              <dd data-run-field="decision" data-decision={decision}>
                {/* The person's decision, its own tag: the same info role for
                    an approval and a rejection, never green and never a
                    plain grey word next to the policy's coloured outcome.
                    The no-authority statement is the Decision tab's, once
                    per view, not repeated here. */}
                <Tag variant="info" icon={DECISION_ICONS[decision]} label={t(`header.decisions.${decision}`)} />
              </dd>
            </div>
          )}
          <div>
            <dt>{t("header.recordsLabel")}</dt>
            <dd className="hc-tabular-nums" data-run-field="records">
              {t("header.records", { count: beforePlayback ? 0 : shown.event_count })}
            </dd>
          </div>
          <div>
            <dt>{t("header.revision")}</dt>
            <dd className="hc-tabular-nums" data-run-field="revision">
              {beforePlayback ? t("header.notShown") : format.number(shown.revision_round)}
            </dd>
          </div>
          <div>
            <dt>{t("header.runId")}</dt>
            <dd>
              <code className="hc-code">{shown.run_id}</code>
            </dd>
          </div>
        </dl>
        {failure === null ? null : (
          <FailureSentence
            failure={failure}
            own={mode.kind === "own"}
            live={publicLive}
            again={again}
            accessHours={accessHours}
          />
        )}
        <div>
          <p className="hc-supporting">{t("header.provenance")}</p>
          <ProvenanceBadges
            provenance={shown.provenance}
            delivery={latest?.delivery}
            recordedAt={
              latest?.event.occurred_at ?? (beforePlayback ? shown.created_at : shown.updated_at)
            }
            withNote
            notesElsewhere={inconsistent ? INCONSISTENT : undefined}
          />
        </div>
        {/* Full width under the labels, so a warning is never one more equal tag in the row. */}
        {inconsistent ? (
          <Callout tone="contested" title={tConsistency("flagTitle")} data-provenance-flag-callout="provenance_inconsistent">
            <p>{tProvenance("flagNotes.provenance_inconsistent")}</p>
            {tab === "audit" ? null : (
              <div className="hc-actions">
                <Button variant="secondary" size="sm" label={t("chainBroken.openAudit")} onClick={openAudit} />
              </div>
            )}
          </Callout>
        ) : null}
        {disagreements.length === 0 ? null : (
          <Callout
            tone="contested"
            title={tConsistency("labelsTitle")}
            data-labels-disagree={disagreements.map((d) => d.kind).join(" ")}
          >
            <ul className="hc-list">
              {disagreements.map((disagreement) => (
                <li key={disagreement.kind} data-labels-disagreement={disagreement.kind}>
                  {tConsistency(`labels.${disagreement.kind}`, { count: disagreement.count })}
                </li>
              ))}
            </ul>
            <p>{tConsistency("labelsBody")}</p>
          </Callout>
        )}
        {chainBreak === null ? null : (
          <ChainBroken
            chainBreak={chainBreak}
            place="header"
            deciding={deciding}
            onOpenAudit={tab === "audit" ? undefined : openAudit}
          />
        )}
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
            archive={archive}
            runTime={runTime}
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
            pin={pin}
            archiveMissing={mode.kind === "recorded" ? missingEntry(mode.data) : null}
            runTime={runTime}
            onChainOutcome={setChainBreak}
          />
        ) : null}
        {tab === "meta" ? (
          <MetaTab detail={detail} frames={frames} detailError={detailError} publicLive={publicLive} />
        ) : null}
        {tab === "decision" ? (
          <DecisionTab
            runId={runId}
            data={mode.data}
            links={links}
            onDetail={mode.kind === "own" ? mode.data.setDetail : undefined}
            decided={decided}
            onDecided={mode.kind === "own" ? setDecided : undefined}
            chainBreak={chainBreak}
            onOpenAudit={openAudit}
          />
        ) : null}
      </div>
    </div>
  );
}

/**
 * What the generation-failed sentence may say about the archive's note on
 * the run's log: that it names the cause only when the note is on the page
 * ("shown"); that it is withheld when the manifest could not be read or
 * parsed ("unread") or its bytes do not match the published digest
 * ("differs"); nothing while the manifest is still being checked, or when
 * there is no listing entry or no note for the log to refer to.
 */
function generationFailedNote(
  data: RecordedRunData,
  entry: ShowcaseEntryV1 | null,
  note: string | null,
): "shown" | "unread" | "differs" | null {
  if (note !== null) {
    return "shown";
  }
  if (entry === null || data.archive.status !== "ready") {
    return null;
  }
  const state = data.archive.manifest;
  switch (state.status) {
    case "loading":
      return null;
    case "error":
      return "unread";
    case "ready":
      return !state.matches ? "differs" : state.manifest === null ? "unread" : null;
  }
}

/**
 * Why the archive's manifest lends nothing to this run's log, once that is
 * known: it could not be downloaded ("unread", with its retry), it does not
 * read as the archive's format ("unparsed"), or its bytes do not match the
 * published digest ("differs"); null while it is being checked, when it was
 * checked, or when there is no listing entry whose log it would speak of.
 */
function manifestProblem(
  data: RecordedRunData,
  entry: ShowcaseEntryV1 | null,
): "unread" | "unparsed" | "differs" | null {
  if (entry === null || data.archive.status !== "ready") {
    return null;
  }
  const state = data.archive.manifest;
  switch (state.status) {
    case "loading":
      return null;
    case "error":
      return "unread";
    case "ready":
      return !state.matches ? "differs" : state.manifest === null ? "unparsed" : null;
  }
}

/**
 * What the archive data says about a recorded run: that it is recorded and
 * read-only, the pre-1.1 label of an unchained run, the reading of a run
 * blocked because no proof could be generated upstream, and the archive
 * manifest's own note on the log the run is a segment of. A listing or a
 * manifest that could not be read is said here, with its retry, and with
 * what it leaves out.
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
  const manifestDate = useManifestDate();
  const failures = useMemo(() => (detail === null ? [] : generationFailures(evidenceView(detail, []))), [detail]);
  const summary = detail?.summary ?? null;
  // The source axis as provenance.ts labels it: RECORDED.
  const source = sourceLabel(summary?.provenance.source ?? "research_archive");
  const unchained =
    detail?.chain.scope === "unchained" || (summary?.provenance.flags ?? []).includes("unchained");
  const manifest = checkedManifest(data.archive);
  const note = entry === null ? null : manifestNote(manifest, entry.file);
  const noteReference = generationFailedNote(data, entry, note);
  const problem = manifestProblem(data, entry);
  return (
    <section className="hc-recorded" aria-labelledby="recorded-heading" data-recorded-run>
      <h3 id="recorded-heading">
        <Tag variant={source.variant} labelKey={source.key} label={tProvenance(source.key)} /> {t("heading")}
      </h3>
      <p>
        {entry === null
          ? t("bannerNoEntry")
          : t("banner", { label: entry.label, battery: entry.battery })}
      </p>
      {data.archive.status === "error" ? (
        <div data-archive-error>
          <p>{t("listingFailed")}</p>
          <ApiErrorNotice error={data.archive.error} onRetry={data.reloadArchive} />
        </div>
      ) : null}
      {problem === null ? null : (
        // A failed callout for a manifest that does not match its digest; a manifest
        // that could not be read or understood is contested: not confirmed either way.
        <Callout
          tone={problem === "differs" ? "failed" : "contested"}
          icon="file-x"
          title={t("manifestProblemTitle")}
          data-manifest-problem-callout={problem}
        >
          <div data-manifest-problem={problem}>
            <p>{t(`manifestProblem.${problem}`)}</p>
            {data.archive.status === "ready" && data.archive.manifest.status === "error" ? (
              <ApiErrorNotice error={data.archive.manifest.error} onRetry={data.retryManifest} />
            ) : null}
          </div>
        </Callout>
      )}
      {unchained ? (
        <p data-archive-label="unchained">
          <Tag variant="warning" icon="link-2-off" label={tShowcase("unchained")} /> {t("unchainedNote")}
        </p>
      ) : null}
      {summary?.recommendation === "blocked" && failures.length > 0 ? (
        <p data-archive-label="generation-failed">
          {t("generationFailed", { count: failures.length })}
          {noteReference === null ? null : (
            <>
              {" "}
              <span data-archive-note-reference={noteReference}>{t(`generationFailedNote.${noteReference}`)}</span>
            </>
          )}
        </p>
      ) : null}
      {note === null ? null : (
        <div data-archive-note>
          <p className="hc-supporting">
            {manifest?.created === null || manifest?.created === undefined
              ? tShowcase("noteHeading")
              : tShowcase("noteHeadingDated", { created: manifestDate(manifest.created) })}
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
