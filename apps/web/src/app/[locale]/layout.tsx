// SPDX-License-Identifier: Apache-2.0
//
// Root layout of every localized page, pre-rendered once per locale, and the
// app shell around them: skip link, top bar (wordmark, language, theme;
// Menu below the desktop breakpoint), primary navigation, main content and
// a footer with the non-affiliation line and the legal pages.

import "../styles";

import type { Metadata, Viewport } from "next";
import Link from "next/link";
import { getMessages, getTranslations } from "next-intl/server";
import type { ReactNode } from "react";

import { FooterNav } from "@/components/FooterNav";
import { LocaleSwitcher } from "@/components/LocaleSwitcher";
import { MobileNav } from "@/components/MobileNav";
import { PrimaryNav } from "@/components/PrimaryNav";
import { Providers } from "@/components/Providers";
import { RouteFocus } from "@/components/RouteFocus";
import { ThemeScript } from "@/components/ThemeScript";
import { ThemeToggle } from "@/components/ThemeToggle";
import { endonyms } from "@/i18n/endonyms";
import { pickClientMessages } from "@/i18n/messages";
import { routing } from "@/i18n/routing";
import { type LocaleParams, pageLocale } from "@/lib/pageLocale";
import { localePath } from "@/lib/site";

import { fontVariables } from "../fonts";

export const dynamicParams = false;

export function generateStaticParams() {
  return routing.locales.map((locale) => ({ locale }));
}

export const viewport: Viewport = {
  colorScheme: "light dark",
};

export async function generateMetadata({
  params,
}: LocaleParams): Promise<Metadata> {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale, namespace: "Metadata" });
  return {
    title: { template: t("titleTemplate"), default: t("title") },
    description: t("description"),
  };
}

export default async function LocaleLayout({
  children,
  params,
}: LocaleParams & { children: ReactNode }) {
  const locale = await pageLocale(params);
  const messages = await getMessages();
  const t = await getTranslations({ locale });

  // suppressHydrationWarning: the head script sets data-theme on <html>
  // before React hydrates, and data-motion is set after hydration.
  return (
    <html
      lang={locale}
      data-astryx-theme="hyphae-mission"
      className={fontVariables}
      suppressHydrationWarning
    >
      <head>
        <ThemeScript />
      </head>
      <body>
        <Providers locale={locale} messages={pickClientMessages(messages)}>
          <a className="hc-skip-link" href="#main">
            {t("Shell.skipToContent")}
          </a>
          <div className="hc-shell">
            <header className="hc-topbar hc-glass">
              <Link href={localePath(locale, "")} className="hc-wordmark">
                {t("App.name")}
              </Link>
              <div className="hc-topbar-controls">
                <LocaleSwitcher current={locale} endonyms={endonyms} />
                <ThemeToggle />
              </div>
              <MobileNav locale={locale} endonyms={endonyms} />
            </header>
            <nav
              className="hc-sidebar hc-glass"
              aria-label={t("Shell.primaryNavLabel")}
            >
              <PrimaryNav locale={locale} />
            </nav>
            <main id="main" tabIndex={-1} className="hc-main">
              {children}
            </main>
            <footer className="hc-footer">
              <p className="hc-non-affiliation">
                {t("Footer.nonAffiliation")}
              </p>
              <FooterNav locale={locale} />
            </footer>
          </div>
          <RouteFocus />
        </Providers>
      </body>
    </html>
  );
}
