// SPDX-License-Identifier: Apache-2.0
//
// What the run workspace shows (src/lib/run/view.ts), over the contract's
// retry fixtures: the stage follows the stream and fills from the detail what
// the stream did not deliver (a gap at once; the rest once the stream is
// settled and nothing is paced), so a failed stream never reports zero proofs
// for a run that has four; the newest summary never goes back to an older
// one when a re-read fails; a detail response never replaces a newer detail;
// and the recommendation of a revised proposal names the original one.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type { StreamState } from "../src/lib/api/sse";
import type { ConsensusFrameV1, RunDetailV1, SseFrameV1 } from "../src/lib/api/types";
import {
  decisionOf,
  endReasonOf,
  followsPlayback,
  headerSummary,
  mergeEvents,
  newerDetail,
  newestSummary,
  stageOf,
  stageRecords,
  streamSettled,
  supersededProposal,
} from "../src/lib/run/view";

function fixture<T>(name: string): T {
  return JSON.parse(readFileSync(path.join(contractDir(), "fixtures", name), "utf8")) as T;
}

const detail = fixture<RunDetailV1>("run-detail-retry.json");
const frames: ConsensusFrameV1[] = fixture<SseFrameV1[]>("sse-frames-retry.json")
  .filter((f): f is Extract<SseFrameV1, { frame: "consensus" }> => f.frame === "consensus")
  .map((f) => {
    const frame: Record<string, unknown> = { ...f };
    delete frame.frame;
    return frame as unknown as ConsensusFrameV1;
  });

const sequences = (records: ReturnType<typeof stageRecords>) => records.map((r) => r.event.sequence);

describe("stageRecords", () => {
  it("while the stream runs, shows the frames only (pacing decides when)", () => {
    const records = stageRecords(detail, frames.slice(0, 3), { settled: false, pending: 11 });
    expect(sequences(records)).toEqual([1, 2, 3]);
    expect(records.every((r) => !r.fromDetail && r.delivery === "replay")).toBe(true);
  });

  it("fills a gap below the newest frame from the detail at once, as replayed", () => {
    const shown = [frames[0], frames[2]].filter((f): f is ConsensusFrameV1 => f !== undefined);
    const records = stageRecords(detail, shown, { settled: false, pending: 0 });
    expect(sequences(records)).toEqual([1, 2, 3]);
    const filled = records.find((r) => r.event.sequence === 2);
    expect(filled?.fromDetail).toBe(true);
    expect(filled?.delivery).toBe("replay");
  });

  it("shows the stored records once the stream failed or ended and nothing is paced", () => {
    const records = stageRecords(detail, [], { settled: true, pending: 0 });
    expect(sequences(records)).toEqual(detail.events.map((e) => e.sequence));
    const stage = stageOf(records);
    expect(stage.retrieved).toBe(2);
    expect(stage.verifications).toHaveLength(4);
    expect(stage.recommendation?.document.kind).toBe("supported");
    expect(stage.proposal?.document.proposal.id).toBe("prop-2");
    expect(stage.proposal?.record.fromDetail).toBe(true);
  });

  it("never runs ahead of the pacer, even after stream_end", () => {
    const records = stageRecords(detail, frames.slice(0, 1), { settled: true, pending: 13 });
    expect(sequences(records)).toEqual([1]);
  });

  it("keeps a frame's own delivery over the detail's copy", () => {
    const live = frames.slice(0, 2).map((f) => ({ ...f, delivery: "live" as const }));
    const records = stageRecords(detail, live, { settled: true, pending: 0 });
    expect(records.slice(0, 2).map((r) => r.delivery)).toEqual(["live", "live"]);
    expect(records[2]?.delivery).toBe("replay");
    expect(records).toHaveLength(detail.events.length);
  });
});

describe("summaries and details", () => {
  const atSequence = (n: number): RunDetailV1 => ({
    ...detail,
    summary: { ...detail.summary, last_sequence: n, event_count: n },
  });

  it("the newest summary is the detail's unless the last frame is further along", () => {
    expect(newestSummary(null, [])).toBeNull();
    expect(newestSummary(detail, [])).toBe(detail.summary);
    const last = frames.at(-1);
    expect(newestSummary(null, frames)).toBe(last?.summary);
    // A re-read that failed leaves the mount-time detail (sequence 3): the
    // stream's newer summary (14) is kept.
    expect(newestSummary(atSequence(3), frames)?.last_sequence).toBe(14);
    // Once the re-read succeeded (15, after the decision) the detail wins.
    expect(newestSummary(detail, frames)?.last_sequence).toBe(15);
  });

  it("a detail response never replaces a detail that is further along", () => {
    expect(newerDetail(atSequence(14), null).summary.last_sequence).toBe(14);
    expect(newerDetail(atSequence(14), atSequence(15)).summary.last_sequence).toBe(15);
    expect(newerDetail(atSequence(15), atSequence(14)).summary.last_sequence).toBe(15);
    expect(newerDetail(atSequence(15), atSequence(15)).summary.last_sequence).toBe(15);
  });

  it("the Explorer's timeline is every record of the detail and the frames", () => {
    expect(mergeEvents(atSequence(15), frames.slice(0, 2)).map((e) => e.sequence)).toEqual(
      detail.events.map((e) => e.sequence),
    );
    expect(mergeEvents(null, frames).map((e) => e.sequence)).toEqual(
      frames.map((f) => f.event.sequence),
    );
  });
});

