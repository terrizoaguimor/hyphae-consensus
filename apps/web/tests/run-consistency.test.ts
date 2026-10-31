// SPDX-License-Identifier: Apache-2.0
//
// The page's own consistency checks (src/lib/run/consistency.ts): labels
// that the run's records contradict (a model label of Meta's model over
// scripted steps, an evidence label of Hyphae over simulated proofs, and the
// other way round), and a receipt that disagrees with the run once neither
// is moving on; every computed message key exists in the four catalogs, with
// its plural branches, and no new sentence carries the LIVE word.

import { readFileSync } from "node:fs";
import path from "node:path";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { routing } from "../src/i18n/routing";
import type { ModelInvocationV1, ProvenanceV1, RunDetailV1, RunReceiptV2, VerificationV1 } from "../src/lib/api/types";
import {
  LABEL_DISAGREEMENT_KINDS,
  labelDisagreements,
  receiptDisagreements,
} from "../src/lib/run/consistency";

function fixture<T>(name: string): T {
  return JSON.parse(readFileSync(path.join(contractDir(), "fixtures", name), "utf8")) as T;
}

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

/** The retry run (scripted roles, simulated proofs) under another provenance. */
function retryAs(model: ProvenanceV1["model"], evidence: ProvenanceV1["evidence"]): RunDetailV1 {
  const detail = clone(fixture<RunDetailV1>("run-detail-retry.json"));
  detail.summary.provenance.model = model;
  detail.summary.provenance.evidence = evidence;
  return detail;
}

/** Every invocation a recorded Meta call, every verification a real verifier's. */
function realRecords(detail: RunDetailV1): RunDetailV1 {
  for (const event of detail.events) {
    const payload = event.payload as { invocation?: ModelInvocationV1; verification?: VerificationV1 };
    if (payload.invocation !== undefined && payload.invocation !== null) {
      payload.invocation = { ...payload.invocation, provider: "meta", model: "muse-wonder-1.0", scripted: false };
    }
    if (payload.verification !== undefined) {
      payload.verification = { ...payload.verification, simulated: false, verifier: "hyphae proof verify" };
    }
  }
  return detail;
}

