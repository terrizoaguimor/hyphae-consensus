// SPDX-License-Identifier: Apache-2.0
//
// New deliberation: the guided examples (GET /api/v1/scenarios) and the live
// presets (GET /api/v1/capabilities), behind the runtime API gate. When the
// API does not answer, the view shows the not-connected placeholder.

import { getTranslations } from "next-intl/server";

import { ApiGate } from "@/components/ApiGate";
import { NewDeliberation } from "@/components/new/NewDeliberation";
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
      <ApiGate
        planned={[
          t("NewDeliberation.planned.guided"),
          t("NewDeliberation.planned.live"),
          t("NewDeliberation.planned.fallback"),
          t("NewDeliberation.planned.return"),
        ]}
      >
        <NewDeliberation locale={locale} />
      </ApiGate>
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
