// SPDX-License-Identifier: Apache-2.0
//
// A failed run is explained from its failure CODE (src/lib/run/failure.ts),
// in every locale, never by the server's English message: every code
// `public_view::failure_message` knows and the live ledger's
// `budget_refused` / `budget_unsettled` have a sentence; any other code gets
// the generic sentence, which still names the code; no code names the stage.
// The same for the codes of an evidence_unavailable record.

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import {
  againPath,
  FAILURE_CLASSES,
  FAILURE_CODES,
  FAILURE_STAGES,
  failureClass,
  failurePanel,
  failureSentence,
  ROLE_CODES,
  stageNameKey,
  stageRole,
  UNAVAILABLE_CODES,
  unavailableSentence,
  visitorRunKind,
} from "../src/lib/run/failure";

// crates/consensus-api/src/public_view.rs failure_message (each `Some("…")`
// arm), and the live ledger's codes (crates/consensus-api/src/meta.rs codes).
// The Rust sources are not mounted in the test container, so the list is
// copied here; FAILURE_CODES must cover it.
const SERVER_FAILURE_CODES = [
  "cancelled",
  "shutdown",
  "deadline",
  "interrupted",
  "panic",
  "model_timeout",
  "model_unavailable",
  "model_malformed_output",
  "evaluation_contract",
  "challenge_contract",
  "proposal_contract",
  "token_budget_exceeded",
  "token_usage_missing",
  "persistence",
];
const LEDGER_FAILURE_CODES = ["budget_refused", "budget_unsettled"];

type Catalog = { [key: string]: string | Catalog };

function runCatalog(locale: string): Catalog {
  const catalog = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  return catalog.Run as Catalog;
}

