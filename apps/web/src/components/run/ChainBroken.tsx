"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A chain check run in this browser that failed travels (heuristic
// evaluation, problem 5): the workspace keeps where it failed, and this
// callout says so under the provenance labels on every tab and next to the
// decision, until the check is run again and does not fail. It is a state of
// this page, not a label of the run: nothing is stored on the server.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";

import { Callout } from "../Callout";

/** Where a failed chain check stopped: a record (its sequence and line), a line, or no line (a copy compared differs). */
export type ChainBreak = { sequence: number | null; line: number | null };

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
  const { sequence, line } = chainBreak;
  return (
    <Callout
      tone="failed"
      title={t("title")}
      data-chain-broken={place}
      data-chain-broken-sequence={sequence ?? ""}
    >
      <p>
        {sequence !== null && line !== null
          ? t("atRecord", { sequence, line })
          : line !== null
            ? t("atLine", { line })
            : t("copies")}{" "}
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
