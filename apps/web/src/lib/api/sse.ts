// SPDX-License-Identifier: Apache-2.0
//
// The live stream of one run over EventSource (GET /api/v1/runs/{id}/events).
// The server sends `event: consensus` frames (one record each, `id:` = the
// record's sequence, `delivery` = `replay` or `live`), `event: resync` when it
// re-read its durable log (the frames that follow are replays), and
// `event: stream_end` once the run awaits a human or is terminal; a heartbeat
// comment and `retry: 3000` keep the connection alive and tell the browser
// how fast to reconnect.
//
// This class tracks the last sequence seen, drops duplicates (a reconnect
// replays from Last-Event-ID, which the browser sends on its own), detects
// gaps, and asks its owner to re-read the run detail on a gap or a resync. It
// never labels a frame: it reports each frame's delivery as the server sent
// it. Reconnects are the browser's and are reported as "reconnecting", never
// hidden.

import { consensusFrameSchema, resyncSchema, streamEndSchema } from "./schemas";
import type { ConsensusFrameV1, ResyncV1, StreamEndV1 } from "./types";

/**
 * Where the stream stands, for the UI:
 * - `connecting`: EventSource created, no response yet;
 * - `open`: connected, no frame received yet;
 * - `replaying`: the last frame carried stored records (`delivery: replay`);
 * - `live`: the last frame was published while connected (`delivery: live`);
 * - `reconnecting`: the connection dropped and the browser is retrying;
 * - `ended`: the server sent `stream_end` (see `endReason`);
 * - `error`: the browser gave up (the route refused or is gone);
 * - `closed`: closed by this client.
 */
export type StreamState =
  | "connecting"
  | "open"
  | "replaying"
  | "live"
  | "reconnecting"
  | "ended"
  | "error"
  | "closed";

export type RunStreamHandlers = {
  onFrame: (frame: ConsensusFrameV1) => void;
  /** A gap or a resync: the run detail must be re-read; the stream goes on. */
  onRefetch: (reason: "gap" | "resync", detail: ResyncV1 | { expected: number; received: number }) => void;
  onEnd: (end: StreamEndV1) => void;
  onState: (state: StreamState) => void;
  /** A frame that fails the contract; it is dropped and reported. */
  onInvalid?: (eventName: string, detail: string) => void;
  /** A frame whose sequence was already seen; dropped. */
  onDuplicate?: (sequence: number) => void;
};

/** The subset of EventSource this class uses; injectable for tests. */
export interface EventSourceLike {
  readonly readyState: number;
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void;
  close(): void;
}

export type EventSourceFactory = (url: string) => EventSourceLike;

const CONNECTING = 0;
const CLOSED = 2;

export type RunStreamOptions = RunStreamHandlers & {
  url: string;
  /** The last sequence the caller already holds; frames at or below it are duplicates. */
  after?: number;
  /** Defaults to the browser's EventSource. */
  eventSource?: EventSourceFactory;
};

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

export class RunStream {
  private source: EventSourceLike | null = null;
  private stateValue: StreamState = "connecting";
  private last: number;
  private endValue: StreamEndV1 | null = null;
  private readonly handlers: RunStreamHandlers;

  constructor(options: RunStreamOptions) {
    this.handlers = options;
    this.last = options.after ?? 0;
    const factory: EventSourceFactory =
      options.eventSource ??
      ((url) => new EventSource(url, { withCredentials: false }));
    const source = factory(options.url);
    this.source = source;
    source.addEventListener("open", () => {
      if (this.stateValue === "connecting" || this.stateValue === "reconnecting") {
        // After a reconnect the server replays from Last-Event-ID, so the
        // frames that follow say what they are; until one arrives the stream
        // is only open.
        this.setState("open");
      }
    });
    source.addEventListener("consensus", (event) => this.onConsensus(event));
    source.addEventListener("resync", (event) => this.onResync(event));
    source.addEventListener("stream_end", (event) => this.onStreamEnd(event));
    source.addEventListener("error", () => this.onError());
  }

  get state(): StreamState {
    return this.stateValue;
  }

  /** The highest sequence delivered so far (or `after` before any). */
  get lastSequence(): number {
    return this.last;
  }

  get endReason(): StreamEndV1 | null {
    return this.endValue;
  }

  /** Closes the connection; no handler is called afterwards. */
  close(): void {
    if (this.source === null) {
      return;
    }
    this.source.close();
    this.source = null;
    if (this.stateValue !== "ended") {
      this.setState("closed");
    }
  }

  private setState(state: StreamState): void {
    if (state === this.stateValue) {
      return;
    }
    this.stateValue = state;
    this.handlers.onState(state);
  }

  private onConsensus(event: MessageEvent<string>): void {
    if (this.source === null) {
      return;
    }
    const parsed = consensusFrameSchema.safeParse(parseJson(event.data));
    if (!parsed.success) {
      this.handlers.onInvalid?.(
        "consensus",
        parsed.error.issues
          .slice(0, 3)
          .map((issue) => `${issue.path.join(".") || "$"}: ${issue.message}`)
          .join("; "),
      );
      return;
    }
    const frame = parsed.data;
    const sequence = frame.event.sequence;
    if (sequence <= this.last) {
      this.handlers.onDuplicate?.(sequence);
      return;
    }
    if (sequence !== this.last + 1) {
      this.handlers.onRefetch("gap", { expected: this.last + 1, received: sequence });
    }
    this.last = sequence;
    this.setState(frame.delivery === "live" ? "live" : "replaying");
    this.handlers.onFrame(frame);
  }

  private onResync(event: MessageEvent<string>): void {
    if (this.source === null) {
      return;
    }
    const parsed = resyncSchema.safeParse(parseJson(event.data));
    if (!parsed.success) {
      this.handlers.onInvalid?.("resync", parsed.error.message);
      return;
    }
    // The server re-read its log; what it replays next may differ from what
    // this client holds, and the detail is the truth to re-read.
    this.handlers.onRefetch("resync", parsed.data);
  }

  private onStreamEnd(event: MessageEvent<string>): void {
    if (this.source === null) {
      return;
    }
    const parsed = streamEndSchema.safeParse(parseJson(event.data));
    if (!parsed.success) {
      this.handlers.onInvalid?.("stream_end", parsed.error.message);
      return;
    }
    this.endValue = parsed.data;
    // Closing first: the server closes the stream after this frame, and an
    // open EventSource would reconnect and replay for nothing.
    this.source.close();
    this.source = null;
    this.setState("ended");
    this.handlers.onEnd(parsed.data);
  }

  private onError(): void {
    const source = this.source;
    if (source === null) {
      return;
    }
    if (source.readyState === CLOSED) {
      // The browser gave up: the response was not an event stream (404, 429,
      // 503) or the server refused. No automatic retry follows.
      this.source = null;
      this.setState("error");
      return;
    }
    if (source.readyState === CONNECTING) {
      this.setState("reconnecting");
    }
  }
}
