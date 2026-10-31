// SPDX-License-Identifier: Apache-2.0
//
// The Quorum logo's files and the page metadata that points at them, shared
// by the three documents of the export (the [locale] layout, the root page's
// layout and the 404 page). The files live in public/ and are built by
// scripts/web_brand_assets.py from design/logo/quorum-final (its README:
// below 64 px the mark is always flat, so the favicon is the 16 px cut and
// the top bar carries the 32 px cut; the brushed-glass mark appears only at
// 64 px or more).
//
// No web app manifest: the app has none, and the API server's CSP sets no
// manifest-src (default-src 'none' would block it).

import type { Metadata, Viewport } from "next";

/** Paths of the brand files, as served by the static export. */
export const brandFiles = {
  /** The 16 px cut; its own style element sets ink #141414, or #EDEDED in a dark browser. Linked only, never inlined. */
  faviconSvg: "/favicon.svg",
  favicon16: "/favicon-16.png",
  favicon32: "/favicon-32.png",
  appleTouchIcon: "/apple-touch-icon.png",
  /** App icons, dark tile (radius 0; the platform applies its own mask), plus the light tile. */
  icon512: "/icon-512.png",
  icon1024: "/icon-1024.png",
  icon512Light: "/icon-512-light.png",
  icon1024Light: "/icon-1024-light.png",
  /** The small glass mark (64 to 192 px), one per surface. */
  glassDark: "/brand/quorum-glass-64-dark.svg",
  glassLight: "/brand/quorum-glass-64-light.svg",
  /** Open Graph image, 1200 x 630, greys only. */
  ogImage: "/og-image.png",
} as const;

export const OG_IMAGE_SIZE = { width: 1200, height: 630 } as const;

/** <link rel="icon"> and <link rel="apple-touch-icon">, same-origin paths. */
export const brandIcons: NonNullable<Metadata["icons"]> = {
  icon: [
    { url: brandFiles.favicon16, type: "image/png", sizes: "16x16" },
    { url: brandFiles.favicon32, type: "image/png", sizes: "32x32" },
    { url: brandFiles.faviconSvg, type: "image/svg+xml", sizes: "any" },
  ],
  apple: [{ url: brandFiles.appleTouchIcon, type: "image/png", sizes: "180x180" }],
};

/**
 * The browser chrome follows the page background (--hc-bg-body: mono/1 in
 * light, mono/13 in dark), in greys.
 */
export const brandViewport: Viewport = {
  colorScheme: "light dark",
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#F4F4F4" },
    { media: "(prefers-color-scheme: dark)", color: "#0B0B0B" },
  ],
};
