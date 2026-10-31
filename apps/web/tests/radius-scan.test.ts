// SPDX-License-Identifier: Apache-2.0
//
// Square-first geometry (design-direction.md 3.7): every corner the app
// ships is 0, 2, 4 or 6 px. This test reads the stylesheets the build
// bundles (the ones src/app/styles.ts imports, in its order: Astryx's reset
// and component sheet, the compiled theme, the token layer, the app's
// rules) and, when the export exists (`pnpm build` writes out/), the built
// CSS itself. The app's own stylesheets must stay on the scale with no
// exception; a value off the scale (a full radius, 9999px or 50%, included)
// may appear in Astryx's sheet only in a class of tests/radius-scan.ts's
// allow-list, each with its reason, and no entry of the list may be stale.
// e2e/shapes.spec.ts checks the built CSS on every run (the e2e suite
// always runs on a build) and the corners the browser computes.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import type { Declaration } from "./colour-scan";
import { IO_BOUND_TEST_TIMEOUT_MS } from "./io-timeout";
import {
  allowedFinding,
  classify,
  evaluate,
  pageTokens,
  radiusFindings,
  radiusValues,
  ruleClass,
  THEME_TOKEN_ALLOW,
  VENDOR_ALLOW_LIST,
} from "./radius-scan";

const appDir = fileURLToPath(new URL("..", import.meta.url));
const read = (relative: string) => readFileSync(path.join(appDir, relative), "utf8");

/** The sheets styles.ts imports, in cascade order. */
const SHIPPED = [
  "node_modules/@astryxdesign/core/src/reset.css",
  "node_modules/@astryxdesign/core/dist/astryx.css",
  "src/theme/hyphae-mission.css",
  "src/app/hc-mono-tokens.css",
  "src/app/hc-mono-glass.css",
  "src/app/globals.css",
] as const;

describe("the radius evaluator", () => {
  const tokens = pageTokens(`
    :root { --radius-inner: 4px; --radius-element: 8px; --spacing-1: 0.25rem; }
    @layer astryx-theme { :root, [data-astryx-theme="x"] { --radius-inner: 2px; --radius-element: 4px; --radius-full: 9999px; } }
    .local { --_card-radius: 12px; }
  `);

  it("lets the theme layer win over the base defaults", () => {
    expect(tokens.get("--radius-element")).toEqual(["4px"]);
    expect(tokens.get("--_card-radius")).toBeUndefined();
  });

  it("evaluates literals, var(), calc(), max() and shorthands", () => {
    const values = (value: string) => radiusValues(value, tokens).map(classify);
    expect(evaluate("var(--radius-element)", tokens)).toEqual([{ kind: "px", value: 4 }]);
    expect(evaluate("calc(var(--radius-element) - var(--spacing-1))", tokens)).toEqual([{ kind: "px", value: 0 }]);
    expect(evaluate("max(0px, calc(var(--radius-inner) - 1px))", tokens)).toEqual([{ kind: "px", value: 1 }]);
    expect(evaluate("var(--_card-radius, var(--radius-inner))", tokens)).toEqual([
      { kind: "local", value: "--_card-radius" },
      { kind: "px", value: 2 },
    ]);
    expect(evaluate("var(--_card-radius)", tokens)).toEqual([{ kind: "local", value: "--_card-radius" }]);
    expect(values("0 var(--radius-element) 6px 2px")).toEqual(["scale", "scale", "scale", "scale"]);
    expect(values("4px / 2px")).toEqual(["scale", "scale"]);
    expect(values("inherit")).toEqual(["scale"]);
    expect(values("var(--radius-full)")).toEqual(["full"]);
    expect(values("50%")).toEqual(["full"]);
    expect(values("1px")).toEqual(["off-scale"]);
    expect(values("8px")).toEqual(["off-scale"]);
    expect(values("calc(var(--radius-element) - 1px)")).toEqual(["off-scale"]);
    // A component-local var passes on its own (checked where declared); its off-scale fallback does not.
    expect(values("var(--x-unknown-length, 3px)")).toEqual(["scale", "off-scale"]);
    expect(values("calc(100% - 4px)")).toEqual(["unresolved"]);
    expect(values("max(0px, calc(var(--_card-radius) - var(--spacing-1)))")).toEqual(["unresolved"]);
  });

  it("finds every radius declaration, the custom properties that carry one included", () => {
    const { scanned, findings } = radiusFindings(
      `.a { border-radius: 2px; } .b { border-start-end-radius: 50%; } .c { --_x-radius: 7px; color: red; }`,
      tokens,
    );
    expect(scanned).toBe(3);
    expect(findings.map(({ declaration, values, corner }) => `${declaration.selector} ${values.join(" ")} ${corner}`)).toEqual([
      ".b 50% full",
      ".c 7px off-scale",
    ]);
  });
});

