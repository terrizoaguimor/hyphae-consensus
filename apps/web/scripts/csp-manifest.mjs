#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// CSP manifest for the static export (ABUSE-5). Runs after `next build`:
//
//   node scripts/csp-manifest.mjs [outDir]      (default: out)
//
// A static export cannot carry per-request nonces, so every inline <script>
// and <style> has to be allowed by its sha256 hash, page by page. This script
// parses every out/**/*.html with a spec-compliant HTML parser, hashes each
// inline script and style body exactly as a browser does (UTF-8, base64), and
// writes out/csp-manifest.json: page path -> hashes. serve-public turns those
// into each page's `script-src 'self' 'sha256-…'` header.
//
// It fails (exit 1) on what no hash-based policy can allow: inline event
// handler attributes, javascript: URLs, and subresources (scripts, styles,
// fonts, images, frames) loaded from another origin, including absolute URLs
// in the exported CSS. When Turnstile is added, its origin
// (https://challenges.cloudflare.com) must be allowed here explicitly, as in
// script-src and frame-src.

import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parse } from "parse5";

export const MANIFEST_NAME = "csp-manifest.json";

/** CSP source expression body for `text`: "sha256-<base64>". */
export function cspHash(text) {
  return `sha256-${createHash("sha256").update(text, "utf8").digest("base64")}`;
}

// <script type> values that make the browser run the body (and so fall under
// script-src). Anything else is a data block, which CSP does not govern.
const EXECUTABLE_SCRIPT_TYPES = new Set([
  "",
  "module",
  "importmap",
  "speculationrules",
  "text/javascript",
  "application/javascript",
  "application/ecmascript",
  "text/ecmascript",
]);

// Elements and attributes that load a subresource; each must be same-origin.
const SUBRESOURCE_ATTRIBUTES = {
  script: ["src"],
  img: ["src", "srcset"],
  source: ["src", "srcset"],
  iframe: ["src"],
  embed: ["src"],
  object: ["data"],
  video: ["src", "poster"],
  audio: ["src"],
  track: ["src"],
  input: ["src"],
};

const SUBRESOURCE_LINK_RELS = new Set([
  "stylesheet",
  "preload",
  "modulepreload",
  "prefetch",
  "icon",
  "apple-touch-icon",
  "manifest",
]);

function attr(element, name) {
  return element.attrs.find((a) => a.name === name)?.value;
}

function textContent(element) {
  return element.childNodes
    .filter((child) => child.nodeName === "#text")
    .map((child) => child.value)
    .join("");
}

/** The URL string as a URL parser sees it (WHATWG: strip C0/space, drop tab/LF/CR). */
function normalizeUrl(value) {
  return value
    .replace(/^[\u0000- ]+|[\u0000- ]+$/g, "")
    .replace(/[\t\n\r]/g, "");
}

function isJavascriptUrl(value) {
  return /^javascript:/i.test(normalizeUrl(value));
}

/** True for absolute (scheme or protocol-relative) URLs, except data:. */
function isCrossOrigin(url) {
  const value = normalizeUrl(url);
  if (/^data:/i.test(value)) {
    return false;
  }
  return /^[a-z][a-z0-9+.-]*:/i.test(value) || value.startsWith("//");
}

function srcsetUrls(value) {
  return value
    .split(",")
    .map((candidate) => candidate.trim().split(/\s+/)[0])
    .filter(Boolean);
}