function lookup(catalog: Catalog, key: string): string {
  let node: string | Catalog | undefined = catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key}`);
  }
  return node;
}

describe("failureSentence", () => {
  it("covers every code the server's failure_message knows and the ledger's", () => {
    for (const code of [...SERVER_FAILURE_CODES, ...LEDGER_FAILURE_CODES]) {
      expect(FAILURE_CODES as readonly string[], code).toContain(code);
      expect(failureSentence({ code, stage: "evaluate" })).toEqual({
        key: `codes.${code}`,
        values: {},
      });
    }
  });

  it("names an unknown code, and the stage when there is no code", () => {
    expect(failureSentence({ code: "a_new_code", stage: "critique" })).toEqual({
      key: "unknownCode",
      values: { code: "a_new_code" },
    });
    expect(failureSentence({ code: null, stage: "critique" })).toEqual({
      key: "noCode",
      values: { stage: "critique" },
    });
    expect(failureSentence({ stage: "propose" }).key).toBe("noCode");
  });

  it("reads evidence_unavailable codes the same way", () => {
    expect(unavailableSentence("memory_unavailable").key).toBe("codes.memory_unavailable");
    expect(unavailableSentence("foreign_record")).toEqual({
      key: "unknownCode",
      values: { code: "foreign_record" },
    });
    expect(unavailableSentence(undefined).key).toBe("noCode");
  });
});

describe.each(routing.locales)("%s failure sentences", (locale) => {
  const run = runCatalog(locale);

  it("has a sentence for every code", () => {
    for (const code of FAILURE_CODES) {
      expect(lookup(run, `failure.codes.${code}`).trim(), code).not.toBe("");
    }
    expect(Object.keys((run.failure as Catalog).codes as Catalog).sort()).toEqual(
      [...FAILURE_CODES].sort(),
    );
    for (const code of UNAVAILABLE_CODES) {
      expect(lookup(run, `unavailable.codes.${code}`).trim(), code).not.toBe("");
    }
  });

  it("the generic sentences still show the code or the stage", () => {
    const format = (key: string, values: Record<string, string>) =>
      String(new IntlMessageFormat(lookup(run, key), locale).format(values));
    expect(format("failure.unknownCode", { code: "x_code_1" })).toContain("x_code_1");
    expect(format("failure.noCode", { stage: "x_stage_1" })).toContain("x_stage_1");
    expect(format("failure.detail", { stage: "x_stage_1", code: "x_code_1" })).toContain("x_code_1");
    expect(format("unavailable.unknownCode", { code: "x_code_2" })).toContain("x_code_2");
  });
});

// The codes lanes A and B of round 8 add (crates/consensus-api/src/meta.rs,
// the live gate's GIVE_BACK_CODES): each has a sentence of its own.
const ROUND_8_CODES = [
  "model_no_claims",
  "model_content_filtered",
  "model_unexpected_finish",
  "adapter_schema_error",
  "meta_bad_envelope",
  "run_deadline_reached",
];

// crates/consensus-api/src/live/gate.rs GIVE_BACK_CODES: a run that ends with
// one of them gives its daily count back. The web's transient class is the
// same list (the panel says such a failure is not counted).
const GIVE_BACK_CODES = [
  "meta_timeout",
  "meta_unavailable",
  "meta_transport",
  "meta_rate_limited",
  "meta_bad_envelope",
  "model_timeout",
  "model_unavailable",
  "model_malformed_output",
  "model_no_claims",
  "model_output_truncated",
  "model_refusal",
  "model_content_filtered",
  "model_unexpected_finish",
  "interrupted",
  "shutdown",
  "run_deadline_reached",
];

/** The ICU argument names of a message, sorted. */
function argumentsOf(text: string): string[] {
  const names = new Set<string>();
  for (const match of text.matchAll(/\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*[,}]/g)) {
    names.add(match[1] ?? "");
  }
  return [...names].sort();
}

describe("the failure panel's classes, stages and roles", () => {
  it("knows the round 8 codes", () => {
    for (const code of ROUND_8_CODES) {
      expect(FAILURE_CODES as readonly string[], code).toContain(code);
    }
  });

  it("puts every code in exactly one class", () => {
    for (const code of FAILURE_CODES) {
      const classes = Object.entries(FAILURE_CLASSES).filter(([, codes]) =>
        (codes as readonly string[]).includes(code),
      );
      expect(classes.map(([name]) => name), code).toHaveLength(1);
    }
    const classified = Object.values(FAILURE_CLASSES).flat();
    expect([...classified].sort()).toEqual([...FAILURE_CODES].sort());
    expect(failureClass("a_new_code")).toBeNull();
    expect(failureClass(null)).toBeNull();
  });

  it("calls transient exactly the codes the live gate gives the daily count back for", () => {
    expect([...FAILURE_CLASSES.transient].sort()).toEqual([...GIVE_BACK_CODES].sort());
    // Never the visitor's own act, the deadline, a protocol refusal or the budget.
    for (const code of ["cancelled", "deadline", "evaluation_contract", "budget_refused", "token_budget_exceeded"]) {
      expect(FAILURE_CLASSES.transient as readonly string[], code).not.toContain(code);
    }
    expect(FAILURE_CLASSES.protocol.every((code) => code.endsWith("_contract"))).toBe(true);
    expect(FAILURE_CODES.filter((code) => code.endsWith("_contract")).sort()).toEqual(
      [...FAILURE_CLASSES.protocol].sort(),
    );
  });

  it("maps each stage to the role whose call it is", () => {
    expect(stageRole("propose")).toBe("proposer");
    expect(stageRole("revise")).toBe("proposer");
    expect(stageRole("critique")).toBe("critic");
    expect(stageRole("evaluate")).toBe("evaluator");
    for (const stage of ["token_budget", "deadline", "shutdown", "panic", "interrupted", "x"]) {
      expect(stageRole(stage), stage).toBe("runtime");
    }
    expect(stageNameKey("propose")).toBe("stageNames.propose");
    expect(stageNameKey("panic")).toBeNull();
  });

  it("names the role for a refused model document, and the stage, never for a cancellation", () => {
    const malformed = failurePanel({ stage: "propose", code: "model_malformed_output" }, true);
    expect(malformed.sentence).toEqual({ key: "roleCodes.model_malformed_output", values: { role: "proposer" } });
    expect(malformed.where).toEqual({ stageName: "stageNames.propose", stage: "propose", role: "proposer" });
    expect(malformed.next).toBe("transient");
    expect(malformed.notCounted).toBe(true);
    // The same failure on a guided run counts nothing toward a live quota.
    expect(failurePanel({ stage: "propose", code: "model_malformed_output" }, false).notCounted).toBe(false);
    // A role-free code keeps its list sentence; an unknown stage falls back to its raw name.
    const panic = failurePanel({ stage: "panic", code: "panic" }, true);
    expect(panic.sentence).toEqual({ key: "codes.panic", values: {} });
    expect(panic.where).toEqual({ stageName: null, stage: "panic", role: "runtime" });
    expect(panic.next).toBe("closed");
    expect(panic.notCounted).toBe(false);
    const cancelled = failurePanel({ stage: "cancelled", code: "cancelled" }, true);
    expect(cancelled.where).toBeNull();
    expect(cancelled.next).toBe("cancelled");
    expect(failurePanel({ stage: "critique", code: "challenge_contract" }, true).next).toBe("protocol");
    expect(failurePanel({ stage: "critique", code: "a_new_code" }, true).next).toBeNull();
  });

  it("links a failed live run to the live form with its preset, and a guided one to the guided examples", () => {
    const live = {
      source: "this_server" as const,
      declared: { model: "meta" as const, evidence: "sealed_snapshot" as const, origin: "public" as const, preset: "R01-reference" },
    };
    expect(visitorRunKind(live)).toBe("live");
    expect(againPath("/en/new/", live)).toEqual({ kind: "live", href: "/en/new/?preset=R01-reference#live-heading" });
    const guided = {
      source: "this_server" as const,
      declared: { model: "scripted" as const, evidence: "simulated" as const, origin: "public" as const },
    };
    expect(againPath("/fr/new/", guided)).toEqual({ kind: "guided", href: "/fr/new/#guided-heading" });
    // Neither an operator run nor a recorded run nor a run that declares nothing.
    expect(againPath("/en/new/", { ...live, declared: { ...live.declared, origin: "operator" as const } })).toBeNull();
    expect(againPath("/en/new/", { ...live, source: "research_archive" as const })).toBeNull();
    expect(againPath("/en/new/", { source: "this_server" as const, declared: null })).toBeNull();
  });
});

describe.each(routing.locales)("%s failure panel copy", (locale) => {
  const run = runCatalog(locale);
  const english = runCatalog("en");

  it("names every stage", () => {
    for (const stage of FAILURE_STAGES) {
      expect(lookup(run, `failure.stageNames.${stage}`).trim(), stage).not.toBe("");
    }
    expect(Object.keys((run.failure as Catalog).stageNames as Catalog).sort()).toEqual([...FAILURE_STAGES].sort());
  });

  it("has the panel's sentences, with the English placeholders", () => {
    const keys = [
      "failure.where",
      "failure.nothingDecided",
      "failure.technical",
      "failure.next.transient",
      "failure.next.protocol",
      "failure.next.closed",
      "failure.next.cancelled",
      "failure.next.deadline",
      "failure.quota.notCounted",
      "failure.again.live",
      "failure.again.guided",
      "failure.again.note",
      "failure.again.noteShort",
      "failure.subjects.proposer",
      "failure.subjects.critic",
      "failure.subjects.evaluator",
      "failure.subjects.runtime",
      "stage.failedHere",
      "stage.notReached",
      "meta.noCostLive",
      "restartHint",
      ...ROLE_CODES.map((code) => `failure.roleCodes.${code}`),
      ...ROUND_8_CODES.map((code) => `failure.codes.${code}`),
    ];
    for (const key of keys) {
      const text = lookup(run, key);
      expect(text.trim(), key).not.toBe("");
      expect(argumentsOf(text), key).toEqual(argumentsOf(lookup(english, key)));
    }
    expect(argumentsOf(lookup(run, "failure.where"))).toEqual(["role", "stageName"]);
    for (const code of ROLE_CODES) {
      expect(argumentsOf(lookup(run, `failure.roleCodes.${code}`)), code).toEqual(["role"]);
    }
  });

  it("reads the first live failure as a sentence about the Proposer, at the proposal step", () => {
    const format = (key: string, values: Record<string, string | number> = {}) =>
      String(new IntlMessageFormat(lookup(run, key), locale).format(values));
    const subject = lookup(run, "failure.subjects.proposer");
    const sentence = format("failure.roleCodes.model_malformed_output", { role: subject });
    expect(sentence.startsWith(subject)).toBe(true);
    const where = format("failure.where", {
      stageName: lookup(run, "failure.stageNames.propose"),
      role: lookup(run, "stage.proposer"),
    });
    expect(where).toContain(lookup(run, "failure.stageNames.propose"));
    expect(where).toContain(lookup(run, "stage.proposer"));
    expect(where).not.toContain("propose");
    expect(format("failure.again.note", { hours: 24 })).toContain("24");
  });
});
