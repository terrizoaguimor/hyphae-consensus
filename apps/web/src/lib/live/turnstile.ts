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

/** The widget action the server expects for a live-run token. */
export const TURNSTILE_ACTION_LIVE = "live-run";

export type TurnstileRenderOptions = {
  sitekey: string;
  action: string;
  callback: (token: string) => void;
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

export type TurnstileStatus = "loading" | "passed" | "failed";

export type TurnstileListener = {
  /** The current token, or null when there is none (expired, failed, spent). */
  onToken: (token: string | null) => void;
  onStatus: (status: TurnstileStatus) => void;
};

export class TurnstileWidget {
  private widgetId: string | null = null;
  private token: string | null = null;

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
        this.listener.onStatus("passed");
      },
      "expired-callback": () => {
        // Turnstile refreshes an expired widget on its own.
        this.setToken(null);
        this.listener.onStatus("loading");
      },
      "error-callback": () => {
        this.setToken(null);
        this.listener.onStatus("failed");
      },
      "timeout-callback": () => {
        this.setToken(null);
        this.listener.onStatus("failed");
      },
    });
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
      this.listener.onStatus("loading");
      this.api.reset(this.widgetId);
    }
  }

  remove(): void {
    if (this.widgetId !== null) {
      this.api.remove(this.widgetId);
      this.widgetId = null;
    }
    this.token = null;
  }

  private setToken(token: string | null): void {
    this.token = token;
    this.listener.onToken(token);
  }
}
