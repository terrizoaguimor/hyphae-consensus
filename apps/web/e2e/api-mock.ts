// SPDX-License-Identifier: Apache-2.0
//
// A mocked /api/v1 over the contract fixtures, installed with page.route():
// sessions, scenarios, capabilities (with or without the live section),
// health, the run list, the detail and the SSE stream of each mocked run
// (the frames as one text/event-stream body), the decision and the cancel.
// Every call is recorded so a spec can assert headers and bodies. The
// variants derived here (a run awaiting its decision, a live-model run, a run
// whose provenance is inconsistent, a run in progress, a failed run) are
// built from the fixtures, never typed in. The live section of the
// capabilities has the contract's shape (LiveCapabilitiesV1, as
// crates/consensus-api/src/live/gate.rs publishes it) and the whole
// capabilities answer is validated with the client's own zod mirror, so the
// mock cannot drift from the contract: its one preset is the live R01
// fixture's task, its budget amounts are test values in the ledger's format.

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import path from "node:path";

import type { Page, Request } from "@playwright/test";

import { capabilitiesSchema, receiptSchema, runDetailSchema, showcaseSchema } from "../src/lib/api/schemas";
import type { LiveCapabilitiesV1, PresetV1, PublicBudgetV1 } from "../src/lib/api/types";
import { recordHash } from "../src/lib/chain/verify";

import { fixture } from "./contract";
import { logSegment, researchBytes, researchDir } from "./research";

type Json = Record<string, unknown>;

export type RecordedCall = {
  method: string;
  path: string;
  headers: Record<string, string>;
  body: unknown;
};

/** One answer of a route: a status and a JSON body. */
export type Answer = { status: number; body: Json; retryAfter?: string };

/** A run the mock serves under its own id. */
export type MockRun = {
  detail: Json;
  /** The SSE frames (`frame`-tagged, as sse-frames-*.json); default none. */
  frames?: Json[];
};

export type ApiMockOptions = {
  /** GET /session answers 200 (true) or 401 session_required (false). */
  session?: boolean;
  liveEnabled?: boolean;
  /** Replaces the live section's presets (default: the live R01 fixture's task). */
  livePresets?: PresetV1[];
  /** Replaces the budget; `null` leaves it out (the ledger could not be read). */
  liveBudget?: PublicBudgetV1 | null;
  liveRequiresTurnstile?: boolean;
  liveRequiresAttestation?: boolean;
  /** Overrides of the capabilities fixture (e.g. guided_enabled). */
  capabilities?: Json;
  /** Overrides of the health fixture (e.g. contract_sha256). */
  health?: Json;
  /** What POST /live-runs answers, in order (the last one repeats); default 202. */
  liveRuns?: Answer[];
  /** What POST /runs answers; default 202 with the created run. */
  guidedRun?: Answer;
  detail?: Json;
  /** The detail served once a decision was recorded; default the retry fixture. */
  decidedDetail?: Json;
  /** What GET /runs/{id}/receipt answers once a decision was recorded; default `receipt`. */
  decidedReceipt?: Json;
  /** What GET /runs/{id}/events.ndjson serves once a decision was recorded, by run id; default `ndjson`. */
  decidedNdjson?: Record<string, Buffer>;
  frames?: Json[];
  /** Other runs, served by the run_id of their detail. */
  extraRuns?: MockRun[];
  /** The first N GET /runs/{id} answer this error (default none). */
  detailFailures?: { count: number; answer: Answer };
  /** The first N GET /runs/{id}/events answer this error (default none). */
  streamFailures?: { count: number; answer: Answer };
  /** What GET /runs/{id}/receipt answers; default the retry fixture's receipt. */
  receipt?: Json;
  /** The first N GET /runs/{id}/receipt answer this error (default none). */
  receiptFailures?: { count: number; answer: Answer };
  runs?: Json[];
  decision?: Answer;
  /** The entries GET /showcase lists (ShowcaseEntryV1); default none (an empty archive). */
  showcase?: Json[];
  /** The first N GET /showcase answer this error (default none). */
  showcaseFailures?: { count: number; answer: Answer };
  /** Recorded runs GET /showcase/{id} and /showcase/{id}/receipt answer (recordedRun). */
  recordedRuns?: Array<{ detail: Json; receipt: Json }>;
  /** Archive files served at /research/<path> instead of the archive's own bytes. */
  research?: Record<string, Buffer>;
  /**
   * GET /showcase publishes the digest of the MANIFEST.json served here (a
   * `research` replacement included) instead of the archive's own: a test
   * that serves another manifest on purpose makes the two match. Default false.
   */
  publishServedManifest?: boolean;
  /** What GET /runs/{id}/events.ndjson serves, by run id; default 404 ndjson_unavailable. */
  ndjson?: Record<string, Buffer>;
};

