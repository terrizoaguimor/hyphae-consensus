"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Runtime connectivity: GET /api/v1/health once per page load, shared by
// every view through a module-level store (useSyncExternalStore). The static
// HTML and the hydrating render say "pending" (the server snapshot); the
// answer arrives once the probe settles. There is no build-time flag any
// more: whether the API is there is a fact of the server the export is served
// from, found out at runtime.

import { useCallback, useEffect, useSyncExternalStore } from "react";

import { api, ApiError, isApiError } from "./client";
import type { HealthV1 } from "./types";

export type ApiHealth =
  | { status: "pending" }
  | { status: "ok"; health: HealthV1 }
  | { status: "unreachable"; error: ApiError };

type Settled = Exclude<ApiHealth, { status: "pending" }>;

const PENDING: ApiHealth = { status: "pending" };

let inflight: Promise<Settled> | null = null;
let settled: Settled | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) {
    listener();
  }
}

function probe(): Promise<Settled> {
  if (inflight === null) {
    const attempt: Promise<Settled> = api
      .health()
      .then<Settled>((health) => ({ status: "ok", health }))
      .catch<Settled>((error: unknown) => ({
        status: "unreachable",
        error: isApiError(error)
          ? error
          : new ApiError({
              code: "unreachable",
              message: error instanceof Error ? error.message : "health check failed",
              status: 0,
              path: "/health",
            }),
      }))
      .then((result) => {
        // A reset while this probe was in flight supersedes it.
        if (inflight === attempt) {
          settled = result;
          notify();
        }
        return result;
      });
    inflight = attempt;
  }
  return inflight;
}

/** Forgets the cached answer so the next mount (or retry) probes again. */
export function resetApiHealth(): void {
  inflight = null;
  settled = null;
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): ApiHealth {
  return settled ?? PENDING;
}

function getServerSnapshot(): ApiHealth {
  return PENDING;
}

/** The cached health probe; `retry()` probes again after a failure. */
export function useApiHealth(): ApiHealth & { retry: () => void } {
  const state = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  useEffect(() => {
    if (settled === null) {
      void probe();
    }
  }, []);

  const retry = useCallback(() => {
    resetApiHealth();
    void probe();
  }, []);

  return { ...state, retry };
}
