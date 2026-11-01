// SPDX-License-Identifier: Apache-2.0
//
// Site map and fixed references. Every internal link of the app is built from
// these lists, so there is one place to check: tests/site-map.test.ts requires
// a route file for each entry, and the e2e crawler (e2e/navigation.spec.ts)
// requires every exported link to answer 200 without a redirect.

import type { Locale } from "@/i18n/routing";

/** Primary navigation, in display order. `path` is relative to /<locale>/. */
export const primaryNav = [
  { key: "overview", path: "" },
  { key: "new", path: "new" },
  { key: "runs", path: "runs" },
  { key: "showcase", path: "showcase" },
  { key: "docs", path: "docs" },
  { key: "settings", path: "settings" },
] as const;

/** Legal and project pages, linked from the footer. */
export const footerNav = [
  { key: "terms", path: "terms" },
  { key: "privacy", path: "privacy" },
  { key: "limitations", path: "limitations" },
  { key: "about", path: "about" },
] as const;

export type NavKey =
  | (typeof primaryNav)[number]["key"]
  | (typeof footerNav)[number]["key"];

/** Docs articles, in reading order. Each one is `Docs.articles.<slug>`. */
export const docSlugs = [
  "what-is",
  "three-roles",
  "not-voting",
  "verified-evidence",
  "abstain-contested",
  "human-authorization",
  "modes",
] as const;

export type DocSlug = (typeof docSlugs)[number];

/** Legal pages rendered from `Legal.<key>`. */
export const legalPages = ["terms", "privacy", "limitations"] as const;

export type LegalPage = (typeof legalPages)[number];

/**
 * "draft" shows a notice on the legal pages (Terms, Privacy, Limitations,
 * About) that their texts await the owner's review. Set it to "final" only
 * after that review.
 */
export const LEGAL_TEXT_STATUS: "draft" | "final" = "draft";

/**
 * The day each legal document's own text last changed (its `Legal.<page>` or
 * `About` subtree in messages/legal/<locale>.json, in any locale), shown on
 * that page only: a change to one document never re-dates another.
 * tests/legal-dates.test.ts pins each date to a digest of its document's
 * text, so a text change without a new date fails.
 */
export const LEGAL_TEXT_CHANGED: Readonly<Record<LegalPage | "about", Date>> = {
  terms: new Date("2026-11-01T00:00:00Z"),
  privacy: new Date("2026-11-01T00:00:00Z"),
  limitations: new Date("2026-10-29T00:00:00Z"),
  about: new Date("2026-10-27T00:00:00Z"),
};

/**
 * Whether the portal talks to the Hyphae Consensus API is no longer a build
 * flag: the static export is served by the API server itself, and every view
 * that needs the API asks GET /api/v1/health at runtime
 * (src/lib/api/useApiHealth.ts, src/components/ApiGate.tsx). When the API
 * does not answer, the view shows the not-connected placeholder.
 */

/**
 * Public contact for the privacy notice and the terms (the data controller is
 * named in the texts): an alias that Cloudflare Email Routing forwards to the
 * owner (set 2026-11-01).
 */
export const CONTACT_EMAIL = "consensus@terrizoaguimor.dev";

/**
 * Country of the owner's hosting provider, named in the privacy notice.
 * The VPS is a Vultr instance in Atlanta (set 2026-11-01).
 */
export const HOSTING_COUNTRY = "United States";

/** External pages the texts link to (checked by the e2e crawler's allowlist). */
export const externalLinks = {
  acceptableUsePolicy: "https://dev.meta.ai/legal/acceptable-use-policy",
  geographicUsePolicy: "https://dev.meta.ai/legal/geographic-use-policy",
  hyphae: "https://github.com/Hyphae-Research-Foundation/hyphae",
  cloudflarePrivacy: "https://www.cloudflare.com/privacypolicy/",
} as const;

/**
 * Public URL of this project's source repository. The owner fills it in when
 * the repository is published; until then the About page shows a sentence,
 * never a link that leads nowhere.
 */
export const SOURCE_REPOSITORY_URL: string | null = null;

/** Path of a page in a locale, with the export's trailing slash. */
export function localePath(locale: Locale, path: string): string {
  const trimmed = path.replace(/^\/+|\/+$/g, "");
  return trimmed === "" ? `/${locale}/` : `/${locale}/${trimmed}/`;
}

/** Path of a docs article. */
export function docPath(locale: Locale, slug: DocSlug): string {
  return localePath(locale, `docs/${slug}`);
}

/**
 * The run workspace of one run: a static route per locale, client-rendered
 * from the `id` query string (src/app/[locale]/run/page.tsx).
 */
export function runPath(locale: Locale, runId: string): string {
  return `${localePath(locale, "run")}?id=${encodeURIComponent(runId)}`;
}

/** The `source` query value that opens the run workspace in RECORDED mode. */
export const SHOWCASE_SOURCE = "showcase";

/**
 * A recorded run of the research showcase in the run workspace (RECORDED
 * mode: read from GET /showcase/{id}, never streamed).
 */
export function showcaseRunPath(locale: Locale, runId: string): string {
  return `${runPath(locale, runId)}&source=${SHOWCASE_SOURCE}`;
}

/**
 * How a pathname relates to a nav entry: "page" when it is that page,
 * "section" when it is below it (a docs article under Docs), else null.
 * The Overview entry only matches itself.
 */
export function navMatch(
  pathname: string | null,
  locale: Locale,
  path: string,
): "page" | "section" | null {
  const current = `${(pathname ?? "/").replace(/\/+$/, "")}/`;
  const target = localePath(locale, path);
  if (current === target) {
    return "page";
  }
  if (path !== "" && current.startsWith(target)) {
    return "section";
  }
  return null;
}
