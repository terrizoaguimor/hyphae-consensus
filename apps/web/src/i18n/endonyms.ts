// SPDX-License-Identifier: Apache-2.0
//
// Each language's name for itself, taken from its own catalog. Imported only
// by server components, so the four catalogs never reach the client bundle.

import en from "../../messages/en.json";
import es from "../../messages/es.json";
import fr from "../../messages/fr.json";
import ptBR from "../../messages/pt-BR.json";

import type { Locale } from "./routing";

export const endonyms: Record<Locale, string> = {
  en: en.LocaleSwitcher.names.en,
  es: es.LocaleSwitcher.names.es,
  "pt-BR": ptBR.LocaleSwitcher.names["pt-BR"],
  fr: fr.LocaleSwitcher.names.fr,
};

/** The English catalog, for the language-neutral root page. */
export const defaultMessages = en;
