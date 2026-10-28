"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Live / Playback tab: the stream status, the readability pacing (label
// and skip control; off under reduced motion), the three-agent stage where
// each card shows the latest document of its role from the frames (proposal
// claims, challenge items by kind, evaluation assessments and verdicts) with
// the delivery badge of the frame that carried it, the runtime lane (evidence
// retrieved, proofs checked, recalls proven) and the recommendation. Every
// number is counted from the frames.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { useFormatter, useTranslations } from "next-intl";
import type { ReactNode } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type {
  Challenge,
  ConsensusFrameV1,
  Evaluation,
  ModelInvocationV1,
  Proposal,
  Recommendation,
  VerificationV1,
} from "@/lib/api/types";
import {
  deliveryLabel,
  evidenceLabel,
  proofToken,
  RECOMMENDATION_VARIANTS,
} from "@/lib/provenance";

import type { RunData } from "./useRunData";

type Carried<T> = { document: T; frame: ConsensusFrameV1 };

type Stage = {
  proposal: Carried<{ proposal: Proposal; supersedes: string | null; invocation: ModelInvocationV1 }> | null;
  challenge: Carried<{ challenge: Challenge; invocation: ModelInvocationV1 }> | null;
  evaluation: Carried<{ evaluation: Evaluation; invocation: ModelInvocationV1 }> | null;
  recommendation: Carried<Recommendation> | null;
  retrieved: number;
  unavailable: number;
  proven: number;
  verifications: Carried<VerificationV1>[];
};

/** The latest document of each role, from the frames shown so far. */
export function stageOf(frames: ConsensusFrameV1[]): Stage {
  const stage: Stage = {
    proposal: null,
    challenge: null,
    evaluation: null,
    recommendation: null,
    retrieved: 0,
    unavailable: 0,
    proven: 0,
    verifications: [],
  };
  for (const frame of frames) {
    const payload = frame.event.payload;
    switch (payload.type) {
      case "proposal_submitted":
        stage.proposal = {
          frame,
          document: { proposal: payload.proposal, supersedes: null, invocation: payload.invocation },
        };
        break;
      case "revision_submitted":
        stage.proposal = {
          frame,
          document: {
            proposal: payload.revision.proposal,
            supersedes: payload.revision.supersedes,
            invocation: payload.invocation,
          },
        };
        break;
      case "challenge_submitted":
        stage.challenge = {
          frame,
          document: { challenge: payload.challenge, invocation: payload.invocation },
        };
        break;
      case "evaluation_submitted":
        stage.evaluation = {
          frame,
          document: { evaluation: payload.evaluation, invocation: payload.invocation },
        };
        break;
      case "recommendation_issued":
        stage.recommendation = { frame, document: payload.recommendation };
        break;
      case "evidence_retrieved":
        stage.retrieved += 1;
        break;
      case "evidence_unavailable":
        stage.unavailable += 1;
        break;
      case "recall_proven":
        stage.proven += 1;
        break;
      case "evidence_verified":
        stage.verifications.push({ frame, document: payload.verification });
        break;
      default:
        break;
    }
  }
  return stage;
}

type StageTabProps = {
  data: RunData;
};

