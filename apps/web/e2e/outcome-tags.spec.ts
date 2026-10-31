// SPDX-License-Identifier: Apache-2.0
//
// An outcome is two tags, the outcome and the evidence it was judged on
// (OutcomeTags), and no tag is ever cut off: the honesty qualifier used to be
// exactly the part an ellipsis removed ("supported — on SIMULAT…", in es
// "respaldada — con eviden…"), and the one-sentence template read "on NO
// EVIDENCE evidence" (es "con evidencia SIMULADO"). Every outcome/evidence
// combination the mock can produce is rendered in the run header (in en, es
// and fr, one test per recommendation kind so each opens only five runs), the
// My runs cards and the showcase cards (in en, es, pt-BR and fr), at 1440 and
// 390 px, and every tag must show its whole text: scrollWidth <= clientWidth
// for the tag and its label, and the tag inside the viewport. The run waiting for the
// visitor's decision shows that state as a status tag, not plain text.

import type { Page } from "@playwright/test";

import { evidenceProvenanceSchema, recommendationKindSchema } from "../src/lib/api/schemas";
import { evidenceLabel } from "../src/lib/provenance";
import { localePath } from "../src/lib/site";

import { mockApi, RUN_ID, storedRun } from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, type Locale, t, test } from "./fixtures";
import { openRun } from "./helpers";

type Json = Record<string, unknown>;

const KINDS = recommendationKindSchema.options;
const EVIDENCE = evidenceProvenanceSchema.options;
const COMBINATIONS = KINDS.flatMap((kind) => EVIDENCE.map((evidence) => ({ kind, evidence })));
/** The showcase also lists runs that failed before a recommendation, and outcomes it does not know. */
const SHOWCASE_OUTCOMES = [...KINDS, "failed:evaluation", "escalated"] as const;
/**
 * The run header in en, es and fr (fr has the longest evidence labels, so it
 * is the one that needs the wrapping CSS); the lists in all four locales.
 */
const HEADER_LOCALES: readonly Locale[] = ["en", "es", "fr"];
const LIST_LOCALES: readonly Locale[] = ["en", "es", "pt-BR", "fr"];
const WIDTHS = [
  { width: 1440, height: 900 },
  { width: 390, height: 844 },
] as const;

