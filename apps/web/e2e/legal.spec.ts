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
    // Terms and Privacy changed together (2026-11-01: the contact address and
    // the hosting country); Limitations (2026-10-29) and About (2026-10-27)
    // each on a day of their own.
    expect(shown[0]).toBe(shown[1]);
    expect(shown[2]).not.toBe(shown[0]);
    expect(shown[3]).not.toBe(shown[0]);
    expect(shown[3]).not.toBe(shown[2]);
  });
}
