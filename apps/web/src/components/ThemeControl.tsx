"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The theme control of the top bar and the mobile drawer
// (design-direction.md 4.2 #5): a 3-segment control, System / Light / Dark,
// in place of the old "Dark theme" Switch. It is a native radio group: Tab
// reaches the selected segment, the arrow keys move and select, Space
// selects, and each segment's name is its word (visually hidden in the top
// bar, where only the icon shows). The choice is the stored theme
// preference (src/lib/preferences.ts): "system" removes the key, "light"
// and "dark" store it. The page colours come from the head script before
// first paint; this control shows "System" in the static HTML and the
// stored choice once the page hydrates.
//
// Square first: outer radius 4, inner joins 0; the selected segment carries
// the ink material and a check mark (globals.css, "Theme control").

import { useTranslations } from "next-intl";
import { useId } from "react";

import { type ThemePreference, themePreferences } from "@/lib/preferences";

import { usePreferences } from "./preferences";

type ThemeControlProps = {
  /** "icons" in the top bar (words visually hidden), "words" elsewhere. */
  variant?: "icons" | "words";
};

export function ThemeControl({ variant = "words" }: ThemeControlProps) {
  const t = useTranslations("Theme");
  const { theme, setTheme } = usePreferences();
  // Two controls can be on one page (top bar and drawer): one name each.
  const name = useId();
  return (
    <div
      role="radiogroup"
      aria-label={t("label")}
      className="hc-segmented"
      data-theme-control={variant}
    >
      {themePreferences.map((option) => (
        <label key={option} className="hc-segment" data-option={option}>
          <input
            type="radio"
            className="hc-segment-input"
            name={name}
            value={option}
            checked={theme === option}
            onChange={() => setTheme(option)}
          />
          <ThemeIcon option={option} />
          <span
            className={
              variant === "icons" ? "hc-visually-hidden" : "hc-segment-label"
            }
          >
            {t(`options.${option}`)}
          </span>
          <CheckIcon />
        </label>
      ))}
    </div>
  );
}

const ICON_PROPS = {
  viewBox: "0 0 24 24",
  width: 16,
  height: 16,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 2,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
  focusable: false,
} as const;

/**
 * Lucide's monitor, sun and moon, redrawn as inline SVG with no icon
 * dependency (ISC; monitor and moon derive from Feather, MIT; notices in
 * THIRD_PARTY_NOTICES.md and public/licenses/Lucide-LICENSE.txt).
 */
function ThemeIcon({ option }: { option: ThemePreference }) {
  if (option === "system") {
    return (
      <svg {...ICON_PROPS} className="hc-segment-icon">
        <rect width="20" height="14" x="2" y="3" rx="2" />
        <path d="M8 21h8M12 17v4" />
      </svg>
    );
  }
  if (option === "light") {
    return (
      <svg {...ICON_PROPS} className="hc-segment-icon">
        <circle cx="12" cy="12" r="4" />
        <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
      </svg>
    );
  }
  return (
    <svg {...ICON_PROPS} className="hc-segment-icon">
      <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
    </svg>
  );
}

/** Lucide's check (from Feather, MIT), 12px: shown on the selected segment only (CSS). */
function CheckIcon() {
  return (
    <svg {...ICON_PROPS} width={12} height={12} className="hc-segment-check">
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}
