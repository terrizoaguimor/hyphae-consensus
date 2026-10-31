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
// - Nothing that is not verified real evidence uses the success (green)
//   variant: a "supported" outcome and a "corroborated" assessment are the
//   info role whatever their evidence (a simulated run's are never green),
//   and only a real verifier's accepted proof is green.
// - One colour per label: META MODEL is the info variant in every state (a
//   flag keeps its own warning badge next to it); the LIVE role (the LIVE
//   delivery badge and LIVE META) has its own variant, LIVE_VARIANT, which
//   no other label uses.
// - Hyphae cyan stays reserved for proof and evidence: no badge here uses the
//   cyan or teal variants.
// - Without hue (round 5, design/palette-mono.md 2.1) a variant is drawn as a
//   status role (`VARIANT_ROLES`): fill weight, label polarity, frame style,
//   an edge mark or marker, and an icon, then the word. A variant can take
//   only the roles `VARIANT_ALLOWED_ROLES` lists (success may be drawn
//   "checked", never the reverse; neutral may be RECORDED; blue may be your
//   pending decision), so no label can be drawn verified, LIVE or simulated
//   by a role override; and an icon outside its role's family
//   (`ROLE_ICONS`, no outline shared by two roles) falls back to the role's
//   own (`tagLook`), so a simulated proof never gets the verified shield.

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

/** The badge variants the views use (every one has a monochrome encoding in globals.css). */
export type TagVariant = Extract<
  BadgeVariant,
  "success" | "error" | "purple" | "pink" | "warning" | "neutral" | "info" | "blue"
>;

export type ProvenanceLabel = {
  /** `Provenance.<key>` in the message catalogs. */
  key: string;
  variant: TagVariant;
};

/**
 * The status roles of design/palette-mono.md 2.1, each told apart without
 * hue (globals.css "Status without hue"):
 *   verified   solid mid grey, inverted label, no frame (a real verifier's accepted proof)
 *   checked    hollow: the surface and a 1px frame (a check run in this browser)
 *   simulated  tint, 1px dashed frame, a 4px pixel hatch strip; never solid, never a check
 *   recorded   its own mid tint and a 1px frame (an archived run)
 *   live       the surface, a 2px ink frame and the 8px square (pulses 3 times)
 *   failed     solid ink, inverted label, a double frame
 *   contested  tint, a double frame
 *   pending    strong tint, a 2px frame and a 3px ink bar (your decision)
 *   info       tint, a hairline, quieter ink
 */
export type TagRole =
  | "verified"
  | "checked"
  | "simulated"
  | "recorded"
  | "live"
  | "failed"
  | "contested"
  | "pending"
  | "info";

export const TAG_ROLES: readonly TagRole[] = [
  "verified",
  "checked",
  "simulated",
  "recorded",
  "live",
  "failed",
  "contested",
  "pending",
  "info",
];

/** The role each variant is drawn as, unless the view names an allowed other role. */
export const VARIANT_ROLES: Record<TagVariant, TagRole> = {
  success: "verified",
  error: "failed",
  purple: "simulated",
  pink: "live",
  warning: "contested",
  neutral: "info",
  info: "info",
  blue: "info",
};

/**
 * The roles a view may draw each variant as (the first is the default). Only
 * a quieter reading is ever allowed: the success variant of a check run in
 * this browser is drawn "checked" (integrity is not truth), the neutral
 * variant of an archived run RECORDED, the blue state that waits for the
 * visitor "pending". Nothing can be drawn verified but the success variant,
 * LIVE but the LIVE variant, or simulated but the simulated one.
 */
export const VARIANT_ALLOWED_ROLES: Record<TagVariant, readonly TagRole[]> = {
  success: ["verified", "checked"],
  error: ["failed"],
  purple: ["simulated"],
  pink: ["live"],
  warning: ["contested"],
  neutral: ["info", "recorded"],
  info: ["info"],
  blue: ["info", "pending"],
};

/** Labels whose role is not their variant's default: RECORDED has its own mid tint. */
const KEY_ROLES: Readonly<Record<string, TagRole>> = {
  "recorded.label": "recorded",
};

/**
 * The icons a tag may carry (Lucide 1.61 shapes, ISC; drawn by
 * src/components/icons.tsx). Names follow Lucide 1.61 (`file-code-corner`
 * is design-direction's `file-code-2`, `file-question-mark` its
 * `file-question`, `history` its alias of `rotate-ccw-clock`).
 */
export type TagIconName =
  | "shield-check"
  | "square-x"
  | "shield-x"
  | "unlink"
  | "file-x"
  | "circle-dashed"
  | "flask-conical"
  | "file-code-corner"
  | "archive"
  | "triangle-alert"
  | "file-question-mark"
  | "shuffle"
  | "link-2-off"
  | "plug-zap"
  | "message-square-warning"
  | "user-round-check"
  | "info"
  | "circle-dot"
  | "circle-pause"
  | "bot"
  | "server"
  | "database"
  | "package"
  | "circle-slash"
  | "circle-question-mark"
  | "history"
  | "activity"
  | "shield-question-mark"
  | "gauge"
  | "hash"
  | "circle-dot-dashed"
  | "stamp"
  | "user-round-x";

/**
 * Each role's icon family, its default first (design-direction 3.2). No icon
 * is in two families, the check glyph (shield-check) is the verified role's
 * alone, and LIVE has none: its 8px square is the mark.
 */
