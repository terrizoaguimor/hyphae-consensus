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
] as const;

export type FailureCode = (typeof FAILURE_CODES)[number];

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
