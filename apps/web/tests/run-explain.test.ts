// SPDX-License-Identifier: Apache-2.0
//
// The explanations of a recommendation (src/lib/run/explain.ts) rendered
// with the real catalogs in the four locales: the plain text of a policy
// trace mentions exactly the trace's counts (every number in it is one of
// them, zeros said in words) and exactly its kind (its translated word, and
// no other kind's), says one sentence per failed check and none for a check
// that did not fail; every computed key exists in every catalog; and what
// the trace does not carry (SIMULATED proofs, the final-round reading of a
// run before protocol 1.3, the objections never judged again) comes from the
// run's records.

import { readFileSync } from "node:fs";
import path from "node:path";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { routing } from "../src/i18n/routing";
import type {
  PolicyRule,
  PolicyTrace,
  RecommendationKind,
  RunDetailV1,
  TraceReason,
} from "../src/lib/api/types";
import {
  contextMessages,
  type ExplainMessage,
  explainContext,
  plainExplanation,
  POLICY_RULES,
  reasonIds,
  TRACE_REASONS,
  traceReading,
  ungeneratedProofs,
} from "../src/lib/run/explain";

import { manifest, researchBytes } from "./research";
import { recordedDetail, runRecords } from "./research-detail";

type Catalog = { [key: string]: string | Catalog };

function fixture<T>(name: string): T {
  return JSON.parse(readFileSync(path.join(contractDir(), "fixtures", name), "utf8")) as T;
}

const catalogs = new Map(
  routing.locales.map((locale) => [
    locale,
    JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8")) as Catalog,
  ]),
);

