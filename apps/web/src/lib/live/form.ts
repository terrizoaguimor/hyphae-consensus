// SPDX-License-Identifier: Apache-2.0
//
// The rules of the live form (New deliberation, `POST /api/v1/live-runs`),
// from what the server publishes in `capabilities.live` (LiveCapabilitiesV1)
// and what this build carries (the Turnstile site key), so the form never
// sends a request it knows the server must refuse. Unit-tested in
// tests/live-form.test.ts.
//
// - The form is CLOSED (every control disabled, one translated sentence)
//   when the ledger is tripped, when the server could not read it (no budget
//   published: no run can be admitted), when there is no preset, or when the
//   server requires Turnstile and this build has no site key (every request
//   would be a guaranteed 403 turnstile_required).
// - While it is open, Start stays disabled until a preset is chosen, the
//   attestation is given (when required), the invite code fits the contract
//   (1 to 256 characters once trimmed, or none) and a Turnstile token exists
//   (when required). A token is single-use: the gate spends it at Siteverify
//   before reserving the budget, so every submit consumes the token and asks
//   the widget for a fresh one (src/lib/live/turnstile.ts).

import { judgeCodeForRequest } from "../api/client";
import type { LiveCapabilitiesV1, PublicBudgetV1 } from "../api/types";

/** Why the whole live form is closed. */
export type LiveClosedReason = "tripped" | "budget_unknown" | "no_presets" | "turnstile_unconfigured";

export function liveClosedReason(
  live: LiveCapabilitiesV1,
  siteKey: string | undefined,
): LiveClosedReason | null {
  const budget: PublicBudgetV1 | null = live.budget ?? null;
  if (budget !== null && budget.tripped) {
    return "tripped";
  }
  if (budget === null) {
    return "budget_unknown";
  }
  if (live.presets.length === 0) {
    return "no_presets";
  }
  if (live.requires_turnstile && (siteKey === undefined || siteKey === "")) {
    return "turnstile_unconfigured";
  }
  return null;
}

/** Why Start is disabled while the form is open. */
export type StartBlocker = "starting" | "preset" | "attestation" | "judge_code" | "turnstile";

export type StartInput = {
  live: LiveCapabilitiesV1;
  presetId: string;
  attested: boolean;
  judgeCode: string;
  token: string | null;
  starting: boolean;
};

export function startBlocker(input: StartInput): StartBlocker | null {
  if (input.starting) {
    return "starting";
  }
  if (!input.live.presets.some((preset) => preset.id === input.presetId)) {
    return "preset";
  }
  if (input.live.requires_adult_attestation && !input.attested) {
    return "attestation";
  }
  if (judgeCodeForRequest(input.judgeCode) === null) {
    return "judge_code";
  }
  if (input.live.requires_turnstile && (input.token === null || input.token === "")) {
    return "turnstile";
  }
  return null;
}

/** Trip codes with their own sentence (`NewDeliberation.live.trip.<code>`), PublicBudgetV1.trip_code. */
export const TRIP_CODES = [
  "kill_switch",
  "lifetime_cap",
  "output_bound",
  "charge_bound",
  "provider_refused_key",
  "inconsistent",
] as const;

export type TripCode = (typeof TRIP_CODES)[number];

export function isTripCode(code: string | null | undefined): code is TripCode {
  return typeof code === "string" && (TRIP_CODES as readonly string[]).includes(code);
}

/**
 * A ledger amount (USD, as the ledger renders it: digits and a six-decimal
 * fraction) as a number for formatting, or null when the string is not such
 * an amount (it is then shown as sent, never guessed).
 */
export function parseUsd(value: string): number | null {
  if (!/^\d+(\.\d+)?$/.test(value)) {
    return null;
  }
  const amount = Number(value);
  return Number.isFinite(amount) ? amount : null;
}
