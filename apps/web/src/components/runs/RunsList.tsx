"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The session's runs. Without the browser's session flag (set once a POST or
// GET /session succeeded, cleared by DELETE /session or a 401) there is no
// session: nothing is asked, nothing listed, no count shown. With it (or
// when the browser's storage cannot hold it, so its absence says nothing), GET
// /session first (a 401 there clears the flag and lists nothing). With a
// session, GET /runs (newest first, cursor
// pagination) and GET /capabilities for the retention limits, from which each
// run's "access ends" and "deleted after" dates are derived. Every number and
// label comes from the run data.

import { Button } from "@astryxdesign/core/Button";
import Link from "next/link";
import { useFormatter, useTranslations } from "next-intl";
import { useEffect, useState } from "react";

import type { Locale } from "@/i18n/routing";
import { api, ApiError, isApiError, sessionFlag } from "@/lib/api/client";
import type { Capabilities } from "@/lib/api/schemas";
import type { RunSummaryV1 } from "@/lib/api/types";
import { OUTCOME_ICONS, RECOMMENDATION_VARIANTS } from "@/lib/provenance";
import { localePath, runPath } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { OutcomeTags, StatusTag } from "../OutcomeTags";
import { ProvenanceBadges } from "../ProvenanceBadges";
import { Tag } from "../Tag";
import { useFailureText } from "../run/FailureSentence";

type RunsListProps = {
  locale: Locale;
};

type ListState =
  | { status: "loading" }
  | { status: "no-session" }
  | { status: "error"; error: ApiError }
  | {
      status: "ready";
      runs: RunSummaryV1[];
      nextCursor: string | null;
      limits: Capabilities["limits"] | null;
    };

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

function toApiError(error: unknown): ApiError {
  return isApiError(error)
    ? error
    : new ApiError({
        code: "unreachable",
        message: error instanceof Error ? error.message : "request failed",
        status: 0,
        path: "/runs",
      });
}

