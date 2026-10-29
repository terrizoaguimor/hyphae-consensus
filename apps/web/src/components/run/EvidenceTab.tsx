"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Evidence tab: every record the run retrieved (the detail's events and
// the frames', src/lib/run/evidence.ts), its kind, project, retrieval mode,
// snapshot id and capture time when the record has them, what its proof
// covers (from the proof's own request, never from the claim: "proof covers:
// complete record list of <project> at anchor <prefix>" for the sealed
// snapshot; for a live recall, the request it sealed and, when that
// request's query is a claim's own text, that the proof covers what the
// recall of that text returned, not the claim), its text as untrusted English content, and its verifications by
// round with the three statuses on their own axes (integrity through
// proofToken: a simulated proof is SIMULATED, never green; factual and
// relevance translated, never a positive tone for "irrelevant"). Failed
// retrievals are listed with the sentence of their code.

import { Badge } from "@astryxdesign/core/Badge";
import { useFormatter, useTranslations } from "next-intl";
import { useMemo } from "react";

import type { ApiError } from "@/lib/api/client";
import type {
  ConsensusFrameV1,
  FactualStatus,
  RunDetailV1,
  RunSummaryV1,
  VerificationV1,
} from "@/lib/api/types";
import {
  byRound,
  type Coverage,
  digestPrefix,
  evidenceView,
  type RetrievedRecord,
  type RoundedVerification,
} from "@/lib/run/evidence";

import { useUnavailableText } from "./FailureSentence";
import { ProofReadingLine } from "./ProofReading";
import { ProofStatus } from "./StageTab";
import { UntrustedText } from "./UntrustedText";

type EvidenceTabProps = {
  detail: RunDetailV1 | null;
  frames: ConsensusFrameV1[];
  detailError: ApiError | null;
  summary: RunSummaryV1 | null;
};

const FACTUAL_VARIANTS: Record<FactualStatus, "neutral" | "warning"> = {
  corroborated: "neutral",
  uncorroborated: "neutral",
  contradicted: "warning",
  not_assessed: "neutral",
};

