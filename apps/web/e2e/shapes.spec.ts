// SPDX-License-Identifier: Apache-2.0
//
// Square-first shapes (design-direction.md 3.7 and 4.2, owner direction
// 2026-10-31): every corner the app ships or the browser draws is 0, 2, 4
// or 6 px; no pill, oval or circle outside the reasoned allow-lists of
// tests/radius-scan.ts (tags belong to the tags lane).
//
// - the built CSS: off the scale only in allow-listed Astryx classes.
// - the rendered pages: every element and its ::before / ::after computes
//   corners on the scale, on every page of the export, a run's tabs, the
//   live form, Settings, the drawer and the tour.
// - the shapes this change draws: the theme control (outer 4, inner joins
//   0), the 24px square Overview step markers (radius 4), the tour's
//   segmented progress bar (24x4 segments, radius 0, the current one 40
//   wide), square-cornered radio marks (radius 2, still native radios) and
//   the navigation rows (radius 0 on every corner).
// - the theme control as a keyboard radio group (one tab stop, arrows move
//   and select, Space selects), keeping the stored preference semantics,
//   and axe with no violation of any impact on it.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";

import { localePath } from "../src/lib/site";
import { allowedFinding, pageTokens, RADIUS_SCALE, radiusFindings, RENDERED_EXCEPTIONS } from "../tests/radius-scan";

import { awaitingDecisionDetail, mockApi } from "./api-mock";
import { AXE_TAGS } from "./axe-results";
import { expect, focusInOpenDialog, hydrated, type Locale, pagesFor, storedKeys, t, test, useTheme } from "./fixtures";
import { openRun } from "./helpers";

const OUT = "out";

test.describe("the built CSS", () => {
  test("keeps every corner on the 0/2/4/6 scale outside the allow-listed Astryx classes", () => {
    const sheets = readdirSync(OUT, { recursive: true, encoding: "utf8" })
      .filter((file) => file.endsWith(".css"))
      .map((file) => path.join(OUT, file));
    expect(sheets.length).toBeGreaterThan(0);
    const css = sheets.map((sheet) => readFileSync(sheet, "utf8")).join("\n");
    const { scanned, findings } = radiusFindings(css, pageTokens(css));
    expect(scanned).toBeGreaterThan(100);
    const offenders = findings
      .filter(({ declaration }) => !allowedFinding(declaration))
      .map(({ declaration, values }) => `${declaration.selector} { ${declaration.property}: ${declaration.value} } -> ${values.join(", ")}`);
    expect(offenders).toEqual([]);
    // The app's own rules are in the bundle and on the scale.
    expect(css).toContain(".hc-segmented");
    expect(css).toContain(".hc-tour-bar");
  });
});

/** Every rendered corner off the scale, as "tag.class::pseudo corner: value". */
async function offScaleCorners(page: Page): Promise<string[]> {
  return page.evaluate(
    ({ scale, exceptions }) => {
      const corners = [
        "border-top-left-radius",
        "border-top-right-radius",
        "border-bottom-right-radius",
        "border-bottom-left-radius",
      ];
      const onScale = (value: string): boolean =>
        value
          .split(/\s+/)
          .filter(Boolean)
          .every((part) => {
            const match = /^(-?[\d.]+)(px|%)$/.exec(part);
            if (match === null) {
              return false;
            }
            const number = Number.parseFloat(match[1] ?? "NaN");
            return match[2] === "px" ? scale.includes(number) : number === 0;
          });
      const found: string[] = [];
      for (const element of Array.from(document.querySelectorAll("*"))) {
        if (exceptions.some((selector) => element.matches(selector))) {
          continue;
        }
        for (const pseudo of [null, "::before", "::after"]) {
          const style = getComputedStyle(element, pseudo);
          if (pseudo !== null && (style.content === "none" || style.content === "normal")) {
            continue;
          }
          for (const corner of corners) {
            const value = style.getPropertyValue(corner);
            if (!onScale(value)) {
              found.push(`${element.tagName.toLowerCase()}.${String(element.className)}${pseudo ?? ""} ${corner}: ${value}`);
            }
          }
        }
      }
      return found.slice(0, 40);
    },
    { scale: [...RADIUS_SCALE], exceptions: Object.keys(RENDERED_EXCEPTIONS) },
  );
}

