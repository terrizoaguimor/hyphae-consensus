// SPDX-License-Identifier: Apache-2.0
//
// The e2e static server (scripts/serve-static.mjs): path resolution never
// leaves the served directory, page paths follow the export's trailing
// slash, and the CSP it sends is hash-only for scripts.

import { describe, expect, it } from "vitest";

import {
  contentSecurityPolicy,
  resolveRequest,
} from "../scripts/serve-static.mjs";
import { cspHash } from "../scripts/csp-manifest.mjs";

const files = new Set([
  "index.html",
  "404.html",
  "en/index.html",
  "en/docs/index.html",
  "en/docs/modes/index.html",
  "_next/static/chunks/app.js",
  "en/__next.$d$locale.__PAGE__.txt",
]);
const directories = new Set(["en", "en/docs", "en/docs/modes", "_next"]);

const checks = {
  isFile: async (rel: string) => files.has(rel),
  isDirectory: async (rel: string) => directories.has(rel),
};

const resolve = (urlPath: string) => resolveRequest(urlPath, checks);

describe("resolveRequest", () => {
  it("serves index.html for directory paths", async () => {
    expect(await resolve("/")).toEqual({ file: "index.html" });
    expect(await resolve("/en/")).toEqual({ file: "en/index.html" });
    expect(await resolve("/en/docs/modes/")).toEqual({
      file: "en/docs/modes/index.html",
    });
  });

  it("redirects a page path without its trailing slash", async () => {
    expect(await resolve("/en")).toEqual({ redirect: "/en/" });
    expect(await resolve("/en/docs")).toEqual({ redirect: "/en/docs/" });
  });

  it("serves files, including percent-encoded names", async () => {
    expect(await resolve("/_next/static/chunks/app.js")).toEqual({
      file: "_next/static/chunks/app.js",
    });
    expect(await resolve("/en/__next.%24d%24locale.__PAGE__.txt")).toEqual({
      file: "en/__next.$d$locale.__PAGE__.txt",
    });
  });

  it("refuses traversal, encoded separators and null bytes", async () => {
    expect(await resolve("/../etc/passwd")).toBeNull();
    expect(await resolve("/en/%2e%2e/%2e%2e/etc/passwd")).toBeNull();
    expect(await resolve("/en%5c..%5cindex.html")).toBeNull();
    expect(await resolve("/en/index.html%00")).toBeNull();
    expect(await resolve("/%E0%A4%A")).toBeNull();
  });

  it("answers null for missing pages, so the server sends 404.html", async () => {
    expect(await resolve("/de/")).toBeNull();
    expect(await resolve("/en/missing")).toBeNull();
    expect(await resolve("/_next")).toBeNull();
  });
});

describe("contentSecurityPolicy", () => {
  it("allows each page's inline scripts by hash only", () => {
    const hash = cspHash("self.__next_f.push([1])");
    const policy = contentSecurityPolicy({
      scriptHashes: [hash],
      styleHashes: [],
    });
    expect(policy).toContain(`script-src 'self' '${hash}'`);
    expect(policy).toContain("default-src 'none'");
    expect(policy).toContain("frame-ancestors 'none'");
    expect(policy).not.toMatch(/script-src[^;]*'unsafe-(inline|eval)'/);
  });

  it("falls back to 'self' only for a page missing from the manifest", () => {
    expect(contentSecurityPolicy(undefined)).toContain("script-src 'self';");
  });
});
