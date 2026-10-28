"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Sends a visitor from the root page to their locale: the language they
// chose before (localStorage), else their browser's preference. Without
// JavaScript, the root layout's <noscript> refresh sends them to /en/.

import { useEffect } from "react";

import { negotiateLocale } from "@/i18n/negotiate";
import { routing } from "@/i18n/routing";
import { parseLocale, readStorage, STORAGE_KEYS } from "@/lib/preferences";

export function LocaleRedirect() {
  useEffect(() => {
    const chosen = parseLocale(readStorage(STORAGE_KEYS.locale));
    const preferred =
      navigator.languages.length > 0
        ? navigator.languages
        : [navigator.language];
    const locale =
      chosen ??
      negotiateLocale(preferred, routing.locales, routing.defaultLocale);
    window.location.replace(`/${locale}/`);
  }, []);
  return null;
}