test.describe("rendered corners are on the scale", () => {
  test.beforeEach(async ({ page }) => {
    await useTheme(page, "light");
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  });

  test("every page of the export", async ({ page }) => {
    test.setTimeout(300_000);
    await mockApi(page);
    for (const { path: pagePath } of pagesFor("en")) {
      await page.goto(pagePath);
      await hydrated(page);
      expect(await offScaleCorners(page), pagePath).toEqual([]);
    }
  });

  test("a run awaiting a decision, every tab", async ({ page }) => {
    test.setTimeout(240_000);
    await mockApi(page, { detail: awaitingDecisionDetail() });
    await openRun(page, "en");
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    expect(await offScaleCorners(page), "playback").toEqual([]);
    for (const tab of ["decision", "explorer", "evidence", "proofs", "audit", "meta"]) {
      const control = page.getByRole("tab", { name: t("en", `Run.tabs.${tab}`), exact: true });
      await control.click();
      await expect(control).toHaveAttribute("aria-selected", "true");
      expect(await offScaleCorners(page), tab).toEqual([]);
    }
  });

  test("the live form, the tour and the drawer", async ({ page }) => {
    await mockApi(page, { liveEnabled: true, liveRequiresTurnstile: false });
    await page.goto(localePath("en", "new"));
    await hydrated(page);
    await expect(page.locator("[data-live='form'][data-live-state='open']")).toBeVisible();
    expect(await offScaleCorners(page), "live form").toEqual([]);

    await page.goto(localePath("en", "docs"));
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
    await expect(page.locator("dialog[open]")).toBeVisible();
    expect(await offScaleCorners(page), "tour").toEqual([]);
    await page.keyboard.press("Escape");

    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(localePath("en", ""));
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
    await expect(page.getByRole("dialog", { name: t("en", "Shell.menuTitle") })).toBeVisible();
    expect(await offScaleCorners(page), "drawer").toEqual([]);
  });
});

type Corners = [string, string, string, string];

/** The four computed corners (top-left, top-right, bottom-right, bottom-left). */
async function corners(locator: Locator, pseudo: string | null = null): Promise<Corners> {
  return locator.evaluate((element, which) => {
    const style = getComputedStyle(element, which);
    return [
      style.borderTopLeftRadius,
      style.borderTopRightRadius,
      style.borderBottomRightRadius,
      style.borderBottomLeftRadius,
    ] as Corners;
  }, pseudo);
}

