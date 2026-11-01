// SPDX-License-Identifier: Apache-2.0
//
// One Cloudflare Turnstile widget for the live form, over the explicit
// rendering API (`window.turnstile`, loaded at runtime by
// src/components/new/Turnstile.tsx only when the build has a site key).
// Unit-tested with a scripted API in tests/turnstile.test.ts.
//
// - The widget is rendered with the action the server verifies
//   (`CONSENSUS_TURNSTILE_ACTION_LIVE`, default `live-run`,
//   crates/consensus-api/src/live/turnstile.rs): a token minted for any
//   other action is refused (403 turnstile_failed).
// - A token is single-use: the live gate spends it at Siteverify (with a
//   fresh idempotency key) before it reserves the budget, so one solved
//   challenge admits at most one run, whatever the answer. `spend()` hands
//   the token to exactly one request and asks the widget for a new
//   challenge at once; the form never sends a token twice.
// - Status: "loading" only until the widget is on the page; "ready" while a
//   rendered challenge waits for the person (after render, reset or expiry,
//   and when it enters interactive mode); "passed" with a token; "failed"
//   on an error or timeout. Without "ready" the form kept saying the
//   challenge was loading while it showed the 'Verify you are human' box.

/** The widget action the server expects for a live-run token. */
export const TURNSTILE_ACTION_LIVE = "live-run";

export type TurnstileRenderOptions = {
  sitekey: string;
  action: string;
  callback: (token: string) => void;
  "before-interactive-callback"?: () => void;
  "expired-callback"?: () => void;
  "error-callback"?: () => void;
  "timeout-callback"?: () => void;
};

/** The part of Cloudflare's `window.turnstile` this module uses. */
export type TurnstileApi = {
  render: (container: HTMLElement, options: TurnstileRenderOptions) => string;
  reset: (widgetId: string) => void;
  remove: (widgetId: string) => void;
};

/** Every widget status, in the order a visitor meets them. */
export const TURNSTILE_STATUSES = ["loading", "ready", "passed", "failed"] as const;

export type TurnstileStatus = (typeof TURNSTILE_STATUSES)[number];

/** The `NewDeliberation.live` message that describes `status` under the widget. */
export function turnstileStatusMessage(
  status: TurnstileStatus,
): "turnstileLoading" | "turnstileReady" | "turnstilePassed" | "turnstileFailed" {
  switch (status) {
    case "loading":
      return "turnstileLoading";
    case "ready":
      return "turnstileReady";
    case "passed":
      return "turnstilePassed";
    case "failed":
      return "turnstileFailed";
  }
}

export type TurnstileListener = {
  /** The current token, or null when there is none (expired, failed, spent). */
  onToken: (token: string | null) => void;
  onStatus: (status: TurnstileStatus) => void;
};

export class TurnstileWidget {
  private widgetId: string | null = null;
  private token: string | null = null;
  private status: TurnstileStatus = "loading";

  constructor(
    private readonly api: TurnstileApi,
    private readonly listener: TurnstileListener,
  ) {}

  /** Renders the widget into `container` for `siteKey`, with the live-run action. */
  render(container: HTMLElement, siteKey: string): void {
    this.widgetId = this.api.render(container, {
      sitekey: siteKey,
      action: TURNSTILE_ACTION_LIVE,
      callback: (token) => {
        this.setToken(token);
        this.setStatus("passed");
      },
      "before-interactive-callback": () => {
        this.setStatus("ready");
      },
      "expired-callback": () => {
        // Turnstile refreshes an expired widget on its own, in place.
        this.setToken(null);
        this.setStatus("ready");
      },
      "error-callback": () => {
        this.setToken(null);
        this.setStatus("failed");
      },
      "timeout-callback": () => {
        this.setToken(null);
        this.setStatus("failed");
      },
    });
    // The widget is on the page and waits for the person, unless a callback
    // already settled it during render (a token, an error or a timeout).
    if (this.status === "loading") {
      this.setStatus("ready");
    }
  }

  /** The token a request may carry now, or null. */
  get current(): string | null {
    return this.token;
  }

  /**
   * Hands the current token to one request and forgets it, then asks for a
   * fresh challenge: the server consumes a token whatever it answers.
   */
  spend(): string | null {
    const token = this.token;
    this.reset();
    return token;
  }

  /** Forgets the token and asks Cloudflare for a new challenge. */
  reset(): void {
    this.setToken(null);
    if (this.widgetId !== null) {
      this.status = "loading";
      this.api.reset(this.widgetId);
      // The fresh challenge is on the page again, waiting for the person,
      // unless a callback already settled it during the reset.
      if (this.status === "loading") {
        this.setStatus("ready");
      }
    }
  }

  remove(): void {
    if (this.widgetId !== null) {
      this.api.remove(this.widgetId);
      this.widgetId = null;
    }
    this.token = null;
  }

  private setStatus(status: TurnstileStatus): void {
    this.status = status;
    this.listener.onStatus(status);
  }

  private setToken(token: string | null): void {
    this.token = token;
    this.listener.onToken(token);
  }
}
