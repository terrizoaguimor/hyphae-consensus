// SPDX-License-Identifier: Apache-2.0
//
// Search and link-preview metadata of the static export (src/lib/seo.ts),
// read from the built pages, per page and locale:
//
// - a title and a description of its own (the catalogs' texts, distinct from
//   every other page's in the locale), 70 to 160 characters of description;
// - a canonical link that is absolute and ends in "/", the full and
//   reciprocal hreflang set (en, es, pt-BR, fr, x-default -> en), og:url,
//   og:locale and its alternates, og:title/description/image and the
//   twitter:title/description/image/image:alt;
// - noindex,follow on /run/ and /settings/, noindex on the 404 page, and
//   nothing of the kind on the indexable pages;
// - JSON-LD (a schema.org WebApplication) on each locale's home page only,
//   which parses, has the required fields, and is not one of the page's
//   hashed inline scripts (the hash-only CSP does not govern data blocks);
// - robots.txt, a sitemap whose URL set is exactly the indexable pages the
//   export contains, the multi-size favicon.ico and the web app manifest,
//   each served with the right type.
//
// The build under test has no NEXT_PUBLIC_SITE_ORIGIN, so addresses name the
// project's public site (DEFAULT_PUBLIC_ORIGIN in src/lib/brand.ts).

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";

import type { APIRequestContext } from "@playwright/test";

import { DEFAULT_PUBLIC_ORIGIN } from "../src/lib/brand";
import { docSlugs, footerNav, localePath, primaryNav } from "../src/lib/site";

import { expect, hydrated, type Locale, locales, t, test } from "./fixtures";

const ORIGIN = DEFAULT_PUBLIC_ORIGIN;
const ogLocale: Record<Locale, string> = {
  en: "en_US",
  es: "es_ES",
  "pt-BR": "pt_BR",
  fr: "fr_FR",
};

type PageSpec = {
  /** Path relative to /<locale>/. */
  path: string;
  title: (locale: Locale) => string;
  description: (locale: Locale) => string;
  indexable: boolean;
};

const fullTitle = (locale: Locale, label: string) => t(locale, "Metadata.titleTemplate").replace("%s", label);

const pages: PageSpec[] = [
  ...primaryNav.map(({ key, path }): PageSpec => ({
    path,
    title: (locale) => (key === "overview" ? t(locale, "Metadata.homeTitle") : fullTitle(locale, t(locale, `Nav.${key}`))),
    description: (locale) =>
      key === "overview" ? t(locale, "Metadata.description") : t(locale, `Metadata.pageDescriptions.${key}`),
    indexable: key !== "settings",
  })),
  {
    path: "run",
    title: (locale) => fullTitle(locale, t(locale, "Run.title")),
    description: (locale) => t(locale, "Metadata.pageDescriptions.run"),
    indexable: false,
  },
  ...docSlugs.map((slug): PageSpec => ({
    path: `docs/${slug}`,
    title: (locale) => fullTitle(locale, t(locale, `Docs.articles.${slug}.title`)),
    description: (locale) => t(locale, `Metadata.docDescriptions.${slug}`),
    indexable: true,
  })),
  ...footerNav.map(({ key, path }): PageSpec => ({
    path,
    title: (locale) => fullTitle(locale, t(locale, `Nav.${key}`)),
    description: (locale) => t(locale, `Metadata.pageDescriptions.${key}`),
    indexable: true,
  })),
];

/** Text and attribute values as the HTML source writes them. */
function decode(value: string): string {
  return value
    .replace(/&#x27;|&#39;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

/** The attributes of every <tag ...> in an HTML text. */
function tags(html: string, name: string): Record<string, string>[] {
  return [...html.matchAll(new RegExp(`<${name}\\s([^>]*?)/?>`, "g"))].map((match) =>
    Object.fromEntries(
      [...match[1]!.matchAll(/([\w:-]+)="([^"]*)"/g)].map((attribute) => [
        attribute[1]!.toLowerCase(),
        decode(attribute[2]!),
      ]),
    ),
  );
}