test.describe("the shapes of this change", () => {
  test.beforeEach(async ({ page }) => {
    await useTheme(page, "light");
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  });

  test("the theme control: outer radius 4, inner joins 0, no Switch", async ({ page }) => {
    await page.goto(localePath("en", ""));
    await hydrated(page);
    const control = page.locator(".hc-topbar-controls [data-theme-control]");
    await expect(control).toBeVisible();
    await expect(page.locator(".astryx-switch")).toHaveCount(0);
    await expect(page.getByRole("switch")).toHaveCount(0);
    expect(await corners(control)).toEqual(["4px", "4px", "4px", "4px"]);
    const segments = control.locator(".hc-segment");
    await expect(segments).toHaveCount(3);
    // Start corners on the first segment, end corners on the last, 0 at
    // every join (left-to-right document).
    expect(await corners(segments.nth(0))).toEqual(["2px", "0px", "0px", "2px"]);
    expect(await corners(segments.nth(1))).toEqual(["0px", "0px", "0px", "0px"]);
    expect(await corners(segments.nth(2))).toEqual(["0px", "2px", "2px", "0px"]);
    // The selected segment carries the ink material and the check; the
    // others neither.
    const selected = control.locator(".hc-segment:has(input:checked)");
    await expect(selected).toHaveCount(1);
    await expect(selected.locator(".hc-segment-check")).toBeVisible();
    await expect(control.locator(".hc-segment:not(:has(input:checked)) .hc-segment-check")).toHaveCount(2);
    for (const check of await control.locator(".hc-segment:not(:has(input:checked)) .hc-segment-check").all()) {
      await expect(check).toBeHidden();
    }
    expect(await selected.evaluate((element) => getComputedStyle(element).backgroundImage)).toContain("linear-gradient");
  });

  test("the Overview step markers are 24px squares with a 4px radius", async ({ page }) => {
    await page.goto(localePath("en", ""));
    await hydrated(page);
    const steps = page.locator(".hc-flow-step");
    await expect(steps).toHaveCount(4);
    for (const step of await steps.all()) {
      const marker = await step.evaluate((element) => {
        const style = getComputedStyle(element, "::before");
        return { width: style.width, height: style.height };
      });
      expect(marker).toEqual({ width: "24px", height: "24px" });
      expect(await corners(step, "::before")).toEqual(["4px", "4px", "4px", "4px"]);
    }
  });

  test("the tour's progress is a segmented bar: 24x4 segments, radius 0, the current one 40 wide", async ({ page }) => {
    await page.goto(localePath("en", "docs"));
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
    const dialog = page.locator("dialog[open]");
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: t("en", "Tour.next") }).click();
    const segments = dialog.locator(".hc-tour-bar li");
    await expect(segments).toHaveCount(6);
    await expect(dialog.locator(".hc-tour-bar")).toHaveAttribute("aria-hidden", "true");
    // Let the step change settle before the one-shot read below (a loaded
    // machine once read the first segment still 40 wide right after Next).
    await expect(segments.nth(1)).toHaveAttribute("data-state", "current");
    await expect(segments.nth(0)).toHaveCSS("width", "24px");
    await expect(segments.nth(1)).toHaveCSS("width", "40px");
    const drawn = await segments.evaluateAll((items) =>
      items.map((item) => {
        const style = getComputedStyle(item);
        return {
          state: item.getAttribute("data-state"),
          width: style.width,
          height: style.height,
          radius: style.borderRadius,
          filled: style.backgroundColor !== "rgba(0, 0, 0, 0)",
          frame: style.borderTopStyle,
        };
      }),
    );
    expect(drawn.map(({ state }) => state)).toEqual(["done", "current", "todo", "todo", "todo", "todo"]);
    for (const segment of drawn) {
      expect(segment.height).toBe("4px");
      expect(segment.radius).toBe("0px");
      expect(segment.frame).toBe("solid");
      expect(segment.width).toBe(segment.state === "current" ? "40px" : "24px");
      // Done and current are filled; upcoming segments are an empty frame.
      expect(segment.filled).toBe(segment.state !== "todo");
    }
  });

  test("radio marks are square-cornered (radius 2) and still native radios", async ({ page }) => {
    await page.goto(localePath("en", "settings"));
    await hydrated(page);
    const group = page.getByRole("radiogroup", { name: t("en", "Settings.motion.label") });
    const radios = group.getByRole("radio");
    await expect(radios).toHaveCount(2);
    for (const radio of await radios.all()) {
      expect(await radio.evaluate((input) => input instanceof HTMLInputElement && input.type === "radio")).toBe(true);
    }
    const marks = page.locator(".hc-settings .astryx-radio-indicator");
    expect(await marks.count()).toBeGreaterThan(5);
    for (const mark of await marks.all()) {
      expect(await corners(mark)).toEqual(["2px", "2px", "2px", "2px"]);
    }
    // The selected dot inside a mark is square-cornered too.
    const dots = page.locator(".hc-settings .astryx-radio-indicator > *");
    expect(await dots.count()).toBeGreaterThan(0);
    for (const dot of await dots.all()) {
      expect(await corners(dot)).toEqual(["2px", "2px", "2px", "2px"]);
    }
    // Arrow keys still move the selection, as in any native radio group.
    await group.getByRole("radio", { name: t("en", "Settings.motion.options.system") }).focus();
    await page.keyboard.press("ArrowDown");
    await expect(group.getByRole("radio", { name: t("en", "Settings.motion.options.reduce") })).toBeChecked();
  });

  test("navigation rows have square corners on every side", async ({ page }) => {
    await page.goto(localePath("en", "docs"));
    await hydrated(page);
    const links = page.locator(".hc-sidebar .hc-nav-link");
    expect(await links.count()).toBeGreaterThan(3);
    for (const link of await links.all()) {
      expect(await corners(link)).toEqual(["0px", "0px", "0px", "0px"]);
    }
  });
});

/** The theme control's radios, by option. */
function themeRadio(scope: Locator | Page, locale: Locale, option: "system" | "light" | "dark") {
  return scope.getByRole("radio", { name: t(locale, `Theme.options.${option}`), exact: true });
}

async function tabInto(page: Page, selector: string): Promise<void> {
  for (let presses = 0; presses < 30; presses += 1) {
    await page.keyboard.press("Tab");
    if (await page.evaluate((target) => document.activeElement?.matches(target) ?? false, selector)) {
      return;
    }
  }
  throw new Error(`Tab never reached ${selector}`);
}

