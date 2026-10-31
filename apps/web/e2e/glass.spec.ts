// SPDX-License-Identifier: Apache-2.0
//
// The brushed glass as the browser renders it (design/palette-mono.md
// section 3, src/app/hc-mono-glass.css), in light and dark:
// - a live backdrop filter only on the top bar and, on a narrow viewport, the
//   decision card; from 672 px the decision card is static glass; an open
//   overlay (the drawer, the tour) is a modal dialog in the top layer, the only
//   one open, with its own blur;
// - static glass (grain, opaque gradient, no backdrop) on the sidebar, the
//   cards of lists and the Settings panels, never nested in other glass, and a
//   card's own ::before (the Overview step number) is left alone;
// - tables and tags are solid: an opaque fill, no backdrop filter, no grain;
// - the grain is the theme's tile from /brand/ (the fixture fails on any
//   failed same-origin request, so a missing tile fails every test here);
// - the primary button: ink gradient with the grain and a highlight; pressed,
//   flat btn/hover-bottom without shadow;
// - the fallbacks: more contrast, forced colours and print drop the backdrop
//   filter (reduced transparency: motion.spec.ts); in forced colours the
//   overlays keep a real 1px border (the tour a frame, the drawer its
//   inline-end edge) and the top bar only its bottom line;
// - a clickable card hovers with its edge at border/strong; disabled-colour
//   text on live glass is the secondary ink;
// - axe finds no violation on the decision tab, at both widths, in both themes.

import AxeBuilder from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";

import { mockApi, storedRun } from "./api-mock";
import { AXE_TAGS, type ScanSummary, summarize, writeScans } from "./axe-results";
import { fixture } from "./contract";
import { expect, hydrated, t, test, useTheme } from "./fixtures";
import { openRun } from "./helpers";

type Json = Record<string, unknown>;

const THEMES = ["light", "dark"] as const;
const NARROW = { width: 390, height: 844 };
const WIDE = { width: 1440, height: 900 };

/**
 * The visible elements whose backdrop filter is not none, sorted, each named
 * by the first of its classes that says what it is (or its first class, or
 * its tag).
 */
async function liveGlass(page: Page): Promise<string[]> {
  const names = await page.evaluate(() => {
    const known = ["hc-topbar", "hc-decision-card", "hc-dialog", "hc-sidebar"];
    return [...document.querySelectorAll<HTMLElement>("body *")]
      .filter((element) => {
        const style = getComputedStyle(element);
        const filter = style.backdropFilter || style.getPropertyValue("-webkit-backdrop-filter");
        return filter !== "" && filter !== "none" && element.checkVisibility();
      })
      .map((element) => {
        const name = known.find((candidate) => element.classList.contains(candidate)) ?? element.classList[0];
        return name === undefined ? element.tagName.toLowerCase() : `.${name}`;
      });
  });
  return names.sort();
}

type Surface = { backdrop: string; image: string; color: string; shadow: string };

async function surfaceOf(locator: Locator): Promise<Surface> {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      backdrop: style.backdropFilter,
      image: style.backgroundImage,
      color: style.backgroundColor,
      shadow: style.boxShadow,
    };
  });
}

/** An opaque colour as getComputedStyle writes it (rgb(), or rgba()/color() with alpha 1). */
const OPAQUE = /^rgb\(|^rgba\([^)]*,\s*1\)$|^color\(srgb [^/]*\)$/;

/** Live glass: the theme's grain tile, the sheen and the fill, over a greyed blur. */
function expectLive(surface: Surface, theme: "light" | "dark", blur: number) {
  expect(surface.backdrop).toBe(`blur(${blur}px) grayscale(1)`);
  expect(surface.image).toContain(`/brand/brushed-${theme}.png`);
  expect(surface.image).toContain("linear-gradient(105deg");
  expect(surface.shadow).toContain("inset");
}