type Head = {
  title: string;
  description: string | undefined;
  robots: string | undefined;
  canonical: string[];
  alternates: Record<string, string>;
  alternateCount: number;
  og: Record<string, string[]>;
  twitter: Record<string, string>;
  manifest: string[];
  jsonLd: string[];
  inlineScriptHashes: string[];
};

function parse(html: string): Head {
  const head = html.slice(0, html.indexOf("</head>"));
  const metas = tags(head, "meta");
  const links = tags(head, "link");
  const og: Record<string, string[]> = {};
  for (const { property, content } of metas) {
    if (property?.startsWith("og:") && content !== undefined) {
      (og[property] ??= []).push(content);
    }
  }
  const alternateLinks = links.filter(({ rel, hreflang }) => rel === "alternate" && hreflang !== undefined);
  return {
    title: decode(/<title>([^<]*)<\/title>/.exec(head)?.[1] ?? ""),
    description: metas.find(({ name }) => name === "description")?.content,
    robots: metas.find(({ name }) => name === "robots")?.content,
    canonical: links.filter(({ rel }) => rel === "canonical").map(({ href }) => href!),
    alternates: Object.fromEntries(alternateLinks.map(({ hreflang, href }) => [hreflang!, href!])),
    alternateCount: alternateLinks.length,
    og,
    twitter: Object.fromEntries(
      metas.filter(({ name }) => name?.startsWith("twitter:")).map(({ name, content }) => [name!, content!]),
    ),
    manifest: links.filter(({ rel }) => rel === "manifest").map(({ href }) => href!),
    jsonLd: [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => m[1]!),
    // Executable inline scripts only: no src, no data-block type.
    inlineScriptHashes: [...html.matchAll(/<script(?![^>]*\b(?:src|type)=)[^>]*>([\s\S]*?)<\/script>/g)]
      .map((m) => m[1]!)
      .filter((body) => body !== "")
      .map((body) => `sha256-${createHash("sha256").update(body, "utf8").digest("base64")}`),
  };
}

type Fetched = { head: Head; html: string; csp: string };

const cache = new Map<string, Fetched>();

async function fetchPage(request: APIRequestContext, path: string): Promise<Fetched> {
  const cached = cache.get(path);
  if (cached !== undefined) {
    return cached;
  }
  const response = await request.get(path, { maxRedirects: 0 });
  expect(response.status(), path).toBe(200);
  const html = await response.text();
  const fetched = { head: parse(html), html, csp: response.headers()["content-security-policy"] ?? "" };
  cache.set(path, fetched);
  return fetched;
}

