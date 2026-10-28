// SPDX-License-Identifier: Apache-2.0
//
// Client-side pacing of replayed frames, for readability only. A guided run
// finishes in milliseconds and its stream arrives all at once as
// `delivery: replay`; showing those frames one every few hundred
// milliseconds lets a reader follow the three roles. Nothing about the data
// changes: the frames keep their server timestamps and order, the pacing is
// labelled as such in the UI ("simulated · paced for readability"), a control
// skips it, and reduced motion (the OS setting or the in-app preference)
// disables it. Frames published live are never delayed: a live frame flushes
// whatever is queued and goes through at once.

export type PacerOptions = {
  /** Delay between two released items, in milliseconds. */
  intervalMs: number;
  /** False releases everything immediately (reduced motion, or skipped). */
  enabled: boolean;
  setTimeout?: (callback: () => void, ms: number) => unknown;
  clearTimeout?: (handle: unknown) => void;
};

export class Pacer<T> {
  private readonly queue: T[] = [];
  private timer: unknown = null;
  private enabled: boolean;
  private readonly intervalMs: number;
  private readonly release: (item: T) => void;
  private readonly schedule: (callback: () => void, ms: number) => unknown;
  private readonly cancel: (handle: unknown) => void;
  private disposed = false;

  constructor(release: (item: T) => void, options: PacerOptions) {
    this.release = release;
    this.enabled = options.enabled;
    this.intervalMs = Math.max(0, options.intervalMs);
    this.schedule = options.setTimeout ?? ((callback, ms) => globalThis.setTimeout(callback, ms));
    this.cancel = options.clearTimeout ?? ((handle) => globalThis.clearTimeout(handle as number));
  }

  /** Items waiting to be released. */
  get pending(): number {
    return this.queue.length;
  }

  get isEnabled(): boolean {
    return this.enabled;
  }

  /**
   * Queues an item. `paced: false` (a live frame, or anything that must not
   * wait) flushes the queue in order and releases the item at once.
   */
  push(item: T, paced = true): void {
    if (this.disposed) {
      return;
    }
    if (!paced || !this.enabled) {
      this.flush();
      this.release(item);
      return;
    }
    this.queue.push(item);
    if (this.timer === null) {
      // The first item of a burst shows at once; the rest follow in step.
      this.releaseNext();
    }
  }

  /** Releases everything queued, in order, now. */
  flush(): void {
    this.stopTimer();
    while (this.queue.length > 0) {
      const next = this.queue.shift();
      if (next !== undefined) {
        this.release(next);
      }
    }
  }

  /** Stops pacing for good: flushes, and later items go through at once. */
  skip(): void {
    this.enabled = false;
    this.flush();
  }

  /** Reduced motion toggled while streaming: off flushes, on resumes pacing. */
  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (!enabled) {
      this.flush();
    }
  }

  dispose(): void {
    this.disposed = true;
    this.stopTimer();
    this.queue.length = 0;
  }

  private releaseNext(): void {
    this.timer = null;
    const next = this.queue.shift();
    if (next === undefined) {
      // The cooldown after the last release passed with nothing queued; the
      // next item shows at once.
      return;
    }
    this.release(next);
    // Always arm the cooldown, even with an empty queue: an item arriving
    // during it waits its turn instead of showing on top of the last one.
    this.timer = this.schedule(() => this.releaseNext(), this.intervalMs);
  }

  private stopTimer(): void {
    if (this.timer !== null) {
      this.cancel(this.timer);
      this.timer = null;
    }
  }
}
