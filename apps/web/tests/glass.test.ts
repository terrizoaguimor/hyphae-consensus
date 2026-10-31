// SPDX-License-Identifier: Apache-2.0
//
// The brushed glass (design/palette-mono.md section 3, src/app/hc-mono-glass.css):
// - the stylesheet is in @layer app, and every fallback of palette-mono 3.6 is
//   present (no backdrop-filter, reduced transparency, more contrast, forced
//   colours, print, the narrow drawer, the dialog scrim);
// - glass is never on a forbidden surface: no rule of the app's stylesheets
//   gives a table, a tag, an input, code or long text a backdrop filter or
//   a glass fill, and only the .hc-glass rule sets a live backdrop filter;
// - live glass is used only where 3.5 allows it (the top bar, the two modal
//   dialogs, the decision card), and no glass class sits on a forbidden element;
// - the grain tiles it names are shipped under public/brand/ at the sizes
//   image-set() expects, same-origin (img-src 'self'), and within their alpha
//   limits;
// - the primary button carries the ink gradient, grain and highlight, and
//   its :active rule is flat btn/hover-bottom with no shadow;
// - palette-mono's worst-case contrast (section 4/D of its checker,
//   palette_mono_check.py) holds on the shipped token values and the shipped
//   tiles: text and strong borders over live glass (every backdrop, stop,
//   sheen and grain combination, no credit for the blur), over static glass,
//   and the button label on every stop of the button gradient with its grain.
// e2e/glass.spec.ts checks the rendered result.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

import { describe, expect, it } from "vitest";

import { contentSecurityPolicy } from "../scripts/serve-static.mjs";

import { declarations, type Declaration } from "./colour-scan";
import { IO_BOUND_TEST_TIMEOUT_MS } from "./io-timeout";

const appDir = fileURLToPath(new URL("..", import.meta.url));
const read = (relative: string) => readFileSync(path.join(appDir, relative), "utf8");

const glassCss = read("src/app/hc-mono-glass.css");
const tokensCss = read("src/app/hc-mono-tokens.css");
const globalsCss = read("src/app/globals.css");
const themeCss = read("src/theme/hyphae-mission.css");
const glass = declarations(glassCss);
const appSheets: Record<string, Declaration[]> = {
  "src/app/hc-mono-glass.css": glass,
  "src/app/hc-mono-tokens.css": declarations(tokensCss),
  "src/app/globals.css": declarations(globalsCss),
};

/** The selectors of a rule, one per comma at the top level. */
function selectorsOf(selector: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const char of selector) {
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    if (char === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
    } else {
      current += char;
    }
  }
  parts.push(current.trim());
  return parts.map((part) => part.replace(/\s+/g, " "));
}

/** The declarations of the glass sheet for `selector` under exactly the at-rules `at` (after @layer app). */
function ruleOf(selector: string, at: string[] = []): Record<string, string> {
  const result: Record<string, string> = {};
  for (const declaration of glass) {
    const context = declaration.context.slice(1).map((entry) => entry.replace(/\s+/g, " "));
    if (context.join("|") !== at.join("|")) continue;
    if (selectorsOf(declaration.selector).includes(selector)) {
      result[declaration.property] = declaration.value;
    }
  }
  return result;
}

const SUPPORTS_NOT = "@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px)))";
const REDUCED_TRANSPARENCY = "@media (prefers-reduced-transparency: reduce)";
const MORE_CONTRAST = "@media (prefers-contrast: more)";
const FORCED = "@media (forced-colors: active)";
const PRINT = "@media print";
const RING = ".hc-glass:where(:not(.hc-glass--static))::before";
const PRIMARY = '.astryx-button[data-variant="primary"]';

