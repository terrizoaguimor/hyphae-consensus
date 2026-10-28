// SPDX-License-Identifier: Apache-2.0
//
// The Meta API panel's rows: one per model invocation the run recorded, read
// from the events that carry them (proposals, challenges, evaluations,
// revisions, and a failed run's last call, as the runtime's aggregate
// records them), so each row knows its role and its log record. Totals are
// computed from the rows: real model calls only (a scripted double made no
// call), each with how many rows reported the field. The contract publishes
// no prices, so nothing here is a cost. Pure functions, unit-tested in
// tests/run-invocations.test.ts.

import type { AgentRole, EventV1, ModelInvocationV1 } from "../api/types";

export type InvocationRow = {
  sequence: number;
  /** The role whose document the call produced; null when the record does not say. */
  role: AgentRole | null;
  /** For a failed run's last call: the stage it failed at. */
  failedStage: string | null;
  invocation: ModelInvocationV1;
};

/** The roles of the runtime's stages (crates/consensus-runtime/src/orchestrator.rs). */
const STAGE_ROLES: Record<string, AgentRole> = {
  propose: "proposer",
  critique: "critic",
  evaluate: "evidence_evaluator",
  revise: "proposer",
};

function roleOf(event: EventV1, fallback: AgentRole): AgentRole {
  return event.actor.kind === "agent" ? event.actor.role : fallback;
}

/** Every recorded model invocation of the events, in log order. */
export function invocationRows(events: EventV1[]): InvocationRow[] {
  const rows: InvocationRow[] = [];
  for (const event of events) {
    const payload = event.payload;
    switch (payload.type) {
      case "proposal_submitted":
      case "revision_submitted":
        rows.push({
          sequence: event.sequence,
          role: roleOf(event, "proposer"),
          failedStage: null,
          invocation: payload.invocation,
        });
        break;
      case "challenge_submitted":
        rows.push({
          sequence: event.sequence,
          role: roleOf(event, "critic"),
          failedStage: null,
          invocation: payload.invocation,
        });
        break;
      case "evaluation_submitted":
        rows.push({
          sequence: event.sequence,
          role: roleOf(event, "evidence_evaluator"),
          failedStage: null,
          invocation: payload.invocation,
        });
        break;
      case "run_failed":
        if (payload.invocation !== undefined && payload.invocation !== null) {
          rows.push({
            sequence: event.sequence,
            role: STAGE_ROLES[payload.stage] ?? null,
            failedStage: payload.stage,
            invocation: payload.invocation,
          });
        }
        break;
      default:
        break;
    }
  }
  return rows;
}

/** A summed field: the sum over the rows that report it, and how many do. */
export type Total = { sum: number; reported: number };

export type InvocationTotals = {
  /** Rows of real model calls (not the scripted double). */
  calls: number;
  /** Rows of the scripted double: no model call was made. */
  scripted: number;
  latencyMs: Total;
  inputTokens: Total;
  outputTokens: Total;
  reasoningTokens: Total;
  attempts: Total;
};

function total(values: Array<number | null | undefined>): Total {
  let sum = 0;
  let reported = 0;
  for (const value of values) {
    if (typeof value === "number") {
      sum += value;
      reported += 1;
    }
  }
  return { sum, reported };
}

/** Totals over the real model calls among the rows; the scripted double's rows are only counted. */
export function invocationTotals(rows: InvocationRow[]): InvocationTotals {
  const calls = rows.filter((row) => !row.invocation.scripted).map((row) => row.invocation);
  return {
    calls: calls.length,
    scripted: rows.length - calls.length,
    latencyMs: total(calls.map((c) => c.latency_ms)),
    inputTokens: total(calls.map((c) => c.input_tokens)),
    outputTokens: total(calls.map((c) => c.output_tokens)),
    reasoningTokens: total(calls.map((c) => c.reasoning_tokens)),
    attempts: total(calls.map((c) => c.attempts)),
  };
}

/** Whether no row carries any of the fields only the operator surface publishes. */
export function operatorFieldsAbsent(rows: InvocationRow[]): boolean {
  return rows.every(({ invocation }) =>
    [
      invocation.max_completion_tokens,
      invocation.attempts,
      invocation.schema_name,
      invocation.schema_sha256,
      invocation.prompt_sha256,
    ].every((value) => value === undefined || value === null),
  );
}
