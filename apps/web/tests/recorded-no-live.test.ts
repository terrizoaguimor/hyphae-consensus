// SPDX-License-Identifier: Apache-2.0
//
// The honesty rule for recorded runs over the catalogs themselves: no string
// the showcase, a recorded run's workspace (its header, every tab, the chain
// check, the explanations), the Overview's figures or the shell around them
// can render carries the locale's LIVE word (LIVE, EN VIVO, AO VIVO, EN
// DIRECT, as a whole word), in any of the four locales. The few strings that
// do name live mode are listed below, each with the reason it can never be
// rendered as a label of a recorded run; anything else with the word fails.
// The whole Provenance namespace is scanned here too (badges, proof statuses,
// flags and their notes, axes, the model notes, the integrity note); the
// label functions of src/lib/provenance.ts have their own scans as well
// (provenance.test.ts, showcase.test.ts).

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";

type Catalog = { [key: string]: string | Catalog };

const read = (locale: string) =>
  JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8")) as Catalog;

function lookup(catalog: Catalog, key: string): string | Catalog | undefined {
  let node: string | Catalog | undefined = catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  return node;
}

/** Every string under a namespace, with its full key. */
function strings(node: string | Catalog | undefined, prefix: string): [string, string][] {
  if (node === undefined) {
    throw new Error(`no namespace ${prefix}`);
  }
  if (typeof node === "string") {
    return [[prefix, node]];
  }
  return Object.entries(node).flatMap(([key, child]) => strings(child, `${prefix}.${key}`));
}

/** The locale's LIVE word as a whole word, as e2e/helpers.ts liveWord matches it. */
function liveWord(catalog: Catalog): RegExp {
  const word = lookup(catalog, "Provenance.live.label");
  if (typeof word !== "string") {
    throw new Error("no Provenance.live.label");
  }
  return new RegExp(`(^|[^\\p{L}])${word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}([^\\p{L}]|$)`, "iu");
}

/**
 * The namespaces the recorded views and the shell around them read: the
 * showcase, the whole run workspace (Run.*: header, tabs, decision, explorer,
 * evidence, proofs, audit, chain check, Meta calls, explanations, revision,
 * failures), the provenance labels and notes every run header and tab shows
 * (Provenance: badges, proof statuses, flags, flag notes, axes, model notes),
 * the Overview's figures, the API error sentences (ApiErrorNotice), and the
 * shell of every page.
 */
const NAMESPACES = [
  "Showcase",
  "Provenance",
  "Run",
  "Overview.figures",
  "Api",
  "Metadata",
  "App",
  "Shell",
  "Nav",
  "Theme",
  "LocaleSwitcher",
  "NotConnected",
  "ApiGate",
  "NotFound",
  "Footer",
] as const;

/**
 * Strings in those namespaces that name live mode on purpose, and why none of
 * them is ever a label of a recorded run. Exact keys only: a new string with
 * the LIVE word is not covered by its neighbour's entry.
 */
const ALLOWED: Record<string, string> = {
  // The sentence says a replay is never presented as a live run; rendered only
  // by the showcase page's not-connected placeholder (ApiGate planned list).
  "Showcase.planned.recorded": "negation, not-connected placeholder only",
  // The tab of a live run's stream; a recorded run has no such tab (e2e
  // expectNoLive asserts there is no tab named Run.tabs.live).
  "Run.tabs.live": "a live run's tab, absent from recorded runs",
  // The LIVE label itself, and the model label and note that only isLive()
  // yields (delivery live and model live_meta, src/lib/provenance.ts
  // modelLabel, deliveryLabel, modelNoteKey); a recorded run is a replay, so
  // it reads META MODEL, REPLAY and "Meta model · recorded <date>" instead
  // (provenance.test.ts, e2e expectNoLive).
  "Provenance.live.label": "the LIVE label, isLive() only",
  "Provenance.liveMeta.label": "the LIVE META label, isLive() only",
  "Provenance.liveMeta.live": "the live model note, isLive() only",
  // The Overview's legend of every label (ProvenanceLabels.tsx, rendered by
  // the Overview page only): its introduction and the descriptions of the
  // live labels, which say what LIVE means and when it is not shown.
  "Provenance.intro": "Overview legend only",
  "Provenance.liveMeta.description": "Overview legend only",
  "Provenance.metaModel.description": "Overview legend only",
  "Provenance.hybrid.description": "Overview legend only",
  // A negation: a run with this flag "is never shown as live".
  "Provenance.flagNotes.provenance_inconsistent": "negation",
  // Shown only for a run of this server whose log is withheld; a recorded run
  // reads Run.decision.archiveLogPending / Run.audit.archivePending instead.
  "Run.decision.ndjsonUnavailable": "this server's runs only",
  "Run.audit.logUnavailable": "this server's runs only",
  // Why a run of this server's live mode stopped: its live budget refused or
  // could not settle a model call. A sentence about the cause, not a label of
  // the run's delivery; no run in research/ records either code, so no
  // recorded view shows them.
  "Run.failure.codes.budget_refused": "a live run's budget failure",
  "Run.failure.codes.budget_unsettled": "a live run's budget failure",
  // Errors the server answers only to the live endpoints (POST /runs/live,
  // the live budget), never to GET /showcase or a recorded run.
  "Api.errors.live_disabled": "live endpoint error",
  "Api.errors.ndjson_unavailable": "a live run's withheld log",
  "Api.errors.live_budget_daily": "live endpoint error",
  "Api.errors.live_budget_lifetime": "live endpoint error",
  "Api.errors.live_concurrency": "live endpoint error",
  "Api.errors.live_hourly_admissions": "live endpoint error",
  "Api.errors.live_client_quota": "live endpoint error",
  "Api.errors.live_tripped": "live endpoint error",
  "Api.errors.live_unavailable": "live endpoint error",
  "Api.errors.turnstile_required": "live endpoint error",
  "Api.errors.attestation_required": "live endpoint error",
  "Api.errors.geo_blocked_live": "live endpoint error",
};

const catalogs = routing.locales.map((locale) => ({ locale, messages: read(locale) }));

describe("no catalog string a recorded view renders carries the LIVE word", () => {
  it.each(catalogs)("$locale", ({ locale, messages }) => {
    const word = liveWord(messages);
    const scanned = NAMESPACES.flatMap((namespace) => strings(lookup(messages, namespace), namespace));
    expect(scanned.length).toBeGreaterThan(500);
    const hits = scanned.filter(([key, text]) => !(key in ALLOWED) && word.test(text));
    expect(hits, `${locale}: strings with the LIVE word`).toEqual([]);
  });

  it.each(catalogs)("every allowed exception exists ($locale)", ({ messages }) => {
    for (const key of Object.keys(ALLOWED)) {
      expect(typeof lookup(messages, key), key).toBe("string");
    }
  });
});
