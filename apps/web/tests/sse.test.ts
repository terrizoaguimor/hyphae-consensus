// SPDX-License-Identifier: Apache-2.0
//
// The run stream over a scripted EventSource fed with the contract's SSE
// fixture (sse-frames-retry.json): in-order delivery, duplicates dropped,
// gaps and resyncs asking for a detail re-read, stream_end closing the
// source, and the connection states reported as they are.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import {
  type EventSourceLike,
  RunStream,
  type RunStreamOptions,
  type StreamState,
} from "../src/lib/api/sse";
import type { ConsensusFrameV1, SseFrameV1 } from "../src/lib/api/types";

const fixture = JSON.parse(
  readFileSync(path.join(contractDir(), "fixtures", "sse-frames-retry.json"), "utf8"),
) as SseFrameV1[];

const consensusFrames = fixture.filter(
  (f): f is Extract<SseFrameV1, { frame: "consensus" }> => f.frame === "consensus",
);
const endFrame = fixture.find((f) => f.frame === "stream_end");

/** The wire form of a fixture frame: the inner JSON, without the `frame` tag. */
function wire(frame: SseFrameV1): string {
  const data: Record<string, unknown> = { ...frame };
  delete data.frame;
  return JSON.stringify(data);
}

class FakeEventSource implements EventSourceLike {
  readyState = 0;
  closed = false;
  readonly url: string;
  private readonly listeners = new Map<string, Array<(event: MessageEvent<string>) => void>>();

  constructor(url: string) {
    this.url = url;
  }

  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close(): void {
    this.closed = true;
    this.readyState = 2;
  }

  open(): void {
    this.readyState = 1;
    this.emit("open", "");
  }

  emit(type: string, data: string): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener({ data, lastEventId: "" } as MessageEvent<string>);
    }
  }
}

function connect(options: Partial<RunStreamOptions> = {}) {
  const frames: ConsensusFrameV1[] = [];
  const states: StreamState[] = [];
  const refetches: string[] = [];
  const duplicates: number[] = [];
  const invalid: string[] = [];
  let source: FakeEventSource | null = null;
  let ended: unknown = null;
  const stream = new RunStream({
    url: "/api/v1/runs/run-x/events",
    eventSource: (url) => {
      source = new FakeEventSource(url);
      return source;
    },
    onFrame: (frame) => frames.push(frame),
    onRefetch: (reason) => refetches.push(reason),
    onEnd: (end) => {
      ended = end;
    },
    onState: (state) => states.push(state),
    onDuplicate: (sequence) => duplicates.push(sequence),
    onInvalid: (name) => invalid.push(name),
    ...options,
  });
  if (source === null) {
    throw new Error("no EventSource created");
  }
  return { stream, source: source as FakeEventSource, frames, states, refetches, duplicates, invalid, ended: () => ended };
}

