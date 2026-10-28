// SPDX-License-Identifier: Apache-2.0
//
// The honesty predicates of release gate G7: when a frame may read LIVE, how
// replayed live-model records and scripted doubles read, what a simulated
// proof renders as, and that every label key exists in every catalog.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { routing } from "../src/i18n/routing";
import type { ProvenanceV1, RunDetailV1 } from "../src/lib/api/types";
import {
  BLOCKING_FLAGS,
  deliveryLabel,
  evidenceLabel,
  flagLabel,
  hasBlockingFlag,
  isAtRest,
  isLive,
  isPaceable,
  isScriptedRun,
  isTerminal,
  MODEL_PROVENANCES,
  modelLabel,
  modelNoteKey,
  ndjsonPublishable,
  proofToken,
  RECOMMENDATION_VARIANTS,
  sourceLabel,
} from "../src/lib/provenance";

const liveMeta: ProvenanceV1 = {
  model: "live_meta",
  evidence: "hyphae_snapshot",
  source: "this_server",
  flags: [],
};
const scripted: ProvenanceV1 = {
  model: "scripted",
  evidence: "simulated",
  source: "this_server",
};

describe("isLive", () => {
  it("needs delivery live AND model live_meta", () => {
    expect(isLive("live", liveMeta)).toBe(true);
    expect(isLive("replay", liveMeta)).toBe(false);
    expect(isLive("live", scripted)).toBe(false);
    expect(isLive("live", { ...liveMeta, model: "mixed" })).toBe(false);
    expect(isLive("live", { ...liveMeta, model: "undetermined" })).toBe(false);
  });

  it.each(BLOCKING_FLAGS)("never renders LIVE with the %s flag", (flag) => {
    const flagged = { ...liveMeta, flags: [flag] };
    expect(hasBlockingFlag(flagged)).toBe(true);
    expect(isLive("live", flagged)).toBe(false);
    expect(deliveryLabel("live", flagged).key).not.toBe("live.label");
    expect(flagLabel(flag).variant).toBe("warning");
  });

  it("treats a missing flags list as no flags", () => {
    expect(hasBlockingFlag({})).toBe(false);
    expect(isLive("live", { model: "live_meta" })).toBe(true);
  });
});

describe("delivery and model labels", () => {
  it("LIVE for a live frame of a live-model run; REPLAY for stored records", () => {
    expect(deliveryLabel("live", liveMeta)).toEqual({ key: "live.label", variant: "success" });
    expect(deliveryLabel("replay", liveMeta)).toEqual({ key: "replay.label", variant: "blue" });
  });

  it("a record published live for a scripted run is STREAMING, never LIVE", () => {
    expect(deliveryLabel("live", scripted).key).toBe("streaming.label");
  });

  it("a replayed frame of a live-model run reads 'Meta model · recorded'", () => {
    expect(modelNoteKey("replay", liveMeta)).toBe("liveMeta.recorded");
    expect(modelNoteKey("live", liveMeta)).toBe("liveMeta.live");
    expect(modelNoteKey("live", { ...liveMeta, flags: ["provenance_inconsistent"] })).toBe(
      "liveMeta.recorded",
    );
  });

  it("a scripted provider reads 'scripted double, no model call'", () => {
    expect(modelNoteKey("live", scripted)).toBe("scripted.note");
    expect(modelNoteKey("replay", scripted)).toBe("scripted.note");
    expect(modelLabel(scripted)).toEqual({ key: "scripted.label", variant: "purple" });
  });

  it("LIVE META only for a live frame of a live-model run (isLive), META MODEL otherwise", () => {
    expect(modelLabel(liveMeta, "live")).toEqual({ key: "liveMeta.label", variant: "info" });
    // A replayed frame, a list entry (no delivery) and a preset read META MODEL.
    expect(modelLabel(liveMeta, "replay")).toEqual({ key: "metaModel.label", variant: "info" });
    expect(modelLabel(liveMeta)).toEqual({ key: "metaModel.label", variant: "info" });
    expect(modelLabel({ model: "live_meta" })).toEqual({ key: "metaModel.label", variant: "info" });
    // A list never reads "Meta model · live".
    expect(modelNoteKey(undefined, liveMeta)).toBe("liveMeta.recorded");
  });

  it.each(BLOCKING_FLAGS)("a %s run's model badge is a warning, never LIVE META", (flag) => {
    const flagged = { ...liveMeta, flags: [flag] };
    for (const delivery of ["live", "replay", undefined] as const) {
      expect(modelLabel(flagged, delivery)).toEqual({ key: "metaModel.label", variant: "warning" });
    }
  });

  it("LIVE META is paired with an evidence label, never cyan or teal", () => {
    expect(evidenceLabel("hyphae_snapshot").key).toBe("liveMeta.evidence");
    expect(evidenceLabel("simulated").key).toBe("simulated.label");
    for (const label of [
      ...MODEL_PROVENANCES.flatMap((model) =>
        (["live", "replay", undefined] as const).map((delivery) =>
          modelLabel({ model }, delivery),
        ),
      ),
      ...(["simulated", "hyphae_snapshot", "hyphae_live", "none", "mixed"] as const).map(
        evidenceLabel,
      ),
      sourceLabel("this_server"),
      sourceLabel("research_archive"),
    ]) {
      expect(label.variant).not.toBe("cyan");
      expect(label.variant).not.toBe("teal");
    }
  });

  it("an archived run reads RECORDED", () => {
    expect(sourceLabel("research_archive").key).toBe("recorded.label");
  });
});

