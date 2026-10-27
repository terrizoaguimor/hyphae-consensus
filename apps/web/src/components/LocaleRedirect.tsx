"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Sends a visitor from the root page to their preferred locale. Without
// JavaScript, the root layout's <noscript> refresh sends them to /en/.

import { useEffect } from "react";

import { negotiateLocale } from "@/i18n/negotiate";
import { routing } from "@/i18n/routing";

export function LocaleRedirect() {
  useEffect(() => {
    const preferred =
      navigator.languages.length > 0
        ? navigator.languages
        : [navigator.language];
    const locale = negotiateLocale(
      preferred,
      routing.locales,
      routing.defaultLocale,
    );
    window.location.replace(`/${locale}/`);
  }, []);
  return null;
}
