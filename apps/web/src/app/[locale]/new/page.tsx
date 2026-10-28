// SPDX-License-Identifier: Apache-2.0
//
// New deliberation. Needs the Hyphae Consensus API (guided runs and live
// presets), so this build shows an honest placeholder.

import { getTranslations } from "next-intl/server";

import { NotConnected } from "@/components/NotConnected";
import { PageHeader } from "@/components/PageHeader";
import { RelatedLinks } from "@/components/RelatedLinks";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "new");
}

export default async function NewDeliberationPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return (
    <>
      <PageHeader title={t("Nav.new")} lead={t("NewDeliberation.lead")} />
      <NotConnected
        locale={locale}
        planned={[
          t("NewDeliberation.planned.guided"),
          t("NewDeliberation.planned.live"),
          t("NewDeliberation.planned.fallback"),
          t("NewDeliberation.planned.return"),
        ]}
      />
      <RelatedLinks
        locale={locale}
        links={[
          { doc: "modes" },
          { doc: "human-authorization" },
          { page: "terms" },
        ]}
      />
    </>
  );
}
