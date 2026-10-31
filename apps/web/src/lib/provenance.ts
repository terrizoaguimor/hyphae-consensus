// SPDX-License-Identifier: Apache-2.0
//
// The honesty predicates (release gate G7) and the label vocabulary every
// view uses for provenance. Labels are message keys under `Provenance` with a
// badge variant; the text is what carries the meaning, color only repeats it.
// Rules, each with a unit test (tests/provenance.test.ts):
//
// - LIVE only when the frame's delivery is `live` AND the model axis is
//   `live_meta` AND no flag is raised; a `provenance_inconsistent`,
//   `unchained` or `mode_undeclared` flag never renders as LIVE. Every label
//   with a LIVE word (the LIVE delivery badge, the LIVE META model badge, the
//   "Meta model · live" note, the Live tab) goes through `isLive`; a view
//   without a frame (a list, a preset) never shows one.
// - A replayed frame of a live-model run reads "Meta model · recorded <date>"
//   next to REPLAY and the model badge META MODEL; a scripted model reads
//   "scripted double, no model call".
// - Client-side pacing ("simulated · paced for readability") only for the
//   replayed frames of a scripted run (`isPaceable`), never for a live or
//   mixed model run.
// - Simulated evidence is always named next to the outcome, as its own tag
//   ("supported" + "Evidence: SIMULATED", components/OutcomeTags), and a
//   simulated proof is never a green "valid": it gets the neutral SIMULATED
//   token with its scripted outcome as text.
// - Hyphae cyan stays reserved for proof and evidence: no badge here uses the
//   cyan or teal variants.

import type { BadgeVariant } from "@astryxdesign/core/Badge";

import type {
  Delivery,
  EvidenceProvenance,
  IntegrityStatus,
  ModelProvenance,
  ProvenanceFlag,
  ProvenanceV1,
  RecommendationKind,
  RunState,
  SourceProvenance,
} from "./api/types";

export type ProvenanceLabel = {
  /** `Provenance.<key>` in the message catalogs. */
  key: string;
  variant: BadgeVariant;
};

/** Flags that must be shown and that forbid the LIVE label. */
export const BLOCKING_FLAGS: readonly ProvenanceFlag[] = [
  "provenance_inconsistent",
  "unchained",
  "mode_undeclared",
];

export function hasBlockingFlag(provenance: Pick<ProvenanceV1, "flags">): boolean {
  return (provenance.flags ?? []).some((flag) => BLOCKING_FLAGS.includes(flag));
}

/**
 * LIVE: the record was published while this client was connected, every
 * recorded model call went to the Meta Model API, and nothing about the
 * run's provenance is in doubt.
 */
export function isLive(
  delivery: Delivery,
  provenance: Pick<ProvenanceV1, "model" | "flags">,
): boolean {
  return (
    delivery === "live" &&
    provenance.model === "live_meta" &&
    !hasBlockingFlag(provenance)
  );
}

/**
 * The model axis as a label. The LIVE-worded LIVE META only when `isLive`
 * holds for the delivery given (the latest frame of a run being watched);
 * without a delivery (a list entry, a preset) or for a replayed frame it is
 * META MODEL, and with a blocking flag META MODEL in the warning variant. No
 * other label of the model axis carries a LIVE word in any locale
 * (tests/provenance.test.ts). Callers always place the evidence label next
 * to it.
 */
export function modelLabel(
  provenance: Pick<ProvenanceV1, "model" | "flags">,
  delivery?: Delivery,
): ProvenanceLabel {
  switch (provenance.model) {
    case "live_meta":
      if (delivery !== undefined && isLive(delivery, provenance)) {
        return { key: "liveMeta.label", variant: "info" };
      }
      return {
        key: "metaModel.label",
        variant: hasBlockingFlag(provenance) ? "warning" : "info",
      };
    case "scripted":
      return { key: "scripted.label", variant: "purple" };
    case "mixed":
      return { key: "mixedModel.label", variant: "warning" };
    case "undetermined":
      return { key: "undetermined.label", variant: "neutral" };
  }
}

/** Every model axis value, for tests and exhaustive views. */
export const MODEL_PROVENANCES: readonly ModelProvenance[] = [
  "scripted",
  "live_meta",
  "mixed",
  "undetermined",
];

export function evidenceLabel(evidence: EvidenceProvenance): ProvenanceLabel {
  switch (evidence) {
    case "simulated":
      return { key: "simulated.label", variant: "purple" };
    case "hyphae_snapshot":
      return { key: "liveMeta.evidence", variant: "neutral" };
    case "hyphae_live":
      return { key: "hyphaeLive.label", variant: "neutral" };
    case "none":
      return { key: "noEvidence.label", variant: "neutral" };
    case "mixed":
      return { key: "mixedEvidence.label", variant: "warning" };
  }
}

export function sourceLabel(source: SourceProvenance): ProvenanceLabel {
  return source === "research_archive"
    ? { key: "recorded.label", variant: "neutral" }
    : { key: "thisServer.label", variant: "neutral" };
}

