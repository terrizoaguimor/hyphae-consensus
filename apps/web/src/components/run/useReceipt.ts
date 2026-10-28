"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The receipt of one run (RunReceiptV2), read once per run by the workspace
// and shared by the tabs that need it (Proof inspector, Audit trail): GET
// /runs/{id}/receipt for a run of this server, GET /showcase/{id}/receipt for
// a recorded run of the research archive. It is requested when such a tab is
// first opened (an event, so nothing loads before it is wanted), can be read
// again (a run still deliberating gets a newer receipt), and a failed request
// is kept as an error to show with a retry, never as a loading line. The
// responses of older requests are dropped.

import { useCallback, useRef, useState } from "react";

import { api, ApiError, isApiError } from "@/lib/api/client";
import type { RunReceiptV2 } from "@/lib/api/types";

export type ReceiptState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; receipt: RunReceiptV2 }
  | { status: "error"; error: ApiError };

export type RunReceipt = {
  state: ReceiptState;
  /** Requests the receipt unless it is loaded or loading. */
  ensure: () => void;
  /** Requests it again (a retry, or a newer receipt of a run in progress). */
  reload: () => void;
};

export type RunSourceKind = "this_server" | "research_archive";

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

export function useReceipt(runId: string, source: RunSourceKind): RunReceipt {
  const [state, setState] = useState<ReceiptState>({ status: "idle" });
  const latest = useRef(0);

  const reload = useCallback(() => {
    latest.current += 1;
    const request = latest.current;
    setState({ status: "loading" });
    const path =
      source === "research_archive" ? `/showcase/${runId}/receipt` : `/runs/${runId}/receipt`;
    const read = source === "research_archive" ? api.showcaseReceipt(runId) : api.getReceipt(runId);
    read
      .then((receipt) => {
        if (request === latest.current) {
          setState({ status: "ready", receipt });
        }
      })
      .catch((error: unknown) => {
        if (request === latest.current) {
          setState({ status: "error", error: toApiError(error, path) });
        }
      });
  }, [runId, source]);

  // Requested at most once by ensure(); reload() always asks again.
  const ensure = useCallback(() => {
    if (latest.current === 0) {
      reload();
    }
  }, [reload]);

  return { state, ensure, reload };
}
