// SPDX-License-Identifier: Apache-2.0
//
// The run workspace: one static route per locale, client-rendered from the
// `id` query string (/<locale>/run/?id=<run_id>), behind the runtime API
// gate. Reading the query needs a Suspense boundary in a static export; its
// fallback is the same one-line status the workspace shows while loading.
// The page header is wide (80rem): the column has its final width while the
// gate probes, while the run loads and when it fails, not only once the
// run's own markup appears (the title jumped about 224px sideways on a
// 1920px screen when the width followed .hc-run).

import { getTranslations } from "next-intl/server";
import { Suspense } from "react";

import { ApiGate } from "@/components/ApiGate";
import { PageHeader } from "@/components/PageHeader";
import { RunWorkspace } from "@/components/run/RunWorkspace";
import { type LocaleParams, pageLocale } from "@/lib/pageLocale";

export async function generateMetadata({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return { title: t("Run.title") };
}

export default async function RunPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return (
    <>
      <PageHeader title={t("Run.title")} wide />
      <ApiGate
        planned={[
          t("Runs.planned.own"),
          t("Runs.planned.status"),
          t("NewDeliberation.planned.return"),
        ]}
      >
        <Suspense
          fallback={
            <p role="status" className="hc-supporting">
              {t("Run.loading")}
            </p>
          }
        >
          <RunWorkspace locale={locale} />
        </Suspense>
      </ApiGate>
    </>
  );
}
