// SPDX-License-Identifier: Apache-2.0
//
// /robots.txt of the static export (built to out/robots.txt). Every crawler
// may read the pages; /api/ (the JSON API the server answers beside the
// export) and /research/ (the archive's raw files, served for the showcase)
// are not pages. The sitemap line is absolute, as the protocol requires.

import type { MetadataRoute } from "next";

import { PUBLIC_ORIGIN } from "@/lib/brand";

export const dynamic = "force-static";

export default function robots(): MetadataRoute.Robots {
  return {
    rules: [{ userAgent: "*", allow: "/", disallow: ["/api/", "/research/"] }],
    sitemap: `${PUBLIC_ORIGIN}/sitemap.xml`,
  };
}
