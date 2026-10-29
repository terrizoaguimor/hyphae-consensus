"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Where a recorded run's receipt comes from. GET /showcase/{id}/receipt is
// built by this server from the archived log on each request: its
// generated_at is the request's time and its proof_limitations are this
// software's current text. The receipt downloaded at run time is the one the
// run's record in the archive holds (ShowcaseEntryV1.record_file, served at
// /research/<path>), with the wording of its day. The Proof inspector and
// the Audit trail say so next to the limitations and the receipt download,
// link the run record, and, when asked (an event, so nothing loads before it
// is wanted), quote that record's own limitations as it states them. The
// record is read once per run (useRunTimeReceipt, mounted by the workspace).

import { Button } from "@astryxdesign/core/Button";
import { useFormatter, useTranslations } from "next-intl";
import { useCallback, useRef, useState } from "react";

import { api, ApiError, isApiError } from "@/lib/api/client";
import type { RunReceiptV2, ShowcaseEntryV1 } from "@/lib/api/types";
import { baseName, runTimeReceipt, type RunTimeReceipt as Parsed } from "@/lib/run/receipt";

import { ApiErrorNotice } from "../ApiErrorNotice";

export type RunTimeReceiptState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; receipt: Parsed }
  | { status: "unreadable" }
  | { status: "error"; error: ApiError };

export type RunTimeReceiptReader = {
  state: RunTimeReceiptState;
  /** Reads the run record (again, after a failure). */
  read: () => void;
};

/** The receipt a recorded run's record holds; `recordFile` null reads nothing. */
export function useRunTimeReceipt(runId: string, recordFile: string | null): RunTimeReceiptReader {
  const [state, setState] = useState<RunTimeReceiptState>({ status: "idle" });
  const latest = useRef(0);
  const read = useCallback(() => {
    if (recordFile === null) {
      return;
    }
    latest.current += 1;
    const request = latest.current;
    const url = api.researchUrl(recordFile);
    setState({ status: "loading" });
    api
      .bytes(url)
      .then((bytes) => {
        if (request === latest.current) {
          const parsed = runTimeReceipt(bytes, runId);
          setState(parsed === null ? { status: "unreadable" } : { status: "ready", receipt: parsed });
        }
      })
      .catch((error: unknown) => {
        if (request === latest.current) {
          setState({
            status: "error",
            error: isApiError(error)
              ? error
              : new ApiError({
                  code: "unreachable",
                  message: error instanceof Error ? error.message : "download failed",
                  status: 0,
                  path: url,
                }),
          });
        }
      });
  }, [runId, recordFile]);
  return { state, read };
}

type RunTimeReceiptNoteProps = {
  /** The showcase entry (null while the listing is not read). */
  archive: ShowcaseEntryV1 | null;
  /** The receipt this server served, when it is read. */
  served: RunReceiptV2 | null;
  reader: RunTimeReceiptReader;
};

/** For a recorded run: the served receipt is this server's rebuild; the run record holds the run-time one. */
export function RunTimeReceiptNote({ archive, served, reader }: RunTimeReceiptNoteProps) {
  const t = useTranslations("Run.runTimeReceipt");
  const format = useFormatter();
  const dateTime = (iso: string) => format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "medium" });
  const record = archive?.record_file ?? null;
  const state = reader.state;
  return (
    <div className="hc-section" data-run-time-receipt={record === null ? "no-record" : state.status}>
      <p data-receipt-regenerated>
        {served === null ? t("regenerated") : t("regeneratedAt", { date: dateTime(served.generated_at) })}
      </p>
      {archive === null ? null : record === null ? (
        <p data-run-record="none">{t("noRecord")}</p>
      ) : (
        <>
          <p>{t("record")}</p>
          <ul className="hc-related" role="list">
            <li>
              <a href={api.researchUrl(record)} download={baseName(record)} className="hc-link" data-run-record-link>
                {t("recordDownload", { name: baseName(record) })}
              </a>
            </li>
          </ul>
          {state.status === "ready" ? (
            <RunTimeLimitations receipt={state.receipt} served={served} dateTime={dateTime} />
          ) : state.status === "unreadable" ? (
            <p data-run-record="unreadable">{t("unreadable")}</p>
          ) : (
            <>
              {state.status === "error" ? <ApiErrorNotice error={state.error} /> : null}
              <div className="hc-actions">
                <Button
                  variant="secondary"
                  size="sm"
                  label={state.status === "error" ? t("readAgain") : t("read")}
                  isDisabled={state.status === "loading"}
                  isLoading={state.status === "loading"}
                  onClick={reader.read}
                />
              </div>
              {state.status === "loading" ? (
                <p role="status" className="hc-supporting">
                  {t("reading")}
                </p>
              ) : null}
            </>
          )}
        </>
      )}
    </div>
  );
}

function RunTimeLimitations({
  receipt,
  served,
  dateTime,
}: {
  receipt: Parsed;
  served: RunReceiptV2 | null;
  dateTime: (iso: string) => string;
}) {
  const t = useTranslations("Run.runTimeReceipt");
  const limits = receipt.proofLimitations;
  const same = limits !== null && served !== null && limits.trim() === served.proof_limitations.trim();
  return (
    <div data-run-record="ready">
      <p>
        {receipt.generatedAt === null
          ? t("recordUndated")
          : t("recordGenerated", { date: dateTime(receipt.generatedAt) })}
      </p>
      {limits === null ? (
        <p data-run-record-limitations="none">{t("noLimitations")}</p>
      ) : (
        <>
          <p className="hc-supporting">
            {served === null ? t("limitsVerbatim") : same ? t("limitsSame") : t("limitsDiffer")}
          </p>
          <blockquote
            lang="en"
            className="hc-untrusted-text"
            data-run-record-limitations={served === null ? "unknown" : same ? "same" : "differ"}
          >
            {limits}
          </blockquote>
        </>
      )}
    </div>
  );
}
