// SPDX-License-Identifier: Apache-2.0
//
// Source of the `hyphae-mission` Astryx theme. The app never imports this
// file: `pnpm theme:build` (`astryx theme build`) compiles it to the committed
// hyphae-mission.css / .js / .d.ts beside it, and `pnpm theme:check` fails
// when those outputs drift from this source. Rebuild after any Astryx upgrade:
// the runtime trusts the built flag and does not repair stale CSS.
//
// Monochrome (owner direction 2026-10-31, design/palette-mono.md revision 2):
// every colour is a true grey (R = G = B) from the 14-step scale below, in
// both themes. Token names are unchanged; only their values moved, following
// the old-to-new mapping of palette-mono.md section 5. Every colour token that
// neutral or Astryx's base stylesheet defines with a hue (the accent, the
// status fills, the nine hue families, the syntax and data-visualisation
// palettes, the inset status shadows) is re-pointed here at a grey, so no hue
// reaches a component: tests/no-hue.test.ts fails on any colour literal in
// the compiled theme whose R, G and B differ, and e2e/no-hue.spec.ts checks
// the built CSS and the rendered pages.

import { defineTheme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

/** The neutral scale, palette-mono.md 1.1: mono/0 (white) to mono/13 (ink). */
const MONO = {
  0: "#FFFFFF",
  1: "#F4F4F4",
  2: "#EAEAEA",
  3: "#DCDCDC",
  4: "#C2C2C2",
  5: "#9E9E9E",
  6: "#6E6E6E",
  7: "#575757",
  8: "#3A3A3A",
  9: "#2B2B2B",
  10: "#242424",
  11: "#1B1B1B",
  12: "#131313",
  13: "#0B0B0B",
} as const;

type Step = keyof typeof MONO;
type Pair = [light: string, dark: string];

/** A [light, dark] token from two steps of the scale. */
const mono = (light: Step, dark: Step): Pair => [MONO[light], MONO[dark]];

// Functional tokens, palette-mono.md 1.2 and 5.1 (Figma name in brackets).
const textPrimary = mono(12, 1); // text/primary
const textSecondary = mono(8, 4); // text/secondary
const textDisabled = mono(7, 5); // text/disabled
const bgHover = mono(2, 10); // bg/hover, and every role tint
const borderStrong = mono(6, 5); // border/strong
const accentFg = mono(13, 0); // accent/fg: links, active bars
const accentBg = mono(3, 9); // accent/bg: selected, current, pending
const accentSolid = mono(13, 1); // accent/solid: primary button edge
const onAccent = mono(0, 13); // accent/on-solid
const verifiedSolid = mono(6, 5); // role/verified/solid (5.10:1 / 7.35:1 under its label)
const failedSolid = mono(13, 0); // role/failed/solid
const contestedLine = mono(8, 4); // role/contested/border

// The nine hue families (Badge colour variants, banners, tints): palette-mono
// 5.3 points each part at one mono alias, so a coloured variant can no longer
// bring a hue in. The status tags' own encodings (fill weight, frame, marker)
// live in globals.css.
const HUES = ["red", "orange", "yellow", "green", "teal", "cyan", "blue", "purple", "pink"] as const;
const hueFamilies = Object.fromEntries(
  HUES.flatMap((hue) => [
    [`--color-text-${hue}`, textSecondary],
    [`--color-background-${hue}`, bgHover],
    [`--color-border-${hue}`, borderStrong],
    [`--color-icon-${hue}`, textSecondary],
  ]),
) as Record<`--color-${"text" | "background" | "border" | "icon"}-${(typeof HUES)[number]}`, Pair>;

// Data visualisation (Astryx's base stylesheet defines these on :root with
// hues; the app draws no Astryx chart, but nothing may resolve to a hue).
// Each ramp runs from the lightest step (1) to the darkest (5).
const DATA_FAMILIES = ["blue", "shamrock", "orange", "pink", "purple", "red", "teal", "yellow", "gray"] as const;
const DATA_RAMP: Record<1 | 2 | 3 | 4 | 5, Step> = { 1: 2, 2: 4, 3: 5, 4: 6, 5: 8 };
const dataRamps = Object.fromEntries(
  DATA_FAMILIES.flatMap((family) =>
    ([1, 2, 3, 4, 5] as const).map((level) => [
      `--color-data-${family}-${level}`,
      MONO[DATA_RAMP[level]],
    ]),
  ),
) as Record<`--color-data-${(typeof DATA_FAMILIES)[number]}-${1 | 2 | 3 | 4 | 5}`, string>;

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
    // The accent is ink (palette-mono 1.2): "what you can do" is carried by
    // material and shape, links by their underline. Neutral sets each of
    // these explicitly, so each one is overridden here.
    "--color-accent": accentSolid,
    "--color-text-accent": accentFg,
    "--color-icon-accent": accentFg,
    "--color-accent-muted": accentBg,
    // Text on the accent fill: white on ink (19.68:1), ink on mono/1 (17.90:1).
    "--color-on-accent": onAccent,
    "--focus-outline-color": accentFg, // focus/ring, 2px with a 2px offset

    // Surfaces: one grey scale for both themes; dark is designed by choosing
    // other steps, not by inversion. Worst text pairs (tests/contrast.test.ts):
    // light secondary #3A3A3A on #F4F4F4 body and muted 10.34:1; dark
    // secondary #C2C2C2 on #242424 popover 8.71:1.
    "--color-background-body": mono(1, 13),
    "--color-background-surface": mono(0, 12),
    "--color-background-card": mono(0, 11),
    "--color-background-popover": mono(0, 10),
    "--color-background-muted": mono(1, 13),
    "--color-background-inverted": mono(13, 0),
    "--color-background-error-inverted": failedSolid,
    "--color-overlay-hover": bgHover,
    "--color-overlay-pressed": mono(4, 8),
    // scrim: rgb(11 11 11 / .55) light, rgb(0 0 0 / .65) dark.
    "--color-overlay": ["#0B0B0B8C", "#000000A6"],
    // shadow/elev2: rgb(0 0 0 / .12) light, rgb(0 0 0 / .55) dark.
    "--color-shadow": ["#0000001F", "#0000008C"],

    "--color-text-primary": textPrimary,
    "--color-text-secondary": textSecondary,
    "--color-text-disabled": textDisabled,
    "--color-icon-primary": textPrimary,
    "--color-icon-secondary": textSecondary,
    "--color-icon-disabled": textDisabled,

    "--color-border": mono(3, 9), // border/subtle
    "--color-border-emphasized": borderStrong,
    "--color-track": mono(4, 8), // border/default
    "--color-skeleton": mono(2, 10),

    // Status colours become role values (palette-mono 5.2): success is the
    // verified solid, error the failed ink, warning the contested line.
    "--color-success": verifiedSolid,
    "--color-error": failedSolid,
    "--color-warning": contestedLine,
    "--color-success-muted": bgHover,
    "--color-error-muted": bgHover,
    "--color-warning-muted": bgHover,
    "--color-on-success": onAccent,
    "--color-on-error": onAccent,
    "--color-on-warning": textPrimary,

    // Inset rings (hover, selected and status states of Astryx controls).
    "--shadow-inset-hover": `inset 0px 0px 0px 1px light-dark(${MONO[6]}, ${MONO[5]})`,
    "--shadow-inset-selected": `inset 0px 0px 0px 2px light-dark(${MONO[7]}, ${MONO[5]})`,
    "--shadow-inset-success": `inset 0px 0px 0px 2px light-dark(${MONO[6]}, ${MONO[5]})`,
    "--shadow-inset-warning": `inset 0px 0px 0px 2px light-dark(${MONO[8]}, ${MONO[4]})`,
    "--shadow-inset-error": `inset 0px 0px 0px 2px light-dark(${MONO[13]}, ${MONO[0]})`,

    // Code highlighting in greys: weight of ink, not hue, separates kinds.
    "--color-syntax-keyword": textPrimary,
    "--color-syntax-string": textSecondary,
    "--color-syntax-comment": mono(7, 5),
    "--color-syntax-number": textSecondary,
    "--color-syntax-function": textPrimary,
    "--color-syntax-type": textPrimary,
    "--color-syntax-variable": textPrimary,
    "--color-syntax-operator": mono(7, 5),
    "--color-syntax-constant": textSecondary,
    "--color-syntax-tag": textPrimary,
    "--color-syntax-attribute": textSecondary,
    "--color-syntax-property": textSecondary,
    "--color-syntax-punctuation": mono(7, 5),
    "--color-syntax-background": mono(0, 12),

    ...hueFamilies,

    "--color-data-categorical-blue": MONO[8],
    "--color-data-categorical-orange": MONO[5],
    "--color-data-categorical-purple": MONO[7],
    "--color-data-categorical-green": MONO[4],
    "--color-data-categorical-pink": MONO[6],
    "--color-data-categorical-cyan": MONO[9],
    "--color-data-categorical-red": MONO[13],
    "--color-data-categorical-teal": MONO[3],
    "--color-data-categorical-brown": MONO[11],
    "--color-data-categorical-indigo": MONO[2],
    "--color-data-neutral": MONO[5],
    ...dataRamps,

    // Square-first radii (design-direction 3.7): controls 4, containers 6,
    // the inner corner of a control 2.
    "--radius-inner": "2px",
    "--radius-element": "4px",
    "--radius-container": "6px",
    "--radius-page": "6px",
  },

  localTokens: {
    // Neutral draws its info badge and accent status dot with these fills
    // instead of --color-accent; they follow the same roles.
    "--astryx-theme-neutral-color-status-fill-accent": accentSolid,
    "--astryx-theme-neutral-color-status-fill-success": verifiedSolid,
    "--astryx-theme-neutral-color-status-fill-warning": bgHover,
    "--astryx-theme-neutral-color-status-fill-error": failedSolid,
    "--astryx-theme-neutral-color-status-muted-accent": accentBg,
    "--astryx-theme-neutral-color-destructive-overlay-hover": ["#0000000D", "#FFFFFF0D"],
    "--astryx-theme-neutral-color-destructive-overlay-pressed": ["#0000001A", "#FFFFFF1A"],

    // Boundaries of controls and of panels that bound a control (WCAG 1.4.11
    // non-text contrast, 3:1): radio rings, checkbox boxes, the off Switch
    // track, the Settings panels, the dashed not-connected panel and native
    // selects. border/strong of palette-mono (mono/6 light, mono/5 dark): light
    // #6E6E6E measures 5.10:1 on surface, card and popover and 4.64:1 on the
    // body and muted (the lowest); dark #9E9E9E measures 7.35:1 on the body
    // and muted, 6.94:1 on the surface, 6.43:1 on the card and 5.79:1 on the
    // popover (the lowest). tests/contrast.test.ts requires 3:1 on each.
    "--hc-color-border-strong": borderStrong,

    // The proof fill, now the verified solid (palette-mono 5.2; the cyan is
    // gone). Under --hc-color-on-proof: white 5.10:1 light, ink 7.35:1 dark.
    "--hc-color-proof": verifiedSolid,
    "--hc-color-on-proof": onAccent,
  },
});
