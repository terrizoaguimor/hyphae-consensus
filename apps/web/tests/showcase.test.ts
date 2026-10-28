// SPDX-License-Identifier: Apache-2.0
//
// The research showcase (src/lib/showcase.ts) over the contract's F03 entry
// (the server's own derivation) and the archive's real manifest: outcomes
// (a kind or failed:<stage>), the pre-1.1 label, the policy rules of each
// schema, the figures counted from the entries (every outcome next to its
// evidence axis), the manifest's own notes; and the honesty rule for
// recorded runs: no evidence or source label carries the LIVE word, in any
// locale (a recorded run never reads LIVE).

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { routing } from "../src/i18n/routing";
import { archiveManifestSchema } from "../src/lib/api/schemas";
import type { EvidenceProvenance, ShowcaseEntryV1 } from "../src/lib/api/types";
import { evidenceLabel, sourceLabel } from "../src/lib/provenance";
import {
  archiveFigures,
  byBattery,
  isUnchained,
  manifestNote,
  parseOutcome,
  policyRulesOf,
} from "../src/lib/showcase";

import { manifest, researchBytes } from "./research";

const F03 = JSON.parse(
  readFileSync(path.join(contractDir(), "fixtures", "showcase-entry-F03.json"), "utf8"),
) as ShowcaseEntryV1;

/** An entry like F03 with other fields (as the server would list another run). */
const like = (fields: Partial<ShowcaseEntryV1>): ShowcaseEntryV1 => ({
  ...F03,
  ...fields,
  provenance: { ...F03.provenance, ...fields.provenance },
});

describe("entries", () => {
  it("reads an outcome as a kind, a failed stage or something else", () => {
    expect(parseOutcome("contested")).toEqual({ kind: "recommendation", value: "contested" });
    expect(parseOutcome("failed:evaluate")).toEqual({ kind: "failed", stage: "evaluate" });
    expect(parseOutcome("awaiting_human_approval")).toEqual({ kind: "other", value: "awaiting_human_approval" });
  });

  it("knows the policy rules each schema version was derived under", () => {
    expect(policyRulesOf("1.0")).toBeNull();
    expect(policyRulesOf("1.1")).toBe("1.1");
    expect(policyRulesOf("1.2")).toBe("1.2");
    expect(policyRulesOf("1.3")).toBe("1.2");
  });

  it("reads a schema-1.0 or unchained entry as pre-1.1, unchained", () => {
    expect(isUnchained(F03)).toBe(false);
    expect(isUnchained(like({ schema_version: "1.0", head_hash: null }))).toBe(true);
    expect(isUnchained(like({ provenance: { ...F03.provenance, flags: ["unchained"] } }))).toBe(true);
  });

  it("groups entries by battery in the order the server lists them", () => {
    const a = like({ battery: "b-1", run_id: "run-a" });
    const b = like({ battery: "b-2", run_id: "run-b" });
    const c = like({ battery: "b-1", run_id: "run-c" });
    expect(byBattery([a, b, c]).map((g) => [g.battery, g.entries.map((e) => e.run_id)])).toEqual([
      ["b-1", ["run-a", "run-c"]],
      ["b-2", ["run-b"]],
    ]);
  });
});

