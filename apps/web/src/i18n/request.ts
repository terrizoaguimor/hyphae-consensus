// SPDX-License-Identifier: Apache-2.0
//
// next-intl request configuration. Pages call setRequestLocale() first, so
// this runs with the locale of the page being pre-rendered.

import { hasLocale } from "next-intl";
import { getRequestConfig } from "next-intl/server";

import { routing } from "./routing";

export default getRequestConfig(async ({ requestLocale }) => {
  const requested = await requestLocale;
  const locale = hasLocale(routing.locales, requested)
    ? requested
    : routing.defaultLocale;
  return {
    locale,
    messages: (await import(`../../messages/${locale}.json`)).default,
    // Fixed so the pre-rendered HTML and the browser format dates alike.
    timeZone: "UTC",
  };
});
