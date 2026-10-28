// SPDX-License-Identifier: Apache-2.0
//
// The Evidence tab and the Proof inspector, computed from the run's own
// records (src/lib/run/view.ts mergeEvents: every event of the detail and of
// the frames, by sequence): each record the run retrieved, its verifications
// by deliberation round, what its proof covers, and how each proof is to be
// read. Pure functions, unit-tested in tests/run-evidence.test.ts.
//
// What a proof covers is said from the proof's OWN request
// (EvidenceReferenceV1.proof_request, schema 1.3), never from the claim or
// the query that led to the retrieval (ADR-0008, decision 4): for the sealed
// snapshot, the complete record list of its project at its anchor; for a
// live recall, the recall the proof sealed. A record that names no request
// (before schema 1.3) says so instead of guessing. Integrity is read on its
// own axis: a simulated proof checked nothing, a proof whose generation
// failed upstream was never checked, and a valid proof says nothing about
// whether the record is true, relevant or complete.

import type {
  ConsensusFrameV1,
  EventV1,
  EvidenceReferenceV1,
  IntegrityStatus,
  ProofRequest,
  RunDetailV1,
  VerificationV1,
} from "../api/types";
import { mergeEvents } from "./view";

/** One verification record, with the round it belongs to and where it is in the log. */
export type RoundedVerification = {
  verification: VerificationV1;
  /**
   * The deliberation round (1 = first): the record's own `round` (schema
   * 1.3), else its position (the records after the k-th evaluation are
   * round k), at least 1, as the detail's `rounds` group them.
   */
  round: number;
  sequence: number;
};

/** What a record's proof covers, from the record's own fields. */
export type Coverage =
  /** A simulated record: no proof covers it. */
  | { kind: "simulated" }
  /** Proof generation failed upstream for this record: there is no proof. */
  | { kind: "proof_error" }
  /** The sealed snapshot: the proof's own request lists every record of the project. */
  | { kind: "snapshot_list"; project: string; anchor: string | null }
  /** Any other recorded request: the operation, project, query and limit the proof sealed. */
  | { kind: "request"; request: ProofRequest; anchor: string | null }
  /** A record that names no request (before schema 1.3). */
  | { kind: "unrecorded"; anchor: string | null };

/** One record the run retrieved (an evidence id with one text), with everything recorded about it. */
export type RetrievedRecord = {
  evidence: EvidenceReferenceV1;
  /** Sequences of the evidence_retrieved records carrying this id and text. */
  sequences: number[];
  /** The time of the first of those records. */
  occurredAt: string;
  /** Every verification of this evidence id, in log order. */
  verifications: RoundedVerification[];
  coverage: Coverage;
};

/** A retrieval that returned nothing usable (evidence_unavailable). */
export type FailedRetrieval = {
  sequence: number;
  code: string | null;
  /** The query of the failed retrieval: model or task text, not a proof request. */
  query: string;
};

export type EvidenceView = {
  records: RetrievedRecord[];
  /** Verifications of an evidence id the run never retrieved (shown, never dropped). */
  orphanVerifications: RoundedVerification[];
  failed: FailedRetrieval[];
};

function coverageOf(evidence: EvidenceReferenceV1, verifications: RoundedVerification[]): Coverage {
  if (evidence.proof_error_reported) {
    return { kind: "proof_error" };
  }
  const anchor = evidence.anchor ?? null;
  if (evidence.retrieval === "simulated") {
    return { kind: "simulated" };
  }
  const request = evidence.proof_request ?? null;
  if (request !== null) {
    if (evidence.retrieval === "sealed_snapshot" && request.operation === "list") {
      return { kind: "snapshot_list", project: request.project, anchor };
    }
    return { kind: "request", request, anchor };
  }
  // Records before schema 1.3 name no retrieval mode; the scripted verifier
  // (simulated) is the one public sign that such a record was simulated.
  if (
    (evidence.retrieval === undefined || evidence.retrieval === null) &&
    verifications.length > 0 &&
    verifications.every((v) => v.verification.simulated)
  ) {
    return { kind: "simulated" };
  }
  return { kind: "unrecorded", anchor };
}

/** Every verification of the events, with its round, in log order. */
export function roundedVerifications(events: EventV1[]): RoundedVerification[] {
  const out: RoundedVerification[] = [];
  let evaluations = 0;
  for (const event of events) {
    const payload = event.payload;
    if (payload.type === "evaluation_submitted") {
      evaluations += 1;
    } else if (payload.type === "evidence_verified") {
      const own = payload.verification.round;
      out.push({
        verification: payload.verification,
        round: Math.max(1, own === undefined || own === null ? evaluations : own),
        sequence: event.sequence,
      });
    }
  }
  return out;
}

