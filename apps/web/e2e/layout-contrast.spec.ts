// SPDX-License-Identifier: Apache-2.0
//
// Two findings of the 2026-10 heuristic evaluation, measured in the browser.
//
// HE #9: no main screen is wider than a 390 px phone. The decision page and
// the recorded decision page measured 437 px: the revision comparison's
// 4-column claim table set the minimum width of the panel around it. Every
// screen is opened at 390 px (with touch, as the evaluation's shots were) and
// document.scrollingElement.scrollWidth must stay at 390 or less; claim ids
// stay on one line.
//
// HE #10: control boundaries meet WCAG 1.4.11 (3:1 against the colors next
// to them): the unselected radio ring, the unchecked checkbox box, the off
// Switch track, the Settings panels and the dashed not-connected panel, in
// both themes. Colors are read from the computed styles; a translucent
// background is composited over what is behind it.

import type { Page } from "@playwright/test";

import { docSlugs, localePath, runPath, showcaseRunPath } from "../src/lib/site";

import { awaitingDecisionDetail, mockApi, recordedRun, RUN_ID } from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, type Locale, t, test, useTheme } from "./fixtures";
import { archiveEntry, manifestRun, metaRequestIds } from "./research";

type Json = Record<string, unknown>;

const F03 = fixture<Json>("showcase-entry-F03.json");

/** The showcase list the evaluation used: C01, L01, R06 (live model, Hyphae evidence) and F03. */
function entries() {
  const live = (label: string) => {
    const run = manifestRun(label);
    return {
      model: "live_meta",
      evidence: "hyphae_live",
      flags: ["mode_undeclared"],
      meta_request_ids: metaRequestIds(run.file, run.run_id),
    };
  };
  const c01 = archiveEntry("C01", {
    model: "scripted",
    evidence: "simulated",
    flags: ["mode_undeclared", "unchained"],
    meta_request_ids: [],
  });
  const r06 = archiveEntry("R06", live("R06"));
  return { r06, list: [c01, archiveEntry("L01", live("L01")), r06, F03] };
}

const PHONE = 390;

/** The page's width and the elements that reach past the viewport (for the failure message). */
async function pageWidth(page: Page): Promise<{ width: number; wide: string[] }> {
  return page.evaluate((limit) => {
    const wide: string[] = [];
    for (const element of Array.from(document.querySelectorAll("body *"))) {
      const box = element.getBoundingClientRect();
      if (box.width > 0 && box.right > limit + 0.5 && wide.length < 10) {
        wide.push(`${element.tagName.toLowerCase()}.${element.className}: right ${Math.round(box.right)}`);
      }
    }
    return { width: document.scrollingElement?.scrollWidth ?? Number.POSITIVE_INFINITY, wide };
  }, PHONE);
}

async function expectFitsPhone(page: Page) {
  // Fonts change line breaks and so widths: measure once they are in.
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  const { width, wide } = await pageWidth(page);
  expect(width, `scrollWidth ${width}; wider elements: ${wide.join("; ")}`).toBeLessThanOrEqual(PHONE);
}

async function openDecision(page: Page, locale: Locale) {
  await mockApi(page, { detail: awaitingDecisionDetail() });
  await page.goto(runPath(locale, RUN_ID));
  await hydrated(page);
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t(locale, "Run.tabs.decision") }).click();
  await expect(page.locator("[data-decision-form]")).toBeVisible();
  await expect(page.locator("[data-superseded] [data-revision-compare]")).toBeVisible();
}

async function openRecordedDecision(page: Page, locale: Locale) {
  const { r06, list } = entries();
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await page.goto(showcaseRunPath(locale, String(r06.run_id)));
  await hydrated(page);
  await expect(page.locator("[data-run-mode='recorded']")).toBeVisible();
  await expect(page.locator("[data-superseded] [data-revision-compare]")).toBeVisible();
}

