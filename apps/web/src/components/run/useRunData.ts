"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The data behind the run workspace: the detail (GET /runs/{id}), the stream
// (GET /runs/{id}/events, every stored frame then the live tail, until
// stream_end), and the readability pacer between the stream and the view.
// A gap, a resync or the end of the stream re-reads the detail. Frames keep
// their server data untouched; pacing only decides when a replayed frame is
// shown, and reduced motion (the OS or the in-app preference) turns it off.
// The hook holds the state of one run: the workspace mounts it under a key
// per run id, so a different run starts from a fresh state.

import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { api, ApiError, isApiError } from "@/lib/api/client";
import { RunStream, type StreamState } from "@/lib/api/sse";
import type { ConsensusFrameV1, RunDetailV1, RunSummaryV1, StreamEndV1 } from "@/lib/api/types";
import { Pacer } from "@/lib/run/pacing";

import { usePreferences } from "../preferences";

export const PACING_INTERVAL_MS = 650;

export type StreamNotices = {
  gaps: number;
  resyncs: number;
  invalid: number;
};

export type RunData = {
  detail: RunDetailV1 | null;
  detailError: ApiError | null;
  /** Frames shown so far, in sequence order, deduplicated. */
  frames: ConsensusFrameV1[];
  /** The freshest summary: the last frame's while streaming, else the detail's. */
  summary: RunSummaryV1 | null;
  stream: StreamState;
  end: StreamEndV1 | null;
  notices: StreamNotices;
  /** Replayed frames queued by the pacer. */
  pending: number;
  pacing: "on" | "skipped" | "reduced";
  skipPacing: () => void;
  /** Replaces the detail after a decision or a cancel. */
  setDetail: (detail: RunDetailV1) => void;
};

function toApiError(error: unknown, path: string): ApiError {
  return isApiError(error)
    ? error
    : new ApiError({
        code: "unreachable",
        message: error instanceof Error ? error.message : "request failed",
        status: 0,
        path,
      });
}

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

function subscribeReducedMotion(listener: () => void): () => void {
  const query = window.matchMedia(REDUCED_MOTION_QUERY);
  query.addEventListener("change", listener);
  return () => query.removeEventListener("change", listener);
}

function systemReducedMotion(): boolean {
  return window.matchMedia(REDUCED_MOTION_QUERY).matches;
}

/** The OS setting (live) or the in-app preference. */
function useReducedMotion(): boolean {
  const { hydrated, motion } = usePreferences();
  const system = useSyncExternalStore(subscribeReducedMotion, systemReducedMotion, () => false);
  return system || (hydrated && motion === "reduce");
}

export function useRunData(runId: string): RunData {
  const [detail, setDetailState] = useState<RunDetailV1 | null>(null);
  const [detailError, setDetailError] = useState<ApiError | null>(null);
  const [frames, setFrames] = useState<ConsensusFrameV1[]>([]);
  const [stream, setStream] = useState<StreamState>("connecting");
  const [end, setEnd] = useState<StreamEndV1 | null>(null);
  const [notices, setNotices] = useState<StreamNotices>({ gaps: 0, resyncs: 0, invalid: 0 });
  const [pending, setPending] = useState(0);
  const [skipped, setSkipped] = useState(false);
  const reduced = useReducedMotion();
  const pacerRef = useRef<Pacer<ConsensusFrameV1> | null>(null);

  const pacingEnabled = !reduced && !skipped;

  useEffect(() => {
    pacerRef.current?.setEnabled(pacingEnabled);
  }, [pacingEnabled]);

  useEffect(() => {
    let cancelled = false;

    const loadDetail = () =>
      api
        .getRun(runId)
        .then((loaded) => {
          if (!cancelled) {
            setDetailState(loaded);
            setDetailError(null);
          }
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setDetailError(toApiError(error, `/runs/${runId}`));
          }
        });

    const pacer = new Pacer<ConsensusFrameV1>(
      (frame) => {
        if (cancelled) {
          return;
        }
        setFrames((previous) => {
          const sequence = frame.event.sequence;
          if (previous.some((f) => f.event.sequence === sequence)) {
            return previous;
          }
          const next = [...previous, frame];
          next.sort((a, b) => a.event.sequence - b.event.sequence);
          return next;
        });
        setPending(pacer.pending);
      },
      { intervalMs: PACING_INTERVAL_MS, enabled: pacingEnabled },
    );
    pacerRef.current = pacer;

    void loadDetail();

    const runStream = new RunStream({
      url: api.eventsUrl(runId),
      onFrame: (frame) => {
        pacer.push(frame, frame.delivery === "replay");
        setPending(pacer.pending);
      },
      onRefetch: (reason) => {
        setNotices((n) =>
          reason === "gap" ? { ...n, gaps: n.gaps + 1 } : { ...n, resyncs: n.resyncs + 1 },
        );
        void loadDetail();
      },
      onEnd: (streamEnd) => {
        setEnd(streamEnd);
        void loadDetail();
      },
      onState: (state) => {
        if (!cancelled) {
          setStream(state);
        }
      },
      onInvalid: () => setNotices((n) => ({ ...n, invalid: n.invalid + 1 })),
    });

    return () => {
      cancelled = true;
      runStream.close();
      pacer.dispose();
      pacerRef.current = null;
    };
    // pacingEnabled is applied through the effect above, not by reconnecting.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runId]);

  const skipPacing = useCallback(() => {
    setSkipped(true);
    pacerRef.current?.skip();
    setPending(0);
  }, []);

  const setDetail = useCallback((next: RunDetailV1) => setDetailState(next), []);

  const summary = useMemo<RunSummaryV1 | null>(() => {
    const last = frames.at(-1);
    if (last !== undefined && end === null && stream !== "ended") {
      return last.summary;
    }
    return detail?.summary ?? last?.summary ?? null;
  }, [frames, detail, end, stream]);

  return {
    detail,
    detailError,
    frames,
    summary,
    stream,
    end,
    notices,
    pending,
    pacing: reduced ? "reduced" : skipped ? "skipped" : "on",
    skipPacing,
    setDetail,
  };
}
