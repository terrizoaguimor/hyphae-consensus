// SPDX-License-Identifier: Apache-2.0
//
// Sample provenance badges. Every label is text; color only repeats it.
// Hyphae cyan stays reserved for proof and evidence, so no badge here uses
// the cyan or teal variants.

import { Badge, type BadgeVariant } from "@astryxdesign/core/Badge";
import { Heading } from "@astryxdesign/core/Heading";
import { Text } from "@astryxdesign/core/Text";
import { getTranslations } from "next-intl/server";

const labels = [
  { key: "simulated", variant: "purple" },
  { key: "liveMeta", variant: "info" },
  { key: "recorded", variant: "neutral" },
  { key: "replay", variant: "blue" },
] as const satisfies ReadonlyArray<{ key: string; variant: BadgeVariant }>;

export async function ProvenanceLabels() {
  const t = await getTranslations("Provenance");
  return (
    <section aria-labelledby="provenance-heading" className="hc-section">
      <Heading level={2} id="provenance-heading">
        {t("heading")}
      </Heading>
      <Text as="p">{t("intro")}</Text>
      <dl className="hc-provenance-list">
        {labels.map(({ key, variant }) => (
          <div key={key} className="hc-provenance-item">
            <dt>
              <Badge variant={variant} label={t(`${key}.label`)} />
            </dt>
            <dd>
              <Text color="secondary">{t(`${key}.description`)}</Text>
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