export const RUN_ID = "run-00000000-0000-4000-8000-000000000001";

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

/** The same run with a live model axis (declared scripted, so flags tell the rest). */
export function liveModelDetail(flags: string[] = []): Json {
  const detail = awaitingDecisionDetail();
  const provenance = (detail.summary as Json).provenance as Json;
  provenance.model = "live_meta";
  provenance.evidence = "hyphae_snapshot";
  provenance.flags = flags;
  provenance.meta_request_ids = ["chatcmpl-fixture-1"];
  return detail;
}

/**
 * The live-model run with every invocation a recorded Meta call (test
 * values in the record format: request ids, latencies, token counts and a
 * finish reason), as the public surface shows them: no output bound, no
 * attempt count, no schema or prompt digests. Built from the retry fixture.
 */
export function metaCallsDetail(): Json {
  const detail = liveModelDetail();
  const invocations: Json[] = [];
  for (const event of detail.events as Json[]) {
    const payload = event.payload as Json;
    if (payload.invocation !== undefined && payload.invocation !== null) {
      const n = invocations.length + 1;
      const invocation: Json = {
        provider: "meta",
        model: "muse-wonder-1.0",
        request_id: `chatcmpl-fixture-${n}`,
        latency_ms: 1000 * n,
        input_tokens: 100 * n,
        output_tokens: 10 * n,
        finish_reason: "stop",
        scripted: false,
      };
      payload.invocation = invocation;
      invocations.push(invocation);
    }
  }
  detail.model_invocations = invocations;
  const provenance = (detail.summary as Json).provenance as Json;
  provenance.meta_request_ids = invocations.map((i) => i.request_id);
  return detail;
}

/** The receipt's note on a multi-run log (crates/consensus-runtime/src/receipts.rs CHAIN_NOTE_LOG). */
const CHAIN_NOTE_LOG =
  "This run is one segment of a multi-run log: its records are linked to records of other runs, so verifying its chain needs the whole log file, from genesis. head_hash is the hash of this run's last event, not the head of the file; it commits to every earlier record in the file, not to records written after it.";

/** The receipt's note on unchained records (receipts.rs CHAIN_NOTE_UNCHAINED). */
const CHAIN_NOTE_UNCHAINED =
  "Pre-1.1 records carry no hash chain: nothing in this run is tamper-evident, and there is no head_hash.";

/** The demo store's anchor the 2026-10-27 batteries verified against (research/README.md). */
export const DEMO_ANCHOR = "d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168";

export type RecordedOptions = {
  /** The guided fixture the run's records are derived from. */
  base: "retry" | "invalid_proof";
  /** Every record reports a proof error and every proof is invalid (the 2026-10-26 battery's shape). */
  generationFailed?: boolean;
};

/**
 * A recorded run of the archive as GET /showcase/{id} and its receipt would
 * answer, for a showcase entry: the records of a guided fixture (the
 * variants convention: no server logic is re-implemented here) under the
 * entry's run id, title, provenance and schema version, at rest, every
 * verification a real verifier's (or, with `generationFailed`, every record
 * reporting a proof error and every proof invalid, as the 2026-10-26
 * battery's records read), and the chain of the run's real segment in its
 * archived log (scope, first and last sequence, head).
 */
