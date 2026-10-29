"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The chain checked in this browser (src/lib/chain/verify.ts): the verbatim
// log is downloaded as bytes (a recorded run's research file, or this
// server's events.ndjson of a run whose log the public surface publishes),
// every record's hash is recomputed with WebCrypto from the raw line with
// its hash blanked textually (crates/consensus-runtime/src/event_log.rs
// hashes its re-serialization instead: the two agree on the canonical lines
// the runtime writes, and this check is stricter on any other line; it reads
// only the chain fields and replays nothing, see the header of verify.ts and
// the Run.chain.limits text), every link is followed, and the
// run's own segment is compared with what the views hold: the file's SHA-256
// with the digest the archive's manifest pins (ShowcaseEntryV1.sha256), the
// head (the hash of the run's last record) with the receipt's, the record
// count with the receipt's, the first and last sequence with the detail's
// chain. The result is pass, the first failing record, or, for a run whose
// records carry no hash (schema 1.0), a neutral "no chain to check" that says
// what was and was not checked: it is never shown as a chain that checks out.
// A run that began before schema 1.1 (its first records carry no hash, the
// rest are chained) reads as checked in part, naming its unverified records.
// A chain that checks out is consistent and matches those copies; it is
// unsigned, and it says nothing about whether the content is true.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";
import { useState } from "react";

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

export type ChainSource = {
  url: string;
  /** The file name the log is saved under. */
  name: string;
  /** The file's pinned SHA-256 (a research file); null for this server's log. */
  sha256: string | null;
};

type ChainCheckProps = {
  runId: string;
  source: ChainSource;
  chain: ChainV1 | null;
  /** The receipt, once read: its head and record count are compared. */
  receipt: RunReceiptV2 | null;
};

type CheckState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "done"; result: ChainResult; segment: SegmentResult }
  | { status: "error"; error: ApiError }
  | { status: "unsupported" };

/** The verdict a finished check exposes (data-chain-verdict): "pass" only for a chain verified link by link. */
const VERDICT_ATTRIBUTE: Record<SegmentOutcome, string> = {
  verified: "pass",
  failed: "fail",
  unchained: "unchained",
  partial: "partial",
};

const short = (value: string) => `${digestPrefix(value) ?? value}…`;

/** The values a problem's sentence names, hashes cut to their prefix. */
function problemValues(problem: ChainProblem): Record<string, string | number> {
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

export function ChainCheck({ runId, source, chain, receipt }: ChainCheckProps) {
  const t = useTranslations("Run.chain");
  const tProblem = dynamicTranslator(useTranslations("Run.chain.problems"));
  const [state, setState] = useState<CheckState>({ status: "idle" });
  // The detail says the run's records carry no hash (schema 1.0): the check
  // recomputes nothing for them, so the method is described without hashing.
  const unchainedScope = chain?.scope === "unchained";

  const run = async () => {
    if (globalThis.crypto?.subtle === undefined) {
      setState({ status: "unsupported" });
      return;
    }
    setState({ status: "running" });
    try {
      const bytes = await api.bytes(source.url);
      const result = await verifyLog(bytes);
      const segment = checkSegment(result, {
        runId,
        sha256: source.sha256,
        receiptHead: receipt === null ? undefined : (receipt.chain.head_hash ?? null),
        receiptEventCount: receipt?.event_count ?? null,
        firstSequence: chain?.first_sequence ?? null,
        lastSequence: chain?.last_sequence ?? null,
      });
      setState({ status: "done", result, segment });
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

  const verdictBadge = (verdict: Verdict) =>
    verdict === "pass" ? (
      <Badge variant="success" label={t("verdicts.pass")} />
    ) : verdict === "fail" ? (
      <Badge variant="error" label={t("verdicts.fail")} />
    ) : verdict === "not_checked" ? (
      <Badge variant="neutral" label={t("verdicts.not_checked")} />
    ) : (
      <Badge variant="neutral" label={t("verdicts.not_compared")} />
    );

  /** The headline of a finished check: green only for a chain verified link by link. */
  const headline = (segment: SegmentResult) => {
    switch (segment.outcome) {
      case "verified":
        return (
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
      <p data-chain-intro={unchainedScope ? "unchained" : "chained"}>
        {unchainedScope ? t("introUnchained", { name: source.name }) : t("intro", { name: source.name })}
      </p>
      <div className="hc-actions">
        <Button
          variant="secondary"
          label={state.status === "done" || state.status === "error" ? t("again") : t("run")}
          isDisabled={state.status === "running"}
          isLoading={state.status === "running"}
          onClick={() => void run()}
        />
      </div>
      {state.status === "running" ? (
        <p role="status" className="hc-supporting">
          {unchainedScope ? t("runningUnchained", { name: source.name }) : t("running", { name: source.name })}
        </p>
      ) : null}
      {state.status === "unsupported" ? <p role="status">{t("unsupported")}</p> : null}
      {state.status === "error" ? <ApiErrorNotice error={state.error} /> : null}
      {state.status === "done" ? (
        <div className="hc-panel" role="status" data-chain-verdict={VERDICT_ATTRIBUTE[state.segment.outcome]}>
          <p className="hc-chain-verdict">{headline(state.segment)}</p>
          {state.result.failure === null ? null : (
            <p data-chain-failure={state.result.failure.problem.kind} data-chain-failure-sequence={state.result.failure.sequence ?? ""}>
              {state.result.failure.sequence === null
                ? t("failureAtLine", {
                    line: state.result.failure.line,
                    problem: tProblem(state.result.failure.problem.kind, problemValues(state.result.failure.problem)),
                  })
                : t("failureAt", {
                    sequence: state.result.failure.sequence,
                    line: state.result.failure.line,
                    problem: tProblem(state.result.failure.problem.kind, problemValues(state.result.failure.problem)),
                  })}
            </p>
          )}
          <ul className="hc-list" data-chain-results>
            <li data-chain-result="sha256">
              {verdictBadge(state.segment.sha256)}{" "}
              {source.sha256 === null
                ? t("sha256Own", { sha256: state.result.sha256, bytes: state.result.bytes })
                : t("sha256", { sha256: state.result.sha256, bytes: state.result.bytes })}
            </li>
            <li data-chain-result="chain">
              {verdictBadge(state.segment.chain)}{" "}
              {state.segment.unchained
                ? t("unchained", { count: state.segment.records.length })
                : state.segment.legacyRecords === 0
                  ? t("records", { count: state.segment.chainedRecords })
                  : t("recordsLegacy", { chained: state.segment.chainedRecords, legacy: state.segment.legacyRecords })}
            </li>
            <li data-chain-result="range">
              {verdictBadge(state.segment.range)}{" "}
              {state.segment.first === null || state.segment.last === null
                ? t("noSegment")
                : t("segment", {
                    first: state.segment.first,
                    last: state.segment.last,
                    count: state.segment.records.length,
                  })}
            </li>
            <li data-chain-result="head">
              {verdictBadge(state.segment.head_matches)}{" "}
              {state.segment.head === null
                ? t("noHead")
                : receipt === null
                  ? t("headNoReceipt", { head: state.segment.head })
                  : t("head", { head: state.segment.head })}
            </li>
            <li data-chain-result="count">
              {verdictBadge(state.segment.count)}{" "}
              {receipt === null ? t("countNoReceipt") : t("count", { count: receipt.event_count })}
            </li>
          </ul>
          <p className="hc-supporting">
            {!state.segment.unchained
              ? t("limits")
              : source.sha256 === null
                ? t("limitsUnchainedOwn")
                : t("limitsUnchained")}
          </p>
        </div>
      ) : null}
    </section>
  );
}
