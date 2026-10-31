// SPDX-License-Identifier: Apache-2.0
//
// Where the human decision sits and how its form is worded
// (src/lib/run/decision.ts), over the contract's fixtures: a run awaits a
// decision here only while it is awaiting_human_approval, of this server and
// undecided; Decision is the tab right after the first one (first for a
// recorded run); on a blocked or abstained run Approve reads "Approve as an
// override" and the note is required before any click, in every locale, and
// the two buttons always share one variant (Approve is never promoted over
// Reject).

import { readFileSync } from "node:fs";
import path from "node:path";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { routing } from "../src/i18n/routing";
import type { RecommendationKind, RunDetailV1 } from "../src/lib/api/types";
import { awaitsDecision, decisionFormCopy, isOverride, runTabOrder } from "../src/lib/run/decision";

function fixture<T>(name: string): T {
  return JSON.parse(readFileSync(path.join(contractDir(), "fixtures", name), "utf8")) as T;
}

type Catalog = { [key: string]: string | Catalog };

function message(locale: string, key: string): string {
  let node: string | Catalog | undefined = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key} in ${locale}`);
  }
  return node;
}

const KINDS: RecommendationKind[] = ["supported", "contested", "insufficient_evidence", "abstained", "blocked"];

describe("awaitsDecision", () => {
  const blocked = fixture<RunDetailV1>("run-detail-invalid_proof.json");
  const abstained = fixture<RunDetailV1>("run-detail-abstain.json");
  const approved = fixture<RunDetailV1>("run-detail-retry.json");

  it("is true for an undecided run of this server awaiting its decision", () => {
    expect(blocked.summary.state).toBe("awaiting_human_approval");
    expect(awaitsDecision(blocked.summary, blocked.decision)).toBe(true);
    expect(awaitsDecision(abstained.summary, abstained.decision)).toBe(true);
  });

  it("is false once a decision is recorded, in any other state, for a recorded run, or without a summary", () => {
    expect(approved.decision).toBeTruthy();
    expect(awaitsDecision(approved.summary, approved.decision)).toBe(false);
    // Awaiting, but a decision was just sent from this tab.
    expect(awaitsDecision(blocked.summary, approved.decision)).toBe(false);
    expect(awaitsDecision({ ...blocked.summary, state: "approved" }, null)).toBe(false);
    expect(awaitsDecision({ ...blocked.summary, state: "evaluated" }, null)).toBe(false);
    expect(
      awaitsDecision(
        { ...blocked.summary, provenance: { ...blocked.summary.provenance, source: "research_archive" } },
        null,
      ),
    ).toBe(false);
    expect(awaitsDecision(null, null)).toBe(false);
    expect(awaitsDecision(undefined, undefined)).toBe(false);
  });
});

describe("runTabOrder", () => {
  it("puts Decision right after the first tab of an own run", () => {
    expect(runTabOrder(false)).toEqual(["stage", "decision", "explorer", "evidence", "proofs", "audit", "meta"]);
  });

  it("puts Decision first for a recorded run, which has no stage", () => {
    expect(runTabOrder(true)).toEqual(["decision", "explorer", "evidence", "proofs", "audit", "meta"]);
  });
});

describe("decisionFormCopy", () => {
  it.each(["blocked", "abstained"] as const)("words the override of a %s run before any click", (kind) => {
    expect(isOverride(kind)).toBe(true);
    expect(decisionFormCopy(kind)).toEqual({
      approve: "approveOverride",
      noteLabel: "noteRequired",
      noteDescription: "noteRequiredDescription",
      noteOptional: false,
      overrideNotice: "overrideNotice",
      approveVariant: "secondary",
      rejectVariant: "secondary",
    });
  });

  it.each(["supported", "contested", "insufficient_evidence"] as const)("keeps the plain form on a %s run", (kind) => {
    expect(isOverride(kind)).toBe(false);
    expect(decisionFormCopy(kind)).toEqual({
      approve: "approve",
      noteLabel: "note",
      noteDescription: "noteDescription",
      noteOptional: true,
      overrideNotice: null,
      approveVariant: "secondary",
      rejectVariant: "secondary",
    });
  });

  it("never promotes Approve over Reject", () => {
    for (const kind of [...KINDS, null, undefined]) {
      const copy = decisionFormCopy(kind);
      expect(copy.approveVariant).toBe(copy.rejectVariant);
      expect(copy.approveVariant).not.toBe("primary");
    }
    expect(isOverride(null)).toBe(false);
    expect(isOverride(undefined)).toBe(false);
  });
});

describe.each(routing.locales)("the override wording (%s)", (locale) => {
  const text = (key: string, values: Record<string, string> = {}) =>
    String(new IntlMessageFormat(message(locale, key), locale).format(values));

  it("labels Approve as an override, distinct from the plain Approve", () => {
    const override = text("Run.decision.approveOverride");
    const plain = text("Run.decision.approve");
    expect(override).not.toBe(plain);
    expect(override.toLowerCase()).toContain(plain.toLowerCase());
  });

  it("marks the note required for this run, never optional", () => {
    const label = text("Run.decision.noteRequired");
    expect(label.startsWith(text("Run.decision.note"))).toBe(true);
    expect(label).not.toBe(text("Run.decision.note"));
    const optional = { en: "optional", es: "opcional", "pt-BR": "opcional", fr: "facultati" }[locale];
    expect(label.toLowerCase()).not.toContain(optional);
    expect(text("Run.decision.noteRequiredDescription").toLowerCase()).not.toContain(optional);
  });

  it("names the recommendation in the override notice", () => {
    expect(text("Run.decision.overrideNotice", { kind: "KIND" })).toContain("KIND");
  });

  it("has the header's way to the decision", () => {
    expect(text("Run.header.goToDecision").length).toBeGreaterThan(0);
    expect(text("Run.header.awaitingDecision").length).toBeGreaterThan(0);
  });
});

describe("the English override wording", () => {
  it("reads as the evaluation asks", () => {
    expect(message("en", "Run.decision.approveOverride")).toBe("Approve as an override");
    expect(message("en", "Run.decision.noteRequired")).toContain("Required for this run");
    expect(message("en", "Run.header.goToDecision")).toBe("Go to your decision");
  });
});
