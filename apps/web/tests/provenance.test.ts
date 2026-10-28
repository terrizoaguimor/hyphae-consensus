// SPDX-License-Identifier: Apache-2.0
//
// The honesty predicates of release gate G7: when a frame may read LIVE, how
// replayed live-model records and scripted doubles read, what a simulated
// proof renders as, and that every label key exists in every catalog.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import type { ProvenanceV1 } from "../src/lib/api/types";
import {
  BLOCKING_FLAGS,
  deliveryLabel,
  evidenceLabel,
  flagLabel,
  hasBlockingFlag,
  isAtRest,
  isLive,
  isTerminal,
  modelLabel,
  modelNoteKey,
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
    expect(modelLabel("scripted")).toEqual({ key: "scripted.label", variant: "purple" });
  });

  it("LIVE META is paired with an evidence label, never cyan or teal", () => {
    expect(modelLabel("live_meta")).toEqual({ key: "liveMeta.label", variant: "info" });
    expect(evidenceLabel("hyphae_snapshot").key).toBe("liveMeta.evidence");
    expect(evidenceLabel("simulated").key).toBe("simulated.label");
    for (const label of [
      ...(["scripted", "live_meta", "mixed", "undetermined"] as const).map(modelLabel),
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
    ...(["scripted", "live_meta", "mixed", "undetermined"] as const).map((m) => modelLabel(m).key),
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
