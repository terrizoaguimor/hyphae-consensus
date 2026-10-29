// SPDX-License-Identifier: Apache-2.0
//
// The revision view (src/lib/run/revision.ts): the claims of the revised
// proposal against the original's, claim by claim from the proposal records
// (unchanged, changed, added, dropped), and each revision with the items it
// says it addresses, over the retry fixture (a revision answering both
// objections) and variants of it; and claimComparison, which pairs by id
// only when the proposals share an id, over the real archived revisions of
// research/ (the Meta model adapter numbers every proposal's claims anew, so
// none of its revisions shares one).

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type { Proposal, RunDetailV1 } from "../src/lib/api/types";
import { claimComparison, compareClaims, revisionSteps } from "../src/lib/run/revision";
import { supersededProposal } from "../src/lib/run/view";

import { manifest, researchBytes } from "./research";

const detail = JSON.parse(
  readFileSync(path.join(contractDir(), "fixtures", "run-detail-retry.json"), "utf8"),
) as RunDetailV1;

const original = supersededProposal(detail) as Proposal;
const revised = detail.current_proposal as Proposal;

describe("compareClaims", () => {
  it("pairs the claims by id: the retry revision rewrote both", () => {
    const changes = compareClaims(original, revised);
    expect(changes.map((c) => [c.kind, c.id])).toEqual([
      ["changed", "claim-1"],
      ["changed", "claim-2"],
    ]);
    const first = changes[0];
    expect(first?.kind === "changed" ? [first.before.statement, first.after.statement] : null).toEqual([
      "Payment timeouts are caused by insufficient retries",
      "Payment timeouts correlate with retry bursts above 5 attempts",
    ]);
  });

  it("says unchanged, added and dropped claims as such, in the revision's order then the dropped ones", () => {
    const base = original.claims[0];
    if (base === undefined) {
      throw new Error("the original has no claim");
    }
    const next: Proposal = {
      ...revised,
      claims: [{ ...base }, { id: "claim-9", statement: "A new claim", evidence_refs: [] }],
    };
    expect(compareClaims(original, next).map((c) => [c.kind, c.id])).toEqual([
      ["unchanged", "claim-1"],
      ["added", "claim-9"],
      ["removed", "claim-2"],
    ]);
  });

  it("a claim whose citations changed is changed, even with the same statement", () => {
    const base = original.claims[0];
    if (base === undefined) {
      throw new Error("the original has no claim");
    }
    const next: Proposal = { ...revised, claims: [{ ...base, evidence_refs: ["ev-fact"] }] };
    expect(compareClaims(original, next)[0]?.kind).toBe("changed");
  });
});

// The archived runs of research/ (verbatim): each run's records from its log,
// as a run detail carries them (events), with its last proposal as the
// current one. supersededProposal and the comparison read nothing else.
type ArchivedRun = { label: string; detail: RunDetailV1; revisionProviders: string[] };

let archiveCache: ArchivedRun[] | null = null;

function archivedRuns(): ArchivedRun[] {
  archiveCache ??= readArchivedRuns();
  return archiveCache;
}

function readArchivedRuns(): ArchivedRun[] {
  const m = manifest();
  const logs = new Map<string, Record<string, unknown>[]>();
  return m.runs.map((run) => {
    let records = logs.get(run.file);
    if (records === undefined) {
      records = new TextDecoder()
        .decode(researchBytes(run.file))
        .split("\n")
        .filter((line) => line.trim() !== "")
        .map((line) => JSON.parse(line) as Record<string, unknown>);
      logs.set(run.file, records);
    }
    const events = records.filter((record) => record.run_id === run.run_id) as unknown as RunDetailV1["events"];
    let current: Proposal | null = null;
    const revisionProviders: string[] = [];
    for (const event of events) {
      if (event.payload.type === "proposal_submitted") {
        current = event.payload.proposal;
      } else if (event.payload.type === "revision_submitted") {
        current = event.payload.revision.proposal;
        const invocation = (event.payload as { invocation?: { provider?: string } | null }).invocation;
        revisionProviders.push(invocation?.provider ?? "none");
      }
    }
    return { label: run.label, detail: { events, current_proposal: current } as unknown as RunDetailV1, revisionProviders };
  });
}

function archivedRun(label: string): ArchivedRun {
  const run = archivedRuns().find((r) => r.label === label);
  if (run === undefined) {
    throw new Error(`no run ${label} in the archive`);
  }
  return run;
}

