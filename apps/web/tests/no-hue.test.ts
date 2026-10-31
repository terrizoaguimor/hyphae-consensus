// SPDX-License-Identifier: Apache-2.0
//
// The palette is fully monochromatic (owner direction 2026-10-31,
// design/palette-mono.md): every colour the app ships is a true grey, R = G
// = B, with a tolerance of 0. This test reads the theme output (the compiled
// hyphae-mission CSS and JS, which `pnpm theme:check` keeps in step with its
// source) and every stylesheet of the app, and fails on any colour literal
// with a hue. It also checks that every colour token Astryx's own base
// stylesheet defines with a hue is redeclared by the theme, so no Astryx
// component can resolve one inside the theme's scope (the whole page: <html>
// carries data-astryx-theme). e2e/no-hue.spec.ts checks the built CSS and the
// colours the browser actually renders.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { declarations, hueDeclarations, hueLiterals, isGreyLiteral, withoutDefinedFallbacks } from "./colour-scan";
import { IO_BOUND_TEST_TIMEOUT_MS } from "./io-timeout";

const appDir = fileURLToPath(new URL("..", import.meta.url));
const read = (relative: string) => readFileSync(path.join(appDir, relative), "utf8");

const themeCss = read("src/theme/hyphae-mission.css");
const themeJs = read("src/theme/hyphae-mission.js");

/** "selector { property: value }" for a failure message. */
function describeHues(css: string): string[] {
  return hueDeclarations(css).map(
    ({ declaration, hues }) =>
      `${declaration.selector} { ${declaration.property}: ${declaration.value} } (${hues.join(", ")})`,
  );
}

describe("the colour scanner", () => {
  it("tells a grey from a hue, with a tolerance of 0", () => {
    for (const grey of [
      "#FFFFFF",
      "#6e6e6e",
      "#0B0B0B8C",
      "#777",
      "rgb(11 11 11 / .55)",
      "rgba(0, 0, 0, 0.1)",
      "hsl(210 0% 40%)",
      "hwb(0 40% 60%)",
      "oklch(0 0 0 / 5%)",
      "oklab(0.5 0 0)",
      "color(srgb 0.2 0.2 0.2)",
      "black",
      "white",
      "transparent",
    ]) {
      expect(isGreyLiteral(grey), grey).toBe(true);
    }
    for (const hue of [
      "#0064E0",
      "#45E4D0",
      "#7A8496",
      "#F4F6F9",
      "#FEFFFF",
      "rgb(0 100 224 / 0.06)",
      "rgb(255, 255, 254)",
      "hsl(210 10% 40%)",
      "oklch(0.6 0.01 250)",
      "color(display-p3 1 0 0)",
      "red",
      "teal",
    ]) {
      expect(isGreyLiteral(hue), hue).toBe(false);
    }
  });

  it("reads declarations, not selectors or identifiers", () => {
    const css = `/* #0064E0 in a comment */
      @layer app { .x[data-variant="red"] { color: var(--color-text-red); border: 1px solid #F4F6F9; } }
      @media print { :root { --a: light-dark(#FFFFFF, #131313); } }`;
    expect(declarations(css)).toEqual([
      { property: "color", value: "var(--color-text-red)", selector: '.x[data-variant="red"]', context: ["@layer app"] },
      { property: "border", value: "1px solid #F4F6F9", selector: '.x[data-variant="red"]', context: ["@layer app"] },
      { property: "--a", value: "light-dark(#FFFFFF, #131313)", selector: ":root", context: ["@media print"] },
    ]);
    expect(describeHues(css)).toEqual(['.x[data-variant="red"] { border: 1px solid #F4F6F9 } (#F4F6F9)']);
    expect(hueLiterals("var(--color-border-pink) blueviolet")).toEqual(["blueviolet"]);
    // A fallback counts only while its token is undefined.
    const value = "var(--a, #0064E0) var(--b, rgb(1 2 3))";
    expect(hueLiterals(withoutDefinedFallbacks(value, new Set(["--a"])))).toEqual(["rgb(1 2 3)"]);
    expect(withoutDefinedFallbacks(value, new Set(["--a", "--b"]))).toBe("var(--a) var(--b)");
  });
});

