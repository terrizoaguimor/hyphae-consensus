// SPDX-License-Identifier: Apache-2.0
//
// Every catalog file (UI, Docs, Legal) must exist in all four locales with
// identical key sets, every message must be valid ICU MessageFormat (parsed
// with intl-messageformat, the parser next-intl and Astryx use), rich-text
// tags must come from the fixed list in src/i18n/richTags.ts and match the
// English message's tags exactly (except the language tag <en>, which only a
// translation needs), and the files must not share a top-level namespace
// (they are merged per locale).

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { clientNamespaces } from "../src/i18n/messages";
import { LANGUAGE_TAGS, RICH_TAGS } from "../src/i18n/richTags";
import { routing } from "../src/i18n/routing";
import { docSlugs, footerNav, legalPages, primaryNav } from "../src/lib/site";

type Catalog = { [key: string]: string | Catalog };

/** Catalog files, relative to messages/, by group. */
const groups = {
  ui: (locale: string) => `${locale}.json`,
  docs: (locale: string) => `docs/${locale}.json`,
  legal: (locale: string) => `legal/${locale}.json`,
} as const;

type Group = keyof typeof groups;
const groupNames = Object.keys(groups) as Group[];

function load(file: string): Catalog {
  const url = new URL(`../messages/${file}`, import.meta.url);
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

const raw = new Map<string, Catalog>(
  groupNames.flatMap((group) =>
    routing.locales.map(
      (locale) => [`${group}:${locale}`, load(groups[group](locale))] as const,
    ),
  ),
);

function rawCatalog(group: Group, locale: string): Catalog {
  const found = raw.get(`${group}:${locale}`);
  if (found === undefined) {
    throw new Error(`no ${group} catalog for ${locale}`);
  }
  return found;
}

function catalog(group: Group, locale: string): Map<string, string> {
  return flatten(rawCatalog(group, locale));
}

/** All messages of a locale, merged as src/i18n/messages.ts merges them. */
function merged(locale: string): Map<string, string> {
  return new Map(groupNames.flatMap((group) => [...catalog(group, locale)]));
}

// The parser's AST, read structurally: a tag element has a name and
// children; plural and select elements have options holding sub-messages.
type AstNode = {
  value?: unknown;
  children?: AstNode[];
  options?: Record<string, { value: AstNode[] }>;
};

function tagsOf(elements: AstNode[], found = new Set<string>()): Set<string> {
  for (const element of elements) {
    if (Array.isArray(element.children) && typeof element.value === "string") {
      found.add(element.value);
      tagsOf(element.children, found);
    }
    for (const option of Object.values(element.options ?? {})) {
      tagsOf(option.value, found);
    }
  }
  return found;
}

function tags(message: string, locale: string): string[] {
  const ast = new IntlMessageFormat(message, locale).getAst() as AstNode[];
  return [...tagsOf(ast)].sort();
}

/** A catalog subtree at a dotted path. */
function at(root: Catalog, path: string[]): Catalog {
  let node: string | Catalog = root;
  for (const key of path) {
    if (typeof node === "string" || node[key] === undefined) {
      throw new Error(`missing ${path.join(".")}`);
    }
    node = node[key];
  }
  if (typeof node === "string") {
    throw new Error(`${path.join(".")} is a message, not a group`);
  }
  return node;
}

describe("message catalogs", () => {
  it("cover exactly the four portal locales", () => {
    expect([...routing.locales]).toEqual(["en", "es", "pt-BR", "fr"]);
  });

  describe.each(groupNames)("%s files", (group) => {
    const englishKeys = [...catalog(group, "en").keys()].sort();

    it("are not empty", () => {
      expect(englishKeys.length).toBeGreaterThan(0);
    });

    it.each(routing.locales)("%s has exactly the English keys", (locale) => {
      expect([...catalog(group, locale).keys()].sort()).toEqual(englishKeys);
    });

    it.each(routing.locales)(
      "every %s message is non-empty and parses as ICU",
      (locale) => {
        for (const [key, message] of catalog(group, locale)) {
          expect(message.trim(), key).not.toBe("");
          expect(
            () => new IntlMessageFormat(message, locale),
            key,
          ).not.toThrow();
        }
      },
    );

    it.each(routing.locales)(
      "%s uses only allowed rich-text tags, the same as English",
      (locale) => {
        const english = catalog(group, "en");
        const isLanguageTag = (tag: string) =>
          (LANGUAGE_TAGS as readonly string[]).includes(tag);
        for (const [key, message] of catalog(group, locale)) {
          const used = tags(message, locale);
          for (const tag of used) {
            expect(RICH_TAGS as readonly string[], `${key}: <${tag}>`).toContain(
              tag,
            );
          }
          // <en> marks an English term kept in a translation; English text
          // has none to mark, so it is left out of the comparison.
          const englishTags = tags(english.get(key) ?? "", "en");
          expect(englishTags.filter(isLanguageTag), key).toEqual([]);
          expect(used.filter((tag) => !isLanguageTag(tag)), key).toEqual(
            englishTags,
          );
        }
      },
    );

    it.each(routing.locales)("%s messages carry no URLs or raw HTML", (locale) => {
      for (const [key, message] of catalog(group, locale)) {
        // Links come from the tag renderers, never from catalog text.
        expect(message, key).not.toMatch(/https?:\/\//);
        expect(message, key).not.toMatch(/<\/?(a|script|img|iframe|style)\b/i);
      }
    });
  });

  it.each(routing.locales)(
    "%s files do not share a top-level namespace",
    (locale) => {
      const seen = new Map<string, Group>();
      for (const group of groupNames) {
        for (const namespace of Object.keys(rawCatalog(group, locale))) {
          expect(seen.get(namespace), namespace).toBeUndefined();
          seen.set(namespace, group);
        }
      }
    },
  );

  // CLDR maps 0 to "other" in Spanish but to "one" in Portuguese and French,
  // so counts carry an explicit =0 branch that must win in every locale.
  it.each(routing.locales)(
    "%s run count picks its =0 branch for zero",
    (locale) => {
      const format = new IntlMessageFormat(
        catalog("ui", locale).get("Runs.count") ?? "",
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
    expect(catalog("ui", "en").get("Footer.nonAffiliation")).toBe(
      "Independent project; not affiliated with, sponsored by, or endorsed by Meta.",
    );
  });

  // Round 4 (HE #8): the placeholder is shown only when GET /api/v1/health
  // did not answer, so it says that, never "not connected in this build".
  it("carries the exact English not-connected statement", () => {
    expect(catalog("ui", "en").get("NotConnected.message")).toBe(
      "The Hyphae Consensus API did not answer, so this view cannot show anything right now.",
    );
  });

  it("never words an outage as a build that is not connected", () => {
    for (const key of [
      "NotConnected.message",
      "App.buildNote",
      "Overview.primaryActionNote",
      "Tour.steps.welcome.body",
      "Settings.explanation.descriptions.technical",
    ]) {
      const text = catalog("ui", "en").get(key) ?? "";
      expect(text, key).toMatch(/did not answer/u);
      expect(text, key).not.toMatch(/in this build|this build of the portal is not connected/iu);
    }
    for (const key of ["Docs.buildNote", "Docs.articles.modes.sections.build.body"]) {
      const text = catalog("docs", "en").get(key) ?? "";
      expect(text, key).toMatch(/did not answer/u);
      expect(text, key).not.toMatch(/in this build/iu);
    }
  });

  it.each(routing.locales)("%s names the app Hyphae Consensus", (locale) => {
    expect(catalog("ui", locale).get("App.name")).toBe("Hyphae Consensus");
  });
});

describe("catalogs cover the site map", () => {
  it.each(routing.locales)("%s labels every nav entry", (locale) => {
    const messages = merged(locale);
    for (const { key } of [...primaryNav, ...footerNav]) {
      expect(messages.get(`Nav.${key}`), key).toBeTruthy();
    }
  });

  it.each(routing.locales)(
    "%s has a title, summary, sections and technical details for every docs article",
    (locale) => {
      const docs = rawCatalog("docs", locale);
      for (const slug of docSlugs) {
        const article = at(docs, ["Docs", "articles", slug]);
        expect(typeof article.title, slug).toBe("string");
        expect(typeof article.summary, slug).toBe("string");
        expect(Object.keys(at(article, ["sections"])).length, slug).toBeGreaterThan(0);
        expect(Object.keys(at(article, ["technical"])).length, slug).toBeGreaterThan(0);
      }
    },
  );

  it.each(routing.locales)(
    "%s gives every catalog section a heading and a body or items",
    (locale) => {
      const docs = rawCatalog("docs", locale);
      const legal = rawCatalog("legal", locale);
      const pages: Array<[string, Catalog]> = [
        ...legalPages.map((page) => [page, at(legal, ["Legal", page])] as [string, Catalog]),
        ["About", at(legal, ["About"])],
        ...docSlugs.map(
          (slug) => [slug, at(docs, ["Docs", "articles", slug])] as [string, Catalog],
        ),
      ];
      for (const [name, page] of pages) {
        for (const [key, value] of Object.entries(at(page, ["sections"]))) {
          expect(typeof value, `${name}.${key}`).toBe("object");
          const section = value as Catalog;
          expect(typeof section.heading, `${name}.${key}`).toBe("string");
          expect(
            typeof section.body === "string" || typeof section.items === "object",
            `${name}.${key}`,
          ).toBe(true);
        }
      }
    },
  );

  it("sends only UI namespaces to the client", () => {
    const ui = Object.keys(rawCatalog("ui", "en"));
    const docs = Object.keys(rawCatalog("docs", "en"));
    const legal = Object.keys(rawCatalog("legal", "en"));
    for (const namespace of clientNamespaces) {
      expect(ui).toContain(namespace);
      expect(docs).not.toContain(namespace);
      expect(legal).not.toContain(namespace);
    }
  });
});

describe("legal texts state the required facts in every locale", () => {
  // Facts the drafts must carry; their wording awaits the owner's review.
  const facts: Array<[string, RegExp]> = [
    ["Legal.updated", /\{date, date, long\}/],
    ["Legal.terms.sections.who.items.age", /18/],
    ["Legal.terms.sections.who.items.territories", /21/],
    ["Legal.terms.sections.who.items.territories", /<geo>/],
    // Owner decisions (2026-10-27): the whole portal is blocked at country
    // level, Hong Kong and Macau as part of China, all of Ukraine because its
    // listed regions cannot be isolated; South Korea for live mode only.
    ["Legal.terms.sections.who.items.territories", /Hong Kong/],
    ["Legal.terms.sections.who.items.territories", /Maca[uo]/],
    ["Legal.terms.sections.who.items.territories", /Ukraine|Ucrania|Ucrânia/],
    ["Legal.terms.sections.who.items.territories", /Korea|Corea|Coreia|Corée/],
    ["Legal.terms.sections.use.body", /<aup>/],
    ["Legal.terms.sections.outputs.body", /muse-wonder-1\.0/],
    // The model split of 2026-11-01: the Proposer on muse-spark-1.3, the
    // Critic and the Evidence Evaluator on muse-wonder-1.0. The AI-generated
    // content disclosure and the About page name both models.
    ["Legal.terms.sections.outputs.body", /muse-spark-1\.3/],
    ["About.sections.independent.body", /muse-spark-1\.3/],
    ["About.sections.independent.body", /muse-wonder-1\.0/],
    ["About.sections.ai.body", /muse-spark-1\.3/],
    ["About.sections.ai.body", /muse-wonder-1\.0/],
    ["Docs.articles.three-roles.technical.model", /muse-spark-1\.3/],
    ["Docs.articles.three-roles.technical.model", /muse-wonder-1\.0/],
    ["Legal.privacy.sections.summary.items.retention", /24/],
    ["Legal.privacy.sections.summary.items.retention", /7/],
    ["Legal.privacy.sections.runs.body", /24/],
    ["Legal.privacy.sections.runs.body", /7/],
    ["Legal.privacy.sections.retention.items.runs", /7/],
    ["Legal.privacy.sections.controller.body", /Mario Gutiérrez/],
    ["Legal.privacy.sections.controller.body", /\{contact\}/],
    ["Legal.privacy.sections.hosting.body", /\{hostingCountry\}/],
    ["Legal.privacy.sections.hosting.body", /Cloudflare/],
    ["Legal.privacy.sections.cloudflare.body", /<cloudflare>/],
    ["Legal.privacy.sections.cloudflare.body", /__cf_bm/],
    ["Legal.limitations.sections.integrity.body", /HY-1/],
    ["Legal.limitations.sections.chain.body", /SHA-256/],
    ["Legal.limitations.sections.prototype.body", /MA-1/],
    ["Legal.limitations.sections.prototype.body", /RT-1/],
    ["About.sections.hyphae.body", /Celiums Solutions LLC/],
    ["About.sections.hyphae.body", /CC-BY-SA-4\.0/],
    ["About.sections.license.body", /Apache/],
    ["About.sections.ai.body", /Claude/],
    ["About.sections.ai.body", /Muse Code/],
    ["Docs.articles.not-voting.technical.final-round", /MA-1/],
    ["Docs.articles.not-voting.technical.final-round", /RT-1/],
    ["Docs.articles.not-voting.technical.final-round", /1\.3/],
  ];

  it.each(routing.locales)("%s", (locale) => {
    const messages = merged(locale);
    for (const [key, pattern] of facts) {
      expect(messages.get(key), key).toMatch(pattern);
    }
  });
});

// The model split of 2026-11-01 says which role runs which model: the
// Proposer (proposals and revisions) on muse-spark-1.3, the Critic and the
// Evidence Evaluator on muse-wonder-1.0. Mentioning both ids is not enough:
// the superseded split (the Critic on Spark) mentions both too. Every role
// word in a message that names a model id is bound to a model by how the
// sentences are written ("<model> for the Proposer", "<model> (Critic)",
// "the Proposer ... runs on <model>"), and the binding must be the decided
// one, in every catalog and every locale.
describe("the copy binds each role to its model", () => {
  type Model = "muse-spark-1.3" | "muse-wonder-1.0";
  type RoleName = "proposer" | "critic" | "evaluator";
  const modelPattern = /muse-spark-1\.3|muse-wonder-1\.0/g;
  // Role nouns in the four locales, also the outputs they name
  // (proposals, critiques, evaluations). Letter lookarounds, not \b, so
  // accented initials (Évaluateur) and endings (Crítico) delimit correctly.
  const rolePatterns: Record<RoleName, RegExp> = {
    proposer:
      /(?<!\p{L})(?:proposers?|proposals?|proposants?|propositions?|proponentes?|propuestas?|propostas?)(?!\p{L})/giu,
    critic: /(?<!\p{L})(?:critics?|critiques?|críticos?|críticas?)(?!\p{L})/giu,
    evaluator:
      /(?<!\p{L})(?:evaluators?|evaluations?|évaluateurs?|évaluations?|evaluador(?:es)?|evaluaciones|avaliador(?:es)?|avaliações)(?!\p{L})/giu,
  };
  const decided: Record<RoleName, Model> = {
    proposer: "muse-spark-1.3",
    critic: "muse-wonder-1.0",
    evaluator: "muse-wonder-1.0",
  };

  /**
   * Each role word with the model it is bound to: the model right before it
   * when no clause break separates them ("<model> for the Critic"), else the
   * model right after it within the sentence ("the Proposer runs on <model>").
   */
  function bindings(text: string): Array<{ role: RoleName; word: string; model: Model | null }> {
    const models = [...text.matchAll(modelPattern)].map((m) => ({
      model: m[0] as Model,
      start: m.index,
      end: m.index + m[0].length,
    }));
    const out: Array<{ role: RoleName; word: string; model: Model | null }> = [];
    for (const role of Object.keys(rolePatterns) as RoleName[]) {
      for (const m of text.matchAll(rolePatterns[role])) {
        const start = m.index;
        const end = start + m[0].length;
        const before = models.filter((x) => x.end <= start).at(-1);
        const after = models.find((x) => x.start >= end);
        let model: Model | null = null;
        if (before && !/[,;:]|\.\s/.test(text.slice(before.end, start))) {
          model = before.model;
        } else if (after && !/;|\.\s/.test(text.slice(end, after.start))) {
          model = after.model;
        }
        out.push({ role, word: m[0], model });
      }
    }
    return out;
  }

  it("the binder reads the superseded split as wrong", () => {
    const superseded =
      "The Proposer and the Evaluator run on Meta’s muse-wonder-1.0, the Critic on muse-spark-1.3.";
    const found = bindings(superseded);
    expect(found.some((b) => b.model !== null && b.model !== decided[b.role])).toBe(true);
    const current =
      "Live runs call Meta’s muse-spark-1.3 for the Proposer and muse-wonder-1.0 for the Critic and the Evidence Evaluator.";
    expect(bindings(current).every((b) => b.model === decided[b.role])).toBe(true);
  });

  // The messages that name the split (keys as of round 10); any other
  // message naming a model id is checked too.
  const splitKeys = [
    "NewDeliberation.planned.live",
    "Tour.steps.live.body",
    "Provenance.liveMeta.description",
    "Docs.articles.what-is.technical.integrations",
    "Docs.articles.three-roles.technical.model",
    "Docs.articles.modes.sections.live.body",
    "Legal.terms.sections.outputs.body",
    "About.sections.independent.body",
    "About.sections.ai.body",
  ];

  it.each(routing.locales)("%s", (locale) => {
    const messages = merged(locale);
    const naming = [...messages].filter(([, text]) => /muse-spark-1\.3/.test(text));
    expect(naming.map(([key]) => key).sort()).toEqual([...splitKeys].sort());
    for (const [key, text] of naming) {
      const found = bindings(text);
      for (const b of found) {
        if (b.model !== null) {
          expect(`${key}: ${b.word} -> ${b.model}`).toBe(`${key}: ${b.word} -> ${decided[b.role]}`);
        }
      }
      for (const role of Object.keys(decided) as RoleName[]) {
        expect(
          found.some((b) => b.role === role && b.model === decided[role]),
          `${key}: ${role} bound to ${decided[role]}`,
        ).toBe(true);
      }
    }
  });
});
