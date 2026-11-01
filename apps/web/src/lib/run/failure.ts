// SPDX-License-Identifier: Apache-2.0
//
// A failed run is explained by a translated sentence chosen from its stable
// failure CODE, never by the server's English `message` (FailureV1 and
// RunFailed carry both; the message is a display copy for logs). The codes
// with their own sentence are every code `public_view::failure_message`
// knows, the live ledger's (`budget_refused`, `budget_unsettled`), the Meta
// adapter's provider codes and the runtime's model-output codes
// (crates/consensus-api/src/meta.rs `codes`,
// crates/consensus-runtime/src/orchestrator.rs `codes`). Any other code gets
// the generic sentence, which still names the code; a failure without a code
// names its stage. The same holds for an `evidence_unavailable` record.
// tests/run-failure.test.ts requires every sentence in every catalog.

import type { ProvenanceV1 } from "../api/types";

/** Failure codes with their own sentence, `Run.failure.codes.<code>`. */
export const FAILURE_CODES = [
  // public_view::failure_message
  "cancelled",
  "shutdown",
  "deadline",
  "interrupted",
  "panic",
  "model_timeout",
  "model_unavailable",
  "model_malformed_output",
  "evaluation_contract",
  "challenge_contract",
  "proposal_contract",
  "token_budget_exceeded",
  "token_usage_missing",
  "persistence",
  // The live ledger refused or could not settle a model call (meta.rs).
  "budget_refused",
  "budget_unsettled",
  // The Meta adapter's provider failures (meta.rs).
  "meta_unauthorized",
  "meta_billing",
  "meta_forbidden",
  "meta_bad_request",
  "meta_rate_limited",
  "meta_unavailable",
  "meta_timeout",
  "meta_transport",
  "meta_config",
  // The runtime's model-output codes (orchestrator.rs).
  "model_output_truncated",
  "model_refusal",
  // The model-robustness and provider lanes (round 8): a proposal without
  // claims, a filtered or otherwise unfinished answer, a request the server
  // could not build, an answer that is not a chat completion, a call not
  // made because the run's deadline was too close.
  "model_no_claims",
  "model_content_filtered",
  "model_unexpected_finish",
  "adapter_schema_error",
  "meta_bad_envelope",
  "run_deadline_reached",
] as const;

export type FailureCode = (typeof FAILURE_CODES)[number];

/**
 * What a failure means for the next run, by code (`Run.failure.next.<class>`):
 * - transient: Meta's service or the model ended the run (or the server
 *   did: an interruption, a shutdown, a deadline too close for the next
 *   call); the same preset can succeed on a new run, and the live gate gives
 *   the daily count back (crates/consensus-api/src/live/gate.rs
 *   GIVE_BACK_CODES is the same list);
 * - protocol: the protocol refused a document on purpose (every
 *   `*_contract` code);
 * - closed: starting again may not work right now (the budget, a refused
 *   key, billing, configuration, persistence, an internal error);
 * - cancelled and deadline: their own sentences.
 * tests/run-failure.test.ts requires every FAILURE_CODES entry in exactly
 * one class.
 */
export const FAILURE_CLASSES = {
  transient: [
    "meta_timeout",
    "meta_unavailable",
    "meta_transport",
    "meta_rate_limited",
    "meta_bad_envelope",
    "model_timeout",
    "model_unavailable",
    "model_malformed_output",
    "model_no_claims",
    "model_output_truncated",
    "model_refusal",
    "model_content_filtered",
    "model_unexpected_finish",
    "interrupted",
    "shutdown",
    "run_deadline_reached",
  ],
  protocol: ["evaluation_contract", "challenge_contract", "proposal_contract"],
  closed: [
    "budget_refused",
    "budget_unsettled",
    "token_budget_exceeded",
    "token_usage_missing",
    "meta_unauthorized",
    "meta_billing",
    "meta_forbidden",
    "meta_bad_request",
    "meta_config",
    "adapter_schema_error",
    "persistence",
    "panic",
  ],
  cancelled: ["cancelled"],
  deadline: ["deadline"],
} as const satisfies Record<string, readonly FailureCode[]>;

export type FailureClass = keyof typeof FAILURE_CLASSES;

/** The class of a failure code; null for a code this client does not know (or none). */
export function failureClass(code: string | null | undefined): FailureClass | null {
  if (code === null || code === undefined || code === "") {
    return null;
  }
  for (const [name, codes] of Object.entries(FAILURE_CLASSES) as Array<[FailureClass, readonly string[]]>) {
    if (codes.includes(code)) {
      return name;
    }
  }
  return null;
}

/** Stages with a name of their own (`Run.failure.stageNames.<stage>`). */
export const FAILURE_STAGES = [
  "propose",
  "critique",
  "evaluate",
  "revise",
  "token_budget",
  "deadline",
  "shutdown",
  // A run a server restart interrupted is closed as Failed{stage:
  // "interrupted"} (consensus-runtime orchestrator.rs, recovery).
  "interrupted",
] as const;

export type FailureStage = (typeof FAILURE_STAGES)[number];

/** The stage's own name key, or null: the panel then shows the raw stage. */
export function stageNameKey(stage: string): `stageNames.${FailureStage}` | null {
  return (FAILURE_STAGES as readonly string[]).includes(stage)
    ? `stageNames.${stage as FailureStage}`
    : null;
}

/** The role whose call a stage is (`Run.stage.<role>`). */
export type FailureRole = "proposer" | "critic" | "evaluator" | "runtime";

/** propose and revise: the Proposer; critique: the Critic; evaluate: the Evaluator; anything else: the runtime. */
export function stageRole(stage: string): FailureRole {
  switch (stage) {
    case "propose":
    case "revise":
      return "proposer";
    case "critique":
      return "critic";
    case "evaluate":
      return "evaluator";
    default:
      return "runtime";
  }
}

