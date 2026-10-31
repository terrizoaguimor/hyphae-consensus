// SPDX-License-Identifier: Apache-2.0
//
// The human decision is easy to find and does not take the policy's side
// (heuristic evaluation, round 4, problems 4 and 12), at 1440 and 390 px:
// - while a run awaits a decision the header says so and offers "Go to your
//   decision" on every tab, which opens the Decision tab and moves the focus
//   to the decision card; the link is gone once the decision is recorded,
//   even when the run's detail cannot be read again after it;
// - Decision is the tab right after the first one;
// - the decision card comes before the long explanation, with Approve and
//   Reject full-size (at least 40 px high), of the same variant and colour;
// - on blocked and abstained runs Approve reads "Approve as an override"
//   (in every locale), the note is marked "Required for this run" and the
//   override is said before any click, and the no-authority notice stays.

import type { Page } from "@playwright/test";

import type { EvidenceProvenance } from "../src/lib/api/types";
import { evidenceLabel } from "../src/lib/provenance";

import { awaitingDecisionDetail, mockApi, storedRun } from "./api-mock";
import { fixture } from "./contract";
import { expect, locales, t, test, type Locale } from "./fixtures";
import { dropProblems, openRun } from "./helpers";

type Json = Record<string, unknown>;

const VIEWPORTS = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844 },
] as const;

const OVERRIDE_RUNS = [
  { kind: "blocked", file: "run-detail-invalid_proof.json" },
  { kind: "abstained", file: "run-detail-abstain.json" },
] as const;

function runIdOf(detail: Json): string {
  return String((detail.summary as Json).run_id);
}

/** Own-run tabs, in order: the stage, then Decision. */
const OWN_ORDER = ["playback", "decision", "explorer", "evidence", "proofs", "audit", "meta"] as const;

async function boxOf(page: Page, selector: string) {
  const box = await page.locator(selector).first().boundingBox();
  expect(box, selector).not.toBeNull();
  return box as { x: number; y: number; width: number; height: number };
}

/** Approve and Reject: full-size, the same variant, the same colours. */
async function expectEqualButtons(page: Page) {
  const form = page.locator("[data-decision-form]");
  const approve = form.locator("[data-decide='approve']");
  const reject = form.locator("[data-decide='reject']");
  await expect(approve).toBeVisible();
  await expect(reject).toBeVisible();
  for (const button of [approve, reject]) {
    const box = await button.boundingBox();
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(40);
    await expect(button).toHaveAttribute("data-variant", "secondary");
  }
  const style = (selector: string) =>
    page.locator(selector).evaluate((element) => {
      const computed = getComputedStyle(element);
      return [computed.backgroundColor, computed.color, computed.borderTopColor, computed.height].join(" | ");
    });
  expect(await style("[data-decision-form] [data-decide='approve']")).toBe(
    await style("[data-decision-form] [data-decide='reject']"),
  );
}

