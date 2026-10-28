// SPDX-License-Identifier: Apache-2.0
//
// Clears the axe results of an earlier run, so the summary written by
// global-teardown.ts counts this run only.

import { rm } from "node:fs/promises";

import { AXE_RESULTS_DIR } from "./axe-results";

export default async function globalSetup(): Promise<void> {
  await rm(AXE_RESULTS_DIR, { recursive: true, force: true });
}
