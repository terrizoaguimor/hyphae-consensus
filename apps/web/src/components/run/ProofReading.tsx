"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The sentence under a proof's status token (src/lib/run/evidence.ts
// proofReading): a simulated proof is already said by its SIMULATED token;
// a proof whose generation failed upstream was never checked; a proof a
// verifier ran on names the verifier and the time, and for a run of the
// research archive adds that the artifacts stay with the operator, so the
// public can re-verify the event chain, not the proof. Shared by the
// Evidence tab and the Proof inspector.

import { useFormatter, useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { EvidenceReferenceV1, VerificationV1 } from "@/lib/api/types";
import { proofReading } from "@/lib/run/evidence";

type ProofReadingLineProps = {
  verification: VerificationV1;
  evidence: EvidenceReferenceV1 | undefined;
  /** The run comes from the research archive (provenance source research_archive). */
  recorded: boolean;
};

export function ProofReadingLine({ verification, evidence, recorded }: ProofReadingLineProps) {
  const t = useTranslations("Run.proofs.reading");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const reading = proofReading(verification, evidence);
  const date = format.dateTime(new Date(verification.checked_at), {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const verifier = verification.verifier ?? null;
  let text: string | null;
  switch (reading.kind) {
    case "simulated":
      // The SIMULATED token and its sentence already say it.
      text = null;
      break;
    case "generation_failed":
      text = t("generationFailed");
      break;
    case "accepted":
    case "rejected":
      if (recorded) {
        text =
          verifier === null
            ? t(reading.kind === "accepted" ? "recordedAcceptedUnnamed" : "recordedRejectedUnnamed", { date })
            : t(reading.kind === "accepted" ? "recordedAccepted" : "recordedRejected", { date, verifier });
      } else {
        text =
          verifier === null
            ? t(reading.kind === "accepted" ? "acceptedUnnamed" : "rejectedUnnamed", { date })
            : t(reading.kind, { date, verifier });
      }
      break;
    case "not_verified":
      text = t("notVerified", { status: tProvenance(`proof.${reading.status}`) });
      break;
  }
  return text === null ? null : (
    <span className="hc-supporting" data-proof-reading={reading.kind}>
      {text}
    </span>
  );
}
