// SPDX-License-Identifier: Apache-2.0
//
// Each legal page (Terms, Privacy, Limitations, About) shows the day its own
// text last changed (LEGAL_TEXT_CHANGED in src/lib/site.ts), never a date
// shared with the others. Each date is pinned here to a SHA-256 of its
// document's text in all four locales (its Legal.<page> or About subtree of
// messages/legal/<locale>.json): when a document's text changes, its digest
// below no longer matches, and the test fails until the date in site.ts is
// set to the day of that change and the digest here is updated with it. A
// change to one document never re-dates another.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import { LEGAL_TEXT_CHANGED, legalPages } from "../src/lib/site";

type Catalog = { [key: string]: string | Catalog };

const documents = [...legalPages, "about"] as const;
type LegalDocument = (typeof documents)[number];

/** The day each document's text last changed, with the digest of that text. */
const PINNED: Record<LegalDocument, { date: string; sha256: string }> = {
  terms: { date: "2026-10-27", sha256: "3740c519eec1141ac59c8cfad43fb66a52c4a9260050a2c50bfb2ebe0f21b960" },
  privacy: { date: "2026-10-27", sha256: "730cd623538eb38d9fbb65d3f8fd2db8121a5518d08fa549ddbdad93d9523414" },
  limitations: { date: "2026-10-28", sha256: "d514d95006770fd7b27b4e2e0635bb5143f7732f9fb21a60e5d95c8d277939d1" },
  about: { date: "2026-10-27", sha256: "029fa44967102b6041534f40679797b046d4a773e17b2d2f94c483ec8625de80" },
};

function legalCatalog(locale: string): Catalog {
  const url = new URL(`../messages/legal/${locale}.json`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8")) as Catalog;
}

function documentText(page: LegalDocument, locale: string): Catalog | string | undefined {
  const catalog = legalCatalog(locale);
  if (page === "about") {
    return catalog.About;
  }
  const legal = catalog.Legal;
  return typeof legal === "object" ? legal[page] : undefined;
}

/** SHA-256 of the document's text in every locale, in a fixed order. */
function documentDigest(page: LegalDocument): string {
  const locales = [...routing.locales].sort();
  const text = locales.map((locale) => [locale, documentText(page, locale)]);
  return createHash("sha256").update(JSON.stringify(text)).digest("hex");
}

describe("legal page dates", () => {
  it("dates every legal document on its own", () => {
    expect(Object.keys(LEGAL_TEXT_CHANGED).sort()).toEqual([...documents].sort());
    for (const page of documents) {
      expect(documentText(page, "en"), page).toBeTypeOf("object");
    }
  });

  it.each(documents)("%s shows the day its own text last changed", (page) => {
    expect(LEGAL_TEXT_CHANGED[page].toISOString().slice(0, 10)).toBe(PINNED[page].date);
    expect(
      documentDigest(page),
      `the ${page} text changed: set LEGAL_TEXT_CHANGED.${page} in src/lib/site.ts to the day of the change, then pin its new digest here`,
    ).toBe(PINNED[page].sha256);
  });

  it("does not re-date Terms and Privacy for a change to Limitations only", () => {
    // 2026-10-28 (e0f0ebd) changed one line of Limitations and no other legal text.
    expect(LEGAL_TEXT_CHANGED.limitations.toISOString().slice(0, 10)).toBe("2026-10-28");
    expect(LEGAL_TEXT_CHANGED.terms.toISOString().slice(0, 10)).toBe("2026-10-27");
    expect(LEGAL_TEXT_CHANGED.privacy.toISOString().slice(0, 10)).toBe("2026-10-27");
    expect(LEGAL_TEXT_CHANGED.about.toISOString().slice(0, 10)).toBe("2026-10-27");
  });

  it("dates each day at midnight UTC, the time zone the pages format in", () => {
    for (const page of documents) {
      expect(LEGAL_TEXT_CHANGED[page].getTime() % 86_400_000, page).toBe(0);
    }
  });
});
