"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Visitor preferences (theme, motion, explanation level, tour) as React
// state, mirrored to localStorage (src/lib/preferences.ts) and to <html>:
// data-theme through Astryx's Theme, data-motion here. The static HTML is
// rendered with the defaults; the stored values are read in a layout effect,
// which React flushes before the first paint after hydration.

import {
  createContext,
  use,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import {
  clearStorage,
  type ExplanationLevel,
  type MotionPreference,
  parseExplanation,
  parseMotion,
  parseTheme,
  parseTour,
  readStorage,
  STORAGE_KEYS,
  storedValue,
  type ThemePreference,
  type TourState,
  writeStorage,
} from "@/lib/preferences";

export type ColorMode = "light" | "dark";

export type Preferences = {
  /** False in the static HTML and the first client render. */
  hydrated: boolean;
  /** The mode on screen; null until the head script's choice was read. */
  colorMode: ColorMode | null;
  theme: ThemePreference;
  motion: MotionPreference;
  explanation: ExplanationLevel;
  /** null until read from storage, so a returning visitor never sees the invitation flash. */
  tour: TourState | null;
  tourOpen: boolean;
  setTheme: (theme: ThemePreference) => void;
  setMotion: (motion: MotionPreference) => void;
  setExplanation: (level: ExplanationLevel) => void;
  openTour: () => void;
  /** Closes the tour (or its invitation) and remembers how. */
  closeTour: (outcome: "skipped" | "completed") => void;
  /** Removes every stored preference and restores the defaults. */
  resetAll: () => void;
};

const PreferencesContext = createContext<Preferences | null>(null);

export const PreferencesProvider = PreferencesContext;

export function usePreferences(): Preferences {
  const value = use(PreferencesContext);
  if (value === null) {
    throw new Error("usePreferences must be used inside <Providers>");
  }
  return value;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function systemMode(): ColorMode {
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

function readDocumentMode(): ColorMode | null {
  if (typeof document === "undefined") {
    return null;
  }
  const mode = document.documentElement.getAttribute("data-theme");
  return mode === "light" || mode === "dark" ? mode : null;
}

/** The state behind <Providers>; call it once, in Providers. */
export function usePreferenceState(): Preferences {
  // The static HTML is rendered with Theme in "system" mode, and hydration
  // must render the same. The mode the head script put on <html> is read
  // during that first render, before Theme's layout effect removes the
  // attribute for "system", and applied in a layout effect, which React
  // flushes before the browser paints. Until then globals.css makes the root
  // Theme wrapper follow <html>, so the static page already shows the mode
  // the head script chose.
  const [initialMode] = useState(readDocumentMode);
  const [hydrated, setHydrated] = useState(false);
  const [colorMode, setColorMode] = useState<ColorMode | null>(null);
  const [theme, setThemeState] = useState<ThemePreference>("system");
  const [motion, setMotionState] = useState<MotionPreference>("system");
  const [explanation, setExplanationState] =
    useState<ExplanationLevel>("simple");
  const [tour, setTourState] = useState<TourState | null>(null);
  const [tourOpen, setTourOpen] = useState(false);
  const themeRef = useRef<ThemePreference>("system");

  const readAll = useCallback(() => {
    const storedTheme = parseTheme(readStorage(STORAGE_KEYS.theme));
    themeRef.current = storedTheme;
    setThemeState(storedTheme);
    setMotionState(parseMotion(readStorage(STORAGE_KEYS.motion)));
    setExplanationState(
      parseExplanation(readStorage(STORAGE_KEYS.explanation)),
    );
    setTourState(parseTour(readStorage(STORAGE_KEYS.tour)));
    return storedTheme;
  }, []);

  useLayoutEffect(() => {
    // The extra render is the point: it lands before the first paint after
    // hydration, which an event or passive effect would not.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    readAll();
    if (initialMode !== null) {
      setColorMode(initialMode);
    }
    setHydrated(true);
    // Marks the page as interactive (the e2e tests wait for it).
    document.documentElement.setAttribute("data-hydrated", "");
  }, [initialMode, readAll]);

  // data-motion on <html> drives the reduced-motion rules in globals.css,
  // next to the OS-level prefers-reduced-motion query.
  useLayoutEffect(() => {
    if (!hydrated) {
      return;
    }
    const root = document.documentElement;
    if (motion === "reduce") {
      root.setAttribute("data-motion", "reduce");
    } else {
      root.removeAttribute("data-motion");
    }
  }, [hydrated, motion]);

  // Follow the OS color scheme while the preference is "system".
  useEffect(() => {
    const query = window.matchMedia(DARK_QUERY);
    const onChange = () => {
      if (themeRef.current === "system") {
        setColorMode(query.matches ? "dark" : "light");
      }
    };
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  // Keep tabs in step: another tab's change arrives as a storage event.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.storageArea !== window.localStorage) {
        return;
      }
      const storedTheme = readAll();
      setColorMode(storedTheme === "system" ? systemMode() : storedTheme);
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [readAll]);

  const setTheme = useCallback((next: ThemePreference) => {
    themeRef.current = next;
    writeStorage(STORAGE_KEYS.theme, storedValue(next));
    setThemeState(next);
    setColorMode(next === "system" ? systemMode() : next);
  }, []);

  const setMotion = useCallback((next: MotionPreference) => {
    writeStorage(STORAGE_KEYS.motion, storedValue(next));
    setMotionState(next);
  }, []);

  const setExplanation = useCallback((next: ExplanationLevel) => {
    writeStorage(STORAGE_KEYS.explanation, storedValue(next));
    setExplanationState(next);
  }, []);

  const openTour = useCallback(() => setTourOpen(true), []);

  const closeTour = useCallback((outcome: "skipped" | "completed") => {
    writeStorage(STORAGE_KEYS.tour, outcome);
    setTourState(outcome);
    setTourOpen(false);
  }, []);

  const resetAll = useCallback(() => {
    clearStorage();
    themeRef.current = "system";
    setThemeState("system");
    setColorMode(systemMode());
    setMotionState("system");
    setExplanationState("simple");
    setTourState("new");
  }, []);

  return useMemo(
    () => ({
      hydrated,
      colorMode,
      theme,
      motion,
      explanation,
      tour,
      tourOpen,
      setTheme,
      setMotion,
      setExplanation,
      openTour,
      closeTour,
      resetAll,
    }),
    [
      hydrated,
      colorMode,
      theme,
      motion,
      explanation,
      tour,
      tourOpen,
      setTheme,
      setMotion,
      setExplanation,
      openTour,
      closeTour,
      resetAll,
    ],
  );
}
