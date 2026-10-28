"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Proof inspector: one entry per verification record of the run (the
// detail's events and the frames', src/lib/run/evidence.ts), each with a
// plain-language view (what was checked and what it does not establish) and
// a technical view (verifier, checked_at, verified anchor, the report's kind,
// scope and digests, round). The plain view is worded from the receipt's own
// `proof_limitations` and the record's fields: the catalogs translate the
// text the server writes (src/lib/run/receipt.ts), and a receipt that says
// anything else is shown as written, never paraphrased. A simulated proof
// says that nothing cryptographic was checked; a proof whose generation
// failed upstream says nothing was checked; a recorded run's proofs say they
// were verified at run time and that their artifacts stay with the operator.

import { Button } from "@astryxdesign/core/Button";
import { useFormatter, useTranslations } from "next-intl";
import { useMemo } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { ApiError } from "@/lib/api/client";
import type {
  ConsensusFrameV1,
  EvidenceReferenceV1,
  RunDetailV1,
  RunSummaryV1,
  VerificationV1,
} from "@/lib/api/types";
import {
  type Coverage,
  digestPrefix,
  evidenceView,
  proofReading,
  type RoundedVerification,
} from "@/lib/run/evidence";
import { limitationsTranslated } from "@/lib/run/receipt";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { ProofReadingLine } from "./ProofReading";
import { ProofStatus } from "./StageTab";
import type { RunReceipt } from "./useReceipt";

type ProofsTabProps = {
  detail: RunDetailV1 | null;
  frames: ConsensusFrameV1[];
  detailError: ApiError | null;
  summary: RunSummaryV1 | null;
  receipt: RunReceipt;
};

type Entry = RoundedVerification & {
  evidence: EvidenceReferenceV1 | undefined;
  coverage: Coverage | undefined;
};

export function ProofsTab({ detail, frames, detailError, summary, receipt }: ProofsTabProps) {
  const t = useTranslations("Run.proofs");
  const view = useMemo(() => evidenceView(detail, frames), [detail, frames]);
  const entries: Entry[] = useMemo(() => {
    const all: Entry[] = [
      ...view.records.flatMap((record) =>
        record.verifications.map((v) => ({ ...v, evidence: record.evidence, coverage: record.coverage })),
      ),
      ...view.orphanVerifications.map((v) => ({ ...v, evidence: undefined, coverage: undefined })),
    ];
    return all.sort((a, b) => a.sequence - b.sequence);
  }, [view]);
  const recorded = summary?.provenance.source === "research_archive";
  const state = receipt.state;
  const limits = state.status === "ready" ? state.receipt.proof_limitations : null;
  const translated = limits !== null && limitationsTranslated(limits);

  return (
    <div className="hc-proofs-tab" data-proofs-tab>
      <div className="hc-list-head">
        <h3 id="proofs-heading">{t("heading")}</h3>
        <p className="hc-supporting hc-tabular-nums" data-proof-count={entries.length}>
          {t("count", { count: entries.length })}
        </p>
      </div>
      <p className="hc-supporting">{t("intro")}</p>
      {detailError === null ? null : (
        <p className="hc-supporting" data-proofs-partial>
          {t("partial")}
        </p>
      )}

      <section className="hc-panel" aria-labelledby="proofs-limits" data-proof-limits={state.status}>
        <h4 id="proofs-limits">{t("limitsHeading")}</h4>
        {state.status === "ready" ? (
          <>
            {translated ? (
              <ul className="hc-list" data-limits-translated>
                <li>{t("limits.valid")}</li>
                <li>{t("limits.covers")}</li>
                <li>{t("limits.simulated")}</li>
                <li>{t("limits.notEstablished")}</li>
              </ul>
            ) : (
              <p data-limits-untranslated>{t("limitsUntranslated")}</p>
            )}
            <p className="hc-supporting">{t("limitsVerbatim")}</p>
            <blockquote lang="en" className="hc-untrusted-text" data-proof-limitations>
              {limits}
            </blockquote>
          </>
        ) : state.status === "error" ? (
          <>
            <p>{t("receiptFailed")}</p>
            <ApiErrorNotice error={state.error} onRetry={receipt.reload} />
          </>
        ) : (
          <p role="status" className="hc-supporting">
            {t("receiptLoading")}
          </p>
        )}
      </section>

      {entries.length === 0 ? (
        <p className="hc-supporting" data-proofs-none>
          {t("none")}
        </p>
      ) : (
        <ol className="hc-proof-list" role="list" aria-labelledby="proofs-heading">
          {entries.map((entry) => (
            <ProofCard
              key={entry.sequence}
              entry={entry}
              recorded={recorded}
              plain={state.status === "ready" && translated}
              limitsShown={state.status === "ready"}
            />
          ))}
        </ol>
      )}
      {state.status === "ready" ? (
        <div className="hc-actions">
          <Button variant="secondary" size="sm" label={t("receiptReload")} onClick={receipt.reload} />
        </div>
      ) : null}
    </div>
  );
}

