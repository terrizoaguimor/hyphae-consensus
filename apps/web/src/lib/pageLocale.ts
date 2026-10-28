// SPDX-License-Identifier: Apache-2.0
//
// The two things every localized page does first: validate the locale (an
// unknown one is a 404; with dynamicParams = false none is ever rendered)
// and register it for next-intl's static rendering. Plus the page <title>.

import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { hasLocale } from "next-intl";
import { getTranslations, setRequestLocale } from "next-intl/server";

import { type Locale, routing } from "@/i18n/routing";

import type { NavKey } from "./site";

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
 * Page title from the navigation label. The layout's title template adds the
 * app name to pages in child segments; Overview shares the layout's segment,
 * where a template does not apply, so it gets the full title directly.
 */
export async function navPageMetadata(
  params: Promise<{ locale: string }>,
  key: NavKey,
): Promise<Metadata> {
  const locale = await pageLocale(params);
  const t = await getTranslations({ locale });
  const label = t(`Nav.${key}`);
  return key === "overview"
    ? { title: { absolute: t("Metadata.titleTemplate").replace("%s", label) } }
    : { title: label };
}
