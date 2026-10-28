// SPDX-License-Identifier: Apache-2.0
//
// The Turnstile widget (src/lib/live/turnstile.ts) over a scripted
// `window.turnstile`: it is rendered with the action the server verifies,
// a solved challenge yields one token, and spending it (every submit) hands
// it to exactly one request, forgets it and resets the widget, because the
// live gate consumes a token at Siteverify whatever it answers.

import { describe, expect, it } from "vitest";

import {
  TURNSTILE_ACTION_LIVE,
  type TurnstileApi,
  type TurnstileRenderOptions,
  type TurnstileStatus,
  TurnstileWidget,
} from "../src/lib/live/turnstile";

function scriptedApi() {
  const calls: string[] = [];
  let options: TurnstileRenderOptions | null = null;
  const api: TurnstileApi = {
    render: (_container, given) => {
      options = given;
      calls.push("render");
      return "widget-1";
    },
    reset: (id) => {
      calls.push(`reset:${id}`);
    },
    remove: (id) => {
      calls.push(`remove:${id}`);
    },
  };
  return {
    api,
    calls,
    options: () => {
      if (options === null) {
        throw new Error("not rendered");
      }
      return options;
    },
  };
}

function widget() {
  const scripted = scriptedApi();
  const tokens: Array<string | null> = [];
  const statuses: TurnstileStatus[] = [];
  const w = new TurnstileWidget(scripted.api, {
    onToken: (token) => tokens.push(token),
    onStatus: (status) => statuses.push(status),
  });
  // The container is only handed to the scripted API, which ignores it.
  w.render({} as HTMLElement, "site-key");
  return { w, ...scripted, tokens, statuses };
}

describe("TurnstileWidget", () => {
  it("renders with the site key and the live-run action the server verifies", () => {
    const { options } = widget();
    expect(options().sitekey).toBe("site-key");
    expect(options().action).toBe(TURNSTILE_ACTION_LIVE);
    expect(TURNSTILE_ACTION_LIVE).toBe("live-run");
  });

  it("a solved challenge yields a token", () => {
    const { w, options, tokens, statuses } = widget();
    expect(w.current).toBeNull();
    options().callback("tok-1");
    expect(w.current).toBe("tok-1");
    expect(tokens).toEqual(["tok-1"]);
    expect(statuses).toEqual(["passed"]);
  });

  it("spend() hands the token to one request, forgets it and resets the widget", () => {
    const { w, options, calls, tokens, statuses } = widget();
    options().callback("tok-1");
    expect(w.spend()).toBe("tok-1");
    expect(w.current).toBeNull();
    expect(calls).toEqual(["render", "reset:widget-1"]);
    expect(tokens.at(-1)).toBeNull();
    expect(statuses.at(-1)).toBe("loading");
    // The same token is never handed out twice; a second submit waits.
    expect(w.spend()).toBeNull();
    options().callback("tok-2");
    expect(w.spend()).toBe("tok-2");
  });

  it("an expired, failed or timed-out challenge leaves no token", () => {
    const { w, options, statuses } = widget();
    options().callback("tok-1");
    options()["expired-callback"]?.();
    expect(w.current).toBeNull();
    expect(statuses.at(-1)).toBe("loading");
    options().callback("tok-2");
    options()["error-callback"]?.();
    expect(w.current).toBeNull();
    expect(statuses.at(-1)).toBe("failed");
    options().callback("tok-3");
    options()["timeout-callback"]?.();
    expect(w.current).toBeNull();
  });

  it("remove() removes the widget once", () => {
    const { w, calls } = widget();
    w.remove();
    w.remove();
    expect(calls).toEqual(["render", "remove:widget-1"]);
  });
});
