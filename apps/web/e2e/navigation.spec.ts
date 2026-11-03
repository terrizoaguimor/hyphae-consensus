// SPDX-License-Identifier: Apache-2.0
//
// Navigation: every primary nav item in every locale, a crawl of every
// exported page for broken internal links, the skip link, focus after a
// route change, and the mobile drawer.

import {
  SOURCE_REPOSITORY_URL,
  externalLinks,
  localePath,
  primaryNav,
} from "../src/lib/site";

import {
  expect,
  focusInOpenDialog,
  hydrated,
  type Locale,
  locales,
  pagesFor,
  t,
  test,
} from "./fixtures";

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function heading(locale: Locale, key: string): string {
  return key === "overview" ? t(locale, "App.tagline") : t(locale, `Nav.${key}`);
}

for (const locale of locales) {
  test(`every primary nav item renders its page (${locale}) @smoke`, async ({
    page,
  }) => {
    // Start outside the primary nav, so every click is a navigation.
    await page.goto(localePath(locale, "about"));
    await hydrated(page);
    const nav = page.getByRole("navigation", {
      name: t(locale, "Shell.primaryNavLabel"),
    });
    for (const { key, path } of primaryNav) {
      const link = nav.getByRole("link", {
        name: t(locale, `Nav.${key}`),
        exact: true,
      });
      await link.click();
      await expect(page).toHaveURL(
        new RegExp(`${escapeRegExp(localePath(locale, path))}$`),
      );
      const h1 = page.locator("main h1");
      await expect(h1).toHaveText(heading(locale, key));
      await expect(h1).toBeFocused();
      await expect(link).toHaveAttribute("aria-current", "page");
      await expect(page.locator("html")).toHaveAttribute("lang", locale);
      // The overview is the home page: its title is the full headline.
      await expect(page).toHaveTitle(
        key === "overview"
          ? t(locale, "Metadata.homeTitle")
          : `${t(locale, `Nav.${key}`)} · Hyphae Consensus`,
      );
    }
  });
}

test("no broken internal links: every exported link answers 200 without a redirect", async ({
  page,
  request,
  baseURL,
}) => {
  test.setTimeout(600_000);
  const origin = new URL(baseURL ?? "").origin;
  const allowedExternal = new Set<string>(Object.values(externalLinks));
  if (SOURCE_REPOSITORY_URL !== null) {
    // The About page links to the published repository.
    allowedExternal.add(new URL(SOURCE_REPOSITORY_URL).href);
  }
  const pages = ["/", ...locales.map((locale) => localePath(locale, ""))];
  const seenPages = new Set(pages);
  const checkedLinks = new Set<string>();
  const external = new Set<string>();
  let visited = 0;

  while (pages.length > 0) {
    const path = pages.shift() ?? "/";
    let hrefs: string[];
    if (path === "/") {
      // The root page redirects as soon as it runs; read its HTML instead.
      const response = await request.get(path, { maxRedirects: 0 });
      expect(response.status(), path).toBe(200);
      const html = await response.text();
      hrefs = [...html.matchAll(/<a\b[^>]*\bhref="([^"]+)"/g)].map(
        (match) => new URL(match[1] ?? "", `${origin}/`).href,
      );
    } else {
      const response = await page.goto(path);
      expect(response?.status(), path).toBe(200);
      await hydrated(page);
      await expect(page.locator("main h1"), path).toHaveCount(1);
      await expect(page.getByRole("main"), path).toHaveCount(1);
      const locale = path.split("/")[1] as Locale;
      await expect(
        page.getByText(t(locale, "Footer.nonAffiliation"), { exact: true }),
        path,
      ).toBeVisible();
      hrefs = await page.$$eval("a[href]", (anchors) =>
        anchors.map((anchor) => (anchor as HTMLAnchorElement).href),
      );
    }
    visited += 1;

    for (const href of hrefs) {
      const url = new URL(href);
      if (url.origin !== origin) {
        external.add(url.href);
        continue;
      }
      const target = url.pathname;
      if (!checkedLinks.has(target)) {
        checkedLinks.add(target);
        const response = await request.get(target, { maxRedirects: 0 });
        expect(response.status(), `${path} links to ${target}`).toBe(200);
      }
      if (target.endsWith("/") && !seenPages.has(target)) {
        seenPages.add(target);
        pages.push(target);
      }
    }
  }

  // 1 root page + 17 pages in each of the 4 locales.
  expect(visited).toBe(1 + locales.length * pagesFor("en").length);
  for (const locale of locales) {
    for (const { path } of pagesFor(locale)) {
      expect(seenPages.has(path), `${path} is linked from somewhere`).toBe(true);
    }
  }
  for (const href of external) {
    expect(allowedExternal.has(href), `unexpected external link ${href}`).toBe(
      true,
    );
  }
});