describe("supersededProposal", () => {
  it("names the original proposal a revision replaced", () => {
    expect(detail.current_proposal?.id).toBe("prop-2");
    expect(supersededProposal(detail)?.id).toBe("prop-1");
  });

  it("is null for a run whose current proposal is its first", () => {
    const offline = fixture<RunDetailV1>("run-detail-offline.json");
    const firstOnly = offline.events.every((e) => e.payload.type !== "revision_submitted");
    expect(supersededProposal(offline) === null).toBe(firstOnly);
    const created = fixture<RunDetailV1>("run-detail-live-R01-reference.json");
    expect(supersededProposal(created)).toBeNull();
  });
});

// HE #6 (round 4): one clock for the header and the stage, a status line
// derived from the run state, and the person's decision as its own value.
describe("the header follows the playback", () => {
  const paced = { pacedFrames: 3, pending: 11, settled: false };

  it("follows a paced playback until it reaches its end", () => {
    expect(followsPlayback(paced)).toBe(true);
    // The stream ended but the pacer still holds frames back.
    expect(followsPlayback({ pacedFrames: 14, pending: 3, settled: true })).toBe(true);
    // Every frame shown, but the stream may still send some.
    expect(followsPlayback({ pacedFrames: 14, pending: 0, settled: false })).toBe(true);
    // The end: settled and nothing queued; the stage shows the detail's tail too.
    expect(followsPlayback({ pacedFrames: 14, pending: 0, settled: true })).toBe(false);
    // Nothing was paced (reduced motion, a live run): the run's own state.
    expect(followsPlayback({ pacedFrames: 0, pending: 0, settled: false })).toBe(false);
  });

  it("shows the summary of the newest frame on the stage, never the stored ending", () => {
    const shown = frames.slice(0, 3);
    const summary = headerSummary(detail, shown, paced);
    expect(summary?.last_sequence).toBe(3);
    expect(summary?.state).toBe("evidence_review");
    expect(summary?.recommendation ?? null).toBeNull();
    expect(summary?.authorization).toBe("pending");
    // The ending (approved, supported) is what the detail holds.
    expect(detail.summary.state).toBe("approved");
    expect(newestSummary(detail, shown)?.state).toBe("approved");
  });

  it("matches the stage's own records at every step of the playback", () => {
    for (let n = 1; n <= frames.length; n += 1) {
      const shown = frames.slice(0, n);
      const position = { pacedFrames: n, pending: frames.length - n, settled: false };
      const stage = stageOf(stageRecords(detail, shown, position));
      const summary = headerSummary(detail, shown, position);
      // No outcome in the header before the stage shows the recommendation.
      expect(summary?.recommendation ?? null).toBe(stage.recommendation?.document.kind ?? null);
      expect(summary?.last_sequence).toBe(n);
    }
  });

  it("shows the newest summary once the playback ended, or when nothing was paced", () => {
    expect(headerSummary(detail, frames, { pacedFrames: 14, pending: 0, settled: true })?.state).toBe(
      "approved",
    );
    expect(
      headerSummary(detail, frames.slice(0, 2), { pacedFrames: 0, pending: 0, settled: false })?.state,
    ).toBe("approved");
    // Paced, but no frame shown yet: the newest known summary (the workspace
    // waits for the first frame of a scripted run before showing a header).
    expect(headerSummary(detail, [], paced)?.state).toBe("approved");
  });

  it("a stream is settled once it ended, failed or was closed", () => {
    const settled: StreamState[] = ["ended", "error", "closed"];
    const open: StreamState[] = ["connecting", "open", "replaying", "live", "reconnecting"];
    expect(settled.every(streamSettled)).toBe(true);
    expect(open.some(streamSettled)).toBe(false);
  });
});

describe("the status line's end reason comes from the run state", () => {
  it("a decided run reached its final state, even when its stream ended at the decision point", () => {
    expect(endReasonOf("approved", "awaiting_human_approval")).toBe("terminal");
    expect(endReasonOf("rejected", "awaiting_human_approval")).toBe("terminal");
    expect(endReasonOf("failed", "awaiting_human_approval")).toBe("terminal");
  });

  it("a run at its decision point awaits the decision", () => {
    expect(endReasonOf("awaiting_human_approval", "terminal")).toBe("awaiting_human_approval");
    expect(endReasonOf("awaiting_human_approval", "awaiting_human_approval")).toBe(
      "awaiting_human_approval",
    );
  });

  it("any other state keeps the end frame's own reason", () => {
    expect(endReasonOf("evaluated", "terminal")).toBe("terminal");
    expect(endReasonOf(null, "awaiting_human_approval")).toBe("awaiting_human_approval");
  });
});

describe("decisionOf", () => {
  it("names the person's decision from the summary, and none while pending", () => {
    expect(decisionOf(detail.summary)).toBe("approved");
    expect(decisionOf({ authorization: "approved", override: true })).toBe("approvedOverride");
    expect(decisionOf({ authorization: "rejected", override: false })).toBe("rejected");
    expect(decisionOf({ authorization: "pending", override: false })).toBeNull();
    // The playback's frames carry no decision before the end.
    expect(frames.every((f) => decisionOf(f.summary) === null)).toBe(true);
  });
});
