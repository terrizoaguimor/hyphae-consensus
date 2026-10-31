// SPDX-License-Identifier: Apache-2.0
//
// Finds colours with a hue in CSS text (owner direction 2026-10-31: the
// palette is fully monochromatic, design/palette-mono.md). A colour is grey
// when its red, green and blue are equal, with a tolerance of 0; alpha does
// not matter. Shared by tests/no-hue.test.ts (the theme output and the app's
// stylesheets) and e2e/no-hue.spec.ts (the built CSS and the rendered pages).

/** CSS named colours that are grey (R = G = B), plus the colourless keywords. */
const GREY_NAMES = new Set([
  "black",
  "white",
  "gray",
  "grey",
  "silver",
  "dimgray",
  "dimgrey",
  "darkgray",
  "darkgrey",
  "lightgray",
  "lightgrey",
  "gainsboro",
  "whitesmoke",
  "transparent",
  "currentcolor",
]);

/** Every other CSS named colour (CSS Color 4), each one with a hue. */
const HUE_NAMES = [
  "aliceblue", "antiquewhite", "aqua", "aquamarine", "azure", "beige", "bisque", "blanchedalmond", "blue",
  "blueviolet", "brown", "burlywood", "cadetblue", "chartreuse", "chocolate", "coral", "cornflowerblue",
  "cornsilk", "crimson", "cyan", "darkblue", "darkcyan", "darkgoldenrod", "darkgreen", "darkkhaki",
  "darkmagenta", "darkolivegreen", "darkorange", "darkorchid", "darkred", "darksalmon", "darkseagreen",
  "darkslateblue", "darkslategray", "darkslategrey", "darkturquoise", "darkviolet", "deeppink", "deepskyblue",
  "dodgerblue", "firebrick", "floralwhite", "forestgreen", "fuchsia", "gold", "goldenrod", "green",
  "greenyellow", "honeydew", "hotpink", "indianred", "indigo", "ivory", "khaki", "lavender", "lavenderblush",
  "lawngreen", "lemonchiffon", "lightblue", "lightcoral", "lightcyan", "lightgoldenrodyellow", "lightgreen",
  "lightpink", "lightsalmon", "lightseagreen", "lightskyblue", "lightslategray", "lightslategrey",
  "lightsteelblue", "lightyellow", "lime", "limegreen", "linen", "magenta", "maroon", "mediumaquamarine",
  "mediumblue", "mediumorchid", "mediumpurple", "mediumseagreen", "mediumslateblue", "mediumspringgreen",
  "mediumturquoise", "mediumvioletred", "midnightblue", "mintcream", "mistyrose", "moccasin", "navajowhite",
  "navy", "oldlace", "olive", "olivedrab", "orange", "orangered", "orchid", "palegoldenrod", "palegreen",
  "paleturquoise", "palevioletred", "papayawhip", "peachpuff", "peru", "pink", "plum", "powderblue", "purple",
  "rebeccapurple", "red", "rosybrown", "royalblue", "saddlebrown", "salmon", "sandybrown", "seagreen",
  "seashell", "sienna", "skyblue", "slateblue", "slategray", "slategrey", "snow", "springgreen", "steelblue",
  "tan", "teal", "thistle", "tomato", "turquoise", "violet", "wheat", "yellow", "yellowgreen",
];

// A name counts only as a whole word that is not part of an identifier
// (`var(--color-text-red)`, `.astryx-badge`, `"pink"` in a selector are not
// colours): no letter, digit, hyphen or underscore on either side.
const NAMED = new RegExp(`(?<![\\w-])(${[...HUE_NAMES, ...GREY_NAMES].join("|")})(?![\\w-])`, "gi");
const HEX = /(?<![\w-])#([0-9a-fA-F]{3,8})(?![\w-])/g;
const FUNCTION = /\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(([^()]*)\)/gi;

function number(part: string | undefined, percentScale = 1): number {
  if (part === undefined) {
    return Number.NaN;
  }
  const value = Number.parseFloat(part);
  return part.trim().endsWith("%") ? (value * percentScale) / 100 : value;
}

