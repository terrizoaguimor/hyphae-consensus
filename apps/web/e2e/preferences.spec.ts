// SPDX-License-Identifier: Apache-2.0
//
// Preferences: the header theme toggle, the Settings page (theme, language,
// motion, explanation level, reset) with its live-region feedback, the
// locale switcher, and the rule that everything stays in localStorage: no
// cookie is ever set.

import {
  expect,
  hydrated,
  storedKeys,
  t,
  test,
} from "./fixtures";

test("the header theme control switches, persists and sets no cookie @smoke", async ({
  page,
  context,
}) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/en/");
  await hydrated(page);
  const html = page.locator("html");
  await expect(html).toHaveAttribute("data-theme", "light");
  // The System / Light / Dark control (a radio group) replaced the Switch.
  const control = page
    .locator(".hc-topbar-controls")
    .getByRole("radiogroup", { name: t("en", "Theme.label") });
  const option = (name: "system" | "light" | "dark") =>
    control.getByRole("radio", { name: t("en", `Theme.options.${name}`), exact: true });
  await expect(page.getByRole("switch")).toHaveCount(0);
  await expect(option("system")).toBeChecked();
  await expect(option("dark")).not.toBeChecked();
  await option("dark").click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  expect(await storedKeys(page)).toEqual({ "hc-theme": "dark" });

  await page.reload();
  await hydrated(page);
  await expect(html).toHaveAttribute("data-theme", "dark");
  await expect(option("dark")).toBeChecked();
  await expect(option("system")).not.toBeChecked();

  await option("light").click();
  await expect(html).toHaveAttribute("data-theme", "light");
  expect(await storedKeys(page)).toEqual({ "hc-theme": "light" });

  // System removes the stored choice; the OS decides again.
  await option("system").click();
  expect(await storedKeys(page)).toEqual({});
  await expect(html).toHaveAttribute("data-theme", "light");
  expect(await context.cookies()).toEqual([]);
  expect(await page.evaluate(() => document.cookie)).toBe("");
});

test("with no stored choice the theme follows the OS, live", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/en/");
  await hydrated(page);
  const html = page.locator("html");
  await expect(html).toHaveAttribute("data-theme", "dark");
  await page.emulateMedia({ colorScheme: "light" });
  await expect(html).toHaveAttribute("data-theme", "light");
  expect(await storedKeys(page)).toEqual({});
});

test("Settings apply at once, announce each change and persist", async ({
  page,
  context,
}) => {
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "no-preference" });
  await page.goto("/en/settings/");
  await hydrated(page);
  // Astryx buttons carry their own (empty) status regions; this is ours.
  const status = page.locator("p.hc-status[role=status]");
  const html = page.locator("html");
  const group = (label: string) => page.locator("main").getByRole("radiogroup", { name: label });

  await group(t("en", "Settings.theme.label"))
    .getByRole("radio", { name: t("en", "Settings.theme.options.dark") })
    .check();
  await expect(status).toHaveText(
    t("en", "Settings.feedback.theme", {
      value: t("en", "Settings.theme.options.dark"),
    }),
  );
  await expect(html).toHaveAttribute("data-theme", "dark");

  await group(t("en", "Settings.motion.label"))
    .getByRole("radio", { name: t("en", "Settings.motion.options.reduce") })
    .check();
  await expect(status).toHaveText(
    t("en", "Settings.feedback.motion", {
      value: t("en", "Settings.motion.options.reduce"),
    }),
  );
  await expect(html).toHaveAttribute("data-motion", "reduce");

  await group(t("en", "Settings.explanation.label"))
    .getByRole("radio", { name: t("en", "Settings.explanation.options.technical") })
    .check();
  await expect(status).toHaveText(
    t("en", "Settings.feedback.explanation", {
      value: t("en", "Settings.explanation.options.technical"),
    }),
  );
  expect(await storedKeys(page)).toEqual({
    "hc-theme": "dark",
    "hc-motion": "reduce",
    "hc-explanation": "technical",
  });

  // Persisted: a reload shows the same choices.
  await page.reload();
  await hydrated(page);
  await expect(html).toHaveAttribute("data-theme", "dark");
  await expect(html).toHaveAttribute("data-motion", "reduce");
  await expect(
    group(t("en", "Settings.theme.label")).getByRole("radio", {
      name: t("en", "Settings.theme.options.dark"),
    }),
  ).toBeChecked();
  await expect(
    group(t("en", "Settings.motion.label")).getByRole("radio", {
      name: t("en", "Settings.motion.options.reduce"),
    }),
  ).toBeChecked();
  await expect(
    group(t("en", "Settings.explanation.label")).getByRole("radio", {
      name: t("en", "Settings.explanation.options.technical"),
    }),
  ).toBeChecked();

  // The technical level opens technical details in Docs; simple closes them.
  await page.goto("/en/docs/modes/");
  await hydrated(page);
  await expect(page.locator("details.hc-details")).toHaveAttribute("open", "");
  await page.goto("/en/settings/");
  await hydrated(page);
  await group(t("en", "Settings.explanation.label"))
    .getByRole("radio", { name: t("en", "Settings.explanation.options.simple") })
    .check();
  await page.goto("/en/docs/modes/");
  await hydrated(page);
  await expect(page.locator("details.hc-details")).not.toHaveAttribute("open");

  // Remove everything.
  await page.goto("/en/settings/");
  await hydrated(page);
  await page.getByRole("button", { name: t("en", "Settings.reset.button") }).click();
  await expect(status).toHaveText(t("en", "Settings.feedback.reset"));
  expect(await storedKeys(page)).toEqual({});
  await expect(html).not.toHaveAttribute("data-motion");
  await expect(
    group(t("en", "Settings.theme.label")).getByRole("radio", {
      name: t("en", "Settings.theme.options.system"),
    }),
  ).toBeChecked();
  expect(await context.cookies()).toEqual([]);
});

