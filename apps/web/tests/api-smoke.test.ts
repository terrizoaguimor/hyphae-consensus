// SPDX-License-Identifier: Apache-2.0
//
// Responses captured from a real `consensus-api serve-public` (the manual
// smoke described in README.md, "Build only through Docker" cannot run the
// server, so the capture is a host-side curl into the git-ignored
// test-results/smoke/) must pass the zod mirrors like the contract fixtures
// do. The suite skips when no capture is present, so it never depends on a
// running server; with one, it is the strongest check that the mirrors match
// the server as deployed, including the archived live runs of the showcase.
//
// Files it reads when present: health.json, capabilities.json,
// scenarios.json, session.json, run-list.json, run-detail.json,
// receipt.json, showcase.json, showcase-detail.json, showcase-receipt.json
// (JSON bodies) and events.sse.txt (a text/event-stream capture).

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  capabilitiesSchema,
  consensusFrameSchema,
  healthSchema,
  receiptSchema,
  runDetailSchema,
  runListSchema,
  scenariosSchema,
  sessionSchema,
  showcaseSchema,
  streamEndSchema,
} from "../src/lib/api/schemas";

const smokeDir = fileURLToPath(new URL("../test-results/smoke/", import.meta.url));

function has(name: string): boolean {
  return existsSync(path.join(smokeDir, name));
}

function json(name: string): unknown {
  return JSON.parse(readFileSync(path.join(smokeDir, name), "utf8"));
}

/** `event:` / `data:` blocks of a text/event-stream capture. */
export function parseSse(text: string): Array<{ event: string; id: string | null; data: unknown }> {
  const frames: Array<{ event: string; id: string | null; data: unknown }> = [];
  for (const block of text.split(/\r?\n\r?\n/)) {
    let event = "message";
    let id: string | null = null;
    const data: string[] = [];
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith(":") || line.trim() === "") {
        continue;
      }
      const colon = line.indexOf(":");
      const field = colon === -1 ? line : line.slice(0, colon);
      const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
      if (field === "event") {
        event = value;
      } else if (field === "id") {
        id = value;
      } else if (field === "data") {
        data.push(value);
      }
    }
    if (data.length > 0) {
      frames.push({ event, id, data: JSON.parse(data.join("\n")) });
    }
  }
  return frames;
}

describe.skipIf(!existsSync(smokeDir))("captured real-server responses", () => {
  const cases: Array<[string, { parse: (value: unknown) => unknown }]> = [
    ["health.json", healthSchema],
    ["capabilities.json", capabilitiesSchema],
    ["scenarios.json", scenariosSchema],
    ["session.json", sessionSchema],
    ["run-list.json", runListSchema],
    ["run-detail.json", runDetailSchema],
    ["receipt.json", receiptSchema],
    ["showcase.json", showcaseSchema],
    ["showcase-detail.json", runDetailSchema],
    ["showcase-receipt.json", receiptSchema],
    ["showcase-detail-live.json", runDetailSchema],
    ["showcase-receipt-live.json", receiptSchema],
  ];

  for (const [name, schema] of cases) {
    it.skipIf(!has(name))(`${name} parses`, () => {
      expect(() => schema.parse(json(name))).not.toThrow();
    });
  }

  it.skipIf(!has("events.sse.txt"))("events.sse.txt: consensus frames in sequence, then stream_end", () => {
    const frames = parseSse(readFileSync(path.join(smokeDir, "events.sse.txt"), "utf8"));
    const consensus = frames.filter((f) => f.event === "consensus");
    expect(consensus.length).toBeGreaterThan(0);
    let expected = 1;
    for (const frame of consensus) {
      const parsed = consensusFrameSchema.parse(frame.data);
      expect(frame.id).toBe(String(parsed.event.sequence));
      expect(parsed.event.sequence).toBe(expected);
      expected += 1;
    }
    const end = frames.find((f) => f.event === "stream_end");
    if (end !== undefined) {
      const parsed = streamEndSchema.parse(end.data);
      expect(parsed.last_sequence).toBe(expected - 1);
    }
  });

  it.skipIf(!has("showcase-detail.json"))("an archived run is the archive's and never active", () => {
    const detail = runDetailSchema.parse(json("showcase-detail.json"));
    expect(detail.summary.provenance.source).toBe("research_archive");
    expect(detail.summary.active).toBe(false);
  });

  it.skipIf(!has("showcase-detail-live.json"))("an archived live run carries its Meta request ids", () => {
    const detail = runDetailSchema.parse(json("showcase-detail-live.json"));
    expect(detail.summary.provenance.model).toBe("live_meta");
    expect(detail.summary.provenance.meta_request_ids?.length ?? 0).toBeGreaterThan(0);
    expect(detail.summary.active).toBe(false);
  });
});

describe("parseSse", () => {
  it("reads ids, event names and multi-line data, skipping comments", () => {
    const frames = parseSse(": stream-start\nretry: 3000\n\nid: 7\nevent: consensus\ndata: {\"a\":\ndata: 1}\n\nevent: stream_end\ndata: {}\n\n");
    expect(frames).toEqual([
      { event: "consensus", id: "7", data: { a: 1 } },
      { event: "stream_end", id: null, data: {} },
    ]);
  });
});