describe("labelDisagreements", () => {
  it("finds nothing on a run whose labels match its records", () => {
    const scripted = fixture<RunDetailV1>("run-detail-retry.json");
    expect(scripted.summary.provenance.model).toBe("scripted");
    expect(labelDisagreements(scripted.summary.provenance, scripted.events)).toEqual([]);
    const real = realRecords(retryAs("live_meta", "hyphae_snapshot"));
    expect(labelDisagreements(real.summary.provenance, real.events)).toEqual([]);
    const r01 = fixture<RunDetailV1>("run-detail-live-R01-reference.json");
    expect(labelDisagreements(r01.summary.provenance, r01.events)).toEqual([]);
  });

  it("names Meta's model over scripted steps and Hyphae evidence over simulated proofs, with counts", () => {
    const detail = retryAs("live_meta", "hyphae_snapshot");
    const scripted = detail.events.filter((e) => (e.payload as { invocation?: ModelInvocationV1 }).invocation?.scripted === true).length;
    const simulated = detail.events.filter((e) => e.payload.type === "evidence_verified").length;
    expect(scripted).toBeGreaterThan(0);
    expect(simulated).toBeGreaterThan(0);
    expect(labelDisagreements(detail.summary.provenance, detail.events)).toEqual([
      { kind: "metaModelScriptedSteps", count: scripted },
      { kind: "memoryEvidenceSimulatedProofs", count: simulated },
    ]);
    // A Hyphae recall label reads the same.
    expect(labelDisagreements({ model: "live_meta", evidence: "hyphae_live" }, detail.events).map((d) => d.kind)).toEqual([
      "metaModelScriptedSteps",
      "memoryEvidenceSimulatedProofs",
    ]);
  });

  it("names a scripted label over model calls and a simulated label over real proofs", () => {
    const detail = realRecords(retryAs("scripted", "simulated"));
    expect(labelDisagreements(detail.summary.provenance, detail.events).map((d) => d.kind)).toEqual([
      "scriptedModelCalls",
      "simulatedEvidenceRealProofs",
    ]);
  });

  it("counts no proof as a real verifier's when no verifier ran", () => {
    // The proof verifier with its artifacts missing: status unavailable and no
    // verifier named, so the proof is not simulated, and not a real verifier's either.
    const detail = realRecords(retryAs("scripted", "simulated"));
    const verifications = detail.events
      .filter((event) => event.payload.type === "evidence_verified")
      .map((event) => (event.payload as { verification?: VerificationV1 }).verification)
      .filter((verification): verification is VerificationV1 => verification !== undefined);
    expect(verifications.length).toBeGreaterThan(0);
    for (const verification of verifications) {
      verification.verifier = null;
    }
    expect(labelDisagreements(detail.summary.provenance, detail.events).map((d) => d.kind)).toEqual([
      "scriptedModelCalls",
    ]);
    for (const verification of verifications) {
      delete verification.verifier;
    }
    expect(labelDisagreements(detail.summary.provenance, detail.events).map((d) => d.kind)).toEqual([
      "scriptedModelCalls",
    ]);
    // One proof that names its verifier is counted, alone.
    verifications[0]!.verifier = "hyphae proof verify";
    expect(labelDisagreements(detail.summary.provenance, detail.events)).toContainEqual({
      kind: "simulatedEvidenceRealProofs",
      count: 1,
    });
  });

  it("says nothing for a mixed or undetermined axis, nor for no evidence", () => {
    const detail = retryAs("mixed", "mixed");
    expect(labelDisagreements(detail.summary.provenance, detail.events)).toEqual([]);
    expect(labelDisagreements({ model: "undetermined", evidence: "none" }, detail.events)).toEqual([]);
    expect(labelDisagreements({ model: "live_meta", evidence: "hyphae_snapshot" }, [])).toEqual([]);
  });
});

describe("receiptDisagreements", () => {
  const receipt = fixture<RunReceiptV2>("receipt-v2-retry.json");
  const atRest = { state: receipt.final_state, active: false, event_count: receipt.event_count };

  it("agrees when the receipt and the run say the same", () => {
    expect(receiptDisagreements(receipt, atRest, false)).toEqual([]);
  });

  it("names the final state and the event count when they differ, for a run at rest", () => {
    expect(
      receiptDisagreements(
        { final_state: "approved", event_count: 15 },
        { state: "awaiting_human_approval", active: false, event_count: 14 },
        false,
      ),
    ).toEqual([
      { field: "finalState", receipt: "approved", run: "awaiting_human_approval" },
      { field: "eventCount", receipt: 15, run: 14 },
    ]);
  });

  it("compares a run still moving only with a receipt that claims a final state", () => {
    const moving = { state: "created" as const, active: true, event_count: 1 };
    // A receipt read a little before or after the run's newest record: not compared.
    expect(receiptDisagreements({ final_state: "evidence_review", event_count: 4 }, moving, false)).toEqual([]);
    // A final state a run never leaves: compared (the shot-38 case: approved, 15 events, against created, 1).
    expect(receiptDisagreements({ final_state: "approved", event_count: 15 }, moving, false)).toEqual([
      { field: "finalState", receipt: "approved", run: "created" },
      { field: "eventCount", receipt: 15, run: 1 },
    ]);
  });

  it("compares nothing while a newer receipt is being read", () => {
    expect(
      receiptDisagreements(
        { final_state: "awaiting_human_approval", event_count: 14 },
        { state: "approved", active: false, event_count: 15 },
        true,
      ),
    ).toEqual([]);
  });
});

type Catalog = { [key: string]: string | Catalog };

/** Each catalog read and parsed once (the bind-mounted file is slow to read again for every key). */
const catalogs = new Map<string, Catalog>();

function catalog(locale: string): Catalog {
  let parsed = catalogs.get(locale);
  if (parsed === undefined) {
    parsed = JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8")) as Catalog;
    catalogs.set(locale, parsed);
  }
  return parsed;
}

