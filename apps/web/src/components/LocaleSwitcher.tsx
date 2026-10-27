"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Links to the current page in each locale. Each language is shown by its
// own name (marked with its lang); the title gives the name in the page
// language. Plain links, so switching works before hydration too.

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useTranslations } from "next-intl";

import { switchLocalePath } from "@/i18n/paths";
import { type Locale, routing } from "@/i18n/routing";

type LocaleSwitcherProps = {
  current: Locale;
  endonyms: Record<Locale, string>;
};

export function LocaleSwitcher({ current, endonyms }: LocaleSwitcherProps) {
  const t = useTranslations("LocaleSwitcher");
  const pathname = usePathname();
  return (
    <nav aria-label={t("label")}>
      <ul className="hc-locale-list">
        {routing.locales.map((locale) => {
          const isCurrent = locale === current;
          return (
            <li key={locale}>
              <Link
                href={switchLocalePath(pathname, locale, routing.locales)}
                hrefLang={locale}
                aria-current={isCurrent ? "page" : undefined}
                title={isCurrent ? undefined : t(`names.${locale}`)}
              >
                <span lang={locale}>{endonyms[locale]}</span>
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
