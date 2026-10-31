"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The provenance axes of one run or frame as badges, from the recorded
// provenance (never from a request): model next to its evidence label, the
// source, the delivery of the latest frame when one is known, and every flag
// the server raised. The model badge reads LIVE META only when the latest
// frame passes isLive; a list entry (no delivery), a replayed frame or a
// flagged run reads META MODEL, with "Meta model · recorded <date>" under it.
// The predicates and the label vocabulary live in src/lib/provenance.ts; the
// legend on the Overview (ProvenanceLabels) uses the same variants.

import { Badge } from "@astryxdesign/core/Badge";
import { useFormatter, useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Delivery, ProvenanceFlag, ProvenanceV1 } from "@/lib/api/types";
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
  /** Adds the model note and the flag notes (for a header) to the badges. */
  withNote?: boolean;
  /** Flags whose note the caller shows itself (in a full-width callout); their badge stays here. */
  notesElsewhere?: readonly ProvenanceFlag[];
};

export function ProvenanceBadges({
  provenance,
  delivery,
  recordedAt,
  withNote = false,
  notesElsewhere = [],
}: ProvenanceBadgesProps) {
  // Label keys are computed from the recorded provenance; every one of them
  // is checked against every catalog by tests/provenance.test.ts.
  const t = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const model = modelLabel(provenance, delivery);
  const evidence = evidenceLabel(provenance.evidence);
  const source = sourceLabel(provenance.source);
  const flags = provenance.flags ?? [];
  const noteKey = modelNoteKey(delivery, provenance);
  // A list (no delivery) names only the recorded date of a live-model run;
  // a header (withNote) names the model note whatever the model.
  const showNote =
    noteKey !== null && (withNote || (noteKey === "liveMeta.recorded" && recordedAt !== undefined));
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
      {showNote && noteKey !== null ? (
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
        ? flags.filter((flag) => !notesElsewhere.includes(flag)).map((flag) => (
            <p key={flag} className="hc-supporting">
              {t(`flagNotes.${flag}`)}
            </p>
          ))
        : null}
    </div>
  );
}