test.describe("HE #9: every main screen fits a 390 px phone", () => {
  test.use({ viewport: { width: PHONE, height: 844 }, hasTouch: true });

  const screens: { name: string; open: (page: Page, locale: Locale) => Promise<void> }[] = [
    {
      name: "overview",
      open: async (page, locale) => {
        await mockApi(page, { showcase: entries().list });
        await page.goto(localePath(locale, ""));
        await hydrated(page);
        await expect(page.locator("[data-figures='ready']")).toBeVisible();
      },
    },
    {
      name: "new deliberation",
      open: async (page, locale) => {
        await mockApi(page);
        await page.goto(localePath(locale, "new"));
        await hydrated(page);
        await expect(page.locator("[data-scenario]").first()).toBeVisible();
      },
    },
    {
      name: "new deliberation, live form open",
      open: async (page, locale) => {
        await mockApi(page, { liveEnabled: true, liveRequiresTurnstile: false });
        await page.goto(localePath(locale, "new"));
        await hydrated(page);
        await expect(page.locator("[data-live='form'][data-live-state='open']")).toBeVisible();
      },
    },
    {
      name: "runs",
      open: async (page, locale) => {
        await mockApi(page);
        await page.goto(localePath(locale, "runs"));
        await hydrated(page);
        await expect(page.locator("[data-run-id]").first()).toBeVisible();
      },
    },
    {
      name: "run playback, awaiting a decision",
      open: async (page, locale) => {
        await mockApi(page, { detail: awaitingDecisionDetail() });
        await page.goto(runPath(locale, RUN_ID));
        await hydrated(page);
        await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
      },
    },
    { name: "run decision, awaiting, with a revision", open: openDecision },
    {
      name: "showcase",
      open: async (page, locale) => {
        await mockApi(page, { showcase: entries().list });
        await page.goto(localePath(locale, "showcase"));
        await hydrated(page);
        await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
      },
    },
    { name: "recorded run R06, decision", open: openRecordedDecision },
    ...["docs", `docs/${docSlugs[0] ?? ""}`, "settings", "terms", "privacy", "limitations", "about"].map(
      (path) => ({
        name: path,
        open: async (page: Page, locale: Locale) => {
          await page.goto(localePath(locale, path));
          await hydrated(page);
        },
      }),
    ),
  ];

  // English, and Spanish for its longer words.
  for (const locale of ["en", "es"] as const) {
    for (const screen of screens) {
      test(`${screen.name} (${locale}) is no wider than ${PHONE} px`, async ({ page }) => {
        await useTheme(page, "light");
        await screen.open(page, locale);
        await expectFitsPhone(page);
      });
    }
  }

  test("the claim comparison is one block per claim on a phone, its ids on one line", async ({ page }) => {
    await useTheme(page, "light");
    await openDecision(page, "en");
    const table = page.locator("[data-superseded] [data-revision-compare] table");
    // Stacked: no visible column header row, each cell under its own column name.
    await expect(table.locator("thead")).toHaveCSS("position", "absolute");
    const first = table.locator("[data-claim-id='claim-1']");
    await expect(first).toHaveCSS("display", "block");
    await expect(first.locator("td").nth(0)).toHaveAttribute(
      "data-label",
      t("en", "Run.revision.originalColumn", { id: "prop-1" }),
    );
    const before = await first.locator("td").nth(0).evaluate((cell) => getComputedStyle(cell, "::before").content);
    expect(before).toBe(`"${t("en", "Run.revision.originalColumn", { id: "prop-1" })}"`);
    // Every claim id is one line high.
    for (const id of await table.locator(".hc-id").all()) {
      const lines = await id.evaluate((element) => element.getClientRects().length);
      expect(lines).toBe(1);
      await expect(id).toHaveCSS("white-space", "nowrap");
    }
  });

  test("the claim comparison is still a table from 672 px", async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 800 });
    await useTheme(page, "light");
    await openDecision(page, "en");
    const table = page.locator("[data-superseded] [data-revision-compare] table");
    await expect(table).toHaveCSS("display", "table");
    await expect(table.locator("thead")).toBeVisible();
    await expect(table.locator("thead th")).toHaveCount(4);
    const before = await table
      .locator("[data-claim-id='claim-1'] td")
      .nth(0)
      .evaluate((cell) => getComputedStyle(cell, "::before").content);
    expect(before).toBe("none");
  });
});

// ---- non-text contrast -------------------------------------------------------

type Rgb = [number, number, number];

function luminance([r, g, b]: Rgb): number {
  const [lr, lg, lb] = [r, g, b].map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (lr ?? 0) + 0.7152 * (lg ?? 0) + 0.0722 * (lb ?? 0);
}

function contrast(a: Rgb, b: Rgb): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((high ?? 0) + 0.05) / ((low ?? 0) + 0.05);
}

type Boundary = {
  /** The boundary's own color: its border, or its fill for a Switch track. */
  mark: Rgb;
  /** The opaque colors next to it: the fill inside and the background outside. */
  inside: Rgb;
  outside: Rgb;
};

/**
 * The colors of the boundary of each element matching `selector`: its border
 * color (or background color when `fill`), the background it encloses and the
 * background around it, each translucent layer composited over the next.
 */
async function boundaries(page: Page, selector: string, fill = false): Promise<Boundary[]> {
  return page.locator(selector).evaluateAll(
    (elements, useFill) => {
      type Rgba = [number, number, number, number];
      const parse = (value: string): Rgba => {
        const parts = /rgba?\(([^)]+)\)/.exec(value)?.[1]?.split(/[\s,/]+/).filter(Boolean).map(Number);
        if (parts === undefined || parts.length < 3) {
          throw new Error(`unparsed color ${value}`);
        }
        return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0, parts[3] ?? 1];
      };
      const over = (top: Rgba, bottom: [number, number, number]): [number, number, number] => [
        top[0] * top[3] + bottom[0] * (1 - top[3]),
        top[1] * top[3] + bottom[1] * (1 - top[3]),
        top[2] * top[3] + bottom[2] * (1 - top[3]),
      ];
      /** The opaque color behind `start` and its ancestors, layers composited. */
      const behind = (start: Element | null): [number, number, number] => {
        const layers: Rgba[] = [];
        for (let element = start; element !== null; element = element.parentElement) {
          const layer = parse(getComputedStyle(element).backgroundColor);
          if (layer[3] > 0) {
            layers.push(layer);
            if (layer[3] >= 1) {
              break;
            }
          }
        }
        let color: [number, number, number] = [255, 255, 255];
        const root = layers.at(-1);
        if (root === undefined || root[3] < 1) {
          color = over(parse(getComputedStyle(document.documentElement).backgroundColor), color);
        }
        for (const layer of layers.reverse()) {
          color = over(layer, color);
        }
        return color;
      };
      return elements.map((element) => {
        const style = getComputedStyle(element);
        const outside = behind(element.parentElement);
        const markLayer = parse(useFill ? style.backgroundColor : style.borderTopColor);
        return {
          mark: over(markLayer, outside),
          inside: useFill ? outside : behind(element),
          outside,
        };
      });
    },
    fill,
  );
}

