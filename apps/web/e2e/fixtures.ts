// SPDX-License-Identifier: Apache-2.0
//
// Shared helpers for the e2e suite: the site map (from src/lib/site.ts), the
// catalogs (expected texts come from messages/*.json, never from copies), a
// theme switch that applies before the head script runs, and a fixture that
// fails any test whose pages logged an error, threw, or violated the CSP.

import { readFileSync } from "node:fs";
import path from "node:path";

import { expect, type Page, test as base } from "@playwright/test";

import {
  docSlugs,
  footerNav,
  localePath,
  primaryNav,
} from "../src/lib/site";

export { expect };

export const locales = ["en", "es", "pt-BR", "fr"] as const;
export type Locale = (typeof locales)[number];

type Catalog = { [key: string]: string | Catalog };

function load(file: string): Catalog {
  return JSON.parse(
    // CommonJS under Playwright: __dirname, not import.meta.url.
    readFileSync(path.join(__dirname, "..", "messages", file), "utf8"),
  ) as Catalog;
}

const catalogs = new Map<Locale, Catalog>(
  locales.map((locale) => [
    locale,
    {
      ...load(`${locale}.json`),
      ...load(`docs/${locale}.json`),
      ...load(`legal/${locale}.json`),
    },
  ]),
);

/**
 * A message as plain text: ICU arguments filled from `values`, rich-text tags
 * dropped (their content kept). Enough for the messages the tests read.
 */
export function t(
  locale: Locale,
  key: string,
  values: Record<string, string | number> = {},
): string {
  let node: string | Catalog | undefined = catalogs.get(locale);
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key} in ${locale}`);
  }
  return node
    .replace(/\{(\w+)\}/g, (_, name: string) => String(values[name] ?? ""))
    .replace(/<\/?\w+>/g, "");
}

export type SitePage = { path: string; heading: string };

/** Every localized page of the export, with its expected <h1>. */
export function pagesFor(locale: Locale): SitePage[] {
  const nav = [...primaryNav, ...footerNav].map(({ key, path }) => ({
    path: localePath(locale, path),
    heading: key === "overview" ? t(locale, "App.tagline") : t(locale, `Nav.${key}`),
  }));
  const docs = docSlugs.map((slug) => ({
    path: localePath(locale, `docs/${slug}`),
    heading: t(locale, `Docs.articles.${slug}.title`),
  }));
  return [...nav, ...docs];
}

/** Waits until React hydrated the page (src/components/preferences.tsx). */
export async function hydrated(page: Page): Promise<void> {
  await page.locator("html[data-hydrated]").waitFor({ state: "attached" });
}

/**
 * Starts every page of this test in `theme`: the stored choice is in place
 * before the head script reads it, and the OS preference agrees with it.
 */
export async function useTheme(page: Page, theme: "light" | "dark") {
  await page.emulateMedia({ colorScheme: theme });
  await page.addInitScript((mode) => {
    try {
      window.localStorage.setItem("hc-theme", mode);
    } catch {
      // Storage unavailable: the OS preference above still applies.
    }
  }, theme);
}

/** localStorage keys the app stored (hc-*). */
export async function storedKeys(page: Page): Promise<Record<string, string>> {
  return page.evaluate(() =>
    Object.fromEntries(
      Object.keys(window.localStorage)
        .filter((key) => key.startsWith("hc-"))
        .map((key) => [key, window.localStorage.getItem(key) ?? ""]),
    ),
  );
}

/** True when focus is inside the open dialog. */
export async function focusInOpenDialog(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const dialog = document.querySelector("dialog[open]");
    return dialog !== null && dialog.contains(document.activeElement);
  });
}

/**
 * WebKit reports a same-origin load that a navigation cancelled (Next's link
 * prefetches, when a test reloads or leaves the page while they are in
 * flight) as an unhandled rejection "/host:port/path due to access control
 * checks." A real failure of such a request shows up separately as an HTTP
 * error response or a CSP violation, both recorded by the fixture, so this
 * message alone is noise. Cross-origin URLs are never ignored.
 */
export function isCancelledSameOriginLoad(message: string, origin: string): boolean {
  const match = /^\/{1,2}([^/\s]+)(?:\/\S*)? due to access control checks\.$/.exec(
    message,
  );
  return match !== null && match[1] === new URL(origin).host;
}

export const test = base.extend<{ pageProblems: string[] }>({
  pageProblems: [
    async ({ page, baseURL }, use) => {
      const problems: string[] = [];
      const origin = new URL(baseURL ?? "http://127.0.0.1").origin;
      page.on("console", (message) => {
        if (message.type() !== "error") {
          return;
        }
        // Chromium logs every 4xx/5xx fetch as "Failed to load resource";
        // for /api/v1 requests that is the same fact the response listener
        // below records (or deliberately excepts) as "HTTP <status>: <path>",
        // so it is not counted twice. Any other console error still counts.
        if (/^Failed to load resource: the server responded with a status of/.test(message.text())) {
          const source = message.location()?.url ?? "";
          if (source.startsWith(origin) && new URL(source).pathname.startsWith("/api/v1/")) {
            return;
          }
        }
        problems.push(`console: ${message.text()}`);
      });
      page.on("pageerror", (error) => {
        if (!isCancelledSameOriginLoad(error.message, origin)) {
          problems.push(`page error: ${error.message}`);
        }
      });
      page.on("response", (response) => {
        const url = new URL(response.url());
        if (url.origin === origin && response.status() >= 400) {
          // The e2e static server has no API: its not-found page (HTML, 404)
          // for an /api/v1 path is the "API unreachable" condition the views
          // must handle (ApiGate shows NotConnected), not a broken link. A
          // mocked API error (a JSON body) still counts, and api.spec.ts
          // takes the ones it expects out of the list.
          const contentType = response.headers()["content-type"] ?? "";
          if (
            url.pathname.startsWith("/api/v1/") &&
            response.status() === 404 &&
            contentType.startsWith("text/html")
          ) {
            return;
          }
          problems.push(`HTTP ${response.status()}: ${url.pathname}`);
        }
      });
      page.on("requestfailed", (request) => {
        const url = new URL(request.url());
        // Navigations a test starts can abort the previous page's prefetches.
        if (url.origin !== origin) {
          problems.push(`request to another origin: ${url.href}`);
        }
      });
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (event) => {
          console.error(
            `CSP violation: ${event.violatedDirective} blocked ${event.blockedURI || "inline"}`,
          );
        });
      });
      await use(problems);
      expect(problems, "errors, failed requests or CSP violations").toEqual([]);
    },
    { auto: true },
  ],
});
