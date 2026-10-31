// SPDX-License-Identifier: Apache-2.0
//
// Open Graph and Twitter card tags, written out here rather than through
// Next's `openGraph` metadata: Next resolves a relative og:image against
// metadataBase and, with none set, against http://localhost:3000, which
// would ship a broken address. The export is served by the API server
// wherever it is deployed, so the origin is known only when the build is
// given NEXT_PUBLIC_SITE_ORIGIN (src/lib/brand.ts): then og:image is
// absolute, as the Open Graph protocol and the link-preview crawlers want;
// without it the image stays a same-origin path.
//
// No og:url: these tags are written by layouts shared by every page under
// them, and a static layout has no page path, so an og:url here would name
// one page (the locale's home) as the address of all of them. Crawlers use
// the address they fetched when og:url is absent.

import { OG_IMAGE_SIZE, ogImageUrl, SITE_ORIGIN } from "@/lib/brand";

export function SocialMeta({
  siteName,
  title,
  description,
  imageAlt,
  origin = SITE_ORIGIN,
}: {
  siteName: string;
  title: string;
  description: string;
  imageAlt: string;
  /** The deployment's origin; defaults to the build's NEXT_PUBLIC_SITE_ORIGIN. */
  origin?: string | undefined;
}) {
  return (
    <>
      <meta property="og:type" content="website" />
      <meta property="og:site_name" content={siteName} />
      <meta property="og:title" content={title} />
      <meta property="og:description" content={description} />
      <meta property="og:image" content={ogImageUrl(origin)} />
      <meta property="og:image:type" content="image/png" />
      <meta property="og:image:width" content={String(OG_IMAGE_SIZE.width)} />
      <meta property="og:image:height" content={String(OG_IMAGE_SIZE.height)} />
      <meta property="og:image:alt" content={imageAlt} />
      <meta name="twitter:card" content="summary_large_image" />
    </>
  );
}
