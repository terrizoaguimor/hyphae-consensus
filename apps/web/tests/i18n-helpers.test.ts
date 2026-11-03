// SPDX-License-Identifier: Apache-2.0

import { describe, expect, it } from "vitest";

import { negotiateLocale } from "../src/i18n/negotiate";
import { switchLocalePath, withLocation } from "../src/i18n/paths";

const locales = ["en", "es", "pt-BR", "fr"] as const;

describe("negotiateLocale", () => {
  const pick = (...preferred: string[]) =>
    negotiateLocale(preferred, locales, "en");

  it("prefers an exact tag, case-insensitively", () => {
    expect(pick("pt-br")).toBe("pt-BR");
    expect(pick("FR")).toBe("fr");
  });

  it("falls back to the primary language", () => {
    expect(pick("es-MX")).toBe("es");
    expect(pick("pt-PT", "en")).toBe("pt-BR");
    expect(pick("fr-CA")).toBe("fr");
  });

  it("keeps the visitor's order and defaults to English", () => {
    expect(pick("de-DE", "fr", "es")).toBe("fr");
    expect(pick("de", "ja")).toBe("en");
    expect(pick()).toBe("en");
  });
});

describe("switchLocalePath", () => {
  it("replaces the locale segment and keeps the rest of the path", () => {
    expect(switchLocalePath("/en/", "es", locales)).toBe("/es/");
    expect(switchLocalePath("/en", "fr", locales)).toBe("/fr/");
    expect(switchLocalePath("/pt-BR/runs/abc/", "en", locales)).toBe(
      "/en/runs/abc/",
    );
  });

  it("prefixes the locale when the path has none", () => {
    expect(switchLocalePath("/", "fr", locales)).toBe("/fr/");
    expect(switchLocalePath(null, "es", locales)).toBe("/es/");
  });
});

describe("withLocation", () => {
  it("carries the query string and the hash over", () => {
    expect(withLocation("/es/run/", "?id=run-1&source=showcase", "")).toBe(
      "/es/run/?id=run-1&source=showcase",
    );
    expect(withLocation("/es/run/", "?id=run-1", "#evidence")).toBe(
      "/es/run/?id=run-1#evidence",
    );
    expect(withLocation("/fr/docs/", "", "#top")).toBe("/fr/docs/#top");
  });

  it("adds nothing when the URL has no query or hash", () => {
    expect(withLocation("/es/docs/", "", "")).toBe("/es/docs/");
    expect(withLocation("/es/docs/", "?", "#")).toBe("/es/docs/");
  });
});
