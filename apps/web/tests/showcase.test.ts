// SPDX-License-Identifier: Apache-2.0
//
// The research showcase (src/lib/showcase.ts) over the contract's F03 entry
// (the server's own derivation) and the archive's real manifest: outcomes
// (a kind or failed:<stage>), the pre-1.1 label, the policy rules of each
// schema, the figures counted from the entries (every outcome next to its
// evidence axis), the manifest's own notes; and the honesty rule for
// recorded runs: no evidence or source label carries the LIVE word, in any
// locale (a recorded run never reads LIVE).

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";

import { IntlMessageFormat } from "intl-messageformat";
import { afterEach, describe, expect, it, vi } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { archivePin, readManifest } from "../src/components/showcase/useArchive";
import { routing } from "../src/i18n/routing";
import { api } from "../src/lib/api/client";
import { archiveManifestSchema } from "../src/lib/api/schemas";
import type { EvidenceProvenance, ShowcaseEntryV1 } from "../src/lib/api/types";
import { evidenceLabel, sourceLabel } from "../src/lib/provenance";
import {
  archiveFigures,
  byBattery,
  isUnchained,
  logPin,
  manifestNote,
  parseOutcome,
  pinDisplay,
  pinSource,
  policyRulesOf,
} from "../src/lib/showcase";

import { manifest, metaRequestIds, researchBytes } from "./research";

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

  it("counts the runs that recorded a Meta request id, not the scripted ones", () => {
    expect((F03.provenance.meta_request_ids ?? []).length).toBeGreaterThan(0);
    expect(figures.metaRuns).toBe(3);
    expect(archiveFigures([]).metaRuns).toBe(0);
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

  // CRITIC-2: the listing's digest of a log is compared with the checked manifest's own pin.
  it("cross-checks a listed log digest against the checked manifest's own pin", () => {
    expect(logPin(notes, F03.file, F03.sha256)).toEqual({ status: "matches", sha256: F03.sha256 });
    expect(logPin(notes, F03.file, F03.sha256.toUpperCase())).toEqual({ status: "matches", sha256: F03.sha256 });
    const rewritten = "a".repeat(64);
    // The manifest's pin is the one a downloaded file is compared with; the listing's is kept to be named.
    expect(logPin(notes, F03.file, rewritten)).toEqual({ status: "differs", sha256: F03.sha256, listing: rewritten });
    expect(logPin(notes, "no/such/file.jsonl", rewritten)).toEqual({ status: "unlisted", sha256: rewritten });
    // No checked manifest: only the listing's digest is known, and said to be unchecked.
    expect(logPin(null, F03.file, rewritten)).toEqual({ status: "unchecked", sha256: rewritten });
  });

  // A digest is labelled pinned by the manifest only when no checked manifest contradicts that.
  it("labels the manifest's own pin as pinned, and the listing's digest as listed where the manifest disagrees", () => {
    const rewritten = "a".repeat(64);
    expect(pinDisplay(logPin(notes, F03.file, F03.sha256))).toEqual({ pinned: F03.sha256, listed: null });
    expect(pinDisplay(logPin(notes, F03.file, rewritten))).toEqual({ pinned: F03.sha256, listed: rewritten });
    expect(pinDisplay(logPin(notes, "no/such/file.jsonl", rewritten))).toEqual({ pinned: null, listed: rewritten });
    // No checked manifest: the listing's copy of the pin, as the contract defines it.
    expect(pinDisplay(logPin(null, F03.file, rewritten))).toEqual({ pinned: rewritten, listed: null });
    expect(pinDisplay({ status: "checking", sha256: rewritten })).toEqual({ pinned: rewritten, listed: null });
  });

  it("says where the digest a downloaded file is compared with comes from", () => {
    const rewritten = "a".repeat(64);
    expect(pinSource(logPin(notes, F03.file, F03.sha256))).toBe("manifest");
    expect(pinSource(logPin(notes, F03.file, rewritten))).toBe("manifest");
    expect(pinSource(logPin(notes, "no/such/file.jsonl", rewritten))).toBe("unlisted");
    expect(pinSource(logPin(null, F03.file, rewritten))).toBe("listing");
    expect(pinSource({ status: "checking", sha256: rewritten })).toBe("listing");
  });

  it("reads a pin as being checked while the manifest is read, never as unchecked", () => {
    const showcase = { entries: [F03], manifest_file: "MANIFEST.json", manifest_sha256: "0".repeat(64) };
    expect(archivePin({ status: "ready", showcase, manifest: { status: "loading" } }, F03.file, F03.sha256)).toEqual({
      status: "checking",
      sha256: F03.sha256,
    });
    expect(
      archivePin({ status: "ready", showcase, manifest: { status: "ready", sha256: "0".repeat(64), matches: true, manifest: notes } }, F03.file, F03.sha256),
    ).toEqual({ status: "matches", sha256: F03.sha256 });
    expect(
      archivePin({ status: "ready", showcase, manifest: { status: "ready", sha256: "1".repeat(64), matches: false, manifest: null } }, F03.file, F03.sha256),
    ).toEqual({ status: "unchecked", sha256: F03.sha256 });
  });
});

