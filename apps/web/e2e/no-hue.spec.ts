// SPDX-License-Identifier: Apache-2.0
//
// The palette is fully monochromatic (owner direction 2026-10-31,
// design/palette-mono.md): no hue anywhere, R = G = B with a tolerance of 0.
// tests/no-hue.test.ts checks the theme output and the app's stylesheets;
// this spec checks what ships and what the browser paints:
//
// - the built CSS (every stylesheet of the export): a colour with a hue may
//   appear only as one of Astryx's own default tokens (a custom property in
//   @layer astryx-base) that the theme redeclares in grey, or in the one
//   Astryx class that sets a hue directly, its Badge test-violation fixture,
//   which no page renders, or as the var() fallback, in Astryx's reset or
//   components, of a token the theme defines (never painted while it is
//   set). Anything the theme or the app declares is grey.
// - the rendered pages, in both themes: every colour the browser computes
//   for every element and its ::before and ::after (text, fill, borders,
//   outline, decoration, shadows, gradients, caret, accent, SVG paint), and
//   the text selection, is grey.
// - links are underlined, and a button drawn as an <a> is not.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import type { Page } from "@playwright/test";

import { localePath } from "../src/lib/site";
import { declarations, hueDeclarations, hueLiterals, withoutDefinedFallbacks } from "../tests/colour-scan";

import { awaitingDecisionDetail, inProgressLiveRun, mockApi } from "./api-mock";
import { expect, hydrated, pagesFor, t, test, useTheme } from "./fixtures";
import { openRun } from "./helpers";

const OUT = "out";

function builtStylesheets(): string[] {
  return readdirSync(OUT, { recursive: true, encoding: "utf8" })
    .filter((file) => file.endsWith(".css"))
    .map((file) => path.join(OUT, file));
}

test.describe("the built CSS", () => {
  test("has no hue outside Astryx's own default tokens, all of which the theme redeclares in grey", () => {
    const sheets = builtStylesheets();
    expect(sheets.length).toBeGreaterThan(0);
    const css = sheets.map((sheet) => readFileSync(sheet, "utf8")).join("\n");
    // The properties the hyphae-mission theme declares, read from the build
    // (its @layer astryx-theme rules) rather than from the source.
    const themed = new Set(
      declarations(css)
        .filter(({ context }) => context.some((entry) => entry.replace(/\s+/g, " ") === "@layer astryx-theme"))
        .map(({ property }) => property),
    );
    expect(themed.has("--color-accent")).toBe(true);
    const offenders: string[] = [];
    let vendorTokens = 0;
    let vendorFallbacks = 0;
    for (const { declaration, hues } of hueDeclarations(css)) {
      const layer = declaration.context.find((entry) => entry.startsWith("@layer"))?.replace(/\s+/g, " ");
      const where = `${declaration.context.join(" > ")} ${declaration.selector} { ${declaration.property}: ${declaration.value} }`;
      if (layer === "@layer astryx-base" && declaration.property.startsWith("--") && themed.has(declaration.property)) {
        vendorTokens += 1;
        continue;
      }
      if (layer === "@layer astryx-base" && declaration.selector.startsWith(".xj8ax7g")) {
        continue;
      }
      // Astryx's reset and components may name a hue as the fallback of a
      // token the theme defines (`var(--color-text-secondary, #4e606f)`):
      // never painted while the token is set.
      const vendor = layer === "@layer astryx-base" || layer === "@layer reset";
      if (vendor && hueLiterals(withoutDefinedFallbacks(declaration.value, themed)).length === 0) {
        vendorFallbacks += 1;
        continue;
      }
      offenders.push(`${where} (${hues.join(", ")})`);
    }
    expect(offenders).toEqual([]);
    // The exemption is used: Astryx's base palette is in the bundle, overridden.
    expect(vendorTokens).toBeGreaterThan(100);
    expect(vendorFallbacks).toBeLessThan(20);
  });
});

