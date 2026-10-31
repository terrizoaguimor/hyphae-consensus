// SPDX-License-Identifier: Apache-2.0
//
// The provenance legend (Overview): what each label means, aligned with the
// Modes article (messages/docs); its intro is in the present tense once the
// API answered and in the planned wording otherwise. Every label is
// text; color only repeats it. The labels sit on separate axes (plan B4), so
// a model label never stands alone: it is always next to the evidence label
// (SEALED SNAPSHOT for public live runs, SIMULATED for the archived hybrid
// runs of a live model with simulated proofs). LIVE META is the label of a
// record delivered live while it is watched; the same model shown afterwards
// (replayed, listed, archived) reads META MODEL (src/lib/provenance.ts). Hyphae
// cyan stays reserved for proof and evidence, so no badge here uses the cyan
// or teal variants.

import { Badge, type BadgeVariant } from "@astryxdesign/core/Badge";
import { Heading } from "@astryxdesign/core/Heading";
import { Text } from "@astryxdesign/core/Text";
import { getTranslations } from "next-intl/server";

import { ConnectionText } from "./ConnectionText";

type LabelKey =
  | "simulated.label"
  | "liveMeta.label"
  | "liveMeta.evidence"
  | "metaModel.label"
  | "recorded.label"
  | "replay.label";

type Sample = {
  /** `Provenance.<key>.description` */
  key: "simulated" | "liveMeta" | "metaModel" | "hybrid" | "recorded" | "replay";
  badges: ReadonlyArray<{ label: LabelKey; variant: BadgeVariant }>;
};

const liveMeta = { label: "liveMeta.label", variant: "info" } as const;
const metaModel = { label: "metaModel.label", variant: "info" } as const;
const sealed = { label: "liveMeta.evidence", variant: "neutral" } as const;
const simulated = { label: "simulated.label", variant: "purple" } as const;

const samples: readonly Sample[] = [
  { key: "simulated", badges: [simulated] },
  { key: "liveMeta", badges: [liveMeta, sealed] },
  { key: "metaModel", badges: [metaModel, sealed] },
  { key: "hybrid", badges: [metaModel, simulated] },
  { key: "recorded", badges: [{ label: "recorded.label", variant: "neutral" }] },
  { key: "replay", badges: [{ label: "replay.label", variant: "blue" }] },
];

export async function ProvenanceLabels() {
  const t = await getTranslations("Provenance");
  return (
    <section className="hc-section">
      <Heading level={2} id="provenance-heading">
        {t("heading")}
      </Heading>
      <Text as="p">
        {/* Present tense once the API answered; the planned wording otherwise (HE #8). */}
        <ConnectionText
          as="span"
          connected={t("introConnected")}
          unreachable={t("intro")}
          pending={t("intro")}
        />
      </Text>
      <dl className="hc-provenance-list">
        {samples.map(({ key, badges }) => (
          <div key={key} className="hc-provenance-item">
            <dt>
              {badges.map(({ label, variant }) => (
                <Badge key={label} variant={variant} label={t(label)} />
              ))}
            </dt>
            <dd>
              <Text color="secondary">{t(`${key}.description` as const)}</Text>
            </dd>
          </div>
        ))}
      </dl>
      <Text as="p" color="secondary">
        {t("note")}
      </Text>
    </section>
  );
}
