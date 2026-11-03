// SPDX-License-Identifier: Apache-2.0
//
// /sitemap.xml of the static export (built to out/sitemap.xml): every
// indexable page in each of the four locales, each entry with the
// xhtml:link hreflang alternates of its translations and x-default (the
// English page). Not listed: /run/ (an empty shell until a ?id= query fills
// it), /settings/ and the 404 page. src/lib/seo.ts holds the list;
// e2e/seo.spec.ts compares it with the pages the export actually contains.

import type { MetadataRoute } from "next";

import { PUBLIC_ORIGIN } from "@/lib/brand";
import { sitemapEntries } from "@/lib/seo";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  return sitemapEntries(PUBLIC_ORIGIN);
}
