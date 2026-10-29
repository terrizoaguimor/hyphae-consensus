// SPDX-License-Identifier: Apache-2.0
//
// Fixed copy about the research archive, in every locale:
// - the Showcase lead and the Modes article's showcase section carry no date
//   (the archive's batteries are listed from the data, so a fixed date range
//   would turn false as soon as another battery is archived) and do not call
//   the recorded runs "real" (scripted runs with SIMULATED proofs are among
//   them);
// - the blocked-run sentence of a recorded run cites the archive's own note
//   only through Showcase.run.generationFailedNote.shown, which the run view
//   renders only when that note is on the page.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";

type Catalog = { [key: string]: string | Catalog };

function load(file: string): Catalog {
  const url = new URL(`../messages/${file}`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8")) as Catalog;
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

/** "Real" as each locale says it of runs. */
const REAL: Record<string, RegExp> = {
  en: /(^|[^\p{L}])real([^\p{L}]|$)/iu,
  es: /(^|[^\p{L}])reales?([^\p{L}]|$)/iu,
  "pt-BR": /(^|[^\p{L}])re(al|ais)([^\p{L}]|$)/iu,
  fr: /(^|[^\p{L}])réel(le)?s?([^\p{L}]|$)/iu,
};

/** A year, a day-month date or a month name, as a fixed date in copy would read. */
const DATE =
  /\b20\d\d\b|\b\d{1,2}\s+(de\s+)?(October|octubre|outubro|octobre)\b|(^|[^\p{L}])(January|February|March|April|May|June|July|August|September|October|November|December|enero|febrero|marzo|abril|mayo|junio|julio|agosto|septiembre|octubre|noviembre|diciembre|janeiro|fevereiro|março|maio|junho|julho|setembro|outubro|novembro|dezembro|janvier|février|mars|avril|mai|juin|juillet|août|septembre|octobre|novembre|décembre)([^\p{L}]|$)/iu;

describe("showcase copy", () => {
  it.each(routing.locales)("the Showcase lead and the Modes showcase text carry no date and no 'real' (%s)", (locale) => {
    const ui = load(`${locale}.json`);
    const docs = load(`docs/${locale}.json`);
    const texts = [
      message(ui, "Showcase.lead"),
      message(docs, "Docs.articles.modes.sections.showcase.body"),
    ];
    for (const text of texts) {
      expect(text, text).not.toMatch(DATE);
      expect(text, text).not.toMatch(REAL[locale] ?? /$^/);
    }
  });

  it.each(routing.locales)("the blocked-run sentence cites the archive's note only in its 'shown' clause (%s)", (locale) => {
    const ui = load(`${locale}.json`);
    const shown = message(ui, "Showcase.run.generationFailedNote.shown");
    const sentence = message(ui, "Showcase.run.generationFailed");
    // The clause that names the cause is never part of the sentence itself.
    expect(sentence).not.toContain(shown.replace(/[.,].*$/u, ""));
    for (const key of ["unread", "differs"]) {
      const text = message(ui, `Showcase.run.generationFailedNote.${key}`);
      expect(text).not.toBe(shown);
    }
  });

  it("the blocked-run sentence no longer says the note names the cause (en)", () => {
    const ui = load("en.json");
    expect(message(ui, "Showcase.run.generationFailed")).not.toMatch(/names the cause/);
    expect(message(ui, "Showcase.run.generationFailedNote.shown")).toMatch(/names the cause/);
    expect(message(ui, "Showcase.run.generationFailedNote.unread")).not.toMatch(/names the cause/);
    expect(message(ui, "Showcase.run.generationFailedNote.differs")).not.toMatch(/names the cause/);
  });
});
