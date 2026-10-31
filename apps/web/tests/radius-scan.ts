// SPDX-License-Identifier: Apache-2.0
//
// Square-first geometry (design-direction.md 3.7, owner direction
// 2026-10-31): every corner the app ships is 0, 2, 4 or 6 px. A full radius
// (9999px, 50%) or any other value may appear only in a class of the
// explicit allow-list below, each entry with its reason. Shared by
// tests/radius-scan.test.ts (the stylesheets the build bundles, and the
// built CSS when there is one) and e2e/shapes.spec.ts (the built CSS and the
// corners the browser actually computes).
//
// How a value is read: every declaration of a radius property
// (border-radius, border-*-radius) and of a custom property whose name ends
// in "radius" is evaluated on its own. var() resolves against the
// page-level definitions (`:root` and the theme's scope; the theme layer
// wins over Astryx's base defaults, as it does on the page, where <html>
// carries data-astryx-theme); a component-local custom property with no
// page-level definition is checked where it is declared, so a reference to
// it resolves to its fallback, or to nothing. calc(), min() and max() are
// evaluated in px (1rem = 16px); what cannot be evaluated counts as
// unresolved and needs an allow-list entry like any off-scale value.

import { type Declaration, declarations } from "./colour-scan";

/** The radius scale, in px (design-direction 3.7: radius/0, /2, /4, /6). */
export const RADIUS_SCALE: readonly number[] = [0, 2, 4, 6];

const RADIUS_PROPERTY = /^border(-[a-z]+){0,2}-radius$/;
// The theme's scale (--radius-none ... --radius-full) and the component
// locals that carry a corner (--_card-radius, --_button-radius, ...).
const RADIUS_TOKEN = /^--(radius-[\w-]+|[\w-]*radius)$/;

/** Whether `property` sets a corner, or carries a value that may become one. */
export function isRadiusProperty(property: string): boolean {
  return RADIUS_PROPERTY.test(property) || RADIUS_TOKEN.test(property);
}

export type Length =
  | { kind: "px"; value: number }
  | { kind: "percent"; value: number }
  | { kind: "keyword"; value: string }
  | { kind: "unresolved"; value: string }
  /** A component-local custom property, checked where it is declared. */
  | { kind: "local"; value: string };

/** The page-level custom properties: name -> its values (one per definition). */
export type Tokens = ReadonlyMap<string, readonly string[]>;

const PAGE_SCOPE = /^(:root|:scope|html|\[data-astryx-theme[^\]]*\]|:where\(\[data-astryx-theme[^\]]*\]\)|\.xrw1vgx)(,\s*.+)?$/;

/**
 * The custom properties defined at page level in `css`. A property the
 * theme layer (`@layer astryx-theme`) defines keeps only the theme's values;
 * otherwise every page-level definition counts.
 */
export function pageTokens(css: string): Tokens {
  const themed = new Map<string, string[]>();
  const base = new Map<string, string[]>();
  for (const { property, value, selector, context } of declarations(css)) {
    if (!property.startsWith("--")) {
      continue;
    }
    const scopes = selector.split(",").map((part) => part.trim());
    if (!scopes.some((scope) => PAGE_SCOPE.test(scope))) {
      continue;
    }
    const inTheme = context.some((entry) => entry.replace(/\s+/g, " ") === "@layer astryx-theme");
    const target = inTheme ? themed : base;
    target.set(property, [...(target.get(property) ?? []), value]);
  }
  const tokens = new Map(base);
  for (const [property, values] of themed) {
    tokens.set(property, values);
  }
  return tokens;
}

// --- a small evaluator for radius values -------------------------------

type Token =
  | { type: "number"; value: number; unit: string }
  | { type: "ident"; value: string }
  | { type: "function"; name: string; args: string[] }
  | { type: "op"; value: string };

/** Splits `text` at top-level `separator` characters (commas, or whitespace). */
function splitTopLevel(text: string, separator: "," | " "): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const char of text) {
    if (char === "(") {
      depth += 1;
    } else if (char === ")") {
      depth -= 1;
    }
    const splits = depth === 0 && (separator === "," ? char === "," : /\s/.test(char));
    if (splits) {
      if (current.trim() !== "") {
        parts.push(current.trim());
      }
      current = "";
    } else {
      current += char;
    }
  }
  if (current.trim() !== "") {
    parts.push(current.trim());
  }
  return parts;
}