function message(locale: string, key: string): string {
  let node: string | Catalog | undefined = catalogs.get(locale as (typeof routing.locales)[number]);
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key} in ${locale}`);
  }
  return node;
}

function render(locale: string, entry: ExplainMessage): string {
  const values: Record<string, string | number> = { ...entry.values };
  if (entry.kind !== undefined) {
    values.kind = message(locale, `Run.kinds.${entry.kind}`);
  }
  return String(new IntlMessageFormat(message(locale, `Run.explain.${entry.key}`), locale).format(values));
}

const KINDS: RecommendationKind[] = ["supported", "contested", "insufficient_evidence", "abstained", "blocked"];

const KIND_OF_RULE: Record<PolicyRule, RecommendationKind> = {
  invalid_proof: "blocked",
  recorded_contradiction: "contested",
  abstention: "abstained",
  unresolved_items: "contested",
  insufficient_evidence: "insufficient_evidence",
  all_checks_passed: "supported",
};

const SAMPLE_REASONS: Record<TraceReason["reason"], TraceReason> = {
  invalid_proofs: { reason: "invalid_proofs", evidence_ids: ["ev-1"] },
  contradicted_evidence: { reason: "contradicted_evidence", evidence_ids: ["ev-2"] },
  evaluator_abstained: { reason: "evaluator_abstained", abstain_reason: "not enough to go on" },
  open_items: { reason: "open_items", item_ids: ["item-7", "item-7"] },
  no_assessments: { reason: "no_assessments" },
  uncorroborated_claims: { reason: "uncorroborated_claims", claim_ids: ["claim-3"] },
  claims_without_own_evidence: { reason: "claims_without_own_evidence", claim_ids: ["claim-4"] },
  cited_evidence_not_valid: { reason: "cited_evidence_not_valid", evidence_ids: ["ev-5"] },
  no_checked_evidence: { reason: "no_checked_evidence" },
};

/** The traces of the guided fixtures, and one per rule with every count distinct. */
function traces(): PolicyTrace[] {
  const fromFixtures = ["retry", "offline", "invalid_proof", "injection", "abstain", "retry-operator"].map(
    (name) => {
      const trace = fixture<RunDetailV1>(`run-detail-${name}.json`).recommendation?.trace;
      if (trace === undefined || trace === null) {
        throw new Error(`no trace in run-detail-${name}.json`);
      }
      return trace;
    },
  );
  const synthetic = POLICY_RULES.map((rule, index): PolicyTrace => {
    const reasons =
      rule === "all_checks_passed"
        ? []
        : TRACE_REASONS.slice(index).map((reason) => SAMPLE_REASONS[reason]);
    return {
      rule,
      kind: KIND_OF_RULE[rule],
      version: index % 2 === 0 ? "1.2" : "1.1",
      reasons,
      counts: {
        claims: 7 + index,
        corroborated: 3,
        proofs_valid: 11,
        proofs_invalid: rule === "invalid_proof" ? 2 : 0,
        proofs_unavailable_or_error: 5,
        unresolved_items: rule === "unresolved_items" ? 4 : 0,
      },
    };
  });
  return [...fromFixtures, ...synthetic];
}

function plainText(locale: string, trace: PolicyTrace, proofsUngenerated = false): string {
  const plain = plainExplanation(trace, { proofsUngenerated });
  return [plain.lead, ...plain.counts, ...plain.reasons].map((entry) => render(locale, entry)).join(" ");
}

const sortedNumbers = (values: number[]) => [...values].sort((a, b) => a - b);

describe.each(routing.locales)("the plain explanation (%s)", (locale) => {
  it("mentions exactly the trace's counts: every number in it, zeros in words", () => {
    for (const trace of traces()) {
      const text = plainText(locale, trace);
      const numbers = [...text.matchAll(/\d+/g)].map((match) => Number(match[0]));
      const counts = Object.values(trace.counts).filter((value) => value > 0);
      expect(sortedNumbers(numbers), `${trace.rule}: ${text}`).toEqual(sortedNumbers(counts));
    }
  });

  it("names exactly the trace's kind, and no other kind", () => {
    for (const trace of traces()) {
      const text = plainText(locale, trace).toLocaleLowerCase(locale);
      const own = message(locale, `Run.kinds.${trace.kind}`).toLocaleLowerCase(locale);
      expect(text, `${trace.rule}: ${text}`).toContain(own);
      for (const kind of KINDS.filter((k) => k !== trace.kind)) {
        const word = message(locale, `Run.kinds.${kind}`).toLocaleLowerCase(locale);
        if (own.includes(word)) {
          continue;
        }
        expect(text, `${trace.rule} mentions ${kind}: ${text}`).not.toContain(word);
      }
    }
  });

  it("says one sentence per failed check and none for a check that did not fail", () => {
    for (const trace of traces()) {
      const text = plainText(locale, trace);
      const failed = new Set(trace.reasons.map((r) => r.reason));
      for (const reason of TRACE_REASONS) {
        const sentence = render(locale, { key: `plain.reasons.${reason}` });
        // The lead of the abstention rule and its reason may say the same thing.
        if (!failed.has(reason) && text.includes(sentence)) {
          throw new Error(`${trace.rule}: ${reason} is said but did not fail: ${text}`);
        }
        if (failed.has(reason)) {
          expect(text, `${trace.rule}: ${reason}`).toContain(sentence);
        }
      }
    }
  });

  it("with no proof generated upstream, says nothing was checked: the same counts, the same kind", () => {
    const all = traces();
    const blocking = all.filter((trace) => trace.reasons.some((r) => r.reason === "invalid_proofs"));
    expect(blocking.length).toBeGreaterThan(0);
    const generic = render(locale, { key: "plain.reasons.invalid_proofs" });
    const genericLead = render(locale, { key: "plain.rules.invalid_proof", kind: "blocked" });
    for (const trace of blocking) {
      const plain = plainExplanation(trace, { proofsUngenerated: true });
      expect(plain.lead.key).toBe("plain.rules.invalid_proof_ungenerated");
      const keys = plain.reasons.map((r) => r.key);
      expect(keys).toContain("plain.reasons.invalid_proofs_ungenerated");
      expect(keys).not.toContain("plain.reasons.invalid_proofs");
      // Every other failed check keeps its own sentence, in the trace's order.
      expect(keys).toEqual(
        trace.reasons.map((r) =>
          r.reason === "invalid_proofs" ? "plain.reasons.invalid_proofs_ungenerated" : `plain.reasons.${r.reason}`,
        ),
      );
      const text = plainText(locale, trace, true);
      const numbers = [...text.matchAll(/\d+/g)].map((match) => Number(match[0]));
      const counts = Object.values(trace.counts).filter((value) => value > 0);
      expect(sortedNumbers(numbers), text).toEqual(sortedNumbers(counts));
      const lower = text.toLocaleLowerCase(locale);
      const own = message(locale, `Run.kinds.${trace.kind}`).toLocaleLowerCase(locale);
      expect(lower).toContain(own);
      for (const kind of KINDS.filter((k) => k !== trace.kind)) {
        expect(lower, `mentions ${kind}: ${text}`).not.toContain(
          message(locale, `Run.kinds.${kind}`).toLocaleLowerCase(locale),
        );
      }
      // Never the sentences of a check that came back invalid.
      expect(text).not.toContain(generic);
      expect(text).not.toContain(genericLead);
      // The proofs count reads the same trace counts as integrity statuses:
      // never proofs found invalid, never "none ... failed".
      const proofs = plain.counts.map((c) => c.key).filter((key) => key.startsWith("plain.counts.proofs"));
      expect(proofs).toEqual(["plain.counts.proofs_ungenerated"]);
      const values = {
        valid: trace.counts.proofs_valid,
        invalid: trace.counts.proofs_invalid,
        unavailable: trace.counts.proofs_unavailable_or_error,
      };
      expect(plain.counts.find((c) => c.key === "plain.counts.proofs_ungenerated")?.values).toEqual(values);
      expect(text).not.toContain(render(locale, { key: "plain.counts.proofs", values }));
      expect(text).toContain(render(locale, { key: "plain.counts.proofs_ungenerated", values }));
      expect(lower).not.toMatch(/failed|fallida|com falha|en échec/);
      // Without the reading the generic count stays.
      expect(plainExplanation(trace).counts.map((c) => c.key)).toContain("plain.counts.proofs");
    }
    // A trace without an invalid proof is unchanged by the reading.
    for (const trace of all.filter((t) => !t.reasons.some((r) => r.reason === "invalid_proofs"))) {
      expect(plainExplanation(trace, { proofsUngenerated: true })).toEqual(plainExplanation(trace));
    }
  }, 30_000);

  it("has a message for every rule, reason and context key", () => {
    for (const rule of POLICY_RULES) {
      expect(() => message(locale, `Run.explain.plain.rules.${rule}`)).not.toThrow();
      expect(() => message(locale, `Run.explain.ruleNames.${rule}`)).not.toThrow();
    }
    for (const reason of TRACE_REASONS) {
      expect(() => message(locale, `Run.explain.plain.reasons.${reason}`)).not.toThrow();
      expect(() => message(locale, `Run.explain.reasonNames.${reason}`)).not.toThrow();
    }
    for (const key of ["context.simulated", "context.finalRoundOnly", "context.neverRejudged"]) {
      expect(() => message(locale, `Run.explain.${key}`)).not.toThrow();
    }
    for (const key of [
      "plain.rules.invalid_proof_ungenerated",
      "plain.reasons.invalid_proofs_ungenerated",
      "plain.counts.proofs_ungenerated",
      "context.neverRejudgedUnlisted",
    ]) {
      expect(() => message(locale, `Run.explain.${key}`)).not.toThrow();
    }
    expect(() => message(locale, "Run.audit.neverRejudgedUnlisted")).not.toThrow();
  });
});

describe("what the records add to the trace", () => {
  it("a guided run's proofs are SIMULATED; it is a 1.3 run with nothing never re-judged", () => {
    const detail = fixture<RunDetailV1>("run-detail-retry.json");
    const context = explainContext(detail);
    expect(context).toEqual({ simulatedProofs: true, beforeCarryForward: false, neverRejudged: null });
    expect(contextMessages(context).map((m) => m.key)).toEqual(["context.simulated"]);
  });

  it("a run before protocol 1.3 with several rounds says how many earlier objections were never re-judged", () => {
    const detail = JSON.parse(JSON.stringify(fixture<RunDetailV1>("run-detail-retry.json"))) as RunDetailV1;
    for (const event of detail.events) {
      event.schema_version = "1.2";
    }
    detail.challenge_items = detail.challenge_items.map((item) => ({ ...item, status: "never_rejudged" }));
    const context = explainContext(detail);
    expect(context.beforeCarryForward).toBe(true);
    expect(context.neverRejudged).toBe(2);
    const messages = contextMessages(context);
    expect(messages.map((m) => m.key)).toEqual([
      "context.simulated",
      "context.finalRoundOnly",
      "context.neverRejudged",
    ]);
    for (const locale of routing.locales) {
      const text = render(locale, messages[2] as ExplainMessage);
      expect(text, locale).toMatch(/\b2\b/);
    }
    // None never re-judged in a multi-round pre-1.3 run is said too (=0 in words).
    detail.challenge_items = detail.challenge_items.map((item) => ({ ...item, status: "resolved" }));
    expect(explainContext(detail).neverRejudged).toBe(0);
    expect(render("en", { key: "context.neverRejudged", values: { count: 0 } })).toBe(
      "Earlier-round objections never re-judged: none.",
    );
  });

  it("a run with objections even their own round left unlisted says how many, apart from the count", () => {
    const detail = JSON.parse(JSON.stringify(fixture<RunDetailV1>("run-detail-retry.json"))) as RunDetailV1;
    for (const event of detail.events) {
      event.schema_version = "1.2";
    }
    detail.challenge_items = detail.challenge_items.map((item) => ({ ...item, status: "never_rejudged" }));
    // Round 1's evaluation listed item-1 and item-2; it now lists item-1 only.
    const first = detail.rounds.find((r) => r.round === 1)?.evaluation;
    if (first === undefined || first === null) {
      throw new Error("no round-1 evaluation in run-detail-retry.json");
    }
    first.unresolved_items = ["item-1"];
    const context = explainContext(detail);
    expect(context.neverRejudged).toBe(2);
    expect(context.neverRejudgedUnlisted).toBe(1);
    const messages = contextMessages(context);
    expect(messages.map((m) => m.key)).toEqual([
      "context.simulated",
      "context.finalRoundOnly",
      "context.neverRejudged",
      "context.neverRejudgedUnlisted",
    ]);
    expect(render("en", messages[3] as ExplainMessage)).toBe(
      "Of them, 1 was not listed as unresolved even by the evaluation of its own round, which left it out without a stated reason.",
    );
    for (const locale of routing.locales) {
      expect(render(locale, messages[3] as ExplainMessage), locale).toMatch(/\b1\b/);
      expect(render(locale, { key: "context.neverRejudgedUnlisted", values: { count: 3 } }), locale).toMatch(/\b3\b/);
    }
    // A pre-1.1 item (no id) counts as listed when its round listed its challenge's id.
    detail.challenge_items = detail.challenge_items.map((item) => ({ ...item, id: "" }));
    first.unresolved_items = ["chal-1"];
    expect(explainContext(detail).neverRejudgedUnlisted).toBe(0);
  });

  it("names the ids of each failed check, duplicates of open items included", () => {
    expect(reasonIds(SAMPLE_REASONS.open_items)).toEqual(["item-7", "item-7"]);
    expect(reasonIds(SAMPLE_REASONS.no_checked_evidence)).toEqual([]);
    expect(reasonIds(SAMPLE_REASONS.invalid_proofs)).toEqual(["ev-1"]);
  });
});

/**
 * Per archived run: earlier-round objections never judged again, and how
 * many of them their own round's evaluation had listed as unresolved, as
 * the Rust research test pins them from the raw logs
 * (crates/consensus-tests/tests/research_receipts.rs NEVER_REJUDGED).
 */
const NEVER_REJUDGED: Record<string, [number, number]> = {
  C01: [2, 2], C02: [2, 2], C03: [0, 0], C04: [0, 0], C05: [2, 2], C06: [0, 0], C07: [0, 0],
  L01: [0, 0], L02: [0, 0], L03: [0, 0], L04: [0, 0], L05: [0, 0], L06: [0, 0], L07: [0, 0],
  L08: [0, 0], L09: [0, 0], L10: [0, 0], I01: [0, 0], I02: [0, 0], S01: [0, 0], S02: [0, 0],
  S03: [0, 0], P01: [0, 0], P02: [8, 8], R01: [5, 5], R02: [7, 7], R03: [6, 6], R04: [0, 0],
  R05: [0, 0], R06: [7, 7], R07: [7, 7], R08: [8, 8], F01: [13, 13], F02: [6, 6], F03: [5, 5],
  F04: [13, 13], F05: [6, 6], F06: [8, 7], H01: [11, 11], H02: [9, 9], K01: [0, 0], K02: [0, 0],
  K03: [0, 0], K04: [0, 0],
};

const archivedRun = (label: string) => {
  const run = manifest().runs.find((r) => r.label === label);
  if (run === undefined) {
    throw new Error(`no run ${label} in the manifest`);
  }
  return { run, detail: recordedDetail(run.file, run.run_id) };
};

type RawVerification = { evidence_id: string; integrity_status: string };

/**
 * The invalid_proofs check of a recorded run's trace, as consensus-runtime
 * policy.rs `explain` builds it: the distinct evidence ids with an invalid
 * verification among the records after the last evaluation (the
 * aggregate's round_verifications).
 */
function blockedTrace(file: string, runId: string): PolicyTrace {
  const payloads = runRecords(file, runId).map((r) => r.payload as { type: string; verification?: RawVerification });
  let last = -1;
  payloads.forEach((p, index) => {
    if (p.type === "evaluation_submitted") {
      last = index;
    }
  });
  const verifications = payloads
    .slice(last + 1)
    .flatMap((p) => (p.type === "evidence_verified" && p.verification !== undefined ? [p.verification] : []));
  const invalid = verifications.filter((v) => v.integrity_status === "invalid");
  return {
    rule: "invalid_proof",
    kind: "blocked",
    version: "1.1",
    reasons: [{ reason: "invalid_proofs", evidence_ids: [...new Set(invalid.map((v) => v.evidence_id))] }],
    counts: {
      claims: 0,
      corroborated: 0,
      proofs_valid: verifications.filter((v) => v.integrity_status === "valid").length,
      proofs_invalid: invalid.length,
      proofs_unavailable_or_error: 0,
      unresolved_items: 0,
    },
  };
}

describe("over the research archive's own records", () => {
  it("counts the objections never re-judged, and those their own round listed, as the Rust research test pins them", () => {
    const runs = manifest().runs;
    expect(runs.map((r) => r.label).sort()).toEqual(Object.keys(NEVER_REJUDGED).sort());
    for (const run of runs) {
      const context = explainContext(recordedDetail(run.file, run.run_id));
      const never = context.neverRejudged ?? 0;
      const listed = never - (context.neverRejudgedUnlisted ?? 0);
      expect([never, listed], run.label).toEqual(NEVER_REJUDGED[run.label]);
    }
  }, 60_000);

  it("research/README.md states the same never re-judged counts as the records", () => {
    const readme = new TextDecoder().decode(researchBytes("README.md")).replace(/\s+/g, " ");
    const start = readme.indexOf("**Earlier-round objections never re-judged**");
    expect(start).toBeGreaterThan(-1);
    const section = readme.slice(start, readme.indexOf("Runs with a single evaluation have none.", start));
    const stated = new Map<string, number>();
    for (const [, label, count] of section.matchAll(/\b([A-Z]\d\d) (\d+)\b/g)) {
      stated.set(label as string, Number(count));
    }
    const each = /(\d+) in each of the scripted (C\d\d), (C\d\d) and (C\d\d)/.exec(section);
    expect(each).not.toBeNull();
    for (const label of (each ?? []).slice(2)) {
      stated.set(label, Number(each?.[1]));
    }
    const expected = Object.entries(NEVER_REJUDGED)
      .filter(([, [never]]) => never > 0)
      .map(([label, [never]]) => [label, never] as const)
      .sort();
    expect([...stated.entries()].sort()).toEqual(expected);
    // F06 is the one run where a never re-judged item was not listed even by its own round.
    expect(section).toContain("F06 8 (7 of them listed unresolved in their own round)");
  });

  it("R06 reads 7 earlier-round objections never re-judged, each listed unresolved by its own round", () => {
    const { detail } = archivedRun("R06");
    const context = explainContext(detail);
    expect(context.beforeCarryForward).toBe(true);
    expect(context.neverRejudged).toBe(7);
    expect(context.neverRejudgedUnlisted).toBe(0);
    const messages = contextMessages(context);
    expect(messages.map((m) => m.key)).not.toContain("context.neverRejudgedUnlisted");
    expect(render("en", messages.find((m) => m.key === "context.neverRejudged") as ExplainMessage)).toBe(
      "Earlier-round objections never re-judged: 7.",
    );
  });

  it("F06 reads 8 and says the 1 its own round left unlisted: research/README.md's 7 are the others", () => {
    const { detail } = archivedRun("F06");
    const context = explainContext(detail);
    expect(context.neverRejudged).toBe(8);
    expect(context.neverRejudgedUnlisted).toBe(1);
    const messages = contextMessages(context);
    const text = messages.map((m) => render("en", m));
    expect(text).toContain("Earlier-round objections never re-judged: 8.");
    expect(text).toContain(
      "Of them, 1 was not listed as unresolved even by the evaluation of its own round, which left it out without a stated reason.",
    );
    const unlisted = messages.find((m) => m.key === "context.neverRejudgedUnlisted") as ExplainMessage;
    for (const locale of routing.locales) {
      expect(render(locale, unlisted), locale).toMatch(/\b1\b/);
    }
  });

  it("C01, C02 and C05 read 2 each, both listed by their round through their challenge's id (pre-1.1)", () => {
    for (const label of ["C01", "C02", "C05"]) {
      const { run, detail } = archivedRun(label);
      expect(run.schema_version, label).toBe("1.0");
      const context = explainContext(detail);
      expect(context.neverRejudged, label).toBe(2);
      expect(context.neverRejudgedUnlisted, label).toBe(0);
      // Their round-1 evaluation listed one challenge id, which covers both items.
      const first = detail.rounds.find((r) => r.round === 1);
      expect(first?.evaluation?.unresolved_items, label).toEqual([first?.challenge?.id]);
      expect(first?.challenge?.items.length, label).toBe(2);
    }
  });

  it("L01: no proof could be generated, so the explanation says nothing was checked, in the four locales", () => {
    const { run, detail } = archivedRun("L01");
    const trace = blockedTrace(run.file, run.run_id);
    expect(reasonIds(trace.reasons[0] as TraceReason).length).toBeGreaterThan(0);
    expect(ungeneratedProofs(trace, detail)).toBe(true);
    const plain = plainExplanation(trace, traceReading(trace, detail));
    expect(plain.lead.key).toBe("plain.rules.invalid_proof_ungenerated");
    expect(plain.reasons.map((r) => r.key)).toEqual(["plain.reasons.invalid_proofs_ungenerated"]);
    expect(render("en", plain.lead)).toBe(
      "None of the records whose integrity reads invalid got a proof: it could not be generated upstream, so nothing was checked. The policy still treats that status as invalid, and that rule comes before every other one: the policy recommends blocked.",
    );
    const proofsCount = plain.counts.find((c) => c.key.startsWith("plain.counts.proofs"));
    expect(proofsCount?.key).toBe("plain.counts.proofs_ungenerated");
    expect([trace.counts.proofs_valid, trace.counts.proofs_invalid, trace.counts.proofs_unavailable_or_error]).toEqual([
      0, 2, 0,
    ]);
    expect(render("en", proofsCount as ExplainMessage)).toBe(
      "Integrity statuses of the final round: none valid, 2 invalid, with no proof generated, none unavailable or with a verification error.",
    );
    for (const locale of routing.locales) {
      const text = [plain.lead, ...plain.counts, ...plain.reasons].map((entry) => render(locale, entry)).join(" ");
      expect(text, locale).not.toMatch(/failed|fallida|com falha|en échec/);
      expect(text, locale).not.toContain(render(locale, { key: "plain.rules.invalid_proof", kind: "blocked" }));
      expect(text, locale).not.toContain(render(locale, { key: "plain.reasons.invalid_proofs" }));
      expect(text, locale).toContain(message(locale, "Run.kinds.blocked"));
    }
  });

  it("every blocked run of the archive is blocked by proofs that could not be generated", () => {
    const blocked = manifest().runs.filter((r) => r.outcome === "blocked");
    expect(blocked.length).toBeGreaterThan(0);
    for (const run of blocked) {
      const detail = recordedDetail(run.file, run.run_id);
      expect(ungeneratedProofs(blockedTrace(run.file, run.run_id), detail), run.label).toBe(true);
    }
  }, 60_000);

  it("a proof the scripted verifier recorded invalid, or one a real verifier rejected, keeps the invalid-check wording", () => {
    const simulated = fixture<RunDetailV1>("run-detail-invalid_proof.json");
    const trace = simulated.recommendation?.trace;
    if (trace === undefined || trace === null) {
      throw new Error("no trace in run-detail-invalid_proof.json");
    }
    expect(ungeneratedProofs(trace, simulated)).toBe(false);
    expect(plainExplanation(trace, traceReading(trace, simulated)).lead.key).toBe("plain.rules.invalid_proof");

    // L01 with one record that reports no proof error: a real verifier rejected that proof.
    const { run, detail } = archivedRun("L01");
    const l01 = blockedTrace(run.file, run.run_id);
    const rejected = JSON.parse(JSON.stringify(detail)) as RunDetailV1;
    const retrieved = rejected.events.find((e) => e.payload.type === "evidence_retrieved");
    if (retrieved === undefined || retrieved.payload.type !== "evidence_retrieved") {
      throw new Error("L01 retrieved nothing");
    }
    retrieved.payload.evidence.proof_error_reported = false;
    expect(ungeneratedProofs(l01, rejected)).toBe(false);
    expect(plainExplanation(l01, traceReading(l01, rejected)).reasons.map((r) => r.key)).toEqual([
      "plain.reasons.invalid_proofs",
    ]);
    // An invalid proof of a record the run never retrieved is no generation failure either.
    const orphan: PolicyTrace = { ...l01, reasons: [{ reason: "invalid_proofs", evidence_ids: ["ev-never-retrieved"] }] };
    expect(ungeneratedProofs(orphan, detail)).toBe(false);
  });
});
