// SPDX-License-Identifier: Apache-2.0
//
// Docs index: the explainer articles, the tour launcher (the tour is always
// reachable from here), and how the explanation level works.

import Link from "next/link";
import { getTranslations } from "next-intl/server";

import { PageHeader } from "@/components/PageHeader";
import { richTags } from "@/components/rich";
import { TourLauncher } from "@/components/TourInvite";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";
import { docPath, docSlugs } from "@/lib/site";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "docs");
}

export default async function DocsPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  const tags = richTags(locale);
  return (
    <>
      <PageHeader title={t("Nav.docs")} lead={t("Docs.lead")}>
        <p className="hc-build-note">{t("Docs.buildNote")}</p>
      </PageHeader>

      <section className="hc-section">
        <h2 id="docs-articles">{t("Docs.articlesHeading")}</h2>
        {/* role="list": Safari drops list semantics when list-style is none. */}
        <ul className="hc-card-list" role="list">
          {docSlugs.map((slug) => (
            <li key={slug} className="hc-card">
              <h3 className="hc-card-title">
                <Link href={docPath(locale, slug)} className="hc-card-link">
                  {t(`Docs.articles.${slug}.title`)}
                </Link>
              </h3>
              <p>{t.rich(`Docs.articles.${slug}.summary`, tags)}</p>
            </li>
          ))}
        </ul>
      </section>

      <section className="hc-panel">
        <h2 id="docs-tour" className="hc-panel-title">
          {t("Docs.tourHeading")}
        </h2>
        <p>{t("Docs.tourText")}</p>
        <div className="hc-actions">
          <TourLauncher />
        </div>
      </section>

      <section className="hc-section">
        <h2 id="docs-level">{t("Docs.levelHeading")}</h2>
        <p>{t.rich("Docs.levelText", tags)}</p>
      </section>
    </>
  );
}
