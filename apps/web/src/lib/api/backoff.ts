"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The wait a 429 asks for. When the server refuses a run's creation with
// HTTP 429 and says how long to wait (Retry-After, or retry_after_secs in the
// body: ApiError.retryAfterSecs), the buttons that would send the same
// request stay disabled until the wait is over and say how long is left
// ("Try again in 0:27"); the countdown is the server's own number, counted
// down in this browser, never a guess. A 429 without a wait, and every other
// refusal, leaves the buttons as they are.

import { useCallback, useEffect, useState } from "react";

import type { ApiError } from "./client";

/** The seconds a refusal asks to wait before the same request, or null. */
export function backoffSeconds(error: ApiError | null): number | null {
  if (error === null || error.status !== 429) {
    return null;
  }
  const seconds = error.retryAfterSecs;
  return seconds === null || !Number.isFinite(seconds) || seconds <= 0 ? null : Math.ceil(seconds);
}

/** "0:27", "1:05": minutes and zero-padded seconds, the same in every locale. */
export function formatWait(seconds: number): string {
  const whole = Math.max(0, Math.ceil(seconds));
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  return `${minutes}:${String(rest).padStart(2, "0")}`;
}

/** Whole seconds left until `until` (epoch ms) at `now`, or null once it passed. */
export function secondsLeft(until: number | null, now: number): number | null {
  if (until === null) {
    return null;
  }
  const left = Math.ceil((until - now) / 1000);
  return left > 0 ? left : null;
}

/**
 * A countdown started from a refusal: `remaining` is the whole seconds left
 * (null when there is no wait), `start(error)` begins the wait the error asks
 * for (and does nothing for an error that asks for none).
 */
export function useBackoff(): { remaining: number | null; start: (error: ApiError | null) => void } {
  const [until, setUntil] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (until === null) {
      return;
    }
    const tick = () => {
      const at = Date.now();
      setNow(at);
      if (at >= until) {
        setUntil(null);
      }
    };
    const timer = window.setInterval(tick, 250);
    return () => window.clearInterval(timer);
  }, [until]);

  const start = useCallback((error: ApiError | null) => {
    const seconds = backoffSeconds(error);
    if (seconds !== null) {
      const at = Date.now();
      setNow(at);
      setUntil(at + seconds * 1000);
    }
  }, []);

  return { remaining: secondsLeft(until, now), start };
}