/** A run id of the contract's shape, unique per index. */
function runId(index: number): string {
  return `run-00000000-0000-4000-8000-${(0x100 + index).toString(16).padStart(12, "0")}`;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function evidenceTag(locale: Locale, evidence: (typeof EVIDENCE)[number]): string {
  return t(locale, "Run.outcomeEvidence", { evidence: t(locale, `Provenance.${evidenceLabel(evidence).key}`) });
}

/**
 * Every tag on the page whose text does not fit (wider content than its box,
 * or outside the viewport): the outcome and status tags, and any other badge
 * in an outcome or state cell (where the one-sentence outcome tag used to be
 * cut off).
 */
async function clippedTags(page: Page): Promise<string[]> {
  const tags = [
    "[data-outcome-tag]",
    "[data-status-tag]",
    "[data-run-field='outcome'] [title]",
    "[data-showcase-outcome] [title]",
    ".hc-run-card dd [title]",
  ].join(", ");
  return page.locator(tags).evaluateAll((elements) => {
    const viewport = document.documentElement.clientWidth;
    const problems: string[] = [];
    for (const tag of elements) {
      for (const box of [tag, ...Array.from(tag.children)]) {
        if (box.scrollWidth > box.clientWidth) {
          problems.push(`"${tag.textContent ?? ""}": scrollWidth ${box.scrollWidth} > clientWidth ${box.clientWidth}`);
        }
      }
      const rect = tag.getBoundingClientRect();
      if (rect.left < 0 || rect.right > viewport + 0.5) {
        problems.push(`"${tag.textContent ?? ""}": outside the ${viewport}px viewport (${rect.left}..${rect.right})`);
      }
    }
    return problems;
  });
}

for (const locale of LIST_LOCALES) {
  for (const size of WIDTHS) {
    test(`My runs: every outcome and evidence as two whole tags, waiting as a status tag (${locale}, ${size.width}px)`, async ({
      page,
    }) => {
      await page.setViewportSize(size);
      const base = fixture<Json>("run-summary-retry.json");
      const runs = COMBINATIONS.map(({ kind, evidence }, index) => {
        const run = clone(base);
        run.run_id = runId(index);
        run.recommendation = kind;
        (run.provenance as Json).evidence = evidence;
        run.state = index === 0 ? "awaiting_human_approval" : "approved";
        if (index === 0) {
          run.authorization = "pending";
        }
        return run;
      });
      await mockApi(page, { runs });
      await page.goto(localePath(locale, "runs"));
      await hydrated(page);
      const cards = page.locator("[data-run-id]");
      await expect(cards).toHaveCount(COMBINATIONS.length);
      for (const [index, { kind, evidence }] of COMBINATIONS.entries()) {
        const card = page.locator(`[data-run-id='${runId(index)}']`);
        await expect(card.locator("[data-outcome-tag='outcome']")).toHaveText(t(locale, `Run.kinds.${kind}`));
        await expect(card.locator("[data-outcome-tag='evidence']")).toHaveText(evidenceTag(locale, evidence));
        await expect(card.locator("[data-outcome-tag='evidence']")).toHaveAttribute("data-evidence", evidence);
      }
      // The template that read "on NO EVIDENCE evidence" (es "con evidencia SIMULADO") is gone.
      for (const evidence of EVIDENCE) {
        const label = t(locale, `Provenance.${evidenceLabel(evidence).key}`);
        await expect(page.getByText(new RegExp(`— .*${label}`))).toHaveCount(0);
      }
      // The run waiting for the visitor's decision says so as a status tag; the others as text.
      const waiting = page.locator(`[data-run-id='${runId(0)}'] [data-runs-state='awaiting_human_approval']`);
      await expect(waiting.locator("[data-status-tag]")).toHaveText(t(locale, "Run.states.awaiting_human_approval"));
      await expect(page.locator("[data-status-tag]")).toHaveCount(1);
      expect(await clippedTags(page)).toEqual([]);
    });

    test(`Showcase: every outcome and evidence as two whole tags (${locale}, ${size.width}px)`, async ({ page }) => {
      await page.setViewportSize(size);
      const base = fixture<Json>("showcase-entry-F03.json");
      const combos = SHOWCASE_OUTCOMES.flatMap((outcome) => EVIDENCE.map((evidence) => ({ outcome, evidence })));
      const showcase = combos.map(({ outcome, evidence }, index) => {
        const entry = clone(base);
        entry.run_id = runId(index);
        entry.outcome = outcome;
        (entry.provenance as Json).evidence = evidence;
        return entry;
      });
      await mockApi(page, { showcase });
      await page.goto(localePath(locale, "showcase"));
      await hydrated(page);
      await expect(page.locator("[data-showcase='list']")).toBeVisible();
      await expect(page.locator("[data-showcase-run]")).toHaveCount(combos.length);
      for (const [index, { outcome, evidence }] of combos.entries()) {
        const card = page.locator(`[data-showcase-run='${runId(index)}']`);
        const words =
          outcome === "failed:evaluation"
            ? t(locale, "Showcase.failedAt", { stage: "evaluation" })
            : outcome === "escalated"
              ? t(locale, "Showcase.otherOutcome", { outcome })
              : t(locale, `Run.kinds.${outcome}`);
        await expect(card.locator("[data-outcome-tag='outcome']")).toHaveText(words);
        await expect(card.locator("[data-outcome-tag='evidence']")).toHaveText(evidenceTag(locale, evidence));
      }
      expect(await clippedTags(page)).toEqual([]);
    });
  }
}

// One test per recommendation kind (its five evidence axes), so no test opens
// more than five runs and the combinations run in parallel.
for (const locale of HEADER_LOCALES) {
  for (const size of WIDTHS) {
    for (const kind of KINDS) {
      test(`Run header: ${kind} on every evidence as two whole tags (${locale}, ${size.width}px)`, async ({ page }) => {
        await page.setViewportSize(size);
        await page.emulateMedia({ reducedMotion: "reduce" });
        const source = JSON.stringify(fixture<Json>("run-detail-retry.json"));
        const extraRuns = EVIDENCE.map((evidence, index) => {
          const detail = JSON.parse(source.split(RUN_ID).join(runId(index))) as Json;
          const summary = detail.summary as Json;
          summary.recommendation = kind;
          (summary.provenance as Json).evidence = evidence;
          return storedRun(detail);
        });
        await mockApi(page, { extraRuns });
        for (const [index, evidence] of EVIDENCE.entries()) {
          await openRun(page, locale, runId(index));
          const outcome = page.locator("header.hc-run-header [data-run-field='outcome']");
          await expect(outcome.locator("[data-outcome-tag='outcome']")).toHaveText(t(locale, `Run.kinds.${kind}`));
          await expect(outcome.locator("[data-outcome-tag='evidence']")).toHaveText(evidenceTag(locale, evidence));
          expect(await clippedTags(page), `${kind} on ${evidence}`).toEqual([]);
        }
      });
    }
  }
}
