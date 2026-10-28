// SPDX-License-Identifier: Apache-2.0
//
// A mocked /api/v1 over the contract fixtures, installed with page.route():
// sessions, scenarios, capabilities (with or without the live section),
// health, the run list, one run's detail, its SSE stream (the frames fixture
// as one text/event-stream body), the decision and the cancel. Every call is
// recorded so a spec can assert headers and bodies. The variants derived here
// (a run awaiting its decision, a live-model run replayed, a run whose
// provenance is inconsistent) are built from the fixtures, never typed in.

import type { Page, Request } from "@playwright/test";

import { fixture } from "./contract";

type Json = Record<string, unknown>;

export type RecordedCall = {
  method: string;
  path: string;
  headers: Record<string, string>;
  body: unknown;
};

export type ApiMockOptions = {
  /** GET /session answers 200 (true) or 401 session_required (false). */
  session?: boolean;
  liveEnabled?: boolean;
  livePresets?: Array<{ id: string; title?: string; description?: string }>;
  liveBudget?: Json;
  /** What POST /live-runs answers; default 202 with the created run. */
  liveRun?: { status: number; body: Json; retryAfter?: string };
  /** What POST /runs answers; default 202 with the created run. */
  guidedRun?: { status: number; body: Json; retryAfter?: string };
  detail?: Json;
  /** The detail served once a decision was recorded; default the retry fixture. */
  decidedDetail?: Json;
  frames?: Json[];
  runs?: Json[];
  decision?: { status: number; body: Json };
};

const RUN_ID = "run-00000000-0000-4000-8000-000000000001";

export const PUBLIC_DEMO_NOTICE =
  "This decision carries no operational authority: it records a visitor's choice on a simulated run.";

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** The retry run as it stands before the human decision (events 1 to 14). */
export function awaitingDecisionDetail(): Json {
  const detail = clone(fixture<Json>("run-detail-retry.json"));
  const events = (detail.events as Json[]).filter((e) => (e.sequence as number) <= 14);
  detail.events = events;
  delete detail.decision;
  const summary = detail.summary as Json;
  summary.state = "awaiting_human_approval";
  summary.authorization = "pending";
  summary.last_sequence = 14;
  summary.event_count = 14;
  const chain = detail.chain as Json;
  chain.last_sequence = 14;
  chain.head_hash = events[events.length - 1]?.hash;
  return detail;
}

/** The same run with a live model axis, replayed: must read REPLAY, never LIVE. */
export function liveModelDetail(flags: string[] = []): Json {
  const detail = awaitingDecisionDetail();
  const provenance = (detail.summary as Json).provenance as Json;
  provenance.model = "live_meta";
  provenance.evidence = "hyphae_snapshot";
  provenance.flags = flags;
  provenance.meta_request_ids = ["chatcmpl-fixture-1"];
  return detail;
}

/** The frames fixture with every summary's provenance replaced and a delivery set. */
export function framesWith(provenance: Json, delivery: "live" | "replay"): Json[] {
  return clone(fixture<Json[]>("sse-frames-retry.json")).map((frame) => {
    if (frame.frame !== "consensus") {
      return frame;
    }
    const summary = frame.summary as Json;
    summary.provenance = { ...(summary.provenance as Json), ...provenance };
    return { ...frame, delivery };
  });
}

/** One text/event-stream body: `id:` is the record sequence, as the server sends it. */
export function sseBody(frames: Json[]): string {
  let out = ": stream-start\nretry: 3000\n\n";
  for (const frame of frames) {
    const { frame: kind, ...data } = frame;
    if (kind === "consensus") {
      const sequence = (data.event as Json).sequence as number;
      out += `id: ${sequence}\nevent: consensus\ndata: ${JSON.stringify(data)}\n\n`;
    } else {
      out += `event: ${String(kind)}\ndata: ${JSON.stringify(data)}\n\n`;
    }
  }
  return out;
}

function decisionResponse(approved: boolean, note: unknown): Json {
  const run = clone(fixture<Json>("run-summary-retry.json"));
  run.state = approved ? "approved" : "rejected";
  run.authorization = approved ? "approved" : "rejected";
  return {
    run,
    decision: {
      approved,
      authority: "public_demo",
      authorized_by: "visitor:fixture0000000",
      decided_at: "2026-10-26T09:00:00Z",
      note: typeof note === "string" ? note : null,
      notice: PUBLIC_DEMO_NOTICE,
    },
  };
}