test("an unknown path answers 404 with the global not-found page", async ({
  request,
}) => {
  for (const path of ["/en/does-not-exist/", "/de/", "/nothing"]) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(404);
    expect(response.headers()["content-security-policy"], path).toContain(
      "script-src 'self'",
    );
    const html = await response.text();
    expect(html, path).toContain('<html lang="en"');
    expect(html, path).toContain(t("en", "NotFound.title"));
    for (const locale of locales) {
      expect(html, `${path} links to /${locale}/`).toContain(`href="/${locale}/"`);
    }
    expect(html, path).toContain(t("en", "Footer.nonAffiliation"));
  }
});

test("the skip link comes first and moves focus to the main content", async ({
  page,
  browserName,
}) => {
  test.skip(browserName === "webkit", "WebKit does not Tab to links by default");
  await page.goto("/en/docs/");
  await hydrated(page);
  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: t("en", "Shell.skipToContent") });
  await expect(skip).toBeFocused();
  await expect(skip).toBeVisible();
  const box = await skip.boundingBox();
  expect(box?.width ?? 0).toBeGreaterThan(20);
  await page.keyboard.press("Enter");
  await expect(page.locator("#main")).toBeFocused();
});

test("landmarks: banner, primary navigation, main, footer, language", async ({
  page,
}) => {
  await page.goto("/en/settings/");
  await hydrated(page);
  await expect(page.getByRole("banner")).toHaveCount(1);
  await expect(page.getByRole("main")).toHaveCount(1);
  await expect(page.getByRole("contentinfo")).toHaveCount(1);
  await expect(page.getByRole("navigation", { name: "Primary" })).toHaveCount(1);
  await expect(page.getByRole("navigation", { name: "Language" })).toHaveCount(1);
  await expect(
    page.getByRole("navigation", { name: t("en", "Shell.footerNavLabel") }),
  ).toHaveCount(1);
});

test("a docs article marks Docs as the current section and links its neighbors", async ({
  page,
}) => {
  await page.goto("/en/docs/not-voting/");
  await hydrated(page);
  const nav = page.getByRole("navigation", { name: "Primary" });
  await expect(nav.getByRole("link", { name: "Docs" })).toHaveAttribute(
    "aria-current",
    "true",
  );
  const pager = page.getByRole("navigation", { name: t("en", "Docs.pagerLabel") });
  await pager
    .getByRole("link", {
      name: t("en", "Docs.next", {
        title: t("en", "Docs.articles.verified-evidence.title"),
      }),
    })
    .click();
  await expect(page).toHaveURL(/\/en\/docs\/verified-evidence\/$/);
  await expect(page.locator("main h1")).toBeFocused();
});

test.describe("mobile", () => {
  test("the Menu button opens a modal drawer that traps focus and closes @mobile", async ({
    page,
  }) => {
    await page.goto("/en/");
    await hydrated(page);
    await expect(page.getByRole("navigation", { name: "Primary" })).toHaveCount(0);
    const button = page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true });
    await expect(button).toHaveAttribute("aria-expanded", "false");
    await button.click();
    const drawer = page.getByRole("dialog", { name: t("en", "Shell.menuTitle") });
    await expect(drawer).toBeVisible();
    await expect(button).toHaveAttribute("aria-expanded", "true");
    expect(await focusInOpenDialog(page)).toBe(true);
    for (let step = 0; step < 16; step += 1) {
      await page.keyboard.press("Tab");
      expect(await focusInOpenDialog(page), `Tab ${step + 1}`).toBe(true);
    }
    for (let step = 0; step < 4; step += 1) {
      await page.keyboard.press("Shift+Tab");
      expect(await focusInOpenDialog(page), `Shift+Tab ${step + 1}`).toBe(true);
    }
    await page.keyboard.press("Escape");
    await expect(drawer).toBeHidden();
    await expect(button).toBeFocused();
    await expect(button).toHaveAttribute("aria-expanded", "false");

    await button.click();
    await drawer
      .getByRole("navigation", { name: "Primary" })
      .getByRole("link", { name: "Docs" })
      .click();
    await expect(page).toHaveURL(/\/en\/docs\/$/);
    await expect(drawer).toBeHidden();
    await expect(page.locator("main h1")).toBeFocused();
  });

  test("the drawer holds the language links and the theme control @mobile", async ({
    page,
  }) => {
    await page.goto("/en/docs/modes/");
    await hydrated(page);
    await page.getByRole("button", { name: t("en", "Shell.menuButton"), exact: true }).click();
    const drawer = page.getByRole("dialog", { name: t("en", "Shell.menuTitle") });
    await drawer
      .getByRole("radiogroup", { name: t("en", "Theme.label") })
      .getByRole("radio", { name: t("en", "Theme.options.dark"), exact: true })
      .click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await drawer
      .getByRole("navigation", { name: "Language" })
      .getByRole("link", { name: "Français" })
      .click();
    await expect(page).toHaveURL(/\/fr\/docs\/modes\/$/);
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });
});