describe("no hue in the theme output", () => {
  it("the compiled theme CSS declares greys only", () => {
    expect(declarations(themeCss).length).toBeGreaterThan(200);
    expect(describeHues(themeCss)).toEqual([]);
  });

  it("the compiled theme module holds greys only", () => {
    // Every token value of the module ("--name": "value"), and any colour
    // literal anywhere in it.
    const values = [...themeJs.matchAll(/"(--[\w-]+)":\s*"([^"]*)"/g)];
    expect(values.length).toBeGreaterThan(200);
    const hued = values
      .map(([, name, value]) => ({ name, hues: hueLiterals(value ?? "") }))
      .filter(({ hues }) => hues.length > 0);
    expect(hued).toEqual([]);
    // Hex and functional colours anywhere; colour names only as values above
    // (the component overrides are keyed "variant:red" and so on).
    const anywhere = hueLiterals(themeJs.replace(/\/\*[\s\S]*?\*\//g, "")).filter(
      (literal) => literal.startsWith("#") || literal.includes("("),
    );
    expect(anywhere).toEqual([]);
  });

  it("the old hues are gone: the Astryx accent, Hyphae cyan and the blue-black surfaces", () => {
    for (const old of ["#0064E0", "#4BA9FE", "#45E4D0", "#F3F7FC", "#0A0F1E", "#121A2D", "#7A8496", "#677388"]) {
      expect(themeCss.toUpperCase()).not.toContain(old);
      expect(themeJs.toUpperCase()).not.toContain(old);
    }
  });
});

describe("no hue in the app's stylesheets", () => {
  const sheets = readdirSync(path.join(appDir, "src"), { recursive: true, encoding: "utf8" })
    .filter((file) => file.endsWith(".css"))
    .map((file) => path.join("src", file).split(path.sep).join("/"))
    .sort();

  it("finds the stylesheets", () => {
    expect(sheets).toEqual(["src/app/globals.css", "src/app/hc-mono-tokens.css", "src/theme/hyphae-mission.css"]);
  });

  it.each(sheets)("%s declares greys only", (sheet) => {
    expect(describeHues(read(sheet))).toEqual([]);
  });

  it("the token layer is the palette-mono scale, inside @layer app, as light-dark() tuples", () => {
    const tokens = declarations(read("src/app/hc-mono-tokens.css"));
    for (const declaration of tokens) {
      expect(declaration.context[0], declaration.property).toBe("@layer app");
    }
    const scale = tokens.filter(({ property }) => /^--hc-mono-\d+$/.test(property)).map(({ value }) => value);
    expect(scale).toEqual([
      "#FFFFFF", "#F4F4F4", "#EAEAEA", "#DCDCDC", "#C2C2C2", "#9E9E9E", "#6E6E6E",
      "#575757", "#3A3A3A", "#2B2B2B", "#242424", "#1B1B1B", "#131313", "#0B0B0B",
    ]);
    const colours = tokens.filter(
      ({ property, selector, context }) =>
        selector === ":root" && context.length === 1 && /^--hc-(bg|text|border|accent|focus|role|body)-/.test(property),
    );
    expect(colours.length).toBeGreaterThan(60);
    for (const { property, value } of colours) {
      expect(value, property).toMatch(/^light-dark\(/);
    }
  });
});

describe("Astryx's base colours never reach a component", () => {
  // Astryx's stylesheet defines its default palette (blues, greens, the hue
  // families, the data-visualisation ramps) on :root in @layer astryx-base.
  // The theme layer comes later, and <html> carries data-astryx-theme, so a
  // token the theme redeclares resolves to the theme's grey everywhere.
  const base = readFileSync(path.join(appDir, "node_modules/@astryxdesign/core/dist/astryx.css"), "utf8");
  const themed = new Set(declarations(themeCss).map(({ property }) => property));

  it("redeclares every custom property the base stylesheet sets to a hue", () => {
    const hued = hueDeclarations(base).filter(({ declaration }) => declaration.property.startsWith("--"));
    expect(hued.length).toBeGreaterThan(100);
    const missing = [...new Set(hued.map(({ declaration }) => declaration.property))].filter(
      (property) => !themed.has(property),
    );
    expect(missing).toEqual([]);
  });

  it("names a hue in its reset only as the fallback of a token the theme defines", () => {
    const reset = readFileSync(path.join(appDir, "node_modules/@astryxdesign/core/src/reset.css"), "utf8");
    const hued = hueDeclarations(reset);
    expect(hued.length).toBeGreaterThan(0);
    for (const { declaration } of hued) {
      expect(hueLiterals(withoutDefinedFallbacks(declaration.value, themed)), declaration.value).toEqual([]);
    }
  });

  it("sets a hue directly on one class only: the Badge's test-violation fixture, never rendered", () => {
    const direct = hueDeclarations(base).filter(({ declaration }) => !declaration.property.startsWith("--"));
    expect(direct.map(({ declaration }) => `${declaration.selector} { ${declaration.property}: ${declaration.value} }`)).toEqual([
      ".xj8ax7g:not(#\\#):not(#\\#):not(#\\#) { color: #FF0000 }",
    ]);
    // The class belongs to Badge.test-violations.js, which the app never imports.
    const components = readdirSync(path.join(appDir, "src"), { recursive: true, encoding: "utf8" }).filter((file) =>
      /\.(ts|tsx)$/.test(file),
    );
    for (const file of components) {
      expect(readFileSync(path.join(appDir, "src", file), "utf8"), file).not.toMatch(/test-violations|xj8ax7g/);
    }
  }, IO_BOUND_TEST_TIMEOUT_MS);
});