for (const locale of locales) {
  test(`every page has its own title and description, canonical, hreflang, Open Graph and Twitter tags (${locale})`, async ({
    request,
  }) => {
    const seenTitles = new Map<string, string>();
    const seenDescriptions = new Map<string, string>();
    for (const spec of pages) {
      const path = localePath(locale, spec.path);
      const { head } = await fetchPage(request, path);
      const where = path;
      const url = `${ORIGIN}${path}`;
      const title = spec.title(locale);
      const description = spec.description(locale);

      expect(head.title, where).toBe(title);
      expect(head.description, where).toBe(description);
      expect(description.length, where).toBeGreaterThanOrEqual(70);
      expect(description.length, where).toBeLessThanOrEqual(160);
      if (spec.path === "") {
        expect(title.length, where).toBeLessThanOrEqual(60);
        expect(title, where).toMatch(/^Hyphae Consensus — /);
      }
      expect(seenTitles.get(title), `${where}: title also used by ${seenTitles.get(title)}`).toBeUndefined();
      expect(seenDescriptions.get(description), `${where}: description also used by ${seenDescriptions.get(description)}`).toBeUndefined();
      seenTitles.set(title, path);
      seenDescriptions.set(description, path);

      // Canonical: one, absolute, trailing slash, itself.
      expect(head.canonical, where).toEqual([url]);
      expect(url, where).toMatch(/^https:\/\/[^/]+\/.*\/$/);

      // hreflang: five links, one per locale and x-default (English).
      expect(head.alternateCount, where).toBe(5);
      expect(head.alternates, where).toEqual({
        en: `${ORIGIN}/en/${spec.path === "" ? "" : `${spec.path}/`}`,
        es: `${ORIGIN}/es/${spec.path === "" ? "" : `${spec.path}/`}`,
        "pt-BR": `${ORIGIN}/pt-BR/${spec.path === "" ? "" : `${spec.path}/`}`,
        fr: `${ORIGIN}/fr/${spec.path === "" ? "" : `${spec.path}/`}`,
        "x-default": `${ORIGIN}/en/${spec.path === "" ? "" : `${spec.path}/`}`,
      });

      // Open Graph.
      expect(head.og["og:url"], where).toEqual([url]);
      expect(head.og["og:locale"], where).toEqual([ogLocale[locale]]);
      expect(head.og["og:locale:alternate"], where).toEqual(
        locales.filter((other) => other !== locale).map((other) => ogLocale[other]),
      );
      expect(head.og["og:type"], where).toEqual(["website"]);
      expect(head.og["og:site_name"], where).toEqual([t(locale, "App.name")]);
      expect(head.og["og:title"], where).toEqual([title]);
      expect(head.og["og:description"], where).toEqual([description]);
      expect(head.og["og:image"], where).toEqual([`${ORIGIN}/og-image.png`]);
      expect(head.og["og:image:alt"], where).toEqual([t(locale, "Metadata.ogImageAlt")]);

      // Twitter card.
      expect(head.twitter, where).toEqual({
        "twitter:card": "summary_large_image",
        "twitter:title": title,
        "twitter:description": description,
        "twitter:image": `${ORIGIN}/og-image.png`,
        "twitter:image:alt": t(locale, "Metadata.ogImageAlt"),
      });

      // Robots: only /run/ and /settings/ ask to stay out of the results.
      if (spec.indexable) {
        expect(head.robots, where).toBeUndefined();
      } else {
        expect(head.robots, where).toBe("noindex, follow");
      }

      // The manifest is linked from every page.
      expect(head.manifest, where).toEqual(["/manifest.webmanifest"]);
    }
  });
}

test("the hreflang sets are reciprocal: every alternate lists the same set, and itself", async ({ request }) => {
  for (const locale of locales) {
    for (const spec of pages) {
      const path = localePath(locale, spec.path);
      const { head } = await fetchPage(request, path);
      for (const [hreflang, href] of Object.entries(head.alternates)) {
        expect(href.startsWith(ORIGIN), `${path} -> ${href}`).toBe(true);
        const other = await fetchPage(request, href.slice(ORIGIN.length));
        expect(other.head.alternates, `${path} -> ${href}`).toEqual(head.alternates);
        if (hreflang !== "x-default") {
          expect(other.head.canonical, `${path} -> ${href}`).toEqual([href]);
        }
      }
    }
  }
});

