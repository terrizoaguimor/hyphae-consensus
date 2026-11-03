// SPDX-License-Identifier: Apache-2.0
//
// The search and link-preview metadata (src/lib/seo.ts, src/lib/pageLocale.ts)
// and its texts in the four catalogs:
//
// - which pages are indexable, the hreflang set, the sitemap entries and
//   what the Metadata object of a page holds (canonical, alternates, Open
//   Graph, Twitter card, noindex);
// - the JSON-LD of the home page and its escaping;
// - every page has its own description (70 to 160 characters) and title (the
//   home title at most 60), in every locale, distinct from every other page's
//   and from the same page in the other locales, and none of them implies
//   Meta's endorsement or claims a performance figure.
//
// The built export is checked by e2e/seo.spec.ts.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import { DEFAULT_PUBLIC_ORIGIN, PUBLIC_ORIGIN } from "../src/lib/brand";
import {
  indexablePaths,
  JSON_LD_FACTS,
  jsonLdText,
  languageAlternates,
  noindexPaths,
  ogLocales,
  pageMetadata,
  pageUrl,
  sitemapEntries,
  webApplicationJsonLd,
} from "../src/lib/seo";
import { docSlugs, footerNav, primaryNav } from "../src/lib/site";

type Catalog = { [key: string]: string | Catalog };

function load(file: string): Catalog {
  return JSON.parse(readFileSync(new URL(`../messages/${file}`, import.meta.url), "utf8")) as Catalog;
}

function section(catalog: Catalog, key: string): Catalog {
  const value = catalog[key];
  if (value === undefined || typeof value === "string") {
    throw new Error(`no ${key} section`);
  }
  return value;
}

function text(catalog: Catalog, ...keys: string[]): string {
  let value: string | Catalog | undefined = catalog;
  for (const key of keys) {
    value = typeof value === "string" || value === undefined ? undefined : value[key];
  }
  if (typeof value !== "string") {
    throw new Error(`no message ${keys.join(".")}`);
  }
  return value;
}

const origin = "https://consensus.example";

describe("which pages are indexable", () => {
  it("lists the overview, the primary and footer pages and every docs article, but not run or settings", () => {
    expect(indexablePaths).toEqual([
      "",
      "new",
      "runs",
      "showcase",
      "docs",
      ...docSlugs.map((slug) => `docs/${slug}`),
      "terms",
      "privacy",
      "limitations",
      "about",
    ]);
    for (const path of noindexPaths) {
      expect(indexablePaths, path).not.toContain(path);
    }
    // Every other page of the site map is indexable.
    const nav = [...primaryNav, ...footerNav].map(({ path }) => path);
    expect(nav.filter((path) => !indexablePaths.includes(path))).toEqual(["settings"]);
  });
});

describe("addresses and hreflang", () => {
  it("builds absolute addresses with a trailing slash", () => {
    expect(pageUrl(origin, "en", "")).toBe(`${origin}/en/`);
    expect(pageUrl(origin, "pt-BR", "docs/modes")).toBe(`${origin}/pt-BR/docs/modes/`);
  });

  it("names every locale and x-default (the English page) for a page", () => {
    expect(languageAlternates(origin, "docs/modes")).toEqual({
      en: `${origin}/en/docs/modes/`,
      es: `${origin}/es/docs/modes/`,
      "pt-BR": `${origin}/pt-BR/docs/modes/`,
      fr: `${origin}/fr/docs/modes/`,
      "x-default": `${origin}/en/docs/modes/`,
    });
  });

  it("uses the project's public site when the build names no origin", () => {
    expect(DEFAULT_PUBLIC_ORIGIN).toBe("https://consensus.terrizoaguimor.dev");
    // The test build sets none (tests/brand-assets.test.ts).
    expect(PUBLIC_ORIGIN).toBe(DEFAULT_PUBLIC_ORIGIN);
  });
});

