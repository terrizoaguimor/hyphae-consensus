// SPDX-License-Identifier: Apache-2.0
//
// The language links keep the page's query string: a run page is addressed by
// ?id=...&source=..., so switching language there must land on the same run,
// not on "no run selected". Pages without a query keep their plain links. The
// header switcher (desktop) and the drawer (390 px) are both covered.

import type { Page } from "@playwright/test";

import { showcaseRunPath } from "../src/lib/site";

import { mockApi, recordedRun } from "./api-mock";
import { archiveEntry, manifestRun, metaRequestIds } from "./research";
import { expect, hydrated, type Locale, locales, t, test } from "./fixtures";

function recordedEntry() {
  const run = manifestRun("R06");
  return archiveEntry("R06", {
    model: "live_meta",
    evidence: "hyphae_live",
    flags: ["mode_undeclared"],
    meta_request_ids: metaRequestIds(run.file, run.run_id),
  });
}

const endonym = (locale: Locale) => t(locale, `LocaleSwitcher.names.${locale}`);

/** The locale link in the header switcher, or in the open drawer at a phone width. */
async function languageLink(page: Page, from: Locale, locale: Locale, drawer: boolean) {
  if (drawer) {
    await page
      .getByRole("button", { name: t(from, "Shell.menuButton"), exact: true })
      .click();
    const dialog = page.getByRole("dialog", { name: t(from, "Shell.menuTitle") });
    return dialog
      .getByRole("navigation", { name: t(from, "LocaleSwitcher.label") })
      .getByRole("link", { name: endonym(locale), exact: true });
  }
  return page
    .getByRole("navigation", { name: t(from, "LocaleSwitcher.label") })
    .getByRole("link", { name: endonym(locale), exact: true });
}

async function expectRunOpen(page: Page, locale: Locale, runId: string) {
  const url = new URL(page.url());
  expect(url.pathname).toBe(`/${locale}/run/`);
  expect(url.searchParams.get("id")).toBe(runId);
  expect(url.searchParams.get("source")).toBe("showcase");
  await hydrated(page);
  await expect(page.locator("[data-run-mode='recorded']")).toBeVisible();
  await expect(page.getByText(t(locale, "Run.noId"), { exact: true })).toHaveCount(0);
}

async function switchThroughLocales(page: Page, drawer: boolean) {
  const r06 = recordedEntry();
  const runId = String(r06.run_id);
  await mockApi(page, { showcase: [r06], recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await page.goto(showcaseRunPath("en", runId));
  await hydrated(page);
  await expect(page.locator("[data-run-mode='recorded']")).toBeVisible();

  for (const target of locales.filter((locale) => locale !== "en")) {
    const link = await languageLink(page, "en", target, drawer);
    // After hydration the link carries the query (the prerendered HTML has none).
    await expect(link).toHaveAttribute("href", new RegExp(`^/${target}/run/\\?id=.*source=showcase`, "u"));
    await expect(link).toHaveAttribute("hreflang", target);
    await link.click();
    await page.waitForURL(new RegExp(`/${target}/run/\\?`, "u"));
    await expectRunOpen(page, target, runId);

    const back = await languageLink(page, target, "en", drawer);
    await expect(back).toHaveAttribute("href", new RegExp(`^/en/run/\\?id=.*source=showcase`, "u"));
    await back.click();
    await page.waitForURL(/\/en\/run\/\?/u);
    await expectRunOpen(page, "en", runId);
  }
}

test("the header language links keep a run page's query string", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await switchThroughLocales(page, false);
});

test("the drawer's language links keep a run page's query string @mobile", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await switchThroughLocales(page, true);
});

test("a page without a query keeps plain language links", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/en/docs/");
  await hydrated(page);
  for (const target of locales) {
    const link = await languageLink(page, "en", target, false);
    await expect(link).toHaveAttribute("href", `/${target}/docs/`);
  }
  await page.setViewportSize({ width: 390, height: 844 });
  for (const target of locales) {
    await page.reload();
    await hydrated(page);
    const link = await languageLink(page, "en", target, true);
    await expect(link).toHaveAttribute("href", `/${target}/docs/`);
  }
});

test("the language links follow client-side navigation", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const r06 = recordedEntry();
  const runId = String(r06.run_id);
  await mockApi(page, { showcase: [r06], recordedRuns: [recordedRun(r06, { base: "retry" })] });
  const es = () => languageLink(page, "en", "es", false);
  await page.goto("/en/showcase/");
  await hydrated(page);
  await expect(await es()).toHaveAttribute("href", "/es/showcase/");

  // Showcase card (an in-app Link) to the run page: the query must appear.
  await page.locator(`a[href*="id=${runId}"]`).first().click();
  await page.waitForURL(/\/en\/run\/\?/u);
  await expect(await es()).toHaveAttribute("href", new RegExp(`^/es/run/[?]id=${runId}.*source=showcase`, "u"));
  await (await es()).click();
  await page.waitForURL(/\/es\/run\/\?/u);
  await expectRunOpen(page, "es", runId);

  // From a run page in-app to Docs: the previous page's query must not leak.
  await page.goto(showcaseRunPath("en", runId));
  await hydrated(page);
  await expect(await es()).toHaveAttribute("href", /\?id=/u);
  await page
    .getByRole("navigation", { name: t("en", "Shell.primaryNavLabel") })
    .getByRole("link", { name: t("en", "Nav.docs"), exact: true })
    .click();
  await page.waitForURL(/\/en\/docs\//u);
  await expect(await es()).toHaveAttribute("href", "/es/docs/");

  // Router push to a run page, then to another run on the same path.
  await page.goto("/en/new/");
  await hydrated(page);
  const push = (u: string) =>
    page.evaluate((url) => (window as unknown as { next: { router: { push(x: string): void } } }).next.router.push(url), u);
  await push(`/en/run/?id=${runId}&source=showcase`);
  await page.waitForURL(/\/en\/run\/\?id=/u);
  await expect(await es()).toHaveAttribute("href", `/es/run/?id=${runId}&source=showcase`);
  await push(`/en/run/?id=${runId}&source=showcase&x=1`);
  await expect(await es()).toHaveAttribute("href", `/es/run/?id=${runId}&source=showcase&x=1`);
});
