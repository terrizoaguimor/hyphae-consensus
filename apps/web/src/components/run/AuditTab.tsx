"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Audit trail: the chain behind the run (ChainV1: scope, first and last
// sequence, head hash; each record's prev_hash -> hash link as the views
// carry it), the receipt (RunReceiptV2: authorization, deliberation summary,
// model calls, verifications, protocol, proof limitations), the verbatim log
// where the public surface publishes it (a guided run's events.ndjson; for a
// recorded run, the research file its showcase entry names, with the SHA-256
// the archive's manifest pins, cross-checked against the manifest this
// browser checked), and the exact command a third party runs to
// check a downloaded log against the receipt, and the same check run in this
// browser over the verbatim log (ChainCheck). The records and the receipt
// shown here are display copies (the public view drops operator-only fields
// and the browser re-serializes), so every one of them says it is not the
// hashed bytes; only the verbatim log is. A recorded run's receipt is this
// server's rebuild from the archived log (generated now, today's limitations
// text); RunTimeReceiptNote says so next to its download and links the run
// record that holds the receipt downloaded at run time.

import { Button } from "@astryxdesign/core/Button";
import { useFormatter, useTranslations } from "next-intl";
import { useMemo } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import { api, type ApiError } from "@/lib/api/client";
import type {
  ConsensusFrameV1,
  RunDetailV1,
  RunReceiptV2,
  RunSummaryV1,
  ShowcaseEntryV1,
} from "@/lib/api/types";
import { ndjsonPublishable } from "@/lib/provenance";
import { digestPrefix } from "@/lib/run/evidence";
import { baseName, ownLogFileName, receiptFileName, verifyCommand } from "@/lib/run/receipt";
import { mergeEvents } from "@/lib/run/view";
import { type LogPin, pinDisplay, pinSource } from "@/lib/showcase";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { LogPinNote } from "../showcase/LogPinNote";
import { ChainCheck } from "./ChainCheck";
import { type RunTimeReceiptReader, RunTimeReceiptNote } from "./RunTimeReceipt";
import type { RunReceipt } from "./useReceipt";
import type { MissingEntry } from "./useRecordedRun";

type AuditTabProps = {
  runId: string;
  detail: RunDetailV1 | null;
  frames: ConsensusFrameV1[];
  detailError: ApiError | null;
  summary: RunSummaryV1 | null;
  receipt: RunReceipt;
  /** The showcase entry of a recorded run (its research file and SHA-256); null for a run of this server. */
  archive: ShowcaseEntryV1 | null;
  /** The entry's log digest cross-checked against the manifest this browser checked; null without an entry. */
  pin: LogPin | null;
  /** Why a recorded run has no entry (the listing is being read, failed, or does not list it); null otherwise. */
  archiveMissing: MissingEntry | null;
  /** The receipt a recorded run's record holds, read on request. */
  runTime: RunTimeReceiptReader;
};

/** Authorities with their own word (`Run.explorer.authority.<value>`). */
const AUTHORITIES = ["operator", "public_demo", "unspecified"] as const;

/** The log section of a recorded run without its entry (data-audit-log), and its sentence, by why. */
const MISSING_LOG = {
  loading: { attribute: "archive-loading", key: "archiveLoading" },
  error: { attribute: "archive-error", key: "archiveFailed" },
  unlisted: { attribute: "archive-pending", key: "archivePending" },
} as const;

