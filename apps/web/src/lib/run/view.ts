// SPDX-License-Identifier: Apache-2.0
//
// What the run workspace shows, computed from the run's own data: the
// newest known summary, the records (the stream's frames and the detail's
// events, by sequence), the latest document of each role, and the counts of
// the runtime lane. Pure functions, unit-tested in tests/run-view.test.ts.
//
// The stage follows the stream, so client-side pacing can space replayed
// frames out; the detail (GET /runs/{id}) fills what the stream did not
// deliver: a record the stream skipped (a gap below the newest frame shown)
// at once, and, once the stream is settled (ended, failed or closed) and
// nothing waits in the pacer, every record after the newest frame. So a
// stream that fails, or loses records, never makes the stage contradict the
// run: its counts come from the same records as the header's. A record known
// only from the detail reads as replayed (it was stored before this client
// saw it).

import type {
  Challenge,
  ConsensusFrameV1,
  Delivery,
  Evaluation,
  EventV1,
  ModelInvocationV1,
  Proposal,
  Recommendation,
  RunDetailV1,
  RunState,
  RunSummaryV1,
  StreamEndReason,
  VerificationV1,
} from "../api/types";
import type { StreamState } from "../api/sse";

/** The detail to keep: a response never replaces a detail that is further along. */
export function newerDetail(next: RunDetailV1, current: RunDetailV1 | null): RunDetailV1 {
  return current === null || next.summary.last_sequence >= current.summary.last_sequence
    ? next
    : current;
}

/**
 * The newest known summary: the detail's, unless the last frame is further
 * along (while the stream follows a run, or when a re-read failed). Never an
 * older one: a failed re-read keeps what the stream already showed.
 */
export function newestSummary(
  detail: RunDetailV1 | null,
  frames: ConsensusFrameV1[],
): RunSummaryV1 | null {
  const fromDetail = detail?.summary ?? null;
  const fromFrame = frames.at(-1)?.summary ?? null;
  if (fromDetail === null) {
    return fromFrame;
  }
  if (fromFrame === null) {
    return fromDetail;
  }
  return fromFrame.last_sequence > fromDetail.last_sequence ? fromFrame : fromDetail;
}

/** The stream ended, failed or was closed: no frame will follow. */
export function streamSettled(stream: StreamState): boolean {
  return stream === "ended" || stream === "error" || stream === "closed";
}

/** Where a paced playback stands, as the run data knows it. */
export type PlaybackPosition = {
  /** Frames that went through the pacer (0: nothing was paced). */
  pacedFrames: number;
  /** Replayed frames the pacer still holds back. */
  pending: number;
  /** The stream ended, failed or was closed: no frame will follow. */
  settled: boolean;
};

/**
 * Whether the run's views follow a paced playback rather than the run's
 * final state: frames went through the pacer and the playback has not
 * reached its end yet (frames still queued, or the stream may still send
 * some). The same condition keeps the stage from showing the detail's tail
 * (stageRecords), so the header and the stage share one clock.
 */
export function followsPlayback(position: PlaybackPosition): boolean {
  return position.pacedFrames > 0 && !(position.settled && position.pending === 0);
}

/**
 * The summary the run header shows. During a paced playback it is the
 * summary carried by the newest frame shown: the header follows the
 * playback position, so no outcome (and no decision) appears before the
 * stage reaches it. Otherwise it is the newest known summary.
 */
export function headerSummary(
  detail: RunDetailV1 | null,
  frames: ConsensusFrameV1[],
  position: PlaybackPosition,
): RunSummaryV1 | null {
  const shown = frames.at(-1)?.summary ?? null;
  if (followsPlayback(position) && shown !== null) {
    return shown;
  }
  return newestSummary(detail, frames);
}

/**
 * Why the stream ended, as the status line says it: derived from the run's
 * state, not from the stream's end frame alone. A run decided after its
 * stream ended (a decision on the Decision tab, or a stream that stopped at
 * the decision point before a newer read) is terminal, never "awaits a human
 * decision"; a run still at its decision point awaits it. Any other state
 * (none is expected once a stream ended) keeps the frame's own reason.
 */
