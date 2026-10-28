// SPDX-License-Identifier: Apache-2.0
//
// Reduced motion and reduced transparency. With the OS preference or the
// in-app "Reduced" setting, overlays lose their movement (no transform in
// the transition, no offset) and keep a short fade; "Match system" follows
// the OS live. Reduced transparency (emulated through Chromium's DevTools
// protocol; Playwright has no option for it) makes the glass solid.

import type { Page } from "@playwright/test";

import { expect, hydrated, t, test } from "./fixtures";

async function openTour(page: Page) {
  await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
  const dialog = page.locator("dialog[open]");
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveCSS("opacity", "1");
  return dialog;
}

async function transitionOf(page: Page) {
  return page.locator("dialog[open]").evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      properties: style.transitionProperty,
      duration: Number.parseFloat(style.transitionDuration) * 1000,
    };
  });
}

test("the OS reduced-motion preference takes the movement out of overlays", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/en/docs/");
  await hydrated(page);
  await openTour(page);
  const normal = await transitionOf(page);
  expect(normal.properties).toContain("transform");
  await page.keyboard.press("Escape");

  await page.emulateMedia({ reducedMotion: "reduce" });
  await openTour(page);
  const reduced = await transitionOf(page);
  expect(reduced.properties).not.toContain("transform");
  expect(reduced.properties).toContain("opacity");
  expect(reduced.duration).toBeLessThanOrEqual(150);
});

test("the in-app Reduced setting does the same without the OS preference", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.addInitScript(() => {
    window.localStorage.setItem("hc-motion", "reduce");
  });
  await page.goto("/en/docs/");
  await hydrated(page);
  await expect(page.locator("html")).toHaveAttribute("data-motion", "reduce");
  await openTour(page);
  const reduced = await transitionOf(page);
  expect(reduced.properties).not.toContain("transform");
  expect(reduced.duration).toBeLessThanOrEqual(150);

  // Other transitions are cut to (almost) nothing.
  const navLink = await page
    .locator(".hc-nav-link")
    .first()
    .evaluate((element) => Number.parseFloat(getComputedStyle(element).transitionDuration));
  expect(navLink).toBeLessThan(0.001);
});

test("Match system follows the OS preference as it changes", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/en/docs/");
  await hydrated(page);
  await expect(page.locator("html")).not.toHaveAttribute("data-motion");
  await openTour(page);
  expect((await transitionOf(page)).properties).not.toContain("transform");
  await page.emulateMedia({ reducedMotion: "no-preference" });
  expect((await transitionOf(page)).properties).toContain("transform");
});

test("the drawer slides in normally and only fades with reduced motion @mobile", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/en/");
  await hydrated(page);
  const button = page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true });
  await button.click();
  await expect(page.locator("dialog[open]")).toHaveCSS("opacity", "1");
  expect((await transitionOf(page)).properties).toContain("transform");
  await page.keyboard.press("Escape");

  await page.emulateMedia({ reducedMotion: "reduce" });
  await button.click();
  await expect(page.locator("dialog[open]")).toHaveCSS("opacity", "1");
  expect((await transitionOf(page)).properties).not.toContain("transform");
});

test("reduced transparency makes the frosted glass solid", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "emulated through Chromium's DevTools protocol");
  await page.goto("/en/");
  await hydrated(page);
  const topbar = page.locator(".hc-topbar");
  const glass = await topbar.evaluate((element) => {
    const style = getComputedStyle(element);
    return { filter: style.backdropFilter, background: style.backgroundColor };
  });
  expect(glass.filter).toContain("blur");

  const session = await page.context().newCDPSession(page);
  await session.send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-transparency", value: "reduce" }],
  });
  const solid = await topbar.evaluate((element) => {
    const style = getComputedStyle(element);
    return { filter: style.backdropFilter, background: style.backgroundColor };
  });
  expect(solid.filter).toBe("none");
  // An opaque color: rgb(...) or an alpha of 1.
  expect(solid.background).toMatch(/^rgb\(|^rgba\([^)]*,\s*1\)$|^color\(srgb [^/]*\)$/);
});
