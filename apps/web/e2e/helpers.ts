// SPDX-License-Identifier: Apache-2.0
//
// Assertions the API specs share (api.spec.ts, showcase.spec.ts): the
// honesty check that nothing of the LIVE family is on a page, the run page
// opener, and the bookkeeping of the API errors a test provokes on purpose.

import type { Page } from "@playwright/test";

import { runPath } from "../src/lib/site";

import { RUN_ID } from "./api-mock";
import { expect, hydrated, type Locale, t } from "./fixtures";

/** Removes an expected API error response from the page's problem list, asserting it happened. */
export function takeProblem(problems: string[], text: string): void {
  const index = problems.indexOf(text);
  expect(index, `expected ${text} among ${JSON.stringify(problems)}`).toBeGreaterThanOrEqual(0);
  problems.splice(index, 1);
}

/** Removes the browser's own console lines about a failure a test provokes on purpose. */
export function dropProblems(problems: string[], pattern: RegExp): void {
  for (let i = problems.length - 1; i >= 0; i -= 1) {
    if (pattern.test(problems[i] ?? "")) {
      problems.splice(i, 1);
    }
  }
}

/** The exact LIVE badge text of a locale; never present on a replayed or flagged run. */
export function liveBadge(page: Page, locale: Locale) {
  return page.getByText(t(locale, "Provenance.live.label"), { exact: true });
}

/** The locale's LIVE word (LIVE, EN VIVO, AO VIVO, EN DIRECT) as a whole word. */
export function liveWord(locale: Locale): RegExp {
  const word = t(locale, "Provenance.live.label").replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(^|[^\\p{L}])${word}([^\\p{L}]|$)`, "iu");
}

/**
 * Nothing of the LIVE family is on the page: no LIVE delivery badge, no LIVE
 * META model badge, no "Meta model · live" note, no tab named Live, and no
 * badge or provenance note carrying the locale's LIVE word at all.
 */
export async function expectNoLive(page: Page, locale: Locale) {
  await expect(liveBadge(page, locale)).toHaveCount(0);
  await expect(page.getByText(t(locale, "Provenance.liveMeta.label"), { exact: true })).toHaveCount(0);
  await expect(page.getByText(t(locale, "Provenance.liveMeta.live"), { exact: true })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: t(locale, "Run.tabs.live"), exact: true })).toHaveCount(0);
  await expect(page.locator("[data-frame-label='live.label']")).toHaveCount(0);
  const labels = await page
    .locator(".hc-badges, .hc-frame-badge, [data-provenance-note], [role=tab]")
    .allInnerTexts();
  for (const text of labels) {
    expect(text, `a label with the LIVE word: ${text}`).not.toMatch(liveWord(locale));
  }
}

export async function openRun(page: Page, locale: Locale, runId = RUN_ID) {
  await page.goto(runPath(locale, runId));
  await hydrated(page);
  await expect(page.locator("[data-run-id]")).toBeVisible();
}
