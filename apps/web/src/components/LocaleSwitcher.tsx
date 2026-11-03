"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Links to the current page in each locale. Each language is shown by its
// own name (marked with its lang); the title gives the name in the page
// language. Plain links, so switching works before hydration too. Following
// one stores the choice (localStorage only), which the root page (/) uses.

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useTranslations } from "next-intl";
import { useSyncExternalStore } from "react";

import { switchLocalePath, withLocation } from "@/i18n/paths";
import { type Locale, routing } from "@/i18n/routing";
import { STORAGE_KEYS, writeStorage } from "@/lib/preferences";

type LocaleSwitcherProps = {
  current: Locale;
  endonyms: Record<Locale, string>;
  /** Called after a language link is followed. */
  onNavigate?: (locale: Locale) => void;
  /**
   * A "Language" navigation landmark (header, drawer). Settings renders the
   * bare list inside its own labelled section instead, so the page never has
   * two landmarks with the same name.
   */
  landmark?: boolean;
  /** Class of the list; Settings uses a roomier layout. */
  className?: string;
};

// The page is prerendered without its query, so the server render and the
// hydration pass produce plain paths; right after hydration the links gain the
// current query string and hash (the run page needs ?id=...). No Suspense
// needed, unlike useSearchParams.
const SEPARATOR = "\u0000";
function subscribeLocation(onChange: () => void): () => void {
  window.addEventListener("popstate", onChange);
  window.addEventListener("hashchange", onChange);
  return () => {
    window.removeEventListener("popstate", onChange);
    window.removeEventListener("hashchange", onChange);
  };
}
const getLocation = () =>
  window.location.search + SEPARATOR + window.location.hash;
const getServerLocation = () => SEPARATOR;

export function LocaleSwitcher({
  current,
  endonyms,
  onNavigate,
  landmark = true,
  className = "hc-locale-list",
}: LocaleSwitcherProps) {
  const t = useTranslations("LocaleSwitcher");
  const pathname = usePathname();
  const [search = "", hash = ""] = useSyncExternalStore(
    subscribeLocation,
    getLocation,
    getServerLocation,
  ).split(SEPARATOR);
  // role="list": Safari drops list semantics when list-style is none.
  const list = (
    <ul className={className} role="list">
      {routing.locales.map((locale) => {
        const isCurrent = locale === current;
        return (
          <li key={locale}>
            <Link
              href={withLocation(
                switchLocalePath(pathname, locale, routing.locales),
                search,
                hash,
              )}
              hrefLang={locale}
              aria-current={isCurrent ? "page" : undefined}
              title={isCurrent ? undefined : t(`names.${locale}`)}
              onClick={() => {
                writeStorage(STORAGE_KEYS.locale, locale);
                onNavigate?.(locale);
              }}
            >
              <span lang={locale}>{endonyms[locale]}</span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
  return landmark ? <nav aria-label={t("label")}>{list}</nav> : list;
}
