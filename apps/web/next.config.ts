// SPDX-License-Identifier: Apache-2.0

import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";

const withNextIntl = createNextIntlPlugin("./src/i18n/request.ts");

const nextConfig: NextConfig = {
  // Static export to out/. In the plan, the Rust server (serve-public) serves
  // these files itself, with per-page CSP headers from out/csp-manifest.json.
  output: "export",
  // /en/ -> out/en/index.html
  trailingSlash: true,
  experimental: {
    // src/app/global-not-found.tsx renders out/404.html as a whole document
    // (lang, theme, language links). The app has two root layouts, so no
    // not-found.tsx could do that. Experimental in Next 16.4.
    globalNotFound: true,
  },
};

export default withNextIntl(nextConfig);
