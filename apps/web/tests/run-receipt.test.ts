// SPDX-License-Identifier: Apache-2.0
//
// What the views read from a receipt (src/lib/run/receipt.ts): the pinned
// limitations text is exactly what every receipt of the contract carries (the
// Proof inspector translates only that text and shows any other verbatim),
// and the read-only check command names the files the browser saves.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { createApiClient, researchUrl } from "../src/lib/api/client";
import type { RunReceiptV2 } from "../src/lib/api/types";
import {
  baseName,
  KNOWN_PROOF_LIMITATIONS,
  limitationsTranslated,
  ownLogFileName,
  receiptFileName,
  verifyCommand,
} from "../src/lib/run/receipt";

const fixturesDir = path.join(contractDir(), "fixtures");
const receipts = readdirSync(fixturesDir)
  .filter((name) => name.startsWith("receipt-v2-"))
  .map((name) => JSON.parse(readFileSync(path.join(fixturesDir, name), "utf8")) as RunReceiptV2);

describe("proof limitations", () => {
  it("the pinned text is the one every receipt fixture carries", () => {
    expect(receipts.length).toBeGreaterThanOrEqual(5);
    for (const receipt of receipts) {
      expect(receipt.proof_limitations).toBe(KNOWN_PROOF_LIMITATIONS);
      expect(limitationsTranslated(receipt.proof_limitations)).toBe(true);
    }
  });

  it("any other text is not translated", () => {
    expect(limitationsTranslated(`${KNOWN_PROOF_LIMITATIONS} And more.`)).toBe(false);
    expect(limitationsTranslated("Proofs establish everything.")).toBe(false);
  });
});

describe("the read-only check of a downloaded log", () => {
  const runId = "run-00000000-0000-4000-8000-000000000001";

  it("names the files the server's downloads are saved as", () => {
    expect(ownLogFileName(runId)).toBe(`events-${runId}.jsonl`);
    expect(receiptFileName(runId)).toBe(`receipt-${runId}.json`);
    expect(verifyCommand(ownLogFileName(runId), runId, receiptFileName(runId))).toBe(
      `consensus-api verify events-${runId}.jsonl --run ${runId} --receipt receipt-${runId}.json`,
    );
  });

  it("uses the archive file's own name for a recorded run", () => {
    expect(baseName("2026-10-27/live-battery-2026-10-27-final.jsonl")).toBe(
      "live-battery-2026-10-27-final.jsonl",
    );
    expect(baseName("MANIFEST.json")).toBe("MANIFEST.json");
  });

  it("links an archive file beside the API, each path segment encoded", () => {
    expect(researchUrl("2026-10-27/live-battery-2026-10-27-final.jsonl")).toBe(
      "/research/2026-10-27/live-battery-2026-10-27-final.jsonl",
    );
    expect(researchUrl("a dir/b?.json")).toBe("/research/a%20dir/b%3F.json");
    const api = createApiClient();
    expect(api.showcaseReceiptUrl(runId)).toBe(`/api/v1/showcase/${runId}/receipt`);
    expect(api.researchUrl("MANIFEST.json")).toBe("/research/MANIFEST.json");
  });

  it("quotes a file name a shell would split", () => {
    expect(verifyCommand("my log.jsonl", runId, "it's.json")).toBe(
      `consensus-api verify 'my log.jsonl' --run ${runId} --receipt 'it'\\''s.json'`,
    );
  });
});
