// SPDX-License-Identifier: Apache-2.0
//
// The revision view: when the current proposal supersedes an earlier one
// (src/lib/run/view.ts supersededProposal), its claims next to the original
// proposal's, from the proposal records alone.
//
// What the records carry to relate the two: a revision names the proposal
// it supersedes (Revision.supersedes) and the challenge items it says it
// addresses; a claim carries only its id, statement and cited evidence
// (contracts/api-v1, Claim). Nothing links a revised claim to an original
// one except a shared id, and the live model adapter never shares one: it
// numbers every claim of every proposal, revisions included, from one
// counter for the run (crates/consensus-api/src/meta.rs, build_proposal),
// and the model's draft claims carry no id of their own, so their order is
// no pairing either. Only scripted runs reuse ids (claim-1, claim-2).
//
// So claimComparison pairs by id only when at least one id is in both
// proposals: then a claim id in both with the same statement and citations
// is unchanged, with another statement or other citations it changed, an id
// only in the revision was added, one only in the original was dropped (a
// scripted run may reuse an id for a different claim, which reads as
// changed). When only some ids are shared, `partial` is set and the view
// says that ids alone cannot tell an added or dropped claim from one
// restated under another id. When no id is shared, it pairs nothing and
// returns the two lists as recorded: no claim is said to be added, changed
// or dropped, and the Meta model adapter is named as the cause only when
// every revision of the run went through it (revisedByMetaModel). The
// revisions in between are listed with the items each says it addresses (a
// claim of the revision, not a verdict). Pure functions, unit-tested in
// tests/run-revision.test.ts over the contract fixtures and the archived
// runs of research/.

import type { Claim, ModelInvocationV1, Proposal, RunDetailV1 } from "../api/types";

export type ClaimChange =
  | { kind: "unchanged"; id: string; claim: Claim }
  | { kind: "changed"; id: string; before: Claim; after: Claim }
  | { kind: "added"; id: string; claim: Claim }
  | { kind: "removed"; id: string; claim: Claim };

const sameRefs = (a: string[], b: string[]) => a.length === b.length && a.every((ref, i) => ref === b[i]);

/**
 * The claims of `revised` against `original` by id, in the revision's order,
 * then the dropped ones. Meaningful only when the two share ids: the view
 * goes through claimComparison, which never calls this on disjoint ids.
 */
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

export type ClaimComparison =
  /**
   * At least one claim id is in both proposals: the claims are paired by id.
   * `partial` when some id is in only one of them: ids alone cannot tell
   * whether such a claim was added or dropped or restated under another id.
   */
  | { pairing: "id"; changes: ClaimChange[]; partial: boolean }
  /** No claim id is in both: nothing in the records pairs them; both lists, as recorded. */
  | { pairing: "none"; original: Claim[]; revised: Claim[] };

/** How the records let the claims of `revised` relate to those of `original`. */
export function claimComparison(original: Proposal, revised: Proposal): ClaimComparison {
  const originalIds = new Set(original.claims.map((claim) => claim.id));
  if (!revised.claims.some((claim) => originalIds.has(claim.id))) {
    return { pairing: "none", original: original.claims, revised: revised.claims };
  }
  const changes = compareClaims(original, revised);
  return {
    pairing: "id",
    changes,
    partial: changes.some((change) => change.kind === "added" || change.kind === "removed"),
  };
}

/**
 * True when the run records at least one revision and every revision's
 * invocation names the Meta model adapter (provider "meta"): only then is
 * the adapter's numbering the recorded reason no claim id is shared. A
 * scripted double (provider "fake") keeps a script's ids as written, so for
 * any other run the view states no cause.
 */
export function revisedByMetaModel(detail: RunDetailV1): boolean {
  const providers = detail.events.flatMap((event) =>
    event.payload.type === "revision_submitted"
      ? // An archived record may predate the field: it then names no adapter.
        [(event.payload.invocation as ModelInvocationV1 | null | undefined)?.provider ?? null]
      : [],
  );
  return providers.length > 0 && providers.every((provider) => provider === "meta");
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
