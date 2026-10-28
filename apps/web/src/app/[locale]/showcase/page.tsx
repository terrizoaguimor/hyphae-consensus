// SPDX-License-Identifier: Apache-2.0
//
// Research showcase: the archived runs GET /api/v1/showcase lists, read as
// RECORDED, each opening in the run workspace in RECORDED mode, with the
// archive's manifest checked in the browser (src/components/showcase). Behind
// the runtime API gate: without the API the honest placeholder says what the
// view does once connected.

import { getTranslations } from "next-intl/server";

import { ApiGate } from "@/components/ApiGate";
import { PageHeader } from "@/components/PageHeader";
import { RelatedLinks } from "@/components/RelatedLinks";
import { ShowcaseList } from "@/components/showcase/ShowcaseList";
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
      <ApiGate
        planned={[
          t("Showcase.planned.recorded"),
          t("Showcase.planned.corrections"),
          t("Showcase.planned.manifest"),
        ]}
      >
        <ShowcaseList locale={locale} />
      </ApiGate>
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
