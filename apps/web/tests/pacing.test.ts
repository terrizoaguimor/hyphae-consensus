// SPDX-License-Identifier: Apache-2.0
//
// The readability pacer: replayed frames are spaced out, live frames and a
// skip flush the queue, reduced motion releases everything at once, and the
// order never changes.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Pacer } from "../src/lib/run/pacing";

describe("Pacer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function pacer(enabled = true) {
    const released: number[] = [];
    const p = new Pacer<number>((n) => released.push(n), { intervalMs: 500, enabled });
    return { p, released };
  }

  it("releases the first item at once and the rest one interval apart", () => {
    const { p, released } = pacer();
    p.push(1);
    p.push(2);
    p.push(3);
    expect(released).toEqual([1]);
    expect(p.pending).toBe(2);
    vi.advanceTimersByTime(499);
    expect(released).toEqual([1]);
    vi.advanceTimersByTime(1);
    expect(released).toEqual([1, 2]);
    vi.advanceTimersByTime(500);
    expect(released).toEqual([1, 2, 3]);
    expect(p.pending).toBe(0);
  });

  it("keeps order when items arrive while paced", () => {
    const { p, released } = pacer();
    p.push(1);
    vi.advanceTimersByTime(200);
    p.push(2);
    p.push(3);
    vi.advanceTimersByTime(300);
    expect(released).toEqual([1, 2]);
    vi.advanceTimersByTime(500);
    expect(released).toEqual([1, 2, 3]);
  });

  it("releases everything immediately when disabled (reduced motion)", () => {
    const { p, released } = pacer(false);
    p.push(1);
    p.push(2);
    expect(released).toEqual([1, 2]);
    expect(p.pending).toBe(0);
  });

  it("an unpaced item (a live frame) flushes the queue first, in order", () => {
    const { p, released } = pacer();
    p.push(1);
    p.push(2);
    p.push(3);
    p.push(4, false);
    expect(released).toEqual([1, 2, 3, 4]);
    expect(p.pending).toBe(0);
    p.push(5);
    expect(released).toEqual([1, 2, 3, 4, 5]);
  });

  it("skip() flushes and stops pacing for the rest of the stream", () => {
    const { p, released } = pacer();
    p.push(1);
    p.push(2);
    p.push(3);
    p.skip();
    expect(released).toEqual([1, 2, 3]);
    expect(p.isEnabled).toBe(false);
    p.push(4);
    expect(released).toEqual([1, 2, 3, 4]);
    vi.advanceTimersByTime(5000);
    expect(released).toEqual([1, 2, 3, 4]);
  });

  it("setEnabled(false) flushes, setEnabled(true) paces again", () => {
    const { p, released } = pacer();
    p.push(1);
    p.push(2);
    p.setEnabled(false);
    expect(released).toEqual([1, 2]);
    p.setEnabled(true);
    p.push(3);
    p.push(4);
    expect(released).toEqual([1, 2, 3]);
    vi.advanceTimersByTime(500);
    expect(released).toEqual([1, 2, 3, 4]);
  });

  it("dispose() drops what is queued and ignores later pushes", () => {
    const { p, released } = pacer();
    p.push(1);
    p.push(2);
    p.dispose();
    vi.advanceTimersByTime(5000);
    p.push(3);
    expect(released).toEqual([1]);
    expect(p.pending).toBe(0);
  });
});
