// SPDX-License-Identifier: Apache-2.0
//
// The page's own consistency checks: where the run's data contradicts a
// label shown for it, said once as a callout instead of left as one more
// equal tag (heuristic evaluation, problem 7). None of this adds, removes or
// changes a label: the labels stay those of src/lib/provenance.ts, and the
// system's own PROVENANCE INCONSISTENT flag is the server's, not this check's.
//
// - Labels that disagree with the steps: the model axis names Meta's model
//   while recorded invocations are the scripted double (or the other way
//   round), or the evidence axis names the shared memory while recorded
//   proofs are simulated (or the other way round).
// - A receipt that disagrees with the run: its final state or its event
//   count differs from the server's copy of the run (the detail), once both
//   are settled: the run at rest, or the receipt claiming a final state, and
//   no newer receipt being read.
//
// Pure functions, unit-tested in tests/run-consistency.test.ts.

import type { EventV1, ProvenanceV1, RunReceiptV2, RunSummaryV1 } from "../api/types";
import { isAtRest, isTerminal } from "../provenance";
import { roundedVerifications } from "./evidence";
import { invocationRows } from "./invocations";

/** One way the labels disagree with the run's own records (`Run.consistency.labels.<kind>`). */
export type LabelDisagreement =
  /** The model axis is Meta's model; `count` recorded invocations are the scripted double. */
  | { kind: "metaModelScriptedSteps"; count: number }
  /** The model axis is scripted; `count` recorded invocations are model calls. */
  | { kind: "scriptedModelCalls"; count: number }
  /** The evidence axis is the shared memory; `count` recorded proofs are simulated. */
  | { kind: "memoryEvidenceSimulatedProofs"; count: number }
  /** The evidence axis is simulated; `count` recorded proofs are not simulated and name the verifier that checked them. */
  | { kind: "simulatedEvidenceRealProofs"; count: number };

export const LABEL_DISAGREEMENT_KINDS: readonly LabelDisagreement["kind"][] = [
  "metaModelScriptedSteps",
  "scriptedModelCalls",
  "memoryEvidenceSimulatedProofs",
  "simulatedEvidenceRealProofs",
];

/** Where the run's provenance labels disagree with its recorded steps and proofs; empty when they agree. */
export function labelDisagreements(
  provenance: Pick<ProvenanceV1, "model" | "evidence">,
  events: EventV1[],
): LabelDisagreement[] {
  const out: LabelDisagreement[] = [];
  const invocations = invocationRows(events).map((row) => row.invocation);
  const scripted = invocations.filter((invocation) => invocation.scripted).length;
  const calls = invocations.length - scripted;
  if (provenance.model === "live_meta" && scripted > 0) {
    out.push({ kind: "metaModelScriptedSteps", count: scripted });
  }
  if (provenance.model === "scripted" && calls > 0) {
    out.push({ kind: "scriptedModelCalls", count: calls });
  }
  const proofs = roundedVerifications(events).map(({ verification }) => verification);
  const simulated = proofs.filter((verification) => verification.simulated).length;
  // A real verifier's proof names its verifier: a verification no verifier
  // ran (the proof unavailable, verifier absent) is not simulated, and not a
  // real verifier's either.
  const real = proofs.filter(
    (verification) => !verification.simulated && typeof verification.verifier === "string" && verification.verifier !== "",
  ).length;
  if ((provenance.evidence === "hyphae_snapshot" || provenance.evidence === "hyphae_live") && simulated > 0) {
    out.push({ kind: "memoryEvidenceSimulatedProofs", count: simulated });
  }
  if (provenance.evidence === "simulated" && real > 0) {
    out.push({ kind: "simulatedEvidenceRealProofs", count: real });
  }
  return out;
}

/** One field on which the receipt and the run disagree, with both values. */
export type ReceiptDisagreement =
  | { field: "finalState"; receipt: RunReceiptV2["final_state"]; run: RunSummaryV1["state"] }
  | { field: "eventCount"; receipt: number; run: number };

/**
 * Where the receipt read for a run disagrees with the server's copy of the
 * run (the detail's summary). Compared only once both are settled: no newer
 * receipt is being read (`pending`), and either the run is at rest (nothing
 * more is published until a human acts) or the receipt claims a final state
 * (approved, rejected, failed), which a run never leaves. A run still moving
 * can be read a little before or after its receipt, so it is not compared.
 */
export function receiptDisagreements(
  receipt: Pick<RunReceiptV2, "final_state" | "event_count">,
  run: Pick<RunSummaryV1, "state" | "active" | "event_count">,
  pending: boolean,
): ReceiptDisagreement[] {
  if (pending || !(isAtRest(run.state, run.active) || isTerminal(receipt.final_state))) {
    return [];
  }
  const out: ReceiptDisagreement[] = [];
  if (receipt.final_state !== run.state) {
    out.push({ field: "finalState", receipt: receipt.final_state, run: run.state });
  }
  if (receipt.event_count !== run.event_count) {
    out.push({ field: "eventCount", receipt: receipt.event_count, run: run.event_count });
  }
  return out;
}
