// SPDX-License-Identifier: Apache-2.0
//
// The 404 page (out/404.html), served for every path that matches no route.
// Next.js renders it outside both root layouts (experimental.globalNotFound
// in next.config.ts), so this file is a whole document: it imports the
// stylesheets and fonts itself, sets lang on <html> (the texts are English,
// each language link is marked with its own lang), runs the theme script, and
// carries the four language links and the non-affiliation line. The texts
// come from the English catalog, like the root page's.

import "./styles";

import type { Metadata, Viewport } from "next";

import { ThemeScript } from "@/components/ThemeScript";
import { defaultMessages, endonyms } from "@/i18n/endonyms";
import { routing } from "@/i18n/routing";

import { fontVariables } from "./fonts";

// Next.js adds <meta name="robots" content="noindex"> to the not-found page
// by itself.
export const metadata: Metadata = {
  title: defaultMessages.Metadata.titleTemplate.replace(
    "%s",
    defaultMessages.NotFound.title,
  ),
};

export const viewport: Viewport = {
  colorScheme: "light dark",
};

export default function GlobalNotFound() {
  // suppressHydrationWarning: the head script sets data-theme on <html>.
  return (
    <html
      lang={routing.defaultLocale}
      data-astryx-theme="hyphae-mission"
      className={fontVariables}
      suppressHydrationWarning
    >
      <head>
        <ThemeScript />
      </head>
      <body>
        <main className="hc-root hc-not-found">
          <p className="hc-wordmark">{defaultMessages.App.name}</p>
          <h1 className="hc-page-title">{defaultMessages.NotFound.title}</h1>
          <p className="hc-lead">{defaultMessages.NotFound.body}</p>
          <nav aria-label={defaultMessages.LocaleSwitcher.label}>
            <p id="hc-not-found-languages">
              {defaultMessages.NotFound.languages}
            </p>
            {/* role="list": Safari drops list semantics when list-style is none. */}
            <ul role="list" aria-describedby="hc-not-found-languages">
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
        </main>
      </body>
    </html>
  );
}
