// SPDX-License-Identifier: Apache-2.0
//
// The Meta API panel's rows (src/lib/run/invocations.ts): one per recorded
// model invocation, in log order, with its role, exactly the invocations the
// runtime's aggregate lists (RunDetailV1.model_invocations); totals over the
// real model calls only, each with how many rows report the field; and the
// operator-only fields, absent on the public surface.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type { EventV1, ModelInvocationV1, RunDetailV1 } from "../src/lib/api/types";
import { invocationRows, invocationTotals, operatorFieldsAbsent } from "../src/lib/run/invocations";

function fixture<T>(name: string): T {
  return JSON.parse(readFileSync(path.join(contractDir(), "fixtures", name), "utf8")) as T;
}

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

/** The retry run with every invocation turned into a recorded Meta call (test values). */
function metaDetail(): RunDetailV1 {
  const detail = clone(fixture<RunDetailV1>("run-detail-retry.json"));
  let n = 0;
  for (const event of detail.events) {
    const payload = event.payload as { invocation?: ModelInvocationV1 };
    if (payload.invocation !== undefined) {
      n += 1;
      payload.invocation = {
        provider: "meta",
        model: "muse-wonder-1.0",
        request_id: `chatcmpl-fixture-${n}`,
        latency_ms: 1000 * n,
        input_tokens: 100 * n,
        output_tokens: 10 * n,
        reasoning_tokens: n === 1 ? 7 : null,
        finish_reason: "stop",
        scripted: false,
      };
    }
  }
  return detail;
}

describe("invocationRows", () => {
  it("lists exactly the run's invocations, in order, with their roles (retry)", () => {
    const detail = fixture<RunDetailV1>("run-detail-retry.json");
    const rows = invocationRows(detail.events);
    expect(rows.map((r) => r.invocation.request_id)).toEqual(
      detail.model_invocations.map((i) => i.request_id),
    );
    expect(rows.map((r) => r.role)).toEqual([
      "proposer",
      "critic",
      "evidence_evaluator",
      "proposer",
      "critic",
      "evidence_evaluator",
    ]);
    expect(rows.map((r) => r.sequence)).toEqual([2, 5, 6, 9, 10, 11]);
    expect(rows.every((r) => r.invocation.scripted && r.failedStage === null)).toBe(true);
  });

  it.each(["offline", "invalid_proof", "injection", "abstain"])(
    "matches the run's list of invocations (%s)",
    (name) => {
      const detail = fixture<RunDetailV1>(`run-detail-${name}.json`);
      expect(invocationRows(detail.events).map((r) => r.invocation)).toEqual(detail.model_invocations);
    },
  );

  it("keeps a failed run's last call, with the role of its stage", () => {
    const detail = clone(fixture<RunDetailV1>("run-detail-retry.json"));
    const last = detail.events.at(-1) as EventV1;
    const failed: EventV1 = {
      ...last,
      sequence: 16,
      event_id: "evt-failed",
      actor: { kind: "runtime" },
      payload: {
        type: "run_failed",
        stage: "evaluate",
        code: "model_malformed_output",
        message: "The model returned output that was refused.",
        invocation: { provider: "meta", model: "muse-wonder-1.0", request_id: "chatcmpl-x", scripted: false },
      },
    };
    const rows = invocationRows([...detail.events, failed]);
    expect(rows.at(-1)).toMatchObject({ sequence: 16, role: "evidence_evaluator", failedStage: "evaluate" });
    const unknownStage = { ...failed, payload: { ...failed.payload, stage: "elsewhere" } } as EventV1;
    expect(invocationRows([unknownStage])[0]?.role).toBeNull();
  });
});

describe("invocationTotals", () => {
  it("counts a scripted double's rows but sums nothing from them", () => {
    const totals = invocationTotals(invocationRows(fixture<RunDetailV1>("run-detail-retry.json").events));
    expect(totals).toEqual({
      calls: 0,
      scripted: 6,
      latencyMs: { sum: 0, reported: 0 },
      inputTokens: { sum: 0, reported: 0 },
      outputTokens: { sum: 0, reported: 0 },
      reasoningTokens: { sum: 0, reported: 0 },
      attempts: { sum: 0, reported: 0 },
    });
  });

  it("sums each field over the real calls that report it", () => {
    const totals = invocationTotals(invocationRows(metaDetail().events));
    // Six calls: latencies 1..6 s, input 100..600, output 10..60, one reasoning count.
    expect(totals.calls).toBe(6);
    expect(totals.scripted).toBe(0);
    expect(totals.latencyMs).toEqual({ sum: 21000, reported: 6 });
    expect(totals.inputTokens).toEqual({ sum: 2100, reported: 6 });
    expect(totals.outputTokens).toEqual({ sum: 210, reported: 6 });
    expect(totals.reasoningTokens).toEqual({ sum: 7, reported: 1 });
    expect(totals.attempts).toEqual({ sum: 0, reported: 0 });
  });
});

describe("operator-only fields", () => {
  it("are absent from every public fixture", () => {
    for (const name of ["retry", "offline", "invalid_proof", "injection", "abstain"]) {
      expect(operatorFieldsAbsent(invocationRows(fixture<RunDetailV1>(`run-detail-${name}.json`).events))).toBe(true);
    }
  });

  it("are noticed when a row carries one", () => {
    const detail = metaDetail();
    const payload = detail.events[1]?.payload as { invocation: ModelInvocationV1 };
    payload.invocation.schema_name = "proposal";
    expect(operatorFieldsAbsent(invocationRows(detail.events))).toBe(false);
  });
});
