"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The data behind the run workspace in RECORDED mode: an archived run of the
// research showcase, read once from GET /showcase/{id} (the detail the server
// derives from the archived chain; there is no stream, nothing runs, and the
// run is never presented as running), with its showcase entry (label,
// battery, the research file it is a segment of and that file's pinned
// SHA-256) and the archive manifest's note on that file (src/components/
// showcase/useArchive.ts: the manifest is hashed in this browser first). A
// failed detail request is kept as an error to show with a retry, never as a
// loading line.

import { useCallback, useEffect, useState } from "react";

import { api, ApiError, isApiError } from "@/lib/api/client";
import type { ConsensusFrameV1, RunDetailV1, RunSummaryV1, ShowcaseEntryV1 } from "@/lib/api/types";

import { type ArchiveState, useArchive } from "../showcase/useArchive";

export type RecordedRunData = {
  detail: RunDetailV1 | null;
  detailError: ApiError | null;
  /** A recorded run has no stream: no frame ever arrives. */
  frames: ConsensusFrameV1[];
  summary: RunSummaryV1 | null;
  reloadDetail: () => void;
  archive: ArchiveState;
  /** The run's showcase entry, once GET /showcase answered (null if it does not list it). */
  entry: ShowcaseEntryV1 | null;
};

const NO_FRAMES: ConsensusFrameV1[] = [];

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

export function useRecordedRun(runId: string): RecordedRunData {
  const [detail, setDetail] = useState<RunDetailV1 | null>(null);
  const [detailError, setDetailError] = useState<ApiError | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { state: archive } = useArchive(true);

  useEffect(() => {
    let cancelled = false;
    api
      .showcaseDetail(runId)
      .then((loaded) => {
        if (!cancelled) {
          setDetail(loaded);
          setDetailError(null);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDetailError(toApiError(error, `/showcase/${runId}`));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [runId, attempt]);

  const reloadDetail = useCallback(() => setAttempt((n) => n + 1), []);
  const entry =
    archive.status === "ready"
      ? (archive.showcase.entries.find((e) => e.run_id === runId) ?? null)
      : null;

  return {
    detail,
    detailError,
    frames: NO_FRAMES,
    summary: detail?.summary ?? null,
    reloadDetail,
    archive,
    entry,
  };
}
