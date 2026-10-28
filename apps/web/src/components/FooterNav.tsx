"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Footer links to the legal and project pages; the current one is marked
// with aria-current="page".

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useTranslations } from "next-intl";

import type { Locale } from "@/i18n/routing";
import { footerNav, localePath, navMatch } from "@/lib/site";

export function FooterNav({ locale }: { locale: Locale }) {
  const t = useTranslations();
  const pathname = usePathname();
  return (
    <nav aria-label={t("Shell.footerNavLabel")}>
      {/* role="list": Safari drops list semantics when list-style is none. */}
      <ul className="hc-footer-links" role="list">
        {footerNav.map(({ key, path }) => (
          <li key={key}>
            <Link
              href={localePath(locale, path)}
              aria-current={
                navMatch(pathname, locale, path) === "page" ? "page" : undefined
              }
            >
              {t(`Nav.${key}`)}
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}
