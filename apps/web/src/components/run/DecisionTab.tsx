"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Decision tab: the recommendation kind with its evidence axis and,
// right under it, the scope line when the recommendation is for a revised
// proposal (which proposal it is for, which one it replaced, the task, both
// summaries side by side: K03/K04), the rationale, the unresolved
// assumptions and uncertainties, the open objections, and the public
// decision form (approve or reject, optional note) with the fixed
// public_demo notice, enabled only while the run awaits a human decision and
// belongs to this session (GET /runs/{id} answers 404 for anyone else). The
// recorded decision carries the server's notice; the receipt is linked, and
// the verbatim log only when the public surface publishes it
// (src/lib/provenance.ts ndjsonPublishable; a live run's answers 404
// ndjson_unavailable), or for a recorded run of the research archive the
// archived log file it is a segment of (the workspace passes the links). A
// recorded run is read-only: its source is the archive, so the form never
// opens. A run still deliberating can be cancelled. A failed detail request
// is said as such, never shown as loading.
//
// While a decision can be recorded, and once one is, the decision card comes
// first in the tab, before the long explanation (src/lib/run/decision.ts):
// it names the recommendation it is about, then the notice, the note and
// two full-size buttons of equal weight. On a blocked or abstained run the
// Approve button reads "Approve as an override" and the note is marked
// required for this run, with the override said in body text before any
// click; neither button is promoted over the other.

import { Button } from "@astryxdesign/core/Button";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useFormatter, useTranslations } from "next-intl";
import { useEffect, useRef, useState } from "react";

import { api, ApiError, isApiError, newIdempotencyKey } from "@/lib/api/client";
import { decisionOutcomeUnknown } from "@/lib/api/errors";
import type { DecisionResponseV1, DecisionV1, RunDetailV1 } from "@/lib/api/types";
import { awaitsDecision, DECIDE_BUTTON_VARIANT, decisionFormCopy, isOverride } from "@/lib/run/decision";
import { explainContext } from "@/lib/run/explain";
import { supersededProposal } from "@/lib/run/view";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { Tag } from "../Tag";
import { type ChainBreak, ChainBroken } from "./ChainBroken";
import { Explanation } from "./Explanation";
import { RevisionComparison } from "./RevisionComparison";
import { RecommendationLine, SimulatedRationaleNote } from "./StageTab";
import type { MissingEntry } from "./useRecordedRun";
import type { RunData } from "./useRunData";

/** Where the receipt and the verbatim log are, for this run's source. */
export type DecisionLinks = {
  receipt: string;
  /**
   * The verbatim log, when the public surface publishes it (`name` is the
   * file name to save it under, `archive` for a recorded run's research
   * file); null when it is not published.
   */
  log: { href: string; name: string | null; archive: boolean } | null;
  /**
   * A recorded run without its archive entry (its log file): the listing is
   * being read, could not be read (its error and retry are in the header),
   * or does not list the run.
   */
  logMissing?: MissingEntry | null;
};

/** The sentence in place of a recorded run's log link, by why it is missing. */
const MISSING_LOG = {
  loading: "archiveLogLoading",
  error: "archiveLogFailed",
  unlisted: "archiveLogPending",
} as const;

type DecisionTabProps = {
  runId: string;
  data: Pick<RunData, "detail" | "detailError" | "summary">;
  links: DecisionLinks;
  /** Replaces the workspace's detail after a decision (a run of this server only). */
  onDetail?: (detail: RunDetailV1) => void;
  /**
   * The decision this session recorded, held by the workspace so its header
   * and this tab agree even when the detail could not be read again after it.
   */
  decided?: DecisionV1 | null;
  /** Called with the decision the server recorded, before the detail is read again. */
  onDecided?: (decision: DecisionV1) => void;
  /** Where a chain check run in this browser failed (it travels here, next to the decision); null when none did. */
  chainBreak?: ChainBreak | null;
  /** Opens the Audit trail, where the failed check is. */
  onOpenAudit?: () => void;
};

function toApiError(error: unknown, path: string): ApiError {
  return isApiError(error)
    ? error
    : new ApiError({
        code: "unreachable",
        message: error instanceof Error ? error.message : "request failed",
        status: 0,
        path,
      });
}

