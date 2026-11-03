// SPDX-License-Identifier: Apache-2.0
//
// The two things every localized page does first: validate the locale (an
// unknown one is a 404; with dynamicParams = false none is ever rendered)
// and register it for next-intl's static rendering. Plus the page <title>.

import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { hasLocale } from "next-intl";
import { getTranslations, setRequestLocale } from "next-intl/server";

import { dynamicTranslator } from "@/i18n/dynamic";
import { type Locale, routing } from "@/i18n/routing";

import { noindexPaths, pageMetadata } from "./seo";
import { footerNav, type NavKey, primaryNav } from "./site";

export type LocaleParams = { params: Promise<{ locale: string }> };

export async function pageLocale(
  params: Promise<{ locale: string }>,
): Promise<Locale> {
  const { locale } = await params;
  if (!hasLocale(routing.locales, locale)) {
    notFound();
  }
  setRequestLocale(locale);
  return locale;
}

/**
 * The head of a localized page: title, its own description, canonical and
 * hreflang alternates, Open Graph and Twitter tags (src/lib/seo.ts).
 * `descriptionKey` is a key of the Metadata catalog (pageDescriptions.<page>
 * or docDescriptions.<slug>). The title is `title` with the layout's
 * template applied ("<title> · Hyphae Consensus"), or `absoluteTitle` as is.
 */
export async function localizedPageMetadata(
  params: Promise<{ locale: string }>,
  page: {
    path: string;
    descriptionKey: string;
    title?: string;
    absoluteTitle?: string;
    noindex?: boolean;
  },
): Promise<Metadata> {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  const title =
    page.absoluteTitle ??
    t("Metadata.titleTemplate").replace("%s", page.title ?? "");
  return pageMetadata({
    locale,
    path: page.path,
    title,
    description: dynamicTranslator(t)(`Metadata.${page.descriptionKey}`),
    siteName: t("App.name"),
    imageAlt: t("Metadata.ogImageAlt"),
    ...(page.noindex === undefined ? {} : { noindex: page.noindex }),
  });
}

/**
 * Metadata of a page in the navigation (primary or footer). The overview is
 * the site's home: its title is the full "Hyphae Consensus - ..." line
 * (Metadata.homeTitle), not "Overview · Hyphae Consensus"; Settings is kept
 * out of search results.
 */
export async function navPageMetadata(
  params: Promise<{ locale: string }>,
  key: NavKey,
): Promise<Metadata> {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  const path = [...primaryNav, ...footerNav].find((entry) => entry.key === key)?.path ?? "";
  if (key === "overview") {
    return localizedPageMetadata(params, {
      path,
      descriptionKey: "description",
      absoluteTitle: t("Metadata.homeTitle"),
    });
  }
  return localizedPageMetadata(params, {
    path,
    descriptionKey: `pageDescriptions.${key}`,
    title: t(`Nav.${key}`),
    noindex: (noindexPaths as readonly string[]).includes(path),
  });
}
