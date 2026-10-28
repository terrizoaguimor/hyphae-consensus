// SPDX-License-Identifier: Apache-2.0
//
// The in-browser chain verifier (src/lib/chain/verify.ts) over the real
// archived bytes of research/ (mounted read-only at /research by
// scripts/in-docker.sh), with WebCrypto's SHA-256 as the browser uses it:
// every file's digest is the one research/MANIFEST.json pins; every chained
// log verifies record by record, with exactly the records the manifest
// counts; the F03 run's segment ends at the head the server computed (the
// contract's showcase entry); the schema-1.0 console log reads as an
// unchained prefix; and a flipped byte, a changed link, a missing record, a
// duplicated id, a torn tail and an empty line each fail at the right line
// and sequence. A small chain written here checks the textual recipe
// itself: only the top-level hash value is blanked.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type { ShowcaseEntryV1 } from "../src/lib/api/types";
import { sha256Hex } from "../src/lib/chain/digest";
import {
  blankedLine,
  checkSegment,
  recordHash,
  topLevelStringSpan,
  verifyLog,
} from "../src/lib/chain/verify";

import { manifest, researchBytes } from "./research";

const F03 = JSON.parse(
  readFileSync(path.join(contractDir(), "fixtures", "showcase-entry-F03.json"), "utf8"),
) as ShowcaseEntryV1;

const PREFIX_LOG = "2026-10-27/live-battery-2026-10-27-prefix.jsonl";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** The byte offsets where each line of `bytes` starts. */
function lineStarts(bytes: Uint8Array): number[] {
  const starts = [0];
  bytes.forEach((b, i) => {
    if (b === 0x0a && i + 1 < bytes.length) {
      starts.push(i + 1);
    }
  });
  return starts;
}

/** The bytes with one line (1-based) replaced by `edit(line)`. */
function editLine(bytes: Uint8Array, lineNo: number, edit: (line: string) => string | null): Uint8Array {
  const lines = decoder.decode(bytes).split("\n");
  const edited = edit(lines[lineNo - 1] ?? "");
  if (edited === null) {
    lines.splice(lineNo - 1, 1);
  } else {
    lines[lineNo - 1] = edited;
  }
  return encoder.encode(lines.join("\n"));
}

describe("the archive's logs, as pinned", () => {
  const logs = manifest().files.filter((file) => file.kind === "event_log");

  it("are the files the manifest pins, byte for byte", async () => {
    expect(logs).toHaveLength(8);
    for (const file of manifest().files) {
      const bytes = researchBytes(file.path);
      expect(bytes.length, file.path).toBe(file.bytes);
      expect(await sha256Hex(bytes), file.path).toBe(file.sha256);
    }
  });

  it.each(logs.map((file) => [file.path, file] as const))(
    "%s verifies record by record",
    async (_path, file) => {
      const result = await verifyLog(researchBytes(file.path));
      expect(result.failure).toBeNull();
      expect(result.sha256).toBe(file.sha256);
      expect(result.records).toHaveLength(file.events ?? -1);
      const chained = result.records.filter((r) => r.hash !== "").length;
      expect(chained).toBe(file.schema_versions.includes("1.0") ? 0 : result.records.length);
    },
  );

  it("F03's segment ends at the head the server computed, in the file it pins", async () => {
    const result = await verifyLog(researchBytes(F03.file));
    expect(result.sha256).toBe(F03.sha256);
    const segment = checkSegment(result, {
      runId: F03.run_id,
      sha256: F03.sha256,
      receiptHead: F03.head_hash ?? null,
      receiptEventCount: F03.event_count,
    });
    expect(segment.head).toBe(F03.head_hash);
    expect(segment.records).toHaveLength(F03.event_count);
    expect(segment).toMatchObject({ chain: "pass", sha256: "pass", head_matches: "pass", count: "pass", pass: true });
    // The run's records are one contiguous segment of the multi-run log.
    expect((segment.last ?? 0) - (segment.first ?? 0) + 1).toBe(F03.event_count);
    expect(segment.first).toBeGreaterThan(1);
  });

  it("the schema-1.0 console log is an unchained prefix: nothing to recompute", async () => {
    const run = manifest().runs.find((r) => r.label === "C01");
    const result = await verifyLog(researchBytes("2026-10-26/console-events.jsonl"));
    expect(result.failure).toBeNull();
    expect(result.records.every((r) => r.hash === "" && r.prevHash === "")).toBe(true);
    const segment = checkSegment(result, { runId: run?.run_id ?? "", receiptHead: null });
    expect(segment.unchained).toBe(true);
    expect(segment.head).toBeNull();
    expect(segment.head_matches).toBe("pass");
  });
});

