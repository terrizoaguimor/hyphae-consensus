// SPDX-License-Identifier: Apache-2.0
//
// The Quorum logo files the app ships (public/, built by
// scripts/web_brand_assets.py from design/logo/quorum-final) and the
// components that draw the mark:
//
// - every PNG has the size the metadata announces, and every pixel is a
//   true grey (R = G = B), the Open Graph image included;
// - every SVG is safe under the hash-only CSP: no style attribute, no
//   script, no external reference, greys only. The favicon is the one SVG
//   with a <style> element (its dark-browser ink), which is why it is only
//   ever linked, never inlined: no source file inlines a <style>;
// - the top bar's flat mark is the 32 px cut (ink box x 2-29, y 3-28), in
//   currentColor, decorative, without a style attribute.

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { QuorumGlass, QuorumMark } from "@/components/Quorum";
import { SocialMeta } from "@/components/SocialMeta";
import {
  brandFiles,
  brandIcons,
  brandViewport,
  OG_IMAGE_SIZE,
  ogImageUrl,
  parseSiteOrigin,
  SITE_ORIGIN,
} from "@/lib/brand";

import { hueLiterals } from "./colour-scan";
import { IO_BOUND_TEST_TIMEOUT_MS } from "./io-timeout";

const appDir = fileURLToPath(new URL("..", import.meta.url));
const publicDir = path.join(appDir, "public");

type Png = { width: number; height: number; pixels: Buffer };

/** Decodes an 8-bit RGBA, non-interlaced PNG (the only kind the set holds). */
function readPng(file: string): Png {
  const data = readFileSync(file);
  expect(data.subarray(0, 8).toString("hex")).toBe("89504e470d0a1a0a");
  let pos = 8;
  let width = 0;
  let height = 0;
  const idat: Buffer[] = [];
  while (pos < data.length) {
    const length = data.readUInt32BE(pos);
    const kind = data.toString("latin1", pos + 4, pos + 8);
    const body = data.subarray(pos + 8, pos + 8 + length);
    if (kind === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      expect([body[8], body[9], body[12]], `${file}: 8-bit RGBA, not interlaced`).toEqual([8, 6, 0]);
    } else if (kind === "IDAT") {
      idat.push(body);
    }
    pos += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const out = pixels.subarray(y * stride, (y + 1) * stride);
    const prev = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : Buffer.alloc(stride);
    for (let x = 0; x < stride; x += 1) {
      const a = x >= 4 ? out[x - 4]! : 0;
      const b = prev[x]!;
      const c = x >= 4 ? prev[x - 4]! : 0;
      let predictor = 0;
      if (filter === 1) predictor = a;
      else if (filter === 2) predictor = b;
      else if (filter === 3) predictor = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        predictor = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      } else expect(filter, `${file}: filter type`).toBe(0);
      out[x] = (line[x]! + predictor) & 255;
    }
  }
  return { width, height, pixels };
}

/** Pixels whose R, G and B differ, as "x,y". */
function huedPixels({ width, pixels }: Png): string[] {
  const hued: string[] = [];
  for (let i = 0; i < pixels.length; i += 4) {
    if (pixels[i] !== pixels[i + 1] || pixels[i] !== pixels[i + 2]) {
      hued.push(`${(i / 4) % width},${Math.floor(i / 4 / width)}`);
      if (hued.length > 5) break;
    }
  }
  return hued;
}

/** SHA-256 of the d attribute of design/logo/quorum-final/mark-flat-32-black.svg (v1.0). */
const FLAT_32_DESIGN_SHA256 = "a6f89cca43a242d50ff7e9a92d71d72aa2557422ec9d6e8f2cc6aaf645d3d7c2";

