// SPDX-License-Identifier: Apache-2.0
//
// End-to-end tests against the static export, served with per-page CSP by
// scripts/serve-static.mjs. Run them only through
// scripts/playwright-docker.sh, after `pnpm build`: the container holds the
// pinned browsers, sees apps/web and nothing else, and has no network.

import { defineConfig, devices } from "@playwright/test";

const PORT = 4173;

export default defineConfig({
  testDir: "e2e",
  outputDir: "test-results/e2e",
  globalSetup: "./e2e/global-setup.ts",
  globalTeardown: "./e2e/global-teardown.ts",
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  workers: process.env.PW_WORKERS === undefined ? 4 : Number(process.env.PW_WORKERS),
  timeout: 120_000,
  expect: { timeout: 10_000 },
  reporter: [
    ["list"],
    ["json", { outputFile: "test-results/e2e-report.json" }],
  ],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: `node scripts/serve-static.mjs --dir out --port ${PORT} --csp`,
    url: `http://127.0.0.1:${PORT}/en/`,
    reuseExistingServer: false,
    timeout: 60_000,
    stdout: "pipe",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      grepInvert: /@mobile/,
    },
    {
      name: "chromium-mobile",
      use: { ...devices["Pixel 7"] },
      grep: /@mobile/,
    },
    {
      name: "firefox",
      use: { ...devices["Desktop Firefox"] },
      grep: /@smoke/,
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
      grep: /@smoke/,
    },
  ],
});