/** The Evidence tab's records, from the detail and the frames. */
export function evidenceView(detail: RunDetailV1 | null, frames: ConsensusFrameV1[]): EvidenceView {
  const events = mergeEvents(detail, frames);
  const verifications = roundedVerifications(events);
  const byId = new Map<string, RoundedVerification[]>();
  for (const verification of verifications) {
    const id = verification.verification.evidence_id;
    byId.set(id, [...(byId.get(id) ?? []), verification]);
  }
  const records: RetrievedRecord[] = [];
  const failed: FailedRetrieval[] = [];
  for (const event of events) {
    const payload = event.payload;
    if (payload.type === "evidence_retrieved") {
      const evidence = payload.evidence;
      // The same id with the same text is one record retrieved again; the
      // same id with another text is shown as a record of its own.
      const same = records.find(
        (r) => r.evidence.id === evidence.id && r.evidence.content === evidence.content,
      );
      if (same !== undefined) {
        same.sequences.push(event.sequence);
        continue;
      }
      const own = byId.get(evidence.id) ?? [];
      records.push({
        evidence,
        sequences: [event.sequence],
        occurredAt: event.occurred_at,
        verifications: own,
        coverage: coverageOf(evidence, own),
      });
    } else if (payload.type === "evidence_unavailable") {
      failed.push({ sequence: event.sequence, code: payload.code ?? null, query: payload.query });
    }
  }
  const retrievedIds = new Set(records.map((r) => r.evidence.id));
  const orphanVerifications = verifications.filter(
    (v) => !retrievedIds.has(v.verification.evidence_id),
  );
  return { records, orphanVerifications, failed };
}

/** The verifications of one record, grouped by round, rounds ascending. */
export function byRound(verifications: RoundedVerification[]): Array<[number, RoundedVerification[]]> {
  const rounds = new Map<number, RoundedVerification[]>();
  for (const verification of verifications) {
    rounds.set(verification.round, [...(rounds.get(verification.round) ?? []), verification]);
  }
  return [...rounds.entries()].sort((a, b) => a[0] - b[0]);
}

/** How one proof is to be read, from the verification and the record it checked. */
export type ProofReading =
  /** The scripted verifier: an outcome was scripted, nothing was computed. */
  | { kind: "simulated"; status: IntegrityStatus }
  /** Integrity invalid because no proof could be generated upstream for the record. */
  | { kind: "generation_failed" }
  /** A verifier ran and accepted the proof. */
  | { kind: "accepted" }
  /** A verifier ran and did not accept the proof. */
  | { kind: "rejected" }
  /** No proof was verified for the record. */
  | { kind: "not_verified"; status: Exclude<IntegrityStatus, "valid" | "invalid"> };

export function proofReading(
  verification: VerificationV1,
  evidence: EvidenceReferenceV1 | undefined,
): ProofReading {
  const status = verification.integrity_status;
  if (verification.simulated) {
    return { kind: "simulated", status };
  }
  switch (status) {
    case "valid":
      return { kind: "accepted" };
    case "invalid":
      return evidence?.proof_error_reported === true
        ? { kind: "generation_failed" }
        : { kind: "rejected" };
    case "unavailable":
    case "verification_error":
    case "not_checked":
      return { kind: "not_verified", status };
  }
}

/** The first characters of a digest or anchor, for display next to its label. */
export function digestPrefix(value: string | null | undefined, length = 12): string | null {
  if (value === undefined || value === null || value === "") {
    return null;
  }
  return value.length <= length ? value : value.slice(0, length);
}

/**
 * Evidence ids whose every invalid proof is a generation failure reported
 * upstream (the record carries proof_error_reported): the run's invalid
 * statuses then record that no proof could be produced, not that a check
 * found a record altered. Empty unless every invalid verification is one.
 */
export function generationFailures(view: EvidenceView): string[] {
  const invalid = [
    ...view.records.flatMap((r) =>
      r.verifications
        .filter((v) => !v.verification.simulated && v.verification.integrity_status === "invalid")
        .map(() => ({ id: r.evidence.id, failed: r.evidence.proof_error_reported })),
    ),
    ...view.orphanVerifications
      .filter((v) => !v.verification.simulated && v.verification.integrity_status === "invalid")
      .map((v) => ({ id: v.verification.evidence_id, failed: false })),
  ];
  if (invalid.length === 0 || invalid.some((entry) => !entry.failed)) {
    return [];
  }
  return [...new Set(invalid.map((entry) => entry.id))];
}
