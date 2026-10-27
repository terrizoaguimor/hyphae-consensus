// SPDX-License-Identifier: Apache-2.0
//
// Day-0 page per locale: header, a short description, sample provenance
// badges and the non-affiliation footer. Its only links are this page in
// each locale.

import { Heading } from "@astryxdesign/core/Heading";
import { Text } from "@astryxdesign/core/Text";
import { notFound } from "next/navigation";
import { hasLocale } from "next-intl";
import { getTranslations, setRequestLocale } from "next-intl/server";

import { LocaleSwitcher } from "@/components/LocaleSwitcher";
import { ProvenanceLabels } from "@/components/ProvenanceLabels";
import { ThemeToggle } from "@/components/ThemeToggle";
import { endonyms } from "@/i18n/endonyms";
import { routing } from "@/i18n/routing";

export default async function HomePage({
  params,
}: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await params;
  if (!hasLocale(routing.locales, locale)) {
    notFound();
  }
  setRequestLocale(locale);
  const t = await getTranslations();

  return (
    <div className="hc-page">
      <header className="hc-header">
        <p className="hc-wordmark">{t("App.name")}</p>
        <div className="hc-header-controls">
          <LocaleSwitcher current={locale} endonyms={endonyms} />
          <ThemeToggle />
        </div>
      </header>
      <main className="hc-main">
        <div className="hc-intro">
          <Heading level={1}>{t("App.tagline")}</Heading>
          <Text as="p">{t("App.intro")}</Text>
          <Text as="p" color="secondary">
            {t("App.scaffoldNote")}
          </Text>
        </div>
        <ProvenanceLabels />
      </main>
      <footer className="hc-footer">
        <Text as="p" type="supporting" color="secondary">
          {t("Footer.nonAffiliation")}
        </Text>
      </footer>
    </div>
  );
}