export function recordedRun(entry: Json, options: RecordedOptions): { detail: Json; receipt: Json } {
  const runId = String(entry.run_id);
  const base = options.base === "retry" ? awaitingDecisionDetail() : clone(fixture<Json>("run-detail-invalid_proof.json"));
  const baseId = String((base.summary as Json).run_id);
  const detail = JSON.parse(JSON.stringify(base).split(baseId).join(runId)) as Json;
  const unchained = entry.head_hash === null || entry.head_hash === undefined;
  const schema = String(entry.schema_version);
  const segment = logSegment(String(entry.file), runId);
  const verifier = options.generationFailed === true ? "recall(prove) upstream" : "hyphae proof verify";
  const recordVerification = (verification: Json) => {
    verification.simulated = false;
    verification.verifier = verifier;
    verification.verified_anchor = DEMO_ANCHOR;
    delete verification.round;
    delete verification.detail;
    if (options.generationFailed === true) {
      verification.integrity_status = "invalid";
    }
  };
  for (const event of detail.events as Json[]) {
    event.schema_version = schema;
    if (unchained) {
      event.hash = "";
      event.prev_hash = "";
    }
    const payload = event.payload as Json;
    if (payload.type === "evidence_retrieved") {
      const evidence = payload.evidence as Json;
      delete evidence.retrieval;
      evidence.anchor = DEMO_ANCHOR;
      evidence.proof_error_reported = options.generationFailed === true;
    }
    if (payload.type === "evidence_verified") {
      recordVerification(payload.verification as Json);
    }
  }
  for (const round of detail.rounds as Json[]) {
    for (const verification of round.verifications as Json[]) {
      recordVerification(verification);
    }
  }
  // A live-model run's calls carry the archive's own Meta request ids, in
  // order (latency and token counts are test values); a scripted run keeps
  // the fixture's scripted doubles.
  const provenance = entry.provenance as Json;
  const requestIds = (provenance.meta_request_ids as string[] | undefined) ?? [];
  if (provenance.model === "live_meta" && requestIds.length > 0) {
    const invocations: Json[] = [];
    for (const event of detail.events as Json[]) {
      const payload = event.payload as Json;
      if (payload.invocation !== undefined && payload.invocation !== null) {
        const n = invocations.length;
        const invocation: Json = {
          provider: "meta",
          model: "muse-wonder-1.0",
          request_id: requestIds[n % requestIds.length],
          latency_ms: 20000 + 100 * n,
          input_tokens: 200 + n,
          output_tokens: 2800 + n,
          scripted: false,
        };
        payload.invocation = invocation;
        invocations.push(invocation);
      }
    }
    detail.model_invocations = invocations;
  }
  const summary = detail.summary as Json;
  summary.provenance = entry.provenance;
  summary.title = entry.title;
  summary.active = false;
  // The server counts the detail's records and the receipt's from the same
  // events (showcase.rs detail() and receipt()): the run's real segment.
  summary.event_count = segment.count;
  (detail.task as Json).title = entry.title;
  (detail.task as Json).schema_version = schema;
  delete (detail.task as Json).mode;
  detail.chain = {
    scope: unchained ? "unchained" : segment.first === 1 ? "run" : "log",
    first_sequence: segment.first,
    last_sequence: segment.last,
    head_hash: segment.head,
  };
  const receipt = JSON.parse(
    JSON.stringify(fixture<Json>(`receipt-v2-${options.base}.json`)).split(baseId).join(runId),
  ) as Json;
  receipt.run_schema_version = schema;
  receipt.event_count = segment.count;
  receipt.final_state = summary.state;
  receipt.authorization = { state: summary.authorization };
  receipt.chain = {
    scope: (detail.chain as Json).scope,
    first_sequence: segment.first,
    last_sequence: segment.last,
    head_hash: segment.head,
    note: unchained ? CHAIN_NOTE_UNCHAINED : CHAIN_NOTE_LOG,
  };
  for (const { verification } of receipt.verifications as Array<{ verification: Json }>) {
    verification.verifier = verifier;
    delete verification.detail;
    if (options.generationFailed === true) {
      verification.integrity_status = "invalid";
    }
  }
  // The receipt lists the same calls (ModelInvocation: no `scripted` field).
  receipt.model_invocations = (detail.model_invocations as Json[]).map((invocation) => {
    const call = { ...invocation };
    delete call.scripted;
    return call;
  });
  return { detail, receipt };
}

/**
 * A run of this server with a verbatim log: its events written as JSON lines
 * and chained with the textual recipe of event_log.rs (SHA-256 of prev_hash
 * followed by the line with its hash blanked, src/lib/chain/verify.ts, here
 * through Node's WebCrypto), and the detail's hashes and chain and the
 * receipt's head and record count set to match. The bytes are test bytes:
 * the guided fixtures carry public views of their records, display copies
 * whose own hashes cannot be recomputed.
 */
