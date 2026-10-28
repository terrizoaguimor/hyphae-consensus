// SPDX-License-Identifier: Apache-2.0
//
// The generated contract types (src/lib/api/types.ts) must be exactly what
// scripts/gen-api-types.mjs produces from contracts/api-v1, and the contract
// directory must be the one its contract-sha256.txt names. Run `pnpm
// api:types` after the Rust exporter changes the contract.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  canonical,
  contractDir,
  generate,
  loadContract,
  OUTPUT_RELATIVE,
  schemasSha256,
} from "../scripts/gen-api-types.mjs";

const appDir = fileURLToPath(new URL("..", import.meta.url));
const contract = loadContract(contractDir());

describe("the API contract", () => {
  it("has one schema per contract type, each titled after its file", () => {
    expect(contract.schemas.length).toBeGreaterThanOrEqual(28);
    for (const { file, title } of contract.schemas) {
      expect(file).toBe(`${title}.schema.json`);
    }
  });

  it("hashes to its pinned contract-sha256.txt", () => {
    expect(schemasSha256(contract.schemas)).toBe(contract.sha256);
  });
});

describe("src/lib/api/types.ts", () => {
  const generated = generate(contract);
  const committed = readFileSync(path.join(appDir, OUTPUT_RELATIVE), "utf8");

  it("is the generator's output for the current contract (no drift)", () => {
    expect(committed).toBe(generated);
  });

  it("names the contract hash in its header and exports it", () => {
    expect(committed).toContain(`// contract-sha256: ${contract.sha256}`);
    expect(committed).toContain(
      `export const CONTRACT_SHA256 = "${contract.sha256}";`,
    );
  });

  it("exports a type for every schema title", () => {
    for (const { title } of contract.schemas) {
      expect(committed, title).toMatch(
        new RegExp(`^export type ${title} =`, "m"),
      );
    }
  });

  it("is generated deterministically", () => {
    expect(generate(contract)).toBe(generated);
  });

  it("uses LF line endings and ends with one newline", () => {
    expect(committed).not.toContain("\r");
    expect(committed.endsWith("\n")).toBe(true);
    expect(committed.endsWith("\n\n")).toBe(false);
  });
});

describe("canonical()", () => {
  it("orders object keys so equal schemas compare equal", () => {
    expect(canonical({ b: 1, a: [true, null] })).toBe(
      canonical({ a: [true, null], b: 1 }),
    );
    expect(canonical({ a: 1 })).not.toBe(canonical({ a: 2 }));
  });
});