describe("the glass stylesheet", () => {
  it("is inside @layer app, has no [data-theme] override and is loaded between the tokens and globals.css", () => {
    expect(glass.length).toBeGreaterThan(100);
    for (const declaration of glass) {
      expect(declaration.context[0], `${declaration.selector} { ${declaration.property} }`).toBe("@layer app");
    }
    expect(glassCss.replace(/\/\*[\s\S]*?\*\//g, "")).not.toMatch(/\[data-theme/);
    const styles = read("src/app/styles.ts");
    const order = ["./hc-mono-tokens.css", "./hc-mono-glass.css", "./globals.css"].map((sheet) => styles.indexOf(`"${sheet}"`));
    expect(order.every((index) => index >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });

  it("live glass: grain, sheen and fill, a greyed blur, highlight, lowlight and shadow, and the masked edge ring", () => {
    const live = ruleOf(".hc-glass");
    expect(live["background-color"]).toBe("transparent");
    expect(live["background-image"]).toMatch(/^var\(--hc-glass-grain\),\s*linear-gradient\(105deg[^)]*var\(--hc-glass-sheen\)/);
    expect(live["background-image"]).toContain("linear-gradient(180deg, var(--hc-g-top) 0%, var(--hc-g-bottom) 100%)");
    expect(live["background-size"]).toBe("128px 128px, 720px 100%, 100% 100%");
    expect(live["backdrop-filter"]).toBe("blur(var(--hc-g-blur)) grayscale(1)");
    expect(live["-webkit-backdrop-filter"]).toBe("blur(var(--hc-g-blur)) grayscale(1)");
    expect(live["box-shadow"]).toMatch(/inset 0 1px 0 var\(--hc-glass-highlight\),\s*inset 0 -1px 0 var\(--hc-glass-lowlight\),\s*var\(--hc-g-shadow\)/);
    const ring = ruleOf(RING);
    expect(ring.mask).toContain("exclude");
    expect(ring["-webkit-mask-composite"]).toBe("xor");
    // Tertiary text is remapped on live glass (it fails there in dark).
    expect(ruleOf(".hc-glass:not(.hc-glass--static)")["--hc-text-tertiary"]).toBe("var(--hc-text-secondary)");
    for (const level of [2, 3]) {
      expect(ruleOf(`.hc-glass--${level}`)["--hc-g-blur"]).toBe(`var(--hc-glass-${level}-blur)`);
    }
  });

  it("the tertiary remap reaches the name the components read (Astryx's --color-text-disabled), on live glass and static glass/3", () => {
    // Astryx's disabled Text and Heading colour is a literal, not a var() of --hc-text-tertiary, so remapping the
    // token alone would change nothing on screen.
    expect(themeCss).toMatch(/--color-text-disabled:\s*light-dark\(#575757, #9E9E9E\)/);
    expect(themeCss).toMatch(/\[data-color="disabled"\]\s*\{\s*color:\s*var\(--color-text-disabled\)/);
    for (const selector of [".hc-glass:not(.hc-glass--static)", ".hc-glass--static:where(.hc-glass--3)"]) {
      expect(ruleOf(selector)["--hc-text-tertiary"], selector).toBe("var(--hc-text-secondary)");
      expect(ruleOf(selector)["--color-text-disabled"], selector).toBe("var(--hc-text-secondary)");
    }
  });

  it("the root class .hc-transparency-reduced has no setter yet: a future one is reviewed with these rules", { timeout: IO_BOUND_TEST_TIMEOUT_MS }, () => {
    // palette-mono 3.6 plans a Settings toggle; until it lands, only the OS setting applies. When a component or
    // script starts setting the class, this test fails and the toggle (i18n in four locales, the theme script's
    // CSP hash) gets reviewed with the CSS it switches on.
    const files = readdirSync(path.join(appDir, "src"), { recursive: true, encoding: "utf8" })
      .filter((file) => /\.(tsx?|mjs|js)$/.test(file))
      .map((file) => file.split(path.sep).join("/"));
    const setters = files.filter((file) => readFileSync(path.join(appDir, "src", file), "utf8").includes("hc-transparency-reduced"));
    expect(setters).toEqual([]);
    expect(glassCss).toContain(":where(.hc-transparency-reduced) .hc-glass {");
  });

  it("the live glass radius is the container radius token (6px), not a literal", () => {
    expect(ruleOf(".hc-glass")["border-radius"]).toBe("var(--radius-container)");
    expect(themeCss).toMatch(/--radius-container:\s*6px;/);
    // regions stay square
    expect(ruleOf(".hc-glass--region")["border-radius"]).toBe("0");
  });

  it("static glass and the floating card's static form: opaque stops, no blur, a border-box edge, no pseudo-element", () => {
    for (const [selector, at, level] of [
      [".hc-glass--static", [], 1],
      [".hc-glass--float", ["@media (min-width: 672px)"], 2],
    ] as const) {
      const rule = ruleOf(selector, [...at]);
      expect(rule["--hc-g-top"], selector).toBe(`var(--hc-glass-${level}-top-opaque)`);
      expect(rule["--hc-g-bottom"], selector).toBe(`var(--hc-glass-${level}-bottom-opaque)`);
      expect(rule["backdrop-filter"], selector).toBe("none");
      expect(rule["-webkit-backdrop-filter"], selector).toBe("none");
      expect(rule.border, selector).toBe("1px solid transparent");
      expect(rule["background-clip"], selector).toBe("padding-box, padding-box, border-box");
      expect(rule["background-image"], selector).not.toContain("--hc-glass-sheen");
    }
    expect(ruleOf(".hc-glass--float::before", ["@media (min-width: 672px)"]).display).toBe("none");
    // The ring never reaches static glass, so a card's own ::before (the Overview step number) is untouched.
    expect(glass.filter(({ selector }) => selectorsOf(selector).includes(".hc-glass::before"))).toEqual([]);
    expect(glass.filter(({ selector }) => selectorsOf(selector).includes(".hc-glass--static::before"))).toEqual([]);
  });

  it("has every fallback of palette-mono 3.6", () => {
    // No backdrop-filter: the opaque stops on every level.
    for (const [selector, level] of [[".hc-glass", 1], [".hc-glass--2", 2], [".hc-glass--3", 3]] as const) {
      expect(ruleOf(selector, [SUPPORTS_NOT])["--hc-g-top"], selector).toBe(`var(--hc-glass-${level}-top-opaque)`);
      expect(ruleOf(selector, [SUPPORTS_NOT])["--hc-g-bottom"], selector).toBe(`var(--hc-glass-${level}-bottom-opaque)`);
    }
    // Reduced transparency (OS setting and root class): opaque stops and colour, no blur, an opaque scrim.
    for (const [selector, at] of [
      [".hc-glass", [REDUCED_TRANSPARENCY]],
      [":where(.hc-transparency-reduced) .hc-glass", []],
    ] as const) {
      const rule = ruleOf(selector, [...at]);
      expect(rule["--hc-g-top"], selector).toBe("var(--hc-glass-1-top-opaque)");
      expect(rule["background-color"], selector).toBe("var(--hc-g-bottom)");
      expect(rule["backdrop-filter"], selector).toBe("none");
      expect(rule["-webkit-backdrop-filter"], selector).toBe("none");
    }
    for (const level of [2, 3]) {
      expect(ruleOf(`.hc-glass--${level}`, [REDUCED_TRANSPARENCY])["--hc-g-top"]).toBe(`var(--hc-glass-${level}-top-opaque)`);
      expect(ruleOf(`:where(.hc-transparency-reduced) .hc-glass--${level}`)["--hc-g-top"]).toBe(`var(--hc-glass-${level}-top-opaque)`);
    }
    expect(ruleOf(".hc-dialog::backdrop").background).toBe("var(--hc-scrim)");
    expect(ruleOf(".hc-dialog::backdrop", [REDUCED_TRANSPARENCY]).background).toBe("var(--hc-scrim-opaque)");
    expect(ruleOf(":where(.hc-transparency-reduced) .hc-dialog::backdrop").background).toBe("var(--hc-scrim-opaque)");
    // More contrast: flat, a strong ring, no backdrop, no edge pseudo.
    const contrast = ruleOf(".hc-glass", [MORE_CONTRAST]);
    expect(contrast["background-image"]).toBe("none");
    expect(contrast["background-color"]).toBe("var(--hc-bg-popover)");
    expect(contrast["backdrop-filter"]).toBe("none");
    expect(contrast["box-shadow"]).toBe("0 0 0 1px var(--hc-border-strong)");
    expect(ruleOf(RING, [MORE_CONTRAST]).display).toBe("none");
    // Forced colours: system colours, a real border, nothing decorative.
    const forced = ruleOf(".hc-glass", [FORCED]);
    expect(forced.background).toBe("Canvas");
    expect(forced.color).toBe("CanvasText");
    expect(forced.border).toBe("1px solid CanvasText");
    expect(forced["backdrop-filter"]).toBe("none");
    expect(forced["box-shadow"]).toBe("none");
    expect(ruleOf(RING, [FORCED]).display).toBe("none");
    // Print: the surface and a strong border, no backdrop, no body gradient.
    const print = ruleOf(".hc-glass", [PRINT]);
    expect(print.background).toBe("var(--hc-bg-surface)");
    expect(print["backdrop-filter"]).toBe("none");
    expect(print["box-shadow"]).toBe("none");
    expect(ruleOf("body::before", [PRINT]).display).toBe("none");
    // The tokens print in the light scheme on every theme.
    expect(tokensCss).toMatch(/@media print\s*\{\s*:root,\s*\[data-theme\]\s*\{\s*color-scheme:\s*light/);
    // The drawer at the level 1 blur under 672px.
    expect(ruleOf(".hc-dialog--drawer", ["@media (max-width: 671.98px)"])["--hc-g-blur"]).toBe("var(--hc-glass-1-blur)");
  });

  it("each fallback comes after the rules it overrides (equal specificity wins by source order)", () => {
    const index = (needle: string) => glassCss.indexOf(needle);
    const lastBase = Math.max(index(".hc-glass--static {"), index(".hc-glass--float {"), index(".hc-glass--interactive {"));
    for (const fallback of [SUPPORTS_NOT, REDUCED_TRANSPARENCY, ":where(.hc-transparency-reduced) .hc-glass {", MORE_CONTRAST, "@media (forced-colors: active) {\n    .hc-glass {", PRINT]) {
      expect(index(fallback), fallback).toBeGreaterThan(lastBase);
    }
  });
});

describe("glass only where palette-mono 3.5 allows it", () => {
  // A table, a tag, an input, code or long text: always a solid surface.
  const FORBIDDEN = /(^|[\s>+~(,])(table|thead|tbody|tr|th|td|input|textarea|select|code|pre|article)\b|\.hc-timeline|\.hc-revision-table|\.astryx-badge|\.hc-tag\b|\.hc-code|\.hc-json|\.hc-select|\.hc-details|\.hc-article|\.hc-summary|\.hc-prose|\.hc-callout|\.astryx-text-input|\.astryx-textarea/;
  const GLASS_VALUE = /--hc-glass-(grain|sheen|\d-(top|bottom))|--hc-g-(top|bottom)\b/;

  it("no rule gives a forbidden surface a backdrop filter or a glass fill", () => {
    const offenders: string[] = [];
    for (const [sheet, list] of Object.entries(appSheets)) {
      for (const { selector, property, value } of list) {
        const blurs = /backdrop-filter$/.test(property) && value !== "none";
        const fills = /^background/.test(property) && GLASS_VALUE.test(value);
        if ((blurs || fills) && selectorsOf(selector).some((part) => FORBIDDEN.test(part))) {
          offenders.push(`${sheet}: ${selector} { ${property}: ${value} }`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });

  it("only the .hc-glass rule sets a live backdrop filter; globals.css sets none", () => {
    const live = Object.entries(appSheets).flatMap(([sheet, list]) =>
      list
        .filter(({ property, value }) => /backdrop-filter$/.test(property) && value !== "none")
        .map(({ selector, property, context }) => `${sheet}: ${[...context.slice(1), selector].join(" ")} { ${property} }`),
    );
    expect(live).toEqual([
      "src/app/hc-mono-glass.css: .hc-glass { -webkit-backdrop-filter }",
      "src/app/hc-mono-glass.css: .hc-glass { backdrop-filter }",
    ]);
    expect(appSheets["src/app/globals.css"]?.filter(({ property }) => /backdrop-filter$/.test(property))).toEqual([]);
  });

  it("globals.css touches the glass only to clear a static panel's border, on screen without forced colours", () => {
    const touching = (appSheets["src/app/globals.css"] ?? [])
      .filter(({ selector }) => /hc-glass/.test(selector))
      .map(({ selector, property, value, context }) => `${[...context.slice(1), selector].join(" ")} { ${property}: ${value} }`);
    expect(touching).toEqual([
      "@media screen and (forced-colors: none) .hc-panel:where(.hc-glass--static) { border-color: transparent }",
    ]);
  });

  it("globals.css never clears the border of a glass element where the glass draws a real one (forced colours, print)", () => {
    // On screen the glass edge is its ring or a border-box layer, so a component may reset its border there; in forced
    // colours and print the ring is hidden and .hc-glass's 1px border is the boundary (palette-mono 3.6, WCAG 1.4.11).
    const GLASS_ELEMENTS = [
      ".hc-topbar", ".hc-sidebar", ".hc-dialog", ".hc-panel", ".hc-flow-step", ".hc-card", ".hc-run-card",
      ".hc-scenario-card", ".hc-run-header", ".hc-decision-card",
    ];
    const BORDER = /^border(-(block|inline|top|right|bottom|left))?(-(start|end))?(-(style|width))?$/;
    const clears = (value: string) => /^(0|0px|none|hidden)(\s|$)/.test(value);
    const resets = (appSheets["src/app/globals.css"] ?? [])
      .filter(({ selector, property, value }) =>
        BORDER.test(property) && clears(value) && selectorsOf(selector).some((part) => GLASS_ELEMENTS.includes(part)),
      )
      .map(({ selector, property, value, context }) => `${[...context.slice(1), selector].join(" ")} { ${property}: ${value} }`);
    expect(resets).toEqual([
      // the top bar keeps the bottom line it draws on screen, in CanvasText
      "@media (forced-colors: active) .hc-topbar { border-width: 0 0 1px }",
      // the dialogs' screen-only reset
      "@media screen and (forced-colors: none) .hc-dialog { border: 0 }",
    ]);
  });

  it("dense tables sit on a solid surface", () => {
    const table = appSheets["src/app/globals.css"]?.find(
      ({ selector, property, context }) => selector === ".hc-timeline" && property === "background-color" && context.length === 1,
    );
    expect(table?.value).toBe("var(--color-background-surface)");
  });

  it(
    "live glass sits only on the top bar, the two modal dialogs and the decision card; no glass class on a forbidden element",
    () => {
      const files = readdirSync(path.join(appDir, "src"), { recursive: true, encoding: "utf8" })
        .filter((file) => file.endsWith(".tsx"))
        .map((file) => file.split(path.sep).join("/"));
      const live: string[] = [];
      const statics: string[] = [];
      const wrong: string[] = [];
      for (const file of files) {
        const source = readFileSync(path.join(appDir, "src", file), "utf8");
        for (const match of source.matchAll(/className="([^"]*\bhc-glass\b[^"]*)"/g)) {
          const classes = match[1] ?? "";
          // The element: the last JSX tag opened before the attribute.
          const tag = [...source.slice(0, match.index).matchAll(/<([A-Za-z]\w*)\b/g)].at(-1)?.[1] ?? "";
          const names = classes.split(/\s+/);
          if (/^(table|thead|tbody|tr|th|td|input|textarea|select|code|pre|article|Badge|TextInput|TextArea)$/.test(tag)) {
            wrong.push(`${file}: <${tag} className="${classes}">`);
          }
          (names.includes("hc-glass--static") ? statics : live).push(`${file}: ${classes}`);
          if (!names.includes("hc-glass")) {
            wrong.push(`${file}: ${classes} (a modifier without .hc-glass)`);
          }
        }
        for (const match of source.matchAll(/className="([^"]*\bhc-glass--[^"]*)"/g)) {
          if (!(match[1] ?? "").split(/\s+/).includes("hc-glass")) {
            wrong.push(`${file}: ${match[1]} (a modifier without .hc-glass)`);
          }
        }
      }
      expect(wrong).toEqual([]);
      expect(live.sort()).toEqual([
        "app/[locale]/layout.tsx: hc-topbar hc-glass hc-glass--region",
        "components/MobileNav.tsx: hc-dialog hc-dialog--drawer hc-glass hc-glass--3",
        "components/Tour.tsx: hc-dialog hc-dialog--tour hc-glass hc-glass--3",
        "components/run/DecisionTab.tsx: hc-section hc-decision-card hc-glass hc-glass--2 hc-glass--float",
      ]);
      // The two overlays are native modal dialogs (top layer), never nested in the top bar's backdrop root.
      expect(read("src/components/Modal.tsx")).toMatch(/dialog\.showModal\(\)/);
      expect(read("src/components/Modal.tsx")).toMatch(/<dialog\b/);
      // Static glass: the sidebar, the run header, the cards of lists and the Settings panels.
      expect(statics.length).toBeGreaterThanOrEqual(12);
      for (const expected of [
        "app/[locale]/layout.tsx: hc-sidebar hc-glass hc-glass--static hc-glass--region",
        "components/run/RunWorkspace.tsx: hc-run-header hc-panel hc-glass hc-glass--static",
        "components/runs/RunsList.tsx: hc-run-card hc-glass hc-glass--static hc-glass--interactive",
        "components/showcase/ShowcaseList.tsx: hc-run-card hc-glass hc-glass--static hc-glass--interactive",
        "app/[locale]/page.tsx: hc-flow-step hc-glass hc-glass--static",
        "components/SettingsForm.tsx: hc-panel hc-glass hc-glass--static",
      ]) {
        expect(statics).toContain(expected);
      }
    },
    IO_BOUND_TEST_TIMEOUT_MS,
  );
});

describe("the primary button: ink material", () => {
  it("an ink gradient with the button grain, a highlight and a lowlight", () => {
    const rule = ruleOf(PRIMARY);
    expect(rule).toEqual(ruleOf(".hc-btn--primary"));
    expect(rule["background-image"]).toBe(
      "var(--hc-btn-grain), linear-gradient(180deg, var(--hc-btn-top) 0%, var(--hc-btn-bottom) 100%)",
    );
    expect(rule["background-color"]).toBe("var(--hc-btn-bottom)");
    expect(rule.color).toBe("var(--hc-btn-text)");
    expect(rule["box-shadow"]).toMatch(/^inset 0 1px 0 var\(--hc-btn-highlight\), inset 0 -1px 0 var\(--hc-btn-lowlight\)/);
    const hover = ruleOf(`${PRIMARY}:hover:not(:disabled, [aria-disabled="true"])`);
    expect(hover["background-image"]).toBe(
      "var(--hc-btn-grain), linear-gradient(180deg, var(--hc-btn-hover-top) 0%, var(--hc-btn-hover-bottom) 100%)",
    );
  });

  it("pressed (:active): flat btn/hover-bottom, no shadow, after the hover rule", () => {
    for (const selector of [`${PRIMARY}:active:not(:disabled, [aria-disabled="true"])`, ".hc-btn--primary:active"]) {
      const active = ruleOf(selector);
      expect(active["background-image"], selector).toBe("none");
      expect(active["background-color"], selector).toBe("var(--hc-btn-hover-bottom)");
      expect(active["box-shadow"], selector).toBe("none");
    }
    expect(glassCss.indexOf(`${PRIMARY}:active`)).toBeGreaterThan(glassCss.indexOf(`${PRIMARY}:hover`));
  });

  it("forced colours: the system button, hovered and pressed too", () => {
    for (const selector of [PRIMARY, `${PRIMARY}:is(:hover, :active):not(:disabled, [aria-disabled="true"])`, ".hc-btn--primary"]) {
      const forced = ruleOf(selector, [FORCED]);
      expect(forced.background, selector).toBe("ButtonFace");
      expect(forced.color, selector).toBe("ButtonText");
      expect(forced["border-color"], selector).toBe("ButtonText");
      expect(forced["box-shadow"], selector).toBe("none");
    }
  });
});

// ------------------------------------------------------------------ the tiles

type Png = { width: number; height: number; colorType: number; depth: number; pixels: Uint8Array };

/** A non-interlaced 8-bit PNG, unfiltered into its raw pixel bytes. */
function readPng(file: string): Png {
  const bytes = readFileSync(file);
  expect(bytes.subarray(0, 8).toString("hex")).toBe("89504e470d0a1a0a");
  let offset = 8;
  let width = 0;
  let height = 0;
  let depth = 0;
  let colorType = 0;
  const data: Buffer[] = [];
  while (offset < bytes.length) {
    const length = bytes.readUInt32BE(offset);
    const type = bytes.subarray(offset + 4, offset + 8).toString("latin1");
    const body = bytes.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      depth = body[8] ?? 0;
      colorType = body[9] ?? 0;
      expect(body[12], "interlace").toBe(0);
    } else if (type === "IDAT") {
      data.push(body);
    }
    offset += 12 + length;
  }
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[colorType] ?? 0;
  expect(depth).toBe(8);
  expect(channels).toBeGreaterThan(0);
  const raw = inflateSync(Buffer.concat(data));
  const stride = width * channels;
  const pixels = new Uint8Array(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)] ?? 0;
    for (let x = 0; x < stride; x += 1) {
      const value = raw[y * (stride + 1) + 1 + x] ?? 0;
      const left = x >= channels ? (pixels[y * stride + x - channels] ?? 0) : 0;
      const up = y > 0 ? (pixels[(y - 1) * stride + x] ?? 0) : 0;
      const upLeft = x >= channels && y > 0 ? (pixels[(y - 1) * stride + x - channels] ?? 0) : 0;
      const paeth = () => {
        const p = left + up - upLeft;
        const [pa, pb, pc] = [Math.abs(p - left), Math.abs(p - up), Math.abs(p - upLeft)];
        return pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
      };
      const predictor = [0, left, up, Math.floor((left + up) / 2), paeth()][filter] ?? 0;
      pixels[y * stride + x] = (value + predictor) & 0xff;
    }
  }
  return { width, height, colorType, depth, pixels };
}

/** The largest alpha of the white and of the black pixels of a grey + alpha tile, as 0..1. */
function grainAlpha(png: Png): { white: number; black: number } {
  expect(png.colorType).toBe(4);
  let white = 0;
  let black = 0;
  for (let index = 0; index < png.pixels.length; index += 2) {
    const grey = png.pixels[index] ?? 0;
    const alpha = (png.pixels[index + 1] ?? 0) / 255;
    if (grey === 255) white = Math.max(white, alpha);
    else if (grey === 0) black = Math.max(black, alpha);
    else expect.unreachable(`grain pixel grey ${grey}: the tiles are white or black with an alpha`);
  }
  return { white, black };
}

const brand = (name: string) => path.join(appDir, "public/brand", name);

describe("the grain tiles", () => {
  const urls = [...new Set([...`${tokensCss}\n${glassCss}`.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/url\("([^"]+)"\)/g)].map((match) => match[1] ?? ""))];

  it("every /brand/ tile the glass tokens name is shipped, same-origin, and the policy allows same-origin images", () => {
    const grain = urls.filter((url) => /brushed-/.test(url));
    expect(grain.sort()).toEqual([
      "/brand/brushed-dark.png",
      "/brand/brushed-dark@2x.png",
      "/brand/brushed-light.png",
      "/brand/brushed-light@2x.png",
    ]);
    for (const url of urls) {
      // Absolute, same-origin paths: a relative url() in a custom property resolves against the sheet using it.
      expect(url, url).toMatch(/^\/brand\/[\w@.-]+\.png$/);
    }
    for (const url of grain) {
      expect(() => readFileSync(path.join(appDir, "public", url)), url).not.toThrow();
    }
    // CSS background images fall under img-src: 'self' covers /brand/, no new directive is needed.
    expect(contentSecurityPolicy({ scriptHashes: [], styleHashes: [] })).toContain("img-src 'self'");
  });

  it("the tiles are 128 px (1x) and 256 px (2x) squares, as background-size: 128px and image-set() expect", () => {
    for (const theme of ["light", "dark"]) {
      expect(readPng(brand(`brushed-${theme}.png`))).toMatchObject({ width: 128, height: 128, colorType: 4 });
      expect(readPng(brand(`brushed-${theme}@2x.png`))).toMatchObject({ width: 256, height: 256, colorType: 4 });
    }
    expect(glassCss.match(/128px 128px/g)?.length).toBeGreaterThanOrEqual(4);
  });

  it("the grain stays within palette-mono's alpha limits (.06 light, .04 dark)", () => {
    for (const [theme, limit] of [["light", 0.06], ["dark", 0.04]] as const) {
      for (const file of [`brushed-${theme}.png`, `brushed-${theme}@2x.png`]) {
        const { white, black } = grainAlpha(readPng(brand(file)));
        expect(Math.max(white, black), file).toBeLessThanOrEqual(limit);
        expect(Math.min(white, black), file).toBeGreaterThan(0);
      }
    }
  });
});

// ------------------------------------------------------------------ contrast on the shipped values

type Rgba = [number, number, number, number];
type Theme = "light" | "dark";

function parseColour(text: string): Rgba {
  const hex = /^#([0-9a-fA-F]{6})$/.exec(text.trim());
  if (hex?.[1] !== undefined) {
    const value = hex[1];
    return [0, 2, 4].map((at) => Number.parseInt(value.slice(at, at + 2), 16)).concat(1) as Rgba;
  }
  const rgb = /^rgb\((\d+)\s+(\d+)\s+(\d+)(?:\s*\/\s*([\d.]+))?\)$/.exec(text.trim());
  if (rgb === null) throw new Error(`not a colour: ${text}`);
  return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), rgb[4] === undefined ? 1 : Number(rgb[4])];
}

/** A light-dark() token of `source` (the first :root definition), parsed per theme. */
function tokenOf(name: string, source: string): Record<Theme, Rgba> {
  const match = new RegExp(`${name}:\\s*light-dark\\(((?:[^()]|\\([^()]*\\))+?),\\s*((?:[^()]|\\([^()]*\\))+)\\);`).exec(source);
  if (match?.[1] === undefined || match[2] === undefined) throw new Error(`${name} is not a light-dark() pair`);
  return { light: parseColour(match[1]), dark: parseColour(match[2]) };
}

/** Source-over in sRGB, as browsers composite. */
function over(top: Rgba, bottom: [number, number, number]): [number, number, number] {
  const alpha = top[3];
  return [0, 1, 2].map((channel) => alpha * (top[channel] ?? 0) + (1 - alpha) * (bottom[channel] ?? 0)) as [number, number, number];
}

function luminance([r, g, b]: [number, number, number]): number {
  const lin = (value: number) => {
    const v = value / 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(a: [number, number, number], b: [number, number, number]): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((high ?? 0) + 0.05) / ((low ?? 0) + 0.05);
}

const solid = (colour: Rgba): [number, number, number] => {
  expect(colour[3]).toBe(1);
  return [colour[0], colour[1], colour[2]];
};

describe("palette-mono's worst-case contrast on the shipped tokens and tiles (its checker, section D)", () => {
  const grain = {
    light: grainAlpha(readPng(brand("brushed-light.png"))),
    dark: grainAlpha(readPng(brand("brushed-dark.png"))),
  };
  const backdrops: [number, number, number][] = [[255, 255, 255], [11, 11, 11]];
  /** Text (4.5:1) and boundaries (3:1) that sit on glass: the theme's text tokens, the strong border, the focus ring. */
  const items: [string, string, number][] = [
    ["--color-text-primary", themeCss, 4.5],
    ["--color-text-secondary", themeCss, 4.5],
    ["--hc-text-primary", tokensCss, 4.5],
    ["--hc-text-secondary", tokensCss, 4.5],
    ["--hc-color-border-strong", themeCss, 3],
    ["--hc-border-strong", tokensCss, 3],
    ["--hc-focus-ring", tokensCss, 3],
  ];

  /** Every composite of one glass: backdrop x stop x sheen x grain (live), or opaque stop x grain (static). */
  function composites(theme: Theme, level: number, live: boolean): [number, number, number][] {
    const stops = live
      ? [`--hc-glass-${level}-top`, `--hc-glass-${level}-bottom`]
      : [`--hc-glass-${level}-top-opaque`, `--hc-glass-${level}-bottom-opaque`];
    const sheen = tokenOf("--hc-glass-sheen", tokensCss)[theme];
    const { white, black } = grain[theme];
    const result: [number, number, number][] = [];
    for (const backdrop of live ? backdrops : [[0, 0, 0] as [number, number, number]]) {
      for (const stop of stops) {
        const fill = tokenOf(stop, tokensCss)[theme];
        if (!live) expect(fill[3], stop).toBe(1);
        for (const withSheen of live ? [false, true] : [false]) {
          let base = over(fill, backdrop);
          if (withSheen) base = over(sheen, base);
          result.push(base, over([255, 255, 255, white], base), over([0, 0, 0, black], base));
        }
      }
    }
    return result;
  }

  for (const theme of ["light", "dark"] as const) {
    for (const level of [1, 2, 3]) {
      for (const live of [true, false]) {
        it(`${theme}, ${live ? "live" : "static"} glass/${level}`, () => {
          const all = composites(theme, level, live);
          expect(all.length).toBe(live ? 24 : 6);
          for (const [name, source, need] of items) {
            const colour = solid(tokenOf(name, source)[theme]);
            const worst = Math.min(...all.map((composite) => ratio(colour, composite)));
            expect(worst, `${name} on ${theme} ${live ? "live" : "static"} glass/${level}: ${worst.toFixed(2)}`).toBeGreaterThanOrEqual(need);
          }
        });
      }
    }

    it(`${theme}: the primary button label on every stop of its gradient, with the button grain`, () => {
      // The button grain is the light tile in both themes (hc-mono-tokens.css declares it once).
      expect(tokensCss).toMatch(/--hc-btn-grain:\s*image-set\(url\("\/brand\/brushed-light\.png"\) 1x/);
      const { white, black } = grain.light;
      const label = solid(tokenOf("--hc-btn-text", tokensCss)[theme]);
      for (const stop of ["--hc-btn-top", "--hc-btn-bottom", "--hc-btn-hover-top", "--hc-btn-hover-bottom"]) {
        const fill = solid(tokenOf(stop, tokensCss)[theme]);
        const worst = Math.min(
          ...[fill, over([255, 255, 255, white], fill), over([0, 0, 0, black], fill)].map((composite) => ratio(label, composite)),
        );
        expect(worst, `button label on ${stop} (${theme}): ${worst.toFixed(2)}`).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
});
