"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The chain checked in this browser (src/lib/chain/verify.ts): the verbatim
// log is downloaded as bytes (a recorded run's research file, or this
// server's events.ndjson of a run whose log the public surface publishes),
// every record's hash is recomputed with WebCrypto from the raw line with
// its hash blanked textually (crates/consensus-runtime/src/event_log.rs
// hashes its re-serialization instead: the two agree on the canonical lines
// the runtime writes, and on any other line they can disagree either way; it reads
// only the chain fields and replays nothing, see the header of verify.ts and
// the Run.chain.limits text), every link is followed, and the
// run's own segment is compared with what the views hold: the file's SHA-256
// with the digest the archive's manifest pins (the manifest's own entry once
// this browser checked the manifest, else the listing's copy of it,
// ShowcaseEntryV1.sha256, and said to be that), the
// head (the hash of the run's last record) with the receipt's, the record
// count with the receipt's, the first and last sequence with the detail's
// chain. The result is pass, the first failing record, or, for a run whose
// records carry no hash (schema 1.0), a neutral "no chain to check" that says
// what was and was not checked: it is never shown as a chain that checks out.
// A run that began before schema 1.1 (its first records carry no hash, the
// rest are chained) reads as checked in part, naming its unverified records;
// when the detail says so (scope partly_chained) its intro says up front that
// only the later records' hashes are recomputed.
// A chain that checks out is consistent and matches those copies; it is
// unsigned, and it says nothing about whether the content is true.
//
// The receipt compared is the one this check reads itself when it starts,
// just before the log is downloaded (a run moves on: a decision or a later
// record makes any receipt read earlier stale), and the panel says so. The
// log is append-only, so a log that grew in between holds every record that
// receipt counts: a copy whose head is the hash of the record at its own
// last position reads "earlier" (an extension, never a rewrite), not as a
// difference. The button stays focusable while the check runs (busy, not
// disabled), and one always-mounted, visually hidden status region announces
// the progress and then the short verdict once; the detailed results are
// outside it.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { useFormatter, useTranslations } from "next-intl";
import { type ReactNode, useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import { api, ApiError, isApiError } from "@/lib/api/client";
import type { ChainV1, RunReceiptV2 } from "@/lib/api/types";
import {
  type ChainProblem,
  type ChainResult,
  checkSegment,
  type SegmentOutcome,
  type SegmentResult,
  type Verdict,
  verifyLog,
} from "@/lib/chain/verify";
import { digestPrefix } from "@/lib/run/evidence";

import { ApiErrorNotice } from "../ApiErrorNotice";
import type { ChainBreak } from "./ChainBroken";

export type ChainSource = {
  url: string;
  /** The file name the log is saved under. */
  name: string;
  /** The file's pinned SHA-256 (a research file); null for this server's log. */
  sha256: string | null;
  /**
   * Where `sha256` comes from: the manifest this browser checked ("manifest");
   * the listing's copy of the pin, because the checked manifest does not name
   * the file ("unlisted") or because no manifest was checked ("listing");
   * null for this server's log.
   */
  pinSource: "manifest" | "unlisted" | "listing" | null;
};

/** The digest a finished check compared the file with, and where it came from, as of when it ran. */
type ComparedPin = Pick<ChainSource, "sha256" | "pinSource">;

type ChainCheckProps = {
  runId: string;
  source: ChainSource;
  chain: ChainV1 | null;
  /** Reads the run's receipt now (its head and record count are compared); rejects with an ApiError. */
  readReceipt: () => Promise<RunReceiptV2>;
  /**
   * Told the outcome of every check that finishes: where it failed, or null
   * when it did not fail (so the workspace's "chain broken in this browser"
   * travels to every tab until a later check does not fail). A check that
   * could not run (an error, no WebCrypto) tells nothing.
   */
  onOutcome?: (chainBreak: ChainBreak | null) => void;
};

type CheckState =
  | { status: "idle" }
  | { status: "running" }
  | {
      status: "done";
      result: ChainResult;
      segment: SegmentResult;
      /** The receipt read when this check started; null when it could not be read. */
      receipt: RunReceiptV2 | null;
      /** The detail's chain as it stood when this check started. */
      chain: ChainV1 | null;
      /** The digest the file was compared with, and where it came from, when this check started. */
      pin: ComparedPin;
    }
  | { status: "error"; error: ApiError }
  | { status: "unsupported" };

/** The verdict a finished check exposes (data-chain-verdict): "pass" only for a chain verified link by link. */
const VERDICT_ATTRIBUTE: Record<SegmentOutcome, string> = {
  verified: "pass",
  failed: "fail",
  unchained: "unchained",
  partial: "partial",
};

const VERDICT_BADGES: Record<Verdict, "success" | "error" | "neutral"> = {
  pass: "success",
  fail: "error",
  not_checked: "neutral",
  not_compared: "neutral",
  earlier: "neutral",
};

/** A copy compared (the receipt, the detail's chain) was read before the run's newest records. */
const isEarlier = (segment: SegmentResult) =>
  [segment.head_matches, segment.count, segment.range].includes("earlier");

const short = (value: string) => `${digestPrefix(value) ?? value}…`;

/** The values a problem's sentence names, hashes cut to their prefix. */
export function problemValues(problem: ChainProblem): Record<string, string | number> {
  switch (problem.kind) {
    case "schema_version":
    case "legacy_not_1_0":
      return { found: problem.found };
    case "sequence_gap":
      return { expected: problem.expected, found: problem.found };
    case "duplicate_id":
      return { eventId: problem.eventId };
    case "prev_mismatch":
      return { stored: short(problem.stored), expected: short(problem.expected) };
    case "hash_mismatch":
      return { stored: short(problem.stored), computed: short(problem.computed) };
    case "schema_downgrade":
      return { found: problem.found, earlier: problem.earlier, runId: problem.runId };
    default:
      return {};
  }
}

export function ChainCheck({ runId, source, chain, readReceipt, onOutcome }: ChainCheckProps) {
  const t = useTranslations("Run.chain");
  const tProblem = dynamicTranslator(useTranslations("Run.chain.problems"));
  const format = useFormatter();
  const [state, setState] = useState<CheckState>({ status: "idle" });
  // The detail says the run's records carry no hash (schema 1.0): the check
  // recomputes nothing for them, so the method is described without hashing.
  const unchainedScope = chain?.scope === "unchained";
  // A run that began before schema 1.1 (its first records carry no hash):
  // the method is described as recomputing only the hashes the later
  // records carry, as the "checked in part" outcome says.
  const intro = unchainedScope ? "unchained" : chain?.scope === "partly_chained" ? "partly_chained" : "chained";
  const running = state.status === "running";

  const run = async () => {
    if (running) {
      return;
    }
    if (globalThis.crypto?.subtle === undefined) {
      setState({ status: "unsupported" });
      return;
    }
    setState({ status: "running" });
    // The verdict and the sentence that says what it was compared with come
    // from the same digest: the one in hand when the check ran, even if the
    // manifest is read (or read again) afterwards.
    const pin: ComparedPin = { sha256: source.sha256, pinSource: source.pinSource };
    // The detail's chain as it stands now, and the receipt read now, before
    // the log: the log is append-only, so it holds every record they count.
    const expectedChain = chain;
    let receipt: RunReceiptV2 | null = null;
    try {
      receipt = await readReceipt();
    } catch {
      // Shown in the receipt's own section with a retry; here the head and
      // the count are not compared, and the panel says why.
      receipt = null;
    }
    try {
      const bytes = await api.bytes(source.url);
      const result = await verifyLog(bytes);
      const segment = checkSegment(result, {
        runId,
        sha256: pin.sha256,
        receiptHead: receipt === null ? undefined : (receipt.chain.head_hash ?? null),
        receiptEventCount: receipt?.event_count ?? null,
        firstSequence: expectedChain?.first_sequence ?? null,
        lastSequence: expectedChain?.last_sequence ?? null,
        chainHead: expectedChain?.head_hash ?? null,
      });
      setState({ status: "done", result, segment, receipt, chain: expectedChain, pin });
      onOutcome?.(
        segment.outcome === "failed"
          ? {
              sequence: result.failure?.sequence ?? null,
              line: result.failure?.line ?? null,
              problem: result.failure?.problem ?? null,
            }
          : null,
      );
    } catch (error) {
      setState({
        status: "error",
        error: isApiError(error)
          ? error
          : new ApiError({
              code: "unreachable",
              message: error instanceof Error ? error.message : "download failed",
              status: 0,
              path: source.url,
            }),
      });
    }
  };

  const verdictBadge = (verdict: Verdict) => (
    <Badge variant={VERDICT_BADGES[verdict]} label={t(`verdicts.${verdict}`)} />
  );

  /** Where the check stopped, in words. */
  const failureText = (failure: NonNullable<ChainResult["failure"]>) =>
    failure.sequence === null
      ? t("failureAtLine", {
          line: failure.line,
          problem: tProblem(failure.problem.kind, problemValues(failure.problem)),
        })
      : t("failureAt", {
          sequence: failure.sequence,
          line: failure.line,
          problem: tProblem(failure.problem.kind, problemValues(failure.problem)),
        });

  /** The short sentence the status region announces: the progress, then the verdict (never the detailed results). */
  const announcement = (): string => {
    switch (state.status) {
      case "running":
        return unchainedScope ? t("runningUnchained", { name: source.name }) : t("running", { name: source.name });
      case "unsupported":
        return t("unsupported");
      case "done":
        switch (state.segment.outcome) {
          case "verified":
            return t("pass");
          case "partial":
            return t("partialVerdict");
          case "unchained":
            return t("unchainedVerdict");
          case "failed":
            return t("announcement", {
              verdict: t("fail"),
              detail: state.result.failure === null ? t("failSentence") : failureText(state.result.failure),
            });
        }
        return "";
      default:
        // An error is announced by its own alert.
        return "";
    }
  };

  /** The headline of a finished check: green only for a chain verified link by link. */
  const headline = (segment: SegmentResult) => {
    switch (segment.outcome) {
      case "verified":
        // A copy read before the newest records: the segment matches or extends it, never "matches" alone.
        return isEarlier(segment) ? (
          <>
            <Badge variant="success" label={t("pass")} />{" "}
            {segment.legacyRecords === 0
              ? t("passSentenceEarlier", { count: segment.chainedRecords })
              : t("passSentenceLegacyEarlier", { chained: segment.chainedRecords, legacy: segment.legacyRecords })}
          </>
        ) : (
          <>
            <Badge variant="success" label={t("pass")} />{" "}
            {segment.legacyRecords === 0
              ? t("passSentence", { count: segment.chainedRecords })
              : t("passSentenceLegacy", { chained: segment.chainedRecords, legacy: segment.legacyRecords })}
          </>
        );
      case "partial":
        return (
          <>
            <Badge variant="neutral" label={t("partialVerdict")} />{" "}
            {t("partialSentence", { chained: segment.runChainedRecords, legacy: segment.runLegacyRecords })}
          </>
        );
      case "unchained":
        return (
          <>
            <Badge variant="neutral" label={t("unchainedVerdict")} />{" "}
            {t("unchainedSentence", { count: segment.records.length })}
          </>
        );
      case "failed":
        return (
          <>
            <Badge variant="error" label={t("fail")} /> {t("failSentence")}
          </>
        );
    }
  };

  return (
    <section className="hc-section" aria-labelledby="chain-check" data-chain-check={state.status}>
      <h4 id="chain-check">{t("heading")}</h4>
      <p data-chain-intro={intro}>
        {intro === "unchained"
          ? t("introUnchained", { name: source.name })
          : intro === "partly_chained"
            ? t("introPartlyChained", { name: source.name })
            : t("intro", { name: source.name })}
      </p>
      <div className="hc-actions">
        {/* Busy, never disabled: a disabled button would drop the keyboard focus; Astryx ignores clicks while busy. */}
        <Button
          variant="secondary"
          label={state.status === "done" || state.status === "error" ? t("again") : t("run")}
          isLoading={running}
          onClick={() => void run()}
        />
      </div>
      {/* Mounted empty before anything is written into it: the progress, then the short verdict, once. */}
      <p className="hc-visually-hidden" role="status" data-chain-announcement>
        {announcement()}
      </p>
      {running ? (
        <p className="hc-supporting" data-chain-running>
          {unchainedScope ? t("runningUnchained", { name: source.name }) : t("running", { name: source.name })}
        </p>
      ) : null}
      {state.status === "unsupported" ? <p data-chain-unsupported>{t("unsupported")}</p> : null}
      {state.status === "error" ? <ApiErrorNotice error={state.error} /> : null}
      {state.status === "done" ? (
        <ChainResultPanel
          state={state}
          headline={headline(state.segment)}
          verdictBadge={verdictBadge}
          failureText={failureText}
          receiptDate={(iso) => format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "medium" })}
        />
      ) : null}
    </section>
  );
}