export function endReasonOf(state: RunState | null, reason: StreamEndReason): StreamEndReason {
  switch (state) {
    case "awaiting_human_approval":
      return "awaiting_human_approval";
    case "approved":
    case "rejected":
    case "failed":
      return "terminal";
    default:
      return reason;
  }
}

/**
 * Whether a scripted run's playback has not shown its first record yet: the
 * pacing is on, no frame is on the stage and the stream may still send one.
 * The stored summary is the run's ending, so the header shows nothing of it
 * (no state, outcome or decision) until the playback starts; a stream that
 * failed or was closed settles it, and the stored run shows.
 */
export function awaitsFirstFrame(options: {
  scripted: boolean;
  pacing: "on" | "reduced" | "skipped";
  framesShown: number;
  settled: boolean;
}): boolean {
  return options.scripted && options.pacing === "on" && options.framesShown === 0 && !options.settled;
}

/** What the stream status line says. */
export type StreamLine =
  /** The stream's own state (connecting, open, reconnecting, failed, closed). */
  | { kind: "state"; stream: StreamState }
  /** The stream ended, but the paced playback has not reached its end yet. */
  | { kind: "playing" }
  /** The stream ended and the views show the run's newest state. */
  | { kind: "ended"; reason: StreamEndReason };

/**
 * The status line, on the same clock as the header: while a paced playback
 * is followed, an ended stream says the playback continues (never the run's
 * final state, which the header and the stage have not reached); once the
 * playback reached its end, the reason comes from the run's newest state
 * (endReasonOf).
 */
export function streamLine(
  stream: StreamState,
  end: { reason: StreamEndReason; state: RunState } | null,
  position: PlaybackPosition,
  newestState: RunState | null,
): StreamLine {
  if (stream !== "ended" || end === null) {
    return { kind: "state", stream };
  }
  if (followsPlayback(position)) {
    return { kind: "playing" };
  }
  return { kind: "ended", reason: endReasonOf(newestState ?? end.state, end.reason) };
}

/** The person's decision on a run, from its summary: null while none is recorded. */
export function decisionOf(
  summary: Pick<RunSummaryV1, "authorization" | "override">,
): "approved" | "approvedOverride" | "rejected" | null {
  switch (summary.authorization) {
    case "approved":
      return summary.override ? "approvedOverride" : "approved";
    case "rejected":
      return "rejected";
    case "pending":
      return null;
  }
}

/** One record as a view shows it: the event and how it reached this client. */
export type ShownRecord = {
  event: EventV1;
  /** The frame's delivery; `replay` for a record known only from the detail. */
  delivery: Delivery;
  /** True when the record came from the detail, not from the stream. */
  fromDetail: boolean;
};

export type StageOptions = {
  /** The stream ended, failed or was closed: no frame will follow. */
  settled: boolean;
  /** Replayed frames the pacer still holds back. */
  pending: number;
};

/** The union of the frames shown so far and the detail's events the stage may show. */
export function stageRecords(
  detail: RunDetailV1 | null,
  frames: ConsensusFrameV1[],
  options: StageOptions,
): ShownRecord[] {
  const bySequence = new Map<number, ShownRecord>();
  let newestFrame = 0;
  for (const frame of frames) {
    bySequence.set(frame.event.sequence, {
      event: frame.event,
      delivery: frame.delivery,
      fromDetail: false,
    });
    newestFrame = Math.max(newestFrame, frame.event.sequence);
  }
  const tailAllowed = options.settled && options.pending === 0;
  for (const event of detail?.events ?? []) {
    if (bySequence.has(event.sequence)) {
      continue;
    }
    if (event.sequence < newestFrame || tailAllowed) {
      bySequence.set(event.sequence, { event, delivery: "replay", fromDetail: true });
    }
  }
  return [...bySequence.values()].sort((a, b) => a.event.sequence - b.event.sequence);
}

