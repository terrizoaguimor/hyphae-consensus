// SPDX-License-Identifier: Apache-2.0
//
// The honesty predicates of release gate G7: when a frame may read LIVE, how
// replayed live-model records and scripted doubles read, what a simulated
// proof renders as, and that every label key exists in every catalog.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { routing } from "../src/i18n/routing";
import type { ProvenanceV1, RunDetailV1 } from "../src/lib/api/types";
import {
  assessmentVariant,
  BLOCKING_FLAGS,
  deliveryLabel,
  evidenceLabel,
  flagLabel,
  hasBlockingFlag,
  INFO_VARIANT,
  isAtRest,
  isLive,
  isPaceable,
  isScriptedRun,
  isTerminal,
  LIVE_VARIANT,
  MODEL_PROVENANCES,
  modelLabel,
  modelNoteKey,
  ndjsonPublishable,
  proofToken,
  RECOMMENDATION_VARIANTS,
  sourceLabel,
} from "../src/lib/provenance";

import { IO_BOUND_TEST_TIMEOUT_MS } from "./io-timeout";

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
    // LIVE has its own colour (LIVE_VARIANT), never the green of a verified proof.
    expect(deliveryLabel("live", liveMeta)).toEqual({ key: "live.label", variant: "pink" });
    expect(LIVE_VARIANT).toBe("pink");
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
    // LIVE META in the LIVE colour; META MODEL in the info colour, never the same.
    expect(modelLabel(liveMeta, "live")).toEqual({ key: "liveMeta.label", variant: "pink" });
    // A replayed frame, a list entry (no delivery) and a preset read META MODEL.
    expect(modelLabel(liveMeta, "replay")).toEqual({ key: "metaModel.label", variant: "info" });
    expect(modelLabel(liveMeta)).toEqual({ key: "metaModel.label", variant: "info" });
    expect(modelLabel({ model: "live_meta" })).toEqual({ key: "metaModel.label", variant: "info" });
    // A list never reads "Meta model · live".
    expect(modelNoteKey(undefined, liveMeta)).toBe("liveMeta.recorded");
  });

  it.each(BLOCKING_FLAGS)(
    "a %s run's model badge is META MODEL in the info colour, never LIVE META; the flag is its own warning badge",
    (flag) => {
      const flagged = { ...liveMeta, flags: [flag] };
      for (const delivery of ["live", "replay", undefined] as const) {
        expect(modelLabel(flagged, delivery)).toEqual({ key: "metaModel.label", variant: "info" });
      }
      expect(flagLabel(flag).variant).toBe("warning");
    },
  );

  it("META MODEL has one colour, the info one, in every state (HE #16)", () => {
    const flagSets: ProvenanceV1["flags"][] = [undefined, [], ...BLOCKING_FLAGS.map((flag) => [flag])];
    let seen = 0;
    for (const flags of flagSets) {
      for (const delivery of ["live", "replay", undefined] as const) {
        const label = modelLabel({ model: "live_meta", flags }, delivery);
        if (label.key === "metaModel.label") {
          expect(label.variant, `${JSON.stringify(flags)} ${String(delivery)}`).toBe(INFO_VARIANT);
          seen += 1;
        }
      }
    }
    expect(seen).toBeGreaterThan(10);
    expect(INFO_VARIANT).toBe("info");
  });

  it("the LIVE colour is used by the LIVE labels and by nothing else (HE #16)", () => {
    const flagSets: ProvenanceV1["flags"][] = [[], ...BLOCKING_FLAGS.map((flag) => [flag])];
    const evidences = ["simulated", "hyphae_snapshot", "hyphae_live", "none", "mixed"] as const;
    const statuses = ["valid", "invalid", "unavailable", "verification_error", "not_checked"] as const;
    const labels: Array<{ key: string; variant: string }> = [
      ...MODEL_PROVENANCES.flatMap((model) =>
        flagSets.flatMap((flags) =>
          (["live", "replay", undefined] as const).flatMap((delivery) => [
            modelLabel({ model, flags }, delivery),
            ...(delivery === undefined ? [] : [deliveryLabel(delivery, { model, flags })]),
          ]),
        ),
      ),
      ...evidences.map(evidenceLabel),
      sourceLabel("this_server"),
      sourceLabel("research_archive"),
      ...BLOCKING_FLAGS.map(flagLabel),
      ...statuses.flatMap((s) => [proofToken(s, true), proofToken(s, false)]),
      ...Object.entries(RECOMMENDATION_VARIANTS).map(([kind, variant]) => ({ key: `kinds.${kind}`, variant })),
      { key: "corroborated", variant: assessmentVariant(true) },
      { key: "notCorroborated", variant: assessmentVariant(false) },
    ];
    const live = labels.filter((label) => label.variant === LIVE_VARIANT);
    expect(live.length).toBeGreaterThan(0);
    expect(new Set(live.map((label) => label.key))).toEqual(new Set(["live.label", "liveMeta.label"]));
    for (const label of labels.filter((l) => l.key === "live.label" || l.key === "liveMeta.label")) {
      expect(label.variant).toBe(LIVE_VARIANT);
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
    // "supported" is the info role: never green, whatever the evidence (HE #1).
    expect(RECOMMENDATION_VARIANTS.supported).toBe("info");
  });

  it("no outcome on SIMULATED (or any) evidence maps to the success tone (HE #1)", () => {
    for (const [kind, variant] of Object.entries(RECOMMENDATION_VARIANTS)) {
      expect(variant, kind).not.toBe("success");
      expect(variant, kind).not.toBe("green");
    }
    expect(RECOMMENDATION_VARIANTS.supported).toBe(INFO_VARIANT);
  });

  it("a corroborated assessment is the info role, never green (HE #1)", () => {
    expect(assessmentVariant(true)).toBe("info");
    expect(assessmentVariant(false)).toBe("neutral");
    for (const corroborated of [true, false]) {
      expect(assessmentVariant(corroborated)).not.toBe("success");
      expect(assessmentVariant(corroborated)).not.toBe("green");
    }
  });

  it("the success tone belongs to a real verifier's accepted proof alone", () => {
    const statuses = ["valid", "invalid", "unavailable", "verification_error", "not_checked"] as const;
    const green = statuses.flatMap((status) =>
      [true, false]
        .map((simulated) => ({ status, simulated, token: proofToken(status, simulated) }))
        .filter(({ token }) => token.variant === "success"),
    );
    expect(green.map(({ status, simulated }) => ({ status, simulated }))).toEqual([
      { status: "valid", simulated: false },
    ]);
  });

  it("no view hands a literal success or green variant to an outcome or an assessment", () => {
    // The views take these colours from src/lib/provenance.ts; a literal
    // "success" may appear only where a real check is shown (the in-browser
    // chain check) or in non-badge code (API results).
    const dir = new URL("../src/components/", import.meta.url);
    const offenders: string[] = [];
    const walk = (url: URL) => {
      for (const entry of readdirSync(url, { withFileTypes: true })) {
        const child = new URL(entry.isDirectory() ? `${entry.name}/` : entry.name, url);
        if (entry.isDirectory()) {
          walk(child);
        } else if (entry.name.endsWith(".tsx") || entry.name.endsWith(".ts")) {
          const text = readFileSync(child, "utf8");
          // variant="success", a conditional `? "success"`, a map value `kind: "success",`.
          const literal = [
            /variant[=:]\s*\{?\s*["'](success|green)["']/,
            /[?:]\s*["'](success|green)["']\s*[,}:)]/,
          ];
          if (literal.some((pattern) => pattern.test(text))) {
            offenders.push(path.basename(child.pathname));
          }
        }
      }
    };
    walk(dir);
    expect(offenders.sort()).toEqual(["ChainCheck.tsx"]);
    // Reads every file of src/components/ through the bind mount: see
    // tests/io-timeout.ts.
  }, IO_BOUND_TEST_TIMEOUT_MS);

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
