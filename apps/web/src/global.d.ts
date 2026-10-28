// SPDX-License-Identifier: Apache-2.0
//
// Types next-intl's locale and message keys after the English catalogs, so a
// missing or misspelled key fails `tsc`. tests/locale-parity.test.ts keeps
// the other catalogs' keys identical to them.

import type docs from "../messages/docs/en.json";
import type ui from "../messages/en.json";
import type legal from "../messages/legal/en.json";

import type { routing } from "./i18n/routing";

declare module "next-intl" {
  interface AppConfig {
    Locale: (typeof routing.locales)[number];
    Messages: typeof ui & typeof docs & typeof legal;
  }
}