/** Static glass: the grain and the opaque gradient, no backdrop filter, no sheen. */
function expectStatic(surface: Surface, theme: "light" | "dark") {
  expect(surface.backdrop).toBe("none");
  expect(surface.image).toContain(`/brand/brushed-${theme}.png`);
  // The fill and the border-box edge (a top-to-bottom gradient is written without its 180deg).
  expect(surface.image.match(/linear-gradient\(rgb/g)?.length).toBe(2);
  expect(surface.image).not.toContain("linear-gradient(105deg");
}

/** Every visible glass element (dialogs aside: they render in the top layer) has no glass ancestor. */
async function nestedGlass(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>(".hc-glass")]
      .filter((element) => element.checkVisibility() && element.tagName !== "DIALOG")
      .filter((element) => element.parentElement?.closest(".hc-glass") !== null)
      .map((element) => element.className),
  );
}

/** An awaiting-decision run (the abstained fixture), on its Decision tab. */
async function openDecision(page: Page) {
  const detail = fixture<Json>("run-detail-abstain.json");
  await mockApi(page, { extraRuns: [storedRun(detail)] });
  await openRun(page, "en", String((detail.summary as Json).run_id));
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision"), exact: true }).click();
  await expect(page.locator(".hc-decision-card")).toBeVisible();
}

async function openTab(page: Page, name: string) {
  await page.getByRole("tab", { name: t("en", `Run.tabs.${name}`), exact: true }).click();
  await expect(page.locator("#run-tabpanel")).toHaveAttribute("data-tab", name);
}

/** Tags and tables: an opaque fill, no backdrop filter, no grain or gradient. */
async function expectSolid(locator: Locator, what: string) {
  const count = await locator.count();
  expect(count, what).toBeGreaterThan(0);
  for (let index = 0; index < count; index += 1) {
    const element = locator.nth(index);
    if (!(await element.isVisible())) {
      continue;
    }
    const surface = await surfaceOf(element);
    expect(surface.backdrop, `${what} ${index}`).toBe("none");
    expect(surface.color, `${what} ${index}`).toMatch(OPAQUE);
    expect(surface.image, `${what} ${index}`).not.toContain("brushed-");
    expect(surface.image, `${what} ${index}`).not.toContain("linear-gradient");
  }
}