describe("the sitemap", () => {
  const entries = sitemapEntries(origin);

  it("has one entry per indexable page and locale, each with the full alternates", () => {
    expect(entries).toHaveLength(indexablePaths.length * routing.locales.length);
    expect(new Set(entries.map(({ url }) => url)).size).toBe(entries.length);
    for (const entry of entries) {
      expect(entry.url).toMatch(/^https:\/\/consensus\.example\/(en|es|pt-BR|fr)\/.*\/$|^https:\/\/consensus\.example\/(en|es|pt-BR|fr)\/$/);
      const languages = entry.alternates?.languages ?? {};
      expect(Object.keys(languages).sort()).toEqual(["en", "es", "fr", "pt-BR", "x-default"]);
      // Reciprocal: the entry is among its own alternates, and every alternate lists the same set.
      expect(Object.values(languages)).toContain(entry.url);
      for (const alternate of Object.values(languages)) {
        const other = entries.find(({ url }) => url === alternate);
        expect(other?.alternates?.languages, alternate as string).toEqual(languages);
      }
    }
  });

  it("leaves out the run shell, settings and the 404 page", () => {
    for (const { url } of entries) {
      expect(url).not.toMatch(/\/(run|settings)\/|404/);
    }
  });
});

describe("the metadata of a page", () => {
  const base = {
    locale: "es" as const,
    path: "docs/modes",
    title: "Modos · Hyphae Consensus",
    description: "Una descripción de prueba que tiene la longitud suficiente para una página de documentación de ejemplo.",
    siteName: "Hyphae Consensus",
    imageAlt: "Texto alternativo",
    origin,
  };

  it("carries title, description, canonical, alternates, Open Graph and Twitter fields", () => {
    const metadata = pageMetadata(base);
    expect(metadata.title).toEqual({ absolute: base.title });
    expect(metadata.description).toBe(base.description);
    expect(metadata.alternates?.canonical).toBe(`${origin}/es/docs/modes/`);
    expect(metadata.alternates?.languages).toEqual(languageAlternates(origin, "docs/modes"));
    expect(metadata.openGraph).toMatchObject({
      type: "website",
      siteName: "Hyphae Consensus",
      title: base.title,
      description: base.description,
      url: `${origin}/es/docs/modes/`,
      locale: "es_ES",
      alternateLocale: ["en_US", "pt_BR", "fr_FR"],
      images: [{ url: `${origin}/og-image.png`, type: "image/png", width: 1200, height: 630, alt: "Texto alternativo" }],
    });
    expect(metadata.twitter).toMatchObject({
      card: "summary_large_image",
      title: base.title,
      description: base.description,
      images: [{ url: `${origin}/og-image.png`, alt: "Texto alternativo" }],
    });
    expect(metadata.robots).toBeUndefined();
  });

  it("marks a noindex page noindex,follow", () => {
    expect(pageMetadata({ ...base, noindex: true }).robots).toEqual({ index: false, follow: true });
  });

  it("has an og:locale for every locale, in Open Graph's form", () => {
    expect(Object.keys(ogLocales).sort()).toEqual([...routing.locales].sort());
    for (const value of Object.values(ogLocales)) {
      expect(value).toMatch(/^[a-z]{2}_[A-Z]{2}$/);
    }
  });
});

describe("the JSON-LD of the home page", () => {
  const data = webApplicationJsonLd({ locale: "pt-BR", name: "Hyphae Consensus", description: "Descrição.", origin });

  it("is a schema.org WebApplication with the facts the site states and nothing invented", () => {
    expect(data).toEqual({
      "@context": "https://schema.org",
      "@type": "WebApplication",
      name: "Hyphae Consensus",
      url: `${origin}/pt-BR/`,
      description: "Descrição.",
      inLanguage: "pt-BR",
      applicationCategory: "DeveloperApplication",
      license: "https://www.apache.org/licenses/LICENSE-2.0",
      codeRepository: "https://github.com/terrizoaguimor/hyphae-consensus",
      author: { "@type": "Person", name: "Mario Gutiérrez" },
    });
    expect(JSON_LD_FACTS.author).toBe("Mario Gutiérrez");
    expect(JSON.stringify(data)).not.toMatch(/offers|aggregateRating|review|price/i);
  });

  it("is written so no value can end the script element", () => {
    const text = jsonLdText({ name: "</script><!-- x", other: " " });
    expect(text).not.toContain("<");
    expect(JSON.parse(text)).toEqual({ name: "</script><!-- x", other: " " });
  });
});

