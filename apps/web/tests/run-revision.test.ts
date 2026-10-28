// SPDX-License-Identifier: Apache-2.0
//
// The revision view (src/lib/run/revision.ts): the claims of the revised
// proposal against the original's, claim by claim from the proposal records
// (unchanged, changed, added, dropped), and each revision with the items it
// says it addresses, over the retry fixture (a revision answering both
// objections) and variants of it.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type { Proposal, RunDetailV1 } from "../src/lib/api/types";
import { compareClaims, revisionSteps } from "../src/lib/run/revision";
import { supersededProposal } from "../src/lib/run/view";

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
