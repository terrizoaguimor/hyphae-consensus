// SPDX-License-Identifier: Apache-2.0
//
// About: independence from Meta, Hyphae as the owner's earlier project, the
// license, the AI-assistance disclosure (as in the root README) and the
// source repository, shown as text until the owner sets its public URL. Marked
// Draft like the other legal pages (LEGAL_TEXT_STATUS).

import { getTranslations } from "next-intl/server";

import { LegalPageContent } from "@/components/LegalPageContent";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";
import { SOURCE_REPOSITORY_URL } from "@/lib/site";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "about");
}

export default async function AboutPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return (
    <LegalPageContent locale={locale} page="about">
      <section className="hc-section">
        <h2 id="about-source">{t("About.source.heading")}</h2>
        {SOURCE_REPOSITORY_URL === null ? (
          <p>{t("About.source.pending")}</p>
        ) : (
          <p>
            <a href={SOURCE_REPOSITORY_URL} className="hc-link">
              {t("About.source.link")}
            </a>
          </p>
        )}
      </section>
    </LegalPageContent>
  );
}
