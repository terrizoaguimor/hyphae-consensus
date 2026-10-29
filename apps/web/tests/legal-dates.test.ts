// SPDX-License-Identifier: Apache-2.0
//
// Each legal page (Terms, Privacy, Limitations, About) shows the day its own
// text last changed (LEGAL_TEXT_CHANGED in src/lib/site.ts), never a date
// shared with the others. Each date is pinned here to a SHA-256 of what its
// document shows in all four locales: its Legal.<page> or About subtree of
// messages/legal/<locale>.json, its title (Nav.<page> in
// messages/<locale>.json), and the value of every ICU argument that text
// interpolates from src/lib/site.ts (the contact address and the hosting
// country, as LegalPageContent passes them). When any of it changes, its
// digest below no longer matches, and the test fails until the date in
// site.ts is set to the day of that change and the digest here is updated
// with it. A change to one document never re-dates another.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import { CONTACT_EMAIL, HOSTING_COUNTRY, LEGAL_TEXT_CHANGED, legalPages } from "../src/lib/site";

type Catalog = { [key: string]: string | Catalog };

const documents = [...legalPages, "about"] as const;
type LegalDocument = (typeof documents)[number];

/** The day each document's text last changed, with the digest of that text. */
const PINNED: Record<LegalDocument, { date: string; sha256: string }> = {
  terms: { date: "2026-10-27", sha256: "eea6b0f1328cae634a9ef5eb1afe83d52976820766164eff50ecf101686e934f" },
  privacy: { date: "2026-10-27", sha256: "99368e85299ae9803a131c74244a515f53f0a674c50cf7e33f811d90a2b8a2f8" },
  limitations: { date: "2026-10-28", sha256: "07e65f817a05a539dc9e4518bce5c5c49cd777c0db01fd0bc17c2d2e8f879d6b" },
  about: { date: "2026-10-27", sha256: "31f3dedb6371401c7e439f34bd4a538ee9cd0b39f437c2c7537230fc93a3c5be" },
};

/** The ICU arguments LegalPageContent passes to every document's sections. */
const DOCUMENT_ARGUMENTS: Readonly<Record<string, string>> = {
  contact: CONTACT_EMAIL,
  hostingCountry: HOSTING_COUNTRY,
};

function readCatalog(file: string): Catalog {
  const url = new URL(`../messages/${file}.json`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8")) as Catalog;
}

function documentText(page: LegalDocument, locale: string): Catalog | string | undefined {
  const catalog = readCatalog(`legal/${locale}`);
  if (page === "about") {
    return catalog.About;
  }
  const legal = catalog.Legal;
  return typeof legal === "object" ? legal[page] : undefined;
}

/** The document's title, the page heading LegalPageContent renders. */
function documentTitle(page: LegalDocument, locale: string): Catalog | string | undefined {
  const nav = readCatalog(locale).Nav;
  return typeof nav === "object" ? nav[page] : undefined;
}

/** The arguments the document's text interpolates, with the values it shows. */
function documentArguments(text: unknown): [string, string][] {
  const serialized = JSON.stringify(text);
  return Object.entries(DOCUMENT_ARGUMENTS)
    .filter(([name]) => serialized.includes(`{${name}}`))
    .sort(([a], [b]) => a.localeCompare(b));
}

/**
 * SHA-256 of what the document shows in every locale, in a fixed order: its
 * title, its catalog text, and the values of the arguments that text uses.
 */
function documentDigest(page: LegalDocument): string {
  const locales = [...routing.locales].sort();
  const shown = locales.map((locale) => {
    const text = documentText(page, locale);
    return [locale, documentTitle(page, locale), text, documentArguments(text)];
  });
  return createHash("sha256").update(JSON.stringify(shown)).digest("hex");
}

describe("legal page dates", () => {
  it("dates every legal document on its own", () => {
    expect(Object.keys(LEGAL_TEXT_CHANGED).sort()).toEqual([...documents].sort());
    for (const page of documents) {
      expect(documentText(page, "en"), page).toBeTypeOf("object");
      expect(documentTitle(page, "en"), page).toBeTypeOf("string");
    }
  });

  it.each(documents)("%s shows the day its own text last changed", (page) => {
    expect(LEGAL_TEXT_CHANGED[page].toISOString().slice(0, 10)).toBe(PINNED[page].date);
    expect(
      documentDigest(page),
      `the ${page} text changed: set LEGAL_TEXT_CHANGED.${page} in src/lib/site.ts to the day of the change, then pin its new digest here`,
    ).toBe(PINNED[page].sha256);
  });

  it("counts the site values a document shows as part of its text", () => {
    // Terms names the contact address; Privacy the contact address and the
    // hosting country. A change to either value changes those documents.
    expect(documentArguments(documentText("terms", "en")).map(([name]) => name)).toEqual(["contact"]);
    expect(documentArguments(documentText("privacy", "en")).map(([name]) => name)).toEqual([
      "contact",
      "hostingCountry",
    ]);
    for (const locale of routing.locales) {
      for (const page of documents) {
        expect(documentArguments(documentText(page, locale)), `${page} ${locale}`).toEqual(
          documentArguments(documentText(page, "en")),
        );
      }
    }
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