describe("the texts of the catalogs", () => {
  /** The catalog key of each page with its own entry in pageDescriptions (the overview's is Metadata.description). */
  const pageKeys: string[] = [...primaryNav, ...footerNav]
    .map(({ key }) => key as string)
    .filter((key) => key !== "overview")
    .concat(["run"]);

  for (const locale of routing.locales) {
    const ui = load(`${locale}.json`);
    const docs = load(`docs/${locale}.json`);
    const metadata = section(ui, "Metadata");

    /** Every page's description, by page: the home's, the others', the docs articles'. */
    const descriptions: Record<string, string> = {
      overview: text(metadata, "description"),
      ...Object.fromEntries(pageKeys.map((key) => [key, text(metadata, "pageDescriptions", key)])),
      ...Object.fromEntries(docSlugs.map((slug) => [`docs/${slug}`, text(metadata, "docDescriptions", slug)])),
    };
    /** Every page's title as shown in <title>. */
    const titles: Record<string, string> = {
      overview: text(metadata, "homeTitle"),
      ...Object.fromEntries(
        pageKeys.map((key) => [
          key,
          text(metadata, "titleTemplate").replace("%s", key === "run" ? text(ui, "Run", "title") : text(ui, "Nav", key)),
        ]),
      ),
      ...Object.fromEntries(
        docSlugs.map((slug) => [`docs/${slug}`, text(metadata, "titleTemplate").replace("%s", text(docs, "Docs", "articles", slug, "title"))]),
      ),
    };

    describe(locale, () => {
      it("gives every page a description of 70 to 160 characters", () => {
        expect(Object.keys(descriptions)).toHaveLength(1 + pageKeys.length + docSlugs.length);
        for (const [page, description] of Object.entries(descriptions)) {
          expect(description.length, `${page}: ${description}`).toBeGreaterThanOrEqual(70);
          expect(description.length, `${page}: ${description}`).toBeLessThanOrEqual(160);
        }
      });

      it("gives every page its own description and its own title", () => {
        expect(new Set(Object.values(descriptions)).size).toBe(Object.keys(descriptions).length);
        expect(new Set(Object.values(titles)).size).toBe(Object.keys(titles).length);
      });

      it("keeps the home title to 60 characters and the others short enough to show", () => {
        const home = titles.overview!;
        expect(home.length).toBeLessThanOrEqual(60);
        expect(home).toMatch(/^Hyphae Consensus — /);
        for (const [page, title] of Object.entries(titles)) {
          expect(title.length, `${page}: ${title}`).toBeLessThanOrEqual(70);
          expect(title, page).toContain("Hyphae Consensus");
        }
      });

      it("never implies Meta's endorsement, and says plainly on About that there is none", () => {
        const implies = /official|partner|endors|sponsor|approv|oficial|respald|patroc|endoss|parrain|approuv|afiliad|affili/i;
        for (const [page, description] of Object.entries(descriptions)) {
          if (page === "about") {
            continue;
          }
          expect(description, page).not.toMatch(implies);
        }
        expect(descriptions.about).toMatch(
          /^(Hyphae Consensus is an independent project built for the Meta Global AI Developer Hackathon 2026\. Not affiliated with, sponsored by, or endorsed by Meta\.|Proyecto independiente creado para el Meta Global AI Developer Hackathon 2026; no está afiliado a Meta, ni patrocinado ni respaldado por Meta\.|Projeto independente criado para o Meta Global AI Developer Hackathon 2026; não é afiliado à Meta, nem patrocinado ou endossado pela Meta\.|Hyphae Consensus est un projet indépendant créé pour le Meta Global AI Developer Hackathon 2026\. Non affilié à Meta, ni parrainé ni approuvé par Meta\.)$/,
        );
      });

      it("makes no performance claim", () => {
        const claims = /\d\s?%|\b\d+\s?x\b|faster|fastest|accura|guarante|best|reduc|zero errors|garantiz|garanti|más rápid|mais rápid|plus rapide|mejor|melhor|meilleur|precisi|précis/i;
        for (const [page, description] of Object.entries(descriptions)) {
          expect(description, page).not.toMatch(claims);
        }
      });
    });
  }

  it("differs for every page between any two locales", () => {
    const byLocale = routing.locales.map((locale) => {
      const metadata = section(load(`${locale}.json`), "Metadata");
      return [
        text(metadata, "description"),
        ...pageKeys.map((key) => text(metadata, "pageDescriptions", key)),
        ...docSlugs.map((slug) => text(metadata, "docDescriptions", slug)),
      ];
    });
    for (let page = 0; page < byLocale[0]!.length; page += 1) {
      const texts = byLocale.map((descriptions) => descriptions[page]!);
      expect(new Set(texts).size, texts[0]).toBe(routing.locales.length);
    }
    const homeTitles = routing.locales.map((locale) => text(section(load(`${locale}.json`), "Metadata"), "homeTitle"));
    expect(new Set(homeTitles).size).toBe(routing.locales.length);
  });
});
