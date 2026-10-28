// SPDX-License-Identifier: Apache-2.0
//
// The same-origin client of /api/v1. The static export is served by the same
// server (`consensus-api serve-public`), so every path is relative. JSON only,
// `Content-Type: application/json` on every body, the session cookie through
// `credentials: "same-origin"`, and one error type: every failure, whether the
// server answered `{code, message, retry_after_secs}`, answered something
// that is not the contract, or could not be reached at all, surfaces as an
// ApiError with a stable code the UI localizes. Nothing here throws a string.
//
// Responses are validated against the zod mirrors of the contract
// (schemas.ts), so a view never renders a shape it does not understand.

import type { z } from "zod";

import {
  apiErrorBodySchema,
  cancelResponseSchema,
  type Capabilities,
  capabilitiesSchema,
  decisionResponseSchema,
  healthSchema,
  receiptSchema,
  runCreatedSchema,
  runDetailSchema,
  runListSchema,
  scenariosSchema,
  sessionSchema,
  showcaseSchema,
} from "./schemas";
import type {
  CancelResponseV1,
  DecisionResponseV1,
  GuidedScenario,
  HealthV1,
  RecommendationKind,
  RunCreatedV1,
  RunDetailV1,
  RunListV1,
  RunReceiptV2,
  ScenariosV1,
  SessionV1,
  ShowcaseV1,
} from "./types";

export const API_BASE = "/api/v1";

/** The request header that carries the Turnstile widget's response token. */
export const TURNSTILE_HEADER = "cf-turnstile-response";

/**
 * Codes of this client's own, next to the server's `ErrorCode`s: the server
 * could not be reached (`unreachable`), answered a status without a contract
 * error body (`unexpected_status`), or answered a body that fails the
 * contract (`bad_response`).
 */
export type ClientErrorCode = "unreachable" | "unexpected_status" | "bad_response";

export class ApiError extends Error {
  /** The server's `ErrorCode`, or one of the client's own codes. */
  readonly code: string;
  /** HTTP status; 0 when the server could not be reached. */
  readonly status: number;
  /** Seconds to wait before retrying (body `retry_after_secs`, else the Retry-After header). */
  readonly retryAfterSecs: number | null;
  /** The route that failed, for logs and support (never shown as the message). */
  readonly path: string;

  constructor(options: {
    code: string;
    message: string;
    status: number;
    path: string;
    retryAfterSecs?: number | null;
  }) {
    super(options.message);
    this.name = "ApiError";
    this.code = options.code;
    this.status = options.status;
    this.path = options.path;
    this.retryAfterSecs = options.retryAfterSecs ?? null;
  }
}

export function isApiError(value: unknown): value is ApiError {
  return value instanceof ApiError;
}

type FetchLike = (input: string, init: RequestInit) => Promise<Response>;

export type ApiClientOptions = {
  /** Defaults to the global fetch, resolved at call time. */
  fetch?: FetchLike;
  /** Path prefix; defaults to `/api/v1`. */
  base?: string;
};

type RequestOptions = {
  method?: "GET" | "POST" | "DELETE";
  body?: unknown;
  headers?: Record<string, string>;
  idempotencyKey?: string;
};

/** Parses a Retry-After header given as seconds (the server's form). */
export function parseRetryAfter(value: string | null): number | null {
  if (value === null) {
    return null;
  }
  const seconds = Number(value.trim());
  return Number.isFinite(seconds) && seconds >= 0 ? Math.ceil(seconds) : null;
}

/**
 * A fresh key for `Idempotency-Key`: one per submit, so a retried request
 * returns the run or decision already recorded instead of a second one.
 */
export function newIdempotencyKey(): string {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }
  const bytes = new Uint8Array(16);
  cryptoApi.getRandomValues(bytes);
  const b6 = bytes[6] ?? 0;
  const b8 = bytes[8] ?? 0;
  bytes[6] = (b6 & 0x0f) | 0x40;
  bytes[8] = (b8 & 0x3f) | 0x80;
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

