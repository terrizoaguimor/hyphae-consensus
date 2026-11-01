// SPDX-License-Identifier: Apache-2.0
//
// The fetch wrapper: JSON headers, same-origin credentials, idempotency keys,
// the Turnstile header, typed errors (contract body, Retry-After, unreachable,
// non-contract responses), ensureSession as one POST and never a GET (round
// 8, C7: no console 401 for a visitor without a session), and the session
// flag My runs reads. The fixtures of contracts/api-v1 stand in for the server.

import { readFileSync } from "node:fs";
import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import {
  ApiError,
  createApiClient,
  hasSessionFlag,
  isRunId,
  JUDGE_CODE_MAX_CHARS,
  judgeCodeForRequest,
  newIdempotencyKey,
  parseRetryAfter,
  SESSION_FLAG_KEY,
  TURNSTILE_HEADER,
} from "../src/lib/api/client";

const fixturesDir = path.join(contractDir(), "fixtures");
const fixtureText = (name: string) =>
  readFileSync(path.join(fixturesDir, name), "utf8");

type Call = { url: string; init: RequestInit };

/** A scripted server: answers in order, records every call. */
function server(
  answers: Array<{ status?: number; body?: string; headers?: Record<string, string> } | Error>,
) {
  const calls: Call[] = [];
  const fetch = async (url: string, init: RequestInit): Promise<Response> => {
    calls.push({ url, init });
    const answer = answers.shift();
    if (answer === undefined) {
      throw new Error(`unexpected call ${url}`);
    }
    if (answer instanceof Error) {
      throw answer;
    }
    return new Response(answer.body ?? "", {
      status: answer.status ?? 200,
      headers: { "content-type": "application/json", ...answer.headers },
    });
  };
  return { calls, client: createApiClient({ fetch }) };
}

const headerOf = (call: Call, name: string): string | undefined =>
  (call.init.headers as Record<string, string>)[name];