describe("the theme's radius tokens are the scale", () => {
  it("none 0, inner 2, element 4, container and page 6", () => {
    const tokens = pageTokens(read("src/theme/hyphae-mission.css"));
    for (const [token, px] of [
      ["--radius-none", 0],
      ["--radius-inner", 2],
      ["--radius-element", 4],
      ["--radius-container", 6],
      ["--radius-page", 6],
    ] as const) {
      expect(evaluate(`var(${token})`, tokens), token).toEqual([{ kind: "px", value: px }]);
    }
  });
});

/** "selector { property: value } -> values" for a failure message. */
function describeFindings(
  css: string,
  tokens: ReturnType<typeof pageTokens>,
  allowed: (declaration: Declaration) => boolean,
) {
  const { scanned, findings } = radiusFindings(css, tokens);
  const offenders = findings
    .filter(({ declaration }) => !allowed(declaration))
    .map(({ declaration, values }) => `${declaration.selector} { ${declaration.property}: ${declaration.value} } -> ${values.join(", ")}`);
  const used = new Set(findings.map(({ declaration }) => ruleClass(declaration.selector)));
  return { scanned, findings, offenders, used };
}

describe("the stylesheets the build bundles", () => {
  const css = SHIPPED.map(read).join("\n");
  const tokens = pageTokens(css);

  it.each(["src/app/globals.css", "src/app/hc-mono-tokens.css", "src/app/hc-mono-glass.css"])(
    "%s keeps every corner on the scale, with no exception",
    (sheet) => {
      const { scanned, offenders } = describeFindings(read(sheet), tokens, () => false);
      if (sheet === "src/app/globals.css") {
        expect(scanned).toBeGreaterThan(40);
      }
      expect(offenders).toEqual([]);
    },
  );

  it("the compiled theme is off the scale only in its --radius-full token, which nothing of the app uses", () => {
    const { offenders, findings } = describeFindings(read("src/theme/hyphae-mission.css"), tokens, allowedFinding);
    expect(offenders).toEqual([]);
    expect(findings.map(({ declaration }) => declaration.property)).toEqual(Object.keys(THEME_TOKEN_ALLOW));
    for (const sheet of ["src/app/globals.css", "src/app/hc-mono-tokens.css", "src/app/hc-mono-glass.css"]) {
      expect(read(sheet).includes("--radius-full"), sheet).toBe(false);
    }
  });

  it(
    "Astryx's sheet is off the scale only in allow-listed classes, and every entry is still needed",
    () => {
      const vendor = SHIPPED.slice(0, 2).map(read).join("\n");
      const { scanned, findings, offenders, used } = describeFindings(vendor, tokens, allowedFinding);
      expect(scanned).toBeGreaterThan(50);
      expect(offenders).toEqual([]);
      // The exemption is used, and each entry names a class that needs it.
      expect(findings.length).toBeGreaterThan(5);
      expect(Object.keys(VENDOR_ALLOW_LIST).filter((name) => !used.has(name))).toEqual([]);
      for (const [name, reason] of Object.entries(VENDOR_ALLOW_LIST)) {
        expect(reason.length, name).toBeGreaterThan(20);
      }
    },
    IO_BOUND_TEST_TIMEOUT_MS,
  );

  it("no app component names an allow-listed Astryx class", () => {
    const sources = readdirSync(path.join(appDir, "src"), { recursive: true, encoding: "utf8" }).filter((file) =>
      /\.(ts|tsx|css)$/.test(file),
    );
    expect(sources.length).toBeGreaterThan(20);
    for (const file of sources) {
      const text = readFileSync(path.join(appDir, "src", file), "utf8");
      for (const name of Object.keys(VENDOR_ALLOW_LIST)) {
        expect(text.includes(name), `${file} names ${name}`).toBe(false);
      }
    }
  }, IO_BOUND_TEST_TIMEOUT_MS);
});

// Next.js writes every stylesheet of the export under out/_next/.
const OUT = path.join(appDir, "out", "_next");
const built = existsSync(OUT);

describe.runIf(built)("the built CSS (out/, from the last `pnpm build`)", () => {
  it(
    "is off the scale only in allow-listed Astryx classes",
    () => {
      const sheets = readdirSync(OUT, { recursive: true, encoding: "utf8" }).filter((file) => file.endsWith(".css"));
      expect(sheets.length).toBeGreaterThan(0);
      const css = sheets.map((sheet) => readFileSync(path.join(OUT, sheet), "utf8")).join("\n");
      const tokens = pageTokens(css);
      const { scanned, offenders } = describeFindings(css, tokens, allowedFinding);
      expect(scanned).toBeGreaterThan(100);
      expect(offenders).toEqual([]);
    },
    IO_BOUND_TEST_TIMEOUT_MS,
  );
});
