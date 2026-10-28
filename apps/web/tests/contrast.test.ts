// SPDX-License-Identifier: Apache-2.0
//
// WCAG 2.x contrast of the theme's text tokens on its surface tokens, read
// from the compiled theme (src/theme/hyphae-mission.css), in light and dark
// mode. AA needs 4.5:1 for body text. axe checks rendered pages too, but it
// cannot judge text over gradients or translucent glass; these pairs are the
// solid surfaces that text sits on.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync(
  new URL("../src/theme/hyphae-mission.css", import.meta.url),
  "utf8",
);

/** First definition of a token as light-dark(#light, #dark). */
function token(name: string): { light: string; dark: string } {
  const match = new RegExp(
    `${name}:\\s*light-dark\\((#[0-9a-fA-F]{6}),\\s*(#[0-9a-fA-F]{6})\\)`,
  ).exec(css);
  if (match === null || match[1] === undefined || match[2] === undefined) {
    throw new Error(`${name} is not a light-dark() pair of hex colors`);
  }
  return { light: match[1], dark: match[2] };
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((start) => {
    const value = Number.parseInt(hex.slice(start, start + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  const [r = 0, g = 0, b = 0] = channels;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((high ?? 0) + 0.05) / ((low ?? 0) + 0.05);
}

const texts = [
  "--color-text-primary",
  "--color-text-secondary",
  "--color-text-accent",
] as const;

const surfaces = [
  "--color-background-body",
  "--color-background-surface",
  "--color-background-card",
  "--color-background-popover",
  "--color-background-muted",
] as const;

describe.each(["light", "dark"] as const)("%s mode", (mode) => {
  it.each(texts.flatMap((text) => surfaces.map((surface) => [text, surface])))(
    "%s on %s meets 4.5:1",
    (text, surface) => {
      const ratio = contrast(token(text)[mode], token(surface)[mode]);
      expect(ratio, `${text} on ${surface}: ${ratio.toFixed(2)}`).toBeGreaterThanOrEqual(
        4.5,
      );
    },
  );

  it("keeps on-accent text readable on the accent fill", () => {
    const accent = token("--color-accent")[mode];
    const onAccent = /--color-on-accent:\s*light-dark\((#[0-9a-fA-F]{6}),\s*(#[0-9a-fA-F]{6})\)/i.exec(
      css,
    );
    expect(onAccent).not.toBeNull();
    const text = mode === "light" ? onAccent?.[1] : onAccent?.[2];
    expect(contrast(text ?? "#000000", accent)).toBeGreaterThanOrEqual(4.5);
  });
});

describe("contrast()", () => {
  it("matches the WCAG reference values", () => {
    expect(contrast("#000000", "#FFFFFF")).toBeCloseTo(21, 5);
    expect(contrast("#FFFFFF", "#0064E0")).toBeCloseTo(5.39, 1);
    expect(contrast("#777777", "#FFFFFF")).toBeCloseTo(4.48, 1);
  });
});
