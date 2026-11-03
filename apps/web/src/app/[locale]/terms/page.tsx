// SPDX-License-Identifier: Apache-2.0
//
// Terms of use (final since 2026-11-03; see LEGAL_TEXT_STATUS).

import { LegalPageContent } from "@/components/LegalPageContent";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "terms");
}

export default async function TermsPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  return <LegalPageContent locale={locale} page="terms" />;
}
