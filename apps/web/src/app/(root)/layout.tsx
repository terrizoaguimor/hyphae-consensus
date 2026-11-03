// SPDX-License-Identifier: Apache-2.0
//
// Root layout of the language-neutral root page (/), which only forwards
// visitors to a locale. It is a separate root layout from [locale], so going
// from / to /<locale>/ is a full page load.

import "../styles";

import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";

import { SocialMeta } from "@/components/SocialMeta";
import { ThemeScript } from "@/components/ThemeScript";
import { defaultMessages } from "@/i18n/endonyms";
import { routing } from "@/i18n/routing";
import { brandIcons, brandViewport } from "@/lib/brand";

import { fontVariables } from "../fonts";

export const metadata: Metadata = {
  title: defaultMessages.Metadata.homeTitle,
  description: defaultMessages.Metadata.description,
  icons: brandIcons,
};

export const viewport: Viewport = brandViewport;

export default function RootRedirectLayout({
  children,
}: {
  children: ReactNode;
}) {
  // suppressHydrationWarning: the head script sets data-theme on <html>
  // before React hydrates.
  return (
    <html
      lang={routing.defaultLocale}
      data-astryx-theme="hyphae-mission"
      className={fontVariables}
      suppressHydrationWarning
    >
      <head>
        <ThemeScript />
        <SocialMeta
          siteName={defaultMessages.App.name}
          title={defaultMessages.Metadata.homeTitle}
          description={defaultMessages.Metadata.description}
          imageAlt={defaultMessages.Metadata.ogImageAlt}
        />
        <noscript>
          <meta
            httpEquiv="refresh"
            content={`0; url=/${routing.defaultLocale}/`}
          />
        </noscript>
      </head>
      <body>{children}</body>
    </html>
  );
}