export async function chainedRun(source: Json): Promise<{ detail: Json; receipt: Json; ndjson: Buffer }> {
  const detail = clone(source);
  const events = detail.events as Json[];
  const lines: string[] = [];
  let prev = "genesis";
  for (const event of events) {
    event.prev_hash = prev;
    event.hash = "";
    const hash = await recordHash(prev, JSON.stringify(event));
    if (hash === null) {
      throw new Error("no hash field in the record");
    }
    event.hash = hash;
    lines.push(JSON.stringify(event));
    prev = hash;
  }
  const first = (events[0]?.sequence as number | undefined) ?? 0;
  const last = (events.at(-1)?.sequence as number | undefined) ?? 0;
  detail.chain = { scope: "run", first_sequence: first, last_sequence: last, head_hash: prev };
  const receipt = clone(fixture<Json>("receipt-v2-retry.json"));
  receipt.event_count = events.length;
  receipt.chain = { ...(receipt.chain as Json), first_sequence: first, last_sequence: last, head_hash: prev };
  return { detail, receipt, ndjson: Buffer.from(`${lines.join("\n")}\n`) };
}

/** Test values of a sealed-snapshot capture (ADR-0008), in the record format. */
export const SNAPSHOT_ANCHOR = "d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168";

/**
 * The live-model run with sealed-snapshot evidence: every retrieved record
 * carries the snapshot's id, capture time and the proof's own request (the
 * complete record list of demo-repo), and every verification is a real
 * verifier's (not simulated), with its report. Built from the retry fixture.
 */