export function RunsList({ locale }: RunsListProps) {
  const t = useTranslations("Runs");
  const tRun = useTranslations("Run");
  const format = useFormatter();
  const failureText = useFailureText();
  const [state, setState] = useState<ListState>({ status: "loading" });
  const [loadingMore, setLoadingMore] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    const apply = (next: ListState) => {
      if (!cancelled) {
        setState(next);
      }
    };
    // Without the session flag this browser holds no session (src/lib/api/
    // client.ts SESSION_FLAG_KEY): nothing to list, and nothing to ask, so
    // no 401 reaches the console. A storage that cannot be read or written
    // tells nothing ("unknown"): the server is asked, as before the flag.
    if (sessionFlag() === "absent") {
      apply({ status: "no-session" });
      return () => {
        cancelled = true;
      };
    }
    api
      .getSession()
      .then((session) => {
        if (session === null) {
          apply({ status: "no-session" });
          return;
        }
        return Promise.all([api.listRuns(), api.capabilities().catch(() => null)]).then(
          ([list, capabilities]) => {
            apply({
              status: "ready",
              runs: list.runs,
              nextCursor: list.next_cursor ?? null,
              limits: capabilities?.limits ?? null,
            });
          },
        );
      })
      .catch((error: unknown) => apply({ status: "error", error: toApiError(error) }));
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  const refresh = () => {
    setState({ status: "loading" });
    setAttempt((n) => n + 1);
  };

  const loadMore = async () => {
    if (state.status !== "ready" || state.nextCursor === null) {
      return;
    }
    setLoadingMore(true);
    try {
      const list = await api.listRuns({ cursor: state.nextCursor });
      setState({
        ...state,
        runs: [...state.runs, ...list.runs],
        nextCursor: list.next_cursor ?? null,
      });
    } catch (error) {
      setState({ status: "error", error: toApiError(error) });
    } finally {
      setLoadingMore(false);
    }
  };

  const dateTime = (iso: string) =>
    format.dateTime(new Date(iso), { dateStyle: "medium", timeStyle: "short" });

  if (state.status === "loading") {
    return (
      <p role="status" className="hc-supporting">
        {tRun("loading")}
      </p>
    );
  }
  if (state.status === "error") {
    return (
      <div className="hc-section">
        <ApiErrorNotice error={state.error} />
        <div className="hc-actions">
          <Button variant="secondary" label={t("refresh")} onClick={refresh} />
        </div>
      </div>
    );
  }
  if (state.status === "no-session") {
    return (
      <div className="hc-panel" data-runs="no-session">
        <p>{t("noSession")}</p>
        <Link href={localePath(locale, "new")} className="hc-link">
          {t("startOne")}
        </Link>
      </div>
    );
  }

  const { runs, limits } = state;
  return (
    <section className="hc-section" data-runs="list">
      <div className="hc-list-head">
        <p className="hc-run-count hc-tabular-nums" data-run-count={runs.length}>
          {t("count", { count: runs.length })}
        </p>
        <Button variant="secondary" label={t("refresh")} onClick={refresh} />
      </div>
      {runs.length === 0 ? (
        <div className="hc-panel">
          <p>{t("empty")}</p>
          <Link href={localePath(locale, "new")} className="hc-link">
            {t("startOne")}
          </Link>
        </div>
      ) : (
        <ul className="hc-run-list" role="list">
          {runs.map((run) => {
            const title = run.title ?? t("untitled");
            const created = new Date(run.created_at).getTime();
            return (
              <li key={run.run_id} className="hc-run-card hc-glass hc-glass--static hc-glass--interactive" data-run-id={run.run_id}>
                <h2 className="hc-card-title">
                  <Link
                    href={runPath(locale, run.run_id)}
                    className="hc-card-link"
                    aria-label={t("open", { title })}
                  >
                    {title}
                  </Link>
                </h2>
                <dl className="hc-kv">
                  <div>
                    <dt>{t("state")}</dt>
                    <dd data-runs-state={run.state}>
                      {/* The state that needs the visitor is a status tag, not plain text. */}
                      {run.state === "awaiting_human_approval" ? (
                        <StatusTag variant="blue" role="pending" label={tRun(`states.${run.state}`)} />
                      ) : (
                        tRun(`states.${run.state}`)
                      )}
                      {run.active ? (
                        <Tag variant="blue" label={t("active")} />
                      ) : null}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("outcome")}</dt>
                    <dd>
                      {run.recommendation === undefined || run.recommendation === null ? (
                        run.state === "failed" ? (
                          tRun("header.noOutcomeFailed")
                        ) : (
                          tRun("header.noOutcome")
                        )
                      ) : (
                        <OutcomeTags
                          outcome={tRun(`kinds.${run.recommendation}`)}
                          variant={RECOMMENDATION_VARIANTS[run.recommendation]}
                          icon={OUTCOME_ICONS[run.recommendation]}
                          evidence={run.provenance.evidence}
                        />
                      )}
                    </dd>
                  </div>
                </dl>
                {run.failure === undefined || run.failure === null ? null : (
                  <p className="hc-supporting" data-run-failure={run.failure.code ?? ""}>
                    {failureText(run.failure)}
                  </p>
                )}
                {/* A list never reads LIVE: a live-model run is META MODEL, recorded <date>. */}
                <ProvenanceBadges provenance={run.provenance} recordedAt={run.updated_at} />
                <ul className="hc-run-dates hc-supporting" role="list">
                  <li>{t("created", { date: dateTime(run.created_at) })}</li>
                  <li>{t("updated", { date: dateTime(run.updated_at) })}</li>
                  {limits === null ? null : (
                    <>
                      <li>
                        {t("accessEnds", {
                          date: dateTime(
                            new Date(created + limits.visitor_access_hours * HOUR).toISOString(),
                          ),
                        })}
                      </li>
                      <li>
                        {t("deletedAfter", {
                          date: dateTime(
                            new Date(created + limits.visitor_delete_days * DAY).toISOString(),
                          ),
                        })}
                      </li>
                    </>
                  )}
                </ul>
              </li>
            );
          })}
        </ul>
      )}
      {state.nextCursor === null ? null : (
        <div className="hc-actions">
          <Button
            variant="secondary"
            label={t("loadMore")}
            isLoading={loadingMore}
            onClick={() => void loadMore()}
          />
        </div>
      )}
    </section>
  );
}
