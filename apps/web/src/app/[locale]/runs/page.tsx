// SPDX-License-Identifier: Apache-2.0
//
// My runs. Needs the Hyphae Consensus API (session-scoped runs), so this
// build shows an honest placeholder: no sample runs, no count.

import { getTranslations } from "next-intl/server";

import { NotConnected } from "@/components/NotConnected";
import { PageHeader } from "@/components/PageHeader";
import { RelatedLinks } from "@/components/RelatedLinks";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "runs");
}

export default async function MyRunsPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return (
    <>
      <PageHeader title={t("Nav.runs")} lead={t("Runs.lead")} />
      <NotConnected
        locale={locale}
        planned={[
          t("Runs.planned.own"),
          t("Runs.planned.expiry"),
          t("Runs.planned.status"),
        ]}
      />
      <RelatedLinks
        locale={locale}
        links={[{ page: "privacy" }, { doc: "abstain-contested" }]}
      />
    </>
  );
}
