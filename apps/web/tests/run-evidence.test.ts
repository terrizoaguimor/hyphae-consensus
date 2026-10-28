// SPDX-License-Identifier: Apache-2.0
//
// The Evidence tab and the Proof inspector's records (src/lib/run/evidence.ts)
// over the contract's guided fixtures and variants derived from them: every
// retrieved record with its verifications by round, failed retrievals, what a
// proof covers (from the proof's own request, never from the claim: the
// sealed snapshot's complete record list at its anchor, a live recall's own
// request, a pre-1.3 record that names none), and how a proof is read (a
// simulated proof is SIMULATED, a proof whose generation failed upstream was
// never checked).

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type {
  ConsensusFrameV1,
  EventV1,
  EvidenceReferenceV1,
  RunDetailV1,
  SseFrameV1,
  VerificationV1,
} from "../src/lib/api/types";
import {
  byRound,
  digestPrefix,
  evidenceView,
  generationFailures,
  proofReading,
  roundedVerifications,
} from "../src/lib/run/evidence";

function fixture<T>(name: string): T {
  return JSON.parse(readFileSync(path.join(contractDir(), "fixtures", name), "utf8")) as T;
}

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

const ANCHOR = "d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168";

/** The retry run with its evidence and verifications rewritten by `edit`. */
function variant(edit: (evidence: EvidenceReferenceV1, verification: VerificationV1[]) => void): RunDetailV1 {
  const detail = clone(fixture<RunDetailV1>("run-detail-retry.json"));
  const verifications = detail.events.flatMap((e) =>
    e.payload.type === "evidence_verified" ? [e.payload.verification] : [],
  );
  for (const event of detail.events) {
    if (event.payload.type === "evidence_retrieved") {
      edit(
        event.payload.evidence,
        verifications.filter((v) => v.evidence_id === (event.payload as { evidence: EvidenceReferenceV1 }).evidence.id),
      );
    }
  }
  return detail;
}

describe("evidenceView over the guided fixtures", () => {
  it("lists every retrieved record with its verifications by round (retry)", () => {
    const detail = fixture<RunDetailV1>("run-detail-retry.json");
    const view = evidenceView(detail, []);
    expect(view.records.map((r) => r.evidence.id)).toEqual(["ev-constraint", "ev-fact"]);
    expect(view.failed).toEqual([]);
    expect(view.orphanVerifications).toEqual([]);
    const constraint = view.records[0];
    expect(constraint?.sequences).toEqual([3]);
    expect(constraint?.coverage).toEqual({ kind: "simulated" });
    expect(byRound(constraint?.verifications ?? []).map(([round, list]) => [round, list.length])).toEqual([
      [1, 1],
      [2, 1],
    ]);
    // Every verification of the run is listed under its record, none twice.
    const shown = view.records.reduce((n, r) => n + r.verifications.length, 0);
    expect(shown).toBe(4);
    for (const record of view.records) {
      for (const { verification } of record.verifications) {
        expect(proofReading(verification, record.evidence)).toEqual({ kind: "simulated", status: "valid" });
      }
    }
  });

  it("lists failed retrievals with their code (offline)", () => {
    const view = evidenceView(fixture<RunDetailV1>("run-detail-offline.json"), []);
    expect(view.records).toEqual([]);
    expect(view.failed.map((f) => f.code)).toEqual([
      "memory_unavailable",
      "memory_unavailable",
      "memory_unavailable",
      "memory_unavailable",
    ]);
    expect(view.failed[0]?.query).toBe("Payment timeouts are caused by insufficient retries");
  });

  it("a simulated invalid proof stays SIMULATED, never a generation failure (invalid_proof)", () => {
    const view = evidenceView(fixture<RunDetailV1>("run-detail-invalid_proof.json"), []);
    const constraint = view.records.find((r) => r.evidence.id === "ev-constraint");
    const verification = constraint?.verifications[0]?.verification;
    expect(verification?.integrity_status).toBe("invalid");
    expect(verification === undefined ? null : proofReading(verification, constraint?.evidence)).toEqual({
      kind: "simulated",
      status: "invalid",
    });
    expect(generationFailures(view)).toEqual([]);
  });

  it("reads the records the frames carry when the detail is not there", () => {
    const frames: ConsensusFrameV1[] = fixture<SseFrameV1[]>("sse-frames-retry.json")
      .filter((f): f is Extract<SseFrameV1, { frame: "consensus" }> => f.frame === "consensus")
      .map((f) => ({ delivery: f.delivery, event: f.event, summary: f.summary }));
    const view = evidenceView(null, frames.slice(0, 7));
    expect(view.records.map((r) => r.evidence.id)).toEqual(["ev-constraint", "ev-fact"]);
    expect(view.records[0]?.verifications.map((v) => v.round)).toEqual([1]);
  });
});

