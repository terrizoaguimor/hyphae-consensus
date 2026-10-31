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

    // Surfaces (plan, "Design"): white and ice-blue in light mode; deep
    // midnight in dark mode, not an inversion of the light palette. Neutral's
    // grays are replaced one by one. Text on these surfaces, worst case:
    // light #474747 secondary text on #EDF3FB muted measures 8.4:1, accent
    // #0064E0 on #F3F7FC body 5.0:1; dark #9E9E9E secondary text on #141D33
    // popover 6.3:1, accent #4BA9FE on #121A2D surface 6.9:1 (WCAG AA).
    "--color-background-body": ["#F3F7FC", "#0A0F1E"],
    "--color-background-surface": ["#FFFFFF", "#121A2D"],
    "--color-background-card": ["#FFFFFF", "#0F1628"],
    "--color-background-popover": ["#FFFFFF", "#141D33"],
    "--color-background-muted": ["#EDF3FB", "#0F1628"],

    "--radius-element": "8px",
    "--radius-container": "16px",
  },

  localTokens: {
    // Neutral draws its info badge and accent status dot with this fill
    // instead of --color-accent; point it at the same accent.
    "--astryx-theme-neutral-color-status-fill-accent": accent,

    // Boundaries of controls and of panels that bound a control (WCAG 1.4.11
    // non-text contrast, 3:1): radio rings, checkbox boxes, the off Switch
    // track, the Settings panels, the dashed not-connected panel and native
    // selects. The values are the slate v2 border-strong (design/palette,
    // slate-light/7 and slate-dark/7), kept because they match this theme's
    // blue-tinted surfaces; design-direction.md's mono border/strong belongs
    // to the redesign. On this theme's surface tokens, light #7A8496 measures
    // 3.77:1 on surface, card and popover, 3.51:1 on the body and 3.38:1 on
    // muted (the lowest); dark #677388 measures 3.62:1 on the surface,
    // 3.99:1 on the body, 3.76:1 on card and muted and 3.50:1 on the popover
    // (the lowest). Neutral's --color-border-emphasized (#d4d4d4 / #525252)
    // measures 1.33 to 1.48:1 light and 2.14 to 2.44:1 dark on the same
    // surfaces. tests/contrast.test.ts requires 3:1 on every one of them.
    "--hc-color-border-strong": ["#7A8496", "#677388"],

    // Hyphae cyan, reserved for proof and evidence. Use it as a fill under
    // --hc-color-on-proof text (11.9:1), or as text on dark surfaces only:
    // on white it measures 1.58:1, too low for text or a lone indicator.
    "--hc-color-proof": "#45E4D0",
    "--hc-color-on-proof": "#111112",
  },
});
