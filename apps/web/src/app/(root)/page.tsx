// SPDX-License-Identifier: Apache-2.0
//
// The root page: a language list that LocaleRedirect replaces with the
// visitor's preferred locale as soon as the page runs. Like every page it has
// one <h1> (the wordmark) and the non-affiliation line.

import { LocaleRedirect } from "@/components/LocaleRedirect";
import { defaultMessages, endonyms } from "@/i18n/endonyms";
import { routing } from "@/i18n/routing";

export default function RootPage() {
  return (
    <main className="hc-root">
      <h1 className="hc-wordmark">{defaultMessages.App.name}</h1>
      <nav aria-label={defaultMessages.LocaleSwitcher.label}>
        {/* role="list": Safari drops list semantics when list-style is none. */}
        <ul role="list">
          {routing.locales.map((locale) => (
            <li key={locale}>
              <a href={`/${locale}/`} hrefLang={locale} lang={locale}>
                {endonyms[locale]}
              </a>
            </li>
          ))}
        </ul>
      </nav>
      <p className="hc-non-affiliation">
        {defaultMessages.Footer.nonAffiliation}
      </p>
      <LocaleRedirect />
    </main>
  );
}