function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  let index = 0;
  while (index < text.length) {
    const rest = text.slice(index);
    const space = /^\s+/.exec(rest);
    if (space !== null) {
      index += space[0].length;
      continue;
    }
    const number = /^(-?(?:\d+\.?\d*|\.\d+))([a-z%]*)/i.exec(rest);
    // A sign is part of a number only where an operand is expected.
    const operandExpected = tokens.length === 0 || tokens.at(-1)?.type === "op";
    if (number !== null && (operandExpected || !rest.startsWith("-"))) {
      tokens.push({ type: "number", value: Number.parseFloat(number[1] ?? "0"), unit: (number[2] ?? "").toLowerCase() });
      index += number[0].length;
      continue;
    }
    const fn = /^(-?[a-z][\w-]*)\(/i.exec(rest);
    if (fn !== null) {
      let depth = 1;
      let end = fn[0].length;
      while (end < rest.length && depth > 0) {
        if (rest[end] === "(") {
          depth += 1;
        } else if (rest[end] === ")") {
          depth -= 1;
        }
        end += 1;
      }
      const name = (fn[1] ?? "").toLowerCase();
      tokens.push({ type: "function", name, args: splitTopLevel(rest.slice(fn[0].length, end - 1), ",") });
      index += end;
      continue;
    }
    // An identifier starts with a letter (an optional leading hyphen
    // aside), so a lone "-" between two operands stays an operator.
    const ident = /^-?[a-z][\w-]*/i.exec(rest);
    if (ident !== null) {
      tokens.push({ type: "ident", value: ident[0].toLowerCase() });
      index += ident[0].length;
      continue;
    }
    tokens.push({ type: "op", value: rest[0] ?? "" });
    index += 1;
  }
  return tokens;
}

const KEYWORDS = new Set(["inherit", "initial", "unset", "revert", "revert-layer"]);
const MAX_RESULTS = 64;

function unresolved(value: string): Length {
  return { kind: "unresolved", value };
}

function dedupe(lengths: Length[]): Length[] {
  const seen = new Map<string, Length>();
  for (const length of lengths) {
    seen.set(`${length.kind}:${length.value}`, length);
  }
  return [...seen.values()].slice(0, MAX_RESULTS);
}

function lengthOf(token: Extract<Token, { type: "number" }>): Length {
  if (token.unit === "px" || (token.unit === "" && token.value === 0)) {
    return { kind: "px", value: token.value };
  }
  if (token.unit === "rem" || token.unit === "em") {
    return { kind: "px", value: token.value * 16 };
  }
  if (token.unit === "%") {
    return { kind: "percent", value: token.value };
  }
  return unresolved(`${token.value}${token.unit}`);
}

/** Every value one radius expression can take, given the page-level tokens. */
export function evaluate(text: string, tokens: Tokens, seen: ReadonlySet<string> = new Set()): Length[] {
  const trimmed = text.trim();
  if (KEYWORDS.has(trimmed.toLowerCase())) {
    return [{ kind: "keyword", value: trimmed.toLowerCase() }];
  }
  return expression(tokenize(trimmed), tokens, seen, trimmed);
}

function expression(list: Token[], tokens: Tokens, seen: ReadonlySet<string>, source: string): Length[] {
  // Sums and differences of products and quotients, left to right.
  const terms: { op: string; tokens: Token[] }[] = [{ op: "+", tokens: [] }];
  for (const token of list) {
    if (token.type === "op" && (token.value === "+" || token.value === "-")) {
      terms.push({ op: token.value, tokens: [] });
    } else {
      terms.at(-1)?.tokens.push(token);
    }
  }
  let result: Length[] | null = null;
  for (const term of terms) {
    const value = product(term.tokens, tokens, seen, source);
    result = result === null ? value : combine(result, value, term.op, source);
  }
  return dedupe(result ?? [unresolved(source)]);
}

function product(list: Token[], tokens: Tokens, seen: ReadonlySet<string>, source: string): Length[] {
  let result: Length[] | null = null;
  let op = "*";
  for (const token of list) {
    if (token.type === "op") {
      op = token.value;
      continue;
    }
    const value = operand(token, tokens, seen);
    result = result === null ? value : combine(result, value, op, source);
  }
  return result ?? [unresolved(source)];
}

function combine(left: Length[], right: Length[], op: string, source: string): Length[] {
  // Anything but two px lengths (a component-local operand included) is
  // unresolved, except a px length scaled by a plain number.
  const out: Length[] = [];
  for (const a of left) {
    for (const b of right) {
      if (a.kind === "px" && b.kind === "px") {
        if (op === "+") {
          out.push({ kind: "px", value: a.value + b.value });
        } else if (op === "-") {
          out.push({ kind: "px", value: a.value - b.value });
        } else {
          out.push(unresolved(source));
        }
      } else if (a.kind === "px" && b.kind === "unresolved" && /^-?[\d.]+$/.test(b.value) && (op === "*" || op === "/")) {
        const factor = Number.parseFloat(b.value);
        out.push({ kind: "px", value: op === "*" ? a.value * factor : a.value / factor });
      } else {
        out.push(unresolved(source));
      }
    }
  }
  return dedupe(out);
}

