"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The revision view, next to the scope line of a recommendation for a
// revised proposal (DecisionTab): the original and the revised proposal
// claim by claim (src/lib/run/revision.ts: unchanged, changed, added or
// dropped, by claim id as the records carry them), and each revision with
// the items it says it addresses (its own claim, not a verdict). The
// statements are model output, shown as recorded, in English.

import { Badge } from "@astryxdesign/core/Badge";
import { useLocale, useTranslations } from "next-intl";

import type { Proposal, RunDetailV1 } from "@/lib/api/types";
import { compareClaims, revisionSteps } from "@/lib/run/revision";

type RevisionComparisonProps = {
  original: Proposal;
  revised: Proposal;
  detail: RunDetailV1;
};

export function RevisionComparison({ original, revised, detail }: RevisionComparisonProps) {
  const t = useTranslations("Run.revision");
  const locale = useLocale();
  const changes = compareClaims(original, revised);
  const steps = revisionSteps(detail);
  const statement = (text: string | null) =>
    text === null ? <span className="hc-supporting">{t("none")}</span> : <span lang="en">{text}</span>;
  return (
    <div className="hc-revision" data-revision-compare>
      <h4 id="revision-heading">{t("heading", { original: original.id, revised: revised.id })}</h4>
      <p className="hc-supporting">
        {t("intro")}
        {locale === "en" ? null : ` ${t("english")}`}
      </p>
      <div className="hc-table-scroll" role="region" aria-labelledby="revision-heading" tabIndex={0}>
        <table className="hc-timeline">
          <caption className="hc-visually-hidden">{t("caption")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("claim")}</th>
              <th scope="col">{t("originalColumn", { id: original.id })}</th>
              <th scope="col">{t("revisedColumn", { id: revised.id })}</th>
              <th scope="col">{t("change")}</th>
            </tr>
          </thead>
          <tbody>
            {changes.map((change) => {
              const before = change.kind === "changed" ? change.before : change.kind === "added" ? null : change.claim;
              const after = change.kind === "changed" ? change.after : change.kind === "removed" ? null : change.claim;
              return (
                <tr key={`${change.kind}-${change.id}`} data-claim-change={change.kind} data-claim-id={change.id}>
                  <th scope="row">
                    <code className="hc-code">{change.id}</code>
                  </th>
                  <td>{statement(before?.statement ?? null)}</td>
                  <td>{statement(after?.statement ?? null)}</td>
                  <td>
                    <Badge
                      variant={change.kind === "unchanged" ? "neutral" : "warning"}
                      label={t(`kinds.${change.kind}`)}
                    />
                    {change.kind === "changed" &&
                    change.before.evidence_refs.join(",") !== change.after.evidence_refs.join(",") ? (
                      <span className="hc-supporting">
                        {` ${t("refs", {
                          before: change.before.evidence_refs.join(", ") || t("none"),
                          after: change.after.evidence_refs.join(", ") || t("none"),
                        })}`}
                      </span>
                    ) : null}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {steps.length === 0 ? null : (
        <ul className="hc-list" data-revision-steps>
          {steps.map((step) => (
            <li key={step.sequence}>
              {step.addressedItems.length === 0
                ? t("stepNoItems", { id: step.proposalId, supersedes: step.supersedes })
                : t("step", {
                    id: step.proposalId,
                    supersedes: step.supersedes,
                    items: step.addressedItems.join(", "),
                  })}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
