"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A chain check run in this browser that failed travels (heuristic
// evaluation, problem 5): the workspace keeps where it failed, and this
// callout says so under the provenance labels on every tab and next to the
// decision, until the check is run again and does not fail. It is a state of
// this page, not a label of the run: nothing is stored on the server. The
// reason it gives is the verifier's own (Run.chain.problems), the same
// sentence the Audit trail's panel uses: a torn line or a sequence gap is
// not called a hash-chain mismatch.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { ChainProblem } from "@/lib/chain/verify";

import { Callout } from "../Callout";
import { problemValues } from "./ChainCheck";

/**
 * Where a failed chain check stopped: a record (its sequence and line), a
 * line, or no line (a copy compared differs), and why the line fails (null
 * when no line does).
 */
export type ChainBreak = { sequence: number | null; line: number | null; problem: ChainProblem | null };

type ChainBrokenProps = {
  chainBreak: ChainBreak;
  /** Where the callout is: under the header's labels, or next to the decision. */
  place: "header" | "decision";
  /** A decision can still be recorded on this run: the callout asks to look before deciding. */
  deciding: boolean;
  /** Opens the Audit trail; absent where the reader already is in it. */
  onOpenAudit?: () => void;
};

export function ChainBroken({ chainBreak, place, deciding, onOpenAudit }: ChainBrokenProps) {
  const t = useTranslations("Run.chainBroken");
  const tProblem = dynamicTranslator(useTranslations("Run.chain.problems"));
  const { sequence, line, problem } = chainBreak;
  return (
    <Callout
      tone="failed"
      icon="unlink"
      title={t("title")}
      data-chain-broken={place}
      data-chain-broken-sequence={sequence ?? ""}
      data-chain-broken-problem={problem?.kind ?? ""}
    >
      <p>
        {line === null || problem === null
          ? t("copies")
          : sequence !== null
            ? t("atRecord", { sequence, line, problem: tProblem(problem.kind, problemValues(problem)) })
            : t("atLine", { line, problem: tProblem(problem.kind, problemValues(problem)) })}{" "}
        {deciding ? t("beforeDeciding") : t("details")}
      </p>
      <p className="hc-supporting">{t("pageState")}</p>
      {onOpenAudit === undefined ? null : (
        <div className="hc-actions">
          <Button variant="secondary" size="sm" label={t("openAudit")} onClick={onOpenAudit} />
        </div>
      )}
    </Callout>
  );
}
