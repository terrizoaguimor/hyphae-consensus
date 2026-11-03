// SPDX-License-Identifier: Apache-2.0
//
// /manifest.webmanifest of the static export: the app's name, start page and
// the 512 and 1024 px icons (public/, the dark tile, greys only), in the
// monochrome palette of the page (--hc-bg-body in dark: #0B0B0B). The site
// is a website, not an installable app: display stays "browser". The API
// server's CSP carries `manifest-src 'self'` for this file.

import type { MetadataRoute } from "next";

import { defaultMessages } from "@/i18n/endonyms";
import { brandFiles } from "@/lib/brand";
import { localePath } from "@/lib/site";

export const dynamic = "force-static";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: defaultMessages.App.name,
    short_name: defaultMessages.App.name,
    description: defaultMessages.App.tagline,
    lang: "en",
    start_url: localePath("en", ""),
    scope: "/",
    display: "browser",
    background_color: "#0B0B0B",
    theme_color: "#0B0B0B",
    icons: [
      { src: brandFiles.icon512, sizes: "512x512", type: "image/png" },
      { src: brandFiles.icon1024, sizes: "1024x1024", type: "image/png" },
    ],
  };
}