/** Whether one colour literal is grey; `undefined` when it is not a colour this scanner understands. */
export function isGreyLiteral(literal: string): boolean | undefined {
  const text = literal.trim().toLowerCase();
  if (GREY_NAMES.has(text)) {
    return true;
  }
  if (HUE_NAMES.includes(text)) {
    return false;
  }
  const hex = /^#([0-9a-f]{3,8})$/.exec(text)?.[1];
  if (hex !== undefined) {
    if (hex.length === 3 || hex.length === 4) {
      return hex[0] === hex[1] && hex[1] === hex[2];
    }
    if (hex.length === 6 || hex.length === 8) {
      return hex.slice(0, 2) === hex.slice(2, 4) && hex.slice(2, 4) === hex.slice(4, 6);
    }
    return undefined;
  }
  const call = /^([a-z]+)\((.*)\)$/s.exec(text);
  if (call === null) {
    return undefined;
  }
  const [, name = "", body = ""] = call;
  // Channels before an optional "/ alpha"; "none" counts as 0.
  const channels = (body.split("/")[0] ?? "")
    .split(/[\s,]+/)
    .filter(Boolean)
    .map((part) => (part === "none" ? "0" : part));
  switch (name) {
    case "rgb":
    case "rgba": {
      const [r, g, b] = channels.map((part) => number(part, 255));
      return r !== undefined && r === g && g === b;
    }
    case "hsl":
    case "hsla":
      return number(channels[1], 100) === 0;
    case "hwb":
      return number(channels[1], 100) + number(channels[2], 100) >= 100;
    case "lab":
    case "oklab":
      return number(channels[1], 1) === 0 && number(channels[2], 1) === 0;
    case "lch":
    case "oklch":
      return number(channels[1], 1) === 0;
    case "color": {
      const [, r, g, b] = channels;
      return r !== undefined && r === g && g === b;
    }
    default:
      return undefined;
  }
}

/** Every colour literal in a CSS value (or any text) whose R, G and B are not all equal. */
export function hueLiterals(value: string): string[] {
  const found: string[] = [];
  for (const match of value.matchAll(HEX)) {
    if (isGreyLiteral(match[0]) === false) {
      found.push(match[0]);
    }
  }
  for (const match of value.matchAll(FUNCTION)) {
    if (isGreyLiteral(match[0]) === false) {
      found.push(match[0]);
    }
  }
  for (const match of value.matchAll(NAMED)) {
    if (isGreyLiteral(match[0]) === false) {
      found.push(match[0]);
    }
  }
  return found;
}

/** One declaration of a stylesheet, with the at-rules and the selector around it. */
export type Declaration = {
  property: string;
  value: string;
  selector: string;
  /** Enclosing at-rule preludes, outermost first (e.g. ["@layer app", "@media print"]). */
  context: string[];
};

/**
 * The declarations of a stylesheet. A small brace-matching reader, enough
 * for the app's own CSS and for minified build output: comments and strings
 * are skipped, and only `property: value` pairs inside a block count, so a
 * selector such as `[data-variant="red"]` is never read as a colour.
 */
export function declarations(css: string): Declaration[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const result: Declaration[] = [];
  const stack: string[] = [];
  let buffer = "";
  let quote: string | null = null;
  const flush = () => {
    const chunk = buffer.trim();
    buffer = "";
    if (chunk === "" || stack.length === 0) {
      return;
    }
    const colon = chunk.indexOf(":");
    if (colon <= 0 || chunk.startsWith("@")) {
      return;
    }
    const selector = [...stack].reverse().find((entry) => !entry.startsWith("@")) ?? "";
    result.push({
      property: chunk.slice(0, colon).trim(),
      value: chunk.slice(colon + 1).trim(),
      selector,
      context: stack.filter((entry) => entry.startsWith("@")),
    });
  };
  let depth = 0;
  for (const char of text) {
    if (quote !== null) {
      buffer += char;
      if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      buffer += char;
      continue;
    }
    if (char === "(") {
      depth += 1;
    } else if (char === ")") {
      depth -= 1;
    }
    if (depth > 0) {
      buffer += char;
      continue;
    }
    if (char === "{") {
      stack.push(buffer.trim());
      buffer = "";
    } else if (char === "}") {
      flush();
      stack.pop();
    } else if (char === ";") {
      flush();
    } else {
      buffer += char;
    }
  }
  return result;
}

/**
 * `value` without the fallback of every `var(--name, fallback)` whose
 * property is in `defined`: a fallback is used only when its property is not
 * set, so a hue there never paints while the property is defined.
 */
export function withoutDefinedFallbacks(value: string, defined: ReadonlySet<string>): string {
  let result = "";
  let index = 0;
  while (index < value.length) {
    const match = /var\(\s*(--[\w-]+)\s*,/y;
    match.lastIndex = index;
    const found = match.exec(value);
    if (found === null || !defined.has(found[1] ?? "")) {
      result += value[index];
      index += 1;
      continue;
    }
    // Skip to the parenthesis that closes this var().
    let depth = 1;
    let end = match.lastIndex;
    while (end < value.length && depth > 0) {
      if (value[end] === "(") {
        depth += 1;
      } else if (value[end] === ")") {
        depth -= 1;
      }
      end += 1;
    }
    result += `var(${found[1] ?? ""})`;
    index = end;
  }
  return result;
}

/** Every declaration whose value holds a colour with a hue, as "selector { property: value }" lines. */
export function hueDeclarations(css: string): { declaration: Declaration; hues: string[] }[] {
  return declarations(css)
    .map((declaration) => ({ declaration, hues: hueLiterals(declaration.value) }))
    .filter(({ hues }) => hues.length > 0);
}
