// SPDX-License-Identifier: Apache-2.0
//
// A JSON-LD data block (schema.org), written into the page's HTML.
//
// CSP: the policy is hash-only (script-src 'self' plus the page's inline
// script hashes), but a <script> whose type is not a JavaScript type is a
// data block: the browser never runs it, so script-src does not govern it
// and it needs no hash. scripts/csp-manifest.mjs counts it under
// `dataBlocks` and leaves it out of `scriptHashes` on purpose (a hash for it
// would only widen the header), and tests/csp-manifest.test.ts and
// e2e/seo.spec.ts pin both facts.

import { jsonLdText } from "@/lib/seo";

export function JsonLd({ data }: { data: Record<string, unknown> }) {
  return (
    <script
      type="application/ld+json"
      dangerouslySetInnerHTML={{ __html: jsonLdText(data) }}
    />
  );
}
