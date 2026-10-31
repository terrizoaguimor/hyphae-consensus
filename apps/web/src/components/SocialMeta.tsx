// SPDX-License-Identifier: Apache-2.0
//
// Open Graph and Twitter card tags, written out here rather than through
// Next's `openGraph` metadata: Next resolves a relative og:image against
// metadataBase and, with none set, against http://localhost:3000, which
// would ship a broken address. The public origin is not known at build time
// (the export is served by the API server wherever it is deployed), so the
// image stays a same-origin path.

import { brandFiles, OG_IMAGE_SIZE } from "@/lib/brand";

export function SocialMeta({
  siteName,
  title,
  description,
  imageAlt,
}: {
  siteName: string;
  title: string;
  description: string;
  imageAlt: string;
}) {
  return (
    <>
      <meta property="og:type" content="website" />
      <meta property="og:site_name" content={siteName} />
      <meta property="og:title" content={title} />
      <meta property="og:description" content={description} />
      <meta property="og:image" content={brandFiles.ogImage} />
      <meta property="og:image:type" content="image/png" />
      <meta property="og:image:width" content={String(OG_IMAGE_SIZE.width)} />
      <meta property="og:image:height" content={String(OG_IMAGE_SIZE.height)} />
      <meta property="og:image:alt" content={imageAlt} />
      <meta name="twitter:card" content="summary_large_image" />
    </>
  );
}
