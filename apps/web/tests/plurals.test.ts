// SPDX-License-Identifier: Apache-2.0
//
// Every plural of the UI catalogs carries explicit =0, one and other
// branches, in every locale: CLDR maps 0 to "other" in Spanish but to "one"
// in Portuguese and French, so without =0 a count of zero reads "0 registro"
// (pt-BR) or "0 enregistrement" where the sentence means "no record". The
// =0 branch must win for zero.

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";

type Catalog = { [key: string]: string | Catalog };

type AstNode = {
  value?: unknown;
  pluralType?: string;
  children?: AstNode[];
  options?: Record<string, { value: AstNode[] }>;
};

function flatten(catalog: Catalog, prefix = ""): Array<[string, string]> {
  return Object.entries(catalog).flatMap(([key, value]) => {
    const path = prefix === "" ? key : `${prefix}.${key}`;
    return typeof value === "string" ? [[path, value] as [string, string]] : flatten(value, path);
  });
}

/** Every plural element of a parsed message, nested ones included. */
function plurals(elements: AstNode[], found: AstNode[] = []): AstNode[] {
  for (const element of elements) {
    if (typeof element.pluralType === "string") {
      found.push(element);
    }
    if (Array.isArray(element.children)) {
      plurals(element.children, found);
    }
    for (const option of Object.values(element.options ?? {})) {
      plurals(option.value, found);
    }
  }
  return found;
}

describe.each(routing.locales)("%s plurals", (locale) => {
  const catalog = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  const messages = flatten(catalog);

  it("every plural has =0, one and other branches", () => {
    let count = 0;
    for (const [key, message] of messages) {
      const ast = new IntlMessageFormat(message, locale).getAst() as AstNode[];
      for (const plural of plurals(ast)) {
        count += 1;
        expect(Object.keys(plural.options ?? {}), `${key} (${String(plural.value)})`).toEqual(
          expect.arrayContaining(["=0", "one", "other"]),
        );
      }
    }
    expect(count).toBeGreaterThan(10);
  });

  it("the Explorer's count of zero is its =0 sentence, not the one branch", () => {
    const explorer = messages.find(([key]) => key === "Run.explorer.count")?.[1] ?? "";
    const format = new IntlMessageFormat(explorer, locale);
    const zero = String(format.format({ shown: 0, total: 15 }));
    const one = String(format.format({ shown: 1, total: 15 }));
    // The =0 branch is a sentence of its own, never "1 record" with a 0.
    expect(zero).not.toBe(one.replace("1", "0"));
    expect(zero).toContain("15");
  });
});