for (const viewport of VIEWPORTS) {
  test.describe(`at ${viewport.name} px`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await page.emulateMedia({ reducedMotion: "reduce" });
    });

    for (const run of OVERRIDE_RUNS) {
      test(`a ${run.kind} run: Go to your decision on every tab, Decision second, the override said before any click (${viewport.name})`, async ({
        page,
      }) => {
        const detail = fixture<Json>(run.file);
        const runId = runIdOf(detail);
        const mock = await mockApi(page, { extraRuns: [storedRun(detail)] });
        await openRun(page, "en", runId);
        await expect(page.locator("[data-stream-state='ended']")).toBeVisible();

        // Decision is the tab right after the first one.
        await expect(page.getByRole("tab")).toHaveText(OWN_ORDER.map((name) => t("en", `Run.tabs.${name}`)));

        // The header offers the decision on every tab.
        const go = page.locator("[data-go-to-decision]");
        for (const name of OWN_ORDER) {
          await page.getByRole("tab", { name: t("en", `Run.tabs.${name}`), exact: true }).click();
          await expect(page.locator("#run-tabpanel")).toHaveAttribute("data-tab", name === "playback" ? "stage" : name);
          await expect(go).toBeVisible();
          await expect(go).toContainText(t("en", "Run.header.awaitingDecision"));
          await expect(go.getByRole("button", { name: t("en", "Run.header.goToDecision") })).toBeVisible();
        }

        // From the last tab, the header's button opens Decision and focuses the card.
        await go.getByRole("button", { name: t("en", "Run.header.goToDecision") }).click();
        await expect(page.locator("#run-tabpanel")).toHaveAttribute("data-tab", "decision");
        await expect(page.getByRole("tab", { name: t("en", "Run.tabs.decision") })).toHaveAttribute(
          "aria-selected",
          "true",
        );
        await expect(page.locator("#decision-form")).toBeFocused();
        await expect(page.locator("#decision-form")).toBeInViewport();

        // The card comes before the long explanation.
        const card = page.locator("[data-decision-card='first']");
        await expect(card).toBeVisible();
        const cardBox = await boxOf(page, "[data-decision-card='first']");
        const explanation = await boxOf(page, "#decision-recommendation");
        expect(cardBox.y).toBeLessThan(explanation.y);
        expect(cardBox.x + cardBox.width).toBeLessThanOrEqual(viewport.width);
        // It names the recommendation it is about, with its evidence axis.
        await expect(card.locator("[data-decision-card-lead] [data-recommendation-kind]")).toHaveAttribute(
          "data-recommendation-kind",
          run.kind,
        );
        const evidence = ((detail.summary as Json).provenance as Json).evidence as EvidenceProvenance;
        await expect(card.locator("[data-decision-card-lead] [data-recommendation-kind]")).toHaveText(
          t("en", "Run.outcome", {
            kind: t("en", `Run.kinds.${run.kind}`),
            evidence: t("en", `Provenance.${evidenceLabel(evidence).key}`),
          }),
        );

        // The no-authority notice stays.
        await expect(card.locator("[data-public-demo-notice]")).toHaveText(t("en", "Run.decision.publicDemo"));

        // Before any click: the override is said, the note is required, Approve says it is an override.
        const form = card.locator("[data-decision-form]");
        await expect(form).toHaveAttribute("data-override", "yes");
        await expect(form.locator("[data-override-notice]")).toHaveText(
          t("en", "Run.decision.overrideNotice", { kind: t("en", `Run.kinds.${run.kind}`) }),
        );
        await expect(form).toContainText("Required for this run");
        await expect(form.getByLabel(t("en", "Run.decision.noteRequired"))).toBeVisible();
        await expect(form).not.toContainText(t("en", "Run.decision.noteDescription"));
        await expect(form.getByRole("alert")).toHaveCount(0);
        await expect(form.locator("[data-decide='approve']")).toHaveText(t("en", "Run.decision.approveOverride"));
        await expect(
          form.getByRole("button", { name: t("en", "Run.decision.approveOverride"), exact: true }),
        ).toBeVisible();
        await expect(form.locator("[data-decide='reject']")).toHaveText(t("en", "Run.decision.reject"));
        await expectEqualButtons(page);
        // The buttons are reachable near the top of the tab: in the card, before the explanation.
        const approveBox = await boxOf(page, "[data-decision-form] [data-decide='approve']");
        expect(approveBox.y).toBeLessThan(explanation.y);
        expect(approveBox.x + approveBox.width).toBeLessThanOrEqual(viewport.width);

        // Approving without a note is still refused here, and nothing is sent.
        await form.locator("[data-decide='approve']").click();
        await expect(form.getByRole("alert")).toHaveText(
          t("en", "Run.decision.overrideRequired", { kind: t("en", `Run.kinds.${run.kind}`) }),
        );
        expect(mock.calls.filter((c) => c.method === "POST" && c.path.endsWith("/decision"))).toHaveLength(0);
      });
    }

    test(`a supported run: the plain form, equal buttons, and no header link once decided (${viewport.name})`, async ({
      page,
    }) => {
      await mockApi(page, { detail: awaitingDecisionDetail() });
      await openRun(page, "en");
      await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
      await expect(page.getByRole("tab")).toHaveText(OWN_ORDER.map((name) => t("en", `Run.tabs.${name}`)));
      const go = page.locator("[data-go-to-decision]");
      await go.getByRole("button", { name: t("en", "Run.header.goToDecision") }).click();
      await expect(page.locator("#decision-form")).toBeFocused();
      const form = page.locator("[data-decision-card='first'] [data-decision-form]");
      await expect(form).toHaveAttribute("data-override", "no");
      await expect(form.locator("[data-override-notice]")).toHaveCount(0);
      await expect(form.locator("[data-decide='approve']")).toHaveText(t("en", "Run.decision.approve"));
      await expect(form).not.toContainText("Required for this run");
      await expectEqualButtons(page);
      await form.locator("[data-decide='approve']").click();
      await expect(page.locator("[data-decision-recorded='approved']")).toBeVisible();
      // Decided: the card keeps its place, the header no longer offers the decision.
      await expect(page.locator("[data-decision-card='first'] [data-decision-recorded='approved']")).toBeVisible();
      await expect(go).toHaveCount(0);
    });

    test(`a decision recorded while the detail cannot be read again: the header and the tab agree (${viewport.name})`, async ({
      page,
      pageProblems,
    }) => {
      const mock = await mockApi(page, { detail: awaitingDecisionDetail() });
      // Once the decision is POSTed, every later GET /runs/{id} fails.
      const posted = () => mock.calls.some((c) => c.method === "POST" && c.path.endsWith("/decision"));
      const refetches: string[] = [];
      await page.route(
        (url) => /^\/api\/v1\/runs\/run-[0-9a-f-]+$/.test(url.pathname),
        async (route, request) => {
          if (request.method() === "GET" && posted()) {
            refetches.push(request.url());
            return route.fulfill({
              status: 500,
              contentType: "application/json",
              body: JSON.stringify({ code: "internal", message: "internal error" }),
            });
          }
          return route.fallback();
        },
      );
      await openRun(page, "en");
      await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
      const go = page.locator("[data-go-to-decision]");
      await go.getByRole("button", { name: t("en", "Run.header.goToDecision") }).click();
      const form = page.locator("[data-decision-card='first'] [data-decision-form]");
      await form.locator("[data-decide='reject']").click();
      await expect(page.locator("[data-decision-recorded='rejected']")).toBeVisible();
      expect(refetches.length).toBeGreaterThan(0);
      // The tab shows the decision as recorded, and the header no longer asks for it.
      await expect(go).toHaveCount(0);
      // Leaving the tab and coming back keeps the recorded decision, never the form again.
      await page.getByRole("tab", { name: t("en", "Run.tabs.evidence"), exact: true }).click();
      await expect(go).toHaveCount(0);
      await page.getByRole("tab", { name: t("en", "Run.tabs.decision"), exact: true }).click();
      await expect(page.locator("[data-decision-recorded='rejected']")).toBeVisible();
      await expect(page.locator("[data-decision-form]")).toHaveCount(0);
      // The failed re-reads were provoked here on purpose; any other problem still counts.
      await expect
        .poll(() => pageProblems.filter((p) => p.startsWith("HTTP 500: /api/v1/runs/")).length)
        .toBeGreaterThanOrEqual(1);
      dropProblems(pageProblems, /^HTTP 500: \/api\/v1\/runs\/run-[0-9a-f-]+$/);
    });

    for (const locale of locales.filter((l): l is Exclude<Locale, "en"> => l !== "en")) {
      test(`the override wording in ${locale} (${viewport.name})`, async ({ page }) => {
        const detail = fixture<Json>("run-detail-invalid_proof.json");
        await mockApi(page, { extraRuns: [storedRun(detail)] });
        await openRun(page, locale, runIdOf(detail));
        await expect(page.getByRole("tab").nth(1)).toHaveText(t(locale, "Run.tabs.decision"));
        await page
          .locator("[data-go-to-decision]")
          .getByRole("button", { name: t(locale, "Run.header.goToDecision") })
          .click();
        const form = page.locator("[data-decision-card='first'] [data-decision-form]");
        await expect(form.locator("[data-decide='approve']")).toHaveText(t(locale, "Run.decision.approveOverride"));
        await expect(form.getByLabel(t(locale, "Run.decision.noteRequired"))).toBeVisible();
        await expect(form.locator("[data-override-notice]")).toHaveText(
          t(locale, "Run.decision.overrideNotice", { kind: t(locale, "Run.kinds.blocked") }),
        );
        await expect(page.locator("[data-public-demo-notice]")).toHaveText(t(locale, "Run.decision.publicDemo"));
        await expectEqualButtons(page);
      });
    }
  });
}
