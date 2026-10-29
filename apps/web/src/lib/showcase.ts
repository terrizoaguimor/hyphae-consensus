// SPDX-License-Identifier: Apache-2.0
//
// The research showcase, computed from what the server lists (GET /showcase:
// ShowcaseEntryV1, every field derived by the server from the archived
// chains) and from the archive's own manifest (research/MANIFEST.json, served
// at /research/MANIFEST.json and pinned by ShowcaseV1.manifest_sha256): the
// outcome of an entry (a recommendation kind, or failed:<stage>), the labels
// a recorded run carries (schema 1.0: pre-1.1, unchained), the manifest's
// note on a log file, and the figures of the Overview (runs and records by
// outcome and evidence axis, by battery with its schema versions and policy
// rules, by model axis and authorization, and the recorded Meta request
// ids). Every number is counted from the entries; nothing is typed in. Pure
// functions, unit-tested in tests/showcase.test.ts.

import type {
  EvidenceProvenance,
  ModelProvenance,
  PolicyRules,
  RecommendationKind,
  ShowcaseEntryV1,
} from "./api/types";

export type Outcome =
  | { kind: "recommendation"; value: RecommendationKind }
  | { kind: "failed"; stage: string }
  | { kind: "other"; value: string };

const KINDS: readonly RecommendationKind[] = [
  "supported",
  "contested",
  "insufficient_evidence",
  "abstained",
  "blocked",
];

/** An entry's outcome label: the recommendation kind, `failed:<stage>`, or a run state. */
export function parseOutcome(outcome: string): Outcome {
  if ((KINDS as readonly string[]).includes(outcome)) {
    return { kind: "recommendation", value: outcome as RecommendationKind };
  }
  if (outcome.startsWith("failed:")) {
    return { kind: "failed", stage: outcome.slice("failed:".length) };
  }
  return { kind: "other", value: outcome };
}

/** The policy rules a schema version's records were derived under (consensus-runtime PolicyVersion::for_schema). */
export function policyRulesOf(schema: string): PolicyRules | null {
  switch (schema) {
    case "1.1":
      return "1.1";
    case "1.2":
    case "1.3":
      return "1.2";
    default:
      return null;
  }
}

/** A recorded run from before the hash chain: schema 1.0, or flagged unchained by the server. */
export function isUnchained(entry: Pick<ShowcaseEntryV1, "schema_version" | "provenance" | "head_hash">): boolean {
  return (
    entry.schema_version === "1.0" ||
    (entry.provenance.flags ?? []).includes("unchained") ||
    entry.head_hash === undefined ||
    entry.head_hash === null
  );
}

/** Outcome columns of the figures, in this order. */
export const OUTCOME_KEYS = [...KINDS, "failed", "other"] as const;
export type OutcomeKey = (typeof OUTCOME_KEYS)[number];

export const EVIDENCE_AXES: readonly EvidenceProvenance[] = [
  "simulated",
  "hyphae_snapshot",
  "hyphae_live",
  "mixed",
  "none",
];

export const MODEL_AXES: readonly ModelProvenance[] = ["live_meta", "scripted", "mixed", "undetermined"];

export function outcomeKey(outcome: Outcome): OutcomeKey {
  return outcome.kind === "recommendation" ? outcome.value : outcome.kind;
}

export type BatteryFigures = {
  battery: string;
  runs: number;
  events: number;
  /** Schema versions of its runs, in the order first seen. */
  schemas: string[];
  /** Log files its runs are segments of. */
  files: string[];
  outcomes: Array<{ outcome: OutcomeKey; count: number }>;
};

export type ArchiveFigures = {
  runs: number;
  events: number;
  /**
   * Request ids of recorded Meta calls, over every entry: one per Meta
   * invocation the records carry with its request id (a call that got a
   * response; a retried call is one invocation, its HTTP attempts are not
   * counted). Scripted invocations carry none, and a call that timed out or
   * failed in transport records no invocation, so neither is counted.
   */
  metaRequestIds: number;
  /** Entries with at least one recorded Meta request id. */
  metaRuns: number;
  /** Runs by outcome and evidence axis: every outcome is counted next to its evidence. */
  outcomes: Array<{ outcome: OutcomeKey; total: number; byEvidence: Array<{ evidence: EvidenceProvenance; count: number }> }>;
  /** The evidence axes present, in EVIDENCE_AXES order. */
  evidence: EvidenceProvenance[];
  models: Array<{ model: ModelProvenance; count: number }>;
  authorizations: Array<{ authorization: string; count: number }>;
  batteries: BatteryFigures[];
  /** Runs recorded before the hash chain. */
  unchained: number;
};