/** Every non-grey colour the browser computes on the page, as "tag.class property: value". */
async function renderedHues(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const properties = [
      "color",
      "background-color",
      "background-image",
      "border-top-color",
      "border-right-color",
      "border-bottom-color",
      "border-left-color",
      "outline-color",
      "text-decoration-color",
      "text-emphasis-color",
      "column-rule-color",
      "caret-color",
      "accent-color",
      "fill",
      "stroke",
      "box-shadow",
      "text-shadow",
      "-webkit-text-fill-color",
      "-webkit-text-stroke-color",
    ];
    const grey = (value: string): boolean => {
      const functions = value.match(/[a-z-]+\([^()]*\)/gi) ?? [];
      for (const call of functions) {
        const name = call.slice(0, call.indexOf("(")).toLowerCase();
        const parts = call
          .slice(call.indexOf("(") + 1, -1)
          .split("/")[0]
          ?.split(/[\s,]+/)
          .filter(Boolean) ?? [];
        if (name === "rgb" || name === "rgba") {
          if (!(parts[0] === parts[1] && parts[1] === parts[2])) {
            return false;
          }
        } else if (name === "color") {
          if (!(parts[1] === parts[2] && parts[2] === parts[3])) {
            return false;
          }
        } else if (["oklch", "lch"].includes(name)) {
          if (Number.parseFloat(parts[1] ?? "1") !== 0) {
            return false;
          }
        } else if (["oklab", "lab"].includes(name)) {
          if (Number.parseFloat(parts[1] ?? "1") !== 0 || Number.parseFloat(parts[2] ?? "1") !== 0) {
            return false;
          }
        } else if (["hsl", "hsla", "hwb"].includes(name)) {
          return false;
        }
      }
      return true;
    };
    const found: string[] = [];
    const check = (element: Element, pseudo: string | null) => {
      const style = getComputedStyle(element, pseudo);
      if (pseudo !== null && (style.content === "none" || style.content === "normal")) {
        return;
      }
      for (const property of properties) {
        const value = style.getPropertyValue(property);
        if (value !== "" && !grey(value)) {
          found.push(`${element.tagName.toLowerCase()}.${String(element.className)}${pseudo ?? ""} ${property}: ${value}`);
        }
      }
    };
    for (const element of Array.from(document.querySelectorAll("*"))) {
      check(element, null);
      check(element, "::before");
      check(element, "::after");
      check(element, "::marker");
    }
    check(document.body, "::selection");
    // The one Astryx class that sets a hue directly is never on the page.
    if (document.querySelector(".xj8ax7g") !== null) {
      found.push(".xj8ax7g is rendered");
    }
    return found.slice(0, 40);
  });
}

for (const theme of ["light", "dark"] as const) {
  test.describe(`rendered colours are grey (${theme})`, () => {
    test.beforeEach(async ({ page }) => {
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    });

    test(`every page of the export (${theme})`, async ({ page }) => {
      test.setTimeout(300_000);
      await mockApi(page);
      for (const { path: pagePath } of pagesFor("en")) {
        await page.goto(pagePath);
        await hydrated(page);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        expect(await renderedHues(page), pagePath).toEqual([]);
      }
    });

    test(`a run awaiting a decision, every tab (${theme})`, async ({ page }) => {
      test.setTimeout(240_000);
      await mockApi(page, { detail: awaitingDecisionDetail() });
      await openRun(page, "en");
      await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
      expect(await renderedHues(page), "playback").toEqual([]);
      for (const tab of ["decision", "explorer", "evidence", "proofs", "audit", "meta"]) {
        const control = page.getByRole("tab", { name: t("en", `Run.tabs.${tab}`), exact: true });
        await control.click();
        await expect(control).toHaveAttribute("aria-selected", "true");
        expect(await renderedHues(page), tab).toEqual([]);
      }
    });

    test(`a live run with its LIVE tags, and the drawer and tour (${theme})`, async ({ page }) => {
      const run = inProgressLiveRun(5);
      await mockApi(page, { detail: run.detail, frames: run.frames });
      await openRun(page, "en");
      await expect(page.locator(".astryx-badge[data-variant='pink']").first()).toBeVisible();
      expect(await renderedHues(page)).toEqual([]);
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto(localePath("en", ""));
      await hydrated(page);
      await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
      await expect(page.getByRole("dialog", { name: t("en", "Shell.menuTitle") })).toBeVisible();
      expect(await renderedHues(page), "drawer").toEqual([]);
    });
  });
}

test.describe("links without hue", () => {
  test("every content and footer link is underlined; buttons drawn as links and navigation are not", async ({ page }) => {
    await mockApi(page, { detail: awaitingDecisionDetail() });
    const decorations = () =>
      page.evaluate(() =>
        Array.from(document.querySelectorAll("a[href]"))
          .filter((anchor) => anchor.getClientRects().length > 0)
          .map((anchor) => ({
            name: `${anchor.className} "${(anchor.textContent ?? "").trim().slice(0, 40)}"`,
            kind: anchor.matches(".astryx-button")
              ? "button"
              : anchor.matches(".hc-nav-link, .hc-wordmark, .hc-skip-link, .hc-locale-list a, .hc-card-link, .hc-language-list a")
                ? "navigation"
                : "link",
            underline: getComputedStyle(anchor).textDecorationLine.includes("underline"),
          })),
      );
    const seen = { link: 0, button: 0 };
    for (const target of [localePath("en", ""), localePath("en", "docs/modes"), localePath("en", "about"), localePath("en", "showcase")]) {
      await page.goto(target);
      await hydrated(page);
      for (const anchor of await decorations()) {
        if (anchor.kind === "link") {
          seen.link += 1;
          expect(anchor.underline, `link not underlined: ${anchor.name}`).toBe(true);
        } else if (anchor.kind === "button") {
          seen.button += 1;
          expect(anchor.underline, `button underlined: ${anchor.name}`).toBe(false);
        }
      }
    }
    expect(seen.link).toBeGreaterThan(5);
    expect(seen.button).toBeGreaterThan(0);
  });
});
