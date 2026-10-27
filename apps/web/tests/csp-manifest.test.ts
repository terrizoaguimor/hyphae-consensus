// SPDX-License-Identifier: Apache-2.0

import { describe, expect, it } from "vitest";

import {
  analyzeHtml,
  crossOriginCssUrls,
  cspHash,
  pagePath,
} from "../scripts/csp-manifest.mjs";
import { THEME_SCRIPT } from "../src/components/themeScriptSource";

const page = (head: string, body = "") =>
  `<!DOCTYPE html><html><head>${head}</head><body>${body}</body></html>`;

describe("cspHash", () => {
  it("matches published sha256 source values", () => {
    // The example hash from the CSP specification, and the empty body.
    expect(cspHash("alert('Hello, world.');")).toBe(
      "sha256-qznLcsROx4GACP2dm0UCKCzCG+HiZ1guq6ZZDob/Tng=",
    );
    expect(cspHash("")).toBe(
      "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
    );
  });
});

describe("analyzeHtml", () => {
  it("hashes inline scripts and styles, and skips external scripts", () => {
    const result = analyzeHtml(
      page(
        `<script>${THEME_SCRIPT}</script><style>p{color:red}</style>`,
        '<script>self.__next_f.push([1,"x"])</script><script src="/_next/static/a.js"></script><p style="margin:0">t</p>',
      ),
    );
    expect(result.violations).toEqual([]);
    expect(result.inlineScripts).toEqual({
      total: 2,
      theme: 1,
      nextFlight: 1,
      other: 0,
      dataBlocks: 0,
    });
    expect(result.scriptHashes).toContain(cspHash(THEME_SCRIPT));
    expect(result.themeScriptHashes).toEqual([cspHash(THEME_SCRIPT)]);
    expect(result.styleHashes).toEqual([cspHash("p{color:red}")]);
    expect(result.externalScripts).toBe(1);
    expect(result.styleAttributes).toBe(1);
  });

  it("does not hash data blocks, which CSP does not govern", () => {
    const result = analyzeHtml(
      page('<script type="application/json">{"a":1}</script>'),
    );
    expect(result.scriptHashes).toEqual([]);
    expect(result.inlineScripts.dataBlocks).toBe(1);
  });

  it("rejects inline event handlers, including inside <template>", () => {
    const result = analyzeHtml(
      page(
        "",
        '<button onclick="go()">x</button><template><div onmouseover="x()"></div></template>',
      ),
    );
    expect(result.violations).toHaveLength(2);
    expect(result.violations[0]).toContain("onclick");
    expect(result.violations[1]).toContain("onmouseover");
  });

  it("rejects javascript: URLs however they are spelled", () => {
    const result = analyzeHtml(
      page(
        "",
        '<a href=" JaVaScRiPt:alert(1)">a</a><a href="java&#9;script:alert(1)">b</a><a href="/en/">c</a>',
      ),
    );
    expect(result.violations).toHaveLength(2);
    expect(result.violations.every((v: string) => v.includes("javascript:"))).toBe(
      true,
    );
  });

  it("rejects cross-origin subresources and allows same-origin and data: images", () => {
    const result = analyzeHtml(
      page(
        '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Figtree"><link rel="preload" href="/_next/static/media/a.woff2" as="font">',
        '<script src="//cdn.example/x.js"></script><img src="data:image/png;base64,AA" alt=""><img srcset="/a.png 1x, https://cdn.example/b.png 2x" alt=""><a href="https://example.org/">ok</a>',
      ),
    );
    expect(result.violations).toHaveLength(3);
    expect(result.violations.join("\n")).toContain("fonts.googleapis.com");
    expect(result.violations.join("\n")).toContain("//cdn.example/x.js");
    expect(result.violations.join("\n")).toContain("https://cdn.example/b.png");
  });
});

describe("crossOriginCssUrls", () => {
  it("finds absolute URLs in url() and @import only", () => {
    const css =
      '@import url("https://fonts.googleapis.com/css2?family=Figtree");@import "//cdn.example/a.css";' +
      ".a{background:url(data:image/svg+xml;utf8,x)}" +
      "@font-face{src:url(/_next/static/media/inter.woff2) format('woff2')}";
    expect(crossOriginCssUrls(css)).toEqual([
      "https://fonts.googleapis.com/css2?family=Figtree",
      "//cdn.example/a.css",
    ]);
  });
});

describe("pagePath", () => {
  it("maps exported files to the paths that serve them", () => {
    expect(pagePath("index.html")).toBe("/");
    expect(pagePath("en/index.html")).toBe("/en/");
    expect(pagePath("pt-BR/index.html")).toBe("/pt-BR/");
    expect(pagePath("404.html")).toBe("/404.html");
  });
});