function operand(token: Token, tokens: Tokens, seen: ReadonlySet<string>): Length[] {
  if (token.type === "number") {
    // A unitless factor stays "unresolved" with its number, for combine().
    return token.unit === "" && token.value !== 0 ? [unresolved(String(token.value))] : [lengthOf(token)];
  }
  if (token.type === "ident") {
    return KEYWORDS.has(token.value) ? [{ kind: "keyword", value: token.value }] : [unresolved(token.value)];
  }
  if (token.type === "op") {
    return [unresolved(token.value)];
  }
  const { name, args } = token;
  if (name === "var") {
    const [property = "", fallback] = args;
    const values = tokens.get(property.trim());
    if (values !== undefined && !seen.has(property)) {
      const next = new Set(seen).add(property);
      return dedupe(values.flatMap((value) => evaluate(value, tokens, next)));
    }
    // Component-local: its own values are checked where it is declared, so
    // on its own it passes; in arithmetic it makes the result unresolved.
    const local: Length = { kind: "local", value: property.trim() };
    return fallback === undefined ? [local] : dedupe([local, ...evaluate(fallback, tokens, seen)]);
  }
  if (name === "calc") {
    return evaluate(args.join(","), tokens, seen);
  }
  if (name === "max" || name === "min") {
    let result: Length[] = [{ kind: "px", value: name === "max" ? -Infinity : Infinity }];
    for (const arg of args) {
      const value = evaluate(arg, tokens, seen);
      const next: Length[] = [];
      for (const a of result) {
        for (const b of value) {
          if (a.kind === "px" && b.kind === "px") {
            next.push({ kind: "px", value: name === "max" ? Math.max(a.value, b.value) : Math.min(a.value, b.value) });
          } else {
            next.push(unresolved(`${name}(${args.join(", ")})`));
          }
        }
      }
      result = dedupe(next);
    }
    return result;
  }
  return [unresolved(`${name}(${args.join(", ")})`)];
}

/** Every value one radius declaration can take: each corner of a shorthand. */
export function radiusValues(value: string, tokens: Tokens): Length[] {
  const corners = splitTopLevel(value.replace("/", " "), " ");
  return dedupe(corners.flatMap((corner) => evaluate(corner, tokens)));
}

export type Corner = "scale" | "full" | "off-scale" | "unresolved";

/** How one value sits on the scale. A keyword (inherit) is checked where the parent sets it. */
export function classify(length: Length): Corner {
  if (length.kind === "keyword" || length.kind === "local") {
    return "scale";
  }
  if (length.kind === "px") {
    if (RADIUS_SCALE.includes(length.value)) {
      return "scale";
    }
    // A negative difference clamps to 0 in a radius; Astryx always wraps
    // one in max(0px, ...), so a bare negative is off scale.
    return length.value >= 999 ? "full" : "off-scale";
  }
  if (length.kind === "percent") {
    return length.value === 0 ? "scale" : length.value >= 50 ? "full" : "off-scale";
  }
  return "unresolved";
}

export type RadiusFinding = {
  declaration: Declaration;
  /** The off-scale values, as text ("9999px", "50%", "8px", "?calc(...)"). */
  values: string[];
  corner: Exclude<Corner, "scale">;
};

function show(length: Length): string {
  return length.kind === "px" ? `${length.value}px` : length.kind === "percent" ? `${length.value}%` : `?${length.value}`;
}

/** Every radius declaration of `css` and the off-scale values it can take. */
export function radiusFindings(css: string, tokens: Tokens): { scanned: number; findings: RadiusFinding[] } {
  let scanned = 0;
  const findings: RadiusFinding[] = [];
  for (const declaration of declarations(css)) {
    if (!isRadiusProperty(declaration.property)) {
      continue;
    }
    scanned += 1;
    const off = radiusValues(declaration.value, tokens).filter((length) => classify(length) !== "scale");
    if (off.length === 0) {
      continue;
    }
    const corners = off.map(classify);
    const corner = corners.includes("full") ? "full" : corners.includes("unresolved") ? "unresolved" : "off-scale";
    findings.push({ declaration, values: off.map(show), corner: corner as RadiusFinding["corner"] });
  }
  return { scanned, findings };
}

/** The class a vendor (Astryx) rule is keyed by: its first class selector. */
export function ruleClass(selector: string): string {
  return /\.([\w-]+)/.exec(selector)?.[1] ?? selector;
}

