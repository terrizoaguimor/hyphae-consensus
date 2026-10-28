// SPDX-License-Identifier: Apache-2.0
//
// Visitor preferences. All of them live in localStorage only, never in
// cookies, and never leave the browser. Reads and writes swallow storage
// errors (private modes, disabled storage): a choice then lasts for the
// current page view only.

import { THEME_STORAGE_KEY } from "@/components/themeScriptSource";
import { type Locale, routing } from "@/i18n/routing";

export const STORAGE_KEYS = {
  /** "light" | "dark"; absent means "match the system". Read by the head script. */
  theme: THEME_STORAGE_KEY,
  /** "reduce"; absent means "match the system". */
  motion: "hc-motion",
  /** "technical"; absent means "simple". */
  explanation: "hc-explanation",
  /** The language chosen explicitly; the root page (/) opens it. */
  locale: "hc-locale",
  /** "skipped" | "completed"; absent means the tour was never closed. */
  tour: "hc-tour",
  /** One-shot: the setting whose change caused a page change ("locale"). */
  notice: "hc-notice",
} as const;

export const themePreferences = ["system", "light", "dark"] as const;
export type ThemePreference = (typeof themePreferences)[number];

export const motionPreferences = ["system", "reduce"] as const;
export type MotionPreference = (typeof motionPreferences)[number];

export const explanationLevels = ["simple", "technical"] as const;
export type ExplanationLevel = (typeof explanationLevels)[number];

export type TourState = "new" | "skipped" | "completed";

export function parseTheme(value: string | null): ThemePreference {
  return value === "light" || value === "dark" ? value : "system";
}

export function parseMotion(value: string | null): MotionPreference {
  return value === "reduce" ? "reduce" : "system";
}

export function parseExplanation(value: string | null): ExplanationLevel {
  return value === "technical" ? "technical" : "simple";
}

export function parseTour(value: string | null): TourState {
  return value === "skipped" || value === "completed" ? value : "new";
}

export function parseLocale(value: string | null): Locale | null {
  return routing.locales.find((locale) => locale === value) ?? null;
}

/** The stored value for a preference; null for the default (key removed). */
export function storedValue(
  preference: ThemePreference | MotionPreference | ExplanationLevel,
): string | null {
  return preference === "system" || preference === "simple" ? null : preference;
}

export function readStorage(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Stores `value`, or removes the key when `value` is null. */
export function writeStorage(key: string, value: string | null): void {
  try {
    if (value === null) {
      window.localStorage.removeItem(key);
    } else {
      window.localStorage.setItem(key, value);
    }
  } catch {
    // Storage unavailable: the choice holds for this page view only.
  }
}

/** Removes every key this app stores. */
export function clearStorage(): void {
  for (const key of Object.values(STORAGE_KEYS)) {
    writeStorage(key, null);
  }
}
