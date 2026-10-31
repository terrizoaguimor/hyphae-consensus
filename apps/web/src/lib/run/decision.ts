// SPDX-License-Identifier: Apache-2.0
//
// Where the human decision sits in the run workspace, and how its form is
// worded, computed from the run's own data. Pure functions, unit-tested in
// tests/run-decision.test.ts.
//
// - A run awaits a decision here only while its state is
//   awaiting_human_approval, it is a run of this server (a recorded run of
//   the research archive is read-only) and no decision is recorded yet; the
//   header then offers "Go to your decision" on every tab.
// - The Decision tab comes right after the first tab of an own run (the
//   stage), and first for a recorded run, whose default tab it is.
// - Approving a blocked or abstained run is an override of the policy's
//   recommendation: the Approve button says so ("Approve as an override"),
//   the note is marked required for this run before any click, and neither
//   button is promoted over the other (both keep the same variant on every
//   run; the buttons never take the policy's side).

import type { DecisionV1, RecommendationKind, RunState, RunSummaryV1 } from "../api/types";

export type RunTabKey = "stage" | "explorer" | "evidence" | "proofs" | "audit" | "meta" | "decision";

/** The kinds whose approval is an override and needs a note. */
export const OVERRIDE_KINDS: ReadonlySet<RecommendationKind> = new Set<RecommendationKind>([
  "blocked",
  "abstained",
]);

/** True when approving a run with this recommendation is an override. */
export function isOverride(kind: RecommendationKind | null | undefined): boolean {
  return kind !== null && kind !== undefined && OVERRIDE_KINDS.has(kind);
}

/**
 * True while the run awaits a human decision that can be recorded here: its
 * state is awaiting_human_approval, it is a run of this server, and no
 * decision is recorded for it (neither in its detail nor just sent).
 */
export function awaitsDecision(
  summary: { state: RunState; provenance: Pick<RunSummaryV1["provenance"], "source"> } | null | undefined,
  decision: DecisionV1 | null | undefined,
): boolean {
  return (
    summary !== null &&
    summary !== undefined &&
    summary.state === "awaiting_human_approval" &&
    summary.provenance.source === "this_server" &&
    (decision === null || decision === undefined)
  );
}

/** The tabs of the run workspace, in order: Decision right after the first tab. */
export function runTabOrder(recorded: boolean): RunTabKey[] {
  return recorded
    ? ["decision", "explorer", "evidence", "proofs", "audit", "meta"]
    : ["stage", "decision", "explorer", "evidence", "proofs", "audit", "meta"];
}

/** The wording of the decision form, by recommendation (keys of Run.decision). */
export type DecisionFormCopy = {
  /** The Approve button's label. */
  approve: "approve" | "approveOverride";
  /** The note's label: "Note" (optional) or "Note · Required for this run". */
  noteLabel: "note" | "noteRequired";
  /** The note's description. */
  noteDescription: "noteDescription" | "noteRequiredDescription";
  /** Whether the note is marked optional (never on an override run). */
  noteOptional: boolean;
  /** The override line shown above the note before any click (override runs only). */
  overrideNotice: "overrideNotice" | null;
};

/**
 * The one variant both Approve and Reject take, on every run: equal weight
 * is structural (a single constant), not a per-button choice.
 */
export const DECIDE_BUTTON_VARIANT = "secondary" as const;

export function decisionFormCopy(kind: RecommendationKind | null | undefined): DecisionFormCopy {
  const override = isOverride(kind);
  return {
    approve: override ? "approveOverride" : "approve",
    noteLabel: override ? "noteRequired" : "note",
    noteDescription: override ? "noteRequiredDescription" : "noteDescription",
    noteOptional: !override,
    overrideNotice: override ? "overrideNotice" : null,
  };
}
