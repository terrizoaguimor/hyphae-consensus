// SPDX-License-Identifier: Apache-2.0
//
// Renderers for the tags catalog messages may use with t.rich()
// (src/i18n/richTags.ts). Links point to fixed destinations only: internal
// pages through localePath(), external pages from the externalLinks list.

import Link from "next/link";
import type { ReactNode } from "react";

import type { RichTag } from "@/i18n/richTags";
import type { Locale } from "@/i18n/routing";
import { externalLinks, localePath } from "@/lib/site";

type TagRenderer = (chunks: ReactNode) => ReactNode;

function internalLink(href: string, chunks: ReactNode): ReactNode {
  return (
    <Link href={href} className="hc-link">
      {chunks}
    </Link>
  );
}

function externalLink(href: string, chunks: ReactNode): ReactNode {
  return (
    <a href={href} className="hc-link">
      {chunks}
    </a>
  );
}

export function richTags(locale: Locale): Record<RichTag, TagRenderer> {
  return {
    strong: (chunks) => <strong>{chunks}</strong>,
    em: (chunks) => <em>{chunks}</em>,
    code: (chunks) => <code className="hc-code">{chunks}</code>,
    // An English term kept in a translation (a label, an outcome name).
    en: (chunks) => <span lang="en">{chunks}</span>,
    aup: (chunks) => externalLink(externalLinks.acceptableUsePolicy, chunks),
    geo: (chunks) => externalLink(externalLinks.geographicUsePolicy, chunks),
    hyphae: (chunks) => externalLink(externalLinks.hyphae, chunks),
    cloudflare: (chunks) =>
      externalLink(externalLinks.cloudflarePrivacy, chunks),
    privacy: (chunks) => internalLink(localePath(locale, "privacy"), chunks),
    terms: (chunks) => internalLink(localePath(locale, "terms"), chunks),
    limitations: (chunks) =>
      internalLink(localePath(locale, "limitations"), chunks),
    settings: (chunks) => internalLink(localePath(locale, "settings"), chunks),
  };
}
