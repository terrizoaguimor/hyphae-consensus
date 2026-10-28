"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Decision tab: the recommendation kind with its evidence axis, the
// rationale, the unresolved assumptions and uncertainties, the open
// objections, the superseded-proposal notice when the recommendation is for a
// revision (both summaries side by side), and the public decision form
// (approve or reject, optional note) with the fixed public_demo notice,
// enabled only while the run awaits a human decision and belongs to this
// session (GET /runs/{id} answers 404 for anyone else). The recorded decision
// carries the server's notice; the receipt and the verbatim log are linked.
// A run still deliberating can be cancelled.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useFormatter, useTranslations } from "next-intl";
import { useState } from "react";

import { api, ApiError, isApiError, newIdempotencyKey } from "@/lib/api/client";
import type { DecisionResponseV1, DecisionV1, Proposal, RunDetailV1 } from "@/lib/api/types";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { RecommendationLine } from "./StageTab";
import type { RunData } from "./useRunData";

type DecisionTabProps = {
  runId: string;
  data: RunData;
};

const OVERRIDE_KINDS = new Set(["blocked", "abstained"]);

/** The original proposal when the current one supersedes it (K03/K04: a revision that departs from the task). */
export function supersededProposal(detail: RunDetailV1): Proposal | null {
  const current = detail.current_proposal;
  if (current === undefined || current === null) {
    return null;
  }
  const proposals = new Map<string, Proposal>();
  let supersedes: string | null = null;
  for (const event of detail.events) {
    const payload = event.payload;
    if (payload.type === "proposal_submitted") {
      proposals.set(payload.proposal.id, payload.proposal);
    } else if (payload.type === "revision_submitted") {
      proposals.set(payload.revision.proposal.id, payload.revision.proposal);
      if (payload.revision.proposal.id === current.id) {
        supersedes = payload.revision.supersedes;
      }
    }
  }
  if (supersedes === null) {
    return null;
  }
  // Follow the chain of revisions back to the first proposal.
  let original = proposals.get(supersedes) ?? null;
  const seen = new Set<string>();
  while (original !== null && !seen.has(original.id)) {
    seen.add(original.id);
    const earlier = detail.events.find(
      (event) =>
        event.payload.type === "revision_submitted" &&
        event.payload.revision.proposal.id === original?.id,
    );
    if (earlier === undefined || earlier.payload.type !== "revision_submitted") {
      break;
    }
    original = proposals.get(earlier.payload.revision.supersedes) ?? original;
  }
  return original;
}

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
  const { detail, summary, setDetail } = data;
  const [approvedChoice, setApprovedChoice] = useState<boolean | null>(null);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<DecisionResponseV1 | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [cancelling, setCancelling] = useState<"idle" | "requested">("idle");

  if (detail === null || summary === null) {
    return (
      <p role="status" className="hc-supporting">
        {tRun("loading")}
      </p>
    );
  }

  const recommendation = detail.recommendation ?? null;
  const openItems = detail.challenge_items.filter((item) => item.status === "open");
  const original = supersededProposal(detail);
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
          <p className="hc-supporting">{t("pending")}</p>
        ) : (
          <div className="hc-panel">
            <RecommendationLine kind={recommendation.kind} evidence={detail.summary.provenance.evidence} />
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

      {original !== null && detail.current_proposal !== undefined && detail.current_proposal !== null ? (
        <section className="hc-section" aria-labelledby="decision-superseded" data-superseded>
          <h3 id="decision-superseded">{t("superseded")}</h3>
          <div className="hc-side-by-side">
            <div className="hc-panel">
              <h4>{t("original", { id: original.id })}</h4>
              <p>{original.summary}</p>
            </div>
            <div className="hc-panel">
              <h4>{t("revised", { id: detail.current_proposal.id })}</h4>
              <p>{detail.current_proposal.summary}</p>
            </div>
          </div>
        </section>
      ) : null}

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
            <a href={api.ndjsonUrl(runId)} className="hc-link" data-ndjson-link>
              {t("ndjson")}
            </a>
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
