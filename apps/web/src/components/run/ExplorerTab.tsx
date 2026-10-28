"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Explorer tab: the event timeline (sequence, type, actor, time, a
// one-line summary, and the public event as expandable JSON), filterable by
// type. The rows are the union of the detail's events and the frames'
// events, by sequence, so the list follows the stream. Every status in a
// summary is translated from its enumeration: a simulated proof reads
// SIMULATED with its scripted outcome (never "integrity valid"), the
// factual and relevance statuses and a decision's authority have their own
// words, and a failure or an unavailable retrieval reads the sentence of its
// code, never the server's English message.

import { useFormatter, useTranslations } from "next-intl";
import { useId, useMemo, useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { ApiError } from "@/lib/api/client";
import type { ConsensusFrameV1, EventV1, RunDetailV1, VerificationV1 } from "@/lib/api/types";
import { proofToken } from "@/lib/provenance";
import { mergeEvents } from "@/lib/run/view";

import { useFailureText, useUnavailableText } from "./FailureSentence";

type ExplorerTabProps = {
  detail: RunDetailV1 | null;
  frames: ConsensusFrameV1[];
  /** The detail request failed: the timeline holds only what the stream delivered. */
  detailError?: ApiError | null;
};

const EVENT_TYPES = [
  "task_created",
  "proposal_submitted",
  "evidence_retrieved",
  "evidence_unavailable",
  "recall_proven",
  "evidence_verified",
  "challenge_submitted",
  "evaluation_submitted",
  "revision_submitted",
  "recommendation_issued",
  "human_decision_recorded",
  "run_failed",
] as const;

type EventType = (typeof EVENT_TYPES)[number];

/** Authorities with their own word (`Run.explorer.authority.<value>`); DecisionV1.authority is free text. */
const AUTHORITIES = ["operator", "public_demo", "unspecified"] as const;

export function ExplorerTab({ detail, frames, detailError = null }: ExplorerTabProps) {
  const t = useTranslations("Run.explorer");
  const tRun = useTranslations("Run");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const failureText = useFailureText();
  const unavailableText = useUnavailableText();
  const [filter, setFilter] = useState<EventType | "all">("all");
  const filterId = useId();
  const events = useMemo(() => mergeEvents(detail, frames), [detail, frames]);
  const shown = filter === "all" ? events : events.filter((e) => e.payload.type === filter);

  const actorText = (event: EventV1): string => {
    switch (event.actor.kind) {
      case "agent":
        return t(`roles.${event.actor.role}`);
      case "human":
        return t("actors.human", { id: event.actor.id });
      case "runtime":
        return t("actors.runtime");
    }
  };

  /** A proof's status in words: SIMULATED with the scripted outcome, or "proof <status>". */
  const proofText = (verification: VerificationV1): string => {
    const token = proofToken(verification.integrity_status, verification.simulated);
    const status = tProvenance(token.key);
    return token.simulated
      ? tRun("stage.simulatedProofShort", { status })
      : tRun("stage.proofStatus", { status });
  };

  const authorityText = (authority: string): string =>
    (AUTHORITIES as readonly string[]).includes(authority)
      ? t(`authority.${authority as (typeof AUTHORITIES)[number]}`)
      : t("authority.other", { authority });

  const summaryText = (event: EventV1): string => {
    const payload = event.payload;
    switch (payload.type) {
      case "task_created":
        return t("summaries.task_created", { title: payload.task.title });
      case "proposal_submitted":
        return t("summaries.proposal_submitted", {
          id: payload.proposal.id,
          summary: payload.proposal.summary,
        });
      case "evidence_retrieved":
        return t("summaries.evidence_retrieved", {
          id: payload.evidence.id,
          kind: payload.evidence.kind,
        });
      case "evidence_unavailable":
        return t("summaries.evidence_unavailable", { message: unavailableText(payload.code) });
      case "recall_proven":
        return t("summaries.recall_proven", {
          count: payload.evidence_ids.length,
          anchor: payload.anchor,
        });
      case "evidence_verified":
        return t("summaries.evidence_verified", {
          id: payload.verification.evidence_id,
          proof: proofText(payload.verification),
          factual: t(`factual.${payload.verification.factual_status}`),
          relevance: t(`relevance.${payload.verification.relevance_status}`),
        });
      case "challenge_submitted":
        return t("summaries.challenge_submitted", {
          count: payload.challenge.items.length,
          target: payload.challenge.target_proposal,
        });
      case "evaluation_submitted":
        return t("summaries.evaluation_submitted", {
          corroborated: payload.evaluation.assessments.filter((a) => a.corroborated).length,
          claims: payload.evaluation.assessments.length,
          unresolved: (payload.evaluation.unresolved_items ?? []).length,
        });
      case "revision_submitted":
        return t("summaries.revision_submitted", {
          id: payload.revision.proposal.id,
          supersedes: payload.revision.supersedes,
          summary: payload.revision.proposal.summary,
        });
      case "recommendation_issued":
        return t("summaries.recommendation_issued", {
          kind: tRun(`kinds.${payload.recommendation.kind}`),
        });
      case "human_decision_recorded":
        return t("summaries.human_decision_recorded", {
          decision: payload.decision.approved
            ? tRun("decision.approved")
            : tRun("decision.rejected"),
          by: payload.decision.authorized_by,
          authority: authorityText(payload.decision.authority),
        });
      case "run_failed":
        return t("summaries.run_failed", {
          stage: payload.stage,
          message: failureText({ code: payload.code, stage: payload.stage }),
        });
    }
  };

  return (
    <div className="hc-explorer" data-explorer>
      <div className="hc-list-head">
        <h3 id="explorer-heading">{t("heading")}</h3>
        <p className="hc-supporting hc-tabular-nums">
          {t("count", { shown: shown.length, total: events.length })}
        </p>
      </div>
      {detailError === null ? null : (
        <p className="hc-supporting" data-explorer-partial>
          {t("partial")}
        </p>
      )}
      <div className="hc-field hc-field--inline">
        <label htmlFor={filterId}>{t("filter")}</label>
        <select
          id={filterId}
          className="hc-select"
          value={filter}
          onChange={(event) => setFilter(event.target.value as EventType | "all")}
        >
          <option value="all">{t("all")}</option>
          {EVENT_TYPES.map((type) => (
            <option key={type} value={type}>
              {t(`types.${type}`)}
            </option>
          ))}
        </select>
      </div>
      {shown.length === 0 ? (
        <p className="hc-supporting">{t("empty")}</p>
      ) : (
        <table className="hc-timeline">
          <caption className="hc-visually-hidden">{t("heading")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("sequence")}</th>
              <th scope="col">{t("type")}</th>
              <th scope="col">{t("actor")}</th>
              <th scope="col">{t("time")}</th>
              <th scope="col">{t("summary")}</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((event) => (
              <tr key={event.sequence} data-sequence={event.sequence} data-event-type={event.payload.type}>
                <td className="hc-tabular-nums">{event.sequence}</td>
                <td>{t(`types.${event.payload.type}`)}</td>
                <td>{actorText(event)}</td>
                <td className="hc-tabular-nums">
                  <time dateTime={event.occurred_at}>
                    {format.dateTime(new Date(event.occurred_at), {
                      dateStyle: "short",
                      timeStyle: "medium",
                    })}
                  </time>
                </td>
                <td>
                  <span>{summaryText(event)}</span>
                  <details className="hc-json-details">
                    <summary>{t("json")}</summary>
                    <pre className="hc-json hc-code">{JSON.stringify(event, null, 2)}</pre>
                  </details>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