test("JSON-LD: one WebApplication on each locale's home page, none elsewhere, and no hash of it in the CSP", async ({
  request,
}) => {
  const manifest = JSON.parse(readFileSync("out/csp-manifest.json", "utf8")) as {
    pages: Record<string, { scriptHashes: string[]; inlineScripts: { dataBlocks: number } }>;
  };
  for (const locale of locales) {
    for (const spec of pages) {
      const path = localePath(locale, spec.path);
      const { head, csp } = await fetchPage(request, path);
      if (spec.path !== "") {
        expect(head.jsonLd, path).toEqual([]);
        continue;
      }
      expect(head.jsonLd, path).toHaveLength(1);
      const data = JSON.parse(head.jsonLd[0]!) as Record<string, unknown>;
      expect(data, path).toEqual({
        "@context": "https://schema.org",
        "@type": "WebApplication",
        name: t(locale, "App.name"),
        url: `${ORIGIN}${path}`,
        description: t(locale, "Metadata.description"),
        inLanguage: locale,
        applicationCategory: "DeveloperApplication",
        license: "https://www.apache.org/licenses/LICENSE-2.0",
        codeRepository: "https://github.com/terrizoaguimor/hyphae-consensus",
        author: { "@type": "Person", name: "Mario Gutiérrez" },
      });
      expect(JSON.stringify(data), path).not.toMatch(/offers|aggregateRating|review/i);

      // A data block: counted as such in the manifest, never hashed, and the
      // header allows exactly the page's executable inline scripts.
      const jsonLdHash = `sha256-${createHash("sha256").update(head.jsonLd[0]!, "utf8").digest("base64")}`;
      const entry = manifest.pages[path];
      expect(entry?.inlineScripts.dataBlocks, path).toBe(1);
      expect(entry?.scriptHashes, path).not.toContain(jsonLdHash);
      expect(csp, path).not.toContain(jsonLdHash);
      const headerHashes = [...csp.matchAll(/'(sha256-[A-Za-z0-9+/=]+)'/g)].map((m) => m[1]!);
      expect(headerHashes.sort(), path).toEqual([...new Set(head.inlineScriptHashes)].sort());
      expect(entry?.scriptHashes.slice().sort(), path).toEqual(headerHashes.sort());
    }
  }
});

test("the home page's JSON-LD loads without a CSP violation or console error", async ({ page }) => {
  await page.goto("/es/");
  await hydrated(page);
  await expect(page.locator('script[type="application/ld+json"]')).toHaveCount(1);
  // The fixture fails this test if the page logged an error or a CSP violation.
});

test("run and settings are noindex,follow, and so is the 404 page", async ({ request }) => {
  for (const locale of locales) {
    for (const path of ["run", "settings"]) {
      expect((await fetchPage(request, localePath(locale, path))).head.robots).toBe("noindex, follow");
    }
  }
  const missing = await request.get("/en/no-such-page/");
  expect(missing.status()).toBe(404);
  expect(parse(await missing.text()).robots).toBe("noindex");
  expect((await fetchPage(request, "/404.html")).head.robots).toBe("noindex");
});

test("robots.txt allows crawlers, keeps them off /api/ and /research/, and names the absolute sitemap", async ({
  request,
}) => {
  const response = await request.get("/robots.txt", { maxRedirects: 0 });
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toMatch(/^text\/plain/);
  const lines = (await response.text()).split("\n").map((line) => line.trim());
  expect(lines).toContain("User-Agent: *");
  expect(lines).toContain("Allow: /");
  expect(lines).toContain("Disallow: /api/");
  expect(lines).toContain("Disallow: /research/");
  expect(lines).toContain(`Sitemap: ${ORIGIN}/sitemap.xml`);
  // Nothing else is disallowed: the pages, the static files and the sitemap stay crawlable.
  expect(lines.filter((line) => line.startsWith("Disallow:"))).toEqual(["Disallow: /api/", "Disallow: /research/"]);
});

/** Exported localized pages (out/<locale>/**\/index.html) that do not ask to stay out of the results. */
function exportedIndexablePages(): string[] {
  return readdirSync("out", { recursive: true, encoding: "utf8" })
    .map((file) => file.split("\\").join("/"))
    .filter((file) => /^(en|es|pt-BR|fr)\/(.*\/)?index\.html$/.test(file))
    .filter((file) => parse(readFileSync(`out/${file}`, "utf8")).robots === undefined)
    .map((file) => `/${file.slice(0, -"index.html".length)}`)
    .sort();
}

