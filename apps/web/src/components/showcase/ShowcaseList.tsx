"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The research showcase: every archived run GET /showcase lists, grouped by
// battery, each read as RECORDED (source research_archive: a recorded run is
// never presented as running, and without a delivery its model badge is META
// MODEL by construction), its outcome always next to its evidence axis, its
// schema version (schema 1.0: pre-1.1, unchained), its authorization and
// record count, opening in the run workspace in RECORDED mode. Each battery
// names its log file with the SHA-256 the archive's manifest pins and
// links it for download; the manifest itself is hashed in this browser and
// compared with the digest the server published, and only a manifest that
// matches lends its notes (the archive's own words on each log, in English)
// and its own pin of each log, compared here with the listed one (a
// manifest that could not be read says so, with its own retry).

import { Badge } from "@astryxdesign/core/Badge";
import Link from "next/link";
import { useFormatter, useTranslations } from "next-intl";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { api } from "@/lib/api/client";
import type { ShowcaseEntryV1 } from "@/lib/api/types";
import { evidenceLabel, RECOMMENDATION_VARIANTS } from "@/lib/provenance";
import { digestPrefix } from "@/lib/run/evidence";
import { baseName } from "@/lib/run/receipt";
import { byBattery, isUnchained, manifestNote, parseOutcome, pinDisplay } from "@/lib/showcase";
import { showcaseRunPath } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { Callout } from "../Callout";
import { ProvenanceBadges } from "../ProvenanceBadges";
import { UntrustedText } from "../run/UntrustedText";
import { LogPinNote } from "./LogPinNote";
import { archivePin, checkedManifest, manifestUnconfirmed, type ManifestState, useArchive } from "./useArchive";

type ShowcaseListProps = {
  locale: Locale;
};

const AUTHORIZATIONS = ["pending", "approved", "rejected"] as const;

/** An entry's outcome in words, next to its evidence axis. */
export function useOutcomeText(): (entry: Pick<ShowcaseEntryV1, "outcome" | "provenance">) => {
  text: string;
  variant: "success" | "warning" | "neutral" | "error";
} {
  const tRun = useTranslations("Run");
  const t = useTranslations("Showcase");
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  return (entry) => {
    const evidence = tProvenance(evidenceLabel(entry.provenance.evidence).key);
    const outcome = parseOutcome(entry.outcome);
    switch (outcome.kind) {
      case "recommendation":
        return {
          text: tRun("outcome", { kind: tRun(`kinds.${outcome.value}`), evidence }),
          variant: RECOMMENDATION_VARIANTS[outcome.value] as "success" | "warning" | "neutral" | "error",
        };
      case "failed":
        return { text: t("failedAt", { stage: outcome.stage, evidence }), variant: "error" };
      case "other":
        return { text: t("otherOutcome", { outcome: outcome.value, evidence }), variant: "neutral" };
    }
  };
}

export function useAuthorizationText(): (authorization: string) => string {
  const t = useTranslations("Showcase");
  const tAudit = useTranslations("Run.audit.authorization");
  return (authorization) =>
    (AUTHORIZATIONS as readonly string[]).includes(authorization)
      ? tAudit(authorization as (typeof AUTHORIZATIONS)[number])
      : t("otherAuthorization", { authorization });
}

/**
 * The manifest's `created` date (a calendar day, "2026-10-27") in the page's
 * locale, as every other date is shown; a value that is not a calendar day
 * is shown as the manifest writes it.
 */
export function useManifestDate(): (created: string) => string {
  const format = useFormatter();
  return (created) => {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(created)) {
      return created;
    }
    const date = new Date(`${created}T00:00:00Z`);
    return Number.isNaN(date.getTime())
      ? created
      : format.dateTime(date, { dateStyle: "long", timeZone: "UTC" });
  };
}

export function ManifestLine({
  state,
  file,
  sha256,
  onRetry,
}: {
  state: ManifestState;
  file: string;
  sha256: string;
  /** Reads the manifest again after a failed download. */
  onRetry: () => void;
}) {
  const t = useTranslations("Showcase.manifest");
  return (
    <div className="hc-panel" data-manifest={state.status === "ready" ? (state.matches ? "matches" : "differs") : state.status}>
      <p>
        {t("pinned", { file, sha256: digestPrefix(sha256) ?? sha256 })}{" "}
        <a href={api.researchUrl(file)} download={baseName(file)} className="hc-link" data-manifest-link>
          {t("download", { file })}
        </a>
      </p>
      {/* Mounted empty while the manifest is checked; only the outcome is announced, once. */}
      <p className="hc-visually-hidden" role="status" data-manifest-announcement>
        {state.status !== "ready"
          ? null
          : state.matches
            ? t("matches")
            : t("differs", { sha256: digestPrefix(state.sha256) ?? state.sha256 })}
      </p>
      {state.status === "loading" ? (
        <p className="hc-supporting" data-manifest-checking>
          {t("checking")}
        </p>
      ) : state.status === "error" ? (
        <ApiErrorNotice error={state.error} onRetry={onRetry} />
      ) : state.matches ? (
        // An explicit status: the tag says the result in a word, the sentence how it was found.
        <p className="hc-manifest-status" data-manifest-status="matches">
          <Badge variant="success" label={t("matchesTag")} />{" "}
          <span data-manifest-check="matches">{t("matches")}</span>
        </p>
      ) : (
        <p className="hc-manifest-status" data-manifest-status="differs">
          <Badge variant="error" label={t("differsTag")} />{" "}
          <span data-manifest-check="differs">{t("differs", { sha256: digestPrefix(state.sha256) ?? state.sha256 })}</span>
        </p>
      )}
      <p className="hc-supporting">{t("trust")}</p>
    </div>
  );
}

