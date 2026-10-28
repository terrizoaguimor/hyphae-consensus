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
// ndjson_unavailable). A run still deliberating can be cancelled. A failed
// detail request is said as such, never shown as loading.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useFormatter, useTranslations } from "next-intl";
import { useState } from "react";

import { api, ApiError, isApiError, newIdempotencyKey } from "@/lib/api/client";
import type { DecisionResponseV1, DecisionV1 } from "@/lib/api/types";
import { ndjsonPublishable } from "@/lib/provenance";
import { supersededProposal } from "@/lib/run/view";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { RecommendationLine } from "./StageTab";
import type { RunData } from "./useRunData";

type DecisionTabProps = {
  runId: string;
  data: RunData;
};

const OVERRIDE_KINDS = new Set(["blocked", "abstained"]);

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

export function DecisionTab({ runId, data }: DecisionTabProps) {
  const t = useTranslations("Run.decision");
  const tRun = useTranslations("Run");
  const format = useFormatter();
  const { detail, detailError, summary, setDetail } = data;
  const [approvedChoice, setApprovedChoice] = useState<boolean | null>(null);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<DecisionResponseV1 | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [cancelling, setCancelling] = useState<"idle" | "requested">("idle");

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
  const original = supersededProposal(detail);
  const current = detail.current_proposal ?? null;
  // The recommendation is for a revision: say which proposal it is for and
  // which one it replaced, from the records (K03/K04).
  const revised =
    original !== null && current !== null
      ? { id: current.id, summary: current.summary, original }
      : null;
  const publishesLog = ndjsonPublishable(detail.summary.provenance);
  const recorded: DecisionV1 | null = result?.decision ?? detail.decision ?? null;
  const awaiting =
    detail.summary.state === "awaiting_human_approval" &&
    detail.summary.provenance.source === "this_server" &&
    recorded === null;
  const kind = recommendation?.kind ?? detail.summary.recommendation ?? null;
  const overrideNeeded = approvedChoice === true && kind !== null && OVERRIDE_KINDS.has(kind) && note.trim() === "";

  const submit = async (approved: boolean) => {
    if (kind === null) {
      return;
    }
    setApprovedChoice(approved);
    if (approved && OVERRIDE_KINDS.has(kind) && note.trim() === "") {
      return;
    }
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
      const refreshed = await api.getRun(runId).catch(() => null);
      if (refreshed !== null) {
        setDetail(refreshed);
      }
    } catch (cause) {
      const apiError = toApiError(cause, `/runs/${runId}/decision`);
      setError(apiError);
      if (apiError.code === "decision_conflict") {
        const refreshed = await api.getRun(runId).catch(() => null);
        if (refreshed !== null) {
          setDetail(refreshed);
        }
      }
    } finally {
      setSubmitting(false);
    }
  };

  const cancel = async () => {
    setError(null);
    try {
      await api.cancel(runId);
      setCancelling("requested");
    } catch (cause) {
      setError(toApiError(cause, `/runs/${runId}/cancel`));
    }
  };

  const dateTime = (iso: string) =>
    format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "short" });

  return (
    <div className="hc-decision" data-decision>
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
              </div>
            )}
            <h4>{t("rationale")}</h4>
            <p>{recommendation.rationale}</p>
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
                <Badge variant="warning" label={tRun(`stage.kinds.${item.kind}`)} />{" "}
                {item.description}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="hc-section" aria-labelledby="decision-form">
        <h3 id="decision-form">{t("formHeading")}</h3>
        <p className="hc-build-note" data-public-demo-notice>
          {t("publicDemo")}
        </p>
        {recorded !== null ? (
          <div className="hc-panel" data-decision-recorded={recorded.approved ? "approved" : "rejected"}>
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
            onSubmit={(event) => event.preventDefault()}
          >
            <TextArea
              label={t("note")}
              description={t("noteDescription")}
              value={note}
              onChange={(value) => setNote(value)}
              isOptional
            />
            {overrideNeeded && kind !== null ? (
              <p className="hc-supporting" role="alert">
                {t("overrideRequired", { kind: tRun(`kinds.${kind}`) })}
              </p>
            ) : null}
            <div className="hc-actions">
              <Button
                variant="primary"
                label={submitting && approvedChoice === true ? t("submitting") : t("approve")}
                isDisabled={submitting || kind === null}
                isLoading={submitting && approvedChoice === true}
                onClick={() => void submit(true)}
              />
              <Button
                variant="destructive"
                label={submitting && approvedChoice === false ? t("submitting") : t("reject")}
                isDisabled={submitting || kind === null}
                isLoading={submitting && approvedChoice === false}
                onClick={() => void submit(false)}
              />
            </div>
          </form>
        ) : (
          <p className="hc-supporting" data-decision-unavailable>
            {t("notAwaiting")}
          </p>
        )}
        {error === null ? null : <ApiErrorNotice error={error} />}
        <ul className="hc-related" role="list">
          <li>
            <a href={api.receiptUrl(runId)} className="hc-link" data-receipt-link>
              {t("receipt")}
            </a>
          </li>
          <li>
            {publishesLog ? (
              <a href={api.ndjsonUrl(runId)} className="hc-link" data-ndjson-link>
                {t("ndjson")}
              </a>
            ) : (
              <span className="hc-supporting" data-ndjson-unavailable>
                {t("ndjsonUnavailable")}
              </span>
            )}
          </li>
        </ul>
      </section>

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
        </section>
      ) : null}
    </div>
  );
}
