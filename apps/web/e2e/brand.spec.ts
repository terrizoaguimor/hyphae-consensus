// SPDX-License-Identifier: Apache-2.0
//
// The Quorum logo in the app (design/logo/quorum-final; src/lib/brand.ts,
// src/components/Quorum.tsx):
//
// - the 56 px top bar shows the flat 32 px mark next to the wordmark, inside
//   the home link, which the wordmark names; the mark takes the wordmark's
//   ink in each theme and is never glass (below 64 px the mark is flat);
// - the brushed-glass mark appears only at 64 px or more (the 404 page and
//   the root page's language chooser), in the file for the theme's surface,
//   chosen by the app's theme (data-theme), not by the OS alone;
// - the static export serves the favicon (SVG and the 16/32 PNGs), the
//   apple-touch icon, the 512/1024 app icons and the Open Graph image, and
//   every page links them, with theme-color in greys;
// - axe finds no violation at all (any impact) on the top bar, the 404 page
//   and a page with the top bar, in both themes.

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";

import { AXE_TAGS } from "./axe-results";
import { expect, hydrated, locales, t, test, useTheme } from "./fixtures";

const themes = ["light", "dark"] as const;

/** R, G, B of a computed "rgb(...)" colour. */
function channels(colour: string): number[] {
  return (colour.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
}

/** The attributes of every <tag ...> in an HTML text (enough for the head's links and metas). */
function tags(html: string, name: string): Record<string, string>[] {
  return [...html.matchAll(new RegExp(`<${name}\\s([^>]*)>`, "g"))].map((match) =>
    Object.fromEntries(
      [...match[1]!.matchAll(/([\w:-]+)="([^"]*)"/g)].map((attribute) => [attribute[1]!, attribute[2]!]),
    ),
  );
}

/** Width and height from a PNG's IHDR. */
function pngSize(body: Buffer): [number, number] {
  expect(body.subarray(0, 8).toString("hex")).toBe("89504e470d0a1a0a");
  expect(body.toString("latin1", 12, 16)).toBe("IHDR");
  return [body.readUInt32BE(16), body.readUInt32BE(20)];
}

async function visibleGlass(page: Page) {
  const images = page.locator('img[src^="/brand/quorum-glass"]');
  const visible: { src: string; width: number; height: number; loaded: boolean }[] = [];
  for (const image of await images.all()) {
    if (await image.isVisible()) {
      const box = await image.boundingBox();
      visible.push({
        src: (await image.getAttribute("src")) ?? "",
        width: box?.width ?? 0,
        height: box?.height ?? 0,
        loaded: await image.evaluate((element) => (element as HTMLImageElement).naturalWidth > 0),
      });
    }
  }
  return { count: await images.count(), visible };
}

for (const theme of themes) {
  test(`the top bar shows the flat Quorum mark, named by the wordmark (${theme})`, async ({ page }) => {
    await useTheme(page, theme);
    const inks = new Set<string>();
    for (const locale of locales) {
      await page.goto(`/${locale}/`);
      await hydrated(page);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      const banner = page.getByRole("banner");
      await expect(banner).toHaveCount(1);
      expect((await banner.boundingBox())?.height).toBe(56);

      // The home link, whose accessible name is the wordmark, holds the mark.
      const home = banner.getByRole("link", { name: t(locale, "App.name"), exact: true });
      await expect(home).toHaveCount(1);
      await expect(home).toHaveAttribute("href", `/${locale}/`);
      const mark = home.locator('svg[data-quorum="flat-32"]');
      await expect(mark).toBeVisible();
      await expect(mark).toHaveAttribute("aria-hidden", "true");
      const box = await mark.boundingBox();
      expect([box?.width, box?.height]).toEqual([32, 32]);
      // Centred in the 56 px bar.
      const barBox = await banner.boundingBox();
      expect(Math.abs((box?.y ?? 0) + 16 - ((barBox?.y ?? 0) + 28))).toBeLessThanOrEqual(1);
      // The mark comes before the wordmark text, and is the only image in the bar.
      await expect(home.locator(":scope > *").first()).toHaveAttribute("data-quorum", "flat-32");
      await expect(banner.locator('svg[data-quorum], img[src*="quorum"], [data-quorum="glass"]')).toHaveCount(1);
      await expect(banner.locator("img")).toHaveCount(0);

      // One ink: the wordmark's own colour, a grey, dark on light and light on dark.
      const [fill, text] = await mark.evaluate((svg) => [
        getComputedStyle(svg.querySelector("path")!).fill,
        getComputedStyle(svg.closest("a")!).color,
      ]);
      expect(fill).toBe(text);
      const [r, g, b] = channels(fill!);
      expect(r).toBe(g);
      expect(g).toBe(b);
      if (theme === "light") {
        expect(r).toBeLessThan(0x30);
      } else {
        expect(r).toBeGreaterThan(0xd0);
      }
      inks.add(fill!);
    }
    expect(inks.size).toBe(1);
  });

  test(`axe finds no violation on the top bar and the overview (${theme})`, async ({ page }) => {
    await useTheme(page, theme);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await page.goto("/en/");
    await hydrated(page);
    const bar = await new AxeBuilder({ page }).withTags(AXE_TAGS).include("header.hc-topbar").analyze();
    expect(bar.passes.length).toBeGreaterThan(0);
    expect(bar.violations).toEqual([]);
    const whole = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
    expect(whole.violations).toEqual([]);
  });

  test(`the 404 page shows the glass mark for its surface, at 96 px, and axe finds no violation (${theme})`, async ({
    page,
  }) => {
    await useTheme(page, theme);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await page.goto("/404.html");
    await page.waitForLoadState("load");
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const glass = await visibleGlass(page);
    expect(glass.count).toBe(2);
    expect(glass.visible).toEqual([
      { src: `/brand/quorum-glass-64-${theme}.svg`, width: 96, height: 96, loaded: true },
    ]);
    // Decorative: the wordmark beside it names the page.
    for (const image of await page.locator('img[src^="/brand/quorum-glass"]').all()) {
      await expect(image).toHaveAttribute("alt", "");
    }
    await expect(page.locator(".hc-wordmark")).toHaveText(t("en", "App.name"));
    const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
    expect(results.violations).toEqual([]);
  });
}

test("the glass mark follows the app's theme, not the OS alone", async ({ page }) => {
  // The OS says dark, the stored choice says light: the light-surface glass.
  await page.emulateMedia({ colorScheme: "dark" });
  await page.addInitScript(() => window.localStorage.setItem("hc-theme", "light"));
  await page.goto("/404.html");
  await page.waitForLoadState("load");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  expect((await visibleGlass(page)).visible.map(({ src }) => src)).toEqual(["/brand/quorum-glass-64-light.svg"]);
  // Switched by the attribute alone, without a reload.
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
  expect((await visibleGlass(page)).visible.map(({ src }) => src)).toEqual(["/brand/quorum-glass-64-dark.svg"]);
});

test("no page shows the glass mark below 64 px", async ({ page }) => {
  for (const path of ["/en/", "/en/docs/", "/en/settings/", "/fr/about/"]) {
    await page.goto(path);
    await hydrated(page);
    for (const { width, height } of (await visibleGlass(page)).visible) {
      expect(Math.min(width, height), path).toBeGreaterThanOrEqual(64);
    }
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/en/");
  await hydrated(page);
  const mark = page.getByRole("banner").locator('svg[data-quorum="flat-32"]');
  await expect(mark).toBeVisible();
  expect((await mark.boundingBox())?.width).toBe(32);
});

for (const theme of themes) {
  test(`at 320 px the lockup stays on one line and the bar fits the viewport, in every locale (${theme})`, async ({
    page,
  }) => {
    // WCAG 1.4.10 reflow width: the mark and the wordmark side by side (the
    // README's horizontal lockup, never re-set), the Menu button inside.
    const width = 320;
    await page.setViewportSize({ width, height: 640 });
    await useTheme(page, theme);
    for (const locale of locales) {
      await page.goto(`/${locale}/`);
      await hydrated(page);
      const banner = page.getByRole("banner");
      const home = banner.getByRole("link", { name: t(locale, "App.name"), exact: true });
      const mark = home.locator('svg[data-quorum="flat-32"]');
      const text = home.locator(".hc-wordmark-text");
      const menu = banner.getByRole("button", { name: t(locale, "Shell.menuButton") });
      await expect(menu).toBeVisible();
      const [homeBox, markBox, textBox, menuBox, bannerBox] = await Promise.all(
        [home, mark, text, menu, banner].map((locator) => locator.boundingBox()),
      );
      const where = `${locale} ${theme}`;
      expect([markBox?.width, markBox?.height], where).toEqual([32, 32]);
      expect(homeBox!.height, where).toBeLessThanOrEqual(32);
      // One line of text: no taller than one line box.
      const lineHeight = await text.evaluate((element) => parseFloat(getComputedStyle(element).lineHeight));
      expect(textBox!.height, where).toBeLessThanOrEqual(lineHeight + 1);
      // Side by side, the text after the mark, and clear of the Menu button.
      expect(textBox!.x, where).toBeGreaterThanOrEqual(markBox!.x + 32);
      expect(textBox!.x + textBox!.width, where).toBeLessThanOrEqual(menuBox!.x);
      expect(bannerBox!.height, where).toBe(56);
      expect(bannerBox!.x + bannerBox!.width, where).toBeLessThanOrEqual(width);
      expect(menuBox!.x + menuBox!.width, where).toBeLessThanOrEqual(width);
      expect(homeBox!.x, where).toBeGreaterThanOrEqual(0);
    }
  });
}

test("the static export serves the favicon, the icons and the Open Graph image, and every document links them", async ({
  request,
}) => {
  const files: Record<string, { type: string; size?: [number, number] }> = {
    "/favicon.svg": { type: "image/svg+xml" },
    "/favicon-16.png": { type: "image/png", size: [16, 16] },
    "/favicon-32.png": { type: "image/png", size: [32, 32] },
    "/apple-touch-icon.png": { type: "image/png", size: [180, 180] },
    "/icon-512.png": { type: "image/png", size: [512, 512] },
    "/icon-1024.png": { type: "image/png", size: [1024, 1024] },
    "/icon-512-light.png": { type: "image/png", size: [512, 512] },
    "/icon-1024-light.png": { type: "image/png", size: [1024, 1024] },
    "/og-image.png": { type: "image/png", size: [1200, 630] },
    "/brand/quorum-glass-64-dark.svg": { type: "image/svg+xml" },
    "/brand/quorum-glass-64-light.svg": { type: "image/svg+xml" },
  };
  for (const [path, { type, size }] of Object.entries(files)) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(200);
    expect(response.headers()["content-type"], path).toBe(type);
    const body = await response.body();
    if (size) {
      expect(pngSize(body), path).toEqual(size);
    } else {
      const svg = body.toString("utf8");
      expect(svg, path).toMatch(/^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg"/);
      expect(svg, path).not.toMatch(/\sstyle=|<script/);
    }
  }

  // Every document of the export: the four overviews, a deep page, the root
  // page and the 404 page.
  const documents = [...locales.map((locale) => `/${locale}/`), "/es/docs/modes/", "/", "/404.html"];
  for (const path of documents) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(200);
    const html = await response.text();
    const head = html.slice(0, html.indexOf("</head>"));
    const links = tags(head, "link");
    const icons = links.filter(({ rel }) => rel === "icon").map(({ href, type, sizes }) => ({ href, type, sizes }));
    expect(icons, path).toEqual([
      { href: "/favicon-16.png", type: "image/png", sizes: "16x16" },
      { href: "/favicon-32.png", type: "image/png", sizes: "32x32" },
      { href: "/favicon.svg", type: "image/svg+xml", sizes: "any" },
    ]);
    expect(
      links.filter(({ rel }) => rel === "apple-touch-icon").map(({ href, sizes }) => ({ href, sizes })),
      path,
    ).toEqual([{ href: "/apple-touch-icon.png", sizes: "180x180" }]);
    // The web app manifest (app/manifest.ts), linked from every document.
    expect(links.filter(({ rel }) => rel === "manifest").map(({ href }) => href), path).toEqual([
      "/manifest.webmanifest",
    ]);
    const metas = tags(head, "meta");
    expect(
      metas.filter(({ name }) => name === "theme-color").map(({ media, content }) => ({ media, content })),
      path,
    ).toEqual([
      { media: "(prefers-color-scheme: light)", content: "#F4F4F4" },
      { media: "(prefers-color-scheme: dark)", content: "#0B0B0B" },
    ]);
    if (path !== "/404.html") {
      const og = Object.fromEntries(
        metas.filter(({ property }) => property?.startsWith("og:")).map(({ property, content }) => [property, content]),
      );
      const locale = locales.find((candidate) => path.startsWith(`/${candidate}/`)) ?? "en";
      // The root page keeps the layout-wide tags (same-origin og:image when
      // the build has no origin); the localized pages carry per-page tags,
      // checked in full by e2e/seo.spec.ts, with an absolute og:image.
      const localized = path !== "/";
      expect(og, path).toMatchObject({
        "og:type": "website",
        "og:site_name": t(locale, "App.name"),
        "og:image": localized ? "https://consensus.terrizoaguimor.dev/og-image.png" : "/og-image.png",
        "og:image:type": "image/png",
        "og:image:width": "1200",
        "og:image:height": "630",
        "og:image:alt": t(locale, "Metadata.ogImageAlt"),
      });
      expect(metas.filter(({ name }) => name === "twitter:card").map(({ content }) => content), path).toEqual([
        "summary_large_image",
      ]);
      // Never Next's localhost fallback for a missing metadataBase.
      expect(head, path).not.toContain("localhost");
    }
  }

  // The language chooser (root page) carries the glass mark, both surfaces.
  const root = await (await request.get("/")).text();
  expect(root).toContain('src="/brand/quorum-glass-64-light.svg"');
  expect(root).toContain('src="/brand/quorum-glass-64-dark.svg"');
});