export const ROLE_ICONS: Record<TagRole, readonly TagIconName[]> = {
  verified: ["shield-check"],
  checked: ["circle-dashed"],
  simulated: ["flask-conical", "file-code-corner"],
  recorded: ["archive"],
  live: [],
  failed: ["square-x", "shield-x", "unlink", "file-x"],
  contested: [
    "triangle-alert",
    "file-question-mark",
    "shuffle",
    "link-2-off",
    "plug-zap",
    "message-square-warning",
  ],
  pending: ["user-round-check"],
  info: [
    "info",
    "circle-dot",
    "circle-pause",
    "bot",
    "server",
    "database",
    "package",
    "circle-slash",
    "circle-question-mark",
    "history",
    "activity",
    "shield-question-mark",
    "gauge",
    "hash",
    "circle-dot-dashed",
    "stamp",
    "user-round-x",
  ],
};

/** The icon of each `Provenance.<key>` label (design-direction 3.2), when its role allows it. */
const KEY_ICONS: Readonly<Record<string, TagIconName>> = {
  "scripted.label": "file-code-corner",
  "simulated.label": "flask-conical",
  "metaModel.label": "bot",
  "mixedModel.label": "shuffle",
  "mixedEvidence.label": "shuffle",
  "undetermined.label": "circle-question-mark",
  "liveMeta.evidence": "package",
  "hyphaeLive.label": "database",
  "noEvidence.label": "circle-slash",
  "thisServer.label": "server",
  "recorded.label": "archive",
  "streaming.label": "activity",
  "replay.label": "history",
  "flags.provenance_inconsistent": "triangle-alert",
  "flags.unchained": "triangle-alert",
  "flags.mode_undeclared": "triangle-alert",
  "proof.valid": "shield-check",
  "proof.invalid": "shield-x",
  "proof.unavailable": "shield-question-mark",
  "proof.verification_error": "shield-question-mark",
  "proof.not_checked": "shield-question-mark",
};

/** The icon of each outcome (palette-mono 2.1: "supported" is never a check mark). */
export const OUTCOME_ICONS: Record<RecommendationKind, TagIconName> = {
  supported: "circle-dot",
  contested: "triangle-alert",
  insufficient_evidence: "file-question-mark",
  abstained: "circle-pause",
  blocked: "square-x",
};

/**
 * The icon of a person's recorded decision (design-direction 3.2): an
 * informational tag with the decision word, told apart from the neutral
 * labels beside it by its icon, never by a louder fill.
 */
export const DECISION_ICONS: Record<"approved" | "approvedOverride" | "rejected", TagIconName> = {
  approved: "stamp",
  approvedOverride: "stamp",
  rejected: "user-round-x",
};

/**
 * How a tag is drawn: its role (the variant's default, or an allowed other
 * one named by the view or by the label's key) and its icon (the one named,
 * else the label's, else the role's default; never one outside the role's
 * family, and none for LIVE).
 */
export function tagLook(
  variant: TagVariant,
  options: { role?: TagRole; icon?: TagIconName; key?: string } = {},
): { role: TagRole; icon: TagIconName | null } {
  const allowed = VARIANT_ALLOWED_ROLES[variant];
  const keyRole = options.key === undefined ? undefined : KEY_ROLES[options.key];
  const role =
    options.role !== undefined && allowed.includes(options.role)
      ? options.role
      : keyRole !== undefined && allowed.includes(keyRole)
        ? keyRole
        : VARIANT_ROLES[variant];
  const family = ROLE_ICONS[role];
  const keyIcon = options.key === undefined ? undefined : KEY_ICONS[options.key];
  const icon = [options.icon, keyIcon].find(
    (candidate): candidate is TagIconName => candidate !== undefined && family.includes(candidate),
  );
  return { role, icon: icon ?? family[0] ?? null };
}

/**
 * The LIVE role's variant: the LIVE delivery badge and the LIVE META model
 * badge, and nothing else (tests/provenance.test.ts).
 */
export const LIVE_VARIANT = "pink" satisfies TagVariant;

/**
 * The info role: META MODEL in every state, a "supported" outcome, a
 * "corroborated" assessment. Never the success (green) variant, which only
 * a real verifier's accepted proof uses.
 */
export const INFO_VARIANT = "info" satisfies TagVariant;

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
 * META MODEL, in the info variant in every state (a blocking flag is its own
 * warning badge next to it, never a second colour of this label). No
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
        return { key: "liveMeta.label", variant: LIVE_VARIANT };
      }
      return { key: "metaModel.label", variant: INFO_VARIANT };
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
    ? { key: "live.label", variant: LIVE_VARIANT }
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

/**
 * Outcome as text: the kind, always with its evidence axis next to it. No
 * outcome is green: "supported" is the info role (on SIMULATED evidence
 * nothing was checked, and even on real evidence an outcome is the policy's
 * reading, not a verification).
 */
export const RECOMMENDATION_VARIANTS: Record<RecommendationKind, TagVariant> = {
  supported: INFO_VARIANT,
  contested: "warning",
  insufficient_evidence: "warning",
  abstained: "neutral",
  blocked: "error",
};

/**
 * An Evaluator assessment: "corroborated" is the info role, never green (an
 * assessment is not a verification, and on simulated evidence nothing was
 * checked); "not corroborated" is neutral.
 */
export function assessmentVariant(corroborated: boolean): TagVariant {
  return corroborated ? INFO_VARIANT : "neutral";
}

/**
 * A proof's status token. A simulated verification (the scripted verifier)
 * is never a green "valid": it is the neutral SIMULATED token, and its
 * scripted outcome is text next to it.
 */
export function proofToken(
  status: IntegrityStatus,
  simulated: boolean,
): { key: string; variant: TagVariant; simulated: boolean } {
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
