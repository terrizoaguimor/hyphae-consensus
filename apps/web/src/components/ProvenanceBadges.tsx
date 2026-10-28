"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The provenance axes of one run or frame as badges, from the recorded
// provenance (never from a request): model next to its evidence label (LIVE
// META never stands alone), the source, the delivery of the latest frame when
// one is known, and every flag the server raised. The predicates and the
// label vocabulary live in src/lib/provenance.ts; the legend on the Overview
// (ProvenanceLabels) uses the same variants.

import { Badge } from "@astryxdesign/core/Badge";
import { useFormatter, useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Delivery, ProvenanceV1 } from "@/lib/api/types";
import {
  deliveryLabel,
  evidenceLabel,
  flagLabel,
  modelLabel,
  modelNoteKey,
  sourceLabel,
} from "@/lib/provenance";

type ProvenanceBadgesProps = {
  provenance: ProvenanceV1;
  /** The delivery of the latest frame, when the view has frames. */
  delivery?: Delivery;
  /** The latest record's own time, for "Meta model · recorded <date>". */
  recordedAt?: string;
  /** Adds the axis names (for a header) instead of badges alone. */
  withNote?: boolean;
};

export function ProvenanceBadges({
  provenance,
  delivery,
  recordedAt,
  withNote = false,
}: ProvenanceBadgesProps) {
  // Label keys are computed from the recorded provenance; every one of them
  // is checked against every catalog by tests/provenance.test.ts.
  const t = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const model = modelLabel(provenance.model);
  const evidence = evidenceLabel(provenance.evidence);
  const source = sourceLabel(provenance.source);
  const flags = provenance.flags ?? [];
  const noteKey = delivery === undefined ? null : modelNoteKey(delivery, provenance);
  return (
    <div className="hc-provenance-badges">
      <ul className="hc-badges" role="list" aria-label={t("heading")}>
        <li>
          <Badge variant={model.variant} label={t(model.key)} />
        </li>
        <li>
          <Badge variant={evidence.variant} label={t(evidence.key)} />
        </li>
        <li>
          <Badge variant={source.variant} label={t(source.key)} />
        </li>
        {delivery === undefined ? null : (
          <li>
            <Badge
              variant={deliveryLabel(delivery, provenance).variant}
              label={t(deliveryLabel(delivery, provenance).key)}
            />
          </li>
        )}
        {flags.map((flag) => (
          <li key={flag}>
            <Badge variant={flagLabel(flag).variant} label={t(flagLabel(flag).key)} />
          </li>
        ))}
      </ul>
      {withNote && noteKey !== null ? (
        <p className="hc-supporting" data-provenance-note={noteKey}>
          {noteKey === "liveMeta.recorded"
            ? t("liveMeta.recorded", {
                date:
                  recordedAt === undefined
                    ? ""
                    : format.dateTime(new Date(recordedAt), {
                        dateStyle: "medium",
                        timeStyle: "short",
                      }),
              })
            : t(noteKey)}
        </p>
      ) : null}
      {withNote
        ? flags.map((flag) => (
            <p key={flag} className="hc-supporting">
              {t(`flagNotes.${flag}`)}
            </p>
          ))
        : null}
    </div>
  );
}
