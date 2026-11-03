// SPDX-License-Identifier: Apache-2.0
//
// Privacy notice for the public deployment (final since 2026-11-03; see
// LEGAL_TEXT_STATUS).

import { LegalPageContent } from "@/components/LegalPageContent";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "privacy");
}

export default async function PrivacyPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  return <LegalPageContent locale={locale} page="privacy" />;
}
