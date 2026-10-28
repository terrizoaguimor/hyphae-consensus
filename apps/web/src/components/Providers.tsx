"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The one client boundary around every localized page: visitor preferences,
// Astryx Theme, LinkProvider (next/link), Astryx i18n, next-intl, Motion, and
// the onboarding tour (rendered once here so any page can open it). Theme and
// LinkProvider take props that cannot cross from a server component (a theme
// object with icon components, a link component), so they live here.

import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import { LinkProvider } from "@astryxdesign/core/Link";
import astryxEsES from "@astryxdesign/core/locales/es-ES.generated.js";
import astryxFrFR from "@astryxdesign/core/locales/fr-FR.generated.js";
import astryxPtBR from "@astryxdesign/core/locales/pt-BR.generated.js";
import { Theme } from "@astryxdesign/core/theme";
import { MotionConfig } from "motion/react";
import Link from "next/link";
import { type AbstractIntlMessages, NextIntlClientProvider } from "next-intl";
import type { ReactNode } from "react";

import type { Locale } from "@/i18n/routing";
import { hyphaeMissionTheme } from "@/theme/hyphae-mission";

import { PreferencesProvider, usePreferenceState } from "./preferences";
import { Tour } from "./Tour";

// Astryx resolves a locale from specific to general only (pt-BR, then pt) and
// never maps es to es-ES, so each shipped catalog is registered under the
// exact tag this app uses. English ships inside Astryx.
const astryxCatalogs = {
  es: astryxEsES,
  "pt-BR": astryxPtBR,
  fr: astryxFrFR,
};

type ProvidersProps = {
  locale: Locale;
  messages: AbstractIntlMessages;
  children: ReactNode;
};

export function Providers({ locale, messages, children }: ProvidersProps) {
  const preferences = usePreferenceState();
  return (
    <PreferencesProvider value={preferences}>
      <Theme
        theme={hyphaeMissionTheme}
        mode={preferences.colorMode ?? "system"}
      >
        <LinkProvider component={Link}>
          <InternationalizationProvider
            locale={locale}
            messages={astryxCatalogs}
          >
            <NextIntlClientProvider
              locale={locale}
              messages={messages}
              timeZone="UTC"
            >
              <MotionConfig
                reducedMotion={
                  preferences.motion === "reduce" ? "always" : "user"
                }
              >
                {children}
                <Tour />
              </MotionConfig>
            </NextIntlClientProvider>
          </InternationalizationProvider>
        </LinkProvider>
      </Theme>
    </PreferencesProvider>
  );
}
