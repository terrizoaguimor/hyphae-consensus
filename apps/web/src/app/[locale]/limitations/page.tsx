// SPDX-License-Identifier: Apache-2.0
//
// Limitations: what the results can and cannot tell a visitor (draft until
// the owner's review; see LEGAL_TEXT_STATUS).

import { LegalPageContent } from "@/components/LegalPageContent";
import { type LocaleParams, navPageMetadata, pageLocale } from "@/lib/pageLocale";

export function generateMetadata({ params }: LocaleParams) {
  return navPageMetadata(params, "limitations");
}

export default async function LimitationsPage({ params }: LocaleParams) {
  const locale = await pageLocale(params);
  return <LegalPageContent locale={locale} page="limitations" />;
}
