// SPDX-License-Identifier: Apache-2.0
//
// "Related" links under a placeholder view: docs articles and legal pages
// that exist in this build (no link ever leads to a missing page).

import Link from "next/link";
import { getTranslations } from "next-intl/server";

import type { Locale } from "@/i18n/routing";
import {
  type DocSlug,
  docPath,
  footerNav,
  localePath,
} from "@/lib/site";

type Related =
  | { doc: DocSlug }
  | { page: (typeof footerNav)[number]["key"] };

type RelatedLinksProps = {
  locale: Locale;
  links: Related[];
};

export async function RelatedLinks({ locale, links }: RelatedLinksProps) {
  const t = await getTranslations({ locale });
  return (
    <section className="hc-section">
      <h2 id="related-heading">{t("NotConnected.related")}</h2>
      {/* role="list": Safari drops list semantics when list-style is none. */}
      <ul className="hc-related" role="list">
        {links.map((link) =>
          "doc" in link ? (
            <li key={link.doc}>
              <Link href={docPath(locale, link.doc)} className="hc-link">
                {t(`Docs.articles.${link.doc}.title`)}
              </Link>
            </li>
          ) : (
            <li key={link.page}>
              <Link href={localePath(locale, link.page)} className="hc-link">
                {t(`Nav.${link.page}`)}
              </Link>
            </li>
          ),
        )}
      </ul>
    </section>
  );
}
