// SPDX-License-Identifier: Apache-2.0
//
// Search and link-preview metadata of the pages, as pure functions (the
// pages feed them translated strings; tests/seo.test.ts runs them without
// Next): which pages are indexable, the hreflang set and the sitemap, the
// Metadata object every page returns (title, description, canonical,
// alternates, Open Graph, Twitter card, robots) and the JSON-LD of the home
// page. app/robots.ts, app/sitemap.ts and app/manifest.ts use them too, so
// there is one list of pages and one origin.
//
// Every address is absolute and ends in "/" (the export's trailingSlash):
// that form is the canonical one, and the form the sitemap lists.

import type { Metadata, MetadataRoute } from "next";

import { type Locale, routing } from "@/i18n/routing";

import { OG_IMAGE_SIZE, ogImageUrl, PUBLIC_ORIGIN } from "./brand";
import { docSlugs, footerNav, localePath, primaryNav } from "./site";

/** Pages that carry no meaning for a search engine: the query-driven run shell and the local settings. */
export const noindexPaths = ["run", "settings"] as const;

/**
 * Every indexable page path (relative to /<locale>/, "" is the overview), in
 * navigation order. Not listed: run (an empty, query-driven shell), settings
 * (local preferences) and the 404 page.
 */
export const indexablePaths: readonly string[] = [
  ...primaryNav.map(({ path }) => path).filter((path) => path !== "settings"),
  ...docSlugs.map((slug) => `docs/${slug}`),
  ...footerNav.map(({ path }) => path),
];

/** Absolute, trailing-slash address of a page in a locale. */
export function pageUrl(origin: string, locale: Locale, path: string): string {
  return `${origin}${localePath(locale, path)}`;
}

/**
 * hreflang -> address for one page: each locale (BCP 47, as the routes are
 * named) and x-default, which is the English page.
 */
export function languageAlternates(
  origin: string,
  path: string,
): Record<string, string> {
  return {
    ...Object.fromEntries(
      routing.locales.map((locale) => [locale, pageUrl(origin, locale, path)]),
    ),
    "x-default": pageUrl(origin, routing.defaultLocale, path),
  };
}

/** Open Graph's underscore form of a locale (og:locale). */
export const ogLocales: Record<Locale, string> = {
  en: "en_US",
  es: "es_ES",
  "pt-BR": "pt_BR",
  fr: "fr_FR",
};

/** The sitemap: every indexable page in every locale, each with the full hreflang set. */
export function sitemapEntries(origin: string): MetadataRoute.Sitemap {
  return indexablePaths.flatMap((path) =>
    routing.locales.map((locale) => ({
      url: pageUrl(origin, locale, path),
      alternates: { languages: languageAlternates(origin, path) },
    })),
  );
}

export type PageMetadataInput = {
  locale: Locale;
  /** Path relative to /<locale>/ ("" for the overview). */
  path: string;
  /** The text of <title> as shown, template applied. */
  title: string;
  /** Page-specific, 70 to 160 characters (tests/seo.test.ts). */
  description: string;
  siteName: string;
  imageAlt: string;
  /** robots noindex,follow: the page stays out of search results but its links are followed. */
  noindex?: boolean;
  origin?: string;
};

/**
 * The Metadata of one page. The title is given whole (not as a template
 * input) so <title>, og:title and twitter:title agree.
 */
export function pageMetadata({
  locale,
  path,
  title,
  description,
  siteName,
  imageAlt,
  noindex = false,
  origin = PUBLIC_ORIGIN,
}: PageMetadataInput): Metadata {
  const url = pageUrl(origin, locale, path);
  const image = ogImageUrl(origin);
  return {
    title: { absolute: title },
    description,
    alternates: { canonical: url, languages: languageAlternates(origin, path) },
    openGraph: {
      type: "website",
      siteName,
      title,
      description,
      url,
      locale: ogLocales[locale],
      alternateLocale: routing.locales
        .filter((other) => other !== locale)
        .map((other) => ogLocales[other]),
      images: [
        {
          url: image,
          type: "image/png",
          width: OG_IMAGE_SIZE.width,
          height: OG_IMAGE_SIZE.height,
          alt: imageAlt,
        },
      ],
    },
    twitter: {
      card: "summary_large_image",
      title,
      description,
      images: [{ url: image, alt: imageAlt }],
    },
    ...(noindex ? { robots: { index: false, follow: true } } : {}),
  };
}

/** Where the project's source lives and who wrote it (README, LICENSE). */
export const JSON_LD_FACTS = {
  license: "https://www.apache.org/licenses/LICENSE-2.0",
  codeRepository: "https://github.com/terrizoaguimor/hyphae-consensus",
  author: "Mario Gutiérrez",
} as const;

/**
 * schema.org WebApplication for the home page of a locale. Only facts the
 * site already states: no offers, ratings or reviews.
 */
export function webApplicationJsonLd({
  locale,
  name,
  description,
  origin = PUBLIC_ORIGIN,
}: {
  locale: Locale;
  name: string;
  description: string;
  origin?: string;
}): Record<string, unknown> {
  return {
    "@context": "https://schema.org",
    "@type": "WebApplication",
    name,
    url: pageUrl(origin, locale, ""),
    description,
    inLanguage: locale,
    applicationCategory: "DeveloperApplication",
    license: JSON_LD_FACTS.license,
    codeRepository: JSON_LD_FACTS.codeRepository,
    author: { "@type": "Person", name: JSON_LD_FACTS.author },
  };
}

/**
 * JSON text safe inside a <script> element: "<" is written as the escape
 * <, so no value can end the element or open a comment.
 */
export function jsonLdText(value: unknown): string {
  return JSON.stringify(value).replace(/</g, "\\u003c");
}
