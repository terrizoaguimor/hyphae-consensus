// SPDX-License-Identifier: Apache-2.0
//
// Overview: what Hyphae Consensus does, the main call to action, the optional
// tour invitation, the provenance labels, and the impact figures, counted in
// the browser from the research archive GET /api/v1/showcase lists
// (src/components/ImpactFigures.tsx): no figure is typed into the page, and
// without the API the honest placeholder says what they will be.
//
// The main button promises only what the server can do, decided at runtime
// (src/components/OverviewActions.tsx): until GET /api/v1/health answers, and
// when it does not, it reads "How guided examples will work" and opens the
// Modes article; once the API answered it reads "Run a guided example" and
// opens New deliberation. The caveat under the buttons is not part of a
// button's accessible name, so the name itself must not overpromise.

import { getTranslations } from "next-intl/server";

import { ImpactFigures } from "@/components/ImpactFigures";
import { OverviewActions } from "@/components/OverviewActions";
import { PageHeader } from "@/components/PageHeader";
import { ProvenanceLabels } from "@/components/ProvenanceLabels";
import { TourInvite } from "@/components/TourInvite";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

const flowSteps = ["roles", "evidence", "policy", "human"] as const;

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "overview");
}

export default async function OverviewPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });

  return (
    <>
      <PageHeader
        title={t("App.tagline")}
        lead={t("App.intro")}
        eyebrow={<p className="hc-eyebrow">{t("Nav.overview")}</p>}
      >
        <OverviewActions locale={locale} buildNote={t("App.buildNote")} />
      </PageHeader>

      <TourInvite />

      <section className="hc-section">
        <h2 id="overview-flow">{t("Overview.flowHeading")}</h2>
        {/* role="list": Safari drops list semantics when list-style is none. */}
        <ol className="hc-flow" role="list">
          {flowSteps.map((step) => (
            <li key={step} className="hc-flow-step hc-glass hc-glass--static">
              <h3>{t(`Overview.flow.${step}.title`)}</h3>
              <p>{t(`Overview.flow.${step}.body`)}</p>
            </li>
          ))}
        </ol>
      </section>

      <ProvenanceLabels />

      <section className="hc-section">
        <h2 id="overview-metrics">{t("Overview.metricsHeading")}</h2>
        <ImpactFigures
          locale={locale}
          planned={[
            t("Overview.metricsPlanned.computed"),
            t("Overview.metricsPlanned.labeled"),
          ]}
        />
      </section>
    </>
  );
}
