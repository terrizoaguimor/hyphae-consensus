// SPDX-License-Identifier: Apache-2.0
//
// Source of the `hyphae-mission` Astryx theme. The app never imports this
// file: `pnpm theme:build` (`astryx theme build`) compiles it to the committed
// hyphae-mission.css / .js / .d.ts beside it, and `pnpm theme:check` fails
// when those outputs drift from this source. Rebuild after any Astryx upgrade:
// the runtime trusts the built flag and does not repair stale CSS.

import { defineTheme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

// Actions use Astryx's default light accent. The dark value is the accent for
// text and icons on dark surfaces.
const accent: [light: string, dark: string] = ["#0064E0", "#4BA9FE"];

export const hyphaeMissionTheme = defineTheme({
  name: "hyphae-mission",
  extends: neutralTheme,

  // Fonts go through the typography roles, not plain tokens: neutral's
  // touch-and-narrow adaptation re-derives the font tokens from these roles,
  // so a plain token override would fall back to neutral's Figtree on phones.
  // The scale and heading weights stay inherited from neutral. next/font
  // defines both variables on <html> (src/app/fonts.ts).
  typography: {
    body: { family: "var(--font-inter)", fallbacks: "system-ui, sans-serif" },
    heading: {
      family: "var(--font-inter)",
      fallbacks: "system-ui, sans-serif",
    },
    code: {
      family: "var(--font-jetbrains-mono)",
      fallbacks: "ui-monospace, monospace",
    },
  },

  tokens: {
    // Neutral sets each of these four explicitly from its gray palette, so
    // each one is overridden here rather than derived from the accent.
    "--color-accent": accent,
    "--color-text-accent": accent,
    "--color-icon-accent": accent,
    // Text on accent fills. Light: white on #0064E0 (5.39:1). Dark: #111112 on
    // #4BA9FE (7.55:1); white on #4BA9FE (2.50:1) or #2694FE (3.11:1) fails
    // WCAG AA, so it is never used.
    "--color-on-accent": ["#FFFFFF", "#111112"],

    "--radius-element": "8px",
    "--radius-container": "16px",
  },

  localTokens: {
    // Neutral draws its info badge and accent status dot with this fill
    // instead of --color-accent; point it at the same accent.
    "--astryx-theme-neutral-color-status-fill-accent": accent,

    // Hyphae cyan, reserved for proof and evidence. Use it as a fill under
    // --hc-color-on-proof text (11.9:1), or as text on dark surfaces only:
    // on white it measures 1.58:1, too low for text or a lone indicator.
    "--hc-color-proof": "#45E4D0",
    "--hc-color-on-proof": "#111112",
  },
});