export function flagLabel(flag: ProvenanceFlag): ProvenanceLabel {
  switch (flag) {
    case "provenance_inconsistent":
      return { key: "flags.provenance_inconsistent", variant: "warning" };
    case "unchained":
      return { key: "flags.unchained", variant: "warning" };
    case "mode_undeclared":
      return { key: "flags.mode_undeclared", variant: "warning" };
  }
}

/**
 * The delivery axis of one frame. `live` only passes `isLive`; a record
 * published while connected for any other run is STREAMING (it is not a
 * replay, and it is not a live model); a stored record is REPLAY.
 */
export function deliveryLabel(
  delivery: Delivery,
  provenance: Pick<ProvenanceV1, "model" | "flags">,
): ProvenanceLabel {
  if (delivery === "replay") {
    return { key: "replay.label", variant: "blue" };
  }
  return isLive(delivery, provenance)
    ? { key: "live.label", variant: "success" }
    : { key: "streaming.label", variant: "blue" };
}

/**
 * The sentence under a model label: "Meta model · live" only when `isLive`;
 * for a live model that is replayed, listed or flagged, "Meta model ·
 * recorded <date>" (the date is the record's own); for a scripted double,
 * that no model was called. Without a delivery (a list) a live model is
 * always "recorded".
 */
export function modelNoteKey(
  delivery: Delivery | undefined,
  provenance: Pick<ProvenanceV1, "model" | "flags">,
): "liveMeta.recorded" | "scripted.note" | "liveMeta.live" | null {
  if (provenance.model === "scripted") {
    return "scripted.note";
  }
  if (provenance.model === "live_meta") {
    return delivery !== undefined && isLive(delivery, provenance)
      ? "liveMeta.live"
      : "liveMeta.recorded";
  }
  return null;
}

/**
 * A run whose roles are scripted doubles: by its records (model axis
 * `scripted`), or, before any model call is recorded, by the mode it was
 * declared with (a guided run is declared `scripted`). A live, mixed or
 * live-declared run is never one.
 */
export function isScriptedRun(provenance: Pick<ProvenanceV1, "model" | "declared">): boolean {
  if (provenance.model === "scripted") {
    return true;
  }
  return provenance.model === "undetermined" && provenance.declared?.model === "scripted";
}

/**
 * Whether a frame may be paced client-side and shown under "simulated ·
 * paced for readability": a replayed frame of a scripted run, nothing else.
 * Live frames are never delayed, and a live-model run is never labelled
 * simulated.
 */
export function isPaceable(
  delivery: Delivery,
  provenance: Pick<ProvenanceV1, "model" | "declared">,
): boolean {
  return delivery === "replay" && isScriptedRun(provenance);
}

/**
 * Whether the public surface publishes the run's verbatim log
 * (`events.ndjson`), mirroring the server's rule
 * (`public_view::ndjson_publishable`): declared with a scripted model over
 * simulated or no evidence, and recording nothing else. Any other run
 * answers `404 ndjson_unavailable`; its detail and receipt carry the
 * sanitized view.
 */
export function ndjsonPublishable(
  provenance: Pick<ProvenanceV1, "model" | "evidence" | "declared">,
): boolean {
  const declared = provenance.declared;
  if (declared === undefined || declared === null) {
    return false;
  }
  return (
    declared.model === "scripted" &&
    (declared.evidence === "simulated" || declared.evidence === "offline") &&
    (provenance.model === "scripted" || provenance.model === "undetermined") &&
    (provenance.evidence === "simulated" || provenance.evidence === "none")
  );
}

/** Outcome as text: the kind, always with its evidence axis next to it. */
export const RECOMMENDATION_VARIANTS: Record<RecommendationKind, BadgeVariant> = {
  supported: "success",
  contested: "warning",
  insufficient_evidence: "warning",
  abstained: "neutral",
  blocked: "error",
};

/**
 * A proof's status token. A simulated verification (the scripted verifier)
 * is never a green "valid": it is the neutral SIMULATED token, and its
 * scripted outcome is text next to it.
 */
export function proofToken(
  status: IntegrityStatus,
  simulated: boolean,
): { key: string; variant: BadgeVariant; simulated: boolean } {
  if (simulated) {
    return { key: `proof.${status}`, variant: "purple", simulated: true };
  }
  switch (status) {
    case "valid":
      return { key: "proof.valid", variant: "success", simulated: false };
    case "invalid":
      return { key: "proof.invalid", variant: "error", simulated: false };
    case "unavailable":
    case "verification_error":
    case "not_checked":
      return { key: `proof.${status}`, variant: "neutral", simulated: false };
  }
}

export const TERMINAL_STATES: readonly RunState[] = ["approved", "rejected", "failed"];

export function isTerminal(state: RunState): boolean {
  return TERMINAL_STATES.includes(state);
}

/** A run at rest: nothing more will be published until a human acts, or ever. */
export function isAtRest(state: RunState, active: boolean): boolean {
  return !active && (isTerminal(state) || state === "awaiting_human_approval");
}