function classifyScript(body) {
  if (body.includes('"hc-theme"')) {
    return "theme";
  }
  if (/^\(?self\.__next_f\b/.test(body.trimStart())) {
    return "nextFlight";
  }
  return "other";
}

function* elements(node) {
  for (const child of node.childNodes ?? []) {
    if (child.tagName !== undefined) {
      yield child;
      // <template> keeps its children in a separate document fragment.
      yield* elements(child.tagName === "template" ? child.content : child);
    }
  }
}

function describe(element) {
  return `<${element.tagName}>`;
}

/**
 * Inline script and style facts for one HTML document, plus every violation
 * of a hash-only policy found in it.
 */
export function analyzeHtml(html) {
  const document = parse(html);
  const scriptHashes = new Set();
  const themeScriptHashes = new Set();
  const styleHashes = new Set();
  const inlineScripts = {
    total: 0,
    theme: 0,
    nextFlight: 0,
    other: 0,
    dataBlocks: 0,
  };
  let inlineStyles = 0;
  let styleAttributes = 0;
  let externalScripts = 0;
  const violations = [];

  for (const element of elements(document)) {
    for (const { name, value } of element.attrs) {
      if (/^on/i.test(name)) {
        violations.push(
          `inline event handler ${name}= on ${describe(element)}`,
        );
      }
      if (isJavascriptUrl(value)) {
        violations.push(`javascript: URL in ${name}= on ${describe(element)}`);
      }
      if (name === "style") {
        styleAttributes += 1;
      }
    }

    const urlAttributes = [...(SUBRESOURCE_ATTRIBUTES[element.tagName] ?? [])];
    if (element.tagName === "link") {
      const rels = (attr(element, "rel") ?? "").toLowerCase().split(/\s+/);
      if (rels.some((rel) => SUBRESOURCE_LINK_RELS.has(rel))) {
        urlAttributes.push("href");
      }
    }
    for (const name of urlAttributes) {
      const value = attr(element, name);
      if (value === undefined) {
        continue;
      }
      const urls = name === "srcset" ? srcsetUrls(value) : [value];
      for (const url of urls.filter(isCrossOrigin)) {
        violations.push(
          `cross-origin subresource ${url} in ${name}= on ${describe(element)}`,
        );
      }
    }

    if (element.tagName === "script") {
      if (attr(element, "src") !== undefined) {
        externalScripts += 1;
        continue;
      }
      const type = (attr(element, "type") ?? "").trim().toLowerCase();
      if (!EXECUTABLE_SCRIPT_TYPES.has(type)) {
        inlineScripts.dataBlocks += 1;
        continue;
      }
      const body = textContent(element);
      const kind = classifyScript(body);
      inlineScripts.total += 1;
      inlineScripts[kind] += 1;
      scriptHashes.add(cspHash(body));
      if (kind === "theme") {
        themeScriptHashes.add(cspHash(body));
      }
    } else if (element.tagName === "style") {
      inlineStyles += 1;
      styleHashes.add(cspHash(textContent(element)));
    }
  }

  return {
    scriptHashes: [...scriptHashes].sort(),
    themeScriptHashes: [...themeScriptHashes].sort(),
    styleHashes: [...styleHashes].sort(),
    inlineScripts,
    inlineStyles,
    styleAttributes,
    externalScripts,
    violations,
  };
}

/** Absolute network URLs in a stylesheet (url() and @import). */
export function crossOriginCssUrls(css) {
  const found = [];
  for (const match of css.matchAll(/url\(\s*(['"]?)([^'")]*)\1\s*\)/gi)) {
    found.push(match[2]);
  }
  for (const match of css.matchAll(/@import\s+(['"])([^'"]*)\1/gi)) {
    found.push(match[2]);
  }
  return found.filter(isCrossOrigin);
}

/** URL path that serves an exported file: en/index.html -> /en/. */
export function pagePath(relativeFile) {
  const posix = relativeFile.split(path.sep).join("/");
  if (posix === "index.html") {
    return "/";
  }
  if (posix.endsWith("/index.html")) {
    return `/${posix.slice(0, -"index.html".length)}`;
  }
  return `/${posix}`;
}

async function listFiles(dir) {
  const entries = await readdir(dir, { recursive: true, withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile())
    .map((entry) => path.join(entry.parentPath, entry.name));
}

export async function buildManifest(outDir) {
  const files = await listFiles(outDir);
  const htmlFiles = files.filter((file) => file.endsWith(".html")).sort();
  const cssFiles = files.filter((file) => file.endsWith(".css")).sort();
  const pages = {};
  const violations = [];
  const themeHashes = new Set();

  for (const file of htmlFiles) {
    const relative = path.relative(outDir, file);
    const html = await readFile(file, "utf8");
    const result = analyzeHtml(html);
    for (const violation of result.violations) {
      violations.push(`${relative}: ${violation}`);
    }
    for (const hash of result.themeScriptHashes) {
      themeHashes.add(hash);
    }
    pages[pagePath(relative)] = {
      file: relative.split(path.sep).join("/"),
      scriptHashes: result.scriptHashes,
      styleHashes: result.styleHashes,
      inlineScripts: result.inlineScripts,
      inlineStyles: result.inlineStyles,
      styleAttributes: result.styleAttributes,
      externalScripts: result.externalScripts,
    };
  }

  for (const file of cssFiles) {
    const relative = path.relative(outDir, file);
    for (const url of crossOriginCssUrls(await readFile(file, "utf8"))) {
      violations.push(`${relative}: cross-origin URL ${url} in CSS`);
    }
  }

  const sortedPages = Object.fromEntries(
    Object.keys(pages)
      .sort()
      .map((key) => [key, pages[key]]),
  );
  return {
    manifest: {
      version: 1,
      generator: "apps/web/scripts/csp-manifest.mjs",
      algorithm: "sha256",
      note: "Header form: 'sha256-<base64>'. Style attributes need style-src-attr 'unsafe-inline' if any page has them.",
      themeScriptHashes: [...themeHashes].sort(),
      pages: sortedPages,
    },
    htmlCount: htmlFiles.length,
    violations,
  };
}

async function main() {
  const outDir = path.resolve(process.argv[2] ?? "out");
  const { manifest, htmlCount, violations } = await buildManifest(outDir);
  if (htmlCount === 0) {
    console.error(`csp-manifest: no .html files under ${outDir}`);
    process.exit(2);
  }
  if (violations.length > 0) {
    console.error("csp-manifest: the export breaks a hash-only CSP:");
    for (const violation of violations) {
      console.error(`  - ${violation}`);
    }
    process.exit(1);
  }
  await writeFile(
    path.join(outDir, MANIFEST_NAME),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
  console.log(
    `csp-manifest: ${htmlCount} pages -> ${path.join(path.basename(outDir), MANIFEST_NAME)}`,
  );
  console.log("page | inline scripts (theme/flight/other) | inline styles | style attrs");
  for (const [page, facts] of Object.entries(manifest.pages)) {
    const s = facts.inlineScripts;
    console.log(
      `${page} | ${s.total} (${s.theme}/${s.nextFlight}/${s.other}) | ${facts.inlineStyles} | ${facts.styleAttributes}`,
    );
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await main();
}