describe("a changed file fails at the record that changed", () => {
  const original = researchBytes(PREFIX_LOG);

  it("a flipped byte inside record 20's payload: hash mismatch at sequence 20", async () => {
    const start = lineStarts(original)[19] ?? 0;
    const line = decoder.decode(original.subarray(start, original.indexOf(0x0a, start)));
    // Record 20 opens the second run of the log: its task's description is ASCII text.
    const key = '"description":"';
    expect(line).toContain('"sequence":20,');
    expect(line).toContain(key);
    // The byte offset of the description's second character (UTF-8 bytes, not UTF-16 units).
    const at = start + encoder.encode(line.slice(0, line.indexOf(key) + key.length + 1)).length;
    const flipped = original.slice();
    flipped[at] = (flipped[at] ?? 0) ^ 0x20; // a letter's case: still UTF-8, still JSON
    expect(flipped[at]).not.toBe(original[at]);
    const result = await verifyLog(flipped);
    expect(result.failure).toMatchObject({ line: 20, sequence: 20, problem: { kind: "hash_mismatch" } });
    expect(result.records).toHaveLength(19);
    expect(result.sha256).not.toBe(manifest().files.find((f) => f.path === PREFIX_LOG)?.sha256);
  });

  it("a changed prev_hash: the link fails at that record", async () => {
    const changed = editLine(original, 7, (line) => line.replace(/"prev_hash":"[0-9a-f]{64}"/, `"prev_hash":"${"0".repeat(64)}"`));
    const result = await verifyLog(changed);
    expect(result.failure).toMatchObject({ line: 7, sequence: 7, problem: { kind: "prev_mismatch" } });
  });

  it("a removed record: a sequence gap where it was", async () => {
    const result = await verifyLog(editLine(original, 10, () => null));
    expect(result.failure).toMatchObject({
      line: 10,
      sequence: 11,
      problem: { kind: "sequence_gap", expected: 10, found: 11 },
    });
  });

  it("a duplicated event id, a torn tail and an empty line are refused where they are", async () => {
    const lines = decoder.decode(original).split("\n");
    const firstId = /"event_id":"([^"]+)"/.exec(lines[0] ?? "")?.[1] ?? "";
    const duplicated = editLine(original, 2, (line) => line.replace(/"event_id":"[^"]+"/, `"event_id":"${firstId}"`));
    expect((await verifyLog(duplicated)).failure).toMatchObject({ line: 2, problem: { kind: "duplicate_id" } });

    const torn = original.subarray(0, original.length - 40);
    expect((await verifyLog(torn)).failure).toMatchObject({ line: 47, problem: { kind: "torn_tail" } });

    const blank = encoder.encode(decoder.decode(original).replace("\n", "\n\n"));
    expect((await verifyLog(blank)).failure).toMatchObject({ line: 2, problem: { kind: "empty_line" } });
  });

  it("an unterminated but complete final record is accepted, as event_log.rs accepts it", async () => {
    const result = await verifyLog(original.subarray(0, original.length - 1));
    expect(result.failure).toBeNull();
    expect(result.unterminated).toBe(true);
    expect(result.records).toHaveLength(47);
  });
});

describe("the textual recipe", () => {
  /** A small chain, hashed with the recipe: SHA-256(prev_hash ++ line with "hash":""). */
  async function chain(payloads: string[]): Promise<Uint8Array> {
    let prev = "genesis";
    const out: string[] = [];
    for (const [i, payload] of payloads.entries()) {
      const blank = `{"schema_version":"1.3","sequence":${i + 1},"event_id":"e${i + 1}","run_id":"run-x","actor":{"kind":"runtime"},"occurred_at":"2026-10-28T00:00:00Z","prev_hash":"${prev}","hash":"","payload":${payload}}`;
      const hash = await recordHash(prev, blank);
      if (hash === null) {
        throw new Error("no hash field");
      }
      out.push(blank.replace('"hash":""', `"hash":"${hash}"`));
      prev = hash;
    }
    return encoder.encode(`${out.join("\n")}\n`);
  }

  it("blanks only the top-level hash, never a nested key or a text that looks like one", () => {
    const line = '{"a":{"hash":"nested"},"t":"say \\"hash\\":\\"x\\"","hash":"abc","z":[{"hash":"deep"}]}';
    const span = topLevelStringSpan(line, "hash");
    expect(span === null ? null : line.slice(span.start, span.end)).toBe('"abc"');
    expect(blankedLine(line)).toBe('{"a":{"hash":"nested"},"t":"say \\"hash\\":\\"x\\"","hash":"","z":[{"hash":"deep"}]}');
    expect(blankedLine('{"hash":7}')).toBeNull();
    expect(blankedLine('{"nohash":"x"}')).toBeNull();
  });

  it("a chain written with the recipe verifies, and fails where a record changes", async () => {
    const bytes = await chain([
      '{"type":"note","text":"one"}',
      '{"type":"note","text":"two, with \\"hash\\":\\"fake\\" inside"}',
      '{"type":"note","text":"três"}',
    ]);
    const result = await verifyLog(bytes);
    expect(result.failure).toBeNull();
    expect(result.records.map((r) => r.sequence)).toEqual([1, 2, 3]);
    const segment = checkSegment(result, { runId: "run-x", receiptHead: result.records[2]?.hash, receiptEventCount: 3, firstSequence: 1, lastSequence: 3 });
    expect(segment.pass).toBe(true);
    expect(checkSegment(result, { runId: "run-x", receiptHead: "0".repeat(64) })).toMatchObject({ head_matches: "fail", pass: false });
    expect(checkSegment(result, { runId: "run-x", receiptEventCount: 4 })).toMatchObject({ count: "fail", pass: false });
    expect(checkSegment(result, { runId: "run-y" })).toMatchObject({ chain: "fail", pass: false });
    const changed = encoder.encode(decoder.decode(bytes).replace("três", "tres"));
    expect((await verifyLog(changed)).failure).toMatchObject({ line: 3, sequence: 3, problem: { kind: "hash_mismatch" } });
  });
});