const publicFile = (url: string) => path.join(publicDir, url.replace(/^\//, ""));

describe("the brand PNGs", () => {
  const sizes: Record<string, [number, number]> = {
    [brandFiles.favicon16]: [16, 16],
    [brandFiles.favicon32]: [32, 32],
    [brandFiles.appleTouchIcon]: [180, 180],
    [brandFiles.icon512]: [512, 512],
    [brandFiles.icon1024]: [1024, 1024],
    [brandFiles.icon512Light]: [512, 512],
    [brandFiles.icon1024Light]: [1024, 1024],
    [brandFiles.ogImage]: [OG_IMAGE_SIZE.width, OG_IMAGE_SIZE.height],
  };

  it("are exactly the files public/ holds", () => {
    const shipped = readdirSync(publicDir, { recursive: true, encoding: "utf8" })
      .filter((file) => file.endsWith(".png"))
      .map((file) => `/${file.split(path.sep).join("/")}`)
      .sort();
    expect(shipped).toEqual(Object.keys(sizes).sort());
  });

  it.each(Object.entries(sizes))(
    "%s has its announced size and only grey pixels",
    (url, [width, height]) => {
      const png = readPng(publicFile(url));
      expect([png.width, png.height]).toEqual([width, height]);
      expect(huedPixels(png)).toEqual([]);
    },
    IO_BOUND_TEST_TIMEOUT_MS,
  );

  it("the Open Graph image is opaque and dark, with a light wordmark on it", () => {
    const png = readPng(publicFile(brandFiles.ogImage));
    let min = 255;
    let max = 0;
    let sum = 0;
    for (let i = 0; i < png.pixels.length; i += 4) {
      expect(png.pixels[i + 3]).toBe(255);
      min = Math.min(min, png.pixels[i]!);
      max = Math.max(max, png.pixels[i]!);
      sum += png.pixels[i]!;
    }
    // The dark stage (#060606 to #2B2B2B, a soft backlight), the wordmark in #F2F2F2.
    expect(min).toBeLessThanOrEqual(0x08);
    expect(max).toBeGreaterThanOrEqual(0xf0);
    expect(sum / (png.pixels.length / 4)).toBeLessThan(0x40);
  }, IO_BOUND_TEST_TIMEOUT_MS);

  it("the 16 and 32 px favicons are one ink, #141414, on transparency", () => {
    for (const url of [brandFiles.favicon16, brandFiles.favicon32]) {
      const { pixels } = readPng(publicFile(url));
      const inks = new Set<number>();
      for (let i = 0; i < pixels.length; i += 4) {
        if (pixels[i + 3]! > 0) inks.add(pixels[i]!);
      }
      expect([...inks], url).toEqual([0x14]);
    }
  });
});

describe("the brand SVGs", () => {
  const svgs = readdirSync(publicDir, { recursive: true, encoding: "utf8" })
    .filter((file) => file.endsWith(".svg"))
    .map((file) => `/${file.split(path.sep).join("/")}`)
    .sort();

  it("are the favicon and the two small glass marks", () => {
    expect(svgs).toEqual([brandFiles.glassDark, brandFiles.glassLight, brandFiles.faviconSvg].sort());
  });

  it.each(svgs)("%s has no style attribute, script or external reference, and greys only", (url) => {
    const svg = readFileSync(publicFile(url), "utf8");
    const markup = svg.replace(/<!--[\s\S]*?-->/g, "").replace(/<title>[\s\S]*?<\/title>/g, "");
    expect(markup).toMatch(/^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg"/);
    expect(markup).not.toMatch(/\sstyle\s*=/i);
    expect(markup).not.toMatch(/<script|<foreignObject|\son[a-z]+\s*=|javascript:/i);
    // The only URLs are the namespace and same-document references (url(#id)).
    const urls = [...markup.matchAll(/(?:href|src)\s*=\s*"([^"]*)"|url\(([^)]*)\)/gi)].map((m) => m[1] ?? m[2]);
    for (const url of urls) {
      expect(url).toMatch(/^#/);
    }
    const colours = [
      ...markup.matchAll(/\s(?:fill|stroke|stop-color|flood-color|lighting-color|color)\s*=\s*"([^"]*)"/g),
    ].map((m) => m[1]!);
    const styles = [...markup.matchAll(/<style>([\s\S]*?)<\/style>/g)].map((m) => m[1]!);
    expect(colours.length + styles.length).toBeGreaterThan(0);
    expect([...colours, ...styles].flatMap((value) => hueLiterals(value))).toEqual([]);
    if (url === brandFiles.faviconSvg) {
      // The favicon's one style element: the ink, and the ink in a dark browser.
      expect(styles).toEqual(["path{fill:#141414}@media (prefers-color-scheme:dark){path{fill:#EDEDED}}"]);
    } else {
      expect(markup).not.toMatch(/<style/i);
    }
  });

  it("no source file inlines a <style> element or the favicon's markup", () => {
    const sources = readdirSync(path.join(appDir, "src"), { recursive: true, encoding: "utf8" }).filter((file) =>
      /\.(ts|tsx)$/.test(file),
    );
    expect(sources.length).toBeGreaterThan(20);
    for (const file of sources) {
      // Code only: comments may name the element.
      const text = readFileSync(path.join(appDir, "src", file), "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .replace(/^\s*\/\/.*$/gm, "");
      expect(text, file).not.toMatch(/<style[\s>]/);
      expect(text, file).not.toContain("prefers-color-scheme:dark){path");
    }
  }, IO_BOUND_TEST_TIMEOUT_MS);
});

describe("the metadata", () => {
  it("links the favicon (SVG and the 16/32 PNGs) and the apple-touch icon", () => {
    expect(brandIcons).toEqual({
      icon: [
        { url: "/favicon-16.png", type: "image/png", sizes: "16x16" },
        { url: "/favicon-32.png", type: "image/png", sizes: "32x32" },
        { url: "/favicon.svg", type: "image/svg+xml", sizes: "any" },
      ],
      apple: [{ url: "/apple-touch-icon.png", type: "image/png", sizes: "180x180" }],
    });
  });

  it("sets theme-color in greys, light and dark, as the body background", () => {
    expect(brandViewport.themeColor).toEqual([
      { media: "(prefers-color-scheme: light)", color: "#F4F4F4" },
      { media: "(prefers-color-scheme: dark)", color: "#0B0B0B" },
    ]);
    const tokens = readFileSync(path.join(appDir, "src/app/hc-mono-tokens.css"), "utf8");
    expect(tokens).toContain("--hc-bg-body: light-dark(#F4F4F4, #0B0B0B);");
  });

  it("reads NEXT_PUBLIC_SITE_ORIGIN as a bare https origin, or none", () => {
    expect(parseSiteOrigin(undefined)).toBeUndefined();
    expect(parseSiteOrigin("")).toBeUndefined();
    expect(parseSiteOrigin("https://consensus.example")).toBe("https://consensus.example");
    expect(parseSiteOrigin("https://consensus.example/")).toBe("https://consensus.example");
    expect(parseSiteOrigin("https://consensus.example:8443")).toBe("https://consensus.example:8443");
    for (const bad of [
      "http://consensus.example",
      "consensus.example",
      "https://consensus.example/en/",
      "https://consensus.example/?a=1",
      "https://consensus.example/#top",
      "https://user:pass@consensus.example",
      "https://consensus.example//",
      " https://consensus.example",
      "javascript:alert(1)",
    ]) {
      expect(() => parseSiteOrigin(bad), bad).toThrow(/NEXT_PUBLIC_SITE_ORIGIN/);
    }
    // The test build sets none.
    expect(SITE_ORIGIN).toBeUndefined();
  });

  it("og:image is the same-origin path without an origin, and absolute with one", () => {
    expect(ogImageUrl(undefined)).toBe("/og-image.png");
    expect(ogImageUrl("https://consensus.example")).toBe("https://consensus.example/og-image.png");
    const props = { siteName: "S", title: "T", description: "D", imageAlt: "A" };
    const ogImage = (html: string) =>
      [...html.matchAll(/<meta property="og:image" content="([^"]*)"\/>/g)].map((m) => m[1]);
    const relative = renderToStaticMarkup(createElement(SocialMeta, props));
    expect(ogImage(relative)).toEqual(["/og-image.png"]);
    const absolute = renderToStaticMarkup(createElement(SocialMeta, { ...props, origin: "https://consensus.example" }));
    expect(ogImage(absolute)).toEqual(["https://consensus.example/og-image.png"]);
    for (const html of [relative, absolute]) {
      expect(html).toContain('<meta property="og:image:width" content="1200"/>');
      expect(html).toContain('<meta name="twitter:card" content="summary_large_image"/>');
      // No og:url: a shared layout cannot know the page's own address.
      expect(html).not.toContain("og:url");
      expect(html).not.toContain("localhost");
    }
  });
});

describe("the mark components", () => {
  it("the flat mark is the 32 px cut, in currentColor, decorative, with no style attribute", () => {
    const html = renderToStaticMarkup(createElement(QuorumMark));
    expect(html).toMatch(/^<svg class="hc-quorum-mark" viewBox="0 0 32 32" width="32" height="32" aria-hidden="true"/);
    expect(html).toContain('fill="currentColor"');
    expect(html).not.toMatch(/style=|<style|<title/);
    // Whole-pixel rectangles "Mx yhwv1h-wz"; their union is the cut's ink box.
    const d = /\sd="([^"]+)"/.exec(html)?.[1] ?? "";
    const rects = [...d.matchAll(/M(\d+) (\d+)h(\d+)v1h-(\d+)z/g)].map((m) => m.slice(1).map(Number));
    expect(rects.length).toBe(d.split("M").length - 1);
    for (const [, , w, back] of rects) expect(w).toBe(back);
    const xs = rects.flatMap(([x, , w]) => [x!, x! + w!]);
    const ys = rects.map(([, y]) => y!);
    expect([Math.min(...xs), Math.max(...xs) - 1, Math.min(...ys), Math.max(...ys)]).toEqual([2, 29, 3, 28]);
    // 3 px facets: no run narrower than 2 px (the node apex), none wider than 22.
    for (const [, , w] of rects) expect(w).toBeGreaterThanOrEqual(2);
    expect(Math.max(...rects.map(([, , w]) => w!))).toBe(22);
    expect(rects).toHaveLength(51);
    // The path verbatim from design/logo/quorum-final/mark-flat-32-black.svg:
    // the SHA-256 of that file's d attribute (the design folder is outside
    // the repository, so its digest is pinned here). Dropping, adding or
    // moving any one rectangle changes it.
    expect(createHash("sha256").update(d, "utf8").digest("hex")).toBe(FLAT_32_DESIGN_SHA256);
  });

  it("the glass mark is two decorative same-origin images, light and dark surface, at 96 px", () => {
    const html = renderToStaticMarkup(createElement(QuorumGlass));
    const images = [...html.matchAll(/<img ([^>]*)\/?>/g)].map((m) => m[1]!);
    expect(images).toHaveLength(2);
    expect(images[0]).toContain(`data-surface="light" src="${brandFiles.glassLight}" alt=""`);
    expect(images[1]).toContain(`data-surface="dark" src="${brandFiles.glassDark}" alt=""`);
    for (const image of images) {
      expect(image).toContain('width="96" height="96"');
      expect(image).not.toMatch(/style=/);
    }
  });
});