test.describe("the theme control is a keyboard radio group", () => {
  test("one tab stop on the selected segment; arrows move and select; the choice is stored", async ({
    page,
    context,
  }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.goto(localePath("en", ""));
    await hydrated(page);
    const html = page.locator("html");
    const group = page.locator(".hc-topbar-controls").getByRole("radiogroup", { name: t("en", "Theme.label") });
    await expect(group).toBeVisible();
    await expect(themeRadio(group, "en", "system")).toBeChecked();

    // Tab lands on the selected segment, which shows the focus ring.
    await tabInto(page, ".hc-topbar-controls [data-theme-control] input");
    await expect(themeRadio(group, "en", "system")).toBeFocused();
    const ring = await group
      .locator(".hc-segment[data-option='system']")
      .evaluate((element) => ({ style: getComputedStyle(element).outlineStyle, width: getComputedStyle(element).outlineWidth }));
    expect(ring.style).not.toBe("none");
    expect(Number.parseFloat(ring.width)).toBeGreaterThanOrEqual(2);

    await page.keyboard.press("ArrowRight");
    await expect(themeRadio(group, "en", "light")).toBeFocused();
    await expect(themeRadio(group, "en", "light")).toBeChecked();
    await expect(html).toHaveAttribute("data-theme", "light");
    expect(await storedKeys(page)).toEqual({ "hc-theme": "light" });

    await page.keyboard.press("ArrowRight");
    await expect(themeRadio(group, "en", "dark")).toBeChecked();
    await expect(html).toHaveAttribute("data-theme", "dark");
    expect(await storedKeys(page)).toEqual({ "hc-theme": "dark" });

    // Wraps around to System, which removes the key and follows the OS.
    await page.keyboard.press("ArrowRight");
    await expect(themeRadio(group, "en", "system")).toBeChecked();
    await expect(html).toHaveAttribute("data-theme", "light");
    expect(await storedKeys(page)).toEqual({});
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(html).toHaveAttribute("data-theme", "dark");
    await page.emulateMedia({ colorScheme: "light" });

    await page.keyboard.press("ArrowLeft");
    await expect(themeRadio(group, "en", "dark")).toBeChecked();
    expect(await storedKeys(page)).toEqual({ "hc-theme": "dark" });

    // One tab stop: Tab leaves the group, Shift+Tab comes back to the
    // selected segment.
    await page.keyboard.press("Tab");
    expect(await page.evaluate(() => document.activeElement?.closest("[data-theme-control]") === null)).toBe(true);
    await page.keyboard.press("Shift+Tab");
    await expect(themeRadio(group, "en", "dark")).toBeFocused();

    // The choice survives a reload; no cookie anywhere.
    await page.reload();
    await hydrated(page);
    await expect(html).toHaveAttribute("data-theme", "dark");
    await expect(themeRadio(group, "en", "dark")).toBeChecked();
    await expect(themeRadio(group, "en", "system")).not.toBeChecked();
    expect(await context.cookies()).toEqual([]);
  });

  test("Space selects the focused segment", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await useTheme(page, "dark");
    await page.goto(localePath("en", ""));
    await hydrated(page);
    const group = page.locator(".hc-topbar-controls").getByRole("radiogroup", { name: t("en", "Theme.label") });
    await expect(themeRadio(group, "en", "dark")).toBeChecked();
    // Focus on an unselected segment (as a screen reader's virtual cursor
    // can leave it) does not select it; Space does.
    await themeRadio(group, "en", "light").focus();
    await expect(themeRadio(group, "en", "dark")).toBeChecked();
    await page.keyboard.press("Space");
    await expect(themeRadio(group, "en", "light")).toBeChecked();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  });

  test("names come from the catalogs, in every locale's words", async ({ page }) => {
    for (const locale of ["en", "es", "pt-BR", "fr"] as const) {
      await page.goto(localePath(locale, ""));
      await hydrated(page);
      const group = page.locator(".hc-topbar-controls").getByRole("radiogroup", { name: t(locale, "Theme.label") });
      for (const option of ["system", "light", "dark"] as const) {
        await expect(themeRadio(group, locale, option)).toHaveCount(1);
      }
    }
  });

  test("in the mobile drawer: words shown, keyboard the same", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(localePath("en", "docs"));
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
    const drawer = page.getByRole("dialog", { name: t("en", "Shell.menuTitle") });
    const group = drawer.getByRole("radiogroup", { name: t("en", "Theme.label") });
    await expect(group).toBeVisible();
    for (const option of ["system", "light", "dark"] as const) {
      await expect(group.getByText(t("en", `Theme.options.${option}`), { exact: true })).toBeVisible();
    }
    await themeRadio(group, "en", "system").focus();
    await page.keyboard.press("ArrowLeft");
    await expect(themeRadio(group, "en", "dark")).toBeChecked();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    // The group is the drawer's last Tab stop: Tab wraps inside the dialog,
    // Shift+Tab from the first stop comes back to the selected segment.
    await page.keyboard.press("Tab");
    expect(await focusInOpenDialog(page)).toBe(true);
    expect(await page.evaluate(() => document.activeElement?.closest("[data-theme-control]") === null)).toBe(true);
    await page.keyboard.press("Shift+Tab");
    await expect(themeRadio(group, "en", "dark")).toBeFocused();
    // The segments are touch-sized in the drawer.
    const height = await group.locator(".hc-segment").first().evaluate((element) => element.getBoundingClientRect().height);
    expect(height).toBeGreaterThanOrEqual(44);
  });

  test("in the mobile drawer: the trap holds from every segment, whichever is checked", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(localePath("en", "docs"));
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
    const drawer = page.getByRole("dialog", { name: t("en", "Shell.menuTitle") });
    const group = drawer.getByRole("radiogroup", { name: t("en", "Theme.label") });
    await expect(group).toBeVisible();
    const options = ["system", "light", "dark"] as const;
    for (const checked of options) {
      await themeRadio(group, "en", checked).focus();
      await page.keyboard.press("Space");
      await expect(themeRadio(group, "en", checked)).toBeChecked();
      // Tab from the checked segment (not always the last input in the DOM)
      // wraps inside the dialog and leaves the group.
      await page.keyboard.press("Tab");
      expect(await focusInOpenDialog(page), `Tab from checked ${checked}`).toBe(true);
      expect(await page.evaluate(() => document.activeElement?.closest("[data-theme-control]") === null)).toBe(true);
      // Focus on an unchecked segment (a screen reader's virtual cursor can
      // put it there): Tab still stays in the dialog.
      for (const focused of options.filter((option) => option !== checked)) {
        await themeRadio(group, "en", focused).focus();
        await page.keyboard.press("Tab");
        expect(await focusInOpenDialog(page), `Tab from ${focused} with ${checked} checked`).toBe(true);
        await themeRadio(group, "en", focused).focus();
        await page.keyboard.press("Shift+Tab");
        expect(await focusInOpenDialog(page), `Shift+Tab from ${focused} with ${checked} checked`).toBe(true);
      }
      await expect(themeRadio(group, "en", checked)).toBeChecked();
    }
  });

  test("the icons draw a 1.5px stroke (design-direction 3.9)", async ({ page }) => {
    await page.goto(localePath("en", ""));
    await hydrated(page);
    const strokes = await page.locator(".hc-topbar-controls [data-theme-control] svg").evaluateAll((icons) =>
      icons.map((icon) => {
        const box = (icon as SVGSVGElement).viewBox.baseVal;
        const width = Number(icon.getAttribute("width"));
        return (Number(icon.getAttribute("stroke-width")) * width) / box.width;
      }),
    );
    // Three option icons and three checks.
    expect(strokes).toHaveLength(6);
    for (const stroke of strokes) {
      expect(stroke).toBeCloseTo(1.5, 5);
    }
  });

  for (const theme of ["light", "dark"] as const) {
    test(`axe finds nothing on the theme control, top bar and drawer (${theme})`, async ({ page }) => {
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await page.goto(localePath("en", ""));
      await hydrated(page);
      const top = await new AxeBuilder({ page }).include(".hc-topbar-controls [data-theme-control]").withTags(AXE_TAGS).analyze();
      expect(top.passes.length).toBeGreaterThan(0);
      expect(top.violations.map(({ id, nodes }) => `${id}: ${nodes.length}`)).toEqual([]);

      await page.setViewportSize({ width: 390, height: 844 });
      await page.reload();
      await hydrated(page);
      await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
      await expect(page.locator("dialog[open]")).toHaveCSS("opacity", "1");
      const drawer = await new AxeBuilder({ page }).include("dialog[open] [data-theme-control]").withTags(AXE_TAGS).analyze();
      expect(drawer.passes.length).toBeGreaterThan(0);
      expect(drawer.violations.map(({ id, nodes }) => `${id}: ${nodes.length}`)).toEqual([]);
    });
  }
});