describe("proofs and outcomes", () => {
  it("a simulated proof is never a green valid", () => {
    expect(proofToken("valid", true)).toEqual({ key: "proof.valid", variant: "purple", simulated: true });
    expect(proofToken("invalid", true).variant).toBe("purple");
    expect(proofToken("valid", false)).toEqual({ key: "proof.valid", variant: "success", simulated: false });
    expect(proofToken("invalid", false).variant).toBe("error");
    expect(proofToken("unavailable", false).variant).toBe("neutral");
  });

  it("every recommendation kind has a variant and blocked is the only error", () => {
    expect(Object.keys(RECOMMENDATION_VARIANTS).sort()).toEqual([
      "abstained",
      "blocked",
      "contested",
      "insufficient_evidence",
      "supported",
    ]);
    expect(RECOMMENDATION_VARIANTS.blocked).toBe("error");
    expect(RECOMMENDATION_VARIANTS.supported).toBe("success");
  });

  it("knows which states are terminal and when a run is at rest", () => {
    expect(isTerminal("approved")).toBe(true);
    expect(isTerminal("failed")).toBe(true);
    expect(isTerminal("awaiting_human_approval")).toBe(false);
    expect(isAtRest("awaiting_human_approval", false)).toBe(true);
    expect(isAtRest("rejected", false)).toBe(true);
    expect(isAtRest("evaluated", false)).toBe(false);
    expect(isAtRest("approved", true)).toBe(false);
  });
});

describe("label keys exist in every catalog", () => {
  type Catalog = { [key: string]: string | Catalog };
  const catalogs = routing.locales.map((locale) => ({
    locale,
    messages: JSON.parse(
      readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
    ) as Catalog,
  }));

  const keys = [
    ...MODEL_PROVENANCES.flatMap((model) =>
      (["live", "replay", undefined] as const).map(
        (delivery) => modelLabel({ model }, delivery).key,
      ),
    ),
    modelLabel({ ...liveMeta, flags: ["provenance_inconsistent"] }, "live").key,
    ...(["simulated", "hyphae_snapshot", "hyphae_live", "none", "mixed"] as const).map(
      (e) => evidenceLabel(e).key,
    ),
    sourceLabel("this_server").key,
    sourceLabel("research_archive").key,
    ...BLOCKING_FLAGS.map((f) => flagLabel(f).key),
    deliveryLabel("live", liveMeta).key,
    deliveryLabel("live", scripted).key,
    deliveryLabel("replay", liveMeta).key,
    "liveMeta.recorded",
    "liveMeta.live",
    "scripted.note",
    ...(["valid", "invalid", "unavailable", "verification_error", "not_checked"] as const).map(
      (s) => proofToken(s, false).key,
    ),
  ];

  it.each(catalogs)("$locale", ({ messages }) => {
    const provenance = messages.Provenance as Catalog;
    for (const key of keys) {
      let node: string | Catalog | undefined = provenance;
      for (const part of key.split(".")) {
        node = typeof node === "object" ? node[part] : undefined;
      }
      expect(typeof node, `Provenance.${key}`).toBe("string");
    }
  });
});