function ProofCard({
  entry,
  recorded,
  plain,
  limitsShown,
}: {
  entry: Entry;
  recorded: boolean;
  /** The receipt is read and its limitations are the text the catalogs translate. */
  plain: boolean;
  limitsShown: boolean;
}) {
  const t = useTranslations("Run.proofs");
  const { verification, round, sequence } = entry;
  const titleId = `proof-${sequence}`;
  return (
    <li className="hc-proof-card" data-proof-sequence={sequence} aria-labelledby={titleId}>
      <h4 id={titleId} className="hc-evidence-title">
        {t("title", { id: verification.evidence_id, round })}
      </h4>
      <p>
        <ProofStatus verification={verification} withId={false} />
      </p>
      <h5 className="hc-evidence-subheading">{t("plainHeading")}</h5>
      {plain ? (
        <PlainReading entry={entry} recorded={recorded} />
      ) : (
        <p className="hc-supporting" data-proof-plain="waiting">
          {limitsShown ? t("plainUntranslated") : t("plainNeedsReceipt")}
        </p>
      )}
      <h5 className="hc-evidence-subheading">{t("technicalHeading")}</h5>
      <TechnicalFields verification={verification} round={round} sequence={sequence} />
    </li>
  );
}

function PlainReading({ entry, recorded }: { entry: Entry; recorded: boolean }) {
  const t = useTranslations("Run.proofs.plain");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const { verification, evidence, coverage } = entry;
  const reading = proofReading(verification, evidence);
  const date = format.dateTime(new Date(verification.checked_at), {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const verifier = verification.verifier ?? t("unnamedVerifier");
  const anchor = digestPrefix(verification.verified_anchor);
  const lines: Array<{ key: string; text: string }> = [];
  switch (reading.kind) {
    case "simulated":
      lines.push({
        key: "simulated",
        text: t("simulated", { status: tProvenance(`proof.${reading.status}`) }),
      });
      break;
    case "generation_failed":
      lines.push({ key: "generationFailed", text: t("generationFailed") });
      break;
    case "accepted":
      lines.push({
        key: "accepted",
        text:
          anchor === null
            ? t("acceptedNoAnchor", { verifier, date })
            : t("accepted", { verifier, date, anchor }),
      });
      lines.push({
        key: "covers",
        text: coverage?.kind === "snapshot_list" ? t("coversSnapshot") : t("covers"),
      });
      lines.push({ key: "notEstablished", text: t("notEstablished") });
      break;
    case "rejected":
      lines.push({
        key: "rejected",
        text:
          verification.verifier === undefined || verification.verifier === null
            ? t("rejectedUnnamed", { date })
            : t("rejected", { verifier: verification.verifier, date }),
      });
      break;
    case "not_verified":
      lines.push({
        key: "notVerified",
        text: t("notVerified", { status: tProvenance(`proof.${reading.status}`) }),
      });
      break;
  }
  return (
    <div className="hc-proof-plain" data-proof-plain={reading.kind}>
      {lines.map((line) => (
        <p key={line.key} data-plain-line={line.key}>
          {line.text}
        </p>
      ))}
      {recorded && reading.kind !== "simulated" && reading.kind !== "generation_failed" ? (
        <p>
          <ProofReadingLine verification={verification} evidence={evidence} recorded />
        </p>
      ) : null}
    </div>
  );
}

function TechnicalFields({
  verification,
  round,
  sequence,
}: {
  verification: VerificationV1;
  round: number;
  sequence: number;
}) {
  const t = useTranslations("Run.proofs.fields");
  const format = useFormatter();
  const report = verification.report ?? null;
  const text = (value: string | null | undefined) =>
    value === undefined || value === null || value === "" ? null : value;
  const rows: Array<{ key: string; label: string; value: string | null; code: boolean }> = [
    { key: "verifier", label: t("verifier"), value: text(verification.verifier), code: true },
    {
      key: "checkedAt",
      label: t("checkedAt"),
      value: format.dateTime(new Date(verification.checked_at), {
        dateStyle: "medium",
        timeStyle: "medium",
      }),
      code: false,
    },
    { key: "anchor", label: t("verifiedAnchor"), value: digestPrefix(verification.verified_anchor), code: true },
    { key: "proofRef", label: t("proofRef"), value: text(verification.proof_ref), code: true },
    { key: "reportStatus", label: t("reportStatus"), value: text(report?.status), code: true },
    { key: "reportKind", label: t("reportKind"), value: text(report?.kind), code: true },
    { key: "reportScope", label: t("reportScope"), value: text(report?.scope), code: true },
    { key: "anchorDigest", label: t("anchorDigest"), value: digestPrefix(report?.anchor_digest), code: true },
    { key: "proofDigest", label: t("proofDigest"), value: digestPrefix(report?.proof_digest), code: true },
    { key: "resultDigest", label: t("resultDigest"), value: digestPrefix(report?.result_digest), code: true },
    { key: "round", label: t("round"), value: String(round), code: false },
    { key: "sequence", label: t("sequence"), value: String(sequence), code: false },
    {
      key: "verifierKind",
      label: t("verifierKind"),
      value: verification.simulated ? t("scripted") : t("notScripted"),
      code: false,
    },
  ];
  return (
    <dl className="hc-kv hc-kv--compact" data-proof-fields>
      {rows.map((row) => (
        <div key={row.key} data-proof-field={row.key}>
          <dt>{row.label}</dt>
          <dd>
            {row.value === null ? (
              <span className="hc-supporting">{t("notRecorded")}</span>
            ) : row.code ? (
              <code className="hc-code">{row.value}</code>
            ) : (
              <span className="hc-tabular-nums">{row.value}</span>
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}