type DoneState = Extract<CheckState, { status: "done" }>;

/** The detailed results of a finished check (outside the status region). */
function ChainResultPanel({
  state,
  headline,
  verdictBadge,
  failureText,
  receiptDate,
}: {
  state: DoneState;
  headline: ReactNode;
  verdictBadge: (verdict: Verdict) => ReactNode;
  failureText: (failure: NonNullable<ChainResult["failure"]>) => string;
  receiptDate: (iso: string) => string;
}) {
  const t = useTranslations("Run.chain");
  // The receipt, the detail's chain and the pin as they stood when this check started.
  const { result, segment, receipt, pin } = state;
  const lastSequence = state.chain?.last_sequence;
  const earlier = isEarlier(segment);
  // The record at the receipt's own last position (the receipt's head), when the log grew past it.
  const receiptLast =
    receipt === null || segment.head_matches !== "earlier" ? null : segment.records[receipt.event_count - 1];
  return (
    <div className="hc-panel" data-chain-verdict={VERDICT_ATTRIBUTE[segment.outcome]}>
      <p className="hc-chain-verdict">{headline}</p>
      {earlier ? <p data-chain-earlier>{t("earlierSentence")}</p> : null}
      {result.failure === null ? null : (
        <p data-chain-failure={result.failure.problem.kind} data-chain-failure-sequence={result.failure.sequence ?? ""}>
          {failureText(result.failure)}
        </p>
      )}
      <p className="hc-supporting" data-chain-receipt={receipt === null ? "unread" : "read"}>
        {receipt === null ? t("receiptUnread") : t("receiptRead", { date: receiptDate(receipt.generated_at) })}
      </p>
      <ul className="hc-list" data-chain-results>
        <li data-chain-result="sha256">
          {verdictBadge(segment.sha256)}{" "}
          {pin.sha256 === null
            ? t("sha256Own", { sha256: result.sha256, bytes: result.bytes })
            : pin.pinSource === "manifest"
              ? t("sha256", { sha256: result.sha256, bytes: result.bytes })
              : pin.pinSource === "unlisted"
                ? t("sha256Unlisted", { sha256: result.sha256, bytes: result.bytes })
                : t("sha256Listing", { sha256: result.sha256, bytes: result.bytes })}
        </li>
        <li data-chain-result="chain">
          {verdictBadge(segment.chain)}{" "}
          {segment.unchained
            ? t("unchained", { count: segment.records.length })
            : segment.legacyRecords === 0
              ? t("records", { count: segment.chainedRecords })
              : t("recordsLegacy", { chained: segment.chainedRecords, legacy: segment.legacyRecords })}
        </li>
        <li data-chain-result="range">
          {verdictBadge(segment.range)}{" "}
          {segment.first === null || segment.last === null
            ? t("noSegment")
            : segment.range === "earlier" && lastSequence !== undefined && lastSequence !== null
              ? t("segmentEarlier", {
                  first: segment.first,
                  last: segment.last,
                  count: segment.records.length,
                  detailLast: lastSequence,
                })
              : t("segment", {
                  first: segment.first,
                  last: segment.last,
                  count: segment.records.length,
                })}
        </li>
        <li data-chain-result="head">
          {verdictBadge(segment.head_matches)}{" "}
          {segment.head === null
            ? t("noHead")
            : receipt === null
              ? t("headNoReceipt", { head: segment.head })
              : receiptLast !== null && receiptLast !== undefined
                ? t("headEarlier", {
                    head: segment.head,
                    sequence: receiptLast.sequence,
                    appended: segment.records.length - receipt.event_count,
                  })
                : t("head", { head: segment.head })}
        </li>
        <li data-chain-result="count">
          {verdictBadge(segment.count)}{" "}
          {receipt === null
            ? t("countNoReceipt")
            : segment.count === "earlier"
              ? t("countEarlier", { count: receipt.event_count, total: segment.records.length })
              : t("count", { count: receipt.event_count })}
        </li>
      </ul>
      <p className="hc-supporting">
        {!segment.unchained ? t("limits") : pin.sha256 === null ? t("limitsUnchainedOwn") : t("limitsUnchained")}
      </p>
    </div>
  );
}
