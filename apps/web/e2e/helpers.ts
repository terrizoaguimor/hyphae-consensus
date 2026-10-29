// SPDX-License-Identifier: Apache-2.0
//
// Assertions the API specs share (api.spec.ts, showcase.spec.ts): the
// honesty checks that nothing of the LIVE family is on a page (its labels, or
// any text and accessible name of it at all), the run page
// opener, and the bookkeeping of the API errors a test provokes on purpose.

import type { Locator, Page } from "@playwright/test";

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

/**
 * Every piece of text under `root` that carries the locale's LIVE word: each
 * text node (hidden ones too, since a tab panel or a badge may be hidden
 * from the eye but not from a screen reader) and every attribute a person or
 * an assistive technology reads (aria-label, aria-description,
 * aria-roledescription, aria-valuetext, title, alt, placeholder, the value of
 * a button-like input), plus the document title when the root is the body.
 *
 * Not scanned, because none of it is a label: scripts and styles (the
 * framework's payload carries the whole catalog), identifiers shown as code
 * (code, pre, kbd, samp), and the quoted text of the run itself (the
 * blockquote of an UntrustedText figure: a record's or a model's own words,
 * English whatever the locale). `identifiers` are the archive's own names
 * that a sentence interpolates as plain text (battery ids such as
 * live-battery-2026-10-27, log paths); they are cut out before matching.
 *
 * A term of more than one word (EN VIVO, AO VIVO, EN DIRECT), or a word
 * split by markup (`<strong>EN</strong> VIVO`, `{t("a")} {t("b")}`), may sit
 * in adjacent text nodes, none of which carries it alone. So the joined text
 * of every element (whitespace collapsed) is matched as well, and reported at
 * the innermost element whose joined text carries the word while none of its
 * own text nodes or child elements does: each hit is reported once.
 */
export async function liveWordHits(
  root: Locator,
  locale: Locale,
  identifiers: readonly string[] = [],
): Promise<string[]> {
  const word = liveWord(locale);
  const longestFirst = [...identifiers].sort((a, b) => b.length - a.length);
  return root.evaluate(
    (element, { source, flags, ids }) => {
      const pattern = new RegExp(source, flags);
      const carries = (text: string) =>
        pattern.test(ids.reduce((rest, id) => rest.split(id).join(" "), text));
      const copy = element.cloneNode(true) as HTMLElement;
      copy
        .querySelectorAll("script, style, template, code, pre, kbd, samp, [data-untrusted] > blockquote")
        .forEach((node) => node.remove());
      const found: string[] = [];
      if (element === document.body) {
        found.push(`title: ${document.title}`);
      }
      const walker = document.createTreeWalker(copy, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
        const text = node.textContent ?? "";
        if (text.trim() !== "") {
          found.push(`text: ${text}`);
        }
      }
      const joined = (node: Element) => (node.textContent ?? "").replace(/\s+/g, " ").trim();
      for (const node of [copy, ...copy.querySelectorAll("*")]) {
        const text = joined(node);
        if (
          carries(text) &&
          ![...node.childNodes].some(
            (child) =>
              (child.nodeType === Node.TEXT_NODE && carries(child.textContent ?? "")) ||
              (child instanceof Element && carries(joined(child))),
          )
        ) {
          found.push(`text: ${text}`);
        }
      }
      const attributes = [
        "aria-label",
        "aria-description",
        "aria-roledescription",
        "aria-valuetext",
        "title",
        "alt",
        "placeholder",
      ];
      for (const node of [copy, ...copy.querySelectorAll("*")]) {
        for (const name of attributes) {
          const value = node.getAttribute(name);
          if (value !== null && value.trim() !== "") {
            found.push(`${name}: ${value}`);
          }
        }
        if (node instanceof HTMLInputElement && ["button", "submit", "reset"].includes(node.type)) {
          found.push(`value: ${node.value}`);
        }
      }
      return found.filter(carries);
    },
    { source: word.source, flags: word.flags, ids: longestFirst },
  );
}

/**
 * Everything expectNoLive checks, and no text or accessible name anywhere on
 * the page carrying the locale's LIVE word (see liveWordHits): for the views
 * of a recorded run and the showcase, where nothing may read LIVE at all.
 */
export async function expectNoLiveOnPage(page: Page, locale: Locale, identifiers: readonly string[] = []) {
  await expectNoLive(page, locale);
  expect(await liveWordHits(page.locator("body"), locale, identifiers), `the LIVE word on ${page.url()}`).toEqual([]);
}

export async function openRun(page: Page, locale: Locale, runId = RUN_ID) {
  await page.goto(runPath(locale, runId));
  await hydrated(page);
  await expect(page.locator("[data-run-id]")).toBeVisible();
}
