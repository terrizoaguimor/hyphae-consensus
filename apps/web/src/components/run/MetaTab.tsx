"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Meta API panel: one row per model invocation the run recorded
// (src/lib/run/invocations.ts): role, provider and model, the recorded
// request id, latency, input, output and reasoning tokens, the output bound,
// the finish reason, the attempts, the structured-output schema (name and
// SHA-256 prefix) and the prompt template's SHA-256 prefix. A scripted
// double's row reads "scripted double, no model call" and shows nothing that
// would pass for evidence of a call (no request id, no latency, no tokens).
// A request id is evidence that a Meta call was recorded, never that anything
// is live. Totals are computed from the rows, over real calls only. The
// contract publishes no prices, so no cost is shown.

import { useFormatter, useTranslations } from "next-intl";
import { useMemo } from "react";

import type { ApiError } from "@/lib/api/client";
import type { ConsensusFrameV1, RunDetailV1 } from "@/lib/api/types";
import { digestPrefix } from "@/lib/run/evidence";
import {
  invocationRows,
  invocationTotals,
  operatorFieldsAbsent,
  type Total,
} from "@/lib/run/invocations";
import { mergeEvents } from "@/lib/run/view";

type MetaTabProps = {
  detail: RunDetailV1 | null;
  frames: ConsensusFrameV1[];
  detailError: ApiError | null;
};