describe("what a proof covers", () => {
  it("the sealed snapshot: the complete record list of its project at its anchor, from the proof's own request", () => {
    const detail = variant((evidence) => {
      evidence.retrieval = "sealed_snapshot";
      evidence.anchor = ANCHOR;
      evidence.snapshot_id = "snap-2026-10-29";
      evidence.captured_at = "2026-10-29T10:00:00Z";
      evidence.proof_request = { operation: "list", project: "demo-repo", limit: 64 };
    });
    const [record] = evidenceView(detail, []).records;
    expect(record?.coverage).toEqual({ kind: "snapshot_list", project: "demo-repo", anchor: ANCHOR });
    expect(record?.evidence.snapshot_id).toBe("snap-2026-10-29");
  });

  it("a live recall: the request the proof sealed, never the claim's query", () => {
    const sealed = { operation: "recall", project: "demo-repo", query: "the proof's own query", limit: 5 };
    const detail = variant((evidence) => {
      evidence.retrieval = "live_recall";
      evidence.anchor = ANCHOR;
      evidence.proof_request = sealed;
    });
    const view = evidenceView(detail, []);
    const claims = detail.events.flatMap((e) =>
      e.payload.type === "proposal_submitted" ? e.payload.proposal.claims.map((c) => c.statement) : [],
    );
    for (const record of view.records) {
      expect(record.coverage).toEqual({ kind: "request", request: sealed, anchor: ANCHOR });
      expect(claims).not.toContain(sealed.query);
    }
  });

  it("a record from before schema 1.3 names no request, and says so with its anchor", () => {
    const detail = variant((evidence, verifications) => {
      delete evidence.retrieval;
      evidence.anchor = ANCHOR;
      for (const verification of verifications) {
        verification.simulated = false;
        verification.verifier = "hyphae proof verify";
      }
    });
    const [record] = evidenceView(detail, []).records;
    expect(record?.coverage).toEqual({ kind: "unrecorded", anchor: ANCHOR });
    const verification = record?.verifications[0]?.verification;
    expect(verification === undefined ? null : proofReading(verification, record?.evidence)).toEqual({
      kind: "accepted",
    });
  });

  it("a pre-1.3 record checked only by the scripted verifier reads simulated", () => {
    const detail = variant((evidence) => {
      delete evidence.retrieval;
    });
    expect(evidenceView(detail, []).records[0]?.coverage).toEqual({ kind: "simulated" });
  });
});

describe("proof generation failures", () => {
  it("an invalid proof of a record that reports a proof error was never checked", () => {
    const detail = variant((evidence, verifications) => {
      delete evidence.retrieval;
      evidence.proof_error_reported = true;
      for (const verification of verifications) {
        verification.simulated = false;
        verification.integrity_status = "invalid";
        verification.verifier = "recall(prove) upstream";
      }
    });
    const view = evidenceView(detail, []);
    for (const record of view.records) {
      expect(record.coverage).toEqual({ kind: "proof_error" });
      for (const { verification } of record.verifications) {
        expect(proofReading(verification, record.evidence)).toEqual({ kind: "generation_failed" });
      }
    }
    expect(generationFailures(view)).toEqual(["ev-constraint", "ev-fact"]);
  });

  it("is not claimed when any invalid proof was rejected by a verifier", () => {
    const detail = variant((evidence, verifications) => {
      evidence.proof_error_reported = evidence.id === "ev-fact";
      for (const verification of verifications) {
        verification.simulated = false;
        verification.integrity_status = "invalid";
        verification.verifier = "hyphae proof verify";
      }
    });
    const view = evidenceView(detail, []);
    const constraint = view.records.find((r) => r.evidence.id === "ev-constraint");
    const verification = constraint?.verifications[0]?.verification;
    expect(verification === undefined ? null : proofReading(verification, constraint?.evidence)).toEqual({
      kind: "rejected",
    });
    expect(generationFailures(view)).toEqual([]);
  });

  it("unavailable, verification_error and not_checked are not verified", () => {
    const base = fixture<RunDetailV1>("run-detail-retry.json");
    const event = base.events.find((e) => e.payload.type === "evidence_verified");
    if (event === undefined || event.payload.type !== "evidence_verified") {
      throw new Error("no verification in the retry fixture");
    }
    for (const status of ["unavailable", "verification_error", "not_checked"] as const) {
      const verification = { ...event.payload.verification, simulated: false, integrity_status: status };
      expect(proofReading(verification, undefined)).toEqual({ kind: "not_verified", status });
    }
  });
});

describe("rounds and record identity", () => {
  it("a verification without its own round takes the round of its position", () => {
    const detail = clone(fixture<RunDetailV1>("run-detail-retry.json"));
    for (const event of detail.events) {
      if (event.payload.type === "evidence_verified") {
        delete event.payload.verification.round;
      }
    }
    expect(roundedVerifications(detail.events).map((v) => v.round)).toEqual([1, 1, 2, 2]);
  });

  it("a record retrieved again is one record with both sequences; a verification of no record is kept", () => {
    const detail = clone(fixture<RunDetailV1>("run-detail-retry.json"));
    const retrieval = detail.events.find((e) => e.payload.type === "evidence_retrieved") as EventV1;
    const again: EventV1 = { ...clone(retrieval), sequence: 99, event_id: "evt-again" };
    const orphanSource = detail.events.find((e) => e.payload.type === "evidence_verified") as EventV1;
    const orphan = clone(orphanSource);
    orphan.sequence = 100;
    if (orphan.payload.type === "evidence_verified") {
      orphan.payload.verification.evidence_id = "ev-never-retrieved";
    }
    detail.events = [...detail.events, again, orphan];
    const view = evidenceView(detail, []);
    expect(view.records).toHaveLength(2);
    expect(view.records[0]?.sequences).toEqual([3, 99]);
    expect(view.orphanVerifications.map((v) => v.verification.evidence_id)).toEqual(["ev-never-retrieved"]);
  });

  it("shows a prefix of a digest, and nothing for a missing one", () => {
    expect(digestPrefix(ANCHOR)).toBe("d91ac10d17ce");
    expect(digestPrefix("short")).toBe("short");
    expect(digestPrefix(null)).toBeNull();
    expect(digestPrefix("")).toBeNull();
  });
});