describe("claimComparison", () => {
  it("pairs by id when the proposals share claim ids (the scripted retry fixture)", () => {
    const comparison = claimComparison(original, revised);
    expect(comparison.pairing).toBe("id");
    expect(comparison.pairing === "id" ? comparison.changes : null).toEqual(compareClaims(original, revised));
  });

  it("pairs nothing when no claim id is shared: both lists, as recorded, no verdict", () => {
    const fresh: Proposal = {
      ...revised,
      claims: revised.claims.map((claim, i) => ({ ...claim, id: `claim-${10 + i}` })),
    };
    expect(claimComparison(original, fresh)).toEqual({
      pairing: "none",
      original: original.claims,
      revised: fresh.claims,
    });
  });

  it("R06, a real archived revision: prop-13 shares no claim id with prop-1, so nothing is called added or dropped", () => {
    const { detail } = archivedRun("R06");
    const before = supersededProposal(detail) as Proposal;
    const after = detail.current_proposal as Proposal;
    expect([before.id, after.id]).toEqual(["prop-1", "prop-13"]);
    // What pairing by id alone would say on this run: every claim added or dropped.
    expect(compareClaims(before, after).map((c) => c.kind)).toEqual([
      "added",
      "added",
      "added",
      "removed",
      "removed",
      "removed",
    ]);
    const comparison = claimComparison(before, after);
    expect(comparison.pairing).toBe("none");
    if (comparison.pairing !== "none") {
      throw new Error("R06 paired");
    }
    expect(comparison.original.map((c) => c.id)).toEqual(["claim-2", "claim-3", "claim-4"]);
    expect(comparison.revised.map((c) => c.id)).toEqual(["claim-14", "claim-15", "claim-16"]);
    expect(comparison.original[0]?.statement).toBe(
      "The payment HTTP per-request timeout will be set to exactly 30 seconds.",
    );
    expect(comparison.revised[0]?.statement).toBe(
      "The payment-client RetryPolicy will be set to max_attempts=3 and jitter_ms=50.",
    );
  });

  it("no revision written by the Meta model in the archive shares a claim id; the scripted ones do", () => {
    const revisedRuns = archivedRuns().filter((run) => run.revisionProviders.length > 0);
    const byMeta = revisedRuns.filter((run) => run.revisionProviders.every((p) => p === "meta"));
    const scripted = revisedRuns.filter((run) => run.revisionProviders.every((p) => p === "fake"));
    // Every revised run of the archive is one or the other.
    expect(byMeta.length + scripted.length).toBe(revisedRuns.length);
    expect(byMeta.length).toBeGreaterThan(0);
    expect(scripted.length).toBeGreaterThan(0);
    for (const run of byMeta) {
      const before = supersededProposal(run.detail) as Proposal;
      const comparison = claimComparison(before, run.detail.current_proposal as Proposal);
      expect(comparison.pairing, `${run.label} ${before.id}`).toBe("none");
    }
    for (const run of scripted) {
      const before = supersededProposal(run.detail) as Proposal;
      const comparison = claimComparison(before, run.detail.current_proposal as Proposal);
      expect(comparison.pairing, run.label).toBe("id");
      expect(comparison.pairing === "id" ? comparison.changes.map((c) => c.kind) : null, run.label).not.toContain(
        "added",
      );
    }
    expect(scripted.map((run) => run.label)).toEqual(["C01", "C02", "C05"]);
  });

  it("F01, revised twice: the comparison is against the first proposal, and still pairs nothing", () => {
    const { detail } = archivedRun("F01");
    const before = supersededProposal(detail) as Proposal;
    expect([before.id, detail.current_proposal?.id]).toEqual(["prop-1", "prop-25"]);
    expect(claimComparison(before, detail.current_proposal as Proposal).pairing).toBe("none");
  });
});

describe("revisionSteps", () => {
  it("lists each revision with what it says it addresses", () => {
    expect(revisionSteps(detail)).toEqual([
      { sequence: 9, proposalId: "prop-2", supersedes: "prop-1", addressedItems: ["item-1", "item-2"] },
    ]);
  });

  it("is empty for a run that never revised", () => {
    const abstain = JSON.parse(
      readFileSync(path.join(contractDir(), "fixtures", "run-detail-abstain.json"), "utf8"),
    ) as RunDetailV1;
    expect(revisionSteps(abstain)).toEqual([]);
    expect(supersededProposal(abstain)).toBeNull();
  });
});
