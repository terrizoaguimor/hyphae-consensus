// SPDX-License-Identifier: Apache-2.0
//
// Research showcase. Needs the Hyphae Consensus API (the archive of recorded
// runs), so this build shows an honest placeholder.

import { getTranslations } from "next-intl/server";

import { NotConnected } from "@/components/NotConnected";
import { PageHeader } from "@/components/PageHeader";
import { RelatedLinks } from "@/components/RelatedLinks";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "showcase");
}

export default async function ShowcasePage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return (
    <>
      <PageHeader title={t("Nav.showcase")} lead={t("Showcase.lead")} />
      <NotConnected
        planned={[
          t("Showcase.planned.recorded"),
          t("Showcase.planned.corrections"),
          t("Showcase.planned.manifest"),
        ]}
      />
      <RelatedLinks
        locale={locale}
        links={[
          { page: "limitations" },
          { doc: "modes" },
          { doc: "verified-evidence" },
        ]}
      />
    </>
  );
}
