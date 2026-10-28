// SPDX-License-Identifier: Apache-2.0
//
// axe-core (through @axe-core/playwright) on every page of the export, in
// light and dark, in all four locales, plus the two overlays (tour dialog,
// mobile drawer) and the 404 page. The suite allows no serious or critical
// violation; every result, including minor ones and the rules axe could not
// decide, goes to test-results/axe/ and is added up in
// test-results/axe-summary.json.
//
// The root page (/) is not scanned: it redirects to a locale as soon as it
// loads (with or without JavaScript), so there is no settled page to check.

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";

import {
  AXE_TAGS,
  blocking,
  type ScanSummary,
  summarize,
  writeScans,
} from "./axe-results";
import {
  expect,
  hydrated,
  locales,
  pagesFor,
  t,
  test,
  useTheme,
} from "./fixtures";

const themes = ["light", "dark"] as const;

async function scan(page: Page, name: string): Promise<ScanSummary> {
  const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
  return summarize(name, results);
}

for (const locale of locales) {
  for (const theme of themes) {
    test(`axe: every page (${locale}, ${theme})`, async ({ page }) => {
      test.setTimeout(300_000);
      await useTheme(page, theme);
      // Overlays and transitions are settled when scanned.
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      const scans: ScanSummary[] = [];
      for (const { path } of pagesFor(locale)) {
        await page.goto(path);
        await hydrated(page);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        scans.push(await scan(page, `${locale} ${theme} ${path}`));
      }
      await writeScans(`pages-${locale}-${theme}`, scans);
      expect(scans).toHaveLength(pagesFor(locale).length);
      expect(blocking(scans)).toEqual([]);
    });

    test(`axe: tour dialog and mobile drawer (${locale}, ${theme})`, async ({
      page,
    }) => {
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      const scans: ScanSummary[] = [];

      await page.goto(`/${locale}/docs/`);
      await hydrated(page);
      await page.getByRole("button", { name: t(locale, "Tour.launch") }).click();
      await expect(page.locator("dialog[open]")).toBeVisible();
      await expect(page.locator("dialog[open]")).toHaveCSS("opacity", "1");
      scans.push(await scan(page, `${locale} ${theme} tour dialog`));
      await page.keyboard.press("Escape");

      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto(`/${locale}/`);
      await hydrated(page);
      await page.getByRole("button", { name: t(locale, "Shell.menuButton"), exact: true }).click();
      await expect(page.locator("dialog[open]")).toBeVisible();
      await expect(page.locator("dialog[open]")).toHaveCSS("opacity", "1");
      scans.push(await scan(page, `${locale} ${theme} mobile drawer`));

      await writeScans(`overlays-${locale}-${theme}`, scans);
      expect(blocking(scans)).toEqual([]);
    });
  }
}

// The 404 page (src/app/global-not-found.tsx) is served for every unknown
// path. It is fetched by its file name here, so the request answers 200 and
// the page fixture does not count it as a failed request; the navigation spec
// checks that unknown paths get it with status 404.
for (const theme of themes) {
  test(`axe: the 404 page (${theme})`, async ({ page }) => {
    await useTheme(page, theme);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await page.goto("/404.html");
    await page.waitForLoadState("load");
    const html = page.locator("html");
    await expect(html).toHaveAttribute("lang", "en");
    await expect(html).toHaveAttribute("data-theme", theme);
    await expect(page.locator("main h1")).toHaveText(t("en", "NotFound.title"));
    for (const locale of locales) {
      await expect(
        page.getByRole("link", { name: t(locale, `LocaleSwitcher.names.${locale}`) }),
      ).toHaveAttribute("href", `/${locale}/`);
    }
    await expect(
      page.getByText(t("en", "Footer.nonAffiliation"), { exact: true }),
    ).toBeVisible();
    const scans = [await scan(page, `en ${theme} /404.html`)];
    await writeScans(`not-found-${theme}`, scans);
    expect(blocking(scans)).toEqual([]);
  });
}

test("axe: the mobile layout of a long page (en, light)", async ({ page }) => {
  await useTheme(page, "light");
  await page.setViewportSize({ width: 390, height: 844 });
  const scans: ScanSummary[] = [];
  for (const path of ["/en/", "/en/settings/", "/en/privacy/", "/en/docs/not-voting/"]) {
    await page.goto(path);
    await hydrated(page);
    scans.push(await scan(page, `en light mobile ${path}`));
  }
  await writeScans("mobile-en-light", scans);
  expect(blocking(scans)).toEqual([]);
});