for (const theme of THEMES) {
  test.describe(`${theme} theme`, () => {
    test.beforeEach(async ({ page }) => {
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    });

    test(`a phone: live glass on the top bar and the decision card only (${theme})`, async ({ page }) => {
      await page.setViewportSize(NARROW);
      await openDecision(page);
      expect(await liveGlass(page)).toEqual([".hc-decision-card", ".hc-topbar"]);
      await expect(page.locator(".hc-decision-card")).toHaveCount(1);
      expectLive(await surfaceOf(page.locator(".hc-topbar")), theme, 16);
      const card = await surfaceOf(page.locator(".hc-decision-card"));
      expectLive(card, theme, 24);
      // Its 2px ink border, its own emphasis, stays.
      await expect(page.locator(".hc-decision-card")).toHaveCSS("border-top-width", "2px");
      expect(await nestedGlass(page)).toEqual([]);
    });

    test(`a wide screen: the top bar is the only live glass; the decision card and the sidebar are static (${theme})`, async ({
      page,
    }) => {
      await page.setViewportSize(WIDE);
      await openDecision(page);
      expect(await liveGlass(page)).toEqual([".hc-topbar"]);
      expectLive(await surfaceOf(page.locator(".hc-topbar")), theme, 16);
      expectStatic(await surfaceOf(page.locator(".hc-decision-card")), theme);
      expectStatic(await surfaceOf(page.locator(".hc-sidebar")), theme);
      expectStatic(await surfaceOf(page.locator(".hc-run-header")), theme);
      await expect(page.locator(".hc-decision-card")).toHaveCSS("border-top-width", "2px");
      expect(await nestedGlass(page)).toEqual([]);
    });

    test(`tables and tags are solid, inside and outside the glass (${theme})`, async ({ page }) => {
      await page.setViewportSize(NARROW);
      await openDecision(page);
      for (const viewport of [NARROW, WIDE]) {
        await page.setViewportSize(viewport);
        await openTab(page, "decision");
        await expectSolid(page.locator(".astryx-badge"), `tag on Decision at ${viewport.width}`);
        await openTab(page, "audit");
        await expectSolid(page.locator("table.hc-timeline"), `table on Audit trail at ${viewport.width}`);
        // Nothing under a table blurs it.
        const blurredAncestor = await page.locator("table.hc-timeline").first().evaluate((element) => {
          for (let node = element.parentElement; node !== null; node = node.parentElement) {
            if (getComputedStyle(node).backdropFilter !== "none") return node.className;
          }
          return null;
        });
        expect(blurredAncestor).toBeNull();
        await expectSolid(page.locator(".astryx-badge"), `tag on Audit trail at ${viewport.width}`);
        await openTab(page, "meta");
        await expectSolid(page.locator("table.hc-timeline"), `table on Meta API at ${viewport.width}`);
      }
    });

    test(`static glass on the cards of lists and the Settings panels; the step number stays (${theme})`, async ({ page }) => {
      await page.setViewportSize(WIDE);
      await page.goto("/en/");
      await hydrated(page);
      expect(await liveGlass(page)).toEqual([".hc-topbar"]);
      const steps = page.locator(".hc-flow-step");
      expect(await steps.count()).toBeGreaterThan(0);
      for (let index = 0; index < (await steps.count()); index += 1) {
        expectStatic(await surfaceOf(steps.nth(index)), theme);
        // The step's own ::before (its number) is not the glass's edge ring.
        const marker = await steps.nth(index).evaluate((element) => {
          const style = getComputedStyle(element, "::before");
          return { display: style.display, content: style.content, mask: style.maskImage, position: style.position };
        });
        expect(marker.display).not.toBe("none");
        expect(marker.content).toContain("counter");
        expect(marker.mask).toBe("none");
        expect(marker.position).not.toBe("absolute");
      }
      expect(await nestedGlass(page)).toEqual([]);

      await page.goto("/en/settings/");
      await hydrated(page);
      expect(await liveGlass(page)).toEqual([".hc-topbar"]);
      const panels = page.locator(".hc-settings .hc-panel");
      expect(await panels.count()).toBe(5);
      for (let index = 0; index < 5; index += 1) {
        expectStatic(await surfaceOf(panels.nth(index)), theme);
        // Each panel keeps its 3:1 boundary over the glass edge (WCAG 1.4.11).
        await expect(panels.nth(index)).toHaveCSS("border-top-style", "solid");
        expect(await panels.nth(index).evaluate((element) => getComputedStyle(element).borderTopColor)).toBe(
          theme === "light" ? "rgb(110, 110, 110)" : "rgb(158, 158, 158)",
        );
      }
      expect(await nestedGlass(page)).toEqual([]);

      await page.goto("/en/docs/");
      await hydrated(page);
      const cards = page.locator(".hc-card");
      expect(await cards.count()).toBeGreaterThan(0);
      const card = await surfaceOf(cards.first());
      expectStatic(card, theme);
      // A clickable card carries the top highlight.
      expect(card.shadow).toContain("inset");
      expect(await nestedGlass(page)).toEqual([]);
    });

    test(`an overlay: one modal dialog in the top layer, live glass level 3 (${theme})`, async ({ page }) => {
      // The drawer, on a phone: level 1 blur under 672 px (palette-mono 3.7).
      await page.setViewportSize(NARROW);
      await page.goto("/en/");
      await hydrated(page);
      await page.getByRole("button", { name: t("en", "Shell.menuButton") }).click();
      const drawer = page.locator("dialog[open]");
      await expect(drawer).toHaveCSS("opacity", "1");
      await expect(page.locator("dialog[open]")).toHaveCount(1);
      expect(await drawer.evaluate((element) => element.matches(":modal"))).toBe(true);
      expectLive(await surfaceOf(drawer), theme, 16);
      expect(await drawer.evaluate((element) => getComputedStyle(element).position)).toBe("fixed");
      expect(await liveGlass(page)).toEqual([".hc-dialog", ".hc-topbar"]);
      await page.keyboard.press("Escape");
      await expect(page.locator("dialog[open]")).toHaveCount(0);

      // The tour, on a wide screen: level 3, 24 px.
      await page.setViewportSize(WIDE);
      await page.goto("/en/docs/");
      await hydrated(page);
      await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
      const tour = page.locator("dialog[open]");
      await expect(tour).toHaveCSS("opacity", "1");
      await expect(page.locator("dialog[open]")).toHaveCount(1);
      expect(await tour.evaluate((element) => element.matches(":modal"))).toBe(true);
      expectLive(await surfaceOf(tour), theme, 24);
      const scrim = await tour.evaluate((element) => getComputedStyle(element, "::backdrop").backgroundColor);
      expect(scrim).toBe(theme === "light" ? "rgba(11, 11, 11, 0.55)" : "rgba(0, 0, 0, 0.65)");
    });

    test(`the primary button: ink gradient, grain and highlight; pressed, flat and without shadow (${theme})`, async ({
      page,
    }) => {
      await page.setViewportSize(WIDE);
      await mockApi(page);
      await page.goto("/en/");
      await hydrated(page);
      // The health answer changes the Overview's lines above the button: wait for it, so the button stays put.
      await expect(page.locator("[data-api-status='connected']")).toBeVisible();
      const button = page.locator(".astryx-button[data-variant='primary']").first();
      await expect(button).toBeVisible();
      const rest = await surfaceOf(button);
      expect(rest.image).toContain("/brand/brushed-light.png");
      // btn/top to btn/bottom (a top-to-bottom gradient is written without its 180deg).
      expect(rest.image).toContain(
        theme === "light"
          ? "linear-gradient(rgb(58, 58, 58) 0%, rgb(11, 11, 11) 100%)"
          : "linear-gradient(rgb(255, 255, 255) 0%, rgb(194, 194, 194) 100%)",
      );
      expect(rest.shadow).toContain("inset");
      expect(rest.color).toMatch(OPAQUE);

      await button.hover();
      const box = await button.boundingBox();
      expect(box).not.toBeNull();
      const { x, y, width, height } = box as { x: number; y: number; width: number; height: number };
      await page.mouse.move(x + width / 2, y + height / 2);
      // Hovered first (the hover gradient), then pressed.
      await expect
        .poll(async () => (await surfaceOf(button)).image)
        .toContain(
          theme === "light"
            ? "linear-gradient(rgb(87, 87, 87) 0%, rgb(19, 19, 19) 100%)"
            : "linear-gradient(rgb(255, 255, 255) 0%, rgb(220, 220, 220) 100%)",
        );
      await page.mouse.down();
      // :active applies with the next style update after the press.
      await expect
        .poll(async () => {
          const pressed = await surfaceOf(button);
          return [pressed.image, pressed.shadow, pressed.color];
        })
        .toEqual(["none", "none", theme === "light" ? "rgb(19, 19, 19)" : "rgb(220, 220, 220)"]);
      // Moving off before releasing, so the press is no click.
      await page.mouse.move(x + width / 2, y + height + 200);
      await page.mouse.up();
    });

    test(`more contrast, forced colours and print drop the backdrop filter (${theme})`, async ({ page }) => {
      await page.setViewportSize(NARROW);
      await openDecision(page);
      const both = [".hc-decision-card", ".hc-topbar"];
      // A media change reaches the page's style with its next update, so each check waits for it.
      await expect.poll(() => liveGlass(page)).toEqual(both);

      await page.emulateMedia({ contrast: "more" });
      await expect.poll(() => liveGlass(page)).toEqual([]);
      const contrast = await surfaceOf(page.locator(".hc-topbar"));
      expect(contrast.image).toBe("none");
      expect(contrast.color).toMatch(OPAQUE);
      await page.emulateMedia({ contrast: "no-preference" });
      await expect.poll(() => liveGlass(page)).toEqual(both);

      await page.emulateMedia({ forcedColors: "active" });
      await expect.poll(() => liveGlass(page)).toEqual([]);
      expect((await surfaceOf(page.locator(".hc-decision-card"))).shadow).toBe("none");
      await page.emulateMedia({ forcedColors: "none" });
      await expect.poll(() => liveGlass(page)).toEqual(both);

      await page.emulateMedia({ media: "print" });
      await expect.poll(() => liveGlass(page)).toEqual([]);
      expect((await surfaceOf(page.locator(".hc-topbar"))).shadow).toBe("none");
      await page.emulateMedia({ media: "screen" });
      await expect.poll(() => liveGlass(page)).toEqual(both);
    });

    test(`forced colours: the overlays keep a real 1px border; the top bar only its bottom line (${theme})`, async ({ page }) => {
      await page.emulateMedia({ forcedColors: "active" });
      const edges = (locator: Locator) =>
        locator.evaluate((element) => {
          const style = getComputedStyle(element);
          return [style.borderTop, style.borderRight, style.borderBottom, style.borderLeft].map((edge) =>
            edge.split(" ").slice(0, 2).join(" "),
          );
        });

      // The drawer, on a phone: its inline-end edge (the one over the page), as before the glass.
      await page.setViewportSize(NARROW);
      await page.goto("/en/");
      await hydrated(page);
      expect(await edges(page.locator(".hc-topbar"))).toEqual(["0px solid", "0px solid", "1px solid", "0px solid"]);
      await page.getByRole("button", { name: t("en", "Shell.menuButton") }).click();
      const drawer = page.locator("dialog[open]");
      await expect(drawer).toHaveCSS("opacity", "1");
      expect(await edges(drawer)).toEqual(["0px none", "1px solid", "0px none", "0px none"]);
      expect((await surfaceOf(drawer)).shadow).toBe("none");
      await page.keyboard.press("Escape");
      await expect(page.locator("dialog[open]")).toHaveCount(0);

      // The tour, on a wide screen: a full 1px frame.
      await page.setViewportSize(WIDE);
      await page.goto("/en/docs/");
      await hydrated(page);
      await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
      const tour = page.locator("dialog[open]");
      await expect(tour).toHaveCSS("opacity", "1");
      expect(await edges(tour)).toEqual(["1px solid", "1px solid", "1px solid", "1px solid"]);
      expect((await surfaceOf(tour)).shadow).toBe("none");
      expect(await edges(page.locator(".hc-topbar"))).toEqual(["0px solid", "0px solid", "1px solid", "0px solid"]);
    });

    test(`a clickable card hovers to bg/hover with its edge at border/strong (${theme})`, async ({ page }) => {
      await page.setViewportSize(WIDE);
      await page.goto("/en/docs/");
      await hydrated(page);
      const card = page.locator(".hc-card").first();
      await card.hover();
      const strong = theme === "light" ? "rgb(110, 110, 110)" : "rgb(158, 158, 158)";
      await expect
        .poll(async () => (await surfaceOf(card)).image)
        .toContain(`linear-gradient(${strong}, ${strong})`);
      // No border colour of its own over the border-box edge (the ink accent would hide border/strong).
      expect(await card.evaluate((element) => getComputedStyle(element).borderTopColor)).toBe("rgba(0, 0, 0, 0)");
      await expect(card).toHaveCSS("border-top-width", "1px");
    });

    test(`muted (disabled-colour) text on live glass is the secondary ink (${theme})`, async ({ page }) => {
      await page.setViewportSize(NARROW);
      await openDecision(page);
      const secondary = theme === "light" ? "rgb(58, 58, 58)" : "rgb(194, 194, 194)";
      for (const host of [".hc-topbar", ".hc-decision-card"]) {
        // A test-only probe: Astryx's disabled Text, as a component inside the glass would render it.
        const colour = await page.locator(host).evaluate((element) => {
          const probe = document.createElement("span");
          probe.className = "astryx-text";
          probe.dataset.color = "disabled";
          probe.textContent = "probe";
          element.append(probe);
          const value = getComputedStyle(probe).color;
          probe.remove();
          return value;
        });
        expect(colour, host).toBe(secondary);
      }
    });

    test(`axe: no violation on the decision tab, on a phone and a wide screen (${theme})`, async ({ page }) => {
      const scans: ScanSummary[] = [];
      await page.setViewportSize(NARROW);
      await openDecision(page);
      for (const viewport of [NARROW, WIDE]) {
        await page.setViewportSize(viewport);
        await expect(page.locator(".hc-decision-card")).toBeVisible();
        const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
        scans.push(summarize(`glass decision ${theme} ${viewport.width}`, results));
      }
      await writeScans(`glass-decision-${theme}`, scans);
      expect(scans.flatMap((scan) => scan.violations)).toEqual([]);
    });
  });
}
