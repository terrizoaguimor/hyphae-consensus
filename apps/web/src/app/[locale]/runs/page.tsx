// SPDX-License-Identifier: Apache-2.0
//
// My runs: the session's runs from GET /api/v1/runs (own runs only; no list
// and no count without a session), behind the runtime API gate.

import { getTranslations } from "next-intl/server";

import { ApiGate } from "@/components/ApiGate";
import { PageHeader } from "@/components/PageHeader";
import { RelatedLinks } from "@/components/RelatedLinks";
import { RunsList } from "@/components/runs/RunsList";
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
      <ApiGate
        planned={[
          t("Runs.planned.own"),
          t("Runs.planned.expiry"),
          t("Runs.planned.status"),
        ]}
      >
        <RunsList locale={locale} />
      </ApiGate>
      <RelatedLinks
        locale={locale}
        links={[{ page: "privacy" }, { doc: "abstain-contested" }]}
      />
    </>
  );
}
