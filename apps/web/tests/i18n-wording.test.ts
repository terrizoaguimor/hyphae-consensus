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
//   re-checked: runs before schema 1.1 carry no chain, and for them only the
//   file's SHA-256 can be compared with the manifest's digest;
// - an outcome's evidence is a tag of its own ("Evidence: SIMULATED"), never
//   a sentence around a translated label (round 4, HE #3 and #24: "on NO
//   EVIDENCE evidence", es "con evidencia SIMULADO").

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import { KNOWN_PROOF_LIMITATIONS } from "../src/lib/run/receipt";

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
    ["Run.explorer.summaries.evaluation_submitted", { corroborated: 12345, claims: 2, unresolved: 3 }, 12345],
    ["Run.explorer.summaries.evaluation_submitted", { corroborated: 2, claims: 12345, unresolved: 3 }, 12345],
    ["Run.explorer.summaries.evaluation_submitted", { corroborated: 2, claims: 3, unresolved: 12345 }, 12345],
    ["Run.audit.claims", { count: 2, corroborated: 12345 }, 12345],
    ["Tour.progress", { current: 12345, total: 2 }, 12345],
    ["Tour.progress", { current: 2, total: 12345 }, 12345],
    ["Tour.announcement", { current: 12345, total: 2, title: "x", body: "y" }, 12345],
    ["Tour.announcement", { current: 2, total: 12345, title: "x", body: "y" }, 12345],
    ["Run.header.playback", { shown: 12345, total: 2 }, 12345],
    ["Run.header.playback", { shown: 2, total: 12345 }, 12345],
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

// What a proof covers (ADR-0008, decision 4): the complete record list of the
// snapshot's project at its anchor, or elsewhere the whole result of the
// request the proof sealed; never a claim, even a recall made with a claim's
// text. The receipt's limitations and the catalogs say the same thing.
const coverage = {
  en: { scope: "the snapshot’s project at that anchor", claim: "even when the request’s query is a claim’s text", old: /led to the retrieval/ },
  es: { scope: "del proyecto de la instantánea en ese anclaje", claim: "aunque la consulta de la solicitud sea el texto de una afirmación", old: /llevó a la recuperación/ },
  fr: { scope: "du projet de l’instantané à cette ancre", claim: "même quand le texte de la requête est celui d’une affirmation", old: /mené à la récupération/ },
  "pt-BR": { scope: "do projeto do snapshot nessa âncora", claim: "mesmo quando a consulta da solicitação é o texto de uma afirmação", old: /levou à recuperação/ },
} as const;

describe.each(routing.locales)("what a proof covers (%s)", (locale) => {
  const ui = load(`${locale}.json`);
  const words = coverage[locale as keyof typeof coverage];

  it("the limitations name the snapshot's project and anchor, and never a claim", () => {
    expect(words).toBeDefined();
    const covers = message(ui, "Run.proofs.limits.covers");
    expect(covers).toContain(words.scope);
    expect(covers).toContain(words.claim);
    expect(covers).not.toMatch(words.old);
    expect(covers).not.toMatch(/captur/);
  });

  it("the plain readings of a proof no longer point at the claim that led to the retrieval", () => {
    expect(message(ui, "Run.proofs.plain.coversSnapshot")).toContain(words.scope);
    expect(message(ui, "Run.proofs.plain.coversSnapshot")).not.toMatch(words.old);
    expect(message(ui, "Run.proofs.plain.covers")).toContain(words.claim);
    expect(message(ui, "Run.proofs.plain.covers")).not.toMatch(words.old);
  });
});