export function DecisionTab({
  runId,
  data,
  links,
  onDetail,
  decided,
  onDecided,
  chainBreak = null,
  onOpenAudit,
}: DecisionTabProps) {
  const t = useTranslations("Run.decision");
  const tRun = useTranslations("Run");
  const format = useFormatter();
  const { detail, detailError, summary } = data;
  const [approvedChoice, setApprovedChoice] = useState<boolean | null>(null);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<DecisionResponseV1 | null>(null);
  // A decision's error and a cancel's error are kept apart: a refused cancel
  // is not headed "Your decision was not recorded" (no decision was sent).
  const [error, setError] = useState<ApiError | null>(null);
  const [cancelError, setCancelError] = useState<ApiError | null>(null);
  const [cancelling, setCancelling] = useState<"idle" | "requested">("idle");
  // Where the keyboard focus goes when a decision sent from this tab
  // replaces the form (the pressed button is gone with it): the recorded
  // decision, else the form's heading; never <body>.
  const refocus = useRef(false);
  const recordedRef = useRef<HTMLDivElement>(null);
  const formHeadingRef = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (!refocus.current) {
      return;
    }
    const active = document.activeElement;
    if (active === null || active === document.body) {
      (recordedRef.current ?? formHeadingRef.current)?.focus();
    }
    if (!submitting) {
      refocus.current = false;
    }
  });

  if (detail === null || summary === null) {
    // A settled failure is said as such (the error and its retry are in the
    // workspace above); only a request still in flight reads "Loading".
    return detailError !== null ? (
      <p className="hc-supporting" data-decision-unavailable="detail-error">
        {t("detailUnavailable")}
      </p>
    ) : (
      <p role="status" className="hc-supporting">
        {tRun("loading")}
      </p>
    );
  }

  const recommendation = detail.recommendation ?? null;
  const openItems = detail.challenge_items.filter((item) => item.status === "open");
  const proofContext = explainContext(detail);
  const simulatedProofs = proofContext.simulatedProofs;
  const mixedProofs = proofContext.mixedProofs === true;
  const original = supersededProposal(detail);
  const current = detail.current_proposal ?? null;
  // The recommendation is for a revision: say which proposal it is for and
  // which one it replaced, from the records (K03/K04).
  const revised =
    original !== null && current !== null
      ? { id: current.id, summary: current.summary, original }
      : null;
  const recorded: DecisionV1 | null = result?.decision ?? detail.decision ?? decided ?? null;
  const awaiting = awaitsDecision(detail.summary, recorded);
  const kind = recommendation?.kind ?? detail.summary.recommendation ?? null;
  const overrideNeeded = approvedChoice === true && isOverride(kind) && note.trim() === "";
  // On a blocked or abstained run the form says the override before any click.
  const copy = decisionFormCopy(kind);
  // While a decision can be recorded, and once one is, the decision card
  // comes first in the tab (before the long explanation); otherwise it keeps
  // its place after the recommendation and the open objections.
  const cardFirst = awaiting || recorded !== null;

  const submit = async (approved: boolean) => {
    // The pressed button is busy (aria-disabled, still focusable) rather than
    // disabled, so a second press reaches here and is dropped.
    if (kind === null || submitting) {
      return;
    }
    setApprovedChoice(approved);
    if (approved && isOverride(kind) && note.trim() === "") {
      return;
    }
    refocus.current = true;
    setSubmitting(true);
    setError(null);
    try {
      const response = await api.decide(
        runId,
        {
          approved,
          note,
          expected: { recommendation: kind, lastSequence: detail.summary.last_sequence },
        },
        newIdempotencyKey(),
      );
      setResult(response);
      onDecided?.(response.decision);
      const refreshed = await api.getRun(runId).catch(() => null);
      if (refreshed !== null) {
        onDetail?.(refreshed);
      }
    } catch (cause) {
      const apiError = toApiError(cause, `/runs/${runId}/decision`);
      setError(apiError);
      // A conflict, or a failure after which the decision may have been
      // recorded (no answer, a timeout, an unreadable answer), re-reads the
      // run, so the page shows what the server holds.
      if (apiError.code === "decision_conflict" || decisionOutcomeUnknown(apiError.code)) {
        const refreshed = await api.getRun(runId).catch(() => null);
        if (refreshed !== null) {
          onDetail?.(refreshed);
        }
      }
    } finally {
      setSubmitting(false);
    }
  };

  const cancel = async () => {
    setCancelError(null);
    try {
      await api.cancel(runId);
      setCancelling("requested");
    } catch (cause) {
      setCancelError(toApiError(cause, `/runs/${runId}/cancel`));
    }
  };

  const dateTime = (iso: string) =>
    format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "short" });

  // The decision card: the recommendation it is about (with its evidence
  // axis) while a decision can be recorded, the no-authority notice, then
  // the recorded decision, the form, or why there is none; the receipt and
  // the log.
  const card = (
    <section
      className="hc-section hc-decision-card hc-glass hc-glass--2 hc-glass--float"
      aria-labelledby="decision-form"
      data-decision-card={cardFirst ? "first" : "after"}
    >
      <h3 id="decision-form" ref={formHeadingRef} tabIndex={-1}>
        {t("formHeading")}
      </h3>
      {chainBreak === null ? null : (
        <ChainBroken chainBreak={chainBreak} place="decision" deciding={awaiting} onOpenAudit={onOpenAudit} />
      )}
      {awaiting && recommendation !== null ? (
        <div data-decision-card-lead>
          <p>{t("cardLead")}</p>
          <RecommendationLine kind={recommendation.kind} evidence={detail.summary.provenance.evidence} />
          <p className="hc-supporting">{t("cardReasons")}</p>
        </div>
      ) : null}
      <p className="hc-build-note" data-public-demo-notice>
        {t("publicDemo")}
      </p>
      {recorded !== null ? (
        <div
          className="hc-panel"
          data-decision-recorded={recorded.approved ? "approved" : "rejected"}
          ref={recordedRef}
          tabIndex={-1}
        >
          <p>
            {t("recorded", {
              decision: recorded.approved ? t("approved") : t("rejected"),
              by: recorded.authorized_by,
              date: dateTime(recorded.decided_at),
            })}
          </p>
          {recorded.note === undefined || recorded.note === null ? null : (
            <p>{t("withNote", { note: recorded.note })}</p>
          )}
          {recorded.notice === undefined || recorded.notice === null ? null : (
            <p className="hc-supporting" data-decision-notice>
              {recorded.notice}
            </p>
          )}
        </div>
      ) : awaiting ? (
        <form
          className="hc-form"
          data-decision-form
          data-override={copy.overrideNotice === null ? "no" : "yes"}
          onSubmit={(event) => event.preventDefault()}
        >
          {/* An override is said before any click, in body text, not in helper text. */}
          {copy.overrideNotice === null || kind === null ? null : (
            <p className="hc-override-notice" data-override-notice>
              {t(copy.overrideNotice, { kind: tRun(`kinds.${kind}`) })}
            </p>
          )}
          <TextArea
            label={t(copy.noteLabel)}
            description={t(copy.noteDescription)}
            value={note}
            onChange={(value) => setNote(value)}
            isOptional={copy.noteOptional}
          />
          {overrideNeeded && kind !== null ? (
            <p className="hc-supporting" role="alert">
              {t("overrideRequired", { kind: tRun(`kinds.${kind}`) })}
            </p>
          ) : null}
          {/* Approve and Reject have equal weight on every run: the buttons never take the policy's side. */}
          <div className="hc-actions hc-decide-actions">
            {/* The pressed button is busy, never disabled: a disabled button would drop the keyboard focus. */}
            <Button
              variant={DECIDE_BUTTON_VARIANT}
              className="hc-decide-button"
              data-decide="approve"
              label={submitting && approvedChoice === true ? t("submitting") : t(copy.approve)}
              isDisabled={kind === null || (submitting && approvedChoice !== true)}
              isLoading={submitting && approvedChoice === true}
              onClick={() => void submit(true)}
            />
            <Button
              variant={DECIDE_BUTTON_VARIANT}
              className="hc-decide-button"
              data-decide="reject"
              label={submitting && approvedChoice === false ? t("submitting") : t("reject")}
              isDisabled={kind === null || (submitting && approvedChoice !== false)}
              isLoading={submitting && approvedChoice === false}
              onClick={() => void submit(false)}
            />
          </div>
        </form>
      ) : (
        <p className="hc-supporting" data-decision-unavailable>
          {detail.summary.provenance.source === "research_archive"
            ? t("recordedReadOnly")
            : t("notAwaiting")}
        </p>
      )}
      {error === null ? null : <ApiErrorNotice error={error} context="decision" />}
      <ul className="hc-related" role="list">
        <li>
          <a href={links.receipt} className="hc-link" data-receipt-link>
            {t("receipt")}
          </a>
          {detail.summary.provenance.source === "research_archive" ? (
            <p className="hc-supporting" data-receipt-rebuilt>
              {t("receiptRebuilt")}
            </p>
          ) : null}
        </li>
        <li>
          {links.log !== null ? (
            <a
              href={links.log.href}
              download={links.log.name ?? undefined}
              className="hc-link"
              data-ndjson-link
            >
              {links.log.archive ? t("archiveLog", { name: links.log.name ?? "" }) : t("ndjson")}
            </a>
          ) : (
            <span
              className="hc-supporting"
              data-ndjson-unavailable={links.logMissing ?? "unpublished"}
              role={links.logMissing === "loading" ? "status" : undefined}
            >
              {links.logMissing === undefined || links.logMissing === null
                ? t("ndjsonUnavailable")
                : t(MISSING_LOG[links.logMissing])}
            </span>
          )}
        </li>
      </ul>
    </section>
  );

  return (
    <div className="hc-decision" data-decision>
      {cardFirst ? card : null}
      <section className="hc-section" aria-labelledby="decision-recommendation">
        <h3 id="decision-recommendation">{t("recommendation")}</h3>
        {recommendation === null ? (
          <p className="hc-supporting" data-recommendation-none={detail.summary.state}>
            {detail.summary.state === "failed" ? t("failedNoRecommendation") : t("pending")}
          </p>
        ) : (
          <div className="hc-panel">
            <RecommendationLine kind={recommendation.kind} evidence={detail.summary.provenance.evidence} />
            {revised === null ? null : (
              <div className="hc-scope-change" data-superseded>
                <p data-superseded-line>
                  {t("superseded", { revised: revised.id, original: revised.original.id })}
                </p>
                {detail.task === undefined || detail.task === null ? null : (
                  <p className="hc-supporting">{t("supersededTask", { title: detail.task.title })}</p>
                )}
                <div className="hc-side-by-side">
                  <div className="hc-panel">
                    <h4>{t("original", { id: revised.original.id })}</h4>
                    <p>{revised.original.summary}</p>
                  </div>
                  <div className="hc-panel">
                    <h4>{t("revised", { id: revised.id })}</h4>
                    <p>{revised.summary}</p>
                  </div>
                </div>
                {current === null ? null : (
                  <RevisionComparison original={revised.original} revised={current} detail={detail} />
                )}
              </div>
            )}
            <Explanation recommendation={recommendation} detail={detail} />
            <h4>{t("rationale")}</h4>
            <p>{recommendation.rationale}</p>
            {/* The server's text counts SIMULATED statuses as "valid": say so on the spot. */}
            {simulatedProofs || mixedProofs ? <SimulatedRationaleNote mixed={mixedProofs} /> : null}
            <p className="hc-supporting">
              {t("protocol")}
              {": "}
              {recommendation.protocol}
            </p>
            {recommendation.unresolved_assumptions.length === 0 ? null : (
              <>
                <h4>{t("assumptions")}</h4>
                <ul className="hc-list">
                  {recommendation.unresolved_assumptions.map((item) => (
                    <li key={item}>{item}</li>
                  ))}
                </ul>
              </>
            )}
            {(recommendation.unresolved_uncertainties ?? []).length === 0 ? null : (
              <>
                <h4>{t("uncertainties")}</h4>
                <ul className="hc-list">
                  {(recommendation.unresolved_uncertainties ?? []).map((item) => (
                    <li key={item}>{item}</li>
                  ))}
                </ul>
              </>
            )}
          </div>
        )}
      </section>

      <section className="hc-section" aria-labelledby="decision-objections">
        <h3 id="decision-objections">{t("objections")}</h3>
        {openItems.length === 0 ? (
          <p className="hc-supporting" data-open-objections="0">
            {t("noObjections")}
          </p>
        ) : (
          <ul className="hc-list" data-open-objections={openItems.length}>
            {openItems.map((item) => (
              <li key={item.id}>
                <Tag variant="warning" icon="message-square-warning" label={tRun(`stage.kinds.${item.kind}`)} />{" "}
                {item.description}
              </li>
            ))}
          </ul>
        )}
      </section>

      {cardFirst ? null : card}

      {summary.active ? (
        <section className="hc-section" aria-labelledby="decision-cancel">
          <h3 id="decision-cancel">{t("cancel")}</h3>
          <p className="hc-supporting">{t("cancelHint")}</p>
          {cancelling === "requested" ? (
            <p role="status">{t("cancelling")}</p>
          ) : (
            <div className="hc-actions">
              <Button variant="destructive" label={t("cancel")} onClick={() => void cancel()} />
            </div>
          )}
          {/* Headed by the code's class (a refused cancel is not a refused decision). */}
          {cancelError === null ? null : (
            <div data-cancel-error>
              <ApiErrorNotice error={cancelError} context="read" />
            </div>
          )}
        </section>
      ) : null}
    </div>
  );
}
