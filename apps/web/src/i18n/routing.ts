// SPDX-License-Identifier: Apache-2.0
//
// Locales of the portal. Every page is pre-rendered once per locale under
// /<locale>/ (static export: no middleware or proxy runs).

import { defineRouting } from "next-intl/routing";

export const routing = defineRouting({
  locales: ["en", "es", "pt-BR", "fr"],
  defaultLocale: "en",
  localePrefix: "always",
});

export type Locale = (typeof routing.locales)[number];
