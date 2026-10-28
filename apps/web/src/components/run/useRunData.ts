"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The data behind the run workspace: the detail (GET /runs/{id}), the stream
// (GET /runs/{id}/events, every stored frame then the live tail, until
// stream_end), and the readability pacer between the stream and the view.
// A gap, a resync or the end of the stream re-reads the detail. Frames keep
// their server data untouched; pacing only decides when a replayed frame of a
// scripted run is shown (src/lib/provenance.ts isPaceable: a live or mixed
// model run is never paced), and reduced motion (the OS or the in-app
// preference) turns it off. A failed detail request is kept as an error to
// show (never a loading line), with a retry; a failed stream can be opened
// again. The hook holds the state of one run: the workspace mounts it under a
// key per run id, so a different run starts from a fresh state.

import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { api, ApiError, isApiError } from "@/lib/api/client";
import { RunStream, type StreamState } from "@/lib/api/sse";
import type { ConsensusFrameV1, RunDetailV1, RunSummaryV1, StreamEndV1 } from "@/lib/api/types";
import { isPaceable } from "@/lib/provenance";
import { Pacer } from "@/lib/run/pacing";
import { newerDetail, newestSummary } from "@/lib/run/view";

import { usePreferences } from "../preferences";

export const PACING_INTERVAL_MS = 650;

export type StreamNotices = {
  gaps: number;
  resyncs: number;
  invalid: number;
};

export type RunData = {
  detail: RunDetailV1 | null;
  /** The newest detail request failed (and none succeeded since). */
  detailError: ApiError | null;
  /** Frames shown so far, in sequence order, deduplicated. */
  frames: ConsensusFrameV1[];
  /** The newest known summary: the detail's or the last frame's, whichever is further along. */
  summary: RunSummaryV1 | null;
  stream: StreamState;
  end: StreamEndV1 | null;
  notices: StreamNotices;
  /** Replayed frames queued by the pacer. */
  pending: number;
  pacing: "on" | "skipped" | "reduced";
  /** Frames that went through the pacer (replayed frames of a scripted run, paced). */
  pacedFrames: number;
  skipPacing: () => void;
  /** Replaces the detail after a decision or a cancel (never with an older one). */
  setDetail: (detail: RunDetailV1) => void;
  /** Reads the detail again (the retry of a failed GET /runs/{id}). */
  reloadDetail: () => void;
  /** Opens the stream again after it failed, after the newest record held. */
  reconnect: () => void;
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
  const [pacedFrames, setPacedFrames] = useState(0);
  const [skipped, setSkipped] = useState(false);
  const reduced = useReducedMotion();
  const pacerRef = useRef<Pacer<ConsensusFrameV1> | null>(null);
  const loadRef = useRef<(() => void) | null>(null);
  const connectRef = useRef<(() => void) | null>(null);

  const pacingEnabled = !reduced && !skipped;

  useEffect(() => {
    pacerRef.current?.setEnabled(pacingEnabled);
  }, [pacingEnabled]);

  useEffect(() => {
    let cancelled = false;
    // Responses of older detail requests are dropped: the newest request
    // decides, and a detail never goes back to an earlier sequence.
    let latestRequest = 0;

    const loadDetail = () => {
      latestRequest += 1;
      const request = latestRequest;
      api
        .getRun(runId)
        .then((loaded) => {
          if (!cancelled && request === latestRequest) {
            setDetailState((current) => newerDetail(loaded, current));
            setDetailError(null);
          }
        })
        .catch((error: unknown) => {
          if (!cancelled && request === latestRequest) {
            setDetailError(toApiError(error, `/runs/${runId}`));
          }
        });
    };
    loadRef.current = loadDetail;

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

    let runStream: RunStream | null = null;

    const connect = () => {
      const after = runStream?.lastSequence ?? 0;
      runStream?.close();
      runStream = new RunStream({
        url: api.eventsUrl(runId, after > 0 ? after : undefined),
        after,
        onFrame: (frame) => {
          const paced = isPaceable(frame.delivery, frame.summary.provenance);
          if (paced && pacer.isEnabled) {
            setPacedFrames((n) => n + 1);
          }
          pacer.push(frame, paced);
          setPending(pacer.pending);
        },
        onRefetch: (reason) => {
          setNotices((n) =>
            reason === "gap" ? { ...n, gaps: n.gaps + 1 } : { ...n, resyncs: n.resyncs + 1 },
          );
          loadDetail();
        },
        onEnd: (streamEnd) => {
          setEnd(streamEnd);
          loadDetail();
        },
        onState: (state) => {
          if (!cancelled) {
            setStream(state);
          }
        },
        onInvalid: () => setNotices((n) => ({ ...n, invalid: n.invalid + 1 })),
      });
    };
    // A reconnect (after the stream failed) starts after the newest record
    // the failed stream delivered, so nothing is shown twice.
    connectRef.current = () => {
      setEnd(null);
      setStream("connecting");
      connect();
    };

    loadDetail();
    connect();

    return () => {
      cancelled = true;
      runStream?.close();
      pacer.dispose();
      pacerRef.current = null;
      loadRef.current = null;
      connectRef.current = null;
    };
    // pacingEnabled is applied through the effect above, not by reconnecting.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runId]);

  const skipPacing = useCallback(() => {
    setSkipped(true);
    pacerRef.current?.skip();
    setPending(0);
  }, []);

  const setDetail = useCallback((next: RunDetailV1) => {
    setDetailState((current) => newerDetail(next, current));
    setDetailError(null);
  }, []);

  const reloadDetail = useCallback(() => loadRef.current?.(), []);
  const reconnect = useCallback(() => connectRef.current?.(), []);

  const summary = useMemo(() => newestSummary(detail, frames), [detail, frames]);

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
    pacedFrames,
    skipPacing,
    setDetail,
    reloadDetail,
    reconnect,
  };
}