test("Match system follows the OS after an explicit theme", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/en/settings/");
  await hydrated(page);
  const themes = page.locator("main").getByRole("radiogroup", { name: t("en", "Settings.theme.label") });
  await themes.getByRole("radio", { name: t("en", "Settings.theme.options.light") }).check();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await themes.getByRole("radio", { name: t("en", "Settings.theme.options.system") }).check();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(await storedKeys(page)).toEqual({});
});

test("a language chosen in Settings opens Settings in that language and is confirmed there", async ({
  page,
}) => {
  await page.goto("/en/settings/");
  await hydrated(page);
  const section = page.locator("section", {
    has: page.getByRole("heading", { name: t("en", "Settings.language.label") }),
  });
  await section.getByRole("link", { name: "Français" }).click();
  await expect(page).toHaveURL(/\/fr\/settings\/$/);
  await hydrated(page);
  await expect(page.locator("html")).toHaveAttribute("lang", "fr");
  await expect(page.locator("p.hc-status[role=status]")).toHaveText(
    t("fr", "Settings.feedback.language", { value: "Français" }),
  );
  expect(await storedKeys(page)).toEqual({ "hc-locale": "fr" });
});

test("the locale switcher keeps the page and the root page opens the chosen language @smoke", async ({
  page,
}) => {
  await page.goto("/en/docs/modes/");
  await hydrated(page);
  await page
    .getByRole("navigation", { name: t("en", "LocaleSwitcher.label") })
    .getByRole("link", { name: "Español" })
    .click();
  await expect(page).toHaveURL(/\/es\/docs\/modes\/$/);
  await expect(page.locator("html")).toHaveAttribute("lang", "es");
  await expect(page.locator("main h1")).toHaveText(t("es", "Docs.articles.modes.title"));
  expect(await storedKeys(page)).toEqual({ "hc-locale": "es" });

  await page.goto("/");
  await expect(page).toHaveURL(/\/es\/$/);
});

test.describe("the root page without a stored choice", () => {
  test.use({ locale: "pt-PT" });

  test("negotiates the browser language", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/pt-BR\/$/);
  });
});

test("every locale renders its own catalog in the shell", async ({ page }) => {
  for (const locale of ["en", "es", "pt-BR", "fr"] as const) {
    await page.goto(`/${locale}/settings/`);
    await hydrated(page);
    await expect(page.locator("main h1")).toHaveText(t(locale, "Nav.settings"));
    await expect(
      page.getByText(t(locale, "Footer.nonAffiliation"), { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("link", { name: t(locale, "Shell.skipToContent") }),
    ).toHaveCount(1);
  }
});