export function ShowcaseList({ locale }: ShowcaseListProps) {
  const t = useTranslations("Showcase");
  const tRun = useTranslations("Run");
  const outcomeText = useOutcomeText();
  const authorizationText = useAuthorizationText();
  const manifestDate = useManifestDate();
  const { state, reload, retryManifest } = useArchive(true);

  if (state.status === "loading") {
    return (
      <p role="status" className="hc-supporting" data-showcase="loading">
        {t("loading")}
      </p>
    );
  }
  if (state.status === "error") {
    return (
      <div className="hc-section" data-showcase="error">
        <ApiErrorNotice error={state.error} onRetry={reload} />
      </div>
    );
  }
  const { showcase, manifest } = state;
  const entries = showcase.entries;
  if (entries.length === 0) {
    return (
      <p className="hc-panel" data-showcase="empty">
        {t("empty")}
      </p>
    );
  }
  const notes = checkedManifest(state);
  // A page state, not a label of any run: the archive was not confirmed in this browser.
  const unconfirmed = manifestUnconfirmed(state);
  const events = entries.reduce((sum, entry) => sum + entry.event_count, 0);
  return (
    <div className="hc-showcase" data-showcase="list">
      <p className="hc-run-count hc-tabular-nums" data-showcase-count={entries.length}>
        {t("count", { count: entries.length, events })}
      </p>
      <p className="hc-supporting">{t("recordedNote")}</p>
      {manifest.status === "ready" && !manifest.matches ? (
        <Callout tone="failed" title={t("manifest.differsTitle")} data-manifest-banner="differs">
          <p>{t("manifest.differsBody")}</p>
        </Callout>
      ) : null}
      <ManifestLine
        state={manifest}
        file={showcase.manifest_file}
        sha256={showcase.manifest_sha256}
        onRetry={retryManifest}
      />
      {byBattery(entries).map(({ battery, entries: group }, index) => {
        const files = [...new Set(group.map((entry) => entry.file))];
        const headingId = `battery-${index}`;
        return (
          <section key={battery} className="hc-section" aria-labelledby={headingId} data-battery={battery}>
            <h2 id={headingId}>
              <code className="hc-code">{battery}</code>
            </h2>
            <p className="hc-supporting hc-tabular-nums">{t("batteryCount", { count: group.length })}</p>
            {files.map((file) => {
              const sha256 = group.find((entry) => entry.file === file)?.sha256 ?? "";
              const note = manifestNote(notes, file);
              const pin = archivePin(state, file, sha256);
              // The manifest's own pin when it was checked (the listing's copy of it otherwise);
              // the listing's digest, said to be the server's, when the checked manifest does not name the file.
              const { pinned } = pinDisplay(pin);
              return (
                <div key={file} className="hc-archive-file" data-archive-file={file}>
                  <p>
                    <span data-log-file-sha256={pinned ?? sha256}>
                      {pinned === null
                        ? t("logFileListed", { file, sha256: digestPrefix(sha256) ?? sha256 })
                        : t("logFile", { file, sha256: digestPrefix(pinned) ?? pinned })}
                    </span>{" "}
                    <a href={api.researchUrl(file)} download={baseName(file)} className="hc-link" data-archive-download>
                      {t("download", { name: baseName(file) })}
                    </a>
                  </p>
                  <LogPinNote pin={pin} />
                  {note === null ? null : (
                    <div data-archive-note>
                      <p className="hc-supporting">
                        {notes?.created === null || notes?.created === undefined
                          ? t("noteHeading")
                          : t("noteHeadingDated", { created: manifestDate(notes.created) })}
                      </p>
                      <UntrustedText text={note} source="archive" />
                    </div>
                  )}
                </div>
              );
            })}
            <ul className="hc-run-list" role="list">
              {group.map((entry) => {
                const outcome = outcomeText(entry);
                const unchained = isUnchained(entry);
                return (
                  <li key={entry.run_id} className="hc-run-card" data-showcase-run={entry.run_id} data-label={entry.label}>
                    <h3 className="hc-card-title">
                      {/* The visible content is the link's name: the English
                          archive title keeps its lang="en" for screen readers,
                          which an aria-label in the page language would drop. */}
                      <Link href={showcaseRunPath(locale, entry.run_id)} className="hc-card-link">
                        <span className="hc-visually-hidden">{t("openPrefix")} </span>
                        {entry.label}
                        {" · "}
                        <span lang="en">{entry.title}</span>
                      </Link>
                    </h3>
                    <dl className="hc-kv">
                      <div>
                        <dt>{tRun("header.outcome")}</dt>
                        <dd data-showcase-outcome={entry.outcome}>
                          <Badge variant={outcome.variant} label={outcome.text} />
                        </dd>
                      </div>
                      <div>
                        <dt>{t("authorization")}</dt>
                        <dd>{authorizationText(entry.authorization)}</dd>
                      </div>
                      <div>
                        <dt>{t("schema")}</dt>
                        <dd className="hc-tabular-nums">
                          {t("schemaValue", { version: entry.schema_version })}
                          {unchained ? <Badge variant="warning" label={t("unchained")} /> : null}
                        </dd>
                      </div>
                      <div>
                        <dt>{tRun("header.recordsLabel")}</dt>
                        <dd className="hc-tabular-nums">{tRun("header.records", { count: entry.event_count })}</dd>
                      </div>
                    </dl>
                    {/* A recorded run is never presented as running: RECORDED, and META MODEL without a delivery. */}
                    <ProvenanceBadges provenance={entry.provenance} />
                    {unconfirmed ? (
                      <p data-archive-unconfirmed>
                        <Badge variant="warning" label={t("manifest.unconfirmedTag")} />
                      </p>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </div>
  );
}