describe("the receipt's limitations and the English catalog", () => {
  it("restate the same sentences", () => {
    const en = load("en.json");
    const ascii = (text: string) => text.replaceAll("’", "'");
    for (const key of ["valid", "covers", "simulated"]) {
      expect(KNOWN_PROOF_LIMITATIONS).toContain(ascii(message(en, `Run.proofs.limits.${key}`)));
    }
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

  it("es names the log 'log' in the docs catalog and the planned showcase too", () => {
    const docs = load("docs/es.json");
    const es = load("es.json");
    expect(JSON.stringify(docs)).not.toMatch(/registros? literal/iu);
    expect(JSON.stringify(docs)).not.toMatch(/registro de eventos/iu);
    expect(JSON.stringify(docs)).not.toMatch(/al abrir un registro/iu);
    expect(message(es, "Showcase.planned.manifest")).not.toMatch(/registro archivado/iu);
    expect(message(es, "Showcase.planned.manifest")).toMatch(/(^|[^\p{L}])log([^\p{L}]|$)/iu);
  });

  it.each(routing.locales)("the Limitations page says what an unchained run still allows: the file's SHA-256 (%s)", (locale) => {
    const body = message(load(`legal/${locale}.json`), "Legal.limitations.sections.showcase.body");
    // The old understatement ("nothing in them can be re-checked") is gone ...
    expect(body).not.toMatch(/nothing in them can be re-checked|nada que volver a comprobar|nada nelas pode ser conferido|rien n’y peut être revérifié/u);
    // ... and the SHA-256 clause appears twice: once for chained runs, once for the runs before 1.1.
    expect(body.match(/SHA-256/gu)?.length).toBe(2);
  });

  it.each(routing.locales)("the Limitations page states which recorded runs can be re-checked (%s)", (locale) => {
    const body = message(load(`legal/${locale}.json`), "Legal.limitations.sections.showcase.body");
    // The cut-off is named, and so is the advice to compare with a tagged commit.
    expect(body).toContain("1.1");
    expect(body).toContain("MANIFEST.json");
    // No count is typed into the page: the runs are listed from the data.
    expect(body).not.toMatch(/\b(seven|siete|sete|sept|7)\b/iu);
  });

  it.each([
    // [locale, the partial verdict's own clause for the records without a hash, the narrowed wording that is gone]
    ["en", "not chain-verified or tamper-evident", "For every later record, which carries a hash"],
    ["es", "no están verificados por la cadena ni dejan rastro si se alteran", "Para cada registro posterior, que lleva un hash"],
    ["pt-BR", "não são verificados pela cadeia nem deixam rastro se forem alterados", "Para cada registro posterior, que traz um hash"],
    ["fr", "ne sont pas vérifiés par la chaîne et ne laissent pas de trace s’ils sont modifiés", "Pour chaque enregistrement suivant, qui porte une empreinte"],
  ] as const)("the partly chained intro matches the partial verdict and every hashed record of the file (%s)", (locale, untouched, narrowed) => {
    const ui = load(`${locale}.json`);
    const intro = message(ui, "Run.chain.introPartlyChained");
    const partial = message(ui, "Run.chain.partialSentence");
    // The records without a hash are described with the verdict's own words ...
    expect(partial).toContain(untouched);
    expect(intro).toContain(untouched);
    // ... and the recomputation is not narrowed to this run's later records:
    // the check recomputes every hashed record of the downloaded file.
    expect(intro).not.toContain(narrowed);
  });

  it.each([
    // [locale, the evidence tag for SIMULATED, for NO EVIDENCE]
    ["en", "Evidence: SIMULATED", "Evidence: NO EVIDENCE"],
    ["es", "Evidencia: SIMULADO", "Evidencia: SIN EVIDENCIA"],
    ["pt-BR", "Evidências: SIMULADO", "Evidências: SEM EVIDÊNCIAS"],
    ["fr", "Éléments probants : SIMULÉ", "Éléments probants : AUCUN ÉLÉMENT PROBANT"],
  ] as const)("an outcome's evidence is its own tag, an axis name and the label, never a sentence (%s)", (locale, simulated, none) => {
    const ui = load(`${locale}.json`);
    const tag = (key: string) =>
      format(locale, "Run.outcomeEvidence", { evidence: message(ui, `Provenance.${key}`) });
    // "Evidence: <label>": no "on NO EVIDENCE evidence", no "con evidencia SIMULADO".
    expect(tag("simulated.label")).toBe(simulated);
    expect(tag("noEvidence.label")).toBe(none);
    expect(format(locale, "Overview.figures.onEvidence", { evidence: message(ui, "Provenance.simulated.label") })).toBe(
      simulated,
    );
    // The one-sentence outcome template is gone, and the showcase's outcome words carry no evidence.
    expect(() => message(ui, "Run.outcome")).toThrow();
    for (const key of ["Showcase.failedAt", "Showcase.otherOutcome"]) {
      expect(message(ui, key)).not.toContain("{evidence}");
    }
  });

  it("the English Limitations page no longer promises every recorded run a re-check", () => {
    const body = message(load("legal/en.json"), "Legal.limitations.sections.showcase.body");
    expect(body).not.toMatch(/^For each recorded run you can re-check/u);
    expect(body).toMatch(/before schema 1\.1 carry no hash chain/u);
  });
});

// The guided scenario cards name the model axis with the run pages' label
// (owner decision 2026-10-31): SCRIPTED (es CON GUION, pt-BR ROTEIRIZADO, fr
// SCÉNARISÉ), never a card-only "SIMULATED model" wording; the label words
// stay translated. The cards read Provenance.scripted.label through
// provenance.ts modelLabel, so the card-only key is gone.
describe("the guided cards' model label is the SCRIPTED label of the run pages", () => {
  it.each([
    ["en", "SCRIPTED"],
    ["es", "CON GUION"],
    ["pt-BR", "ROTEIRIZADO"],
    ["fr", "SCÉNARISÉ"],
  ] as const)("%s", (locale, label) => {
    const ui = load(`${locale}.json`);
    expect(message(ui, "Provenance.scripted.label")).toBe(label);
    expect(() => message(ui, "NewDeliberation.guided.modelBadge")).toThrow();
    expect(JSON.stringify(ui.NewDeliberation)).not.toMatch(/SIMULATED model|Modelo SIMULADO|Modèle SIMULÉ/u);
  });
});
