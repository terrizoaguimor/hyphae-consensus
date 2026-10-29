// SPDX-License-Identifier: Apache-2.0
//
// The explanation of a recommendation, from its policy trace
// (RecommendationV1.trace: the rule that decided, every failed check, the
// counts, the rule order) as catalog messages keyed by rule and reason
// (`Run.explain.*`), in plain words or with the technical fields. The plain
// explanation mentions exactly the trace's counts and its kind, nothing
// else (tests/run-explain.test.ts renders it in the four locales and checks
// both); what the trace does not carry, such as the objections earlier
// rounds raised and no later round judged again (and how many of them even
// their own round left unlisted), is said apart from it, from the run's own
// records. The records also say how to word one check: when every record
// the invalid_proofs check names reports that no proof could be generated
// upstream, the lead, the proofs count and that sentence say so (nothing was
// checked; the counts are integrity statuses), never
// that a check came back invalid. Pure functions.

import type {
  ChallengeItemV1,
  PolicyRule,
  PolicyTrace,
  RecommendationKind,
  RunDetailV1,
  TraceCounts,
  TraceReason,
} from "../api/types";
import { evidenceView, proofReading, roundedVerifications } from "./evidence";

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

/** What the records say about the trace's failed checks, beyond the trace itself. */
export type TraceReading = {
  /**
   * Every record the invalid_proofs check names reports that no proof could
   * be generated upstream (ungeneratedProofs): its invalid status records
   * that failure, and nothing was checked.
   */
  proofsUngenerated: boolean;
};

/**
 * The plain explanation of a trace: every number in it is one of the trace's
 * counts. With `reading.proofsUngenerated`, the invalid_proof lead, the
 * proofs count sentence and the invalid_proofs sentence say that no proof
 * could be generated (so nothing was checked), instead of a check that came
 * back invalid.
 */
export function plainExplanation(trace: PolicyTrace, reading?: TraceReading): PlainExplanation {
  const { counts } = trace;
  const ungenerated = reading?.proofsUngenerated === true && invalidProofIds(trace).length > 0;
  return {
    lead: {
      key: ungenerated && trace.rule === "invalid_proof" ? "plain.rules.invalid_proof_ungenerated" : `plain.rules.${trace.rule}`,
      kind: trace.kind,
    },
    counts: [
      {
        key: "plain.counts.claims",
        values: { claims: counts.claims, corroborated: counts.corroborated },
      },
      {
        // The same three trace counts; with no proof generated they are
        // integrity statuses, not proofs that were checked or that failed.
        key: ungenerated ? "plain.counts.proofs_ungenerated" : "plain.counts.proofs",
        values: {
          valid: counts.proofs_valid,
          invalid: counts.proofs_invalid,
          unavailable: counts.proofs_unavailable_or_error,
        },
      },
      { key: "plain.counts.open", values: { unresolved: counts.unresolved_items } },
    ],
    reasons: trace.reasons.map((reason) => ({
      key:
        ungenerated && reason.reason === "invalid_proofs"
          ? "plain.reasons.invalid_proofs_ungenerated"
          : `plain.reasons.${reason.reason}`,
    })),
  };
}

/** The evidence ids the trace's invalid_proofs check names (empty when it did not fail). */
function invalidProofIds(trace: PolicyTrace): string[] {
  return trace.reasons.flatMap((reason) => (reason.reason === "invalid_proofs" ? reason.evidence_ids : []));
}

/**
 * Whether the trace's invalid proofs are all generation failures: the
 * invalid_proofs check names at least one record, and every record it names
 * (an evidence id the policy counted with integrity invalid,
 * consensus-runtime policy.rs `explain`) was retrieved, every retrieval of
 * it reports a proof error, and every invalid verification of it reads as
 * a generation failure (proofReading: a real verifier's status, never the
 * scripted verifier's). The runtime then recorded invalid because the
 * proven recall failed upstream (orchestrator.rs upstream_proof_check):
 * no verifier ran, and nothing was checked.
 */
export function ungeneratedProofs(trace: PolicyTrace, detail: RunDetailV1): boolean {
  const ids = invalidProofIds(trace);
  if (ids.length === 0) {
    return false;
  }
  const view = evidenceView(detail, []);
  return ids.every((id) => {
    if (view.orphanVerifications.some((v) => v.verification.evidence_id === id)) {
      return false;
    }
    const records = view.records.filter((r) => r.evidence.id === id);
    const invalid = records.flatMap((r) =>
      r.verifications
        .filter((v) => v.verification.integrity_status === "invalid")
        .map((v) => proofReading(v.verification, r.evidence).kind),
    );
    return (
      records.length > 0 &&
      records.every((r) => r.evidence.proof_error_reported) &&
      invalid.length > 0 &&
      invalid.every((kind) => kind === "generation_failed")
    );
  });
}

/** The trace's reading from the run's records. */
export function traceReading(trace: PolicyTrace, detail: RunDetailV1): TraceReading {
  return { proofsUngenerated: ungeneratedProofs(trace, detail) };
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
  /**
   * Of those, the ones the evaluation of their own round did not list as
   * unresolved either (RunReceiptV2 UnrejudgedObjection
   * unresolved_in_its_round false): left out without a stated reason, which
   * before protocol 1.3 counted as resolved. Present with neverRejudged.
   */
  neverRejudgedUnlisted?: number;
};

/** Schema versions recorded before protocol 1.3's carry-forward. */
const BEFORE_CARRY_FORWARD = ["1.0", "1.1", "1.2"];

/**
 * Whether the evaluation of the round that raised `item` listed it as
 * unresolved, as consensus-runtime receipts.rs `lists` reads it: by the
 * item's id, or by its challenge's id for a pre-1.1 item (no id), whose
 * evaluations listed challenge ids. False when that round has no
 * evaluation or no challenge on record.
 */
function listedInItsRound(detail: RunDetailV1, item: ChallengeItemV1): boolean {
  const round = detail.rounds.find((r) => r.round === item.raised_in_round);
  const evaluation = round?.evaluation ?? null;
  const challenge = round?.challenge ?? null;
  if (evaluation === null || challenge === null) {
    return false;
  }
  const id = item.id === "" ? challenge.id : item.id;
  return (evaluation.unresolved_items ?? []).includes(id);
}

export function explainContext(detail: RunDetailV1): ExplainContext {
  const verifications = roundedVerifications(detail.events);
  const schema = detail.events[0]?.schema_version ?? detail.task?.schema_version ?? null;
  const beforeCarryForward = schema !== null && BEFORE_CARRY_FORWARD.includes(schema);
  const neverItems = detail.challenge_items.filter((item) => item.status === "never_rejudged");
  const never = neverItems.length;
  const shown = never > 0 || (beforeCarryForward && detail.rounds.length > 1);
  return {
    simulatedProofs:
      verifications.length > 0 && verifications.every((v) => v.verification.simulated),
    beforeCarryForward,
    neverRejudged: shown ? never : null,
    ...(shown
      ? { neverRejudgedUnlisted: neverItems.filter((item) => !listedInItsRound(detail, item)).length }
      : {}),
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
    // Said when the two readings of the figure part: some of these
    // objections were not listed unresolved even in their own round.
    const unlisted = context.neverRejudgedUnlisted ?? 0;
    if (unlisted > 0) {
      out.push({ key: "context.neverRejudgedUnlisted", values: { count: unlisted } });
    }
  }
  return out;
}
