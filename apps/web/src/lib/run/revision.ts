// SPDX-License-Identifier: Apache-2.0
//
// The revision view: when the current proposal supersedes an earlier one
// (src/lib/run/view.ts supersededProposal), its claims next to the original
// proposal's, claim by claim, from the proposal records alone: a claim id in
// both with the same statement and citations is unchanged, with another
// statement or other citations it changed, an id only in the revision was
// added, one only in the original was dropped. Ids are compared as the
// records carry them; a model may reuse an id for a different claim, which
// then reads as changed. The revisions in between are listed with the items
// each says it addresses (a claim of the revision, not a verdict). Pure
// functions, unit-tested in tests/run-revision.test.ts.

import type { Claim, Proposal, RunDetailV1 } from "../api/types";

export type ClaimChange =
  | { kind: "unchanged"; id: string; claim: Claim }
  | { kind: "changed"; id: string; before: Claim; after: Claim }
  | { kind: "added"; id: string; claim: Claim }
  | { kind: "removed"; id: string; claim: Claim };

const sameRefs = (a: string[], b: string[]) => a.length === b.length && a.every((ref, i) => ref === b[i]);

/** The claims of `revised` against `original`, in the revision's order, then the dropped ones. */
export function compareClaims(original: Proposal, revised: Proposal): ClaimChange[] {
  const before = new Map(original.claims.map((claim) => [claim.id, claim]));
  const seen = new Set<string>();
  const out: ClaimChange[] = [];
  for (const after of revised.claims) {
    seen.add(after.id);
    const earlier = before.get(after.id);
    if (earlier === undefined) {
      out.push({ kind: "added", id: after.id, claim: after });
    } else if (earlier.statement === after.statement && sameRefs(earlier.evidence_refs, after.evidence_refs)) {
      out.push({ kind: "unchanged", id: after.id, claim: after });
    } else {
      out.push({ kind: "changed", id: after.id, before: earlier, after });
    }
  }
  for (const claim of original.claims) {
    if (!seen.has(claim.id)) {
      out.push({ kind: "removed", id: claim.id, claim });
    }
  }
  return out;
}

export type RevisionStep = {
  sequence: number;
  proposalId: string;
  supersedes: string;
  /** Item ids the revision says it addresses. */
  addressedItems: string[];
};

/** Every revision of the run, in log order. */
export function revisionSteps(detail: RunDetailV1): RevisionStep[] {
  return detail.events.flatMap((event) =>
    event.payload.type === "revision_submitted"
      ? [
          {
            sequence: event.sequence,
            proposalId: event.payload.revision.proposal.id,
            supersedes: event.payload.revision.supersedes,
            addressedItems: event.payload.revision.addressed_items ?? [],
          },
        ]
      : [],
  );
}
