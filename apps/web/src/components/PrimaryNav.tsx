"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The six primary destinations. The current page is marked with
// aria-current="page" (a docs article marks Docs with aria-current="true"),
// and by weight and an indicator bar, not by color alone.

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useTranslations } from "next-intl";

import type { Locale } from "@/i18n/routing";
import { localePath, navMatch, primaryNav } from "@/lib/site";

type PrimaryNavProps = {
  locale: Locale;
  /** Called when a link is followed (the mobile drawer closes itself). */
  onNavigate?: () => void;
};

export function PrimaryNav({ locale, onNavigate }: PrimaryNavProps) {
  const t = useTranslations("Nav");
  const pathname = usePathname();
  return (
    // role="list": Safari drops list semantics when list-style is none.
    <ul className="hc-nav-list" role="list">
      {primaryNav.map(({ key, path }) => {
        const match = navMatch(pathname, locale, path);
        return (
          <li key={key}>
            <Link
              href={localePath(locale, path)}
              className="hc-nav-link"
              aria-current={
                match === "page" ? "page" : match === "section" ? "true" : undefined
              }
              onClick={onNavigate}
            >
              {t(key)}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
