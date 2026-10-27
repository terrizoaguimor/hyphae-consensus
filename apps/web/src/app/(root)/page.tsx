// SPDX-License-Identifier: Apache-2.0
//
// The root page: a language list that LocaleRedirect replaces with the
// visitor's preferred locale as soon as the page runs.

import { LocaleRedirect } from "@/components/LocaleRedirect";
import { defaultMessages, endonyms } from "@/i18n/endonyms";
import { routing } from "@/i18n/routing";

export default function RootPage() {
  return (
    <main className="hc-root">
      <p className="hc-wordmark">{defaultMessages.App.name}</p>
      <nav aria-label={defaultMessages.LocaleSwitcher.label}>
        <ul>
          {routing.locales.map((locale) => (
            <li key={locale}>
              <a href={`/${locale}/`} hrefLang={locale} lang={locale}>
                {endonyms[locale]}
              </a>
            </li>
          ))}
        </ul>
      </nav>
      <LocaleRedirect />
    </main>
  );
}