const RUN_ID = /^run-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** True for the only run id shape the server serves (`run-` + UUID). */
export function isRunId(value: string | null | undefined): value is string {
  return typeof value === "string" && RUN_ID.test(value);
}

function summarizeIssues(error: z.ZodError): string {
  return error.issues
    .slice(0, 3)
    .map((issue) => `${issue.path.join(".") || "$"}: ${issue.message}`)
    .join("; ");
}

export function createApiClient(options: ApiClientOptions = {}) {
  const base = options.base ?? API_BASE;
  const fetchImpl: FetchLike = (input, init) =>
    (options.fetch ?? globalThis.fetch)(input, init);

  async function request<X>(
    path: string,
    schema: z.ZodType<X>,
    requestOptions: RequestOptions = {},
  ): Promise<X> {
    const method = requestOptions.method ?? "GET";
    const headers: Record<string, string> = {
      Accept: "application/json",
      ...requestOptions.headers,
    };
    const init: RequestInit = { method, headers, credentials: "same-origin" };
    if (requestOptions.body !== undefined) {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(requestOptions.body);
    }
    if (requestOptions.idempotencyKey !== undefined) {
      headers["Idempotency-Key"] = requestOptions.idempotencyKey;
    }
    const url = `${base}${path}`;

    let response: Response;
    try {
      response = await fetchImpl(url, init);
    } catch (cause) {
      throw new ApiError({
        code: "unreachable",
        message: cause instanceof Error ? cause.message : "fetch failed",
        status: 0,
        path,
      });
    }

    const text = await response.text();
    let json: unknown;
    try {
      json = text === "" ? undefined : JSON.parse(text);
    } catch {
      json = undefined;
    }

    if (!response.ok) {
      const body = apiErrorBodySchema.safeParse(json);
      if (body.success) {
        throw new ApiError({
          code: body.data.code,
          message: body.data.message,
          status: response.status,
          path,
          retryAfterSecs:
            body.data.retry_after_secs ??
            parseRetryAfter(response.headers.get("Retry-After")),
        });
      }
      throw new ApiError({
        code: "unexpected_status",
        message: `HTTP ${response.status} without a contract error body`,
        status: response.status,
        path,
        retryAfterSecs: parseRetryAfter(response.headers.get("Retry-After")),
      });
    }

    const parsed = schema.safeParse(json);
    if (!parsed.success) {
      throw new ApiError({
        code: "bad_response",
        message: `the response does not match the contract: ${summarizeIssues(parsed.error)}`,
        status: response.status,
        path,
      });
    }
    return parsed.data;
  }

  const runPath = (runId: string, suffix = ""): string =>
    `/runs/${encodeURIComponent(runId)}${suffix}`;

  return {
    health: (): Promise<HealthV1> => request("/health", healthSchema),

    capabilities: (): Promise<Capabilities> =>
      request("/capabilities", capabilitiesSchema),

    scenarios: (): Promise<ScenariosV1> => request("/scenarios", scenariosSchema),

    /** The current session, or null when the server says there is none. */
    getSession: async (): Promise<SessionV1 | null> => {
      try {
        return await request("/session", sessionSchema);
      } catch (error) {
        if (
          isApiError(error) &&
          (error.code === "session_required" || error.status === 401)
        ) {
          return null;
        }
        throw error;
      }
    },

    createSession: (): Promise<SessionV1> =>
      request("/session", sessionSchema, { method: "POST" }),

    /** Ends the session; the server answers with no contract body. */
    deleteSession: async (): Promise<void> => {
      await request("/session", acceptAnything, { method: "DELETE" });
    },

    /**
     * The session to act under: the existing one when GET says there is
     * one, else a freshly minted one. Only POSTs when needed, so a visitor
     * keeps the session (and the runs) they already have.
     */
    ensureSession: async function ensureSession(): Promise<SessionV1> {
      const existing = await this.getSession();
      return existing ?? (await this.createSession());
    },

    listRuns: (query: { cursor?: string; limit?: number } = {}): Promise<RunListV1> => {
      const params = new URLSearchParams();
      if (query.cursor !== undefined) {
        params.set("cursor", query.cursor);
      }
      if (query.limit !== undefined) {
        params.set("limit", String(query.limit));
      }
      const search = params.toString();
      return request(`/runs${search === "" ? "" : `?${search}`}`, runListSchema);
    },

    getRun: (runId: string): Promise<RunDetailV1> =>
      request(runPath(runId), runDetailSchema),

    getReceipt: (runId: string): Promise<RunReceiptV2> =>
      request(runPath(runId, "/receipt"), receiptSchema),

    createGuidedRun: (
      scenario: GuidedScenario,
      idempotencyKey: string,
    ): Promise<RunCreatedV1> =>
      request("/runs", runCreatedSchema, {
        method: "POST",
        body: { scenario },
        idempotencyKey,
      }),

    createLiveRun: (
      presetId: string,
      gate: { turnstileToken: string | null; attestAdult: boolean; judgeCode?: string },
      idempotencyKey: string,
    ): Promise<RunCreatedV1> => {
      const body: Record<string, unknown> = {
        preset_id: presetId,
        attest_adult: gate.attestAdult,
      };
      if (gate.judgeCode !== undefined && gate.judgeCode !== "") {
        body.judge_code = gate.judgeCode;
      }
      const headers: Record<string, string> = {};
      if (gate.turnstileToken !== null && gate.turnstileToken !== "") {
        headers[TURNSTILE_HEADER] = gate.turnstileToken;
      }
      return request("/live-runs", runCreatedSchema, {
        method: "POST",
        body,
        headers,
        idempotencyKey,
      });
    },

    decide: (
      runId: string,
      decision: {
        approved: boolean;
        note?: string;
        expected: { recommendation: RecommendationKind; lastSequence: number };
      },
      idempotencyKey: string,
    ): Promise<DecisionResponseV1> => {
      const body: Record<string, unknown> = {
        approved: decision.approved,
        expected: {
          recommendation: decision.expected.recommendation,
          last_sequence: decision.expected.lastSequence,
        },
      };
      if (decision.note !== undefined && decision.note.trim() !== "") {
        body.note = decision.note;
      }
      return request(runPath(runId, "/decision"), decisionResponseSchema, {
        method: "POST",
        body,
        idempotencyKey,
      });
    },

    cancel: (runId: string): Promise<CancelResponseV1> =>
      request(runPath(runId, "/cancel"), cancelResponseSchema, {
        method: "POST",
      }),

    showcase: (): Promise<ShowcaseV1> => request("/showcase", showcaseSchema),

    showcaseDetail: (runId: string): Promise<RunDetailV1> =>
      request(`/showcase/${encodeURIComponent(runId)}`, runDetailSchema),

    showcaseReceipt: (runId: string): Promise<RunReceiptV2> =>
      request(`/showcase/${encodeURIComponent(runId)}/receipt`, receiptSchema),

    /** The verbatim event log (404 `ndjson_unavailable` for live-declared public runs). */
    ndjsonUrl: (runId: string): string => `${base}${runPath(runId, "/events.ndjson")}`,

    /** The SSE route; `after` replays the records after that sequence. */
    eventsUrl: (runId: string, after?: number): string =>
      `${base}${runPath(runId, "/events")}${
        after === undefined ? "" : `?after=${encodeURIComponent(String(after))}`
      }`,

    receiptUrl: (runId: string): string => `${base}${runPath(runId, "/receipt")}`,
  };
}

// DELETE /session has no contract body; whatever comes back is accepted.
const acceptAnything = {
  safeParse: (value: unknown) => ({ success: true as const, data: value }),
} as unknown as z.ZodType<unknown>;

export type ApiClient = ReturnType<typeof createApiClient>;

/** The app's client: same origin, global fetch. */
export const api: ApiClient = createApiClient();
