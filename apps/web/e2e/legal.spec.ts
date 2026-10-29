// SPDX-License-Identifier: Apache-2.0
//
// Each legal page (Terms, Privacy, Limitations, About) shows the day its own
// text last changed (LEGAL_TEXT_CHANGED in src/lib/site.ts), formatted per
// locale in UTC; a change to one document never re-dates another
// (tests/legal-dates.test.ts pins each date to its document's text).

import { LEGAL_TEXT_CHANGED, localePath } from "../src/lib/site";

import { expect, hydrated, locales, t, test } from "./fixtures";

const documents = ["terms", "privacy", "limitations", "about"] as const;

for (const locale of locales) {
  test(`each legal page shows the day its own text last changed (${locale})`, async ({ page }) => {
    const shown: string[] = [];
    for (const document of documents) {
      const date = new Intl.DateTimeFormat(locale, {
        year: "numeric",
        month: "long",
        day: "numeric",
        timeZone: "UTC",
      }).format(LEGAL_TEXT_CHANGED[document]);
      const expected = t(locale, "Legal.updated").replace("{date, date, long}", date);
      await page.goto(localePath(locale, document));
      await hydrated(page);
      await expect(page.locator("main").getByText(expected, { exact: true })).toBeVisible();
      shown.push(expected);
    }
    // Limitations changed on a day Terms, Privacy and About did not.
    expect(shown[2]).not.toBe(shown[0]);
    expect(shown[0]).toBe(shown[1]);
    expect(shown[0]).toBe(shown[3]);
  });
}
