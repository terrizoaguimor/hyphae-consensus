// SPDX-License-Identifier: Apache-2.0
//
// Overview: what Hyphae Consensus does, the main call to action, the optional
// tour invitation, the provenance labels, and the impact metrics, which need
// the API and are an honest placeholder in this build.
//
// The main button promises only what this build can do: while API_CONNECTED
// is false it reads "How guided examples will work" and opens the Modes
// article; once connected it reads "Run a guided example" and opens New
// deliberation. The caveat under the buttons is not part of a button's
// accessible name, so the name itself must not overpromise.

import { Button } from "@astryxdesign/core/Button";
import { getTranslations } from "next-intl/server";

import { NotConnected } from "@/components/NotConnected";
import { PageHeader } from "@/components/PageHeader";
import { ProvenanceLabels } from "@/components/ProvenanceLabels";
import { TourInvite } from "@/components/TourInvite";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";
import { API_CONNECTED, docPath, localePath } from "@/lib/site";

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
        <p className="hc-build-note">{t("App.buildNote")}</p>
        <div className="hc-actions">
          {API_CONNECTED ? (
            <Button
              href={localePath(locale, "new")}
              variant="primary"
              label={t("Overview.primaryAction")}
            />
          ) : (
            <Button
              href={docPath(locale, "modes")}
              variant="primary"
              label={t("Overview.primaryActionPlanned")}
            />
          )}
          <Button
            href={localePath(locale, "docs")}
            variant="secondary"
            label={t("Overview.secondaryAction")}
          />
        </div>
        {API_CONNECTED ? null : (
          <p className="hc-supporting">{t("Overview.primaryActionNote")}</p>
        )}
      </PageHeader>

      <TourInvite />

      <section className="hc-section">
        <h2 id="overview-flow">{t("Overview.flowHeading")}</h2>
        {/* role="list": Safari drops list semantics when list-style is none. */}
        <ol className="hc-flow" role="list">
          {flowSteps.map((step) => (
            <li key={step} className="hc-flow-step">
              <h3>{t(`Overview.flow.${step}.title`)}</h3>
              <p>{t(`Overview.flow.${step}.body`)}</p>
            </li>
          ))}
        </ol>
      </section>

      <ProvenanceLabels />

      <section className="hc-section">
        <h2 id="overview-metrics">{t("Overview.metricsHeading")}</h2>
        <NotConnected
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