/**
 * The allow-list: Astryx classes whose radius is off the scale, keyed by
 * class name, with the component that uses them and why the app may ship
 * them. Astryx's CSS is one prebuilt stylesheet (styles.ts imports it
 * whole), so the rules of components the app never renders are in the
 * bundle. Nothing of the app itself is on this list: the app's own
 * stylesheets must be on the scale with no exception
 * (tests/radius-scan.test.ts), and e2e/shapes.spec.ts checks that no
 * rendered element outside RENDERED_EXCEPTIONS computes a corner off the
 * scale.
 */
export const VENDOR_ALLOW_LIST: Readonly<Record<string, string>> = {
  // Component names are where Astryx 0.7.4's dist/ references the class.
  xrw1vgx:
    "Astryx's :root defaults (element 8, container 12, page and chat 28, full 9999): the theme layer redeclares inner/element/container/page as 2/4/6/6 on every page; --radius-chat and --radius-full are used only as listed below",
  // --radius-full (9999px) and 50%.
  xjspbzw:
    "radius-full, an atomic class shared by Badge, the radio, checkbox and input-status indicators, Switch, Tab, Token, Slider, Skeleton, Calendar, Chat and others: rendered by the app only on Badge (tags, RENDERED_EXCEPTIONS) and the radio mark (globals.css draws it at radius 2); e2e/shapes.spec.ts checks every rendered corner",
  xq94oqs: "RadioListItem's press overlay (::after, full): globals.css draws it at radius 2 on every radio of the app",
  xeincmr: "Avatar (--_avatar-radius: full): not used by the app",
  x1rjw8qi: "Carousel (--_button-radius: full): not used by the app",
  xnpej1e: "ChatLayoutScrollButton (--radius-element: full): Chat is not used by the app",
  xy0xnkn: "ChatSendButton (var(--_button-radius, full)): Chat is not used by the app",
  x16rqkct: "50%: StatusDot, ListItem, NavIcon, Pagination, AspectRatio and the Table row status: none used by the app",
  x19415el: "Calendar (start-start corner, full): not used by the app",
  x8agd5o: "Calendar (start-end corner, full): not used by the app",
  x1wc3881: "Calendar (end-start corner, full): not used by the app",
  x1dgc8on: "Calendar (end-end corner, full): not used by the app",
  // Off the scale.
  xur7f20: "8px: Badge.test-violations.js, a test fixture the app never imports (tests/no-hue.test.ts checks)",
  x1nj7uno: "1px: TreeListBranches: TreeList is not used by the app",
  xashgol: "--radius-chat (28px): ChatMessageBubble, not used by the app",
  x1n70msy: "--radius-chat (28px, start-start): ChatComposerDrawer, not used by the app",
  xg0tfu: "--radius-chat (28px, start-end): ChatComposerDrawer, not used by the app",
  xx161y6: "--_chat-composer-radius (28px): ChatComposer, not used by the app",
  // Not resolvable here: a component-local radius minus a local padding.
  xcw7vlg: "ChatComposer's button radius (max of element and composer radius minus padding): not used by the app",
  x1ws5lxm: "DropdownMenu items (max(0, menu radius - padding)): DropdownMenu is not used by the app",
  xc910v0:
    "Astryx SegmentedControlItem (max(0, control radius - padding)): not used by the app; its theme control is its own (ThemeControl.tsx)",
};

/**
 * The theme's tokens that are off the scale. --radius-full stays 9999px
 * (design-direction 3.12 item 4: it is not set to 4 globally, which would
 * re-shape Astryx internals the app does not render); the app's stylesheets
 * never use it (tests/radius-scan.test.ts).
 */
export const THEME_TOKEN_ALLOW: Readonly<Record<string, string>> = {
  "--radius-full": "the theme keeps Astryx's full radius token; no app rule uses it",
};

function layerOf(declaration: Declaration): string | undefined {
  return declaration.context.find((entry) => entry.startsWith("@layer"))?.replace(/\s+/g, " ");
}

/** Whether an off-scale declaration is one the allow-lists exempt. */
export function allowedFinding(declaration: Declaration): boolean {
  const layer = layerOf(declaration);
  if (declaration.property in THEME_TOKEN_ALLOW && declaration.selector === ":scope") {
    return layer === "@layer astryx-theme";
  }
  const vendor = layer === "@layer astryx-base" || layer === "@layer reset";
  return vendor && ruleClass(declaration.selector) in VENDOR_ALLOW_LIST;
}

/**
 * Elements the browser may draw with a corner off the scale, by selector,
 * with the reason. Tags (Astryx Badge) belong to the tags lane of round 5,
 * which squares them on its own branch.
 */
export const RENDERED_EXCEPTIONS: Readonly<Record<string, string>> = {
  ".astryx-badge, .astryx-badge *": "tags: squared by the tags lane (round 5), not by this change",
};
