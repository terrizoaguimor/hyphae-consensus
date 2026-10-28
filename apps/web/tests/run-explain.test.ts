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
} from "../src/lib/run/explain";

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

function plainText(locale: string, trace: PolicyTrace): string {
  const plain = plainExplanation(trace);
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

  it("names the ids of each failed check, duplicates of open items included", () => {
    expect(reasonIds(SAMPLE_REASONS.open_items)).toEqual(["item-7", "item-7"]);
    expect(reasonIds(SAMPLE_REASONS.no_checked_evidence)).toEqual([]);
    expect(reasonIds(SAMPLE_REASONS.invalid_proofs)).toEqual(["ev-1"]);
  });
});
