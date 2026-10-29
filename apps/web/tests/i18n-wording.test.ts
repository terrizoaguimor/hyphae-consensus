// SPDX-License-Identifier: Apache-2.0
//
// Wording the catalogs must keep, in every locale (the W2 review, round 2):
// - numbers interpolated as plain arguments (byte counts, record counts,
//   invocation counts) are formatted in the page's locale, as the plural '#'
//   next to them is, never printed as raw digits;
// - a live recall's coverage never denies the query the row under it shows,
//   and the Proof inspector has a reading of its own for a recall made with
//   a claim's text;
// - a Meta request id is evidence that a call was recorded, never a "proof"
//   (es prueba, fr preuve: the words the Proof inspector reserves for
//   Hyphae proofs);
// - fr does not say that proofs that were never generated "are invalid";
// - pt-BR and fr keep en's split between a recorded proof that was verified
//   (accepted) and one that was only checked (rejected);
// - es names the verbatim log "log", never "registro", which is its word
//   for a record;
// - the Limitations page does not promise that every recorded run can be
//   re-checked: runs before schema 1.1 carry no chain.

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";

type Catalog = { [key: string]: string | Catalog };

function load(file: string): Catalog {
  return JSON.parse(readFileSync(new URL(`../messages/${file}`, import.meta.url), "utf8")) as Catalog;
}

function message(catalog: Catalog, key: string): string {
  let node: string | Catalog | undefined = catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key}`);
  }
  return node;
}

function format(locale: string, key: string, values: Record<string, string | number>): string {
  return String(new IntlMessageFormat(message(load(`${locale}.json`), key), locale).format(values));
}

describe.each(routing.locales)("numbers in sentences (%s)", (locale) => {
  const grouped = (value: number) => new Intl.NumberFormat(locale).format(value);

  it.each([
    ["Run.chain.sha256", { sha256: "ab", bytes: 1234567 }, 1234567],
    ["Run.chain.sha256Own", { sha256: "ab", bytes: 1234567 }, 1234567],
    ["Run.chain.count", { count: 12345 }, 12345],
    ["Overview.figures.outcomeCount", { outcome: "x", count: 12345 }, 12345],
    ["Overview.figures.modelCount", { label: "x", count: 12345 }, 12345],
    ["Overview.figures.authorizationCount", { authorization: "x", count: 12345 }, 12345],
    ["Run.audit.verificationsByStatus", { status: "x", count: 12345 }, 12345],
    ["Run.explorer.count", { shown: 2, total: 12345 }, 12345],
    ["Run.meta.mismatch", { rows: 12345, recorded: 2 }, 12345],
    ["Run.meta.mismatch", { rows: 2, recorded: 12345 }, 12345],
    ["Run.meta.total", { sum: "x", reported: 12345, calls: 2 }, 12345],
  ] as const)("%s groups its number as the locale does", (key, values, value) => {
    const text = format(locale, key, values);
    expect(text).toContain(grouped(value));
    expect(grouped(value)).not.toBe(String(value));
    expect(text).not.toContain(String(value));
  });
});

describe.each(routing.locales)("a live recall's coverage (%s)", (locale) => {
  const en = load("en.json");
  const ui = load(`${locale}.json`);

  it("no request coverage line repeats the old denial of the query", () => {
    expect(message(en, "Run.evidence.coverage.request")).not.toMatch(/query that led/);
    expect(message(en, "Run.evidence.coverage.requestNoAnchor")).not.toMatch(/query that led/);
    expect(message(en, "Run.proofs.plain.coversRequest")).not.toMatch(/led to the retrieval/);
    expect(message(en, "Run.proofs.plain.coversRequestClaim")).not.toMatch(/led to the retrieval/);
  });

  it("the claim reading exists and differs from the plain request reading", () => {
    expect(message(ui, "Run.evidence.coverage.queryIsClaim")).not.toBe("");
    expect(message(ui, "Run.proofs.plain.coversRequestClaim")).not.toBe(message(ui, "Run.proofs.plain.coversRequest"));
  });
});

describe("translations of the review's round 2", () => {
  it("es and fr never call a Meta request id a proof", () => {
    const es = load("es.json");
    const fr = load("fr.json");
    for (const key of ["Overview.figures.requests", "Run.meta.intro"]) {
      expect(message(es, key)).not.toMatch(/(^|[^\p{L}])prueba([^\p{L}]|$)/iu);
      expect(message(es, key)).toMatch(/evidencia/iu);
      expect(message(fr, key)).not.toMatch(/(^|[^\p{L}])(preuves?|prouve)([^\p{L}]|$)/iu);
    }
  });

  it("fr says proofs that were never generated are marked invalid, never that they are invalid", () => {
    const text = message(load("fr.json"), "Showcase.run.generationFailed");
    expect(text).not.toMatch(/preuves? (est|sont) invalides?/u);
    expect(text).toMatch(/marquées? invalides?/u);
  });

  it.each([
    ["pt-BR", "Verificada"],
    ["fr", "Vérifiée"],
    ["es", "Verificada"],
  ] as const)("%s opens a rejected recorded proof with another word than an accepted one", (locale, verified) => {
    const ui = load(`${locale}.json`);
    for (const key of ["recordedAccepted", "recordedAcceptedUnnamed"]) {
      expect(message(ui, `Run.proofs.reading.${key}`).startsWith(verified)).toBe(true);
    }
    for (const key of ["recordedRejected", "recordedRejectedUnnamed"]) {
      expect(message(ui, `Run.proofs.reading.${key}`).startsWith(verified)).toBe(false);
    }
  });

  it("es names the verbatim log 'log', never 'registro literal'", () => {
    const es = load("es.json");
    const text = JSON.stringify(es);
    expect(text).not.toMatch(/registros? literal/iu);
    for (const key of [
      "Run.audit.logHeading",
      "Run.audit.displayCopy",
      "Run.audit.logUnavailable",
      "Run.audit.cli",
      "Run.audit.intro",
      "Run.decision.ndjson",
      "Run.decision.ndjsonUnavailable",
    ]) {
      expect(message(es, key), key).toMatch(/(^|[^\p{L}])log([^\p{L}]|$)/iu);
    }
  });

  it.each(routing.locales)("the Limitations page states which recorded runs can be re-checked (%s)", (locale) => {
    const body = message(load(`legal/${locale}.json`), "Legal.limitations.sections.showcase.body");
    // The cut-off is named, and so is the advice to compare with a tagged commit.
    expect(body).toContain("1.1");
    expect(body).toContain("MANIFEST.json");
    // No count is typed into the page: the runs are listed from the data.
    expect(body).not.toMatch(/\b(seven|siete|sete|sept|7)\b/iu);
  });

  it("the English Limitations page no longer promises every recorded run a re-check", () => {
    const body = message(load("legal/en.json"), "Legal.limitations.sections.showcase.body");
    expect(body).not.toMatch(/^For each recorded run you can re-check/u);
    expect(body).toMatch(/before schema 1\.1 carry no hash chain/u);
  });
});
