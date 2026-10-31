"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The receipt of one run (RunReceiptV2), read by the workspace and shared by
// the tabs that need it (Proof inspector, Audit trail): GET
// /runs/{id}/receipt for a run of this server, GET /showcase/{id}/receipt for
// a recorded run of the research archive. It is requested when such a tab is
// first opened (an event, so nothing loads before it is wanted), and read
// again whenever the run moves on once it has been requested (refresh: the
// server rebuilds the receipt from the run's current records, so a decision
// or a later record makes the copy held here stale), on a retry, and by the
// in-browser chain check, which compares the log with the receipt it read
// itself just before downloading the log (read). A receipt already shown
// stays shown while a newer one is read; a failed request is kept as an
// error to show with a retry, never as a loading line, except a failed read
// of the chain check while a receipt is shown: that receipt stays, and the
// failure goes only to the check, which says it compared no receipt. The
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
  /** A request for the receipt is in flight (the receipt shown, if any, may be older than the run). */
  pending: boolean;
  /** Requests the receipt unless it is loaded or loading. */
  ensure: () => void;
  /** Requests it again (a retry, or a newer receipt of a run in progress). */
  reload: () => void;
  /** Reads it again only if it was requested before (the run moved on). */
  refresh: () => void;
  /** Reads it now and resolves with this very response (also shown, unless a newer request was made); rejects with an ApiError, which replaces only a receipt not yet shown. */
  read: () => Promise<RunReceiptV2>;
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
  // The ticket of the request in flight; 0 once the newest one settled.
  const [inFlight, setInFlight] = useState(0);

  // keepShown: a failure leaves a receipt already shown in place (the chain
  // check's own read); otherwise the error replaces it, with a retry.
  const request = useCallback(
    (keepShown: boolean): Promise<RunReceiptV2> => {
      latest.current += 1;
      const ticket = latest.current;
      setInFlight(ticket);
      // A receipt already shown stays until the newer one arrives.
      setState((current) => (current.status === "ready" ? current : { status: "loading" }));
      const path =
        source === "research_archive" ? `/showcase/${runId}/receipt` : `/runs/${runId}/receipt`;
      const pending = source === "research_archive" ? api.showcaseReceipt(runId) : api.getReceipt(runId);
      return pending.then(
        (receipt) => {
          if (ticket === latest.current) {
            setState({ status: "ready", receipt });
            setInFlight(0);
          }
          return receipt;
        },
        (error: unknown) => {
          const apiError = toApiError(error, path);
          if (ticket === latest.current) {
            setInFlight(0);
            setState((current) =>
              keepShown && current.status === "ready" ? current : { status: "error", error: apiError },
            );
          }
          throw apiError;
        },
      );
    },
    [runId, source],
  );

  const read = useCallback(() => request(true), [request]);

  const reload = useCallback(() => {
    // The outcome is in the state.
    request(false).catch(() => undefined);
  }, [request]);

  // Requested at most once by ensure(); reload() always asks again.
  const ensure = useCallback(() => {
    if (latest.current === 0) {
      reload();
    }
  }, [reload]);

  // Nothing is read before a tab asks for the receipt.
  const refresh = useCallback(() => {
    if (latest.current > 0) {
      reload();
    }
  }, [reload]);

  return { state, pending: inFlight !== 0, ensure, reload, refresh, read };
}
