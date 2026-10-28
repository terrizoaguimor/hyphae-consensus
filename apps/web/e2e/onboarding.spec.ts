// SPDX-License-Identifier: Apache-2.0
//
// The optional one-minute tour: offered on Overview, never opened by itself,
// skippable (button, Escape, backdrop), remembered in localStorage, always
// reachable from Docs, a modal dialog with a strict focus trap.

import {
  expect,
  focusInOpenDialog,
  hydrated,
  locales,
  storedKeys,
  t,
  test,
} from "./fixtures";

const steps = ["welcome", "guided", "live", "runs", "labels", "decide"];

test("the tour is offered, never forced, and Skip remembers it @smoke", async ({
  page,
}) => {
  await page.goto("/en/");
  await hydrated(page);
  // Offered, not opened.
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const start = page.getByRole("button", { name: t("en", "Tour.start") });
  await expect(start).toBeVisible();

  await start.click();
  const dialog = page.getByRole("dialog", {
    name: t("en", "Tour.steps.welcome.title"),
  });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".hc-tour-progress")).toHaveText(
    t("en", "Tour.progress", { current: 1, total: 6 }),
  );
  await expect(dialog.getByRole("button", { name: t("en", "Tour.next") })).toBeFocused();

  await dialog.getByRole("button", { name: t("en", "Tour.skip") }).click();
  await expect(dialog).toBeHidden();
  await expect(start).toBeHidden();
  // The invitation went away with the focused button: focus lands on the h1.
  await expect(page.locator("main h1")).toBeFocused();
  expect(await storedKeys(page)).toEqual({ "hc-tour": "skipped" });

  await page.reload();
  await hydrated(page);
  await expect(page.getByRole("button", { name: t("en", "Tour.start") })).toHaveCount(0);
});

test("Next walks all six steps, Back returns, and Finish remembers completion", async ({
  page,
}) => {
  await page.goto("/en/");
  await hydrated(page);
  await page.getByRole("button", { name: t("en", "Tour.start") }).click();
  const dialog = page.locator("dialog[open]");
  const live = dialog.locator(".hc-tour-live");

  for (const [index, step] of steps.entries()) {
    await expect(dialog.locator("h2")).toHaveText(t("en", `Tour.steps.${step}.title`));
    await expect(dialog.locator(".hc-tour-progress")).toHaveText(
      t("en", "Tour.progress", { current: index + 1, total: 6 }),
    );
    await expect(dialog).toHaveAccessibleName(t("en", `Tour.steps.${step}.title`));
    if (index === 2) {
      await dialog.getByRole("button", { name: t("en", "Tour.back") }).click();
      await expect(dialog.locator("h2")).toHaveText(t("en", "Tour.steps.guided.title"));
      await dialog.getByRole("button", { name: t("en", "Tour.next") }).click();
      await expect(dialog.locator("h2")).toHaveText(t("en", "Tour.steps.live.title"));
    }
    const isLast = index === steps.length - 1;
    // Focus stays on the primary button, so Enter keeps advancing.
    const primary = dialog.getByRole("button", {
      name: t("en", isLast ? "Tour.finish" : "Tour.next"),
    });
    await expect(primary).toBeFocused();
    await page.keyboard.press("Enter");
    if (!isLast) {
      const next = steps[index + 1] ?? "";
      await expect(live).toHaveText(
        t("en", "Tour.announcement", {
          current: index + 2,
          total: 6,
          title: t("en", `Tour.steps.${next}.title`),
          body: t("en", `Tour.steps.${next}.body`),
        }),
      );
    }
  }
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(await storedKeys(page)).toEqual({ "hc-tour": "completed" });
  await expect(page.getByRole("button", { name: t("en", "Tour.start") })).toHaveCount(0);
});

test("Escape closes the tour, focus is trapped inside, and returns to the launcher", async ({
  page,
}) => {
  await page.goto("/en/docs/");
  await hydrated(page);
  const launcher = page.getByRole("button", { name: t("en", "Tour.launch") });
  await launcher.click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: t("en", "Tour.next") }).click();

  // Three controls on step 2 (Skip, Back, Next): Tab and Shift+Tab wrap.
  for (let step = 0; step < 7; step += 1) {
    await page.keyboard.press("Tab");
    expect(await focusInOpenDialog(page), `Tab ${step + 1}`).toBe(true);
  }
  for (let step = 0; step < 7; step += 1) {
    await page.keyboard.press("Shift+Tab");
    expect(await focusInOpenDialog(page), `Shift+Tab ${step + 1}`).toBe(true);
  }

  // The page behind is inert while the dialog is open: focus cannot go there.
  await page.evaluate(() => {
    document.querySelector<HTMLElement>(".hc-wordmark")?.focus();
  });
  expect(await focusInOpenDialog(page)).toBe(true);

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(launcher).toBeFocused();
  expect(await storedKeys(page)).toEqual({ "hc-tour": "skipped" });
});

test("a click on the backdrop closes the tour", async ({ page }) => {
  await page.goto("/en/docs/");
  await hydrated(page);
  await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await page.mouse.click(4, 4);
  await expect(dialog).toBeHidden();
});

test("after the tour was closed, Docs still opens it", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem("hc-tour", "completed");
  });
  await page.goto("/en/");
  await hydrated(page);
  await expect(page.getByRole("button", { name: t("en", "Tour.start") })).toHaveCount(0);
  await page.goto("/en/docs/");
  await hydrated(page);
  await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
  await expect(
    page.getByRole("dialog", { name: t("en", "Tour.steps.welcome.title") }),
  ).toBeVisible();
});

for (const locale of locales) {
  test(`the tour is translated (${locale})`, async ({ page }) => {
    await page.goto(`/${locale}/docs/`);
    await hydrated(page);
    await page.getByRole("button", { name: t(locale, "Tour.launch") }).click();
    const dialog = page.getByRole("dialog", {
      name: t(locale, "Tour.steps.welcome.title"),
    });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: t(locale, "Tour.next") })).toBeVisible();
    await expect(dialog.getByRole("button", { name: t(locale, "Tour.skip") })).toBeVisible();
    await expect(dialog).toHaveAccessibleDescription(
      t(locale, "Tour.steps.welcome.body"),
    );
  });
}