export function StageTab({ data }: StageTabProps) {
  const t = useTranslations("Run");
  const { frames, stream, end, notices, pending, pacing, skipPacing, summary } = data;
  const stage = stageOf(frames);
  const streamText =
    stream === "ended" && end !== null
      ? t("stream.ended", { reason: t(`stream.endReasons.${end.reason}`) })
      : t(`stream.${stream}`);

  return (
    <div className="hc-stage-tab">
      <div className="hc-status-line" data-stream-state={stream}>
        <p role="status">{streamText}</p>
        {notices.gaps > 0 ? <p className="hc-supporting">{t("stream.gap")}</p> : null}
        {notices.resyncs > 0 ? <p className="hc-supporting">{t("stream.resync")}</p> : null}
        {notices.invalid > 0 ? (
          <p className="hc-supporting">{t("stream.invalid", { count: notices.invalid })}</p>
        ) : null}
      </div>

      <div className="hc-pacing" data-pacing={pacing}>
        {pacing === "reduced" ? (
          <p className="hc-supporting">{t("pacing.off")}</p>
        ) : (
          <>
            <Badge variant="purple" label={t("pacing.label")} />
            <p className="hc-supporting hc-tabular-nums" role="status">
              {t("pacing.pending", { count: pending })}
            </p>
            {pacing === "on" ? (
              <Button variant="secondary" size="sm" label={t("pacing.skip")} onClick={skipPacing} />
            ) : null}
          </>
        )}
      </div>

      <section className="hc-section" aria-labelledby="stage-heading">
        <h3 id="stage-heading">{t("stage.heading")}</h3>
        <div className="hc-stage">
          <RoleCard
            title={t("stage.proposer")}
            carried={stage.proposal}
            invocation={stage.proposal?.document.invocation ?? null}
            summary={summary}
          >
            {stage.proposal === null ? null : (
              <ProposalBody
                proposal={stage.proposal.document.proposal}
                supersedes={stage.proposal.document.supersedes}
              />
            )}
          </RoleCard>
          <RoleCard
            title={t("stage.critic")}
            carried={stage.challenge}
            invocation={stage.challenge?.document.invocation ?? null}
            summary={summary}
          >
            {stage.challenge === null ? null : (
              <ChallengeBody challenge={stage.challenge.document.challenge} />
            )}
          </RoleCard>
          <RoleCard
            title={t("stage.evaluator")}
            carried={stage.evaluation}
            invocation={stage.evaluation?.document.invocation ?? null}
            summary={summary}
          >
            {stage.evaluation === null ? null : (
              <EvaluationBody evaluation={stage.evaluation.document.evaluation} />
            )}
          </RoleCard>
        </div>
      </section>

      <section className="hc-section hc-lane" aria-labelledby="lane-heading" data-lane>
        <h3 id="lane-heading">{t("stage.lane")}</h3>
        <ul className="hc-badges" role="list">
          <li>
            <Badge variant="neutral" label={t("stage.retrieved", { count: stage.retrieved })} />
          </li>
          <li>
            <Badge
              variant="neutral"
              label={t("stage.verified", { count: stage.verifications.length })}
            />
          </li>
          <li>
            <Badge variant="neutral" label={t("stage.proven", { count: stage.proven })} />
          </li>
          {stage.unavailable > 0 ? (
            <li>
              <Badge variant="warning" label={t("stage.unavailable", { count: stage.unavailable })} />
            </li>
          ) : null}
        </ul>
        {stage.verifications.length === 0 ? null : (
          <ul className="hc-list" role="list">
            {stage.verifications.map(({ document, frame }) => (
              <li key={`${frame.event.sequence}`}>
                <ProofStatus verification={document} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="hc-section" aria-labelledby="recommendation-heading" data-recommendation>
        <h3 id="recommendation-heading">{t("stage.recommendation")}</h3>
        {stage.recommendation === null || summary === null ? (
          <p className="hc-supporting">{t("stage.noRecommendation")}</p>
        ) : (
          <div className="hc-panel">
            <RecommendationLine
              kind={stage.recommendation.document.kind}
              evidence={summary.provenance.evidence}
            />
            <FrameBadge frame={stage.recommendation.frame} summary={summary} />
            <p>{stage.recommendation.document.rationale}</p>
          </div>
        )}
      </section>
    </div>
  );
}

export function RecommendationLine({
  kind,
  evidence,
}: {
  kind: Recommendation["kind"];
  evidence: ConsensusFrameV1["summary"]["provenance"]["evidence"];
}) {
  const t = useTranslations("Run");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  return (
    <p data-recommendation-kind={kind}>
      <Badge
        variant={RECOMMENDATION_VARIANTS[kind]}
        label={t("outcome", {
          kind: t(`kinds.${kind}`),
          evidence: tProvenance(evidenceLabel(evidence).key),
        })}
      />
    </p>
  );
}

/** The delivery badge of one frame, per the honesty rules (LIVE needs a live model). */
function FrameBadge({ frame, summary }: { frame: ConsensusFrameV1; summary: RunData["summary"] }) {
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const provenance = summary?.provenance ?? frame.summary.provenance;
  const label = deliveryLabel(frame.delivery, provenance);
  return (
    <p className="hc-frame-badge hc-supporting" data-frame-delivery={frame.delivery} data-frame-label={label.key}>
      <Badge variant={label.variant} label={tProvenance(label.key)} />
      <span className="hc-tabular-nums">
        {format.dateTime(new Date(frame.event.occurred_at), {
          dateStyle: "medium",
          timeStyle: "medium",
        })}
      </span>
    </p>
  );
}

function RoleCard({
  title,
  carried,
  invocation,
  summary,
  children,
}: {
  title: string;
  carried: Carried<unknown> | null;
  invocation: ModelInvocationV1 | null;
  summary: RunData["summary"];
  children: ReactNode;
}) {
  const t = useTranslations("Run");
  const tProvenance = useTranslations("Provenance");
  return (
    <article className="hc-stage-card" data-role-card={title}>
      <header className="hc-stage-card-head">
        <h4>{title}</h4>
        {carried === null ? null : <FrameBadge frame={carried.frame} summary={summary} />}
      </header>
      {invocation === null ? null : (
        <p className="hc-supporting" data-model-note={invocation.scripted ? "scripted" : "model"}>
          {invocation.scripted
            ? tProvenance("scripted.note")
            : t("stage.model", { model: invocation.model, provider: invocation.provider })}
        </p>
      )}
      {carried === null ? <p className="hc-supporting">{t("stage.waiting")}</p> : children}
    </article>
  );
}

function ProposalBody({ proposal, supersedes }: { proposal: Proposal; supersedes: string | null }) {
  const t = useTranslations("Run.stage");
  return (
    <div className="hc-stage-body">
      <p className="hc-supporting">
        {supersedes === null
          ? t("proposal", { id: proposal.id })
          : t("revision", { id: proposal.id, supersedes })}
      </p>
      <p>{proposal.summary}</p>
      <h5>{t("claims")}</h5>
      <ul className="hc-list">
        {proposal.claims.map((claim) => (
          <li key={claim.id}>
            <code className="hc-code">{claim.id}</code> {claim.statement}
          </li>
        ))}
      </ul>
      {proposal.assumptions.length === 0 ? null : (
        <>
          <h5>{t("assumptions")}</h5>
          <ul className="hc-list">
            {proposal.assumptions.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </>
      )}
      {proposal.uncertainties.length === 0 ? null : (
        <>
          <h5>{t("uncertainties")}</h5>
          <ul className="hc-list">
            {proposal.uncertainties.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </>
      )}
      {proposal.plan_steps.length === 0 ? null : (
        <>
          <h5>{t("plan")}</h5>
          <ol className="hc-list">
            {proposal.plan_steps.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ol>
        </>
      )}
    </div>
  );
}

function ChallengeBody({ challenge }: { challenge: Challenge }) {
  const t = useTranslations("Run.stage");
  return (
    <div className="hc-stage-body">
      <p className="hc-supporting">
        {t("challenge", { id: challenge.id, target: challenge.target_proposal })}
      </p>
      {challenge.items.length === 0 ? (
        <p>{t("noObjections")}</p>
      ) : (
        <ul className="hc-list">
          {challenge.items.map((item, index) => (
            <li key={item.id ?? `${index}`}>
              <Badge
                variant="warning"
                label={
                  item.claim_id === undefined || item.claim_id === null
                    ? t("objectionNoClaim", { kind: t(`kinds.${item.kind}`) })
                    : t("objection", { kind: t(`kinds.${item.kind}`), claim: item.claim_id })
                }
              />
              <span>{item.description}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function EvaluationBody({ evaluation }: { evaluation: Evaluation }) {
  const t = useTranslations("Run.stage");
  return (
    <div className="hc-stage-body">
      <p className="hc-supporting">{t("evaluation", { id: evaluation.target_proposal })}</p>
      {evaluation.abstain ? (
        <p>
          {evaluation.abstain_reason === undefined || evaluation.abstain_reason === null
            ? t("abstainedNoReason")
            : t("abstained", { reason: evaluation.abstain_reason })}
        </p>
      ) : null}
      <ul className="hc-list">
        {evaluation.assessments.map((assessment) => (
          <li key={assessment.claim_id}>
            <code className="hc-code">{assessment.claim_id}</code>{" "}
            <Badge
              variant={assessment.corroborated ? "success" : "neutral"}
              label={assessment.corroborated ? t("corroborated") : t("notCorroborated")}
            />
            {(assessment.contradicting_evidence ?? []).length === 0 ? null : (
              <span className="hc-supporting">
                {t("contradictedBy", { ids: (assessment.contradicting_evidence ?? []).join(", ") })}
              </span>
            )}
          </li>
        ))}
      </ul>
      {(evaluation.item_verdicts ?? []).length === 0 ? null : (
        <>
          <h5>{t("verdicts")}</h5>
          <ul className="hc-list">
            {(evaluation.item_verdicts ?? []).map((verdict) => (
              <li key={verdict.item_id}>
                {t("verdict", {
                  item: verdict.item_id,
                  status: t(verdict.status),
                  reason: verdict.reason,
                })}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

/** A proof's status: a simulated one is the SIMULATED token with its scripted outcome as text, never a green valid. */
export function ProofStatus({ verification }: { verification: VerificationV1 }) {
  const t = useTranslations("Run.stage");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const token = proofToken(verification.integrity_status, verification.simulated);
  const status = tProvenance(token.key);
  return (
    <span className="hc-proof" data-proof-simulated={verification.simulated} data-proof-status={verification.integrity_status}>
      <code className="hc-code">{verification.evidence_id}</code>{" "}
      <Badge
        variant={token.variant}
        label={
          token.simulated
            ? tProvenance("simulated.label")
            : t("proofStatus", { status })
        }
      />
      {token.simulated ? <span> {t("simulatedProof", { status })}</span> : null}
    </span>
  );
}