test("the sitemap lists exactly the indexable pages the export contains, in all locales, with hreflang alternates", async ({
  request,
}) => {
  const response = await request.get("/sitemap.xml", { maxRedirects: 0 });
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toMatch(/^application\/xml/);
  const xml = await response.text();
  expect(xml).toContain('xmlns:xhtml="http://www.w3.org/1999/xhtml"');
  const entries = [...xml.matchAll(/<url>([\s\S]*?)<\/url>/g)].map((match) => ({
    loc: /<loc>([^<]*)<\/loc>/.exec(match[1]!)?.[1] ?? "",
    links: Object.fromEntries(
      [...match[1]!.matchAll(/<xhtml:link rel="alternate" hreflang="([^"]+)" href="([^"]+)"/g)].map((m) => [m[1]!, m[2]!]),
    ),
  }));
  const exported = exportedIndexablePages();
  // 16 pages x 4 locales: the overview, new, runs, showcase, docs, 7 articles, terms, privacy, limitations, about.
  expect(exported).toHaveLength(64);
  expect(entries.map(({ loc }) => loc).sort()).toEqual(exported.map((path) => `${ORIGIN}${path}`));
  expect(entries.map(({ loc }) => loc).filter((loc) => /\/(run|settings)\/|404/.test(loc))).toEqual([]);
  for (const { loc, links } of entries) {
    expect(Object.keys(links).sort(), loc).toEqual(["en", "es", "fr", "pt-BR", "x-default"]);
    expect(Object.values(links), loc).toContain(loc);
    expect(links["x-default"], loc).toBe(links["en"]);
    // The sitemap and the page agree on the alternates.
    const { head } = await fetchPage(request, loc.slice(ORIGIN.length));
    expect(links, loc).toEqual(head.alternates);
  }
});

test("favicon.ico is a real 16, 32 and 48 px icon, served as an icon", async ({ request }) => {
  const response = await request.get("/favicon.ico", { maxRedirects: 0 });
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toBe("image/x-icon");
  const ico = await response.body();
  expect([ico.readUInt16LE(0), ico.readUInt16LE(2), ico.readUInt16LE(4)]).toEqual([0, 1, 3]);
  const sizes: number[] = [];
  for (let index = 0; index < 3; index += 1) {
    const entry = 6 + index * 16;
    const size = ico[entry]!;
    sizes.push(size);
    expect(ico[entry + 1], `entry ${index} is square`).toBe(size);
    const length = ico.readUInt32LE(entry + 8);
    const offset = ico.readUInt32LE(entry + 12);
    expect(offset + length, `entry ${index} lies inside the file`).toBeLessThanOrEqual(ico.length);
    const png = ico.subarray(offset, offset + length);
    expect(png.subarray(0, 8).toString("hex"), `entry ${index}`).toBe("89504e470d0a1a0a");
    expect([png.readUInt32BE(16), png.readUInt32BE(20)], `entry ${index}`).toEqual([size, size]);
  }
  expect(sizes).toEqual([16, 32, 48]);
});

test("the web app manifest is served as a manifest and names reachable icons in greys", async ({ request }) => {
  const response = await request.get("/manifest.webmanifest", { maxRedirects: 0 });
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toMatch(/^application\/manifest\+json/);
  const manifest = (await response.json()) as Record<string, unknown> & { icons: { src: string; sizes: string; type: string }[] };
  expect(manifest).toMatchObject({
    name: t("en", "App.name"),
    short_name: t("en", "App.name"),
    start_url: "/en/",
    display: "browser",
    theme_color: "#0B0B0B",
    background_color: "#0B0B0B",
  });
  expect(manifest.icons.map(({ sizes }) => sizes)).toEqual(["512x512", "1024x1024"]);
  for (const icon of manifest.icons) {
    const png = await request.get(icon.src, { maxRedirects: 0 });
    expect(png.status(), icon.src).toBe(200);
    expect(png.headers()["content-type"], icon.src).toBe(icon.type);
    const body = await png.body();
    const side = Number(icon.sizes.split("x")[0]);
    expect([body.readUInt32BE(16), body.readUInt32BE(20)], icon.src).toEqual([side, side]);
  }
  expect((await request.get(String(manifest.start_url), { maxRedirects: 0 })).status()).toBe(200);
  // The CSP lets the browser fetch the manifest.
  const { csp } = await fetchPage(request, "/en/");
  expect(csp).toContain("manifest-src 'self'");
});