export function MetaTab({ detail, frames, detailError }: MetaTabProps) {
  const t = useTranslations("Run.meta");
  const tRoles = useTranslations("Run.explorer.roles");
  const tProvenance = useTranslations("Provenance");
  const format = useFormatter();
  const rows = useMemo(() => invocationRows(mergeEvents(detail, frames)), [detail, frames]);
  const totals = useMemo(() => invocationTotals(rows), [rows]);
  const absent = operatorFieldsAbsent(rows);
  const recordedCount = detail?.model_invocations.length ?? null;
  const number = (value: number | null | undefined) =>
    typeof value === "number" ? format.number(value) : null;
  const cell = (value: string | null, code = false) =>
    value === null ? (
      <span className="hc-supporting">{t("notRecorded")}</span>
    ) : code ? (
      <code className="hc-code">{value}</code>
    ) : (
      <span className="hc-tabular-nums">{value}</span>
    );
  const totalText = (field: Total, unit?: "ms") =>
    t("total", {
      sum: unit === "ms" ? t("ms", { value: format.number(field.sum) }) : format.number(field.sum),
      reported: field.reported,
      calls: totals.calls,
    });

  return (
    <div className="hc-meta-tab" data-meta-tab>
      <div className="hc-list-head">
        <h3 id="meta-heading">{t("heading")}</h3>
        <p className="hc-supporting hc-tabular-nums" data-invocation-count={rows.length}>
          {t("count", { count: rows.length })}
        </p>
      </div>
      <p className="hc-supporting">{t("intro")}</p>
      {detailError === null ? null : (
        <p className="hc-supporting" data-meta-partial>
          {t("partial")}
        </p>
      )}
      {recordedCount !== null && recordedCount !== rows.length ? (
        <p className="hc-supporting" data-meta-mismatch>
          {t("mismatch", { rows: rows.length, recorded: recordedCount })}
        </p>
      ) : null}
      {rows.length === 0 ? (
        <p className="hc-supporting" data-meta-none>
          {t("none")}
        </p>
      ) : (
        <>
          <div className="hc-table-scroll" role="region" aria-labelledby="meta-heading" tabIndex={0}>
            <table className="hc-timeline hc-invocations">
              <caption className="hc-visually-hidden">{t("caption")}</caption>
              <thead>
                <tr>
                  <th scope="col">{t("columns.record")}</th>
                  <th scope="col">{t("columns.role")}</th>
                  <th scope="col">{t("columns.model")}</th>
                  <th scope="col">{t("columns.requestId")}</th>
                  <th scope="col">{t("columns.latency")}</th>
                  <th scope="col">{t("columns.input")}</th>
                  <th scope="col">{t("columns.output")}</th>
                  <th scope="col">{t("columns.reasoning")}</th>
                  <th scope="col">{t("columns.maxCompletion")}</th>
                  <th scope="col">{t("columns.finish")}</th>
                  <th scope="col">{t("columns.attempts")}</th>
                  <th scope="col">{t("columns.schema")}</th>
                  <th scope="col">{t("columns.prompt")}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map(({ sequence, role, failedStage, invocation }) => (
                  <tr
                    key={sequence}
                    data-invocation-sequence={sequence}
                    data-invocation-scripted={invocation.scripted}
                  >
                    <td className="hc-tabular-nums">{sequence}</td>
                    <td>
                      {role === null ? t("unknownRole") : tRoles(role)}
                      {failedStage === null ? null : (
                        <span className="hc-supporting">{` ${t("failedAt", { stage: failedStage })}`}</span>
                      )}
                    </td>
                    {invocation.scripted ? (
                      <td colSpan={11} data-model-note="scripted">
                        {tProvenance("scripted.note")}
                      </td>
                    ) : (
                      <>
                        <td>
                          {t("model", { model: invocation.model, provider: invocation.provider })}
                        </td>
                        <td>{cell(invocation.request_id ?? null, true)}</td>
                        <td>
                          {cell(
                            typeof invocation.latency_ms === "number"
                              ? t("ms", { value: format.number(invocation.latency_ms) })
                              : null,
                          )}
                        </td>
                        <td>{cell(number(invocation.input_tokens))}</td>
                        <td>{cell(number(invocation.output_tokens))}</td>
                        <td>{cell(number(invocation.reasoning_tokens))}</td>
                        <td>{cell(number(invocation.max_completion_tokens))}</td>
                        <td>{cell(invocation.finish_reason ?? null, true)}</td>
                        <td>{cell(number(invocation.attempts))}</td>
                        <td>
                          {invocation.schema_name === undefined || invocation.schema_name === null ? (
                            cell(null)
                          ) : (
                            <>
                              <code className="hc-code">{invocation.schema_name}</code>
                              {invocation.schema_sha256 === undefined || invocation.schema_sha256 === null ? null : (
                                <code className="hc-code">{` ${digestPrefix(invocation.schema_sha256) ?? ""}`}</code>
                              )}
                            </>
                          )}
                        </td>
                        <td>{cell(digestPrefix(invocation.prompt_sha256), true)}</td>
                      </>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <section className="hc-panel" aria-labelledby="meta-totals" data-meta-totals>
            <h4 id="meta-totals">{t("totalsHeading")}</h4>
            {totals.calls === 0 ? (
              <p data-meta-no-call>{t("noCall", { scripted: totals.scripted })}</p>
            ) : (
              <>
                <p data-meta-calls={totals.calls}>
                  {t("calls", { count: totals.calls, scripted: totals.scripted })}
                </p>
                <dl className="hc-kv">
                  <div>
                    <dt>{t("columns.latency")}</dt>
                    <dd className="hc-tabular-nums" data-total="latency">
                      {totalText(totals.latencyMs, "ms")}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("columns.input")}</dt>
                    <dd className="hc-tabular-nums" data-total="input">
                      {totalText(totals.inputTokens)}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("columns.output")}</dt>
                    <dd className="hc-tabular-nums" data-total="output">
                      {totalText(totals.outputTokens)}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("columns.reasoning")}</dt>
                    <dd className="hc-tabular-nums" data-total="reasoning">
                      {totalText(totals.reasoningTokens)}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("columns.attempts")}</dt>
                    <dd className="hc-tabular-nums" data-total="attempts">
                      {totalText(totals.attempts)}
                    </dd>
                  </div>
                </dl>
              </>
            )}
            <p className="hc-supporting">{t("requestIds")}</p>
            {absent ? <p className="hc-supporting" data-meta-operator-only>{t("operatorOnly")}</p> : null}
            <p className="hc-supporting" data-meta-no-cost>
              {t("noCost")}
            </p>
          </section>
        </>
      )}
    </div>
  );
}
