// SPDX-License-Identifier: Apache-2.0
//
// The four catalogs must have identical key sets, and every message must be
// valid ICU MessageFormat (parsed with intl-messageformat, the parser
// next-intl and Astryx use).

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";

type Catalog = { [key: string]: string | Catalog };

function load(locale: string): Catalog {
  const url = new URL(`../messages/${locale}.json`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8")) as Catalog;
}

function flatten(catalog: Catalog, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  for (const [key, value] of Object.entries(catalog)) {
    const path = prefix === "" ? key : `${prefix}.${key}`;
    if (typeof value === "string") {
      out.set(path, value);
    } else {
      for (const [nested, text] of flatten(value, path)) {
        out.set(nested, text);
      }
    }
  }
  return out;
}

const catalogs = new Map(
  routing.locales.map((locale) => [locale, flatten(load(locale))]),
);

function catalog(locale: string): Map<string, string> {
  const found = catalogs.get(locale as (typeof routing.locales)[number]);
  if (found === undefined) {
    throw new Error(`no catalog for ${locale}`);
  }
  return found;
}

const englishKeys = [...catalog("en").keys()].sort();

describe("message catalogs", () => {
  it("cover exactly the four portal locales", () => {
    expect([...routing.locales]).toEqual(["en", "es", "pt-BR", "fr"]);
  });

  it.each(routing.locales)("%s has exactly the English keys", (locale) => {
    expect([...catalog(locale).keys()].sort()).toEqual(englishKeys);
  });

  it.each(routing.locales)(
    "every %s message is non-empty and parses as ICU",
    (locale) => {
      for (const [key, message] of catalog(locale)) {
        expect(message.trim(), key).not.toBe("");
        expect(() => new IntlMessageFormat(message, locale), key).not.toThrow();
      }
    },
  );

  // CLDR maps 0 to "other" in Spanish but to "one" in Portuguese and French,
  // so counts carry an explicit =0 branch that must win in every locale.
  it.each(routing.locales)(
    "%s run count picks its =0 branch for zero",
    (locale) => {
      const format = new IntlMessageFormat(
        catalog(locale).get("Runs.count") ?? "",
        locale,
      );
      const [plural] = format.getAst();
      expect(plural).toBeDefined();
      if (plural === undefined || !("options" in plural)) {
        throw new Error("Runs.count is not a plural message");
      }
      const options = plural.options as Record<
        string,
        { value: Array<{ value?: unknown }> }
      >;
      expect(Object.keys(options)).toEqual(
        expect.arrayContaining(["=0", "one", "other"]),
      );
      const zeroText = (options["=0"]?.value ?? [])
        .map((part) => String(part.value ?? ""))
        .join("");
      expect(format.format({ count: 0 })).toBe(zeroText);
      expect(format.format({ count: 1 })).not.toBe(zeroText);
      expect(format.format({ count: 2 })).not.toBe(format.format({ count: 1 }));
    },
  );

  it("carries the exact English non-affiliation line", () => {
    expect(catalog("en").get("Footer.nonAffiliation")).toBe(
      "Independent project; not affiliated with, sponsored by, or endorsed by Meta.",
    );
  });

  it.each(routing.locales)("%s names the app Hyphae Consensus", (locale) => {
    expect(catalog(locale).get("App.name")).toBe("Hyphae Consensus");
  });
});