// GATES-03: a manifest whose bytes do not hash to the published digest lends nothing.
describe("readManifest", () => {
  const genuine = researchBytes("MANIFEST.json");
  const listing = (bytes: Uint8Array) => ({
    entries: [],
    manifest_file: "MANIFEST.json",
    manifest_sha256: createHash("sha256").update(bytes).digest("hex"),
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("parses a manifest that matches the published digest", async () => {
    vi.spyOn(api, "bytes").mockResolvedValue(new Uint8Array(genuine));
    const state = await readManifest(listing(genuine));
    expect(state.status).toBe("ready");
    expect(state.status === "ready" && state.matches).toBe(true);
    expect(state.status === "ready" ? manifestNote(state.manifest, "2026-10-26/live-battery-2026-10-26.jsonl") : null).toContain(
      "a deployment misconfiguration",
    );
  });

  it("keeps nothing of a forged manifest that parses but does not match", async () => {
    const forged = Buffer.from(
      JSON.stringify({
        created: "2026-10-27",
        files: [{ path: "2026-10-26/live-battery-2026-10-26.jsonl", sha256: "0", kind: "event_log", description: "forged note" }],
      }),
    );
    vi.spyOn(api, "bytes").mockResolvedValue(new Uint8Array(forged));
    const state = await readManifest(listing(genuine));
    expect(state).toMatchObject({ status: "ready", matches: false, manifest: null });
  });

  it("keeps a failed download as an error, never as a manifest", async () => {
    vi.spyOn(api, "bytes").mockRejectedValue(new Error("network down"));
    const state = await readManifest(listing(genuine));
    expect(state.status).toBe("error");
  });
});

describe("the archive's own figures", () => {
  // Entries as the server would list the real archive: the manifest's runs,
  // each with the Meta request ids its log records (the server's derivation).
  const runs = manifest().runs;
  const entries = runs.map((run) =>
    like({
      run_id: run.run_id,
      battery: run.battery,
      outcome: run.outcome,
      file: run.file,
      provenance: { ...F03.provenance, meta_request_ids: metaRequestIds(run.file, run.run_id) },
    }),
  );
  const figures = archiveFigures(entries);
  const readme = new TextDecoder().decode(researchBytes("README.md"));

  it("counts 228 recorded Meta request ids in 48 of the 51 runs, every one distinct", () => {
    expect(runs).toHaveLength(51);
    expect(figures.metaRequestIds).toBe(228);
    expect(figures.metaRuns).toBe(48);
    const all = entries.flatMap((e) => e.provenance.meta_request_ids ?? []);
    expect(new Set(all).size).toBe(all.length);
  });

  it("agrees with the number of calls research/README.md states", () => {
    const stated = /: (\d+) calls in this archive/.exec(readme.replace(/\s+/g, " "));
    expect(stated?.[1]).toBe(String(figures.metaRequestIds));
  });

  it.each(routing.locales)("the Overview's sentence names the ids and the runs they were recorded in (%s)", (locale) => {
    const catalog = JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8")) as {
      Overview: { figures: { requests: string } };
    };
    const format = new IntlMessageFormat(catalog.Overview.figures.requests, locale);
    const text = String(format.format({ count: figures.metaRequestIds, runs: figures.metaRuns }));
    expect(text).toMatch(new RegExp(`(^|\\D)${figures.metaRequestIds}(\\D|$)`));
    expect(text).toMatch(new RegExp(`(^|\\D)${figures.metaRuns}(\\D|$)`));
    expect(String(format.format({ count: 0, runs: 0 }))).not.toMatch(/\d/);
  });

  it("says the scripted double makes no call, and names every unanswered call as recording none", () => {
    const catalog = JSON.parse(readFileSync(new URL("../messages/en.json", import.meta.url), "utf8")) as {
      Overview: { figures: { requests: string } };
    };
    const text = String(
      new IntlMessageFormat(catalog.Overview.figures.requests, "en").format({
        count: figures.metaRequestIds,
        runs: figures.metaRuns,
      }),
    );
    // The double makes no Meta call (meta.rs records none for it), so none is "left out".
    expect(text).toContain("The scripted double makes no Meta call and records no id");
    expect(text).not.toContain("calls are not counted");
    // A non-2xx answer after retries records no invocation either (meta.rs send()).
    expect(text).toContain("timed out, failed in transport or was refused with an error status");
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