export function sealedSnapshotDetail(): Json {
  const detail = liveModelDetail();
  for (const event of detail.events as Json[]) {
    const payload = event.payload as Json;
    if (payload.type === "evidence_retrieved") {
      const evidence = payload.evidence as Json;
      evidence.retrieval = "sealed_snapshot";
      evidence.anchor = SNAPSHOT_ANCHOR;
      evidence.snapshot_id = "snapshot-fixture-1";
      evidence.captured_at = "2026-10-26T08:00:00Z";
      evidence.proof_ref = "list-demo-repo";
      evidence.proof_request = { operation: "list", project: "demo-repo", limit: 64 };
    }
    if (payload.type === "evidence_verified") {
      const verification = payload.verification as Json;
      verification.simulated = false;
      verification.verifier = "hyphae proof verify";
      verification.verified_anchor = SNAPSHOT_ANCHOR;
      verification.proof_ref = "list-demo-repo";
      verification.report = {
        status: "verified",
        scope: "semantic_reexecution",
        kind: "memory",
        anchor_digest: SNAPSHOT_ANCHOR,
        proof_digest: "ab".repeat(32),
        result_digest: "cd".repeat(32),
      };
      // A real verifier's raw output stays on the operator surface.
      delete verification.detail;
    }
  }
  for (const round of detail.rounds as Json[]) {
    round.verifications = (detail.events as Json[])
      .map((e) => e.payload as Json)
      .filter((p) => p.type === "evidence_verified")
      .map((p) => p.verification as Json)
      .filter((v) => v.round === round.round);
  }
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

/**
 * A live-model run still deliberating, as a client connected mid-run sees
 * it: the records up to `upTo`, every summary active, the frames delivered
 * live and no stream_end (the stream stays open).
 */
export function inProgressLiveRun(upTo: number): { detail: Json; frames: Json[] } {
  const provenance = {
    model: "live_meta",
    evidence: "hyphae_snapshot",
    flags: [],
    meta_request_ids: ["chatcmpl-fixture-1"],
  };
  const frames = framesWith(provenance, "live")
    .filter((f) => f.frame === "consensus" && ((f.event as Json).sequence as number) <= upTo)
    .map((f) => ({ ...f, summary: { ...(f.summary as Json), active: true } }));
  const detail = clone(fixture<Json>("run-detail-retry.json"));
  const events = (detail.events as Json[]).filter((e) => (e.sequence as number) <= upTo);
  detail.events = events;
  // The proposal as it stands at `upTo`: the last proposal or revision.
  let current: unknown = undefined;
  for (const event of events) {
    const payload = event.payload as Json;
    if (payload.type === "proposal_submitted") {
      current = payload.proposal;
    } else if (payload.type === "revision_submitted") {
      current = (payload.revision as Json).proposal;
    }
  }
  detail.current_proposal = current;
  delete detail.decision;
  delete detail.recommendation;
  const last = frames.at(-1);
  detail.summary = clone((last?.summary ?? detail.summary) as Json);
  (detail.chain as Json).last_sequence = upTo;
  return { detail, frames };
}

/**
 * The retry run failed at the evaluation with `code`: its first four
 * records, then a run_failed record carrying the server's English message
 * (which the UI must never show) and the code it must translate.
 */
export function failedDetail(code: string | null, message: string): { detail: Json; frames: Json[] } {
  const detail = clone(fixture<Json>("run-detail-retry.json"));
  const kept = (detail.events as Json[]).filter((e) => (e.sequence as number) <= 4);
  const previous = kept[kept.length - 1] as Json;
  const failure: Json = { kind: "failed", stage: "evaluate", message };
  if (code !== null) {
    failure.code = code;
  }
  const failed: Json = {
    sequence: 5,
    event_id: "evt-fixture-failed",
    run_id: (detail.summary as Json).run_id,
    schema_version: previous.schema_version,
    actor: { kind: "runtime" },
    occurred_at: previous.occurred_at,
    prev_hash: previous.hash,
    hash: "f".repeat(64),
    payload: { type: "run_failed", stage: "evaluate", ...(code === null ? {} : { code }), message },
  };
  detail.events = [...kept, failed];
  const proposal = kept.find((e) => (e.payload as Json).type === "proposal_submitted");
  detail.current_proposal = (proposal?.payload as Json | undefined)?.proposal;
  delete detail.decision;
  delete detail.recommendation;
  detail.rounds = [];
  detail.challenge_items = [];
  const summary = detail.summary as Json;
  summary.state = "failed";
  summary.authorization = "pending";
  summary.active = false;
  summary.last_sequence = 5;
  summary.event_count = 5;
  summary.failure = failure;
  delete summary.recommendation;
  const chain = detail.chain as Json;
  chain.last_sequence = 5;
  chain.head_hash = failed.hash;
  const frames = (detail.events as Json[]).map((event) => ({
    frame: "consensus",
    event,
    delivery: "replay",
    summary,
  }));
  return { detail, frames: [...frames, { frame: "stream_end", reason: "terminal", state: "failed", last_sequence: 5 }] };
}

/** A run served under its own id: its events replayed as frames, then stream_end. */
export function storedRun(detail: Json): MockRun {
  const summary = detail.summary as Json;
  const frames: Json[] = (detail.events as Json[]).map((event) => ({
    frame: "consensus",
    event,
    delivery: "replay",
    summary,
  }));
  const last = (detail.events as Json[]).at(-1);
  frames.push({
    frame: "stream_end",
    reason: summary.state === "awaiting_human_approval" ? "awaiting_human_approval" : "terminal",
    state: summary.state,
    last_sequence: last?.sequence ?? 0,
  });
  return { detail, frames };
}

/**
 * The retry run as a run recorded before protocol 1.3 would read: every
 * record of schema 1.2 and the two objections of round 1 never judged again
 * (no later evaluation listed or resolved them). Built from the fixture.
 */
export function beforeCarryForwardDetail(): Json {
  const detail = clone(fixture<Json>("run-detail-retry.json"));
  for (const event of detail.events as Json[]) {
    event.schema_version = "1.2";
  }
  (detail.task as Json).schema_version = "1.2";
  (detail.recommendation as Json).protocol = "policy 1.2";
  detail.challenge_items = (detail.challenge_items as Json[]).map((item) => ({
    ...item,
    status: "never_rejudged",
    resolution_reason: null,
  }));
  return detail;
}

/** The live R01 run just created (declared Meta, nothing recorded but the task). */
export function liveR01Run(): MockRun {
  const detail = clone(fixture<Json>("run-detail-live-R01-reference.json"));
  const frames = (detail.events as Json[]).map((event) => ({
    frame: "consensus",
    event,
    delivery: "replay",
    summary: detail.summary,
  }));
  return { detail, frames };
}

/** The live section the gate publishes, from the live R01 fixture's task. */
export function liveSection(options: ApiMockOptions = {}): LiveCapabilitiesV1 {
  const task = fixture<{ task: { title: string; description: string; project_id: string; mode: { preset: string } } }>(
    "run-detail-live-R01-reference.json",
  ).task;
  const presets: PresetV1[] = options.livePresets ?? [
    {
      id: task.mode.preset,
      title: task.title,
      description: task.description,
      project: task.project_id,
      model: "live_meta",
      evidence: "hyphae_snapshot",
    },
  ];
  const budget: PublicBudgetV1 = {
    daily_available_now: "4.250000",
    daily_remaining: "9.120000",
    lifetime_remaining: "51.300000",
    tripped: false,
  };
  const live: LiveCapabilitiesV1 = {
    presets,
    requires_turnstile: options.liveRequiresTurnstile ?? true,
    requires_adult_attestation: options.liveRequiresAttestation ?? true,
  };
  if (options.liveBudget !== null) {
    live.budget = options.liveBudget ?? budget;
  }
  return live;
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
  const state = {
    decided: false,
    session: options.session ?? true,
    detailFailures: options.detailFailures?.count ?? 0,
    streamFailures: options.streamFailures?.count ?? 0,
    receiptFailures: options.receiptFailures?.count ?? 0,
    showcaseFailures: options.showcaseFailures?.count ?? 0,
    liveRuns: [...(options.liveRuns ?? [])],
  };
  const capabilities: Json = {
    ...clone(fixture<Json>("capabilities-public.json")),
    ...options.capabilities,
  };
  if (options.liveEnabled === true) {
    capabilities.live_enabled = true;
    capabilities.live = liveSection(options);
  }
  // The mock answers only what the client's own mirror of the contract accepts.
  capabilitiesSchema.parse(capabilities);
  const health = { ...fixture<Json>("health.json"), ...options.health };
  const created = fixture<Json>("run-created-retry.json");
  const runsById = new Map<string, MockRun>();
  runsById.set(RUN_ID, {
    detail: options.detail ?? fixture<Json>("run-detail-retry.json"),
    frames: options.frames ?? fixture<Json[]>("sse-frames-retry.json"),
  });
  for (const run of options.extraRuns ?? []) {
    runsById.set(String((run.detail.summary as Json).run_id), run);
  }
  const decidedDetail = options.decidedDetail ?? fixture<Json>("run-detail-retry.json");
  // The showcase, validated with the client's own mirrors like the capabilities.
  const researchFile = (file: string): Buffer | null => {
    const override = options.research?.[file];
    if (override !== undefined) {
      return override;
    }
    const full = path.join(researchDir(), ...file.split("/"));
    return existsSync(full) && !file.split("/").includes("..") ? researchBytes(file) : null;
  };
  // The digest GET /showcase publishes is the archive manifest's own, so a
  // test that replaces the served manifest makes them differ (unless it asks
  // for the served manifest's digest to be published).
  const publishedManifest =
    options.publishServedManifest === true
      ? (researchFile("MANIFEST.json") ?? researchBytes("MANIFEST.json"))
      : researchBytes("MANIFEST.json");
  const showcase = {
    entries: options.showcase ?? [],
    manifest_file: "MANIFEST.json",
    manifest_sha256: createHash("sha256").update(publishedManifest).digest("hex"),
  };
  showcaseSchema.parse(showcase);
  const recordedById = new Map<string, { detail: Json; receipt: Json }>();
  for (const recorded of options.recordedRuns ?? []) {
    runDetailSchema.parse(recorded.detail);
    receiptSchema.parse(recorded.receipt);
    recordedById.set(String((recorded.detail.summary as Json).run_id), recorded);
  }
  const runs = options.runs ?? [fixture<Json>("run-summary-retry.json")];

  const json = (status: number, body: unknown, headers: Record<string, string> = {}) => ({
    status,
    contentType: "application/json",
    headers: { "cache-control": "no-store", ...headers },
    body: JSON.stringify(body),
  });
  const answer = (given: Answer) =>
    json(given.status, given.body, given.retryAfter === undefined ? {} : { "retry-after": given.retryAfter });
  const error = (status: number, code: string, message: string) => json(status, { code, message });

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
      return route.fulfill(json(200, health));
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
      return route.fulfill(answer(options.guidedRun ?? { status: 202, body: created }));
    }
    if (method === "POST" && path === "/api/v1/live-runs") {
      const next =
        state.liveRuns.length > 1 ? state.liveRuns.shift() : state.liveRuns[0];
      return route.fulfill(
        answer(
          next ??
            (capabilities.live_enabled === true
              ? { status: 202, body: fixture<Json>("run-created-live-R01-reference.json") }
              : {
                  status: 503,
                  body: { code: "live_disabled", message: "the public live mode is not enabled on this server" },
                }),
        ),
      );
    }
    if (method === "GET" && path === "/api/v1/showcase") {
      if (state.showcaseFailures > 0 && options.showcaseFailures !== undefined) {
        state.showcaseFailures -= 1;
        return route.fulfill(answer(options.showcaseFailures.answer));
      }
      return route.fulfill(json(200, showcase));
    }
    const recordedMatch = /^\/api\/v1\/showcase\/(run-[0-9a-f-]+)(\/receipt)?$/.exec(path);
    if (method === "GET" && recordedMatch !== null) {
      const recorded = recordedById.get(recordedMatch[1] ?? "");
      if (recorded === undefined) {
        return route.fulfill(error(404, "not_found", "not found"));
      }
      return route.fulfill(json(200, recordedMatch[2] === undefined ? recorded.detail : recorded.receipt));
    }
    if (method === "GET" && path === "/api/v1/runs") {
      return route.fulfill(json(200, { runs, next_cursor: null }));
    }
    const match = /^\/api\/v1\/runs\/(run-[0-9a-f-]+)(\/.*)?$/.exec(path);
    const run = match === null ? undefined : runsById.get(match[1] ?? "");
    const suffix = match?.[2] ?? "";
    if (run !== undefined && method === "GET" && suffix === "") {
      if (state.detailFailures > 0 && options.detailFailures !== undefined) {
        state.detailFailures -= 1;
        return route.fulfill(answer(options.detailFailures.answer));
      }
      const decided = state.decided && match?.[1] === RUN_ID;
      return route.fulfill(json(200, decided ? decidedDetail : run.detail));
    }
    if (run !== undefined && method === "GET" && suffix === "/events") {
      if (state.streamFailures > 0 && options.streamFailures !== undefined) {
        state.streamFailures -= 1;
        return route.fulfill(answer(options.streamFailures.answer));
      }
      return route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        headers: { "cache-control": "no-store" },
        body: sseBody(run.frames ?? []),
      });
    }
    if (run !== undefined && method === "GET" && suffix === "/events.ndjson") {
      const decided = state.decided && match?.[1] === RUN_ID;
      const ndjson =
        (decided ? options.decidedNdjson?.[match?.[1] ?? ""] : undefined) ?? options.ndjson?.[match?.[1] ?? ""];
      if (ndjson === undefined) {
        return route.fulfill(
          error(404, "ndjson_unavailable", "the verbatim event log of a live run is not published on the public surface"),
        );
      }
      return route.fulfill({
        status: 200,
        contentType: "application/x-ndjson",
        headers: { "cache-control": "no-store" },
        body: ndjson,
      });
    }
    if (run !== undefined && method === "GET" && suffix === "/receipt") {
      if (state.receiptFailures > 0 && options.receiptFailures !== undefined) {
        state.receiptFailures -= 1;
        return route.fulfill(answer(options.receiptFailures.answer));
      }
      const decided = state.decided && match?.[1] === RUN_ID;
      return route.fulfill(
        json(200, (decided ? options.decidedReceipt : undefined) ?? options.receipt ?? fixture("receipt-v2-retry.json")),
      );
    }
    if (run !== undefined && method === "POST" && suffix === "/decision") {
      if (options.decision !== undefined) {
        return route.fulfill(answer(options.decision));
      }
      const body = calls[calls.length - 1]?.body as Json | null;
      state.decided = true;
      return route.fulfill(json(200, decisionResponse(body?.approved === true, body?.note)));
    }
    if (run !== undefined && method === "POST" && suffix === "/cancel") {
      return route.fulfill(json(202, { run_id: match?.[1], cancelling: true }));
    }
    return route.fulfill(error(404, "not_found", "not found"));
  });

  // The research archive beside the API: /research/<path>, the archive's own
  // bytes (or a test's replacement), with the pinned digest as the ETag.
  await page.route("**/research/**", async (route, request) => {
    record(request);
    const file = decodeURIComponent(new URL(request.url()).pathname.slice("/research/".length));
    const bytes = researchFile(file);
    if (bytes === null) {
      return route.fulfill(error(404, "not_found", "not found"));
    }
    const sha256 = createHash("sha256").update(bytes).digest("hex");
    return route.fulfill({
      status: 200,
      contentType: file.endsWith(".jsonl")
        ? "application/x-ndjson"
        : file.endsWith(".json")
          ? "application/json"
          : "text/plain; charset=utf-8",
      headers: { etag: `"${sha256}"`, "x-content-sha256": `sha256=${sha256}` },
      body: bytes,
    });
  });

  return { calls, runId: RUN_ID };
}