function bump<K>(map: Map<K, number>, key: K, by = 1): void {
  map.set(key, (map.get(key) ?? 0) + by);
}

/** The Overview's figures, counted from the entries the server lists. */
export function archiveFigures(entries: readonly ShowcaseEntryV1[]): ArchiveFigures {
  const byOutcome = new Map<OutcomeKey, Map<EvidenceProvenance, number>>();
  const models = new Map<ModelProvenance, number>();
  const authorizations = new Map<string, number>();
  const batteries = new Map<string, BatteryFigures & { byOutcome: Map<OutcomeKey, number> }>();
  const evidenceSeen = new Set<EvidenceProvenance>();
  let events = 0;
  let metaRequestIds = 0;
  let metaRuns = 0;
  let unchained = 0;
  for (const entry of entries) {
    const outcome = outcomeKey(parseOutcome(entry.outcome));
    const evidence = entry.provenance.evidence;
    evidenceSeen.add(evidence);
    const cell = byOutcome.get(outcome) ?? new Map<EvidenceProvenance, number>();
    bump(cell, evidence);
    byOutcome.set(outcome, cell);
    bump(models, entry.provenance.model);
    bump(authorizations, entry.authorization);
    events += entry.event_count;
    const ids = (entry.provenance.meta_request_ids ?? []).length;
    metaRequestIds += ids;
    if (ids > 0) {
      metaRuns += 1;
    }
    if (isUnchained(entry)) {
      unchained += 1;
    }
    const fresh: BatteryFigures & { byOutcome: Map<OutcomeKey, number> } = {
      battery: entry.battery,
      runs: 0,
      events: 0,
      schemas: [],
      files: [],
      outcomes: [],
      byOutcome: new Map(),
    };
    const battery = batteries.get(entry.battery) ?? fresh;
    battery.runs += 1;
    battery.events += entry.event_count;
    if (!battery.schemas.includes(entry.schema_version)) {
      battery.schemas.push(entry.schema_version);
    }
    if (!battery.files.includes(entry.file)) {
      battery.files.push(entry.file);
    }
    bump(battery.byOutcome, outcome);
    batteries.set(entry.battery, battery);
  }
  const evidence = EVIDENCE_AXES.filter((axis) => evidenceSeen.has(axis));
  return {
    runs: entries.length,
    events,
    metaRequestIds,
    metaRuns,
    outcomes: OUTCOME_KEYS.filter((key) => byOutcome.has(key)).map((outcome) => {
      const cell = byOutcome.get(outcome) ?? new Map<EvidenceProvenance, number>();
      return {
        outcome,
        total: [...cell.values()].reduce((a, b) => a + b, 0),
        byEvidence: evidence.map((axis) => ({ evidence: axis, count: cell.get(axis) ?? 0 })),
      };
    }),
    evidence,
    models: MODEL_AXES.filter((m) => models.has(m)).map((model) => ({ model, count: models.get(model) ?? 0 })),
    authorizations: [...authorizations.entries()].map(([authorization, count]) => ({ authorization, count })),
    batteries: [...batteries.values()].map(({ byOutcome: counts, ...battery }) => ({
      ...battery,
      outcomes: OUTCOME_KEYS.filter((key) => counts.has(key)).map((outcome) => ({
        outcome,
        count: counts.get(outcome) ?? 0,
      })),
    })),
    unchained,
  };
}

/** The entries grouped by battery, in the order the server lists them. */
export function byBattery(entries: readonly ShowcaseEntryV1[]): Array<{ battery: string; entries: ShowcaseEntryV1[] }> {
  const groups = new Map<string, ShowcaseEntryV1[]>();
  for (const entry of entries) {
    groups.set(entry.battery, [...(groups.get(entry.battery) ?? []), entry]);
  }
  return [...groups.entries()].map(([battery, list]) => ({ battery, entries: list }));
}

/** What the views read from the archive's manifest (research/MANIFEST.json). */
export type ArchiveManifest = {
  created: string | null;
  files: Array<{ path: string; sha256: string; kind: string; description: string | null }>;
};

/** The manifest's own note on one archive file, when it has one. */
export function manifestNote(manifest: ArchiveManifest | null, path: string): string | null {
  return manifest?.files.find((file) => file.path === path)?.description ?? null;
}