describe("RunStream", () => {
  it("delivers in-order frames, reports replay then end, and closes the source", () => {
    const t = connect();
    expect(t.stream.state).toBe("connecting");
    t.source.open();
    expect(t.stream.state).toBe("open");
    for (const frame of consensusFrames) {
      t.source.emit("consensus", wire(frame));
    }
    expect(t.frames.map((f) => f.event.sequence)).toEqual(
      consensusFrames.map((f) => f.event.sequence),
    );
    expect(t.stream.state).toBe("replaying");
    expect(t.stream.lastSequence).toBe(14);
    expect(t.frames.every((f) => f.delivery === "replay")).toBe(true);
    t.source.emit("stream_end", wire(endFrame as SseFrameV1));
    expect(t.stream.state).toBe("ended");
    expect(t.source.closed).toBe(true);
    expect(t.ended()).toMatchObject({ reason: "awaiting_human_approval", last_sequence: 14 });
    expect(t.states).toEqual(["open", "replaying", "ended"]);
    expect(t.refetches).toEqual([]);
    expect(t.invalid).toEqual([]);
  });

  it("drops duplicates (a reconnect replaying from Last-Event-ID)", () => {
    const t = connect();
    t.source.open();
    const [first, second, third] = consensusFrames;
    t.source.emit("consensus", wire(first as SseFrameV1));
    t.source.emit("consensus", wire(second as SseFrameV1));
    t.source.emit("consensus", wire(first as SseFrameV1));
    t.source.emit("consensus", wire(second as SseFrameV1));
    t.source.emit("consensus", wire(third as SseFrameV1));
    expect(t.frames.map((f) => f.event.sequence)).toEqual([1, 2, 3]);
    expect(t.duplicates).toEqual([1, 2]);
    expect(t.refetches).toEqual([]);
  });

  it("treats frames at or below `after` as already held", () => {
    const t = connect({ after: 12 });
    t.source.open();
    for (const frame of consensusFrames) {
      t.source.emit("consensus", wire(frame));
    }
    expect(t.frames.map((f) => f.event.sequence)).toEqual([13, 14]);
    expect(t.duplicates).toHaveLength(12);
  });

  it("detects a gap, asks for a re-read and keeps going", () => {
    const t = connect();
    t.source.open();
    const [first, , third, fourth] = consensusFrames;
    t.source.emit("consensus", wire(first as SseFrameV1));
    t.source.emit("consensus", wire(third as SseFrameV1));
    t.source.emit("consensus", wire(fourth as SseFrameV1));
    expect(t.frames.map((f) => f.event.sequence)).toEqual([1, 3, 4]);
    expect(t.refetches).toEqual(["gap"]);
    expect(t.stream.lastSequence).toBe(4);
  });

  it("a resync asks for a re-read; the replayed frames that follow are not duplicates of a gap", () => {
    const t = connect();
    t.source.open();
    const [first, second, third] = consensusFrames;
    t.source.emit("consensus", wire(first as SseFrameV1));
    t.source.emit("resync", JSON.stringify({ reason: "lagged", from_sequence: 1 }));
    t.source.emit("consensus", wire(second as SseFrameV1));
    t.source.emit("consensus", wire(third as SseFrameV1));
    expect(t.refetches).toEqual(["resync"]);
    expect(t.frames.map((f) => f.event.sequence)).toEqual([1, 2, 3]);
  });

  it("reports live frames as live and a browser reconnect as reconnecting", () => {
    const t = connect();
    t.source.open();
    const [first, second] = consensusFrames;
    t.source.emit("consensus", wire(first as SseFrameV1));
    t.source.emit("consensus", JSON.stringify({ ...(second as object), frame: undefined, delivery: "live" }));
    expect(t.stream.state).toBe("live");
    expect(t.frames[1]?.delivery).toBe("live");
    t.source.readyState = 0;
    t.source.emit("error", "");
    expect(t.stream.state).toBe("reconnecting");
    t.source.open();
    expect(t.stream.state).toBe("open");
    expect(t.states).toEqual(["open", "replaying", "live", "reconnecting", "open"]);
  });

  it("reports a closed source as an error, never as live", () => {
    const t = connect();
    t.source.readyState = 2;
    t.source.emit("error", "");
    expect(t.stream.state).toBe("error");
    expect(t.states).toEqual(["error"]);
  });

  it("drops a frame that fails the contract and reports it", () => {
    const t = connect();
    t.source.open();
    t.source.emit("consensus", "{not json");
    t.source.emit("consensus", JSON.stringify({ delivery: "replay", summary: {} }));
    expect(t.frames).toEqual([]);
    expect(t.invalid).toEqual(["consensus", "consensus"]);
    expect(t.stream.lastSequence).toBe(0);
  });

  it("close() stops delivery and reports closed", () => {
    const t = connect();
    t.source.open();
    t.stream.close();
    expect(t.source.closed).toBe(true);
    expect(t.stream.state).toBe("closed");
    t.source.emit("consensus", wire(consensusFrames[0] as SseFrameV1));
    expect(t.frames).toEqual([]);
  });
});