function expectBoundaries(name: string, found: Boundary[], fill = false) {
  expect(found.length, `${name}: none on the page`).toBeGreaterThan(0);
  for (const [index, boundary] of found.entries()) {
    const outside = contrast(boundary.mark, boundary.outside);
    expect(outside, `${name} #${index} against its surroundings: ${outside.toFixed(2)}:1`).toBeGreaterThanOrEqual(3);
    if (!fill) {
      const inside = contrast(boundary.mark, boundary.inside);
      expect(inside, `${name} #${index} against its own fill: ${inside.toFixed(2)}:1`).toBeGreaterThanOrEqual(3);
    }
  }
}

/** Unselected, enabled radio rings and unchecked checkbox boxes. */
const OPEN_RADIO = "input[type='radio']:not(:checked):not(:disabled) + span > .astryx-radio-indicator";
const OPEN_CHECKBOX = "input[type='checkbox']:not(:checked):not(:disabled) + span > .astryx-checkbox-indicator";

for (const theme of ["light", "dark"] as const) {
  test.describe(`HE #10: control boundaries reach 3:1 (${theme})`, () => {
    test.beforeEach(async ({ page }) => {
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    });

    test(`Settings: panel borders and unselected radio rings (${theme})`, async ({ page }) => {
      await page.goto(localePath("en", "settings"));
      await hydrated(page);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      expectBoundaries("Settings panel border", await boundaries(page, ".hc-settings .hc-panel"));
      expectBoundaries("unselected radio ring", await boundaries(page, `.hc-settings ${OPEN_RADIO}`));
    });

    test(`New deliberation: preset radio rings and the attestation checkbox (${theme})`, async ({ page }) => {
      await mockApi(page, { liveEnabled: true, liveRequiresTurnstile: false });
      await page.goto(localePath("en", "new"));
      await hydrated(page);
      const form = page.locator("[data-live='form'][data-live-state='open']");
      await expect(form).toBeVisible();
      expectBoundaries("preset radio ring", await boundaries(page, `[data-live='form'] ${OPEN_RADIO}`));
      expectBoundaries("unchecked checkbox box", await boundaries(page, `[data-live='form'] ${OPEN_CHECKBOX}`));
    });

    test(`the dashed not-connected panel (${theme})`, async ({ page }) => {
      // No mock: the static server has no API, so the runs page says so.
      await page.goto(localePath("en", "runs"));
      await hydrated(page);
      await expect(page.locator(".hc-not-connected").first()).toBeVisible();
      expectBoundaries("not-connected panel border", await boundaries(page, ".hc-not-connected"));
    });
  });
}

test.describe("HE #10: the off Switch track reaches 3:1", () => {
  test.beforeEach(async ({ page }) => {
    // The Switch is "Dark theme": off in the light theme.
    await useTheme(page, "light");
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  });

  test("in the top bar", async ({ page }) => {
    await page.goto(localePath("en", "settings"));
    await hydrated(page);
    const track = page.locator(".hc-topbar .astryx-switch, header .astryx-switch").first();
    await expect(track).not.toHaveAttribute("data-checked", "checked");
    expectBoundaries("off Switch track", await boundaries(page, "header .astryx-switch:not([data-checked])", true), true);
    // The thumb stays distinguishable from the track as well.
    const thumb = await boundaries(page, "header .astryx-switch:not([data-checked]) > .astryx-switch-thumb", true);
    const [trackColor] = await boundaries(page, "header .astryx-switch:not([data-checked])", true);
    expect(trackColor).toBeDefined();
    for (const { mark } of thumb) {
      expect(contrast(mark, trackColor?.mark ?? mark)).toBeGreaterThanOrEqual(3);
    }
  });

  test("in the mobile drawer", async ({ page }) => {
    await page.setViewportSize({ width: PHONE, height: 844 });
    await page.goto(localePath("en", ""));
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
    const drawer = page.getByRole("dialog", { name: t("en", "Shell.menuTitle") });
    await expect(drawer).toBeVisible();
    expectBoundaries(
      "off Switch track in the drawer",
      await boundaries(page, "dialog[open] .astryx-switch:not([data-checked])", true),
      true,
    );
  });
});