export function AuditTab({
  runId,
  detail,
  frames,
  detailError,
  summary,
  receipt,
  archive,
  pin,
  archiveMissing,
  runTime,
}: AuditTabProps) {
  const t = useTranslations("Run.audit");
  const events = useMemo(() => mergeEvents(detail, frames), [detail, frames]);
  const recorded = summary?.provenance.source === "research_archive";
  const chain = detail?.chain ?? null;
  const state = receipt.state;

  // The verbatim log: a recorded run's research file, or this server's
  // events.ndjson when the public surface publishes it.
  const log =
    archive !== null
      ? { href: api.researchUrl(archive.file), name: baseName(archive.file), sha256: archive.sha256 }
      : !recorded && summary !== null && ndjsonPublishable(summary.provenance)
        ? { href: api.ndjsonUrl(runId), name: ownLogFileName(runId), sha256: null }
        : null;
  const receiptHref = recorded ? api.showcaseReceiptUrl(runId) : api.receiptUrl(runId);
  const command = verifyCommand(log?.name ?? ownLogFileName(runId), runId, receiptFileName(runId));
  // A recorded run's missing log, by why: the listing still read, failed, or not listing it.
  const missing = recorded && log === null ? MISSING_LOG[archiveMissing ?? "unlisted"] : null;
  // The digest a downloaded research file is compared with: the checked manifest's own pin when there is one.
  const expected =
    log === null || log.sha256 === null
      ? null
      : pin === null
        ? { sha256: log.sha256, source: "listing" as const }
        : { sha256: pin.sha256, source: pinSource(pin) };
  // The digest shown as the manifest's pin, and the listing's when the checked manifest contradicts it or does not name the file.
  const shown =
    log === null || log.sha256 === null
      ? null
      : pin === null
        ? { pinned: log.sha256, listed: null }
        : pinDisplay(pin);

  return (
    <div className="hc-audit-tab" data-audit-tab>
      <h3 id="audit-heading">{t("heading")}</h3>
      <p className="hc-supporting">{t("intro")}</p>
      {detailError === null ? null : (
        <p className="hc-supporting" data-audit-partial>
          {t("partial")}
        </p>
      )}

      <section className="hc-section" aria-labelledby="audit-chain" data-audit-chain={chain?.scope ?? "unknown"}>
        <h4 id="audit-chain">{t("chainHeading")}</h4>
        {chain === null ? (
          <p className="hc-supporting">{t("chainUnknown")}</p>
        ) : (
          <>
            <p data-chain-scope={chain.scope}>{t(`scopes.${chain.scope}`)}</p>
            <dl className="hc-kv">
              <div>
                <dt>{t("firstSequence")}</dt>
                <dd className="hc-tabular-nums" data-chain-field="first">
                  {chain.first_sequence}
                </dd>
              </div>
              <div>
                <dt>{t("lastSequence")}</dt>
                <dd className="hc-tabular-nums" data-chain-field="last">
                  {chain.last_sequence}
                </dd>
              </div>
              <div className="hc-kv-wide">
                <dt>{t("headHash")}</dt>
                <dd data-chain-field="head">
                  {chain.head_hash === undefined || chain.head_hash === null ? (
                    <span className="hc-supporting">{t("noHead")}</span>
                  ) : (
                    <code className="hc-code">{chain.head_hash}</code>
                  )}
                </dd>
              </div>
            </dl>
          </>
        )}
        <details className="hc-json-details" data-audit-links>
          <summary>{t("linksSummary", { count: events.length })}</summary>
          <p className="hc-supporting" data-display-copy>
            {t("displayCopy")}
          </p>
          <table className="hc-timeline">
            <caption className="hc-visually-hidden">{t("linksCaption")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("sequence")}</th>
                <th scope="col">{t("prevHash")}</th>
                <th scope="col">{t("hash")}</th>
              </tr>
            </thead>
            <tbody>
              {events.map((event) => (
                <tr key={event.sequence} data-link-sequence={event.sequence}>
                  <td className="hc-tabular-nums">{event.sequence}</td>
                  <td>
                    <code className="hc-code">{event.prev_hash === "" ? t("emptyHash") : linkText(event.prev_hash)}</code>
                  </td>
                  <td>
                    <code className="hc-code">{event.hash === "" ? t("emptyHash") : linkText(event.hash)}</code>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      </section>

      <section
        className="hc-section"
        aria-labelledby="audit-log"
        data-audit-log={log === null ? (missing?.attribute ?? "unavailable") : "published"}
      >
        <h4 id="audit-log">{t("logHeading")}</h4>
        {log === null ? (
          <p
            className="hc-supporting"
            data-log-unavailable
            role={missing?.attribute === "archive-loading" ? "status" : undefined}
          >
            {missing === null ? t("logUnavailable") : t(missing.key)}
          </p>
        ) : (
          <>
            <p>
              {archive === null ? t("logOwn") : pin?.status === "unlisted" ? t("logArchiveUnlisted") : t("logArchive")}
            </p>
            <ul className="hc-related" role="list">
              <li>
                <a href={log.href} download={log.name} className="hc-link" data-log-link>
                  {t("logDownload", { name: log.name })}
                </a>
              </li>
            </ul>
            {shown === null ? null : (
              <dl className="hc-kv">
                {shown.pinned === null ? null : (
                  <div className="hc-kv-wide">
                    <dt>{t("pinnedSha256")}</dt>
                    <dd data-log-sha256>
                      <code className="hc-code">{shown.pinned}</code>
                    </dd>
                  </div>
                )}
                {shown.listed === null ? null : (
                  <div className="hc-kv-wide">
                    <dt>{t("listedSha256")}</dt>
                    <dd data-log-listed-sha256>
                      <code className="hc-code">{shown.listed}</code>
                    </dd>
                  </div>
                )}
              </dl>
            )}
            {pin === null ? null : <LogPinNote pin={pin} />}
          </>
        )}
        <ul className="hc-related" role="list">
          <li>
            <a href={receiptHref} download={receiptFileName(runId)} className="hc-link" data-receipt-download>
              {t("receiptDownload", { name: receiptFileName(runId) })}
            </a>
          </li>
        </ul>
        {recorded ? (
          <RunTimeReceiptNote
            archive={archive}
            served={state.status === "ready" ? state.receipt : null}
            reader={runTime}
          />
        ) : null}
        {log === null && recorded ? null : (
          <>
            <p>{log === null ? t("cliOperator") : t("cli")}</p>
            <pre className="hc-cli hc-code" data-cli>
              {command}
            </pre>
          </>
        )}
      </section>

      {log === null ? null : (
        <ChainCheck
          runId={runId}
          source={{
            url: log.href,
            name: log.name,
            sha256: expected?.sha256 ?? null,
            pinSource: expected?.source ?? null,
          }}
          chain={chain}
          receipt={state.status === "ready" ? state.receipt : null}
        />
      )}

      <section className="hc-section" aria-labelledby="audit-receipt" data-audit-receipt={state.status}>
        <h4 id="audit-receipt">{t("receiptHeading")}</h4>
        {state.status === "ready" ? (
          <ReceiptSummary receipt={state.receipt} recorded={recorded} onReload={receipt.reload} />
        ) : state.status === "error" ? (
          <>
            <p>{t("receiptFailed")}</p>
            <ApiErrorNotice error={state.error} onRetry={receipt.reload} />
          </>
        ) : (
          <p role="status" className="hc-supporting">
            {t("receiptLoading")}
          </p>
        )}
      </section>
    </div>
  );
}

/** A hash or a chain anchor (genesis, pre-1.1:<n>) as the record carries it, a hash cut to its prefix. */
function linkText(value: string): string {
  return /^[0-9a-f]{64}$/.test(value) ? `${digestPrefix(value) ?? value}…` : value;
}

function ReceiptSummary({
  receipt,
  recorded,
  onReload,
}: {
  receipt: RunReceiptV2;
  /** A recorded run's receipt: this server's rebuild from the archived log. */
  recorded: boolean;
  onReload: () => void;
}) {
  const t = useTranslations("Run.audit");
  const tRunTime = useTranslations("Run.runTimeReceipt");
  const tRun = useTranslations("Run");
  const tExplorer = useTranslations("Run.explorer");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const format = useFormatter();
  const deliberation = receipt.deliberation;
  const authority = receipt.authorization.authority ?? null;
  const authorityText =
    authority === null
      ? t("noAuthority")
      : (AUTHORITIES as readonly string[]).includes(authority)
        ? tExplorer(`authority.${authority as (typeof AUTHORITIES)[number]}`)
        : tExplorer("authority.other", { authority });
  const dateTime = (iso: string) =>
    format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "medium" });
  const byStatus = new Map<string, number>();
  for (const { verification } of receipt.verifications) {
    byStatus.set(verification.integrity_status, (byStatus.get(verification.integrity_status) ?? 0) + 1);
  }
  const simulated = receipt.verifications.filter(
    ({ verification }) => verification.verifier === "fake-verifier",
  ).length;
  const rounds = [...new Set(receipt.verifications.map((v) => v.round))].sort((a, b) => a - b);
  const corroborated = deliberation.claims.filter((c) => c.assessment?.corroborated === true).length;
  // Of the objections never judged again, those their own round's
  // evaluation did not list as unresolved either.
  const neverRejudgedUnlisted = deliberation.earlier_round_objections_never_rejudged.filter(
    (objection) => !objection.unresolved_in_its_round,
  ).length;
  return (
    <div className="hc-receipt" data-receipt>
      <dl className="hc-kv">
        <div>
          <dt>{t("format")}</dt>
          <dd>
            <code className="hc-code">{receipt.format}</code>
          </dd>
        </div>
        <div>
          <dt>{t("generatedAt")}</dt>
          <dd className="hc-tabular-nums">{dateTime(receipt.generated_at)}</dd>
        </div>
        <div>
          <dt>{t("protocol")}</dt>
          <dd lang="en">{receipt.protocol}</dd>
        </div>
        <div>
          <dt>{t("schema")}</dt>
          <dd className="hc-tabular-nums">
            {t("schemaValue", { run: receipt.run_schema_version, software: receipt.schema_version })}
          </dd>
        </div>
        <div>
          <dt>{t("finalState")}</dt>
          <dd>{tRun(`states.${receipt.final_state}`)}</dd>
        </div>
        <div>
          <dt>{t("eventCount")}</dt>
          <dd className="hc-tabular-nums" data-receipt-field="event_count">
            {receipt.event_count}
          </dd>
        </div>
        <div className="hc-kv-wide">
          <dt>{t("receiptHead")}</dt>
          <dd data-receipt-field="head_hash">
            {receipt.chain.head_hash === undefined || receipt.chain.head_hash === null ? (
              <span className="hc-supporting">{t("noHead")}</span>
            ) : (
              <code className="hc-code">{receipt.chain.head_hash}</code>
            )}
          </dd>
        </div>
      </dl>
      <p className="hc-supporting">{t("chainNote")}</p>
      <blockquote lang="en" className="hc-untrusted-text" data-receipt-chain-note>
        {receipt.chain.note}
      </blockquote>

      <h5 className="hc-evidence-subheading">{t("authorizationHeading")}</h5>
      <p data-receipt-authorization={receipt.authorization.state}>
        {receipt.authorization.decided_at === undefined || receipt.authorization.decided_at === null
          ? t("authorizationPending", { state: t(`authorization.${receipt.authorization.state}`) })
          : t("authorizationDecided", {
              state: t(`authorization.${receipt.authorization.state}`),
              authority: authorityText,
              date: dateTime(receipt.authorization.decided_at),
            })}
      </p>
      {receipt.authorization.note === undefined || receipt.authorization.note === null ? null : (
        <p>{t("note", { note: receipt.authorization.note })}</p>
      )}

      <h5 className="hc-evidence-subheading">{t("deliberationHeading")}</h5>
      <ul className="hc-list" data-receipt-deliberation>
        <li>{t("rounds", { count: deliberation.rounds })}</li>
        <li>
          {t("finalProposal", {
            id: deliberation.final_proposal_id ?? t("none"),
            evaluated: deliberation.evaluated_proposal_id ?? t("none"),
          })}
        </li>
        <li>{t("claims", { count: deliberation.claims.length, corroborated })}</li>
        <li>{t("revisions", { count: deliberation.revisions.length })}</li>
        <li>{t("openItems", { count: deliberation.open_items.length })}</li>
        <li>{t("missingTests", { count: deliberation.missing_tests.length })}</li>
        <li>{t("unevaluatedItems", { count: deliberation.unevaluated_items.length })}</li>
        <li data-receipt-never-rejudged={deliberation.earlier_round_objections_never_rejudged.length}>
          {t("neverRejudged", { count: deliberation.earlier_round_objections_never_rejudged.length })}
        </li>
        {neverRejudgedUnlisted === 0 ? null : (
          <li data-receipt-never-rejudged-unlisted={neverRejudgedUnlisted}>
            {t("neverRejudgedUnlisted", { count: neverRejudgedUnlisted })}
          </li>
        )}
        <li>{t("unmatched", { count: deliberation.unmatched_assessments.length })}</li>
      </ul>

      <h5 className="hc-evidence-subheading">{t("callsHeading")}</h5>
      <p data-receipt-calls={receipt.model_invocations.length}>
        {t("calls", { count: receipt.model_invocations.length })}{" "}
        {receipt.model_invocations.length === 0
          ? null
          : t("callsScripted", {
              count: receipt.model_invocations.filter((i) => i.provider === "fake").length,
            })}
      </p>

      <h5 className="hc-evidence-subheading">{t("verificationsHeading")}</h5>
      <p data-receipt-verifications={receipt.verifications.length}>
        {t("verifications", { count: receipt.verifications.length, rounds: rounds.join(", ") })}
      </p>
      {byStatus.size === 0 ? null : (
        <ul className="hc-list">
          {[...byStatus.entries()].map(([status, count]) => (
            <li key={status}>
              {t("verificationsByStatus", { status: tProvenance(`proof.${status}`), count })}
            </li>
          ))}
        </ul>
      )}
      {simulated === 0 ? null : (
        <p data-receipt-simulated={simulated}>{t("verificationsSimulated", { count: simulated })}</p>
      )}

      <h5 className="hc-evidence-subheading">{t("limitationsHeading")}</h5>
      {recorded ? (
        <p className="hc-supporting" data-receipt-limitations-today>
          {tRunTime("limitsToday")}
        </p>
      ) : null}
      <blockquote lang="en" className="hc-untrusted-text" data-receipt-limitations>
        {receipt.proof_limitations}
      </blockquote>

      <details className="hc-json-details">
        <summary>{t("receiptJson")}</summary>
        <p className="hc-supporting" data-display-copy>
          {t("displayCopy")}
        </p>
        {/* Focusable: the JSON scrolls inside its box. */}
        <pre className="hc-json hc-code" tabIndex={0} aria-label={t("receiptJson")}>
          {JSON.stringify(receipt, null, 2)}
        </pre>
      </details>
      <div className="hc-actions">
        <Button variant="secondary" size="sm" label={t("receiptReload")} onClick={onReload} />
      </div>
    </div>
  );
}
