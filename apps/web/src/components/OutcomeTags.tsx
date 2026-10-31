"use client";
// SPDX-License-Identifier: Apache-2.0
//
// An outcome next to the evidence it was judged on, as two tags: the outcome
// ("supported", "blocked", "failed at stage …") and the evidence axis
// ("Evidence: SIMULATED"). Two tags rather than one sentence, so the honesty
// qualifier is never the part that gets cut off: each tag wraps inside itself
// (.hc-tag) and the pair wraps as a group (.hc-outcome-tags), and neither is
// ever truncated. The evidence label comes from src/lib/provenance.ts; the
// tag carries it after an axis name ("Evidence:"), which keeps every locale
// grammatical (no "on NO EVIDENCE evidence", no gender agreement with a
// translated label).

import { useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { EvidenceProvenance } from "@/lib/api/types";
import { evidenceLabel, type TagIconName, type TagRole, type TagVariant } from "@/lib/provenance";

import { Tag } from "./Tag";

type OutcomeTagsProps = {
  /** The outcome in words, already translated. */
  outcome: string;
  variant: TagVariant;
  /** The outcome's own icon (OUTCOME_ICONS), when the outcome is a recommendation kind. */
  icon?: TagIconName;
  evidence: EvidenceProvenance;
};

export function OutcomeTags({ outcome, variant, icon, evidence }: OutcomeTagsProps) {
  const t = useTranslations("Run");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const label = evidenceLabel(evidence);
  return (
    <span className="hc-outcome-tags" data-outcome-tags>
      <Tag className="hc-tag" variant={variant} icon={icon} label={outcome} data-outcome-tag="outcome" />{" "}
      <Tag
        className="hc-tag"
        variant={label.variant}
        labelKey={label.key}
        label={t("outcomeEvidence", { evidence: tProvenance(label.key) })}
        data-outcome-tag="evidence"
        data-evidence={evidence}
      />
    </span>
  );
}

/** A status word as a tag that wraps instead of truncating (the runs list's "awaiting your decision"). */
export function StatusTag({ label, variant, role }: { label: string; variant: TagVariant; role?: TagRole }) {
  return <Tag className="hc-tag" variant={variant} role={role} label={label} data-status-tag />;
}