describe("requests", () => {
  it("GETs with Accept JSON and same-origin credentials, no body headers", async () => {
    const { calls, client } = server([{ body: fixtureText("health.json") }]);
    const health = await client.health();
    expect(health.contract_sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(calls[0]?.url).toBe("/api/v1/health");
    expect(calls[0]?.init.method).toBe("GET");
    expect(calls[0]?.init.credentials).toBe("same-origin");
    expect(headerOf(calls[0] as Call, "Accept")).toBe("application/json");
    expect(headerOf(calls[0] as Call, "Content-Type")).toBeUndefined();
    expect(calls[0]?.init.body).toBeUndefined();
  });

  it("POSTs JSON with Content-Type and a fresh Idempotency-Key", async () => {
    const { calls, client } = server([{ status: 202, body: fixtureText("run-created-retry.json") }]);
    const created = await client.createGuidedRun("retry", "key-1");
    expect(created.run.run_id).toBe("run-00000000-0000-4000-8000-000000000001");
    const call = calls[0] as Call;
    expect(call.url).toBe("/api/v1/runs");
    expect(call.init.method).toBe("POST");
    expect(headerOf(call, "Content-Type")).toBe("application/json");
    expect(headerOf(call, "Idempotency-Key")).toBe("key-1");
    expect(JSON.parse(String(call.init.body))).toEqual({ scenario: "retry" });
  });

  it("sends the Turnstile token as a header and the gate fields in the body", async () => {
    const { calls, client } = server([{ status: 202, body: fixtureText("run-created-retry.json") }]);
    await client.createLiveRun(
      "retry-cap-3",
      { turnstileToken: "tok", attestAdult: true, judgeCode: "JUDGE" },
      "key-2",
    );
    const call = calls[0] as Call;
    expect(call.url).toBe("/api/v1/live-runs");
    expect(headerOf(call, TURNSTILE_HEADER)).toBe("tok");
    expect(JSON.parse(String(call.init.body))).toEqual({
      preset_id: "retry-cap-3",
      attest_adult: true,
      judge_code: "JUDGE",
    });
  });

  it("omits an empty judge code and a missing Turnstile token", async () => {
    const { calls, client } = server([{ status: 202, body: fixtureText("run-created-retry.json") }]);
    await client.createLiveRun("p", { turnstileToken: null, attestAdult: false }, "k");
    const call = calls[0] as Call;
    expect(headerOf(call, TURNSTILE_HEADER)).toBeUndefined();
    expect(JSON.parse(String(call.init.body))).toEqual({ preset_id: "p", attest_adult: false });
  });

  it("sends the judge code trimmed, and only when 1 to 256 characters remain", async () => {
    const created = { status: 202, body: fixtureText("run-created-live-R01-reference.json") };
    const { calls, client } = server([created, created, created, created]);
    const send = (judgeCode: string) =>
      client.createLiveRun(
        "R01-reference",
        { turnstileToken: "tok", attestAdult: true, judgeCode },
        newIdempotencyKey(),
      );
    await send("  alpha-7231 \t");
    await send("   ");
    await send("x".repeat(JUDGE_CODE_MAX_CHARS + 1));
    await send(` ${"x".repeat(JUDGE_CODE_MAX_CHARS)} `);
    const bodies = calls.map((call) => JSON.parse(String(call.init.body)) as Record<string, unknown>);
    expect(bodies[0]?.judge_code).toBe("alpha-7231");
    expect(bodies[1]).not.toHaveProperty("judge_code");
    expect(bodies[2]).not.toHaveProperty("judge_code");
    expect(bodies[3]?.judge_code).toBe("x".repeat(JUDGE_CODE_MAX_CHARS));
    // Every request carries its own Idempotency-Key.
    expect(new Set(calls.map((call) => headerOf(call, "Idempotency-Key"))).size).toBe(4);
  });

  it("builds the decision body with the expected block, note only when given", async () => {
    const summary = JSON.parse(fixtureText("run-summary-retry.json")) as Record<string, unknown>;
    const decision = {
      approved: true,
      authority: "public_demo",
      authorized_by: "visitor:x",
      decided_at: "2026-10-26T09:00:00Z",
      notice: "no authority",
    };
    const { calls, client } = server([
      { body: JSON.stringify({ run: summary, decision }) },
      { body: JSON.stringify({ run: summary, decision }) },
    ]);
    await client.decide(
      "run-00000000-0000-4000-8000-000000000001",
      { approved: true, note: "  ", expected: { recommendation: "supported", lastSequence: 14 } },
      "d-1",
    );
    expect(JSON.parse(String(calls[0]?.init.body))).toEqual({
      approved: true,
      expected: { recommendation: "supported", last_sequence: 14 },
    });
    expect(headerOf(calls[0] as Call, "Idempotency-Key")).toBe("d-1");
    const response = await client.decide(
      "run-00000000-0000-4000-8000-000000000001",
      { approved: false, note: "no", expected: { recommendation: "supported", lastSequence: 14 } },
      "d-2",
    );
    expect(JSON.parse(String(calls[1]?.init.body)).note).toBe("no");
    expect(response.decision.notice).toBe("no authority");
  });

  it("encodes run ids into paths and builds the stream and log URLs", async () => {
    const { calls, client } = server([{ body: fixtureText("run-detail-retry.json") }]);
    await client.getRun("run-00000000-0000-4000-8000-000000000001");
    expect(calls[0]?.url).toBe("/api/v1/runs/run-00000000-0000-4000-8000-000000000001");
    expect(client.eventsUrl("run-x", 14)).toBe("/api/v1/runs/run-x/events?after=14");
    expect(client.eventsUrl("run-x")).toBe("/api/v1/runs/run-x/events");
    expect(client.ndjsonUrl("a/b")).toBe("/api/v1/runs/a%2Fb/events.ndjson");
    expect(client.receiptUrl("run-x")).toBe("/api/v1/runs/run-x/receipt");
  });

  it("lists runs with cursor pagination", async () => {
    const list = { runs: [JSON.parse(fixtureText("run-summary-retry.json"))], next_cursor: null };
    const { calls, client } = server([{ body: JSON.stringify(list) }, { body: JSON.stringify(list) }]);
    await client.listRuns();
    await client.listRuns({ cursor: "c1", limit: 10 });
    expect(calls[0]?.url).toBe("/api/v1/runs");
    expect(calls[1]?.url).toBe("/api/v1/runs?cursor=c1&limit=10");
  });
});

describe("errors", () => {
  it("parses the contract error body into an ApiError with status", async () => {
    const { client } = server([
      { status: 503, body: JSON.stringify({ code: "live_disabled", message: "not enabled" }) },
    ]);
    const error = await client
      .createLiveRun("p", { turnstileToken: null, attestAdult: true }, "k")
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    const apiError = error as ApiError;
    expect(apiError.code).toBe("live_disabled");
    expect(apiError.status).toBe(503);
    expect(apiError.message).toBe("not enabled");
    expect(apiError.retryAfterSecs).toBeNull();
    expect(apiError.path).toBe("/live-runs");
  });

  it("takes retry_after_secs from the body, else the Retry-After header", async () => {
    const { client } = server([
      {
        status: 429,
        body: JSON.stringify({ code: "rate_limited", message: "slow down", retry_after_secs: 30 }),
        headers: { "Retry-After": "45" },
      },
      {
        status: 503,
        body: JSON.stringify({ code: "too_many_runs", message: "busy" }),
        headers: { "Retry-After": "5" },
      },
    ]);
    const first = (await client.health().catch((e: unknown) => e)) as ApiError;
    expect(first.retryAfterSecs).toBe(30);
    const second = (await client.health().catch((e: unknown) => e)) as ApiError;
    expect(second.retryAfterSecs).toBe(5);
  });

  it("reports a non-contract error status as unexpected_status", async () => {
    const { client } = server([{ status: 502, body: "<html>bad gateway</html>", headers: { "content-type": "text/html" } }]);
    const error = (await client.health().catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe("unexpected_status");
    expect(error.status).toBe(502);
  });

  it("reports a 200 whose body fails the contract as bad_response", async () => {
    const { client } = server([{ body: JSON.stringify({ status: "ok" }) }]);
    const error = (await client.health().catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe("bad_response");
    expect(error.status).toBe(200);
    expect(error.message).toContain("version");
  });

  it("reports a network failure as unreachable with status 0, never a raw string", async () => {
    const { client } = server([new TypeError("Failed to fetch")]);
    const error = (await client.health().catch((e: unknown) => e)) as ApiError;
    expect(error).toBeInstanceOf(ApiError);
    expect(error.code).toBe("unreachable");
    expect(error.status).toBe(0);
  });

  it("parseRetryAfter reads seconds only", () => {
    expect(parseRetryAfter("30")).toBe(30);
    expect(parseRetryAfter(" 2.5 ")).toBe(3);
    expect(parseRetryAfter("Wed, 21 Oct 2026 07:28:00 GMT")).toBeNull();
    expect(parseRetryAfter(null)).toBeNull();
  });
});

describe("ensureSession", () => {
  // POST /session keeps the session a valid cookie names (200, created
  // false, no new cookie) and mints one otherwise (201): one POST either
  // way, and never a GET first, so a visitor without a session sees no 401.
  it("keeps an existing session with exactly one POST and no GET", async () => {
    const existing = JSON.stringify({ ...JSON.parse(fixtureText("session.json")), created: false });
    const { calls, client } = server([{ body: existing }]);
    const session = await client.ensureSession();
    expect(session.pseudonym).toMatch(/^visitor:/);
    expect(session.created).toBe(false);
    expect(calls.map((c) => c.init.method)).toEqual(["POST"]);
    expect(calls[0]?.url).toBe("/api/v1/session");
    expect(headerOf(calls[0] as Call, "Content-Type")).toBeUndefined();
  });

  it("mints a session with exactly one POST and no GET when there is none", async () => {
    const { calls, client } = server([{ status: 201, body: fixtureText("session.json") }]);
    const session = await client.ensureSession();
    expect(session.created).toBe(true);
    expect(calls.map((c) => c.init.method)).toEqual(["POST"]);
  });

  it("propagates a refused POST instead of retrying it", async () => {
    const { calls, client } = server([
      { status: 429, body: JSON.stringify({ code: "rate_limited", message: "x", retry_after_secs: 10 }) },
    ]);
    const error = (await client.ensureSession().catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe("rate_limited");
    expect(calls).toHaveLength(1);
  });
});

describe("the session flag", () => {
  function withStorage(): Map<string, string> {
    const items = new Map<string, string>();
    const storage = {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => void items.set(key, value),
      removeItem: (key: string) => void items.delete(key),
    };
    vi.stubGlobal("window", { localStorage: storage });
    return items;
  }

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("is set by a session the server confirms and cleared by a 401 or a DELETE", async () => {
    const items = withStorage();
    expect(hasSessionFlag()).toBe(false);
    const { client } = server([
      { body: fixtureText("session.json") },
      { status: 401, body: JSON.stringify({ code: "session_required", message: "a session is required" }) },
      { body: fixtureText("session.json") },
      { status: 200, body: "" },
    ]);
    await client.ensureSession();
    expect(items.get(SESSION_FLAG_KEY)).toBe("1");
    expect(hasSessionFlag()).toBe(true);
    expect(await client.getSession()).toBeNull();
    expect(hasSessionFlag()).toBe(false);
    expect(await client.getSession()).not.toBeNull();
    expect(hasSessionFlag()).toBe(true);
    await client.deleteSession();
    expect(hasSessionFlag()).toBe(false);
    expect(SESSION_FLAG_KEY).toBe("hc.session");
  });

  it("is only a hint: without storage there is no flag and nothing throws", async () => {
    expect(hasSessionFlag()).toBe(false);
    const { client } = server([{ body: fixtureText("session.json") }]);
    await expect(client.ensureSession()).resolves.toMatchObject({ created: true });
  });
});

describe("helpers", () => {
  it("newIdempotencyKey yields distinct UUIDs", () => {
    const a = newIdempotencyKey();
    const b = newIdempotencyKey();
    expect(a).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(a).not.toBe(b);
  });

  it("judgeCodeForRequest trims and counts code points, as the server does", () => {
    expect(judgeCodeForRequest(undefined)).toBeUndefined();
    expect(judgeCodeForRequest("")).toBeUndefined();
    expect(judgeCodeForRequest(" \n\t ")).toBeUndefined();
    expect(judgeCodeForRequest("  JUDGE-1 ")).toBe("JUDGE-1");
    // 256 astral characters are 512 UTF-16 units but 256 characters.
    const astral = "\u{1F600}".repeat(JUDGE_CODE_MAX_CHARS);
    expect(judgeCodeForRequest(astral)).toBe(astral);
    expect(judgeCodeForRequest(`${astral}x`)).toBeNull();
    expect(judgeCodeForRequest("x".repeat(JUDGE_CODE_MAX_CHARS + 1))).toBeNull();
  });

  it("isRunId accepts run- + UUID only", () => {
    expect(isRunId("run-00000000-0000-4000-8000-000000000001")).toBe(true);
    expect(isRunId("run-d6f28bca-0a10-4fcf-9257-4cfb7374280c")).toBe(true);
    expect(isRunId("run-1")).toBe(false);
    expect(isRunId("../etc")).toBe(false);
    expect(isRunId(null)).toBe(false);
  });
});