describe("no LIVE word unless isLive, in every locale", () => {
  type Catalog = { [key: string]: string | Catalog };
  const read = (locale: string) =>
    JSON.parse(
      readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
    ) as Catalog;
  const lookup = (catalog: Catalog, key: string): string => {
    let node: string | Catalog | undefined = catalog;
    for (const part of key.split(".")) {
      node = typeof node === "object" ? node[part] : undefined;
    }
    if (typeof node !== "string") {
      throw new Error(`no message ${key}`);
    }
    return node;
  };

  const flagSets: ProvenanceV1["flags"][] = [[], ...BLOCKING_FLAGS.map((flag) => [flag])];
  const deliveries = ["live", "replay", undefined] as const;

  it.each(routing.locales)("%s", (locale) => {
    const provenance = read(locale).Provenance as Catalog;
    // The locale's own LIVE word: LIVE, EN VIVO, AO VIVO, EN DIRECT.
    const liveWord = lookup(provenance, "live.label").toLocaleUpperCase(locale);
    const containsLiveWord = (text: string) =>
      text.toLocaleUpperCase(locale).includes(liveWord);
    let checked = 0;
    for (const model of MODEL_PROVENANCES) {
      for (const flags of flagSets) {
        for (const delivery of deliveries) {
          const p = { model, flags };
          const live = delivery !== undefined && isLive(delivery, p);
          const texts = [lookup(provenance, modelLabel(p, delivery).key)];
          if (delivery !== undefined) {
            texts.push(lookup(provenance, deliveryLabel(delivery, p).key));
          }
          const note = modelNoteKey(delivery, p);
          if (note !== null && note !== "scripted.note") {
            texts.push(lookup(provenance, note));
          }
          for (const text of texts) {
            // A label or note with the LIVE word appears exactly when isLive holds.
            expect(
              containsLiveWord(text),
              `${model} ${JSON.stringify(flags)} ${String(delivery)}: ${text}`,
            ).toBe(live);
          }
          checked += texts.length;
        }
      }
    }
    expect(checked).toBeGreaterThan(40);
    expect(containsLiveWord(lookup(provenance, "liveMeta.label"))).toBe(true);
    expect(containsLiveWord(lookup(provenance, "metaModel.label"))).toBe(false);
  });
});

describe("pacing is for scripted runs only", () => {
  it("a scripted run, or one declared scripted before any model call, is paced when replayed", () => {
    expect(isScriptedRun(scripted)).toBe(true);
    const created = {
      model: "undetermined",
      declared: { model: "scripted", evidence: "simulated", origin: "public", preset: "guided:retry" },
    } as const;
    expect(isScriptedRun(created)).toBe(true);
    expect(isPaceable("replay", scripted)).toBe(true);
    expect(isPaceable("replay", created)).toBe(true);
    // Live frames are never delayed.
    expect(isPaceable("live", scripted)).toBe(false);
  });

  it("a live, mixed or live-declared run is never paced or labelled simulated", () => {
    expect(isPaceable("replay", liveMeta)).toBe(false);
    expect(isPaceable("replay", { ...liveMeta, model: "mixed" })).toBe(false);
    const liveDeclared = {
      model: "undetermined",
      declared: { model: "meta", evidence: "sealed_snapshot", origin: "public", preset: "R01-reference" },
    } as const;
    expect(isScriptedRun(liveDeclared)).toBe(false);
    expect(isPaceable("replay", liveDeclared)).toBe(false);
    expect(isScriptedRun({ model: "undetermined" })).toBe(false);
  });
});

describe("the verbatim log link follows the server's publication rule", () => {
  const detail = (name: string) =>
    JSON.parse(
      readFileSync(path.join(contractDir(), "fixtures", name), "utf8"),
    ) as RunDetailV1;

  it.each([
    "run-detail-retry.json",
    "run-detail-offline.json",
    "run-detail-invalid_proof.json",
    "run-detail-injection.json",
    "run-detail-abstain.json",
  ])("%s (guided) is published", (name) => {
    expect(ndjsonPublishable(detail(name).summary.provenance)).toBe(true);
  });

  it("a live-declared run (R01) is not, even before any model call", () => {
    const live = detail("run-detail-live-R01-reference.json").summary.provenance;
    expect(live.model).toBe("undetermined");
    expect(ndjsonPublishable(live)).toBe(false);
  });

  it("nor is a run whose records prove a live model or real evidence, or one without a declared mode", () => {
    const guided = detail("run-detail-retry.json").summary.provenance;
    expect(ndjsonPublishable({ ...guided, model: "live_meta" })).toBe(false);
    expect(ndjsonPublishable({ ...guided, model: "mixed" })).toBe(false);
    expect(ndjsonPublishable({ ...guided, evidence: "hyphae_snapshot" })).toBe(false);
    expect(ndjsonPublishable({ ...guided, declared: null })).toBe(false);
    expect(ndjsonPublishable({ ...guided, declared: undefined })).toBe(false);
  });
});