export async function mockApi(page: Page, options: ApiMockOptions = {}) {
  const calls: RecordedCall[] = [];
  const state = { decided: false, session: options.session ?? true };
  const capabilities = clone(fixture<Json>("capabilities-public.json"));
  if (options.liveEnabled === true) {
    capabilities.live_enabled = true;
    capabilities.live = {
      presets: options.livePresets ?? [
        {
          id: "retry-cap-3",
          title: "Retry budget (live)",
          description: "The Proposer raises the retry limit against a sealed snapshot.",
        },
      ],
      budget: options.liveBudget ?? { daily_cap_usd: 10, remaining_usd: 7.5 },
    };
  }
  const created = fixture<Json>("run-created-retry.json");
  const detail = options.detail ?? fixture<Json>("run-detail-retry.json");
  const decidedDetail = options.decidedDetail ?? fixture<Json>("run-detail-retry.json");
  const frames = options.frames ?? fixture<Json[]>("sse-frames-retry.json");
  const runs = options.runs ?? [fixture<Json>("run-summary-retry.json")];

  const json = (status: number, body: unknown, headers: Record<string, string> = {}) => ({
    status,
    contentType: "application/json",
    headers: { "cache-control": "no-store", ...headers },
    body: JSON.stringify(body),
  });
  const error = (status: number, code: string, message: string, retryAfter?: number) =>
    json(
      status,
      retryAfter === undefined ? { code, message } : { code, message, retry_after_secs: retryAfter },
      retryAfter === undefined ? {} : { "retry-after": String(retryAfter) },
    );

  const record = (request: Request) => {
    const url = new URL(request.url());
    let body: unknown = null;
    const raw = request.postData();
    if (raw !== null) {
      try {
        body = JSON.parse(raw);
      } catch {
        body = raw;
      }
    }
    calls.push({ method: request.method(), path: url.pathname + url.search, headers: request.headers(), body });
    return url;
  };

  await page.route("**/api/v1/**", async (route, request) => {
    const url = record(request);
    const method = request.method();
    const path = url.pathname;

    if (method === "GET" && path === "/api/v1/health") {
      return route.fulfill(json(200, fixture("health.json")));
    }
    if (method === "GET" && path === "/api/v1/capabilities") {
      return route.fulfill(json(200, capabilities));
    }
    if (method === "GET" && path === "/api/v1/scenarios") {
      return route.fulfill(json(200, fixture("scenarios.json")));
    }
    if (path === "/api/v1/session") {
      if (method === "GET") {
        return state.session
          ? route.fulfill(json(200, { ...fixture<Json>("session.json"), created: false }))
          : route.fulfill(error(401, "session_required", "a session is required: POST /api/v1/session first"));
      }
      if (method === "POST") {
        state.session = true;
        return route.fulfill(json(200, fixture("session.json")));
      }
    }
    if (method === "POST" && path === "/api/v1/runs") {
      const answer = options.guidedRun ?? { status: 202, body: created };
      return route.fulfill(
        json(answer.status, answer.body, answer.retryAfter === undefined ? {} : { "retry-after": answer.retryAfter }),
      );
    }
    if (method === "POST" && path === "/api/v1/live-runs") {
      const answer =
        options.liveRun ??
        (capabilities.live_enabled === true
          ? { status: 202, body: created }
          : { status: 503, body: { code: "live_disabled", message: "the public live mode is not enabled on this server" } });
      return route.fulfill(
        json(answer.status, answer.body, answer.retryAfter === undefined ? {} : { "retry-after": answer.retryAfter }),
      );
    }
    if (method === "GET" && path === "/api/v1/runs") {
      return route.fulfill(json(200, { runs, next_cursor: null }));
    }
    if (method === "GET" && path === `/api/v1/runs/${RUN_ID}`) {
      return route.fulfill(json(200, state.decided ? decidedDetail : detail));
    }
    if (method === "GET" && path === `/api/v1/runs/${RUN_ID}/events`) {
      return route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        headers: { "cache-control": "no-store" },
        body: sseBody(frames),
      });
    }
    if (method === "GET" && path === `/api/v1/runs/${RUN_ID}/receipt`) {
      return route.fulfill(json(200, fixture("receipt-v2-retry.json")));
    }
    if (method === "POST" && path === `/api/v1/runs/${RUN_ID}/decision`) {
      if (options.decision !== undefined) {
        return route.fulfill(json(options.decision.status, options.decision.body));
      }
      const body = calls[calls.length - 1]?.body as Json | null;
      state.decided = true;
      return route.fulfill(json(200, decisionResponse(body?.approved === true, body?.note)));
    }
    if (method === "POST" && path === `/api/v1/runs/${RUN_ID}/cancel`) {
      return route.fulfill(json(202, { run_id: RUN_ID, cancelling: true }));
    }
    return route.fulfill(error(404, "not_found", "not found"));
  });

  return { calls, runId: RUN_ID };
}
