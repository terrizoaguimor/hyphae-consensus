// SPDX-License-Identifier: Apache-2.0
//
// Settings: theme, language, motion and explanation level, saved in
// localStorage only (src/components/SettingsForm.tsx).

import { getTranslations } from "next-intl/server";

import { PageHeader } from "@/components/PageHeader";
import { SettingsForm } from "@/components/SettingsForm";
import { endonyms } from "@/i18n/endonyms";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "settings");
}

export default async function SettingsPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  return (
    <>
      <PageHeader title={t("Nav.settings")} lead={t("Settings.lead")} />
      <SettingsForm locale={locale} endonyms={endonyms} />
    </>
  );
}