function lookup(locale: string, key: string): string {
  let node: string | Catalog | undefined = catalog(locale);
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key} in ${locale}`);
  }
  return node;
}

/** The new keys of the callouts (chain broken, labels and receipt that disagree, manifest not confirmed). */
const STATIC_KEYS = [
  "Run.chainBroken.title",
  "Run.chainBroken.atRecord",
  "Run.chainBroken.atLine",
  "Run.chainBroken.copies",
  "Run.chainBroken.beforeDeciding",
  "Run.chainBroken.details",
  "Run.chainBroken.pageState",
  "Run.chainBroken.openAudit",
  "Run.consistency.flagTitle",
  "Run.consistency.labelsTitle",
  "Run.consistency.labelsBody",
  "Run.consistency.receipt.title",
  "Run.consistency.receipt.finalState",
  "Run.consistency.receipt.eventCount",
  "Run.consistency.receipt.body",
  "Run.audit.finalStateRecorded",
  "Run.audit.finalStateReadOnly",
  "Showcase.run.manifestProblemTitle",
  "Showcase.manifest.matchesTag",
  "Showcase.manifest.differsTag",
  "Showcase.manifest.differsTitle",
  "Showcase.manifest.differsBody",
  "Showcase.manifest.unconfirmedTag",
  "Showcase.manifest.unparsedTag",
  "Showcase.manifest.unparsed",
  "Showcase.manifest.unparsedTitle",
];

describe.each(routing.locales)("%s consistency sentences", (locale) => {
  const live = lookup(locale, "Provenance.live.label").replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const liveWord = new RegExp(`(^|[^\\p{L}])${live}([^\\p{L}]|$)`, "iu");

  it("has every computed label sentence, with its count", () => {
    for (const kind of LABEL_DISAGREEMENT_KINDS) {
      const message = lookup(locale, `Run.consistency.labels.${kind}`);
      for (const count of [0, 1, 3]) {
        const text = String(new IntlMessageFormat(message, locale).format({ count }));
        expect(text.trim(), `${kind} ${count}`).not.toBe("");
        if (count > 0) {
          expect(text, `${kind} ${count}`).toContain(String(count));
        }
        expect(text, `${kind} ${count}`).not.toMatch(liveWord);
      }
    }
  });

  it("has every callout sentence, none with the LIVE word", () => {
    for (const key of STATIC_KEYS) {
      const message = lookup(locale, key);
      expect(message.trim(), key).not.toBe("");
      expect(message, key).not.toMatch(liveWord);
    }
    const format = (key: string, values: Record<string, string | number>) =>
      String(new IntlMessageFormat(lookup(locale, key), locale).format(values));
    const problem = "x_problem";
    expect(format("Run.chainBroken.atRecord", { sequence: 120, line: 121, problem })).toContain("120");
    expect(format("Run.chainBroken.atRecord", { sequence: 120, line: 121, problem })).toContain("121");
    expect(format("Run.chainBroken.atLine", { line: 7, problem })).toContain("7");
    // The reason is the verifier's own sentence, passed in: the catalog claims no cause of its own.
    expect(format("Run.chainBroken.atRecord", { sequence: 120, line: 121, problem })).toContain(problem);
    expect(format("Run.chainBroken.atLine", { line: 7, problem })).toContain(problem);
    expect(lookup(locale, "Run.chainBroken.atRecord")).not.toMatch(/hash|hachage/iu);
    expect(lookup(locale, "Run.chainBroken.atLine")).not.toMatch(/hash|hachage/iu);
    expect(format("Run.consistency.receipt.finalState", { receipt: "x_receipt", run: "x_run" })).toContain("x_receipt");
    expect(format("Run.consistency.receipt.finalState", { receipt: "x_receipt", run: "x_run" })).toContain("x_run");
    expect(format("Run.consistency.receipt.eventCount", { receipt: 15, run: 1 })).toMatch(/15.*1/u);
  });
});
