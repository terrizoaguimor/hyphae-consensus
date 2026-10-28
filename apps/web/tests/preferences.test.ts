// SPDX-License-Identifier: Apache-2.0
//
// Stored preferences: every key is the app's own (hc-*), the theme key is the
// one the head script reads, unknown stored values fall back to the default,
// and defaults are stored as "no key" rather than as a value.

import { describe, expect, it } from "vitest";

import { THEME_STORAGE_KEY } from "../src/components/themeScriptSource";
import {
  explanationLevels,
  motionPreferences,
  parseExplanation,
  parseLocale,
  parseMotion,
  parseTheme,
  parseTour,
  STORAGE_KEYS,
  storedValue,
  themePreferences,
} from "../src/lib/preferences";

describe("storage keys", () => {
  it("are distinct and prefixed with hc-", () => {
    const keys = Object.values(STORAGE_KEYS);
    expect(new Set(keys).size).toBe(keys.length);
    for (const key of keys) {
      expect(key).toMatch(/^hc-[a-z]+$/);
    }
  });

  it("share the theme key with the head script", () => {
    expect(STORAGE_KEYS.theme).toBe(THEME_STORAGE_KEY);
  });
});

describe("parsers", () => {
  it("accept their own values and default everything else", () => {
    for (const theme of themePreferences) {
      expect(parseTheme(storedValue(theme))).toBe(theme);
    }
    for (const motion of motionPreferences) {
      expect(parseMotion(storedValue(motion))).toBe(motion);
    }
    for (const level of explanationLevels) {
      expect(parseExplanation(storedValue(level))).toBe(level);
    }
    expect(parseTheme("sepia")).toBe("system");
    expect(parseTheme(null)).toBe("system");
    expect(parseMotion("off")).toBe("system");
    expect(parseExplanation("expert")).toBe("simple");
  });

  it("remember a closed tour, and treat anything else as new", () => {
    expect(parseTour("skipped")).toBe("skipped");
    expect(parseTour("completed")).toBe("completed");
    expect(parseTour(null)).toBe("new");
    expect(parseTour("done")).toBe("new");
  });

  it("accept only the portal's locale tags, exactly", () => {
    expect(parseLocale("pt-BR")).toBe("pt-BR");
    expect(parseLocale("es")).toBe("es");
    expect(parseLocale("pt-br")).toBeNull();
    expect(parseLocale("de")).toBeNull();
    expect(parseLocale(null)).toBeNull();
  });
});

describe("storedValue", () => {
  it("removes the key for defaults", () => {
    expect(storedValue("system")).toBeNull();
    expect(storedValue("simple")).toBeNull();
    expect(storedValue("dark")).toBe("dark");
    expect(storedValue("reduce")).toBe("reduce");
    expect(storedValue("technical")).toBe("technical");
  });
});
