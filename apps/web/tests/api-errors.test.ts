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
import { IntlMessageFormat } from "intl-messageformat";

import {
  CLIENT_ERROR_CODES,
  CREATE_SENTENCE_CODES,
  decisionOutcomeUnknown,
  errorHeadlineKey,
  errorSentenceKey,
  errorSentencePath,
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

// Round 4 (HE #18): every error is headed by its consequence, chosen by the
// code's class and the request's context, never by one "refused" headline.
describe("error headlines", () => {
  function apiOf(locale: string): Catalog {
    const catalog = JSON.parse(
      readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
    ) as Catalog;
    return catalog.Api as Catalog;
  }

  // One parsed catalog and one formatter per (locale, key): building them is
  // the slow part, and these tests format the same headlines many times.
  const apis = new Map<string, Catalog>();
  const formatters = new Map<string, IntlMessageFormat>();
  function cachedApi(locale: string): Catalog {
    let api = apis.get(locale);
    if (api === undefined) {
      api = apiOf(locale);
      apis.set(locale, api);
    }
    return api;
  }

  function headline(locale: string, code: string, context: "create" | "decision" | "read", seconds?: number): string {
    const headlines = cachedApi(locale).headlines as Catalog;
    const key = errorHeadlineKey(code, context);
    const source = headlines[key];
    expect(typeof source, `${locale} Api.headlines.${key}`).toBe("string");
    const id = `${locale}\u0000${key}`;
    let formatter = formatters.get(id);
    if (formatter === undefined) {
      formatter = new IntlMessageFormat(String(source), locale);
      formatters.set(id, formatter);
    }
    return String(formatter.format({ seconds: seconds ?? 0 }));
  }

  // Generous timeouts: the first IntlMessageFormat of a locale is slow on a
  // loaded machine (review R1-1); no assertion depends on the time.
  it("gives the 500, the 503 and the 429 cases distinct headlines", { timeout: 30_000 }, () => {
    for (const locale of routing.locales) {
      const internal = headline(locale, "internal", "read");
      const shutting = headline(locale, "shutting_down", "read");
      const rate = headline(locale, "rate_limited", "create", 30);
      const busy = headline(locale, "too_many_runs", "create");
      const live = headline(locale, "live_concurrency", "create");
      expect(new Set([internal, shutting, rate, busy, live]).size, locale).toBe(5);
    }
    expect(errorHeadlineKey("internal")).toBe("serverProblem");
    expect(errorHeadlineKey("shutting_down")).toBe("serverUnavailable");
    expect(errorHeadlineKey("rate_limited")).toBe("rateLimited");
  });

  it("says how long a 429 asks to wait, in every locale", { timeout: 30_000 }, () => {
    expect(headline("en", "rate_limited", "create", 30)).toBe(
      "Too many requests from this connection. Retry in 30 seconds.",
    );
    expect(headline("en", "rate_limited", "create", 1)).toBe(
      "Too many requests from this connection. Retry in 1 second.",
    );
    // A wait that is over (the countdown reached zero) reads as over.
    expect(headline("en", "rate_limited", "create", 0)).toBe(
      "Too many requests from this connection. You can retry now.",
    );
    for (const locale of routing.locales) {
      expect(headline(locale, "rate_limited", "create", 27), locale).toMatch(/27/u);
      expect(headline(locale, "rate_limited", "create", 0), locale).not.toMatch(/\d/u);
      expect(typeof (apiOf(locale).headlines as Catalog).rateLimitedNoWait, locale).toBe("string");
    }
  });

  it("never repeats the body's 'try again in a minute' in the 503 headline", () => {
    for (const locale of routing.locales) {
      const api = cachedApi(locale);
      const unavailable = String((api.headlines as Catalog).serverUnavailable);
      const sentence = String((api.errors as Catalog).shutting_down);
      // The sentence keeps the advice; the headline only says the consequence.
      expect(unavailable.length, locale).toBeGreaterThan(0);
      expect(sentence.toLowerCase(), locale).not.toContain(unavailable.toLowerCase());
    }
    expect(String((cachedApi("en").headlines as Catalog).serverUnavailable)).not.toMatch(/minute/u);
    expect(String((cachedApi("en").errors as Catalog).shutting_down)).toMatch(/try again in a minute/u);
  });

  it("heads every definite refusal of a decision with the decision not being recorded", () => {
    for (const code of [
      "internal",
      "shutting_down",
      "rate_limited",
      "decision_conflict",
      "override_note_required",
      "a_code_from_a_newer_server",
    ]) {
      expect(errorHeadlineKey(code, "decision"), code).toBe("decision");
    }
    expect(headline("en", "shutting_down", "decision")).toBe("Your decision was not recorded");
  });

  it("never says a decision was not recorded when the client cannot know", () => {
    // No answer, a server timeout, a status without a contract body, and a
    // 2xx whose body could not be read: the decision may have been recorded.
    for (const code of ["unreachable", "request_timeout", "unexpected_status", "bad_response"]) {
      expect(errorHeadlineKey(code, "decision"), code).toBe("decisionUncertain");
      expect(decisionOutcomeUnknown(code), code).toBe(true);
    }
    expect(decisionOutcomeUnknown("decision_conflict")).toBe(false);
    expect(decisionOutcomeUnknown("internal")).toBe(false);
    for (const locale of routing.locales) {
      const uncertain = headline(locale, "bad_response", "decision");
      expect(uncertain, locale).not.toBe(headline(locale, "internal", "decision"));
    }
    expect(headline("en", "bad_response", "decision")).toBe(
      "Your decision may or may not have been recorded. Check the run before you decide again.",
    );
    // Outside a decision these codes keep their class headline.
    expect(errorHeadlineKey("unreachable", "read")).toBe("unreachable");
    expect(errorHeadlineKey("bad_response", "create")).toBe("unreadable");
  });

  it("heads a refused cancel (read context) by its class, not as a decision", () => {
    expect(errorHeadlineKey("run_not_cancellable", "read")).toBe("notAccepted");
    expect(errorHeadlineKey("run_not_cancellable", "read")).not.toBe("decision");
  });

  it("an outage says the server did not answer, and an unknown code reads as unreadable", () => {
    expect(headline("en", "unreachable", "read")).toBe("The server did not answer");
    expect(errorHeadlineKey("a_code_from_a_newer_server")).toBe("unreadable");
  });

  it.each(routing.locales)("%s has a headline for every class and nothing else", (locale) => {
    const headlines = apiOf(locale).headlines as Catalog;
    const used = new Set(
      [...KNOWN_ERROR_CODES, "unknown"].flatMap((code) =>
        (["create", "decision", "read"] as const).map((context) => errorHeadlineKey(code, context)),
      ),
    );
    expect(Object.keys(headlines).sort()).toEqual([...used, "rateLimitedNoWait"].sort());
    for (const [key, text] of Object.entries(headlines)) {
      expect(String(text).trim(), `${locale} Api.headlines.${key}`).not.toBe("");
    }
    expect("errorTitle" in apiOf(locale), locale).toBe(false);
  });

  it("keeps the run-creation sentence ('admits no new run') to run creation", () => {
    expect(errorSentencePath("shutting_down", "create")).toBe("createErrors.shutting_down");
    expect(errorSentencePath("shutting_down", "read")).toBe("errors.shutting_down");
    expect(errorSentencePath("shutting_down", "decision")).toBe("errors.shutting_down");
    expect(errorSentencePath("internal", "create")).toBe("errors.internal");
    const en = apiOf("en");
    expect(String((en.errors as Catalog).shutting_down)).not.toMatch(/admits/u);
    expect(String((en.createErrors as Catalog).shutting_down)).toMatch(/admits no new run/u);
    for (const locale of routing.locales) {
      const api = apiOf(locale);
      expect(Object.keys(api.createErrors as Catalog), locale).toEqual([...CREATE_SENTENCE_CODES]);
      expect((api.errors as Catalog).shutting_down, locale).not.toBe(
        (api.createErrors as Catalog).shutting_down,
      );
    }
  });
});
