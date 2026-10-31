"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The revision view, next to the scope line of a recommendation for a
// revised proposal (DecisionTab): the claims of the original and the revised
// proposal (src/lib/run/revision.ts claimComparison). When the two share
// claim ids (scripted runs), claim by claim by id: unchanged, changed, added
// or dropped, with a sentence when only some ids are shared (ids alone
// cannot tell an added or dropped claim from a restated one). When they
// share none (every live-model revision: the runtime numbers each proposal's
// claims anew, and nothing else in the records links a revised claim to an
// original one), the two lists side by side with a sentence saying so, the
// Meta model adapter named as the cause only when every revision of the run
// went through it, and no claim marked added, changed or dropped. Then
// each revision with the items it says it addresses (its own claim, not a
// verdict). The statements are model output, shown as recorded, in English.
// Below 42rem (672 px) the claim table is drawn as one block per claim, each
// cell under its column name (data-label, globals.css), so it never makes the
// page wider than a phone; claim ids never wrap (hc-id).

import { Badge } from "@astryxdesign/core/Badge";
import { useLocale, useTranslations } from "next-intl";

import type { Claim, Proposal, RunDetailV1 } from "@/lib/api/types";
import { claimComparison, revisedByMetaModel, revisionSteps } from "@/lib/run/revision";

type RevisionComparisonProps = {
  original: Proposal;
  revised: Proposal;
  detail: RunDetailV1;
};

export function RevisionComparison({ original, revised, detail }: RevisionComparisonProps) {
  const t = useTranslations("Run.revision");
  const locale = useLocale();
  const comparison = claimComparison(original, revised);
  const steps = revisionSteps(detail);
  const statement = (text: string | null) =>
    text === null ? <span className="hc-supporting">{t("none")}</span> : <span lang="en">{text}</span>;
  const english = locale === "en" ? null : ` ${t("english")}`;
  const claimList = (side: "original" | "revised", proposal: Proposal, claims: Claim[]) => (
    <section className="hc-panel" aria-labelledby={`revision-${side}`} data-claim-list={side}>
      <h5 id={`revision-${side}`}>
        {t(side === "original" ? "originalColumn" : "revisedColumn", { id: proposal.id })}
      </h5>
      {claims.length === 0 ? (
        <p className="hc-supporting">{t("noClaims")}</p>
      ) : (
        <ul className="hc-list">
          {claims.map((claim) => (
            <li key={claim.id} data-claim-id={claim.id}>
              <code className="hc-code hc-id">{claim.id}</code> {statement(claim.statement)}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
  return (
    <div className="hc-revision" data-revision-compare data-claim-pairing={comparison.pairing}>
      <h4 id="revision-heading">{t("heading", { original: original.id, revised: revised.id })}</h4>
      {comparison.pairing === "none" ? (
        <>
          <p className="hc-supporting" data-claim-pairing-note>
            {t("unpaired", { original: original.id, revised: revised.id })}
            {revisedByMetaModel(detail) ? <span data-claim-pairing-cause>{` ${t("unpairedMeta")}`}</span> : null}
            {english}
          </p>
          <div className="hc-side-by-side">
            {claimList("original", original, comparison.original)}
            {claimList("revised", revised, comparison.revised)}
          </div>
        </>
      ) : (
        <>
          <p className="hc-supporting">
            {t("intro")}
            {comparison.partial ? <span data-claim-pairing-partial>{` ${t("partialIds")}`}</span> : null}
            {english}
          </p>
          <div className="hc-table-scroll" role="region" aria-labelledby="revision-heading" tabIndex={0}>
            <table className="hc-timeline hc-revision-table">
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
                {comparison.changes.map((change) => {
                  const before =
                    change.kind === "changed" ? change.before : change.kind === "added" ? null : change.claim;
                  const after =
                    change.kind === "changed" ? change.after : change.kind === "removed" ? null : change.claim;
                  return (
                    <tr key={`${change.kind}-${change.id}`} data-claim-change={change.kind} data-claim-id={change.id}>
                      <th scope="row">
                        <code className="hc-code hc-id">{change.id}</code>
                      </th>
                      <td data-label={t("originalColumn", { id: original.id })}>
                        {statement(before?.statement ?? null)}
                      </td>
                      <td data-label={t("revisedColumn", { id: revised.id })}>
                        {statement(after?.statement ?? null)}
                      </td>
                      <td data-label={t("change")}>
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
        </>
      )}
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
