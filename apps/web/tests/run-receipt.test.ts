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
  runTimeReceipt,
  verifyCommand,
} from "../src/lib/run/receipt";

import { manifest, researchBytes } from "./research";

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

describe("the receipt a run record holds from run time", () => {
  const withRecord = manifest().runs.filter(
    (run): run is typeof run & { record: string } => typeof run.record === "string" && run.record !== "",
  );

  it("a run record of every battery holds its run's receipt, with its own limitations and time", () => {
    expect(withRecord).toHaveLength(46);
    // The first record of each battery (ten of them): enough to cover
    // every driver version without reading all 46 files through the
    // container's bind mount while the chain tests read the logs.
    const firsts = new Map<string, (typeof withRecord)[number]>();
    for (const run of withRecord) {
      if (!firsts.has(run.battery)) {
        firsts.set(run.battery, run);
      }
    }
    expect(firsts.size).toBe(10);
    for (const run of firsts.values()) {
      const receipt = runTimeReceipt(researchBytes(run.record), run.run_id);
      expect(receipt, run.label).not.toBeNull();
      // UTC days 2026-10-26 to 2026-10-28, and the operator server's runs of 2026-10-30,
      // 2026-11-01 and 2026-11-02 (the Proposer-on-Spark runs, local evening of 2026-11-01).
      expect(receipt?.generatedAt, run.label).toMatch(/^2026-(10-(2[678]|30)|11-0[12])T/);
      expect(receipt?.proofLimitations, run.label).not.toBeNull();
    }
  }, 30_000);

  it("R06's record states the limitations of its day, not the text this server writes now", () => {
    const r06 = withRecord.find((run) => run.label === "R06");
    expect(r06).toBeDefined();
    const receipt = runTimeReceipt(researchBytes(r06?.record ?? ""), r06?.run_id ?? "");
    expect(receipt?.generatedAt).toBe("2026-10-27T07:12:45.990721500+00:00");
    expect(receipt?.proofLimitations).toMatch(/^Integrity proofs, where valid, establish/);
    expect(receipt?.proofLimitations).not.toBe(KNOWN_PROOF_LIMITATIONS);
    expect(limitationsTranslated(receipt?.proofLimitations ?? "")).toBe(false);
  });

  it("is null for bytes that are not a run record of this run", () => {
    const encode = (value: string) => new TextEncoder().encode(value);
    const runId = "run-00000000-0000-4000-8000-000000000000";
    expect(runTimeReceipt(encode("not json"), runId)).toBeNull();
    expect(runTimeReceipt(new Uint8Array([0xff, 0xfe, 0x7b, 0x7d]), runId)).toBeNull();
    expect(runTimeReceipt(encode("[]"), runId)).toBeNull();
    expect(runTimeReceipt(encode(JSON.stringify({ receipt: null })), runId)).toBeNull();
    expect(runTimeReceipt(encode(JSON.stringify({ receipt: { run_id: "run-other" } })), runId)).toBeNull();
    expect(runTimeReceipt(encode(JSON.stringify({ receipt: { run_id: runId } })), runId)).toEqual({
      generatedAt: null,
      proofLimitations: null,
    });
    expect(
      runTimeReceipt(
        encode(JSON.stringify({ receipt: { run_id: runId, generated_at: "2026-10-27T00:00:00Z", proof_limitations: "As then." } })),
        runId,
      ),
    ).toEqual({ generatedAt: "2026-10-27T00:00:00Z", proofLimitations: "As then." });
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