/**
 * Codes whose panel sentence names the role (`Run.failure.roleCodes.<code>`,
 * with the role as its subject, `Run.failure.subjects.<role>`); lists keep
 * the role-free `codes.<code>`.
 */
export const ROLE_CODES = [
  "model_malformed_output",
  "model_no_claims",
  "model_content_filtered",
  "model_unexpected_finish",
] as const satisfies readonly FailureCode[];

export type RoleCode = (typeof ROLE_CODES)[number];

/** What the failure panel says, as catalog keys under `Run.failure` (never the server's reason). */
export type FailurePanel = {
  /** The code's sentence: the role-naming one when the stage is a role's call. */
  sentence:
    | FailureSentence
    | { key: `roleCodes.${RoleCode}`; values: { role: FailureRole } };
  /** Where it stopped: the stage's own name (or null: the raw stage) and its role; null for a cancellation. */
  where: { stageName: `stageNames.${FailureStage}` | null; stage: string; role: FailureRole } | null;
  /** The class of the code; null for an unknown code. */
  next: FailureClass | null;
  /**
   * The next-step sentence of the class (`Run.failure.<nextKey>`): a closed
   * failure of a live run points to the Live presets page (`next.closedLive`),
   * any other closed failure keeps the neutral `next.closed`.
   */
  nextKey: `next.${Exclude<FailureClass, "closed">}` | "next.closed" | "next.closedLive" | null;
  /** The live gate gave (or may give) the daily count back. */
  notCounted: boolean;
};

/**
 * What kind of visitor run this is, from its declared mode: a live run of
 * the public surface (Meta's model on a curated preset: the daily count and
 * the public budget apply), a guided example, or neither (an operator run,
 * a recorded run of the archive, a run that declares nothing).
 */
export function visitorRunKind(provenance: Pick<ProvenanceV1, "source" | "declared">): "live" | "guided" | null {
  const declared = provenance.declared ?? null;
  if (provenance.source !== "this_server" || declared === null || declared.origin !== "public") {
    return null;
  }
  return declared.model === "meta" ? "live" : "guided";
}

/**
 * Where a visitor starts another run after a failed one: the live form with
 * the same preset preselected (`?preset=`, which the form honours only for a
 * preset the server lists), or the guided examples. Nothing else is carried
 * over: the attestation, the Turnstile token and the invite code are always
 * the visitor's to give again.
 */
export function againPath(
  newPath: string,
  provenance: Pick<ProvenanceV1, "source" | "declared">,
): { kind: "live" | "guided"; href: string } | null {
  const kind = visitorRunKind(provenance);
  if (kind === "guided") {
    return { kind, href: `${newPath}#guided-heading` };
  }
  if (kind === "live") {
    const preset = provenance.declared?.preset ?? null;
    const query = preset === null || preset === "" ? "" : `?preset=${encodeURIComponent(preset)}`;
    return { kind, href: `${newPath}${query}#live-heading` };
  }
  return null;
}

/** The failure panel of a failed run (`live`: a live run of the public surface). */
export function failurePanel(failure: { code?: string | null; stage: string }, live: boolean): FailurePanel {
  const code = failure.code ?? null;
  const role = stageRole(failure.stage);
  const roleCode =
    role !== "runtime" && code !== null && (ROLE_CODES as readonly string[]).includes(code)
      ? (code as RoleCode)
      : null;
  const next = failureClass(code);
  return {
    sentence:
      roleCode === null
        ? failureSentence(failure)
        : { key: `roleCodes.${roleCode}`, values: { role } },
    where:
      next === "cancelled"
        ? null
        : { stageName: stageNameKey(failure.stage), stage: failure.stage, role },
    next,
    nextKey: next === null ? null : next === "closed" && live ? "next.closedLive" : `next.${next}`,
    notCounted: live && next === "transient",
  };
}

export type FailureSentence =
  | { key: `codes.${FailureCode}`; values: Record<string, never> }
  | { key: "unknownCode"; values: { code: string } }
  | { key: "noCode"; values: { stage: string } };

/** The sentence (a key under `Run.failure`) for a failure's code and stage. */
export function failureSentence(failure: {
  code?: string | null;
  stage: string;
}): FailureSentence {
  const code = failure.code ?? null;
  if (code === null || code === "") {
    return { key: "noCode", values: { stage: failure.stage } };
  }
  if ((FAILURE_CODES as readonly string[]).includes(code)) {
    return { key: `codes.${code as FailureCode}`, values: {} };
  }
  return { key: "unknownCode", values: { code } };
}

/**
 * Codes of an `evidence_unavailable` record with their own sentence
 * (`Run.unavailable.codes.<code>`), as `public_view::unavailable_message`
 * knows them.
 */
export const UNAVAILABLE_CODES = ["memory_unavailable", "memory_error", "evidence_cap"] as const;

export type UnavailableCode = (typeof UNAVAILABLE_CODES)[number];

export type UnavailableSentence =
  | { key: `codes.${UnavailableCode}`; values: Record<string, never> }
  | { key: "unknownCode"; values: { code: string } }
  | { key: "noCode"; values: Record<string, never> };

/** The sentence (a key under `Run.unavailable`) for an evidence_unavailable record. */
export function unavailableSentence(code: string | null | undefined): UnavailableSentence {
  if (code === undefined || code === null || code === "") {
    return { key: "noCode", values: {} };
  }
  if ((UNAVAILABLE_CODES as readonly string[]).includes(code)) {
    return { key: `codes.${code as UnavailableCode}`, values: {} };
  }
  return { key: "unknownCode", values: { code } };
}
