// SPDX-License-Identifier: Apache-2.0
//
// Types next-intl's locale and message keys after the English catalog, so a
// missing or misspelled key fails `tsc`. tests/locale-parity.test.ts keeps
// the other catalogs' keys identical to it.

import type messages from "../messages/en.json";

import type { routing } from "./i18n/routing";

declare module "next-intl" {
  interface AppConfig {
    Locale: (typeof routing.locales)[number];
    Messages: typeof messages;
  }
}
