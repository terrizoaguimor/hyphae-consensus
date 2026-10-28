// SPDX-License-Identifier: Apache-2.0
//
// Every error code the contract defines has a sentence in every catalog. The
// list is read from the GENERATED `ErrorCode` union in src/lib/api/types.ts
// (itself checked against contracts/api-v1 by tests/api-types.test.ts), so a
// code the server adds reaches this test with the next `pnpm api:types`, and
// it fails until the four catalogs explain it: no contract code may fall to
// the generic "a code this build does not know" sentence.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import {
  CLIENT_ERROR_CODES,
  errorSentenceKey,
  KNOWN_ERROR_CODES,
  SERVER_ERROR_CODES,
} from "../src/lib/api/errors";

/** The members of `export type ErrorCode = | "a" | "b" ...;` in the generated types. */
function generatedErrorCodes(): string[] {
  const source = readFileSync(new URL("../src/lib/api/types.ts", import.meta.url), "utf8");
  const match = /^export type ErrorCode =([\s\S]*?);$/m.exec(source);
  if (match === null || match[1] === undefined) {
    throw new Error("no ErrorCode union in src/lib/api/types.ts");
  }
  return [...match[1].matchAll(/\|\s*"([a-z_]+)"/g)].map((m) => m[1] ?? "");
}

type Catalog = { [key: string]: string | Catalog };

function errorsOf(locale: string): Catalog {
  const catalog = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  const api = catalog.Api as Catalog;
  return api.errors as Catalog;
}

describe("error sentences", () => {
  const codes = generatedErrorCodes();

  it("reads the contract's codes from the generated union", () => {
    expect(codes.length).toBeGreaterThanOrEqual(45);
    expect(codes).toContain("live_concurrency");
    expect(codes).toContain("turnstile_failed");
    expect(new Set(codes).size).toBe(codes.length);
  });

  it("the registry holds exactly the generated codes", () => {
    expect([...SERVER_ERROR_CODES].sort()).toEqual([...codes].sort());
    expect(KNOWN_ERROR_CODES).toEqual([...SERVER_ERROR_CODES, ...CLIENT_ERROR_CODES]);
  });

  it.each(routing.locales)("%s has a sentence for every contract code and client code", (locale) => {
    const errors = errorsOf(locale);
    for (const code of [...codes, ...CLIENT_ERROR_CODES, "unknown"]) {
      const sentence = errors[code];
      expect(typeof sentence, `${locale} Api.errors.${code}`).toBe("string");
      expect(String(sentence).trim(), `${locale} Api.errors.${code}`).not.toBe("");
    }
    // Nothing else: a sentence for a code that no longer exists is dead text.
    expect(Object.keys(errors).sort()).toEqual(
      [...codes, ...CLIENT_ERROR_CODES, "unknown"].sort(),
    );
  });

  it("the live path's refusals have their own sentence, never the generic one", () => {
    for (const code of [
      "live_concurrency",
      "live_budget_daily",
      "live_tripped",
      "turnstile_required",
      "turnstile_failed",
      "attestation_required",
      "geo_blocked_live",
      "preset_unknown",
    ]) {
      expect(errorSentenceKey(code)).toBe(code);
    }
    expect(errorSentenceKey("unreachable")).toBe("unreachable");
    expect(errorSentenceKey("a_code_from_a_newer_server")).toBe("unknown");
  });
});
