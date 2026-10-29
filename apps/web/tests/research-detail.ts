// SPDX-License-Identifier: Apache-2.0
//
// A recorded run of the research archive as the parts of GET /showcase/{id}
// (RunDetailV1) the explanation reads, built from the run's own records in
// its archived log, for the unit tests: the events as the public view shows
// them (crates/consensus-api/src/public_view.rs event_v1: a record's
// proof_error becomes proof_error_reported, a verification is simulated when
// the scripted verifier made it), the rounds (public_view.rs `rounds`: each
// challenge and evaluation by position) and the challenge items with their
// status (public_view.rs `challenge_items`). These are ports of the server's
// derivations, for the archive's bytes only; tests/run-explain.test.ts checks
// what they yield against the counts the Rust research test pins
// (crates/consensus-tests/tests/research_receipts.rs NEVER_REJUDGED), so a
// port that drifted from the server would fail there.

import type { ChallengeItemV1, RunDetailV1 } from "../src/lib/api/types";

import { researchBytes } from "./research";

type Json = Record<string, unknown>;

/** The scripted verifier's name (public_view.rs FAKE_VERIFIER). */
const FAKE_VERIFIER = "fake-verifier";

/** Each archived log's records, parsed once per test file (the archive is read-only). */
const logs = new Map<string, Json[]>();

/** A run's records in its archived log, in order (copies: callers may change them). */
export function runRecords(file: string, runId: string): Json[] {
  let records = logs.get(file);
  if (records === undefined) {
    records = new TextDecoder()
      .decode(researchBytes(file))
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => JSON.parse(line) as Json);
    logs.set(file, records);
  }
  return records
    .filter((record) => record.run_id === runId)
    .map((record) => JSON.parse(JSON.stringify(record)) as Json);
}

type RawItem = { id?: string; kind: string; claim_id?: string | null; description: string };
type RawChallenge = { id: string; items: RawItem[] };
type RawEvaluation = {
  unresolved_items?: string[];
  /** Pre-1.1 name of unresolved_items (proposal.rs serde alias). */
  unresolved_challenges?: string[];
  item_verdicts?: Array<{ item_id: string; status: string; reason: string }>;
};

const unresolved = (evaluation: RawEvaluation): string[] =>
  evaluation.unresolved_items ?? evaluation.unresolved_challenges ?? [];

/** The event as the public view shows it. */
function publicEvent(event: Json): Json {
  const payload = event.payload as Json;
  if (payload.type === "evidence_retrieved") {
    const evidence = payload.evidence as Json;
    evidence.proof_error_reported = evidence.proof_error !== undefined && evidence.proof_error !== null;
  } else if (payload.type === "evidence_verified") {
    const verification = payload.verification as Json;
    verification.simulated = verification.verifier === FAKE_VERIFIER;
  } else if (payload.type === "evaluation_submitted") {
    const evaluation = payload.evaluation as RawEvaluation & Json;
    evaluation.unresolved_items = unresolved(evaluation);
  }
  return event;
}

/** public_view.rs `rounds`: challenges, evaluations and verifications by position. */
function rounds(events: Json[]): Json[] {
  const out: Json[] = [];
  let evaluations = 0;
  const ensure = (round: number) => {
    while (out.length < round) {
      out.push({ round: out.length + 1, challenge: null, evaluation: null, verifications: [] });
    }
  };
  for (const event of events) {
    const payload = event.payload as Json;
    if (payload.type === "challenge_submitted") {
      ensure(evaluations + 1);
      (out[evaluations] as Json).challenge = payload.challenge;
    } else if (payload.type === "evaluation_submitted") {
      evaluations += 1;
      ensure(evaluations);
      (out[evaluations - 1] as Json).evaluation = payload.evaluation;
    } else if (payload.type === "evidence_verified") {
      const verification = payload.verification as Json;
      const own = verification.round as number | null | undefined;
      const round = Math.max(1, own ?? evaluations);
      ensure(round);
      ((out[round - 1] as Json).verifications as Json[]).push(verification);
    }
  }
  return out;
}

/** public_view.rs `challenge_items`: every Critic objection with its status. */
function challengeItems(events: Json[]): ChallengeItemV1[] {
  const tracks: Array<{ item: RawItem; challenge: string; raised: number; verdict: string | null; reason: string | null }> = [];
  let done = 0;
  let finalUnresolved: string[] = [];
  for (const event of events) {
    const payload = event.payload as Json;
    if (payload.type === "challenge_submitted") {
      const challenge = payload.challenge as RawChallenge;
      for (const item of challenge.items) {
        tracks.push({ item, challenge: challenge.id, raised: done + 1, verdict: null, reason: null });
      }
    } else if (payload.type === "evaluation_submitted") {
      const evaluation = payload.evaluation as RawEvaluation;
      done += 1;
      for (const verdict of evaluation.item_verdicts ?? []) {
        const track = tracks.find((t) => (t.item.id ?? "") === verdict.item_id);
        if (track !== undefined) {
          track.verdict = verdict.status;
          track.reason = verdict.reason;
        }
      }
      finalUnresolved = unresolved(evaluation);
    }
  }
  return tracks.map((t) => {
    const id = t.item.id ?? "";
    const listed = finalUnresolved.includes(id) || finalUnresolved.includes(t.challenge);
    let status: ChallengeItemV1["status"];
    if (t.verdict === "resolved") {
      status = "resolved";
    } else if (t.verdict === "unresolved" || listed || t.raised > done) {
      status = "open";
    } else if (t.raised === done) {
      status = "resolved";
    } else {
      status = "never_rejudged";
    }
    return {
      id,
      kind: t.item.kind as ChallengeItemV1["kind"],
      claim_id: t.item.claim_id ?? null,
      description: t.item.description,
      raised_in_round: t.raised,
      resolution_reason: t.verdict === "resolved" ? t.reason : null,
      status,
    };
  });
}

/** The parts of a recorded run's detail the explanation reads, from its archived records. */
export function recordedDetail(file: string, runId: string): RunDetailV1 {
  const events = runRecords(file, runId).map(publicEvent);
  if (events.length === 0) {
    throw new Error(`no records of ${runId} in ${file}`);
  }
  const task = ((events[0] as Json).payload as Json).task as Json | undefined;
  return {
    task: task ?? null,
    events,
    rounds: rounds(events),
    challenge_items: challengeItems(events),
  } as unknown as RunDetailV1;
}
