// SPDX-License-Identifier: Apache-2.0
//
// The explanation of a recommendation, from its policy trace
// (RecommendationV1.trace: the rule that decided, every failed check, the
// counts, the rule order) as catalog messages keyed by rule and reason
// (`Run.explain.*`), in plain words or with the technical fields. The plain
// explanation mentions exactly the trace's counts and its kind, nothing
// else (tests/run-explain.test.ts renders it in the four locales and checks
// both); what the trace does not carry, such as the objections earlier
// rounds raised and no later round judged again, is said apart from it,
// from the run's own records. Pure functions.

import type {
  PolicyRule,
  PolicyTrace,
  RecommendationKind,
  RunDetailV1,
  TraceCounts,
  TraceReason,
} from "../api/types";
import { roundedVerifications } from "./evidence";

/** One message under `Run.explain`; `kind` fills `{kind}` with the translated kind (`Run.kinds`). */
export type ExplainMessage = {
  key: string;
  values?: Record<string, string | number>;
  kind?: RecommendationKind;
};

export type PlainExplanation = {
  /** The deciding rule, with the kind it derives. */
  lead: ExplainMessage;
  /** The trace's counts. */
  counts: ExplainMessage[];
  /** One sentence per failed check, in the trace's order. */
  reasons: ExplainMessage[];
};

export const POLICY_RULES: readonly PolicyRule[] = [
  "invalid_proof",
  "recorded_contradiction",
  "abstention",
  "unresolved_items",
  "insufficient_evidence",
  "all_checks_passed",
];

export const TRACE_REASONS: ReadonlyArray<TraceReason["reason"]> = [
  "invalid_proofs",
  "contradicted_evidence",
  "evaluator_abstained",
  "open_items",
  "no_assessments",
  "uncorroborated_claims",
  "claims_without_own_evidence",
  "cited_evidence_not_valid",
  "no_checked_evidence",
];

export const COUNT_FIELDS: ReadonlyArray<keyof TraceCounts> = [
  "claims",
  "corroborated",
  "proofs_valid",
  "proofs_invalid",
  "proofs_unavailable_or_error",
  "unresolved_items",
];

/** The plain explanation of a trace: every number in it is one of the trace's counts. */
export function plainExplanation(trace: PolicyTrace): PlainExplanation {
  const { counts } = trace;
  return {
    lead: { key: `plain.rules.${trace.rule}`, kind: trace.kind },
    counts: [
      {
        key: "plain.counts.claims",
        values: { claims: counts.claims, corroborated: counts.corroborated },
      },
      {
        key: "plain.counts.proofs",
        values: {
          valid: counts.proofs_valid,
          invalid: counts.proofs_invalid,
          unavailable: counts.proofs_unavailable_or_error,
        },
      },
      { key: "plain.counts.open", values: { unresolved: counts.unresolved_items } },
    ],
    reasons: trace.reasons.map((reason) => ({ key: `plain.reasons.${reason.reason}` })),
  };
}

/** The ids a failed check names (evidence, claim or item ids), empty for the checks without ids. */
export function reasonIds(reason: TraceReason): string[] {
  switch (reason.reason) {
    case "invalid_proofs":
    case "contradicted_evidence":
    case "cited_evidence_not_valid":
      return reason.evidence_ids;
    case "open_items":
      return reason.item_ids;
    case "uncorroborated_claims":
    case "claims_without_own_evidence":
      return reason.claim_ids;
    case "evaluator_abstained":
    case "no_assessments":
    case "no_checked_evidence":
      return [];
  }
}

/** What the records add to the trace, said apart from it. */
export type ExplainContext = {
  /** Every verification record of the run is the scripted verifier's (and there is one). */
  simulatedProofs: boolean;
  /** The run was recorded before protocol 1.3: each round judged only its own objections, and the policy read the final round. */
  beforeCarryForward: boolean;
  /**
   * Objections raised in earlier rounds that no later round judged again
   * (the detail's challenge items with status never_rejudged), when the
   * data carries the question: a run before protocol 1.3 with more than one
   * round, or any run that has such items. Null otherwise.
   */
  neverRejudged: number | null;
};

/** Schema versions recorded before protocol 1.3's carry-forward. */
const BEFORE_CARRY_FORWARD = ["1.0", "1.1", "1.2"];

export function explainContext(detail: RunDetailV1): ExplainContext {
  const verifications = roundedVerifications(detail.events);
  const schema = detail.events[0]?.schema_version ?? detail.task?.schema_version ?? null;
  const beforeCarryForward = schema !== null && BEFORE_CARRY_FORWARD.includes(schema);
  const never = detail.challenge_items.filter((item) => item.status === "never_rejudged").length;
  return {
    simulatedProofs:
      verifications.length > 0 && verifications.every((v) => v.verification.simulated),
    beforeCarryForward,
    neverRejudged: never > 0 || (beforeCarryForward && detail.rounds.length > 1) ? never : null,
  };
}

/** The sentences the records add (`Run.explain.context.*`), in display order. */
export function contextMessages(context: ExplainContext): ExplainMessage[] {
  const out: ExplainMessage[] = [];
  if (context.simulatedProofs) {
    out.push({ key: "context.simulated" });
  }
  if (context.beforeCarryForward) {
    out.push({ key: "context.finalRoundOnly" });
  }
  if (context.neverRejudged !== null) {
    out.push({ key: "context.neverRejudged", values: { count: context.neverRejudged } });
  }
  return out;
}
