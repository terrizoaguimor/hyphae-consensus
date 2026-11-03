"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Links to the current page in each locale. Each language is shown by its
// own name (marked with its lang); the title gives the name in the page
// language. Plain links, so switching works before hydration too. Following
// one stores the choice (localStorage only), which the root page (/) uses.

import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { Suspense, useEffect, useState } from "react";

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
// hydration pass produce plain paths. After every commit (a full load, a Next
// client navigation, a same-path query change, back/forward, a hash change)
// the query string and hash are re-read from window.location, so the links
// always match the URL on screen (the run page needs ?id=...). Next's
// pushState fires no popstate, hence useSearchParams (inside Suspense, whose
// fallback is the plain-path list) plus the pathname as effect triggers.
function useLocation(): { search: string; hash: string } {
  const pathname = usePathname();
  const searchKey = useSearchParams().toString();
  const [loc, setLoc] = useState({ search: "", hash: "" });
  useEffect(() => {
    const read = () =>
      setLoc((prev) =>
        prev.search === window.location.search &&
        prev.hash === window.location.hash
          ? prev
          : { search: window.location.search, hash: window.location.hash },
      );
    read();
    window.addEventListener("popstate", read);
    window.addEventListener("hashchange", read);
    return () => {
      window.removeEventListener("popstate", read);
      window.removeEventListener("hashchange", read);
    };
  }, [pathname, searchKey]);
  return loc;
}

export function LocaleSwitcher(props: LocaleSwitcherProps) {
  return (
    <Suspense fallback={<LocaleSwitcherView {...props} search="" hash="" />}>
      <LocaleSwitcherLive {...props} />
    </Suspense>
  );
}

function LocaleSwitcherLive(props: LocaleSwitcherProps) {
  const { search, hash } = useLocation();
  return <LocaleSwitcherView {...props} search={search} hash={hash} />;
}

function LocaleSwitcherView({
  current,
  endonyms,
  onNavigate,
  landmark = true,
  className = "hc-locale-list",
  search,
  hash,
}: LocaleSwitcherProps & { search: string; hash: string }) {
  const t = useTranslations("LocaleSwitcher");
  const pathname = usePathname();
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
