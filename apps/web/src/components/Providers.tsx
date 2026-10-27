"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The one client boundary around every localized page: Astryx Theme,
// LinkProvider (next/link), Astryx i18n, next-intl and Motion. All of these
// are client modules, and Theme and LinkProvider take props that cannot cross
// from a server component (a theme object with icon components, a link
// component), so they live here.

import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import { LinkProvider } from "@astryxdesign/core/Link";
import astryxEsES from "@astryxdesign/core/locales/es-ES.generated.js";
import astryxFrFR from "@astryxdesign/core/locales/fr-FR.generated.js";
import astryxPtBR from "@astryxdesign/core/locales/pt-BR.generated.js";
import { Theme } from "@astryxdesign/core/theme";
import { MotionConfig } from "motion/react";
import Link from "next/link";
import { type AbstractIntlMessages, NextIntlClientProvider } from "next-intl";
import {
  createContext,
  type ReactNode,
  use,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import type { Locale } from "@/i18n/routing";
import { hyphaeMissionTheme } from "@/theme/hyphae-mission";

import { THEME_STORAGE_KEY } from "./themeScriptSource";

// Astryx resolves a locale from specific to general only (pt-BR, then pt) and
// never maps es to es-ES, so each shipped catalog is registered under the
// exact tag this app uses. English ships inside Astryx.
const astryxCatalogs = {
  es: astryxEsES,
  "pt-BR": astryxPtBR,
  fr: astryxFrFR,
};

export type ColorMode = "light" | "dark";

type ColorModeValue = {
  /** null until the client has read the mode set by the head script. */
  mode: ColorMode | null;
  /** Applies a mode and stores it as the visitor's explicit choice. */
  setMode: (mode: ColorMode) => void;
};

const ColorModeContext = createContext<ColorModeValue | null>(null);

export function useColorMode(): ColorModeValue {
  const value = use(ColorModeContext);
  if (value === null) {
    throw new Error("useColorMode must be used inside <Providers>");
  }
  return value;
}

function readDocumentMode(): ColorMode | null {
  if (typeof document === "undefined") {
    return null;
  }
  const mode = document.documentElement.getAttribute("data-theme");
  return mode === "light" || mode === "dark" ? mode : null;
}

function readStoredMode(): ColorMode | null {
  try {
    const mode = localStorage.getItem(THEME_STORAGE_KEY);
    return mode === "light" || mode === "dark" ? mode : null;
  } catch {
    return null;
  }
}

type ProvidersProps = {
  locale: Locale;
  messages: AbstractIntlMessages;
  children: ReactNode;
};

export function Providers({ locale, messages, children }: ProvidersProps) {
  // The static HTML is rendered with Theme in "system" mode, and hydration
  // must render the same. The mode the head script put on <html> is read
  // during that first render, before Theme's layout effect removes the
  // attribute for "system", and applied in a layout effect, which React
  // flushes before the browser paints. Until then globals.css makes the
  // root Theme wrapper follow <html>, so the static page already shows the
  // mode the head script chose.
  const [initialMode] = useState(readDocumentMode);
  const [mode, setModeState] = useState<ColorMode | null>(null);

  useLayoutEffect(() => {
    if (initialMode !== null) {
      // The extra render is the point: it lands before the first paint
      // after hydration, which an event or passive effect would not.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setModeState(initialMode);
    }
  }, [initialMode]);

  // Follow the OS setting until the visitor makes an explicit choice.
  const choseThisPage = useRef(false);
  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (!choseThisPage.current && readStoredMode() === null) {
        setModeState(query.matches ? "dark" : "light");
      }
    };
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const setMode = useCallback((next: ColorMode) => {
    choseThisPage.current = true;
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Storage unavailable: the choice holds for this page view only.
    }
    setModeState(next);
  }, []);

  const colorMode = useMemo(() => ({ mode, setMode }), [mode, setMode]);

  return (
    <ColorModeContext value={colorMode}>
      <Theme theme={hyphaeMissionTheme} mode={mode ?? "system"}>
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
              <MotionConfig reducedMotion="user">{children}</MotionConfig>
            </NextIntlClientProvider>
          </InternationalizationProvider>
        </LinkProvider>
      </Theme>
    </ColorModeContext>
  );
}
