// SPDX-License-Identifier: Apache-2.0
//
// The wait a 429 asks for (src/lib/api/backoff.ts): only a 429 with a
// positive wait disables the buttons, the countdown is the server's own
// number, and the label is m:ss in every locale.

import { describe, expect, it } from "vitest";

import { backoffSeconds, formatWait, secondsLeft } from "../src/lib/api/backoff";
import { ApiError } from "../src/lib/api/client";

function refusal(status: number, retryAfterSecs: number | null): ApiError {
  return new ApiError({
    code: status === 429 ? "rate_limited" : "internal",
    message: "refused",
    status,
    path: "/runs",
    retryAfterSecs,
  });
}

describe("backoffSeconds", () => {
  it("is the wait of a 429 that names one", () => {
    expect(backoffSeconds(refusal(429, 30))).toBe(30);
    expect(backoffSeconds(refusal(429, 2.2))).toBe(3);
  });

  it("is null without a wait, for a wait of zero, and for any other status", () => {
    expect(backoffSeconds(null)).toBeNull();
    expect(backoffSeconds(refusal(429, null))).toBeNull();
    expect(backoffSeconds(refusal(429, 0))).toBeNull();
    expect(backoffSeconds(refusal(503, 30))).toBeNull();
    expect(backoffSeconds(refusal(500, 30))).toBeNull();
  });
});

describe("formatWait", () => {
  it("prints minutes and zero-padded seconds", () => {
    expect(formatWait(30)).toBe("0:30");
    expect(formatWait(27)).toBe("0:27");
    expect(formatWait(5)).toBe("0:05");
    expect(formatWait(65)).toBe("1:05");
    expect(formatWait(0)).toBe("0:00");
    expect(formatWait(0.4)).toBe("0:01");
  });
});

describe("secondsLeft", () => {
  it("counts down in whole seconds and ends at the deadline", () => {
    expect(secondsLeft(null, 0)).toBeNull();
    expect(secondsLeft(30_000, 0)).toBe(30);
    expect(secondsLeft(30_000, 2_500)).toBe(28);
    expect(secondsLeft(30_000, 29_001)).toBe(1);
    expect(secondsLeft(30_000, 30_000)).toBeNull();
    expect(secondsLeft(30_000, 31_000)).toBeNull();
  });
});