export function EvidenceTab({ detail, frames, detailError, summary }: EvidenceTabProps) {
  const t = useTranslations("Run.evidence");
  const unavailableText = useUnavailableText();
  const view = useMemo(() => evidenceView(detail, frames), [detail, frames]);
  const recorded = summary?.provenance.source === "research_archive";

  return (
    <div className="hc-evidence-tab" data-evidence-tab>
      <div className="hc-list-head">
        <h3 id="evidence-heading">{t("heading")}</h3>
        <p className="hc-supporting hc-tabular-nums" data-evidence-count={view.records.length}>
          {t("count", { count: view.records.length })}
        </p>
      </div>
      <p className="hc-supporting">{t("intro")}</p>
      {detailError === null ? null : (
        <p className="hc-supporting" data-evidence-partial>
          {t("partial")}
        </p>
      )}
      {view.records.length === 0 ? (
        <p className="hc-supporting" data-evidence-none>
          {t("none")}
        </p>
      ) : (
        <ol className="hc-evidence-list" role="list" aria-labelledby="evidence-heading">
          {view.records.map((record) => (
            <EvidenceCard
              key={`${record.evidence.id}-${record.sequences[0] ?? 0}`}
              record={record}
              recorded={recorded}
            />
          ))}
        </ol>
      )}
      {view.orphanVerifications.length === 0 ? null : (
        <section className="hc-section" aria-labelledby="evidence-orphans" data-evidence-orphans>
          <h4 id="evidence-orphans">{t("orphansHeading")}</h4>
          <p className="hc-supporting">{t("orphansIntro")}</p>
          <VerificationRounds verifications={view.orphanVerifications} record={undefined} recorded={recorded} />
        </section>
      )}
      {view.failed.length === 0 ? null : (
        <section className="hc-section" aria-labelledby="evidence-failed" data-evidence-failed={view.failed.length}>
          <h4 id="evidence-failed">{t("failedHeading", { count: view.failed.length })}</h4>
          <ul className="hc-list" role="list">
            {view.failed.map((failure) => (
              <li key={failure.sequence} data-failed-code={failure.code ?? ""}>
                <span>{t("failedRecord", { sequence: failure.sequence, message: unavailableText(failure.code) })}</span>
                <UntrustedText text={failure.query} source="model" />
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function CoverageLine({ coverage }: { coverage: Coverage }) {
  const t = useTranslations("Run.evidence.coverage");
  switch (coverage.kind) {
    case "simulated":
      return <p data-coverage="simulated">{t("simulated")}</p>;
    case "proof_error":
      return <p data-coverage="proof_error">{t("proofError")}</p>;
    case "snapshot_list": {
      const anchor = digestPrefix(coverage.anchor);
      return (
        <p data-coverage="snapshot_list">
          {anchor === null
            ? t("snapshotNoAnchor", { project: coverage.project })
            : t("snapshot", { project: coverage.project, anchor })}
        </p>
      );
    }
    case "request": {
      const anchor = digestPrefix(coverage.anchor);
      const { request } = coverage;
      return (
        <div data-coverage="request">
          <p>{anchor === null ? t("requestNoAnchor") : t("request", { anchor })}</p>
          {coverage.queryIsClaim ? <p data-coverage-query-is-claim>{t("queryIsClaim")}</p> : null}
          <dl className="hc-kv hc-kv--compact">
            <div>
              <dt>{t("operation")}</dt>
              <dd>
                <code className="hc-code">{request.operation}</code>
              </dd>
            </div>
            <div>
              <dt>{t("project")}</dt>
              <dd>
                <code className="hc-code">{request.project}</code>
              </dd>
            </div>
            {request.query === undefined || request.query === null ? null : (
              <div>
                <dt>{t("query")}</dt>
                <dd lang="en">{request.query}</dd>
              </div>
            )}
            {request.limit === undefined || request.limit === null ? null : (
              <div>
                <dt>{t("limit")}</dt>
                <dd className="hc-tabular-nums">{request.limit}</dd>
              </div>
            )}
          </dl>
        </div>
      );
    }
    case "unrecorded": {
      const anchor = digestPrefix(coverage.anchor);
      return (
        <p data-coverage="unrecorded">
          {anchor === null ? t("unrecordedNoAnchor") : t("unrecorded", { anchor })}
        </p>
      );
    }
  }
}

function EvidenceCard({ record, recorded }: { record: RetrievedRecord; recorded: boolean }) {
  const t = useTranslations("Run.evidence");
  const format = useFormatter();
  const { evidence } = record;
  const mode = evidence.retrieval ?? "unrecorded";
  const dateTime = (iso: string) =>
    format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "medium" });
  return (
    <li
      className="hc-evidence"
      data-evidence-id={evidence.id}
      data-retrieval={mode}
      aria-labelledby={`evidence-${record.sequences[0] ?? 0}`}
    >
      <h4 id={`evidence-${record.sequences[0] ?? 0}`} className="hc-evidence-title">
        <code className="hc-code">{evidence.id}</code>
      </h4>
      <dl className="hc-kv">
        <div>
          <dt>{t("kind")}</dt>
          <dd>
            <code className="hc-code" lang="en">
              {evidence.kind}
            </code>
          </dd>
        </div>
        <div>
          <dt>{t("project")}</dt>
          <dd>
            <code className="hc-code">{evidence.project_id}</code>
          </dd>
        </div>
        <div>
          <dt>{t("retrieval")}</dt>
          <dd data-evidence-field="retrieval">{t(`retrievalModes.${mode}`)}</dd>
        </div>
        {evidence.snapshot_id === undefined || evidence.snapshot_id === null ? null : (
          <div>
            <dt>{t("snapshot")}</dt>
            <dd>
              <code className="hc-code">{evidence.snapshot_id}</code>
            </dd>
          </div>
        )}
        {evidence.captured_at === undefined || evidence.captured_at === null ? null : (
          <div>
            <dt>{t("captured")}</dt>
            <dd className="hc-tabular-nums">
              <time dateTime={evidence.captured_at}>{dateTime(evidence.captured_at)}</time>
            </dd>
          </div>
        )}
        <div>
          <dt>{t("retrieved")}</dt>
          <dd className="hc-tabular-nums">
            <time dateTime={evidence.retrieved_at}>{dateTime(evidence.retrieved_at)}</time>
          </dd>
        </div>
        <div>
          <dt>{t("records")}</dt>
          <dd className="hc-tabular-nums">
            {t("sequences", { count: record.sequences.length, list: record.sequences.join(", ") })}
          </dd>
        </div>
      </dl>
      <CoverageLine coverage={record.coverage} />
      <UntrustedText text={evidence.content} source="record" />
      <h5 className="hc-evidence-subheading">{t("verificationsHeading")}</h5>
      {record.verifications.length === 0 ? (
        <p className="hc-supporting" data-evidence-unverified>
          {t("noVerification")}
        </p>
      ) : (
        <VerificationRounds verifications={record.verifications} record={record} recorded={recorded} />
      )}
    </li>
  );
}

function VerificationRounds({
  verifications,
  record,
  recorded,
}: {
  verifications: RoundedVerification[];
  record: RetrievedRecord | undefined;
  recorded: boolean;
}) {
  const t = useTranslations("Run.evidence");
  return (
    <ul className="hc-rounds" role="list">
      {byRound(verifications).map(([round, list]) => (
        <li key={round} data-round={round}>
          <p className="hc-round-label">{t("round", { round })}</p>
          <ul className="hc-list" role="list">
            {list.map(({ verification, sequence }) => (
              <li key={sequence} data-verification-sequence={sequence}>
                <VerificationLine
                  verification={verification}
                  record={record}
                  recorded={recorded}
                  withId={record === undefined}
                />
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}

function VerificationLine({
  verification,
  record,
  recorded,
  withId,
}: {
  verification: VerificationV1;
  record: RetrievedRecord | undefined;
  recorded: boolean;
  withId: boolean;
}) {
  const t = useTranslations("Run.evidence");
  const tExplorer = useTranslations("Run.explorer");
  return (
    <span className="hc-verification">
      <ProofStatus verification={verification} withId={withId} />
      <Badge
        variant={FACTUAL_VARIANTS[verification.factual_status]}
        label={t("factual", { status: tExplorer(`factual.${verification.factual_status}`) })}
      />
      <Badge
        variant="neutral"
        label={t("relevance", { status: tExplorer(`relevance.${verification.relevance_status}`) })}
      />
      <ProofReadingLine verification={verification} evidence={record?.evidence} recorded={recorded} />
    </span>
  );
}