describe("archiveFigures", () => {
  const entries = [
    F03,
    like({ run_id: "run-2", outcome: "contested" }),
    like({
      run_id: "run-3",
      battery: "console-2026-10-26",
      outcome: "supported",
      authorization: "approved",
      schema_version: "1.0",
      head_hash: null,
      event_count: 15,
      provenance: { ...F03.provenance, model: "scripted", evidence: "simulated", meta_request_ids: [], flags: ["unchained"] },
    }),
    like({ run_id: "run-4", battery: "live-battery-2026-10-26", outcome: "failed:evaluate", schema_version: "1.1" }),
  ];
  const figures = archiveFigures(entries);

  it("counts runs, records and recorded Meta request ids from the entries", () => {
    expect(figures.runs).toBe(4);
    expect(figures.events).toBe(F03.event_count * 3 + 15);
    expect(figures.metaRequestIds).toBe((F03.provenance.meta_request_ids ?? []).length * 3);
    expect(figures.unchained).toBe(1);
  });

  it("counts every outcome next to its evidence axis", () => {
    const evidence = (outcome: string, axis: EvidenceProvenance) =>
      figures.outcomes.find((o) => o.outcome === outcome)?.byEvidence.find((e) => e.evidence === axis)?.count;
    expect(figures.evidence).toEqual(["simulated", "hyphae_live"]);
    expect(figures.outcomes.map((o) => [o.outcome, o.total])).toEqual([
      ["supported", 1],
      ["contested", 2],
      ["failed", 1],
    ]);
    expect(evidence("supported", "simulated")).toBe(1);
    expect(evidence("supported", "hyphae_live")).toBe(0);
    expect(evidence("contested", "hyphae_live")).toBe(2);
  });

  it("counts by model axis, authorization and battery, with each battery's schema versions", () => {
    expect(figures.models).toEqual([
      { model: "live_meta", count: 3 },
      { model: "scripted", count: 1 },
    ]);
    expect(figures.authorizations).toEqual([
      { authorization: "pending", count: 3 },
      { authorization: "approved", count: 1 },
    ]);
    expect(figures.batteries.map((b) => [b.battery, b.runs, b.schemas])).toEqual([
      ["live-battery-2026-10-27-final", 2, ["1.2"]],
      ["console-2026-10-26", 1, ["1.0"]],
      ["live-battery-2026-10-26", 1, ["1.1"]],
    ]);
    expect(figures.batteries[0]?.outcomes).toEqual([{ outcome: "contested", count: 2 }]);
  });

  it("an empty archive has no figure", () => {
    expect(archiveFigures([])).toMatchObject({ runs: 0, events: 0, outcomes: [], batteries: [] });
  });
});

describe("the archive's manifest", () => {
  const parsed = archiveManifestSchema.parse(JSON.parse(new TextDecoder().decode(researchBytes("MANIFEST.json"))));
  const files = parsed.files.map((f) => ({ ...f, description: f.description ?? null }));
  const notes = { created: parsed.created ?? null, files };

  it("parses as the views read it, and pins F03's log with the digest the server lists", () => {
    expect(notes.created).toBe(manifest().created);
    expect(files.find((f) => f.path === F03.file)?.sha256).toBe(F03.sha256);
  });

  it("carries its own note on the 2026-10-26 battery: a deployment misconfiguration, not tampering", () => {
    const note = manifestNote(notes, "2026-10-26/live-battery-2026-10-26.jsonl");
    expect(note).toContain("proof_error");
    expect(note).toContain("a deployment misconfiguration");
    expect(manifestNote(notes, "no/such/file.jsonl")).toBeNull();
    expect(manifestNote(null, F03.file)).toBeNull();
  });
});

describe("a recorded run never reads LIVE", () => {
  type Catalog = { [key: string]: string | Catalog };
  const lookup = (catalog: Catalog, key: string): string => {
    let node: string | Catalog | undefined = catalog;
    for (const part of key.split(".")) {
      node = typeof node === "object" ? node[part] : undefined;
    }
    if (typeof node !== "string") {
      throw new Error(`no message ${key}`);
    }
    return node;
  };

  it.each(routing.locales)("no evidence or source label carries the LIVE word (%s)", (locale) => {
    const provenance = (
      JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8")) as Catalog
    ).Provenance as Catalog;
    const liveWord = lookup(provenance, "live.label").toLocaleUpperCase(locale);
    const keys = [
      ...(["simulated", "hyphae_snapshot", "hyphae_live", "none", "mixed"] as const).map((e) => evidenceLabel(e).key),
      sourceLabel("research_archive").key,
      sourceLabel("this_server").key,
    ];
    for (const key of keys) {
      expect(lookup(provenance, key).toLocaleUpperCase(locale), `${locale} ${key}`).not.toContain(liveWord);
    }
  });
});
