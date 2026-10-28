// SPDX-License-Identifier: Apache-2.0
//
// No dead navigation at the source level: every entry of the site map in
// src/lib/site.ts has a route file, and the helpers build the paths the
// static export serves (trailing slash). The e2e crawler checks the same on
// the exported pages.

import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import {
  docPath,
  docSlugs,
  externalLinks,
  footerNav,
  legalPages,
  localePath,
  navMatch,
  primaryNav,
  SOURCE_REPOSITORY_URL,
} from "../src/lib/site";

const appDir = fileURLToPath(new URL("../src/app/[locale]", import.meta.url));

function routeFile(relative: string): string {
  return path.join(appDir, relative, "page.tsx");
}

describe("site map", () => {
  it.each([...primaryNav, ...footerNav])(
    "$key has a route file",
    ({ path: route }) => {
      expect(existsSync(routeFile(route)), route).toBe(true);
    },
  );

  it("renders every legal page from a footer entry", () => {
    const footerPaths = footerNav.map((entry) => entry.path as string);
    for (const page of legalPages) {
      expect(footerPaths).toContain(page);
    }
  });

  it("has a docs article route and URL-safe slugs", () => {
    expect(existsSync(routeFile("docs/[slug]"))).toBe(true);
    expect(new Set(docSlugs).size).toBe(docSlugs.length);
    for (const slug of docSlugs) {
      expect(slug).toMatch(/^[a-z]+(-[a-z]+)*$/);
    }
  });

  it("links only to HTTPS external pages, and to no repository until it is set", () => {
    for (const url of Object.values(externalLinks)) {
      expect(url).toMatch(/^https:\/\/[a-z0-9.-]+\//);
    }
    if (SOURCE_REPOSITORY_URL !== null) {
      expect(SOURCE_REPOSITORY_URL).toMatch(/^https:\/\//);
    }
  });
});

describe("localePath and docPath", () => {
  it("build the exported paths, with a trailing slash", () => {
    expect(localePath("en", "")).toBe("/en/");
    expect(localePath("pt-BR", "settings")).toBe("/pt-BR/settings/");
    expect(localePath("fr", "/docs/")).toBe("/fr/docs/");
    expect(docPath("es", "modes")).toBe("/es/docs/modes/");
  });
});

describe("navMatch", () => {
  it("marks the current page, with or without the trailing slash", () => {
    expect(navMatch("/en/", "en", "")).toBe("page");
    expect(navMatch("/en", "en", "")).toBe("page");
    expect(navMatch("/en/docs/", "en", "docs")).toBe("page");
    expect(navMatch("/en/docs", "en", "docs")).toBe("page");
  });

  it("marks a section for pages below it, never for Overview", () => {
    expect(navMatch("/en/docs/modes/", "en", "docs")).toBe("section");
    expect(navMatch("/en/docs/modes/", "en", "")).toBeNull();
  });

  it("does not match other pages, prefixes or locales", () => {
    expect(navMatch("/en/runs/", "en", "new")).toBeNull();
    expect(navMatch("/en/docsx/", "en", "docs")).toBeNull();
    expect(navMatch("/es/docs/", "en", "docs")).toBeNull();
    expect(navMatch(null, "en", "docs")).toBeNull();
  });

  it("covers every locale", () => {
    for (const locale of routing.locales) {
      expect(navMatch(localePath(locale, "settings"), locale, "settings")).toBe(
        "page",
      );
    }
  });
});
