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
};

export default withNextIntl(nextConfig);
