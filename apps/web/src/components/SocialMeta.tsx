// SPDX-License-Identifier: Apache-2.0
//
// Open Graph and Twitter card tags for the documents that have no per-page
// metadata: the language-neutral root page (/), whose layout writes them
// here. The localized pages build theirs, with og:url, og:locale, canonical
// and hreflang, in src/lib/seo.ts (pageMetadata) through Next's metadata API,
// which needs a metadataBase: they use PUBLIC_ORIGIN (src/lib/brand.ts), the
// build's NEXT_PUBLIC_SITE_ORIGIN or else the project's public site.
//
// Here og:image is absolute only when the build is given
// NEXT_PUBLIC_SITE_ORIGIN, as the Open Graph protocol and the link-preview
// crawlers want; without it the image stays a same-origin path. No og:url:
// the root page is not a page to index (it forwards visitors to a locale).

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
