<!-- SPDX-License-Identifier: Apache-2.0 -->

# Design tokens

The values the portal ships, copied from the stylesheets. Nothing here is a second source of truth: when a table and the file disagree, the file wins and this page is stale.

| Table | Source file |
|---|---|
| Neutral scale, functional tokens, role tokens, glass and button tokens | [`apps/web/src/app/hc-mono-tokens.css`](../../apps/web/src/app/hc-mono-tokens.css) |
| Glass levels, how the material is built, fallbacks | [`apps/web/src/app/hc-mono-glass.css`](../../apps/web/src/app/hc-mono-glass.css) |
| Radii, type sizes, durations | [`apps/web/src/theme/hyphae-mission.css`](../../apps/web/src/theme/hyphae-mission.css) and [`apps/web/src/app/globals.css`](../../apps/web/src/app/globals.css) |
| Spacing and the 12-column grid | design intent only, see [Spacing](#spacing) |

The five-column tables below were generated from `hc-mono-tokens.css` by a throw-away script (a regular expression over its `--hc-*` custom properties) and then compared by eye with the file. Colours there are `light-dark(light, dark)` tuples, so a token follows the `color-scheme` of the element that uses it. The `mono/N` name in parentheses is added by the script when the hex is a step of the scale; it is also written in the file's own comments.

Contents: [Neutral scale](#neutral-scale) · [Functional tokens](#functional-tokens) · [Role tokens](#role-tokens-status) · [Radii](#radii) · [Type](#type) · [Spacing](#spacing) · [Glass](#glass) · [Motion](#motion)

## Neutral scale

Fourteen true greys, R = G = B on every step: no tint, not even an imperceptible one. Both themes draw from this one scale; dark mode is designed by choosing other steps, not by inverting. The dark body is `#0B0B0B`, not `#000`, which leaves room for shadows and keeps the glass highlight visible.

| Step | Hex |
|---|---|
| `mono/0` | `#FFFFFF` |
| `mono/1` | `#F4F4F4` |
| `mono/2` | `#EAEAEA` |
| `mono/3` | `#DCDCDC` |
| `mono/4` | `#C2C2C2` |
| `mono/5` | `#9E9E9E` |
| `mono/6` | `#6E6E6E` |
| `mono/7` | `#575757` |
| `mono/8` | `#3A3A3A` |
| `mono/9` | `#2B2B2B` |
| `mono/10` | `#242424` |
| `mono/11` | `#1B1B1B` |
| `mono/12` | `#131313` |
| `mono/13` | `#0B0B0B` |

## Functional tokens

| Token (`--hc-…`) | Light | Dark |
|---|---|---|
| `bg-body` | `#F4F4F4` (mono/1) | `#0B0B0B` (mono/13) |
| `bg-surface` | `#FFFFFF` (mono/0) | `#131313` (mono/12) |
| `bg-card` | `#FFFFFF` (mono/0) | `#1B1B1B` (mono/11) |
| `bg-popover` | `#FFFFFF` (mono/0) | `#242424` (mono/10) |
| `bg-muted` | `#F4F4F4` (mono/1) | `#0B0B0B` (mono/13) |
| `bg-hover` | `#EAEAEA` (mono/2) | `#242424` (mono/10) |
| `bg-hover-overlay` | `#EAEAEA` (mono/2) | `#2B2B2B` (mono/9) |
| `bg-pressed` | `#C2C2C2` (mono/4) | `#3A3A3A` (mono/8) |
| `bg-disabled` | `#EAEAEA` (mono/2) | `#1B1B1B` (mono/11) |
| `bg-selected` | `#DCDCDC` (mono/3) | `#2B2B2B` (mono/9) |
| `text-primary` | `#131313` (mono/12) | `#F4F4F4` (mono/1) |
| `text-secondary` | `#3A3A3A` (mono/8) | `#C2C2C2` (mono/4) |
| `text-tertiary` | `#575757` (mono/7) | `#9E9E9E` (mono/5) |
| `text-disabled` | `#575757` (mono/7) | `#9E9E9E` (mono/5) |
| `border-subtle` | `#DCDCDC` (mono/3) | `#2B2B2B` (mono/9) |
| `border-default` | `#C2C2C2` (mono/4) | `#3A3A3A` (mono/8) |
| `border-strong` | `#6E6E6E` (mono/6) | `#9E9E9E` (mono/5) |
| `border-overlay` | `#DCDCDC` (mono/3) | `#6E6E6E` (mono/6) |
| `accent-fg` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `accent-bg` | `#DCDCDC` (mono/3) | `#2B2B2B` (mono/9) |
| `accent-border` | `#575757` (mono/7) | `#9E9E9E` (mono/5) |
| `accent-solid` | `#0B0B0B` (mono/13) | `#F4F4F4` (mono/1) |
| `accent-solid-hover` | `#3A3A3A` (mono/8) | `#C2C2C2` (mono/4) |
| `accent-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `focus-ring` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |

Also in the file: `--hc-focus-ring` is `mono/13` in light and `mono/0` in dark; text on a pressed or selected fill uses primary or secondary ink only, never tertiary.

## Role tokens (status)

Nine roles, five tokens each (`fg`, `bg`, `border`, `solid`, `on-solid`). A role is never told apart by its colour alone: see [the encoding in the README](README.md#status-without-colour).

| Token (`--hc-…`) | Light | Dark |
|---|---|---|
| `role-verified-fg` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-verified-bg` | `#EAEAEA` (mono/2) | `#242424` (mono/10) |
| `role-verified-border` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-verified-solid` | `#6E6E6E` (mono/6) | `#9E9E9E` (mono/5) |
| `role-verified-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-checked-fg` | `#131313` (mono/12) | `#F4F4F4` (mono/1) |
| `role-checked-bg` | `#FFFFFF` (mono/0) | `#131313` (mono/12) |
| `role-checked-border` | `#6E6E6E` (mono/6) | `#9E9E9E` (mono/5) |
| `role-checked-solid` | `#6E6E6E` (mono/6) | `#9E9E9E` (mono/5) |
| `role-checked-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-simulated-fg` | `#131313` (mono/12) | `#F4F4F4` (mono/1) |
| `role-simulated-bg` | `#EAEAEA` (mono/2) | `#242424` (mono/10) |
| `role-simulated-border` | `#6E6E6E` (mono/6) | `#9E9E9E` (mono/5) |
| `role-simulated-solid` | `#6E6E6E` (mono/6) | `#9E9E9E` (mono/5) |
| `role-simulated-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-recorded-fg` | `#131313` (mono/12) | `#F4F4F4` (mono/1) |
| `role-recorded-bg` | `#C2C2C2` (mono/4) | `#3A3A3A` (mono/8) |
| `role-recorded-border` | `#575757` (mono/7) | `#9E9E9E` (mono/5) |
| `role-recorded-solid` | `#575757` (mono/7) | `#9E9E9E` (mono/5) |
| `role-recorded-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-live-fg` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-live-bg` | `#FFFFFF` (mono/0) | `#131313` (mono/12) |
| `role-live-border` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-live-solid` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-live-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-failed-fg` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-failed-bg` | `#EAEAEA` (mono/2) | `#242424` (mono/10) |
| `role-failed-border` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-failed-solid` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-failed-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-contested-fg` | `#131313` (mono/12) | `#F4F4F4` (mono/1) |
| `role-contested-bg` | `#EAEAEA` (mono/2) | `#242424` (mono/10) |
| `role-contested-border` | `#3A3A3A` (mono/8) | `#C2C2C2` (mono/4) |
| `role-contested-solid` | `#3A3A3A` (mono/8) | `#C2C2C2` (mono/4) |
| `role-contested-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-pending-fg` | `#0B0B0B` (mono/13) | `#FFFFFF` (mono/0) |
| `role-pending-bg` | `#DCDCDC` (mono/3) | `#2B2B2B` (mono/9) |
| `role-pending-border` | `#575757` (mono/7) | `#9E9E9E` (mono/5) |
| `role-pending-solid` | `#0B0B0B` (mono/13) | `#F4F4F4` (mono/1) |
| `role-pending-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `role-info-fg` | `#3A3A3A` (mono/8) | `#C2C2C2` (mono/4) |
| `role-info-bg` | `#EAEAEA` (mono/2) | `#242424` (mono/10) |
| `role-info-border` | `#DCDCDC` (mono/3) | `#2B2B2B` (mono/9) |
| `role-info-solid` | `#3A3A3A` (mono/8) | `#C2C2C2` (mono/4) |
| `role-info-on-solid` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |

## Radii

From `hyphae-mission.css` (the Astryx theme the app is built on). The square-first rule: 0 for regions, 2 for tags, 4 for controls, 6 for containers, no pills.

| Token | Value | Used for (design spec, section 3.7) |
|---|---|---|
| `--radius-none` | 0 | layout regions, top bar, sidebar, tables, tab strips, progress bars, the bar side of callouts |
| `--radius-inner` | 2px | tags and honesty labels, checkboxes, inline code, hash chips |
| `--radius-element` | 4px | buttons, inputs, selects, segmented control, step markers |
| `--radius-container` | 6px | cards, panels, popovers, dialogs, the drawer's right corners |
| `--radius-page` | 6px | the page container |
| `--radius-full` | 9999px | present in the theme, used only for radio marks (a convention, so a radio still reads as a radio) |

A Vitest scan (`apps/web/tests/radius-scan.test.ts`) reads the shipped stylesheets, including the glass sheet, and `apps/web/e2e/shapes.spec.ts` checks the rendered corners, tags included (commit `a86fcb2`). `apps/web/tests/no-hue.test.ts` and `apps/web/e2e/no-hue.spec.ts` check that no colour has a hue.

## Type

Families: Inter (variable, optical size axis) for the interface and prose; JetBrains Mono for hashes, ids, digests, JSON and code, always with `font-variant-numeric: slashed-zero` (set on the `.hc-code` rules in `globals.css`). Both are loaded through `next/font/google` at build time; see [Credits](README.md#credits).

Sizes as shipped (`hyphae-mission.css`, rem at a 16px root):

| Token | Size | Weight | Line height |
|---|---|---|---|
| `--text-heading-1` | 1.5rem (24px) | semibold | 1.3333 |
| `--text-heading-2` | 1.25rem (20px) | semibold | 1.4 |
| `--text-heading-3` | 1.0625rem (17px) | bold | 1.4118 |
| `--text-heading-4` | 0.875rem (14px) | bold | 1.4286 |
| `--text-heading-5` | 0.75rem (12px) | semibold | 1.6667 |
| `--text-heading-6` | 0.625rem (10px) | semibold | 1.6 |
| `--text-body` / `--text-label` / `--text-code` | 0.875rem (14px) | normal / medium / normal | 1.4286 |
| `--text-large` | 1.0625rem (17px) | semibold | 1.4118 |
| `--text-supporting` | 0.75rem (12px) | normal | 1.6667 |
| `--text-display-1` / `-2` / `-3` | 2.625rem / 2.1875rem / 1.8125rem | normal | 1.2381 / 1.2571 / 1.3793 |

The Figma design specified a different scale (display 40/48, h1 32/40, h2 24/32, body 16/24, nine desktop sizes). The shipped theme is the Astryx scale above, tuned to the same families; the port did not adopt the Figma sizes, so the screenshots show 14px body text. Treat the Figma scale as intent, not as what is on the site.

## Spacing

The Figma system defined an 8px rhythm with a 4px half step (`space/4, 8, 12, 16, 24, 32, 48, 64`), component heights of 24, 32, 40 and 48, a 12-column grid (margin 32, gutter 24 from 672px up, 16 below), a 56px top bar, a 248px sidebar and prose capped at 40rem. The web app uses Astryx's `--spacing-*` tokens and its own layout rules; this page has not checked each of those numbers against the shipped CSS, so they are listed as design intent only. The responsive behaviour that was measured is in the end-to-end specs (`apps/web/e2e/responsive.spec.ts`: no horizontal overflow from 320px up).

## Glass

Source: `hc-mono-tokens.css` (the stops) and `hc-mono-glass.css` (the rules). Three levels; "top" and "bottom" are the stops of a vertical gradient. "Opaque" stops are what static glass and every fallback use.

| Level | Blur | Light top → bottom (alpha) | Dark top → bottom (alpha) | Used by |
|---|---|---|---|---|
| `glass/1` | 16px | `#FFFFFF` .92 → `#F4F4F4` .90 | `#282828` .95 → `#1B1B1B` .95 | the top bar (live); sidebar, cards and panels (static) |
| `glass/2` | 24px | `#FFFFFF` .93 → `#F4F4F4` .91 | `#2C2C2C` .95 → `#212121` .95 | popovers and menus; the decision card (live on narrow viewports, static from 672px) |
| `glass/3` | 24px (16px for the drawer under 672px) | `#FFFFFF` .94 → `#F4F4F4` .92 | `#303030` .96 → `#242424` .96 | dialogs, the drawer, the tour, over the scrim |

The rest of the material, as the CSS builds it:

- **Live glass** (`.hc-glass`): fill gradient, a 105° sheen on a fixed 720px layer, the brushed grain tile (128px, `public/brand/brushed-{light,dark}.png`, and `@2x`), a 1px top highlight, a 1px gradient edge, inner and outer shadows. `backdrop-filter: blur() grayscale(1)`: the `grayscale` guarantees no hue shows through from content, an image or the OS.
- **Static glass** (`.hc-glass--static`): the same gradient, grain, edge and shadow over opaque stops; no blur, no sheen, no top highlight. A list of 50 cards does not make 50 render surfaces.
- **Ink material** (the primary button only): an opaque gradient from `--hc-btn-top` to `--hc-btn-bottom`, the grain, a 1px highlight. Tags never carry it, so nothing that looks like a button is a tag.
- **Where it is not allowed**: tables, long text, tags, inputs and code stay on solid surfaces (`tests/glass.test.ts` enforces it). At most the top bar, one overlay and one floating card are live at once, and `backdrop-filter` is never animated.
- **Fallbacks**: no `backdrop-filter` support, `prefers-reduced-transparency`, and the `.hc-transparency-reduced` root class use the opaque stops with the gradient and grain kept (the app has no Settings toggle for the class yet, so only the OS setting applies today). `prefers-contrast: more` gives flat `bg/popover` with a 1px `border/strong` ring. `forced-colors: active` gives `Canvas`/`CanvasText` with a real border. Print forces the light scheme.
- **Tiles**: grain and hatch are same-origin PNGs under `/brand/`, because the content-security policy is hash-only (`img-src 'self'`, no `data:` URI, no style attributes, no runtime-injected style).

Glass and button tokens (generated from `hc-mono-tokens.css`):

| Token (`--hc-…`) | Light | Dark |
|---|---|---|
| `scrim` | `rgb(11 11 11 / .55)` | `rgb(0 0 0 / .65)` |
| `scrim-opaque` | `rgb(11 11 11 / .8)` | `rgb(0 0 0 / .8)` |
| `shadow-elev2` | `rgb(0 0 0 / .12)` | `rgb(0 0 0 / .55)` |
| `shadow-elev3` | `rgb(0 0 0 / .18)` | `rgb(0 0 0 / .70)` |
| `body-0` | `#FFFFFF` (mono/0) | `#242424` (mono/10) |
| `body-1` | `#F4F4F4` (mono/1) | `#131313` (mono/12) |
| `body-2` | `#DCDCDC` (mono/3) | `#0B0B0B` (mono/13) |
| `glass-1-top` | `rgb(255 255 255 / 0.92)` | `rgb(40 40 40 / 0.95)` |
| `glass-1-top-opaque` | `#FFFFFF` (mono/0) | `#272727` |
| `glass-1-bottom` | `rgb(244 244 244 / 0.9)` | `rgb(27 27 27 / 0.95)` |
| `glass-1-bottom-opaque` | `#F5F5F5` | `#1A1A1A` |
| `glass-2-top` | `rgb(255 255 255 / 0.93)` | `rgb(44 44 44 / 0.95)` |
| `glass-2-top-opaque` | `#FFFFFF` (mono/0) | `#2A2A2A` |
| `glass-2-bottom` | `rgb(244 244 244 / 0.91)` | `rgb(33 33 33 / 0.95)` |
| `glass-2-bottom-opaque` | `#F5F5F5` | `#202020` |
| `glass-3-top` | `rgb(255 255 255 / 0.94)` | `rgb(48 48 48 / 0.96)` |
| `glass-3-top-opaque` | `#FFFFFF` (mono/0) | `#2F2F2F` |
| `glass-3-bottom` | `rgb(244 244 244 / 0.92)` | `rgb(36 36 36 / 0.96)` |
| `glass-3-bottom-opaque` | `#F5F5F5` | `#232323` |
| `glass-sheen` | `rgb(255 255 255 / 0.22)` | `rgb(255 255 255 / 0.04)` |
| `glass-highlight` | `rgb(255 255 255 / 0.95)` | `rgb(255 255 255 / 0.16)` |
| `glass-edge-top` | `rgb(255 255 255 / 1.0)` | `rgb(255 255 255 / 0.3)` |
| `glass-edge-bottom` | `rgb(124 124 124 / 0.55)` | `rgb(255 255 255 / 0.06)` |
| `glass-lowlight` | `rgb(0 0 0 / 0.05)` | `rgb(0 0 0 / 0.35)` |
| `glass-1-shadow-near` | `rgb(0 0 0 / 0.06)` | `rgb(0 0 0 / 0.4)` |
| `glass-1-shadow-far` | `rgb(0 0 0 / 0.05)` | `rgb(0 0 0 / 0.3)` |
| `glass-2-shadow-near` | `rgb(0 0 0 / 0.08)` | `rgb(0 0 0 / 0.45)` |
| `glass-2-shadow-far` | `rgb(0 0 0 / 0.12)` | `rgb(0 0 0 / 0.55)` |
| `glass-3-shadow-near` | `rgb(0 0 0 / 0.1)` | `rgb(0 0 0 / 0.5)` |
| `glass-3-shadow-far` | `rgb(0 0 0 / 0.18)` | `rgb(0 0 0 / 0.7)` |
| `btn-top` | `#3A3A3A` (mono/8) | `#FFFFFF` (mono/0) |
| `btn-bottom` | `#0B0B0B` (mono/13) | `#C2C2C2` (mono/4) |
| `btn-hover-top` | `#575757` (mono/7) | `#FFFFFF` (mono/0) |
| `btn-hover-bottom` | `#131313` (mono/12) | `#DCDCDC` (mono/3) |
| `btn-text` | `#FFFFFF` (mono/0) | `#0B0B0B` (mono/13) |
| `btn-highlight` | `rgb(255 255 255 / 0.22)` | `rgb(255 255 255 / 0.0)` |
| `btn-lowlight` | `rgb(0 0 0 / 0.0)` | `rgb(0 0 0 / 0.18)` |

Blur values (`--hc-glass-1-blur` 16px, `-2` and `-3` 24px) and the `--hc-glass-shadow-1/2/3` compositions are plain values in the same file.

## Motion

As shipped (`globals.css`): overlays (dialog, drawer, tour) fade and shift over 200ms with `cubic-bezier(0.2, 0, 0, 1)`; the dialog rises 0.5rem and scales to 0.98, the drawer slides in on X. The theme's own durations (`--duration-fast` 125ms, `--duration-medium` 300ms, `--duration-slow` 700ms) drive the library components. Under reduced motion, from the OS setting or the in-app Settings choice (`data-motion="reduce"` on `<html>`): no translation or scale, overlay fades of 150ms, every other animation and transition cut to 0.01ms (not 0, so `transitionend` still fires), the LIVE square static. The Figma spec's 120/200/300ms tokens are documentation of intent; the code uses the values above.