/** Every event of the detail and the frames, by sequence (the Explorer's timeline). */
export function mergeEvents(detail: RunDetailV1 | null, frames: ConsensusFrameV1[]): EventV1[] {
  const bySequence = new Map<number, EventV1>();
  for (const event of detail?.events ?? []) {
    bySequence.set(event.sequence, event);
  }
  for (const frame of frames) {
    if (!bySequence.has(frame.event.sequence)) {
      bySequence.set(frame.event.sequence, frame.event);
    }
  }
  return [...bySequence.values()].sort((a, b) => a.sequence - b.sequence);
}

export type Carried<T> = { document: T; record: ShownRecord };

export type Stage = {
  proposal: Carried<{
    proposal: Proposal;
    supersedes: string | null;
    invocation: ModelInvocationV1;
  }> | null;
  challenge: Carried<{ challenge: Challenge; invocation: ModelInvocationV1 }> | null;
  evaluation: Carried<{ evaluation: Evaluation; invocation: ModelInvocationV1 }> | null;
  recommendation: Carried<Recommendation> | null;
  retrieved: number;
  unavailable: number;
  proven: number;
  verifications: Carried<VerificationV1>[];
};

/** The latest document of each role and the lane's counts, from the records given. */
export function stageOf(records: ShownRecord[]): Stage {
  const stage: Stage = {
    proposal: null,
    challenge: null,
    evaluation: null,
    recommendation: null,
    retrieved: 0,
    unavailable: 0,
    proven: 0,
    verifications: [],
  };
  for (const record of records) {
    const payload = record.event.payload;
    switch (payload.type) {
      case "proposal_submitted":
        stage.proposal = {
          record,
          document: { proposal: payload.proposal, supersedes: null, invocation: payload.invocation },
        };
        break;
      case "revision_submitted":
        stage.proposal = {
          record,
          document: {
            proposal: payload.revision.proposal,
            supersedes: payload.revision.supersedes,
            invocation: payload.invocation,
          },
        };
        break;
      case "challenge_submitted":
        stage.challenge = {
          record,
          document: { challenge: payload.challenge, invocation: payload.invocation },
        };
        break;
      case "evaluation_submitted":
        stage.evaluation = {
          record,
          document: { evaluation: payload.evaluation, invocation: payload.invocation },
        };
        break;
      case "recommendation_issued":
        stage.recommendation = { record, document: payload.recommendation };
        break;
      case "evidence_retrieved":
        stage.retrieved += 1;
        break;
      case "evidence_unavailable":
        stage.unavailable += 1;
        break;
      case "recall_proven":
        stage.proven += 1;
        break;
      case "evidence_verified":
        stage.verifications.push({ record, document: payload.verification });
        break;
      default:
        break;
    }
  }
  return stage;
}

/**
 * The original proposal when the current one is a revision of it (K03/K04:
 * the recommendation is then for a proposal other than the first one), by
 * following the chain of revisions back to the first proposal.
 */
export function supersededProposal(detail: RunDetailV1): Proposal | null {
  const current = detail.current_proposal;
  if (current === undefined || current === null) {
    return null;
  }
  const proposals = new Map<string, Proposal>();
  const supersedesOf = new Map<string, string>();
  for (const event of detail.events) {
    const payload = event.payload;
    if (payload.type === "proposal_submitted") {
      proposals.set(payload.proposal.id, payload.proposal);
    } else if (payload.type === "revision_submitted") {
      proposals.set(payload.revision.proposal.id, payload.revision.proposal);
      supersedesOf.set(payload.revision.proposal.id, payload.revision.supersedes);
    }
  }
  let id = supersedesOf.get(current.id);
  if (id === undefined) {
    return null;
  }
  const seen = new Set<string>([current.id]);
  while (supersedesOf.has(id) && !seen.has(id)) {
    seen.add(id);
    const earlier = supersedesOf.get(id);
    if (earlier === undefined || !proposals.has(earlier)) {
      break;
    }
    id = earlier;
  }
  return proposals.get(id) ?? null;
}
