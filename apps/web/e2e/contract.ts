// SPDX-License-Identifier: Apache-2.0
//
// The API contract (contracts/api-v1) as the e2e suite sees it: on the host
// two directories above apps/web, inside the Playwright container at
// /contracts (playwright-docker.sh mounts it read-only there). Fixtures are
// the responses the real server produced over the guided scenarios; the
// specs serve them as the mocked /api/v1.

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

export function contractDir(): string {
  const fromEnv = process.env.API_CONTRACT_DIR;
  if (fromEnv !== undefined && fromEnv !== "") {
    return path.resolve(fromEnv);
  }
  // CommonJS under Playwright: __dirname is apps/web/e2e.
  for (const candidate of ["../../../contracts/api-v1", "../../contracts/api-v1"]) {
    const resolved = path.resolve(__dirname, candidate);
    if (existsSync(path.join(resolved, "contract-sha256.txt"))) {
      return resolved;
    }
  }
  throw new Error("contracts/api-v1 not found (set API_CONTRACT_DIR)");
}

export function fixture<T = unknown>(name: string): T {
  return JSON.parse(
    readFileSync(path.join(contractDir(), "fixtures", name), "utf8"),
  ) as T;
}
