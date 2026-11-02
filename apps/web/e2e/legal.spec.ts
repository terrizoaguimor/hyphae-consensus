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
    // Two pages show the same day exactly when their texts changed on the
    // same day (tests/legal-dates.test.ts pins which day that is): the
    // Terms, Privacy and Limitations changed on 2026-11-01, and About too
    // (round 10: the model split of 2026-11-01 changed its "Independent" and
    // "AI assistance" sections; it had shown 2026-10-27 until then). (The
    // earlier fixed pairs here went stale when the Terms and the Privacy
    // notice were re-dated on 2026-11-01.)
    for (let a = 0; a < documents.length; a += 1) {
      for (let b = a + 1; b < documents.length; b += 1) {
        const sameDay =
          LEGAL_TEXT_CHANGED[documents[a] ?? "terms"].getTime() ===
          LEGAL_TEXT_CHANGED[documents[b] ?? "terms"].getTime();
        expect(shown[a] === shown[b], `${documents[a]} and ${documents[b]}`).toBe(sameDay);
      }
    }
    // About now shows the same day as Terms, and the About page shows
    // 2026-11-01 itself (formatted here from a literal day, not from
    // LEGAL_TEXT_CHANGED, so the rendered page is what is checked).
    expect(shown[3]).toBe(shown[0]);
    const november1 = new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "long",
      day: "numeric",
      timeZone: "UTC",
    }).format(new Date(Date.UTC(2026, 10, 1)));
    expect(shown[3]).toBe(t(locale, "Legal.updated").replace("{date, date, long}", november1));
  });
}
