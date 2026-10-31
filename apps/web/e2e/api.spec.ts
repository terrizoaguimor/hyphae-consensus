// SPDX-License-Identifier: Apache-2.0
//
// The API-connected views over a mocked /api/v1 (e2e/api-mock.ts, the
// contract fixtures): New deliberation -> run -> frames -> decision ->
// receipt link; the honesty rules (no LIVE word on a replayed, flagged or
// listed run in any locale, LIVE only for a live frame of a live-model run in
// progress, pacing only for scripted runs, simulated evidence named next to
// the outcome, simulated proofs marked); the live form in the contract's
// shape (closed without a Turnstile site key, tripped or unreadable ledger,
// the open form's request); failed runs and failed requests shown from their
// codes; My runs with and without a session; the runtime NotConnected state
// against the static server; the connected and not-connected variants of
// the copy that depends on the connection (round 4, HE #8); error headlines
// by class and context, the 429 countdown and fallback, and the guided error
// inside its card (HE #18); and axe on /new, /runs and /run in every locale
// and theme.

import AxeBuilder from "@axe-core/playwright";
import type { Locator, Page, Route } from "@playwright/test";
import { IntlMessageFormat } from "intl-messageformat";

import { CONTRACT_SHA256 } from "../src/lib/api/types";
import { localePath, runPath } from "../src/lib/site";

import { AXE_TAGS, blocking, type ScanSummary, summarize, writeScans } from "./axe-results";
import {
  awaitingDecisionDetail,
  beforeCarryForwardDetail,
  chainedRun,
  failedDetail,
  framesWith,
  inProgressLiveRun,
  liveModelDetail,
  liveR01Run,
  metaCallsDetail,
  mockApi,
  PUBLIC_DEMO_NOTICE,
  RUN_ID,
  sealedSnapshotDetail,
  SNAPSHOT_ANCHOR,
  storedRun,
} from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, locales, t, test, useTheme } from "./fixtures";
import {
  dropProblems,
  EVIDENCE_BADGE,
  expectNoLive,
  liveBadge,
  openRun,
  OUTCOME_BADGE,
  takeProblem,
} from "./helpers";

type Json = Record<string, unknown>;

const LIVE_RUN_ID = "run-00000000-0000-4000-8000-000000000006";

/**
 * Every badge the locator finds (at least one) is in the `variant` colour
 * token (Astryx reflects it as data-variant), and none is the success or
 * green one: an outcome or a claim tag on simulated evidence is never green
 * (HE #1); one label keeps one colour (HE #16).
 */
async function expectNotGreen(badges: Locator, variant: string) {
  await expect(badges.first()).toBeVisible();
  const variants = await badges.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-variant")));
  expect(variants.length).toBeGreaterThan(0);
  for (const value of variants) {
    expect(value).not.toBe("success");
    expect(value).not.toBe("green");
    expect(value).toBe(variant);
  }
}

test("new -> run -> frames -> decision -> receipt link @smoke", async ({ page, pageProblems }) => {
  const mock = await mockApi(page, { detail: awaitingDecisionDetail() });
  await page.goto("/en/new/");
  await hydrated(page);
  await expect(page.locator("[data-api-gate='ok']")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: t("en", "NewDeliberation.guided.heading") }),
  ).toBeVisible();
  const cards = page.locator("[data-scenario]");
  await expect(cards).toHaveCount(5);
  // The cards' model label is the run pages' SCRIPTED label (owner decision
  // 2026-10-31), never a card-only "SIMULATED model".
  await expect(
    cards.getByText(t("en", "Provenance.scripted.label"), { exact: true }),
  ).toHaveCount(5);
  await expect(cards.locator("[data-scenario-model='scripted']")).toHaveCount(5);
  await expect(page.getByText("SIMULATED model")).toHaveCount(0);
  await expect(cards.first().getByText("supported", { exact: true })).toBeVisible();

  const runButton = page.locator("[data-scenario='retry']").getByRole("button");
  await expect(runButton).toHaveAccessibleName(
    t("en", "NewDeliberation.guided.runScenario", { title: "Reference: retry budget" }),
  );
  await runButton.click();
  await expect(page).toHaveURL(new RegExp(`${localePath("en", "run")}\\?id=${RUN_ID}$`));
  const creation = mock.calls.find((c) => c.method === "POST" && c.path === "/api/v1/runs");
  expect(creation?.body).toEqual({ scenario: "retry" });
  expect(creation?.headers["idempotency-key"]).toMatch(/^[0-9a-f-]{36}$/);
  expect(creation?.headers["content-type"]).toBe("application/json");
  expect(mock.calls.filter((c) => c.path === "/api/v1/session").map((c) => c.method)).toEqual(["GET"]);

  await hydrated(page);
  await expect(page.locator("h2.hc-run-title")).toHaveText("Fix payment client timeouts");
  // An own run's title is the requester's text in any language: never marked lang="en".
  await expect(page.locator("h2.hc-run-title")).not.toHaveAttribute("lang", /.*/u);
  // The guided run finished server-side: its frames replay, paced client-side.
  await expect(page.locator("[data-pacing='on']")).toBeVisible();
  await expect(page.getByText(t("en", "Run.pacing.label"), { exact: true })).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.pacing.skip") }).click();
  await expect(page.locator("[data-pacing='skipped']")).toBeVisible();
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  // Once the stream ended the header follows the re-read detail.
  await expect(page.locator("[data-run-field='state']")).toHaveText(
    t("en", "Run.states.awaiting_human_approval"),
  );
  await expect(page.locator("[data-role-card='Proposer']")).toContainText(
    "Cap retries at 3 attempts with jitter per the documented constraint",
  );
  await expect(page.locator("[data-role-card='Critic']")).toContainText(t("en", "Run.stage.noObjections"));
  await expect(page.locator("[data-role-card='Evidence Evaluator']")).toContainText("claim-2");
  await expect(page.locator("[data-lane]")).toContainText("2 records retrieved");
  // Simulated proofs checked nothing: counted as such, never "proofs checked" (HE #2).
  await expect(page.locator("[data-lane-proofs='simulated']")).toHaveText("4 simulated proofs (not checked)");
  await expect(page.locator("[data-lane]")).not.toContainText("proofs checked");
  await expect(page.locator("[data-lane-proofs='checked']")).toHaveCount(0);
  await expect(page.locator("[data-proof-simulated='true']")).toHaveCount(4);
  await expect(page.locator("[data-recommendation] [data-outcome-tag='outcome']")).toHaveText("supported");
  await expect(page.locator("[data-recommendation] [data-outcome-tag='evidence']")).toHaveText(
    "Evidence: SIMULATED",
  );
  // "supported" on SIMULATED evidence is never green: the info role (HE #1),
  // in the stage and in the header; "corroborated" likewise.
  // The outcome is its own tag (OutcomeTags): the outcome tag is info, the
  // evidence tag next to it is SIMULATED's purple, and neither is green.
  await expectNotGreen(page.locator("[data-recommendation] [data-recommendation-kind]").locator(OUTCOME_BADGE), "info");
  await expectNotGreen(page.locator("[data-run-field='outcome']").locator(OUTCOME_BADGE), "info");
  await expectNotGreen(page.locator("[data-recommendation] [data-recommendation-kind]").locator(EVIDENCE_BADGE), "purple");
  await expectNotGreen(page.locator("[data-run-field='outcome']").locator(EVIDENCE_BADGE), "purple");
  await expectNotGreen(
    page.locator("[data-role-card='Evidence Evaluator'] .astryx-badge", {
      hasText: new RegExp(`^${t("en", "Run.stage.corroborated")}$`),
    }),
    "info",
  );
  await expect(page.locator("[data-rationale-simulated]")).toHaveText(t("en", "Run.stage.rationaleSimulated"));
  await expect(page.locator("[data-frame-delivery='replay']").first()).toBeVisible();
  await expect(liveBadge(page, "en")).toHaveCount(0);
  await expect(page.locator("[data-model-note='scripted']").first()).toHaveText(
    t("en", "Provenance.scripted.note"),
  );

  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-public-demo-notice]")).toHaveText(t("en", "Run.decision.publicDemo"));
  await expect(page.locator("[data-superseded]")).toContainText(
    "Raise the payment client retry limit to fix timeouts",
  );
  await expect(page.locator("[data-superseded]")).toContainText(
    "Cap retries at 3 attempts with jitter per the documented constraint",
  );
  await expect(page.locator("[data-open-objections='0']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.decision.approve") }).click();
  await expect(page.locator("[data-decision-recorded='approved']")).toBeVisible();
  await expect(page.locator("[data-decision-notice]")).toHaveText(PUBLIC_DEMO_NOTICE);
  const decision = mock.calls.find((c) => c.method === "POST" && c.path.endsWith("/decision"));
  expect(decision?.body).toEqual({
    approved: true,
    expected: { recommendation: "supported", last_sequence: 14 },
  });
  expect(decision?.headers["idempotency-key"]).toMatch(/^[0-9a-f-]{36}$/);
  await expect(page.locator("[data-receipt-link]")).toHaveAttribute(
    "href",
    `/api/v1/runs/${RUN_ID}/receipt`,
  );
  // A live run's receipt is not an archive rebuild: no such note.
  await expect(page.locator("[data-receipt-rebuilt]")).toHaveCount(0);
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Run.states.approved"));
  expect(pageProblems).toEqual([]);
});

test("a replayed finished run never shows LIVE and is named Playback @smoke", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-pacing='reduced']")).toBeVisible();
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.playback") })).toBeVisible();
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.live") })).toHaveCount(0);
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(page.locator("[data-frame-delivery='replay']").first()).toBeVisible();
  await expect(page.locator("[data-frame-label='live.label']")).toHaveCount(0);
  await expect(liveBadge(page, "en")).toHaveCount(0);
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Run.states.approved"));
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-decision-recorded='approved']")).toBeVisible();
  await expect(page.locator("[data-decision-form]")).toHaveCount(0);
});

test("a replayed live-model run reads REPLAY and 'Meta model · recorded', never LIVE", async ({
  page,
}) => {
  const detail = liveModelDetail();
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  // A replay of a live model is META MODEL next to its evidence, never LIVE META.
  await expect(page.getByText(t("en", "Provenance.metaModel.label"), { exact: true }).first()).toBeVisible();
  await expect(page.getByText(t("en", "Provenance.liveMeta.evidence"), { exact: true }).first()).toBeVisible();
  await expect(page.locator("[data-provenance-note='liveMeta.recorded']")).toContainText("Meta model");
  await expect(page.getByText(t("en", "Provenance.replay.label"), { exact: true }).first()).toBeVisible();
  await expectNoLive(page, "en");
});

test("a provenance_inconsistent run never shows LIVE, even on live-delivered frames", async ({
  page,
}) => {
  const detail = liveModelDetail(["provenance_inconsistent"]);
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "live") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(
    page.getByText(t("en", "Provenance.flags.provenance_inconsistent"), { exact: true }).first(),
  ).toBeVisible();
  await expect(page.getByText(t("en", "Provenance.flagNotes.provenance_inconsistent"))).toBeVisible();
  await expect(page.locator("[data-frame-delivery='live']").first()).toBeVisible();
  await expect(page.locator("[data-frame-label='live.label']")).toHaveCount(0);
  await expect(page.locator("[data-frame-label='streaming.label']").first()).toBeVisible();
  await expect(liveBadge(page, "en")).toHaveCount(0);
  await expect(page.locator("[data-provenance-note='liveMeta.recorded']")).toBeVisible();
  await expectNoLive(page, "en");
});

for (const locale of locales) {
  test(`no LIVE word on a replayed, flagged or listed live-model run (${locale})`, async ({ page }) => {
    const replayed = liveModelDetail();
    const replayedProvenance = (replayed.summary as { provenance: Record<string, unknown> }).provenance;
    const flagged = liveModelDetail(["mode_undeclared"]);
    const flaggedProvenance = (flagged.summary as { provenance: Record<string, unknown> }).provenance;
    await page.emulateMedia({ reducedMotion: "reduce" });

    // A replayed live-model run: META MODEL, recorded, REPLAY; header labels, never ICU source.
    await mockApi(page, { detail: replayed, frames: framesWith(replayedProvenance, "replay") });
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    await expect(
      page.getByText(t(locale, "Provenance.metaModel.label"), { exact: true }).first(),
    ).toBeVisible();
    await expect(page.locator("[data-provenance-note='liveMeta.recorded']")).toBeVisible();
    await expect(page.getByRole("tab", { name: t(locale, "Run.tabs.playback") })).toBeVisible();
    await expectNoLive(page, locale);
    // META MODEL in the info colour here too: one colour per label (HE #16).
    await expectNotGreen(
      page.locator(".astryx-badge", { hasText: new RegExp(`^${t(locale, "Provenance.metaModel.label")}$`) }),
      "info",
    );
    const header = page.locator("header.hc-run-header");
    await expect(header.locator("dl dt")).toHaveText([
      t(locale, "Run.header.state"),
      t(locale, "Run.header.outcome"),
      t(locale, "Run.header.recordsLabel"),
      t(locale, "Run.header.revision"),
      t(locale, "Run.header.runId"),
    ]);
    expect(await header.innerText()).not.toContain("{");
    await page.unrouteAll({ behavior: "ignoreErrors" });

    // A flagged run with live-delivered frames: the flag in its own warning
    // badge, the model badge META MODEL in its one (info) colour, no LIVE.
    await mockApi(page, { detail: flagged, frames: framesWith(flaggedProvenance, "live") });
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    await expect(
      page.getByText(t(locale, "Provenance.flags.mode_undeclared"), { exact: true }).first(),
    ).toBeVisible();
    await expectNotGreen(
      page.locator(".astryx-badge", { hasText: new RegExp(`^${t(locale, "Provenance.metaModel.label")}$`) }),
      "info",
    );
    await expectNotGreen(
      page.locator(".astryx-badge", { hasText: new RegExp(`^${t(locale, "Provenance.flags.mode_undeclared")}$`) }),
      "warning",
    );
    await expect(page.locator("[data-frame-delivery='live']").first()).toBeVisible();
    await expectNoLive(page, locale);
    await page.unrouteAll({ behavior: "ignoreErrors" });

    // My runs: a list never reads LIVE; a live-model run is META MODEL, recorded <date>.
    await mockApi(page, { runs: [replayed.summary as Record<string, unknown>] });
    await page.goto(localePath(locale, "runs"));
    await hydrated(page);
    await expect(page.locator("[data-run-id]")).toHaveCount(1);
    await expect(
      page.getByText(t(locale, "Provenance.metaModel.label"), { exact: true }),
    ).toBeVisible();
    await expect(page.locator("[data-provenance-note='liveMeta.recorded']")).toBeVisible();
    await expectNoLive(page, locale);
  });
}

test("a live-model run in progress with live frames reads LIVE and is named Live", async ({ page }) => {
  const run = inProgressLiveRun(5);
  await mockApi(page, { detail: run.detail, frames: run.frames });
  await openRun(page, "en");
  await expect(page.locator("[data-frame-label='live.label']").first()).toBeVisible();
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.live") })).toBeVisible();
  const header = page.locator("header.hc-run-header");
  await expect(header.getByText(t("en", "Provenance.liveMeta.label"), { exact: true })).toBeVisible();
  await expect(header.getByText(t("en", "Provenance.live.label"), { exact: true })).toBeVisible();
  await expect(header.getByText(t("en", "Provenance.liveMeta.evidence"), { exact: true })).toBeVisible();
  // LIVE and LIVE META keep the LIVE colour, which nothing else uses: never
  // the green of a verified proof, never the info colour of META MODEL (HE #16).
  const liveLabels = [t("en", "Provenance.live.label"), t("en", "Provenance.liveMeta.label")];
  for (const label of liveLabels) {
    await expectNotGreen(header.locator(".astryx-badge", { hasText: new RegExp(`^${label}$`) }), "pink");
  }
  const pink = await page.locator(".astryx-badge[data-variant='pink']").allInnerTexts();
  expect(pink.length).toBeGreaterThan(1);
  for (const text of pink) {
    expect(liveLabels, `a non-LIVE label in the LIVE colour: ${text}`).toContain(text.trim());
  }
  await expect(page.locator("[data-provenance-note='liveMeta.live']")).toHaveText(
    t("en", "Provenance.liveMeta.live"),
  );
  // Live frames are never paced, and a live model is never labelled simulated.
  await expect(page.locator("[data-pacing]")).toHaveCount(0);
});

for (const locale of locales) {
  test(`the Overview legend: LIVE META in the LIVE colour, META MODEL in info, nothing green (HE #16, ${locale})`, async ({
    page,
  }) => {
    await mockApi(page);
    await page.goto(`/${locale}/`);
    await hydrated(page);
    const legend = page.locator(".hc-provenance-list");
    const badges = legend.locator(".astryx-badge");
    await expect(badges.first()).toBeVisible();
    const liveMeta = t(locale, "Provenance.liveMeta.label");
    const metaModel = t(locale, "Provenance.metaModel.label");
    await expectNotGreen(legend.locator(".astryx-badge", { hasText: new RegExp(`^${liveMeta}$`) }), "pink");
    // META MODEL appears twice (the live model, and the hybrid runs with simulated proofs): info both times.
    const metaModels = legend.locator(".astryx-badge", { hasText: new RegExp(`^${metaModel}$`) });
    await expect(metaModels).toHaveCount(2);
    await expectNotGreen(metaModels, "info");
    for (const variant of await badges.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-variant")))) {
      expect(variant).not.toBe("success");
      expect(variant).not.toBe("green");
    }
    // The LIVE colour is LIVE META's alone.
    const pink = await legend.locator(".astryx-badge[data-variant='pink']").allInnerTexts();
    expect(pink.map((text) => text.trim())).toEqual([liveMeta]);
  });
}

for (const delivery of ["live", "replay"] as const) {
  test(`a live-model run is never paced or labelled simulated (${delivery} frames)`, async ({ page }) => {
    const detail = liveModelDetail();
    const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
    await mockApi(page, { detail, frames: framesWith(provenance, delivery) });
    // Pacing is on (no reduced motion): it must still not apply.
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await openRun(page, "en");
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    // Every frame shows at once: nothing was held back. (This fixture's
    // proofs are the scripted verifier's, so they count as simulated.)
    await expect(page.locator("[data-lane-proofs='simulated']")).toHaveText("4 simulated proofs (not checked)");
    await expect(page.locator("[data-pacing]")).toHaveCount(0);
    await expect(page.getByText(t("en", "Run.pacing.label"), { exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: t("en", "Run.pacing.skip") })).toHaveCount(0);
  });
}

test("pacing is announced when it starts and when it ends, not per record", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await openRun(page, "en");
  const pacing = page.locator("[data-pacing='on']");
  await expect(pacing.getByText(t("en", "Run.pacing.label"), { exact: true })).toBeVisible();
  // One live region, saying that pacing started; the waiting count is not one.
  const announcement = page.locator("[data-pacing-announcement]");
  await expect(announcement).toHaveAttribute("role", "status");
  await expect(announcement).toHaveText(t("en", "Run.pacing.started"));
  await expect(page.locator("[data-pacing-pending]")).toBeVisible();
  await expect(page.locator("[data-pacing-pending][role=status]")).toHaveCount(0);
  await page.getByRole("button", { name: t("en", "Run.pacing.skip") }).click();
  await expect(announcement).toHaveText(t("en", "Run.pacing.done"));
  await expect(page.locator("[data-pacing='skipped']")).toContainText(t("en", "Run.pacing.skipped"));
});

test("the in-app Reduced motion setting turns pacing off", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/en/settings/");
  await hydrated(page);
  await page
    .getByRole("radiogroup", { name: t("en", "Settings.motion.label") })
    .getByRole("radio", { name: t("en", "Settings.motion.options.reduce") })
    .check();
  await expect(page.locator("html")).toHaveAttribute("data-motion", "reduce");
  await openRun(page, "en");
  await expect(page.locator("[data-pacing='reduced']")).toHaveText(t("en", "Run.pacing.off"));
  await expect(page.getByText(t("en", "Run.pacing.label"), { exact: true })).toHaveCount(0);
});

test("the live section shows the disabled state when live_enabled is false", async ({ page }) => {
  await mockApi(page);
  await page.goto("/en/new/");
  await hydrated(page);
  const section = page.locator("[data-live-enabled='false']");
  await expect(section.locator("[data-live='disabled']")).toContainText(
    t("en", "NewDeliberation.live.disabled"),
  );
  await expect(
    section.getByRole("link", { name: t("en", "NewDeliberation.live.showcaseLink") }),
  ).toHaveAttribute("href", "/en/showcase/");
  await expect(page.locator("[data-live='form']")).toHaveCount(0);
});

test("without a Turnstile site key the live form is closed, in the contract's shape", async ({ page }) => {
  const mock = await mockApi(page, { liveEnabled: true });
  await page.goto("/en/new/");
  await hydrated(page);
  const form = page.locator("[data-live='form']");
  await expect(form).toHaveAttribute("data-live-state", "closed");
  await expect(form.locator("[data-turnstile='missing']")).toHaveText(
    t("en", "NewDeliberation.live.turnstileMissing"),
  );
  // The preset's badges are its own recorded axes, never a section-wide LIVE META.
  const preset = form.locator("[data-preset='R01-reference']");
  await expect(preset).toContainText("Payment requests intermittently time out under load.");
  await expect(preset.getByText(t("en", "Provenance.metaModel.label"), { exact: true })).toBeVisible();
  await expect(preset.getByText(t("en", "Provenance.liveMeta.evidence"), { exact: true })).toBeVisible();
  await expect(page.getByText(t("en", "Provenance.liveMeta.label"), { exact: true })).toHaveCount(0);
  // The budget: translated labels, US dollars, never a raw key.
  const budget = form.locator("[data-live-budget]");
  await expect(budget.locator("dt")).toHaveText([
    t("en", "NewDeliberation.live.budget.availableNow"),
    t("en", "NewDeliberation.live.budget.dailyRemaining"),
    t("en", "NewDeliberation.live.budget.lifetimeRemaining"),
  ]);
  await expect(budget.locator("[data-budget='daily_available_now']")).toHaveText("$4.25");
  await expect(budget.locator("[data-budget='daily_remaining']")).toHaveText("$9.12");
  await expect(budget.locator("[data-budget='lifetime_remaining']")).toHaveText("$51.30");
  await expect(form).not.toContainText("daily_available_now");
  // Nothing can be sent: the presets are listed to read, and Start is disabled.
  await expect(form.getByRole("list", { name: t("en", "NewDeliberation.live.presetLegend") })).toContainText(
    "Fix payment client timeouts",
  );
  await expect(form.getByRole("radio")).toHaveCount(0);
  await expect(form.getByRole("checkbox")).toHaveCount(0);
  await expect(page.getByRole("button", { name: t("en", "NewDeliberation.live.start") })).toBeDisabled();
  expect(mock.calls.filter((c) => c.path === "/api/v1/live-runs")).toEqual([]);
});

test("a tripped or unreadable ledger closes the live form with its reason", async ({ page }) => {
  await mockApi(page, {
    liveEnabled: true,
    liveRequiresTurnstile: false,
    liveBudget: {
      daily_available_now: "0.000000",
      daily_remaining: "0.000000",
      lifetime_remaining: "12.500000",
      tripped: true,
      trip_code: "kill_switch",
    },
  });
  await page.goto("/en/new/");
  await hydrated(page);
  const closed = page.locator("[data-live-closed='tripped']");
  await expect(closed).toContainText(t("en", "NewDeliberation.live.closed.tripped"));
  await expect(closed.locator("[data-trip-code='kill_switch']")).toHaveText(
    t("en", "NewDeliberation.live.trip.kill_switch"),
  );
  await expect(closed.getByRole("link", { name: t("en", "NewDeliberation.live.showcaseLink") })).toHaveAttribute(
    "href",
    "/en/showcase/",
  );
  await expect(page.getByRole("button", { name: t("en", "NewDeliberation.live.start") })).toBeDisabled();
  await page.unrouteAll({ behavior: "ignoreErrors" });

  await mockApi(page, { liveEnabled: true, liveRequiresTurnstile: false, liveBudget: null });
  await page.goto("/en/new/");
  await hydrated(page);
  await expect(page.locator("[data-live-closed='budget_unknown']")).toContainText(
    t("en", "NewDeliberation.live.closed.budgetUnknown"),
  );
  await expect(page.locator("[data-live-budget]")).toHaveCount(0);
  await expect(page.getByRole("button", { name: t("en", "NewDeliberation.live.start") })).toBeDisabled();
});

test("the open live form sends the contract's request and explains a live refusal", async ({
  page,
  pageProblems,
}) => {
  // A server that requires no Turnstile token (a contract-valid answer) opens
  // the form in this build, which has no site key: the request path is real.
  const mock = await mockApi(page, {
    liveEnabled: true,
    liveRequiresTurnstile: false,
    liveRuns: [
      {
        status: 429,
        body: { code: "live_concurrency", message: "every live slot is busy; retry shortly", retry_after_secs: 30 },
        retryAfter: "30",
      },
    ],
  });
  // The refusal's 30-second wait is counted down by the page's own clock.
  await page.clock.install();
  await page.goto("/en/new/");
  await hydrated(page);
  const form = page.locator("[data-live='form']");
  await expect(form).toHaveAttribute("data-live-state", "open");
  const start = page.getByRole("button", { name: t("en", "NewDeliberation.live.start") });
  await expect(start).toBeDisabled();
  await expect(form.locator("[data-start-blocker='preset']")).toHaveText(t("en", "NewDeliberation.live.needsPreset"));
  await form.getByRole("radio", { name: "Fix payment client timeouts" }).check();
  await expect(form.locator("[data-start-blocker='attestation']")).toBeVisible();
  // The attestation is the one the server records: age AND territory, with the Terms.
  const attest = form.getByRole("checkbox", { name: t("en", "NewDeliberation.live.attest") });
  await expect(form.locator("[data-live-attestation]")).toContainText("restricted territory");
  await expect(
    form.locator("[data-live-attestation]").getByRole("link", { name: t("en", "Nav.terms") }),
  ).toHaveAttribute("href", "/en/terms/");
  await attest.check();
  await page.getByText(t("en", "NewDeliberation.live.inviteSummary")).click();
  const invite = form.getByRole("textbox", { name: t("en", "NewDeliberation.live.inviteLabel") });
  await invite.fill("x".repeat(257));
  await expect(form.locator("[data-start-blocker='judge_code']")).toBeVisible();
  await expect(start).toBeDisabled();
  await invite.fill("  JUDGE-1  ");
  await expect(start).toBeEnabled();
  await start.click();
  const notice = page.locator("[data-error-code='live_concurrency']");
  await expect(notice).toContainText(t("en", "Api.errors.live_concurrency"));
  await expect(notice).toContainText("Retry in 30 seconds.");
  await expect(notice).toContainText(t("en", "Api.errorDetail", { code: "live_concurrency", status: 429 }));
  // Round 4 (HE #18): headed by its consequence, Start disabled for the wait
  // with a countdown, and a recorded run offered instead.
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.liveInProgress"));
  // During the wait the button is named by its countdown.
  const submit = form.locator("button[type='submit']");
  await expect(submit).toBeDisabled();
  await expect(submit).toContainText(/^Try again in 0:[0-3]\d$/u);
  await expect(notice.locator("[data-error-fallback]")).toHaveText(t("en", "Api.fallback"));
  await expect(notice.locator("[data-error-fallback]")).toHaveAttribute("href", "/en/showcase/");
  await page.clock.fastForward("00:31");
  await expect(start).toBeEnabled();
  // A blank code is not sent at all: the run takes the public lane.
  await invite.fill("   ");
  await start.click();
  const liveRuns = () => mock.calls.filter((c) => c.method === "POST" && c.path === "/api/v1/live-runs");
  await expect.poll(() => liveRuns().length).toBe(2);
  await expect(notice).toBeVisible();
  await expect(submit).toContainText(/^Try again in /u);
  await page.clock.fastForward("00:31");
  await expect(start).toBeEnabled();
  const sent = liveRuns();
  expect(sent.map((c) => c.body)).toEqual([
    { preset_id: "R01-reference", attest_adult: true, judge_code: "JUDGE-1" },
    { preset_id: "R01-reference", attest_adult: true },
  ]);
  expect(sent.map((c) => c.headers["cf-turnstile-response"])).toEqual([undefined, undefined]);
  expect(new Set(sent.map((c) => c.headers["idempotency-key"])).size).toBe(2);
  takeProblem(pageProblems, "HTTP 429: /api/v1/live-runs");
  takeProblem(pageProblems, "HTTP 429: /api/v1/live-runs");
});

test("an API error shows its code's sentence and the Retry-After wait", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    guidedRun: {
      status: 429,
      body: { code: "rate_limited", message: "too many requests from this client", retry_after_secs: 30 },
      retryAfter: "30",
    },
  });
  await page.goto("/en/new/");
  await hydrated(page);
  await page.locator("[data-scenario='offline']").getByRole("button").click();
  const notice = page.locator("[data-error-code='rate_limited']");
  await expect(notice).toContainText(t("en", "Api.errors.rate_limited"));
  await expect(notice).toContainText("Retry in 30 seconds.");
  await expect(notice).toContainText("Code rate_limited, HTTP 429.");
  takeProblem(pageProblems, "HTTP 429: /api/v1/runs");
});

/**
 * Holds every `method` request to `pattern` until the returned release is
 * called; fallback() then hands it to the mock (registered before, so it
 * answers). Other methods pass straight through.
 */
async function holdRequests(page: Page, pattern: string, method: string): Promise<() => void> {
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(pattern, async (route) => {
    if (route.request().method() === method) {
      await held;
    }
    await route.fallback();
  });
  return () => release();
}

/** The button has the keyboard focus, is busy, and is not natively disabled (which would drop the focus). */
async function expectBusyAndFocused(button: Locator) {
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-disabled", "true");
  expect(await button.evaluate((el) => el.hasAttribute("disabled"))).toBe(false);
}

// Follow-up of A11Y-I18N-1: the buttons that send a request keep the
// keyboard focus while it is in flight (busy, never natively disabled), and
// after it the focus is on the same button or, when the form is replaced, on
// what replaced it: never dropped to <body>.
test("the guided Run button keeps the keyboard focus while its request is in flight and after a refusal", async ({
  page,
  pageProblems,
}) => {
  const mock = await mockApi(page, {
    guidedRun: {
      status: 429,
      body: { code: "rate_limited", message: "too many requests from this client", retry_after_secs: 30 },
      retryAfter: "30",
    },
  });
  const release = await holdRequests(page, "**/api/v1/runs", "POST");
  await page.clock.install();
  await page.goto("/en/new/");
  await hydrated(page);
  const button = page.locator("[data-scenario='offline']").getByRole("button");
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(button).toContainText(t("en", "NewDeliberation.guided.starting"));
  await expectBusyAndFocused(button);
  // A second press while it is busy sends nothing more.
  await page.keyboard.press("Enter");
  release();
  await expect(page.locator("[data-error-code='rate_limited']")).toBeVisible();
  // Round 4 (HE #18): the 30-second wait disables the button, so the notice
  // inside its card holds the focus until the wait is over, then gives it back.
  await expect(page.locator("[data-scenario='offline'] [data-scenario-error]")).toBeFocused();
  await expect(button).toBeDisabled();
  await page.clock.fastForward("00:31");
  await expect(button).toBeFocused();
  await expect(button).not.toHaveAttribute("aria-disabled", "true");
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe("BUTTON");
  expect(mock.calls.filter((c) => c.method === "POST" && c.path === "/api/v1/runs")).toHaveLength(1);
  takeProblem(pageProblems, "HTTP 429: /api/v1/runs");
  expect(pageProblems).toEqual([]);
});

test("the live Start button keeps the keyboard focus while its request is in flight and after a refusal", async ({
  page,
  pageProblems,
}) => {
  const mock = await mockApi(page, {
    liveEnabled: true,
    liveRequiresTurnstile: false,
    liveRuns: [
      {
        status: 429,
        body: { code: "live_concurrency", message: "every live slot is busy; retry shortly", retry_after_secs: 30 },
        retryAfter: "30",
      },
    ],
  });
  const release = await holdRequests(page, "**/api/v1/live-runs", "POST");
  await page.clock.install();
  await page.goto("/en/new/");
  await hydrated(page);
  const form = page.locator("[data-live='form']");
  await form.getByRole("radio", { name: "Fix payment client timeouts" }).check();
  await form.getByRole("checkbox", { name: t("en", "NewDeliberation.live.attest") }).check();
  const start = form.locator("button[type='submit']");
  await expect(start).toBeEnabled();
  await start.focus();
  await page.keyboard.press("Enter");
  await expect(start).toContainText(t("en", "NewDeliberation.live.starting"));
  await expectBusyAndFocused(start);
  release();
  const notice = page.locator("[data-error-code='live_concurrency']");
  await expect(notice).toBeVisible();
  // Round 4 (HE #18): the wait disables Start; the notice holds the focus
  // until it is over, then Start takes it back.
  await expect(form.locator("[data-live-error]")).toBeFocused();
  await expect(start).toBeDisabled();
  await page.clock.fastForward("00:31");
  await expect(start).toContainText(t("en", "NewDeliberation.live.start"));
  await expect(start).toBeFocused();
  // Sent from the invite field with Enter: that field is disabled while the
  // request is in flight, so the focus moves to the busy Start button.
  await page.getByText(t("en", "NewDeliberation.live.inviteSummary")).click();
  const invite = form.getByRole("textbox", { name: t("en", "NewDeliberation.live.inviteLabel") });
  await invite.fill("JUDGE-1");
  const releaseAgain = await holdRequests(page, "**/api/v1/live-runs", "POST");
  await invite.press("Enter");
  await expect(start).toContainText(t("en", "NewDeliberation.live.starting"));
  await expectBusyAndFocused(start);
  releaseAgain();
  await expect(start).toContainText(/^Try again in /u);
  await expect(form.locator("[data-live-error]")).toBeFocused();
  await page.clock.fastForward("00:31");
  await expect(start).toContainText(t("en", "NewDeliberation.live.start"));
  await expect(start).toBeFocused();
  await expect.poll(() => mock.calls.filter((c) => c.method === "POST" && c.path === "/api/v1/live-runs").length).toBe(2);
  takeProblem(pageProblems, "HTTP 429: /api/v1/live-runs");
  takeProblem(pageProblems, "HTTP 429: /api/v1/live-runs");
  expect(pageProblems).toEqual([]);
});

test("Approve keeps the keyboard focus while the decision is sent, and the recorded decision takes it after", async ({
  page,
}) => {
  const mock = await mockApi(page, { detail: awaitingDecisionDetail() });
  const release = await holdRequests(page, `**/api/v1/runs/${RUN_ID}/decision`, "POST");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const form = page.locator("[data-decision-form]");
  const approve = form.getByRole("button", { name: t("en", "Run.decision.approve") });
  await approve.focus();
  await page.keyboard.press("Enter");
  // The same button (its label now says the decision is being recorded).
  const busy = form.locator("button").nth(0);
  await expect(busy).toContainText(t("en", "Run.decision.submitting"));
  await expectBusyAndFocused(busy);
  // A second press while it is busy sends nothing more.
  await page.keyboard.press("Enter");
  release();
  const recorded = page.locator("[data-decision-recorded='approved']");
  await expect(recorded).toBeVisible();
  // The form (and its button) is gone: the recorded decision has the focus.
  await expect(recorded).toBeFocused();
  expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe("BODY");
  expect(mock.calls.filter((c) => c.method === "POST" && c.path.endsWith("/decision"))).toHaveLength(1);
});

test("Reject keeps the keyboard focus while the decision is sent and after a refusal", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    detail: awaitingDecisionDetail(),
    decision: { status: 503, body: { code: "shutting_down", message: "shutting down" } },
  });
  const release = await holdRequests(page, `**/api/v1/runs/${RUN_ID}/decision`, "POST");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const form = page.locator("[data-decision-form]");
  const reject = form.getByRole("button", { name: t("en", "Run.decision.reject") });
  await reject.focus();
  await page.keyboard.press("Enter");
  const busy = form.locator("button").nth(1);
  await expect(busy).toContainText(t("en", "Run.decision.submitting"));
  await expectBusyAndFocused(busy);
  release();
  await expect(page.locator("[data-error-code='shutting_down']")).toBeVisible();
  await expect(reject).toBeFocused();
  await expect(reject).not.toHaveAttribute("aria-disabled", "true");
  dropProblems(pageProblems, /HTTP 503: \/api\/v1\/runs\/.*\/decision/u);
  expect(pageProblems).toEqual([]);
});

/** The painted background of a tag: its text's element or the nearest ancestor with a fill. */
async function tagBackground(locator: Locator): Promise<string> {
  return locator.evaluate((element) => {
    let node: Element | null = element;
    for (let depth = 0; node !== null && depth < 4; depth += 1) {
      const color = getComputedStyle(node).backgroundColor;
      if (color !== "rgba(0, 0, 0, 0)" && color !== "transparent") {
        return color;
      }
      node = node.parentElement;
    }
    return "transparent";
  });
}

test("during a paced playback the header follows the stage, never the stored ending", async ({
  page,
  pageProblems,
}) => {
  // The retry run is approved and supported; its frames replay paced.
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await openRun(page, "en");
  await expect(page.locator("[data-pacing='on']")).toBeVisible();
  const header = page.locator("header.hc-run-header");
  await expect(header).toHaveAttribute("data-header-clock", "playback");
  await expect(page.locator("[data-header-playback]")).toBeVisible();
  // While the stage has no recommendation, neither has the header: no
  // outcome, no decision, and not the final state.
  await expect(page.locator("[data-recommendation]")).toContainText(t("en", "Run.stage.noRecommendation"));
  await expect(page.locator("[data-run-field='outcome']")).toHaveText(t("en", "Run.header.noOutcome"));
  await expect(page.locator("[data-run-field='decision']")).toHaveCount(0);
  await expect(page.locator("[data-run-field='state']")).not.toHaveText(t("en", "Run.states.approved"));
  await expect(header).not.toHaveAttribute("data-header-state", "approved");
  // The run's own state stays on the workspace for anything that needs it.
  await expect(page.locator("[data-run-state]")).toHaveAttribute("data-run-state", "approved");
  // The stream ends long before the pacer has shown its frames; the status
  // line runs on the header's clock: the playback continues, and it names
  // neither the final state nor a pending decision.
  const status = page.locator("[data-stream-state] > [role=status]");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(header).toHaveAttribute("data-header-clock", "playback");
  await expect(page.locator("[data-stream-line]")).toHaveAttribute("data-stream-line", "playing");
  await expect(status).toHaveText(t("en", "Run.stream.endedPlaying"));
  await expect(status).not.toContainText(t("en", "Run.stream.endReasons.terminal"));
  await expect(status).not.toContainText(t("en", "Run.stream.endReasons.awaiting_human_approval"));

  await page.getByRole("button", { name: t("en", "Run.pacing.skip") }).click();
  await expect(header).toHaveAttribute("data-header-clock", "run");
  await expect(page.locator("[data-stream-line]")).toHaveAttribute("data-stream-line", "ended");
  await expect(status).toHaveText(
    t("en", "Run.stream.ended", { reason: t("en", "Run.stream.endReasons.terminal") }),
  );
  await expect(page.locator("[data-header-playback]")).toHaveCount(0);
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Run.states.approved"));
  await expect(page.locator("[data-run-field='outcome']")).toContainText(t("en", "Run.kinds.supported"));
  await expect(page.locator("[data-run-field='decision']")).toHaveAttribute("data-decision", "approved");
  expect(pageProblems).toEqual([]);
});

test("a decided run's status line says it reached its final state, and its header names the decision", async ({
  page,
}) => {
  // The stored stream of the retry run ended at the decision point
  // (stream_end awaiting_human_approval); the run has been approved since.
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  const status = page.locator("[data-stream-state='ended'] [role=status]");
  await expect(status).toHaveText(
    t("en", "Run.stream.ended", { reason: t("en", "Run.stream.endReasons.terminal") }),
  );
  await expect(status).not.toContainText(t("en", "Run.stream.endReasons.awaiting_human_approval"));
  const decision = page.locator("[data-run-field='decision']");
  await expect(decision).toHaveAttribute("data-decision", "approved");
  await expect(decision.getByText(t("en", "Run.header.decisions.approved"), { exact: true })).toBeVisible();
  // The tag alone: the no-authority statement is the Decision tab's, not
  // repeated in the header.
  await expect(decision).toHaveText(t("en", "Run.header.decisions.approved"));
});

test("a scripted run whose stream sends no frame shows its workspace and stream status, not a loading line", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page);
  // The stream answers nothing: the request is held, so no frame arrives
  // and the stream is not settled, until the test lets it through.
  const held: Route[] = [];
  await page.route(/\/api\/v1\/runs\/[^/]+\/events(\?.*)?$/u, (route) => {
    held.push(route);
  });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await openRun(page, "en");
  await expect(page.locator("[data-run='loading']")).toHaveCount(0);
  await expect(page.locator("[data-run-state]")).toHaveAttribute("data-run-state", "approved");
  // The header shows nothing of the stored ending until the playback starts.
  const header = page.locator("header.hc-run-header");
  await expect(header).toHaveAttribute("data-header-clock", "playback");
  await expect(header).toHaveAttribute("data-header-state", "not_shown");
  const total = (fixture<{ summary: { event_count: number } }>("run-detail-retry.json")).summary.event_count;
  await expect(page.locator("[data-header-playback]")).toHaveText(
    String(new IntlMessageFormat(t("en", "Run.header.playback"), "en").format({ shown: 0, total })),
  );
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Run.header.notShown"));
  await expect(page.locator("[data-run-field='outcome']")).toHaveText(t("en", "Run.header.noOutcome"));
  await expect(page.locator("[data-run-field='decision']")).toHaveCount(0);
  await expect(page.locator("[data-run-field='revision']")).toHaveText(t("en", "Run.header.notShown"));
  await expect(page.locator("[data-run-field='records']")).toHaveText(tCountEn("Run.header.records", 0));
  // The stream's own status stays visible, and so do the run's tabs.
  await expect(page.locator("[data-stream-state='connecting'] > [role=status]")).toHaveText(
    t("en", "Run.stream.connecting"),
  );
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.evidence") })).toBeVisible();
  await expect(page.locator("[data-recommendation]")).toContainText(t("en", "Run.stage.noRecommendation"));
  // Once the stream answers, the playback starts and the header follows it;
  // the mocked stream (the earlier route) serves the run's frames.
  await page.unroute(/\/api\/v1\/runs\/[^/]+\/events(\?.*)?$/u);
  expect(held.length).toBeGreaterThan(0);
  for (const route of held.splice(0)) {
    await route.fallback();
  }
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(header).not.toHaveAttribute("data-header-state", "not_shown");
  await page.getByRole("button", { name: t("en", "Run.pacing.skip") }).click();
  await expect(header).toHaveAttribute("data-header-state", "approved");
  expect(pageProblems).toEqual([]);
});

test("after a rejection the header shows it as its own tag, never a grey word, and the status line agrees", async ({
  page,
  pageProblems,
}) => {
  const rejected = fixture<Record<string, unknown>>("run-detail-retry.json");
  const summary = rejected.summary as Record<string, unknown>;
  summary.state = "rejected";
  summary.authorization = "rejected";
  const recorded = rejected.decision as Record<string, unknown> | undefined;
  if (recorded !== undefined && recorded !== null) {
    recorded.approved = false;
  }
  await mockApi(page, { detail: awaitingDecisionDetail(), decidedDetail: rejected });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  const status = page.locator("[data-stream-state='ended'] [role=status]");
  await expect(status).toHaveText(
    t("en", "Run.stream.ended", { reason: t("en", "Run.stream.endReasons.awaiting_human_approval") }),
  );
  await expect(page.locator("[data-run-field='decision']")).toHaveCount(0);
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await page.locator("[data-decision-form]").getByRole("button", { name: t("en", "Run.decision.reject") }).click();
  await expect(page.locator("[data-decision-recorded='rejected']")).toBeVisible();
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Run.states.rejected"));
  const decision = page.locator("[data-run-field='decision']");
  await expect(decision).toHaveAttribute("data-decision", "rejected");
  const tag = decision.getByText(t("en", "Run.header.decisions.rejected"), { exact: true });
  await expect(tag).toBeVisible();
  // A tag with a fill of its own, not the grey of a neutral label.
  const neutral = page
    .locator("header.hc-run-header")
    .getByText(t("en", "Provenance.thisServer.label"), { exact: true })
    .first();
  const rejectedFill = await tagBackground(tag);
  expect(rejectedFill).not.toBe("transparent");
  expect(rejectedFill).not.toBe(await tagBackground(neutral));
  // The Playback tab's status line follows the run: the decision is recorded.
  await page.getByRole("tab", { name: t("en", "Run.tabs.playback") }).click();
  await expect(status).toHaveText(
    t("en", "Run.stream.ended", { reason: t("en", "Run.stream.endReasons.terminal") }),
  );
  expect(pageProblems).toEqual([]);
});

test("the guided cards label the model SCRIPTED in every locale, as the run pages do", async ({ page }) => {
  await mockApi(page);
  for (const locale of locales) {
    await page.goto(localePath(locale, "new"));
    await hydrated(page);
    const cards = page.locator("[data-scenario]");
    await expect(cards).toHaveCount(5);
    await expect(
      cards.getByText(t(locale, "Provenance.scripted.label"), { exact: true }),
    ).toHaveCount(5);
  }
});

test("the guided list is counted from what the server lists", async ({ page }) => {
  await mockApi(page);
  await page.goto("/en/new/");
  await hydrated(page);
  const count = page.locator("[data-scenario-count]");
  await expect(count).toHaveAttribute("data-scenario-count", "5");
  await expect(count).toHaveText("The server lists 5 guided scenarios.");
  await expect(page.locator("[data-scenario]")).toHaveCount(5);
});

test("My runs lists the session's runs with their retention dates", async ({ page }) => {
  const summaries = ["retry", "offline", "invalid_proof", "injection", "abstain"].map((name) =>
    fixture<Record<string, unknown>>(`run-summary-${name}.json`),
  );
  await mockApi(page, { runs: summaries });
  await page.goto("/en/runs/");
  await hydrated(page);
  await expect(page.locator("[data-run-count='5']")).toHaveText("5 runs");
  const cards = page.locator("[data-run-id]");
  await expect(cards).toHaveCount(5);
  const first = cards.first();
  await expect(first).toContainText("Fix payment client timeouts");
  await expect(first).toContainText(t("en", "Run.states.approved"));
  await expect(first.locator("[data-outcome-tag='outcome']")).toHaveText("supported");
  await expect(first.locator("[data-outcome-tag='evidence']")).toHaveText("Evidence: SIMULATED");
  await expect(first).toContainText("Access ends");
  await expect(first).toContainText("Deleted after");
  await expect(first.getByRole("link")).toHaveAttribute("href", runPath("en", RUN_ID));
  await expect(page.getByText(t("en", "Provenance.scripted.label"), { exact: true })).toHaveCount(5);
  await expect(liveBadge(page, "en")).toHaveCount(0);
});

test("My runs shows no list and no count without a session", async ({ page, pageProblems }) => {
  await mockApi(page, { session: false });
  await page.goto("/en/runs/");
  await hydrated(page);
  await expect(page.locator("[data-runs='no-session']")).toContainText(t("en", "Runs.noSession"));
  await expect(page.locator("[data-run-count]")).toHaveCount(0);
  await expect(page.locator("[data-run-id]")).toHaveCount(0);
  takeProblem(pageProblems, "HTTP 401: /api/v1/session");
});

test("the Explorer tab lists every record and filters by type", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.explorer") }).click();
  const rows = page.locator("[data-explorer] tbody tr");
  await expect(rows).toHaveCount(15);
  await expect(rows.first()).toContainText("task created");
  // The authority is translated, never the raw enumeration.
  await expect(rows.last()).toContainText(
    `approved by visitor:fixture0000000 (${t("en", "Run.explorer.authority.public_demo")})`,
  );
  await page.getByLabel(t("en", "Run.explorer.filter")).selectOption("evidence_verified");
  await expect(rows).toHaveCount(4);
  // A scripted proof reads SIMULATED with its outcome, never "integrity valid".
  await expect(rows.first()).toContainText(
    t("en", "Run.stage.simulatedProofShort", { status: t("en", "Provenance.proof.valid") }),
  );
  await expect(page.locator("[data-explorer]")).not.toContainText("integrity valid");
  await rows.first().getByText(t("en", "Run.explorer.json")).click();
  await expect(rows.first().locator("pre")).toContainText('"type": "evidence_verified"');
  // The JSON says "valid" for a simulated proof: a caption reads it on the spot (HE #2).
  await expect(rows.first().locator("[data-json-simulated]")).toHaveText(t("en", "Run.explorer.jsonSimulated"));
  // Zero matches read the =0 sentence, never "0 record".
  await page.getByLabel(t("en", "Run.explorer.filter")).selectOption("run_failed");
  await expect(page.locator("[data-explorer] .hc-list-head p")).toHaveText("0 records of 15");
});

test("the Explorer translates proof statuses and the authority (es, pt-BR)", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  // The =0 branch of Run.explorer.count (an ICU plural the t() helper does not format).
  const zeroOf15 = { es: "0 registros de 15", "pt-BR": "0 registros de 15" } as const;
  for (const locale of ["es", "pt-BR"] as const) {
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    await page.getByRole("tab", { name: t(locale, "Run.tabs.explorer") }).click();
    const rows = page.locator("[data-explorer] tbody tr");
    await expect(rows.last()).toContainText(t(locale, "Run.explorer.authority.public_demo"));
    await page.getByLabel(t(locale, "Run.explorer.filter")).selectOption("evidence_verified");
    await expect(rows.first()).toContainText(
      t(locale, "Run.stage.simulatedProofShort", { status: t(locale, "Provenance.proof.valid") }),
    );
    await expect(rows.first()).toContainText(t(locale, "Run.explorer.relevance.relevant"));
    await page.getByLabel(t(locale, "Run.explorer.filter")).selectOption("run_failed");
    await expect(page.locator("[data-explorer] .hc-list-head p")).toHaveText(zeroOf15[locale]);
  }
});

test("the Evidence tab lists each record with its coverage, text and verifications by round @smoke", async ({
  page,
}) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  await expect(page.locator("[data-evidence-id]")).toHaveCount(2);
  const constraint = page.locator("[data-evidence-id='ev-constraint']");
  await expect(constraint.locator("[data-coverage='simulated']")).toHaveText(
    t("en", "Run.evidence.coverage.simulated"),
  );
  await expect(constraint.locator("[data-evidence-field='retrieval']")).toHaveText(
    t("en", "Run.evidence.retrievalModes.simulated"),
  );
  // The record's text, as data, in English.
  await expect(constraint.locator("blockquote[lang='en']")).toHaveText(
    "Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage.",
  );
  await expect(constraint.locator("figcaption")).toHaveText(t("en", "Run.content.recordNote"));
  // Its verifications, round by round: SIMULATED, never a green valid.
  await expect(constraint.locator("[data-round]")).toHaveCount(2);
  await expect(constraint.locator("[data-proof-simulated='true']")).toHaveCount(2);
  await expect(constraint.locator("[data-proof-simulated='false']")).toHaveCount(0);
  await expect(constraint.locator("[data-round='1']")).toContainText(
    t("en", "Run.stage.simulatedProof", { status: t("en", "Provenance.proof.valid") }),
  );
  await expect(constraint.locator("[data-round='1']")).toContainText(
    t("en", "Run.evidence.factual", { status: t("en", "Run.explorer.factual.contradicted") }),
  );
  await expect(constraint.locator("[data-round='2']")).toContainText(
    t("en", "Run.evidence.factual", { status: t("en", "Run.explorer.factual.corroborated") }),
  );
  await expect(page.locator("[data-evidence-tab]")).not.toContainText("integrity valid");
  await expectNoLive(page, "en");
});

test("sealed-snapshot evidence says what its proof covers, from the proof's own request", async ({ page }) => {
  const detail = sealedSnapshotDetail();
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  const record = page.locator("[data-evidence-id='ev-fact']");
  await expect(record.locator("[data-coverage='snapshot_list']")).toHaveText(
    t("en", "Run.evidence.coverage.snapshot", { project: "demo-repo", anchor: SNAPSHOT_ANCHOR.slice(0, 12) }),
  );
  await expect(record.locator("[data-evidence-field='retrieval']")).toHaveText(
    t("en", "Run.evidence.retrievalModes.sealed_snapshot"),
  );
  await expect(record).toContainText("snapshot-fixture-1");
  // A real verifier's proof: the status token, the verifier and the time.
  await expect(record.locator("[data-proof-simulated='false'][data-proof-status='valid']")).toHaveCount(2);
  await expect(record.locator("[data-proof-reading='accepted']").first()).toContainText(
    "hyphae proof verify accepted the proof",
  );
  // Never the claim's query: the coverage names the proof's own list request.
  await expect(record).not.toContainText("Payment timeouts correlate");
  await expectNoLive(page, "en");
});

test("a live recall whose query is a claim's own text says the proof covers that recall, not the claim", async ({
  page,
}) => {
  // As the runtime records it: the recall is made with the claim statement.
  const claim = "Payment timeouts correlate with retry bursts above 5 attempts";
  const detail = sealedSnapshotDetail();
  for (const event of detail.events as Array<{ payload: Record<string, unknown> }>) {
    if (event.payload.type === "evidence_retrieved") {
      const evidence = event.payload.evidence as Record<string, unknown>;
      evidence.retrieval = "live_recall";
      delete evidence.snapshot_id;
      delete evidence.captured_at;
      evidence.proof_request = { operation: "recall", project: "demo-repo", query: claim, limit: 5 };
    }
  }
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  const coverage = page.locator("[data-evidence-id='ev-fact'] [data-coverage='request']");
  await expect(coverage.locator("p").first()).toHaveText(
    t("en", "Run.evidence.coverage.request", { anchor: SNAPSHOT_ANCHOR.slice(0, 12) }),
  );
  await expect(coverage.locator("[data-coverage-query-is-claim]")).toHaveText(
    t("en", "Run.evidence.coverage.queryIsClaim"),
  );
  await expect(coverage.locator("dd[lang='en']")).toHaveText(claim);
  // The sentence never denies the query the row under it shows.
  await expect(coverage).not.toContainText("the query that led to the retrieval");
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  const card = page.locator("[data-proof-sequence='7']");
  await expect(card.locator("[data-plain-line='covers']")).toHaveText(t("en", "Run.proofs.plain.coversRequestClaim"));
  await expectNoLive(page, "en");
});

test("record text keeps its English original note in another locale (pt-BR)", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "pt-BR");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("pt-BR", "Run.tabs.evidence") }).click();
  const figure = page.locator("[data-evidence-id='ev-fact'] figure[data-untrusted='record']");
  await expect(figure.locator("figcaption")).toHaveText(
    `${t("pt-BR", "Run.content.recordNote")} ${t("pt-BR", "Run.content.originalRecord")}`,
  );
  await expect(figure.locator("blockquote")).toHaveAttribute("lang", "en");
  await expectNoLive(page, "pt-BR");
});

test("the Proof inspector says a simulated proof checked nothing, worded from the receipt", async ({ page }) => {
  const mock = await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  // Nothing reads the receipt before a tab that shows it is opened.
  expect(mock.calls.filter((c) => c.path.endsWith("/receipt"))).toEqual([]);
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  await expect(page.locator("[data-proof-sequence]")).toHaveCount(4);
  await expect(page.locator("[data-limits-translated] li")).toHaveText([
    t("en", "Run.proofs.limits.valid"),
    t("en", "Run.proofs.limits.covers"),
    t("en", "Run.proofs.limits.simulated"),
    t("en", "Run.proofs.limits.notEstablished"),
  ]);
  const receipt = fixture<{ proof_limitations: string }>("receipt-v2-retry.json");
  await expect(page.locator("[data-proof-limitations]")).toHaveText(receipt.proof_limitations);
  const first = page.locator("[data-proof-sequence='7']");
  await expect(first.locator("[data-proof-plain='simulated']")).toHaveText(
    t("en", "Run.proofs.plain.simulated", { status: t("en", "Provenance.proof.valid") }),
  );
  await expect(first.locator("[data-proof-field='verifier'] code")).toHaveText("fake-verifier");
  await expect(first.locator("[data-proof-field='verifierKind']")).toContainText(t("en", "Run.proofs.fields.scripted"));
  await expect(page.locator("[data-proof-simulated='true']")).toHaveCount(4);
  await expect(page.locator("[data-proof-simulated='false']")).toHaveCount(0);
  await expectNoLive(page, "en");
  expect(mock.calls.filter((c) => c.path.endsWith("/receipt"))).toHaveLength(1);
});

test("the Proof inspector reads a real verifier's proof in plain words and technical fields", async ({ page }) => {
  const detail = sealedSnapshotDetail();
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  const card = page.locator("[data-proof-sequence='7']");
  const accepted = card.locator("[data-plain-line='accepted']");
  await expect(accepted).toContainText(
    "What was checked: hyphae proof verify re-executed the request this record’s proof sealed against its witness and accepted it at anchor d91ac10d17ce",
  );
  await expect(card.locator("[data-plain-line='covers']")).toHaveText(t("en", "Run.proofs.plain.coversSnapshot"));
  await expect(card.locator("[data-plain-line='notEstablished']")).toHaveText(
    t("en", "Run.proofs.plain.notEstablished"),
  );
  const field = (name: string) => card.locator(`[data-proof-field='${name}'] dd`);
  await expect(field("verifier")).toHaveText("hyphae proof verify");
  await expect(field("anchor")).toHaveText(SNAPSHOT_ANCHOR.slice(0, 12));
  await expect(field("reportKind")).toHaveText("memory");
  await expect(field("reportScope")).toHaveText("semantic_reexecution");
  await expect(field("proofDigest")).toHaveText("ab".repeat(6));
  await expect(field("resultDigest")).toHaveText("cd".repeat(6));
  await expect(field("round")).toHaveText("1");
  await expect(card.locator("[data-proof-simulated='false'][data-proof-status='valid']")).toHaveCount(1);
  await expectNoLive(page, "en");
});

test("a receipt that cannot be read is said with a retry, and the plain reading waits for it", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    receiptFailures: { count: 1, answer: { status: 500, body: { code: "internal", message: "internal error" } } },
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  const limits = page.locator("[data-proof-limits='error']");
  await expect(limits.locator("[data-error-code='internal']")).toContainText(t("en", "Api.errors.internal"));
  await expect(page.locator("[data-proof-plain='waiting']").first()).toHaveText(
    t("en", "Run.proofs.plainNeedsReceipt"),
  );
  await expect(page.getByText(t("en", "Run.proofs.receiptLoading"))).toHaveCount(0);
  await limits.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(page.locator("[data-limits-translated]")).toBeVisible();
  await expect(page.locator("[data-proof-plain='simulated']")).toHaveCount(4);
  takeProblem(pageProblems, `HTTP 500: /api/v1/runs/${RUN_ID}/receipt`);
});

test("a receipt with other limitations is shown as written, never paraphrased", async ({ page }) => {
  const receipt = { ...fixture<Record<string, unknown>>("receipt-v2-retry.json"), proof_limitations: "Proofs here mean something else." };
  await mockApi(page, { receipt });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  await expect(page.locator("[data-limits-untranslated]")).toHaveText(t("en", "Run.proofs.limitsUntranslated"));
  await expect(page.locator("[data-limits-translated]")).toHaveCount(0);
  await expect(page.locator("[data-proof-limitations]")).toHaveText("Proofs here mean something else.");
  await expect(page.locator("[data-proof-plain='waiting']").first()).toHaveText(
    t("en", "Run.proofs.plainUntranslated"),
  );
});

test("the Audit trail shows the chain, the receipt, the verbatim log and the read-only check @smoke", async ({
  page,
}) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  const detail = fixture<{ chain: { head_hash: string } }>("run-detail-retry.json");
  const chain = page.locator("[data-audit-chain='run']");
  await expect(chain.locator("[data-chain-scope='run']")).toHaveText(t("en", "Run.audit.scopes.run"));
  await expect(chain.locator("[data-chain-field='first']")).toHaveText("1");
  await expect(chain.locator("[data-chain-field='last']")).toHaveText("15");
  await expect(chain.locator("[data-chain-field='head']")).toHaveText(detail.chain.head_hash);
  // Each record's link as the public view carries it: a display copy.
  await chain.locator("[data-audit-links] > summary").click();
  await expect(chain.locator("[data-link-sequence]")).toHaveCount(15);
  await expect(chain.locator("[data-link-sequence='1']")).toContainText("genesis");
  await expect(chain.locator("[data-display-copy]")).toHaveText(t("en", "Run.audit.displayCopy"));
  // The verbatim log of a guided run, its receipt and the exact check.
  const log = page.locator("[data-audit-log='published'] [data-log-link]");
  await expect(log).toHaveAttribute("href", `/api/v1/runs/${RUN_ID}/events.ndjson`);
  await expect(log).toHaveAttribute("download", `events-${RUN_ID}.jsonl`);
  await expect(page.locator("[data-receipt-download]")).toHaveAttribute("href", `/api/v1/runs/${RUN_ID}/receipt`);
  await expect(page.locator("[data-cli]")).toHaveText(
    `consensus-api verify events-${RUN_ID}.jsonl --run ${RUN_ID} --receipt receipt-${RUN_ID}.json`,
  );
  // The receipt: its own head, the authorization, the deliberation, the proofs.
  const receipt = page.locator("[data-audit-receipt='ready']");
  await expect(receipt.locator("[data-receipt-field='event_count']")).toHaveText("15");
  await expect(receipt.locator("[data-receipt-field='head_hash']")).toHaveText(detail.chain.head_hash);
  await expect(receipt.locator("[data-receipt-authorization='approved']")).toContainText(
    t("en", "Run.explorer.authority.public_demo"),
  );
  await expect(receipt.locator("[data-receipt-never-rejudged='0']")).toContainText(
    "Earlier-round objections never re-judged: none",
  );
  await expect(receipt.locator("[data-receipt-simulated='4']")).toBeVisible();
  // Simulated proofs are never counted as "integrity valid": their line says
  // SIMULATED and not checked, on the same line (HE #2).
  await expect(receipt.locator("[data-receipt-by-status] li")).toHaveText([
    "SIMULATED, scripted “valid”: 4 proofs (not checked)",
  ]);
  await expect(receipt.locator("[data-receipt-by-status]")).not.toContainText("integrity valid");
  const limitations = fixture<{ proof_limitations: string }>("receipt-v2-retry.json").proof_limitations;
  await expect(receipt.locator("[data-receipt-limitations]")).toHaveText(limitations);
  await receipt.locator("details > summary").click();
  await expect(receipt.locator("details [data-display-copy]")).toHaveText(t("en", "Run.audit.displayCopy"));
  await expect(receipt.locator("details pre")).toContainText('"format": "hyphae-consensus-run-receipt-v2"');
  await expectNoLive(page, "en");
});

test("a guided run's verbatim log is checked in this browser; a changed record fails at its sequence", async ({
  page,
}) => {
  const chained = await chainedRun(awaitingDecisionDetail());
  const head = (chained.detail.chain as { head_hash: string }).head_hash;
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: chained.ndjson } });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const verdict = page.locator("[data-chain-verdict='pass']");
  await expect(verdict).toBeVisible();
  const result = (name: string) => verdict.locator(`[data-chain-result='${name}']`);
  // This server's log has no pinned digest; its head and count are the receipt's.
  await expect(result("sha256")).toContainText(t("en", "Run.chain.verdicts.not_compared"));
  await expect(result("head")).toContainText(head);
  await expect(result("head")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("count")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("range")).toContainText("sequences 1 to 14");
  await page.unrouteAll({ behavior: "ignoreErrors" });

  // The same log with one record's text changed (the fact record, sequence 4).
  const lines = chained.ndjson.toString("utf8").split("\n");
  expect(lines[3]).toContain('"sequence":4}');
  const tampered = Buffer.from(lines.map((line, i) => (i === 3 ? line.replace("Fact:", "Fake:") : line)).join("\n"));
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: tampered } });
  await openRun(page, "en");
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const failed = page.locator("[data-chain-verdict='fail']");
  await expect(failed.locator("[data-chain-failure='hash_mismatch'][data-chain-failure-sequence='4']")).toContainText(
    "The check stops at record 4 (line 4)",
  );
});

/** An English catalog plural with its count, formatted as next-intl formats it (the t() helper does not format plurals). */
const tCountEn = (key: string, count: number) => String(new IntlMessageFormat(t("en", key), "en").format({ count }));

// RECORDED-UX-2: the Audit trail opened before the decision read the receipt
// of the run awaiting it (14 records, pending); the decision appends record
// 15. The chain check reads the receipt again when it starts, before the log,
// so the intact 15-record log checks out against the receipt of the same run
// state, and the receipt shown follows the decision.
test("Audit before decide: after the decision the check compares the log with a receipt read with it", async ({
  page,
}) => {
  const before = await chainedRun(awaitingDecisionDetail());
  before.receipt.authorization = { state: "pending" };
  before.receipt.final_state = "awaiting_human_approval";
  const after = await chainedRun(fixture<Json>("run-detail-retry.json"));
  // The decision only appends: the first 14 records are the same bytes.
  expect(after.ndjson.subarray(0, before.ndjson.length).equals(before.ndjson)).toBe(true);
  const headAfter = (after.receipt.chain as { head_hash: string }).head_hash;
  const mock = await mockApi(page, {
    detail: before.detail,
    receipt: before.receipt,
    ndjson: { [RUN_ID]: before.ndjson },
    decidedDetail: after.detail,
    decidedReceipt: after.receipt,
    decidedNdjson: { [RUN_ID]: after.ndjson },
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  const receipt = page.locator("[data-audit-receipt='ready']");
  await expect(receipt.locator("[data-receipt-authorization='pending']")).toBeVisible();
  await expect(receipt.locator("[data-receipt-field='event_count']")).toHaveText("14");

  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await page.getByRole("button", { name: t("en", "Run.decision.approve") }).click();
  await expect(page.locator("[data-decision-recorded='approved']")).toBeVisible();

  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  // The run moved on, so the receipt shown was read again.
  await expect(receipt.locator("[data-receipt-authorization='approved']")).toBeVisible();
  await expect(receipt.locator("[data-receipt-field='event_count']")).toHaveText("15");
  const readsBefore = mock.calls.filter((c) => c.path === `/api/v1/runs/${RUN_ID}/receipt`).length;
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const verdict = page.locator("[data-chain-verdict='pass']");
  await expect(verdict).toBeVisible();
  const result = (name: string) => verdict.locator(`[data-chain-result='${name}']`);
  await expect(result("chain")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("head")).toContainText(headAfter);
  await expect(result("head")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("count")).toContainText(tCountEn("Run.chain.count", 15));
  await expect(result("count")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("range")).toContainText("sequences 1 to 15");
  await expect(result("range")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(verdict.locator("[data-chain-earlier]")).toHaveCount(0);
  await expect(verdict.locator(".hc-chain-verdict")).toContainText(
    tCountEn("Run.chain.passSentence", after.receipt.event_count as number),
  );
  // The panel says which receipt it compared: the one read when the check started.
  await expect(verdict.locator("[data-chain-receipt='read']")).toContainText(
    "Compared with the receipt read from the server when this check started, just before the log was downloaded",
  );
  // That receipt was read by the check itself, before the log was downloaded.
  const paths = mock.calls.map((c) => c.path);
  const receiptReads = paths.flatMap((p, i) => (p === `/api/v1/runs/${RUN_ID}/receipt` ? [i] : []));
  expect(receiptReads.length).toBe(readsBefore + 1);
  const download = paths.lastIndexOf(`/api/v1/runs/${RUN_ID}/events.ndjson`);
  expect(receiptReads.at(-1) ?? -1).toBeLessThan(download);
  expect(receiptReads.at(-1) ?? -1).toBeGreaterThan(paths.lastIndexOf(`/api/v1/runs/${RUN_ID}/decision`));
});

// RECORDED-UX-2: a run that moves on between the receipt read and the log
// download (the log is append-only) holds every record the receipt counts:
// the receipt and the detail's chain read as an earlier state of the same
// log, not as a difference, but only when their head is the hash of the
// record at their own last position.
test("a log that grew past the receipt and the detail reads as an extension, a rewritten one still fails", async ({
  page,
}) => {
  const before = await chainedRun(awaitingDecisionDetail());
  const after = await chainedRun(fixture<Json>("run-detail-retry.json"));
  const headBefore = (before.receipt.chain as { head_hash: string }).head_hash;
  await mockApi(page, { detail: before.detail, receipt: before.receipt, ndjson: { [RUN_ID]: after.ndjson } });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const verdict = page.locator("[data-chain-verdict='pass']");
  await expect(verdict).toBeVisible();
  const result = (name: string) => verdict.locator(`[data-chain-result='${name}']`);
  await expect(verdict.locator("[data-chain-earlier]")).toHaveText(t("en", "Run.chain.earlierSentence"));
  // The headline does not claim a plain match the rows below qualify: the segment matches or extends the copies.
  const records = after.receipt.event_count as number;
  const sentence = verdict.locator(".hc-chain-verdict");
  await expect(sentence).toContainText(tCountEn("Run.chain.passSentenceEarlier", records));
  await expect(sentence).not.toContainText(tCountEn("Run.chain.passSentence", records));
  // The sentence about the earlier copy points at the rows it names, which come after it.
  expect(
    await verdict.evaluate((panel) => {
      const note = panel.querySelector("[data-chain-earlier]");
      const rows = panel.querySelector("[data-chain-results]");
      return note !== null && rows !== null && (note.compareDocumentPosition(rows) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
    }),
  ).toBe(true);
  for (const name of ["head", "count", "range"]) {
    await expect(result(name)).toContainText(t("en", "Run.chain.verdicts.earlier"));
    await expect(result(name)).not.toContainText(t("en", "Run.chain.verdicts.fail"));
  }
  await expect(result("head")).toContainText("The receipt’s head is the hash of this run’s record 14 in the file");
  await expect(result("head")).toContainText("1 later record was appended");
  await expect(result("count")).toContainText("The receipt counts 14 records; the file holds 15 of this run");
  await expect(result("range")).toContainText("sequences 1 to 15");
  await expect(result("range")).toContainText("the file’s record 14 hashes to that chain’s head");
  expect(headBefore).not.toBe((after.receipt.chain as { head_hash: string }).head_hash);
  await page.unrouteAll({ behavior: "ignoreErrors" });

  // The same counts with the head of another record: a rewrite, never an extension.
  const forged = JSON.parse(JSON.stringify(before.receipt)) as Json;
  const lines = after.ndjson.toString("utf8").split("\n");
  const record13 = JSON.parse(lines[12] ?? "{}") as { hash: string };
  forged.chain = { ...(forged.chain as Json), head_hash: record13.hash };
  await mockApi(page, { detail: before.detail, receipt: forged, ndjson: { [RUN_ID]: after.ndjson } });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const failed = page.locator("[data-chain-verdict='fail']");
  await expect(failed).toBeVisible();
  await expect(failed.locator("[data-chain-result='head']")).toContainText(t("en", "Run.chain.verdicts.fail"));
  await expect(failed.locator("[data-chain-result='count']")).toContainText(t("en", "Run.chain.verdicts.fail"));
});

// CHAIN-UI-R1-1: the chain check reads the receipt itself; when that read
// fails, the receipt already shown in the Audit trail stays shown (the
// failure goes to the check, which compares no receipt and says so).
test("a failed receipt read of the chain check leaves the receipt shown in place", async ({ page, pageProblems }) => {
  const chained = await chainedRun(awaitingDecisionDetail());
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: chained.ndjson } });
  // Registered after mockApi, so it answers first: the first read is served
  // by the mock, every later one fails.
  let reads = 0;
  await page.route(`**/api/v1/runs/${RUN_ID}/receipt`, async (route) => {
    reads += 1;
    if (reads === 1) {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ code: "internal", message: "internal error" }),
    });
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  const receipt = page.locator("[data-audit-receipt='ready']");
  const count = String(chained.receipt.event_count as number);
  await expect(receipt.locator("[data-receipt-field='event_count']")).toHaveText(count);

  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const panel = page.locator("[data-chain-verdict]");
  await expect(panel).toBeVisible();
  expect(reads).toBe(2);
  await expect(panel.locator("[data-chain-receipt='unread']")).toHaveText(t("en", "Run.chain.receiptUnread"));
  await expect(panel.locator("[data-chain-result='head']")).toContainText(t("en", "Run.chain.verdicts.not_compared"));
  await expect(panel.locator("[data-chain-result='count']")).toContainText(t("en", "Run.chain.verdicts.not_compared"));
  // The receipt shown before the check is still there, not replaced by the error.
  await expect(receipt).toBeVisible();
  await expect(receipt.locator("[data-receipt-field='event_count']")).toHaveText(count);
  await expect(page.locator("[data-audit-receipt='error']")).toHaveCount(0);
  takeProblem(pageProblems, `HTTP 500: /api/v1/runs/${RUN_ID}/receipt`);
});

// A11Y-I18N-1 and A11Y-I18N-2: the button keeps the keyboard focus while the
// check runs (busy, never natively disabled), and one status region, mounted
// empty before the check, announces the progress and then the short verdict,
// once; the detailed results are not a live region.
test("the chain check keeps the focus on its button and announces only the short verdict, once", async ({ page }) => {
  const chained = await chainedRun(awaitingDecisionDetail());
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: chained.ndjson } });
  // Registered after mockApi, so it holds the log open; fallback() then
  // hands the request to the mock, which serves the log.
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(`**/api/v1/runs/${RUN_ID}/events.ndjson`, async (route) => {
    await held;
    await route.fallback();
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  const section = page.locator("[data-chain-check]");
  const announcement = section.locator("[data-chain-announcement]");
  // The region is there, empty, before anything is written into it.
  await expect(announcement).toHaveAttribute("role", "status");
  await expect(announcement).toHaveText("");
  const region = await announcement.elementHandle();
  const button = section.getByRole("button", { name: t("en", "Run.chain.run") });
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(section).toHaveAttribute("data-chain-check", "running");
  await expect(announcement).toHaveText(t("en", "Run.chain.running", { name: `events-${RUN_ID}.jsonl` }));
  // Busy, not disabled: the focus stays on the button.
  const focused = section.locator("button");
  await expect(focused).toBeFocused();
  expect(await focused.evaluate((el) => el.hasAttribute("disabled"))).toBe(false);
  await expect(focused).toHaveAttribute("aria-disabled", "true");
  release();
  await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
  await expect(section.getByRole("button", { name: t("en", "Run.chain.again") })).toBeFocused();
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe("BUTTON");
  // The same region holds the short verdict; the detailed results are outside any live region.
  await expect(announcement).toHaveText(t("en", "Run.chain.pass"));
  expect(await region?.evaluate((el) => el.isConnected && el.getAttribute("data-chain-announcement") !== null)).toBe(true);
  await expect(page.locator("[data-chain-verdict]")).not.toHaveAttribute("role", /.+/);
  const live = await section.evaluate((root) =>
    [...root.querySelectorAll("[role='status'], [role='alert'], [aria-live]")]
      .filter((el) => el.getAttribute("aria-live") !== "off")
      .map((el) => el.textContent ?? ""),
  );
  expect(live.filter((text) => text.includes(t("en", "Run.chain.pass")))).toEqual([t("en", "Run.chain.pass")]);
  expect(live.join(" ")).not.toContain(t("en", "Run.chain.limits"));
  expect(live.join(" ")).not.toContain((chained.receipt.chain as { head_hash: string }).head_hash);
  // Pressing it again keeps the focus and announces the verdict again (after the progress).
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
  await expect(section.getByRole("button", { name: t("en", "Run.chain.again") })).toBeFocused();
  await expect(announcement).toHaveText(t("en", "Run.chain.pass"));
  await page.unrouteAll({ behavior: "ignoreErrors" });

  // A changed record: the verdict and where the check stopped, nothing more.
  const lines = chained.ndjson.toString("utf8").split("\n");
  const tampered = Buffer.from(lines.map((line, i) => (i === 3 ? line.replace("Fact:", "Fake:") : line)).join("\n"));
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: tampered } });
  await openRun(page, "en");
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(announcement).toHaveText("");
  await section.getByRole("button", { name: t("en", "Run.chain.run") }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-chain-verdict='fail']")).toBeVisible();
  await expect(announcement).toContainText(`${t("en", "Run.chain.fail")}. The check stops at record 4 (line 4)`);
  await expect(announcement).not.toContainText(t("en", "Run.chain.limits"));
  await expect(section.getByRole("button", { name: t("en", "Run.chain.again") })).toBeFocused();
});

test("a live-declared run's Audit trail names the operator's log for the check", async ({ page }) => {
  await mockApi(page, { extraRuns: [liveR01Run()] });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en", LIVE_RUN_ID);
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-log-unavailable]")).toHaveText(t("en", "Run.audit.logUnavailable"));
  await expect(page.locator("[data-log-link]")).toHaveCount(0);
  await expect(page.locator("[data-chain-check]")).toHaveCount(0);
  // The receipt carries the sanitized view: it stays downloadable.
  await expect(page.locator("[data-receipt-download]")).toHaveAttribute("href", `/api/v1/runs/${LIVE_RUN_ID}/receipt`);
  await expect(page.locator("[data-audit-log='unavailable']")).toContainText(t("en", "Run.audit.cliOperator"));
  await expect(page.locator("[data-cli]")).toHaveText(
    `consensus-api verify events-${LIVE_RUN_ID}.jsonl --run ${LIVE_RUN_ID} --receipt receipt-${LIVE_RUN_ID}.json`,
  );
  await expectNoLive(page, "en");
});

test("the Meta API panel lists every invocation with its role; a scripted double made no call @smoke", async ({
  page,
}) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.meta") }).click();
  const rows = page.locator("[data-invocation-sequence]");
  await expect(rows).toHaveCount(6);
  await expect(rows.first().locator("td").nth(1)).toHaveText(t("en", "Run.explorer.roles.proposer"));
  await expect(rows.nth(1).locator("td").nth(1)).toHaveText(t("en", "Run.explorer.roles.critic"));
  await expect(page.locator("[data-invocation-scripted='true'] [data-model-note='scripted']")).toHaveCount(6);
  await expect(page.locator("[data-model-note='scripted']").first()).toHaveText(t("en", "Provenance.scripted.note"));
  // A scripted double's id is never shown as evidence of a call.
  await expect(page.locator("[data-meta-tab]")).not.toContainText("fake-propose-1");
  await expect(page.locator("[data-meta-no-call]")).toContainText("No model call was made");
  await expect(page.locator("[data-meta-no-cost]")).toHaveText(t("en", "Run.meta.noCost"));
  await expectNoLive(page, "en");
});

test("recorded Meta calls: request ids, tokens and totals computed from the rows", async ({ page }) => {
  const detail = metaCallsDetail();
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.meta") }).click();
  const first = page.locator("[data-invocation-sequence='2']");
  await expect(first).toContainText("chatcmpl-fixture-1");
  await expect(first).toContainText(t("en", "Run.meta.model", { model: "muse-wonder-1.0", provider: "meta" }));
  await expect(first).toContainText("1,000 ms");
  await expect(page.locator("[data-invocation-scripted='false']")).toHaveCount(6);
  await expect(page.locator("[data-meta-calls='6']")).toBeVisible();
  await expect(page.locator("[data-total='latency']")).toHaveText("21,000 ms (reported by 6 of 6 calls)");
  await expect(page.locator("[data-total='input']")).toHaveText("2,100 (reported by 6 of 6 calls)");
  await expect(page.locator("[data-total='output']")).toHaveText("210 (reported by 6 of 6 calls)");
  await expect(page.locator("[data-total='reasoning']")).toHaveText("0 (reported by 0 of 6 calls)");
  await expect(page.locator("[data-meta-operator-only]")).toHaveText(t("en", "Run.meta.operatorOnly"));
  // A recorded request id is evidence of a recorded call, never of anything live.
  await expectNoLive(page, "en");
});

test("the Decision tab explains the recommendation from its policy trace @smoke", async ({ page }) => {
  await mockApi(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const explanation = page.locator("[data-explanation]");
  await expect(explanation.locator("[data-explain-lead]")).toHaveText(
    t("en", "Run.explain.plain.rules.all_checks_passed", { kind: t("en", "Run.kinds.supported") }),
  );
  // The trace's counts, zeros in words (ICU plurals, rendered by the app);
  // this run's proofs are SIMULATED, so its proof count says so on the same
  // line, never "2 valid" (HE #2).
  await expect(explanation.locator("[data-explain-counts] li")).toHaveText([
    "The final evaluation assessed 2 claims; 2 were corroborated.",
    "Proofs of the final round, all SIMULATED: 2 simulated “valid”, none simulated “invalid”, none unavailable or failed (scripted, not checked).",
    "The final evaluation listed no objection as unresolved.",
  ]);
  await expect(explanation.locator("[data-explain-counts]")).not.toContainText("2 valid");
  // The recommendation opening the tab is never green on simulated evidence (HE #1).
  await expectNotGreen(page.locator("[data-decision] [data-recommendation-kind]").locator(OUTCOME_BADGE), "info");
  await expectNotGreen(page.locator("[data-decision] [data-recommendation-kind]").locator(EVIDENCE_BADGE), "purple");
  // The server's own rationale says "2 valid": a caption says those are scripted statuses.
  await expect(page.locator("[data-decision] > section [data-rationale-simulated]")).toHaveText(
    t("en", "Run.stage.rationaleSimulated"),
  );
  await expect(explanation.locator("[data-explain-reasons]")).toHaveCount(0);
  // What the records add: these proof statuses are SIMULATED.
  await expect(explanation.locator("[data-context='context.simulated']")).toHaveText(
    t("en", "Run.explain.context.simulated"),
  );
  // Simple level: the technical details start collapsed, and open on request.
  const details = explanation.locator("details.hc-details");
  await expect(details).not.toHaveAttribute("open");
  await details.locator("summary").click();
  await expect(explanation.locator("[data-technical-rule='all_checks_passed']")).toHaveText(
    t("en", "Run.explain.technical.rule", { rule: "all checks passed", id: "all_checks_passed", version: "1.2" }),
  );
  await expect(explanation.locator("[data-count-field='proofs_valid']")).toHaveText("2");
  // The raw counts keep their field names; a caption says they are SIMULATED.
  await expect(explanation.locator("[data-counts-simulated]")).toHaveText(t("en", "Run.explain.technical.countsSimulated"));
  await expect(explanation.locator("figure[data-untrusted='server'] blockquote")).toHaveText(
    "Supported: 2/2 claims corroborated; proofs: 2 valid, 0 invalid, 0 unavailable/error; 0 unresolved challenges.",
  );
  await expectNoLive(page, "en");
});

test("at the technical level the trace starts expanded, every failed check with its ids", async ({ page }) => {
  await page.addInitScript(() => window.localStorage.setItem("hc-explanation", "technical"));
  const blocked = fixture<Record<string, unknown>>("run-detail-invalid_proof.json");
  const runId = (blocked.summary as { run_id: string }).run_id;
  await mockApi(page, { extraRuns: [storedRun(blocked)] });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en", runId);
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const explanation = page.locator("[data-explanation]");
  // A guided run: its "invalid" proof is the scripted verifier's, said as
  // SIMULATED and not checked, never as a check that came back invalid (HE #2).
  await expect(explanation.locator("[data-explain-lead]")).toHaveText(
    t("en", "Run.explain.plain.rules.invalid_proof_simulated", { kind: t("en", "Run.kinds.blocked") }),
  );
  await expect(explanation).not.toContainText(
    t("en", "Run.explain.plain.rules.invalid_proof", { kind: t("en", "Run.kinds.blocked") }),
  );
  await expect(explanation.locator("[data-explain-counts] li").nth(1)).toContainText("simulated “invalid”");
  await expect(explanation.locator("[data-explain-reasons] li")).toHaveText([
    t("en", "Run.explain.plain.reasons.invalid_proofs_simulated"),
    t("en", "Run.explain.plain.reasons.contradicted_evidence"),
    t("en", "Run.explain.plain.reasons.open_items"),
    t("en", "Run.explain.plain.reasons.uncorroborated_claims"),
    t("en", "Run.explain.plain.reasons.cited_evidence_not_valid"),
  ]);
  await expect(explanation.locator("details.hc-details")).toHaveAttribute("open");
  const reasons = explanation.locator("[data-technical-reasons] li");
  await expect(reasons).toHaveCount(5);
  await expect(reasons.first()).toContainText("ev-constraint");
  await expect(explanation.locator("[data-technical-reasons] [data-reason='open_items']")).toContainText(
    "item-1, item-2",
  );
});

test("a run before protocol 1.3 says how many earlier-round objections were never re-judged", async ({ page }) => {
  await mockApi(page, { detail: beforeCarryForwardDetail() });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const context = page.locator("[data-explain-context]");
  await expect(context.locator("[data-context='context.finalRoundOnly']")).toHaveText(
    t("en", "Run.explain.context.finalRoundOnly"),
  );
  await expect(context.locator("[data-context='context.neverRejudged']")).toHaveText(
    "Earlier-round objections never re-judged: 2.",
  );
  // Both were listed unresolved by their own round: nothing more is said.
  await expect(context.locator("[data-context='context.neverRejudgedUnlisted']")).toHaveCount(0);
});

test("an objection its own round left unlisted too is said apart, on the Decision and Audit tabs", async ({ page }) => {
  // Round 1's evaluation listed item-1 only; item-2 was left out without a stated reason.
  const detail = beforeCarryForwardDetail();
  const first = (detail.rounds as Array<{ round: number; evaluation: { unresolved_items: string[] } }>).find(
    (r) => r.round === 1,
  );
  if (first === undefined) {
    throw new Error("no round 1");
  }
  first.evaluation.unresolved_items = ["item-1"];
  const receipt = fixture<Record<string, unknown>>("receipt-v2-retry.json");
  const items = detail.challenge_items as Array<Record<string, unknown>>;
  (receipt.deliberation as Record<string, unknown>).earlier_round_objections_never_rejudged = items.map((item) => ({
    id: item.id,
    raised_in_round: item.raised_in_round,
    kind: item.kind,
    claim_id: item.claim_id,
    description: item.description,
    unresolved_in_its_round: item.id === "item-1",
  }));
  await mockApi(page, { detail, receipt });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const context = page.locator("[data-explain-context]");
  await expect(context.locator("[data-context='context.neverRejudged']")).toHaveText(
    "Earlier-round objections never re-judged: 2.",
  );
  await expect(context.locator("[data-context='context.neverRejudgedUnlisted']")).toHaveText(
    "Of them, 1 was not listed as unresolved even by the evaluation of its own round, which left it out without a stated reason.",
  );
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  const audit = page.locator("[data-audit-receipt='ready']");
  await expect(audit.locator("[data-receipt-never-rejudged='2']")).toHaveText(
    "Earlier-round objections never re-judged: 2",
  );
  await expect(audit.locator("[data-receipt-never-rejudged-unlisted='1']")).toHaveText(
    "Of them, 1 was not listed as unresolved even by the evaluation of its own round, which left it out without a stated reason",
  );
  await expectNoLive(page, "en");
});

for (const locale of ["es", "pt-BR", "fr"] as const) {
  test(`the explanation is translated, the kind in the locale's word (${locale})`, async ({ page }) => {
    await mockApi(page);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    await page.getByRole("tab", { name: t(locale, "Run.tabs.decision") }).click();
    await expect(page.locator("[data-explain-lead]")).toHaveText(
      t(locale, "Run.explain.plain.rules.all_checks_passed", { kind: t(locale, "Run.kinds.supported") }),
    );
    await expect(page.locator("[data-context='context.simulated']")).toHaveText(
      t(locale, "Run.explain.context.simulated"),
    );
    await expectNoLive(page, locale);
  });
}

test("the scope change sits under the recommendation it qualifies", async ({ page }) => {
  await mockApi(page, { detail: awaitingDecisionDetail() });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(page.locator("[data-outcome-revision]")).toHaveText(
    t("en", "Run.header.forRevision", { id: "prop-2" }),
  );
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const panel = page.locator(".hc-panel", { has: page.locator("[data-recommendation-kind]") }).first();
  await expect(panel.locator("[data-superseded-line]")).toHaveText(
    t("en", "Run.decision.superseded", { revised: "prop-2", original: "prop-1" }),
  );
  await expect(panel).toContainText(
    t("en", "Run.decision.supersededTask", { title: "Fix payment client timeouts" }),
  );
  // A guided run's verbatim log is published: the link is offered.
  await expect(page.locator("[data-ndjson-link]")).toHaveAttribute(
    "href",
    `/api/v1/runs/${RUN_ID}/events.ndjson`,
  );
});

test("the revision view compares the original and the revised proposal claim by claim", async ({ page }) => {
  await mockApi(page, { detail: awaitingDecisionDetail() });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  // Next to the scope line: the claims of prop-1 and prop-2, by claim id.
  const compare = page.locator("[data-superseded] [data-revision-compare]");
  // The scripted revision reuses claim-1 and claim-2: paired by id.
  await expect(compare).toHaveAttribute("data-claim-pairing", "id");
  await expect(compare.locator("h4")).toHaveText(
    t("en", "Run.revision.heading", { original: "prop-1", revised: "prop-2" }),
  );
  await expect(compare.locator("tbody tr")).toHaveCount(2);
  const first = compare.locator("[data-claim-id='claim-1'][data-claim-change='changed']");
  await expect(first.locator("td").nth(0)).toHaveText("Payment timeouts are caused by insufficient retries");
  await expect(first.locator("td").nth(1)).toHaveText("Payment timeouts correlate with retry bursts above 5 attempts");
  await expect(first.locator("td").nth(2)).toHaveText(t("en", "Run.revision.kinds.changed"));
  await expect(first.locator("td [lang='en']")).toHaveCount(2);
  await expect(compare.locator("[data-claim-id='claim-2'][data-claim-change='changed']")).toContainText(
    "Capping retries at 3 attempts with jitter matches the documented constraint",
  );
  // The revision's own claim of what it addresses, never a verdict.
  await expect(compare.locator("[data-revision-steps] li")).toHaveText(
    t("en", "Run.revision.step", { id: "prop-2", supersedes: "prop-1", items: "item-1, item-2" }),
  );
  // Every id is in both proposals: nothing is only in one of them.
  await expect(compare.locator("[data-claim-pairing-partial]")).toHaveCount(0);
  await expectNoLive(page, "en");
});

test("a revision that shares only some claim ids says that ids alone cannot tell a dropped claim from a restated one", async ({
  page,
}) => {
  const detail = awaitingDecisionDetail();
  // The scripted revision keeps claim-1 and gives its second claim a new id.
  for (const event of detail.events as Json[]) {
    const payload = event.payload as Json;
    if (payload.type === "revision_submitted") {
      const proposal = (payload.revision as Json).proposal as Json;
      ((proposal.claims as Json[])[1] as Json).id = "claim-9";
      detail.current_proposal = proposal;
    }
  }
  await mockApi(page, { detail });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const compare = page.locator("[data-superseded] [data-revision-compare]");
  await expect(compare).toHaveAttribute("data-claim-pairing", "id");
  await expect(compare.locator("[data-claim-id='claim-9'][data-claim-change='added']")).toHaveCount(1);
  await expect(compare.locator("[data-claim-id='claim-2'][data-claim-change='removed']")).toHaveCount(1);
  await expect(compare.locator("[data-claim-pairing-partial]")).toHaveText(t("en", "Run.revision.partialIds"));
  await expectNoLive(page, "en");
});

test("a live-declared run offers no verbatim log link and is never named Live", async ({ page }) => {
  await mockApi(page, { extraRuns: [liveR01Run()] });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en", LIVE_RUN_ID);
  // Active, but no model call recorded yet: a playback, not LIVE.
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.playback") })).toBeVisible();
  await expectNoLive(page, "en");
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-receipt-link]")).toBeVisible();
  await expect(page.locator("[data-ndjson-link]")).toHaveCount(0);
  await expect(page.locator("[data-ndjson-unavailable]")).toHaveText(
    t("en", "Run.decision.ndjsonUnavailable"),
  );
});

test("a failed detail request is shown with a retry, never as loading", async ({ page, pageProblems }) => {
  await mockApi(page, {
    detailFailures: { count: 2, answer: { status: 500, body: { code: "internal", message: "internal error" } } },
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  const failure = page.locator("[data-run='detail-error']");
  await expect(failure.locator("[data-error-code='internal']")).toContainText(t("en", "Api.errors.internal"));
  // The header keeps the newest summary the stream delivered.
  await expect(page.locator("[data-run-field='state']")).toHaveText(
    t("en", "Run.states.awaiting_human_approval"),
  );
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-decision-unavailable='detail-error']")).toHaveText(
    t("en", "Run.decision.detailUnavailable"),
  );
  await expect(page.getByText(t("en", "Run.loading"))).toHaveCount(0);
  await page.getByRole("tab", { name: t("en", "Run.tabs.explorer") }).click();
  await expect(page.locator("[data-explorer-partial]")).toBeVisible();
  await failure.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(page.locator("[data-run='detail-error']")).toHaveCount(0);
  await expect(page.locator("[data-explorer-partial]")).toHaveCount(0);
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-decision-recorded='approved']")).toBeVisible();
  takeProblem(pageProblems, `HTTP 500: /api/v1/runs/${RUN_ID}`);
  takeProblem(pageProblems, `HTTP 500: /api/v1/runs/${RUN_ID}`);
});

test("a failed stream: the stage shows the stored records and offers a reconnect", async ({
  page,
  pageProblems,
}) => {
  const mock = await mockApi(page, {
    streamFailures: {
      count: 1,
      answer: {
        status: 429,
        body: { code: "too_many_streams", message: "too many streams", retry_after_secs: 30 },
        retryAfter: "30",
      },
    },
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='error']")).toBeVisible();
  await expect(page.locator("[data-stage-source='stored']")).toHaveText(t("en", "Run.stream.storedRecords"));
  // The stage counts the run's own records, as the header does.
  await expect(page.locator("[data-lane]")).toContainText("2 records retrieved");
  await expect(page.locator("[data-lane-proofs='simulated']")).toHaveText("4 simulated proofs (not checked)");
  await expect(page.locator("[data-lane]")).not.toContainText("proofs checked");
  await expect(page.locator("[data-role-card='Proposer']")).toContainText(
    "Cap retries at 3 attempts with jitter per the documented constraint",
  );
  await expect(page.locator("[data-frame-delivery='replay']").first()).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.stream.reconnect") }).click();
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  expect(mock.calls.filter((c) => c.path.startsWith(`/api/v1/runs/${RUN_ID}/events`))).toHaveLength(2);
  takeProblem(pageProblems, `HTTP 429: /api/v1/runs/${RUN_ID}/events`);
  dropProblems(pageProblems, /EventSource/);
});

for (const locale of ["en", "es"] as const) {
  test(`a failed run reads the sentence of its failure code, never the server's message (${locale})`, async ({
    page,
  }) => {
    const english = "The run failed (budget_refused).";
    const run = failedDetail("budget_refused", english);
    await mockApi(page, { detail: run.detail, frames: run.frames, runs: [run.detail.summary as Record<string, unknown>] });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    const failure = page.locator("[data-failure-code='budget_refused']");
    await expect(failure).toContainText(t(locale, "Run.failure.codes.budget_refused"));
    await expect(failure).toContainText(t(locale, "Run.failure.detail", { stage: "evaluate", code: "budget_refused" }));
    // What the page shows (not the raw records under a closed disclosure).
    await expect(page.locator("main")).not.toContainText(english, { useInnerText: true });
    // A failed run has no recommendation and never will: nothing reads "yet".
    await expect(page.locator("[data-run-field='outcome']")).toHaveText(
      t(locale, "Run.header.noOutcomeFailed"),
    );
    await expect(page.locator("[data-recommendation]")).toContainText(
      t(locale, "Run.stage.noRecommendationFailed"),
    );
    await page.getByRole("tab", { name: t(locale, "Run.tabs.decision") }).click();
    await expect(page.locator("[data-recommendation-none='failed']")).toHaveText(
      t(locale, "Run.decision.failedNoRecommendation"),
    );
    await page.getByRole("tab", { name: t(locale, "Run.tabs.explorer") }).click();
    await page.getByLabel(t(locale, "Run.explorer.filter")).selectOption("run_failed");
    const summary = page.locator("[data-explorer] tbody tr td:last-child > span");
    await expect(summary).toContainText(t(locale, "Run.failure.codes.budget_refused"));
    await expect(summary).not.toContainText(english);
    await page.goto(localePath(locale, "runs"));
    await hydrated(page);
    await expect(page.locator("[data-run-failure='budget_refused']")).toHaveText(
      t(locale, "Run.failure.codes.budget_refused"),
    );
    await expect(page.locator("[data-run-id]")).toContainText(t(locale, "Run.header.noOutcomeFailed"));
  });
}

test("a failure code without a sentence still names its code", async ({ page }) => {
  const english = "The run failed (a_new_failure_code).";
  const run = failedDetail("a_new_failure_code", english);
  await mockApi(page, { detail: run.detail, frames: run.frames });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "fr");
  await expect(page.locator("[data-failure-code='a_new_failure_code']")).toContainText(
    t("fr", "Run.failure.unknownCode", { code: "a_new_failure_code" }),
  );
  await expect(page.locator("main")).not.toContainText(english, { useInnerText: true });
});

test("the run page without an id, and with a malformed one, says so", async ({ page }) => {
  await mockApi(page);
  await page.goto("/en/run/");
  await hydrated(page);
  await expect(page.locator("[data-run='no-id']")).toContainText(t("en", "Run.noId"));
  await page.goto("/en/run/?id=../etc");
  await hydrated(page);
  await expect(page.locator("[data-run='no-id']")).toContainText(t("en", "Run.invalidId"));
});

test("without an API the views show the runtime NotConnected state", async ({ page }) => {
  // No mock: the static server answers 404 (its HTML not-found page) for /api/v1.
  await page.goto("/en/new/");
  await hydrated(page);
  const gate = page.locator("[data-api-gate='unreachable']");
  await expect(gate.getByText(t("en", "NotConnected.badge"), { exact: true })).toBeVisible();
  await expect(gate).toContainText(t("en", "NotConnected.message"));
  await expect(gate).toContainText(t("en", "NotConnected.unreachable", { code: "unexpected_status" }));
  await expect(page.locator("[data-scenario]")).toHaveCount(0);
  await page.goto("/en/");
  await hydrated(page);
  await expect(
    page.getByRole("link", { name: t("en", "Overview.primaryActionPlanned") }),
  ).toHaveAttribute("href", "/en/docs/modes/");
  await expect(page.getByText(t("en", "Overview.primaryActionNote"))).toBeVisible();
});

test("with an API the Overview offers a guided example and names the server's versions", async ({
  page,
}) => {
  await mockApi(page);
  await page.goto("/en/");
  await hydrated(page);
  await expect(page.getByRole("link", { name: t("en", "Overview.primaryAction") })).toHaveAttribute(
    "href",
    "/en/new/",
  );
  await expect(page.locator("[data-api-status='connected']")).toContainText("schema 1.3, policy 1.2");
  await expect(page.getByText(t("en", "Overview.primaryActionNote"))).toHaveCount(0);
  await expect(page.locator("[data-contract-mismatch]")).toHaveCount(0);
});

test("the Overview promises a guided example only when the server enables them", async ({ page }) => {
  await mockApi(page, { capabilities: { guided_enabled: false } });
  await page.goto("/en/");
  await hydrated(page);
  await expect(page.getByRole("link", { name: t("en", "Overview.primaryActionNeutral") })).toHaveAttribute(
    "href",
    "/en/new/",
  );
  await expect(page.getByRole("link", { name: t("en", "Overview.primaryAction") })).toHaveCount(0);
});

test("a server on another contract is named as such", async ({ page }) => {
  const server = "0".repeat(64);
  await mockApi(page, { health: { contract_sha256: server } });
  const note = t("en", "ApiGate.contractMismatch", {
    build: CONTRACT_SHA256.slice(0, 12),
    server: server.slice(0, 12),
  });
  await page.goto("/en/");
  await hydrated(page);
  await expect(page.locator("[data-contract-mismatch]")).toHaveText(note);
  await page.goto("/en/new/");
  await hydrated(page);
  await expect(page.locator("[data-api-gate='ok'] [data-contract-mismatch]")).toHaveText(note);
});

for (const locale of locales) {
  test(`the API views are translated (${locale})`, async ({ page }) => {
    await mockApi(page, { liveEnabled: true });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(localePath(locale, "new"));
    await hydrated(page);
    await expect(
      page.getByRole("heading", { name: t(locale, "NewDeliberation.guided.heading") }),
    ).toBeVisible();
    await expect(page.getByText(t(locale, "NewDeliberation.live.turnstileMissing"))).toBeVisible();
    await page.goto(localePath(locale, "runs"));
    await hydrated(page);
    // Runs.count is an ICU plural, which the e2e t() helper does not format.
    await expect(page.locator("[data-run-count='1']")).toContainText("1");
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    await expect(page.getByRole("tab", { name: t(locale, "Run.tabs.playback") })).toBeVisible();
    await expect(page.locator("[data-recommendation]")).toContainText(t(locale, "Run.kinds.supported"));
    await expect(page.locator("[data-recommendation]")).toContainText(
      t(locale, "Provenance.simulated.label"),
    );
    await expect(liveBadge(page, locale)).toHaveCount(0);
  });
}

const themes = ["light", "dark"] as const;

async function scan(page: Page, name: string): Promise<ScanSummary> {
  const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
  return summarize(name, results);
}

for (const locale of locales) {
  for (const theme of themes) {
    test(`axe: /new, /runs and /run connected (${locale}, ${theme})`, async ({ page }) => {
      test.setTimeout(240_000);
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await mockApi(page, { liveEnabled: true, detail: awaitingDecisionDetail() });
      const scans: ScanSummary[] = [];

      await page.goto(localePath(locale, "new"));
      await hydrated(page);
      await expect(page.locator("[data-live='form']")).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      scans.push(await scan(page, `${locale} ${theme} /new (connected)`));

      await page.goto(localePath(locale, "runs"));
      await hydrated(page);
      await expect(page.locator("[data-run-id]").first()).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /runs (connected)`));

      await openRun(page, locale);
      await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run stage`));
      await page.getByRole("tab", { name: t(locale, "Run.tabs.explorer") }).click();
      scans.push(await scan(page, `${locale} ${theme} /run explorer`));
      await page.getByRole("tab", { name: t(locale, "Run.tabs.evidence") }).click();
      await expect(page.locator("[data-evidence-id]").first()).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run evidence`));
      await page.getByRole("tab", { name: t(locale, "Run.tabs.proofs") }).click();
      await expect(page.locator("[data-limits-translated]")).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run proofs`));
      await page.getByRole("tab", { name: t(locale, "Run.tabs.audit") }).click();
      await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
      await page.locator("[data-audit-links] > summary").click();
      await expect(page.locator("[data-link-sequence]").first()).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run audit`));
      await page.getByRole("tab", { name: t(locale, "Run.tabs.meta") }).click();
      await expect(page.locator("[data-invocation-sequence]").first()).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run meta`));
      await page.getByRole("tab", { name: t(locale, "Run.tabs.decision") }).click();
      await expect(page.locator("[data-decision-form]")).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run decision`));

      await writeScans(`api-${locale}-${theme}`, scans);
      expect(scans).toHaveLength(9);
      expect(blocking(scans)).toEqual([]);
    });
  }
}

for (const theme of themes) {
  test(`axe: the open live form, a failed run and a failed detail (en, ${theme})`, async ({
    page,
    pageProblems,
  }) => {
    test.setTimeout(240_000);
    await useTheme(page, theme);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    const scans: ScanSummary[] = [];

    await mockApi(page, { liveEnabled: true, liveRequiresTurnstile: false });
    await page.goto(localePath("en", "new"));
    await hydrated(page);
    await expect(page.locator("[data-live-state='open']")).toBeVisible();
    scans.push(await scan(page, `en ${theme} /new (live form open)`));
    await page.unrouteAll({ behavior: "ignoreErrors" });

    const failed = failedDetail("meta_timeout", "The run failed (meta_timeout).");
    await mockApi(page, { detail: failed.detail, frames: failed.frames });
    await openRun(page, "en");
    await expect(page.locator("[data-failure-code='meta_timeout']")).toBeVisible();
    scans.push(await scan(page, `en ${theme} /run failed`));
    await page.unrouteAll({ behavior: "ignoreErrors" });

    await mockApi(page, {
      detailFailures: { count: 2, answer: { status: 500, body: { code: "internal", message: "internal error" } } },
    });
    await openRun(page, "en");
    await expect(page.locator("[data-run='detail-error']")).toBeVisible();
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    scans.push(await scan(page, `en ${theme} /run detail error`));

    const failed500 = `HTTP 500: /api/v1/runs/${RUN_ID}`;
    await expect.poll(() => pageProblems.filter((p) => p === failed500).length).toBe(2);
    takeProblem(pageProblems, `HTTP 500: /api/v1/runs/${RUN_ID}`);
    takeProblem(pageProblems, `HTTP 500: /api/v1/runs/${RUN_ID}`);
    await writeScans(`api-states-en-${theme}`, scans);
    expect(scans).toHaveLength(3);
    expect(blocking(scans)).toEqual([]);
  });
}

// Heuristic evaluation, problem 5: a chain check that fails in this browser
// travels. It raises a failed callout under the header's labels on every tab
// and next to the decision, until a later check does not fail; it is a
// state of this page, not a label of the run.
test("a chain check that fails in this browser travels to the header and to the decision", async ({ page }) => {
  const chained = await chainedRun(awaitingDecisionDetail());
  const lines = chained.ndjson.toString("utf8").split("\n");
  const tampered = Buffer.from(lines.map((line, i) => (i === 3 ? line.replace("Fact:", "Fake:") : line)).join("\n"));
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: tampered } });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  const header = page.locator("header.hc-run-header");
  // Before any check: nothing says the chain is broken.
  await expect(page.locator("[data-chain-broken]")).toHaveCount(0);
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  await expect(page.locator("[data-chain-verdict='fail']")).toBeVisible();
  // The header says it at once, in the failed role, with the record it stopped at.
  const broken = header.locator("[data-chain-broken='header']");
  await expect(broken).toBeVisible();
  await expect(broken).toHaveAttribute("data-callout-tone", "failed");
  await expect(broken).toHaveAttribute("data-chain-broken-sequence", "4");
  await expect(broken.locator(".hc-callout-title")).toHaveText(t("en", "Run.chainBroken.title"));
  await expect(broken.locator("svg[aria-hidden='true']")).toHaveCount(1);
  await expect(broken).toContainText("failed at record 4 (line 4)");
  // The reason is the verifier's own: an altered record is a hash mismatch.
  await expect(broken).toHaveAttribute("data-chain-broken-problem", "hash_mismatch");
  await expect(broken).toContainText("is not the SHA-256 recomputed here");
  await expect(broken).toContainText(t("en", "Run.chainBroken.beforeDeciding"));
  await expect(broken).toContainText(t("en", "Run.chainBroken.pageState"));
  // Inside the Audit trail there is no button to open it.
  await expect(broken.getByRole("button", { name: t("en", "Run.chainBroken.openAudit") })).toHaveCount(0);

  // The Decision tab: the header callout stays, and the decision carries its own, next to the buttons.
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-decision-form]")).toBeVisible();
  await expect(broken).toBeVisible();
  const atDecision = page.locator("[data-decision] [data-chain-broken='decision']");
  await expect(atDecision).toBeVisible();
  await expect(atDecision).toHaveAttribute("data-callout-tone", "failed");
  await expect(atDecision).toContainText("failed at record 4 (line 4)");
  await expect(atDecision).toContainText(t("en", "Run.chainBroken.beforeDeciding"));
  // Every other tab keeps the header callout.
  for (const tab of ["explorer", "evidence", "proofs", "meta", "playback"] as const) {
    await page.getByRole("tab", { name: t("en", `Run.tabs.${tab}`) }).click();
    await expect(page.locator(`[data-tab='${tab === "playback" ? "stage" : tab}']`)).toBeVisible();
    await expect(broken).toBeVisible();
  }
  // Its button opens the Audit trail.
  await broken.getByRole("button", { name: t("en", "Run.chainBroken.openAudit") }).click();
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.audit") })).toHaveAttribute("aria-selected", "true");
  await expect(broken).toBeVisible();

  // Checked again over the intact log (same page): the callout goes away everywhere.
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: chained.ndjson } });
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
  await expect(page.locator("[data-chain-broken]")).toHaveCount(0);
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await expect(page.locator("[data-decision-form]")).toBeVisible();
  await expect(page.locator("[data-chain-broken]")).toHaveCount(0);
});

// The callout gives the verifier's own reason: a failure that is not a hash
// mismatch (here a line that is not JSON) is never called one.
test("a chain check that fails on a line that is not a record says so, not a hash-chain mismatch", async ({ page }) => {
  const chained = await chainedRun(awaitingDecisionDetail());
  const lines = chained.ndjson.toString("utf8").split("\n");
  const tampered = Buffer.from(lines.map((line, i) => (i === 3 ? "this line is not a record" : line)).join("\n"));
  await mockApi(page, { detail: chained.detail, receipt: chained.receipt, ndjson: { [RUN_ID]: tampered } });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  await expect(page.locator("[data-chain-verdict='fail']")).toBeVisible();
  const broken = page.locator("header.hc-run-header [data-chain-broken='header']");
  await expect(broken).toHaveAttribute("data-callout-tone", "failed");
  await expect(broken).toHaveAttribute("data-chain-broken-problem", "not_json");
  await expect(broken).toContainText(t("en", "Run.chain.problems.not_json"));
  await expect(broken).toContainText("line 4");
  await expect(broken).not.toContainText("hash chain");
  await expect(broken).not.toContainText("hash");
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const atDecision = page.locator("[data-decision] [data-chain-broken='decision']");
  await expect(atDecision).toContainText(t("en", "Run.chain.problems.not_json"));
  await expect(atDecision).not.toContainText("hash");
});

// Heuristic evaluation, problem 7: the system's PROVENANCE INCONSISTENT flag
// keeps its tag in the row and gets a full-width callout with its note (said
// once), and the page's own check names the labels the records contradict.
test("PROVENANCE INCONSISTENT is a full-width callout, and labels that disagree with the steps are named", async ({
  page,
}) => {
  const detail = liveModelDetail(["provenance_inconsistent"]);
  const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "live") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  const header = page.locator("header.hc-run-header");
  // The tag stays among the labels.
  await expect(
    header.locator(".hc-badges").getByText(t("en", "Provenance.flags.provenance_inconsistent"), { exact: true }),
  ).toBeVisible();
  // The callout: contested, full width, a title in words and the flag's note, once on the page.
  const flag = header.locator("[data-provenance-flag-callout='provenance_inconsistent']");
  await expect(flag).toBeVisible();
  await expect(flag).toHaveAttribute("data-callout-tone", "contested");
  await expect(flag.locator(".hc-callout-title")).toHaveText(t("en", "Run.consistency.flagTitle"));
  await expect(flag).toContainText(t("en", "Provenance.flagNotes.provenance_inconsistent"));
  await expect(page.getByText(t("en", "Provenance.flagNotes.provenance_inconsistent"))).toHaveCount(1);
  const badges = await header.locator(".hc-provenance-badges").boundingBox();
  const callout = await flag.boundingBox();
  expect(callout?.width ?? 0).toBeGreaterThanOrEqual((badges?.width ?? 0) - 1);
  // The page's own check: Meta's model over scripted steps, the snapshot over simulated proofs.
  const disagree = header.locator("[data-labels-disagree]");
  await expect(disagree).toHaveAttribute("data-callout-tone", "contested");
  await expect(disagree.locator(".hc-callout-title")).toHaveText(t("en", "Run.consistency.labelsTitle"));
  await expect(disagree.locator("[data-labels-disagreement='metaModelScriptedSteps']")).toContainText(
    "The model label names Meta’s model, but 6 recorded steps are the scripted double",
  );
  await expect(disagree.locator("[data-labels-disagreement='memoryEvidenceSimulatedProofs']")).toContainText(
    "recorded proofs are SIMULATED",
  );
  // It opens the Audit trail.
  await flag.getByRole("button", { name: t("en", "Run.chainBroken.openAudit") }).click();
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.audit") })).toHaveAttribute("aria-selected", "true");
  await expectNoLive(page, "en");
});

test("a LIVE frame over the scripted double is reconciled by a 'labels disagree' callout", async ({ page }) => {
  const run = inProgressLiveRun(5);
  await mockApi(page, { detail: run.detail, frames: run.frames });
  await openRun(page, "en");
  const header = page.locator("header.hc-run-header");
  // LIVE stays what the rules make it (a live frame of a live-model run) ...
  await expect(header.getByText(t("en", "Provenance.live.label"), { exact: true })).toBeVisible();
  // ... and the steps that say no model was called are named next to it, not left unflagged.
  await expect(page.locator("[data-model-note='scripted']").first()).toBeVisible();
  const disagree = header.locator("[data-labels-disagree]");
  await expect(disagree).toBeVisible();
  // Records 1 to 5: the proposal and the challenge, each the scripted double.
  await expect(disagree.locator("[data-labels-disagreement='metaModelScriptedSteps']")).toContainText(
    "The model label names Meta’s model, but 2 recorded steps are the scripted double",
  );
});

test("a run whose labels match its records shows no 'labels disagree' callout", async ({ page }) => {
  await mockApi(page, { detail: metaCallsDetail() });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(page.locator("[data-labels-disagreement='metaModelScriptedSteps']")).toHaveCount(0);
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await mockApi(page);
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await expect(page.locator("[data-labels-disagree]")).toHaveCount(0);
  await expect(page.locator("[data-provenance-flag-callout]")).toHaveCount(0);
});

// Heuristic evaluation, problem 7: a receipt that contradicts the run it is
// for is flagged, field by field, instead of shown as if it agreed.
test("a receipt that disagrees with the run says what disagrees; an agreeing one says nothing", async ({ page }) => {
  // The receipt of the decided run (approved, 15 events) against the run awaiting its decision (14 events).
  const detail = awaitingDecisionDetail();
  await mockApi(page, { detail });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  const disagrees = page.locator("[data-audit-receipt='ready'] [data-receipt-disagrees]");
  await expect(disagrees).toHaveAttribute("data-callout-tone", "contested");
  await expect(disagrees.locator(".hc-callout-title")).toHaveText(t("en", "Run.consistency.receipt.title"));
  await expect(disagrees.locator("[data-receipt-disagreement='finalState']")).toHaveText(
    "Final state: the receipt says “approved”, the run says “awaiting your decision”.",
  );
  await expect(disagrees.locator("[data-receipt-disagreement='eventCount']")).toHaveText(
    "Events: the receipt counts 15, the run 14.",
  );

  // The same run with the receipt of its own state: nothing to flag.
  const receipt = fixture<Json>("receipt-v2-retry.json");
  receipt.final_state = "awaiting_human_approval";
  receipt.event_count = 14;
  receipt.authorization = { state: "pending" };
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await mockApi(page, { detail, receipt });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await expect(page.locator("[data-receipt-field='event_count']")).toHaveText("14");
  await expect(page.locator("[data-receipt-disagrees]")).toHaveCount(0);
});

// Round 4 (HE #8): copy that depends on the connection is chosen from the
// health probe, per state: connected pages never say they are not connected,
// and a server that did not answer is never "not connected in this build".
test("connected: the legend intro, the tour, Docs, Modes and Settings say the portal is connected", async ({
  page,
}) => {
  await mockApi(page);
  await page.goto("/en/");
  await hydrated(page);
  await expect(page.locator("[data-api-status='connected']")).toBeVisible();
  await expect(page.locator("[data-connection-copy='connected']")).toHaveText(
    t("en", "Provenance.introConnected"),
  );
  await expect(page.getByText(t("en", "Provenance.intro"))).toHaveCount(0);
  await expect(page.getByText(t("en", "App.buildNote"))).toHaveCount(0);
  // Tour steps 1 and 2.
  await page.getByRole("button", { name: t("en", "Tour.start") }).click();
  const dialog = page.getByRole("dialog", { name: t("en", "Tour.steps.welcome.title") });
  await expect(dialog).toHaveAccessibleDescription(t("en", "Tour.steps.welcome.bodyConnected"));
  await expect(dialog).not.toContainText("did not answer");
  await dialog.getByRole("button", { name: t("en", "Tour.next") }).click();
  await expect(page.getByRole("dialog")).toHaveAccessibleDescription(
    t("en", "Tour.steps.guided.bodyConnected"),
  );
  await page.keyboard.press("Escape");

  // Docs: no build note once connected (the tour proves the probe answered).
  await page.goto("/en/docs/");
  await hydrated(page);
  await page.getByRole("button", { name: t("en", "Tour.launch") }).click();
  await expect(page.getByRole("dialog")).toHaveAccessibleDescription(
    t("en", "Tour.steps.welcome.bodyConnected"),
  );
  await expect(page.locator(".hc-build-note")).toHaveCount(0);
  await expect(page.getByText(t("en", "Docs.buildNote"))).toHaveCount(0);

  await page.goto("/en/docs/modes/");
  await hydrated(page);
  const build = page.locator("section", { has: page.locator("#modes-build") });
  await expect(build.locator("[data-connection-copy='connected']")).toHaveText(
    t("en", "Docs.modesBuild.connected"),
  );
  await expect(build).not.toContainText("did not answer");

  await page.goto("/en/settings/");
  await hydrated(page);
  await expect(page.locator("[data-connection-copy='connected']")).toHaveText(
    t("en", "Settings.explanation.technicalConnected"),
  );
});

test("unreachable: every connection-dependent sentence says the server did not answer", async ({ page }) => {
  // No mock: the static server answers 404 (its HTML not-found page) for /api/v1.
  await page.goto("/en/");
  await hydrated(page);
  await expect(page.locator("[data-connection-copy='unreachable']")).toHaveText(t("en", "Provenance.intro"));
  await expect(page.getByText(t("en", "App.buildNote"))).toBeVisible();
  await expect(page.getByText(t("en", "Overview.primaryActionNote"))).toContainText("did not answer");
  await expect(page.locator("main")).not.toContainText("in this build");
  await page.getByRole("button", { name: t("en", "Tour.start") }).click();
  const dialog = page.getByRole("dialog", { name: t("en", "Tour.steps.welcome.title") });
  await expect(dialog).toHaveAccessibleDescription(t("en", "Tour.steps.welcome.body"));
  await expect(dialog).toContainText("did not answer");
  await page.keyboard.press("Escape");

  await page.goto("/en/docs/");
  await hydrated(page);
  await expect(page.locator(".hc-build-note[data-connection-copy='unreachable']")).toHaveText(
    t("en", "Docs.buildNote"),
  );

  await page.goto("/en/docs/modes/");
  await hydrated(page);
  const build = page.locator("section", { has: page.locator("#modes-build") });
  await expect(build.locator("[data-connection-copy='unreachable']")).toContainText("did not answer");

  await page.goto("/en/settings/");
  await hydrated(page);
  await expect(page.locator("[data-connection-copy='unreachable']")).toHaveText(
    t("en", "Settings.explanation.descriptions.technical"),
  );

  // An outage reads "did not answer", never "not connected in this build".
  for (const path of ["/en/runs/", "/en/run/?id=run-00000000-0000-4000-8000-000000000001", "/en/showcase/"]) {
    await page.goto(path);
    await hydrated(page);
    const gate = page.locator("[data-api-gate='unreachable']");
    await expect(gate, path).toContainText(t("en", "NotConnected.message"));
    await expect(gate, path).toContainText("did not answer");
    await expect(gate, path).not.toContainText("in this build");
  }
});

for (const locale of ["es", "pt-BR", "fr"] as const) {
  test(`the connection-dependent copy has both variants (${locale})`, async ({ page }) => {
    await page.goto(`/${locale}/settings/`);
    await hydrated(page);
    await expect(page.locator("[data-connection-copy='unreachable']")).toHaveText(
      t(locale, "Settings.explanation.descriptions.technical"),
    );
    await mockApi(page);
    await page.goto(`/${locale}/`);
    await hydrated(page);
    await expect(page.locator("[data-connection-copy='connected']")).toHaveText(
      t(locale, "Provenance.introConnected"),
    );
    await page.goto(`/${locale}/docs/modes/`);
    await hydrated(page);
    await expect(page.locator("[data-connection-copy='connected']")).toHaveText(
      t(locale, "Docs.modesBuild.connected"),
    );
  });
}

// Round 4 (HE #18): a 500, a 503 and a 429 have distinct headlines that say
// the consequence first; the guided refusal sits inside the card pressed.
test("a guided 500 is headed as a server problem, inside the card that was pressed", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, { guidedRun: { status: 500, body: { code: "internal", message: "boom" } } });
  await page.goto("/en/new/");
  await hydrated(page);
  const card = page.locator("[data-scenario='offline']");
  await card.getByRole("button").click();
  const notice = card.locator("[data-scenario-error] [data-error-code='internal']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.serverProblem"));
  // A refusal without a wait leaves the pressed button usable, with the focus.
  await expect(card.getByRole("button")).toBeFocused();
  await expect(card.getByRole("button")).not.toHaveAttribute("aria-disabled", "true");
  expect(await card.getByRole("button").evaluate((el) => el.hasAttribute("disabled"))).toBe(false);
  await expect(notice).toContainText(t("en", "Api.errors.internal"));
  await expect(notice.locator("[data-error-fallback]")).toHaveCount(0);
  // Only inside that card, and no wait: every Run button still works.
  await expect(page.locator("[data-error-code]")).toHaveCount(1);
  for (const button of await page.locator("[data-scenario] button").all()) {
    await expect(button).toBeEnabled();
  }
  takeProblem(pageProblems, "HTTP 500: /api/v1/runs");
});

test("a guided 503 is headed as an unavailable server, with the run-creation sentence", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, { guidedRun: { status: 503, body: { code: "shutting_down", message: "shutting down" } } });
  await page.goto("/en/new/");
  await hydrated(page);
  const card = page.locator("[data-scenario='abstain']");
  await card.getByRole("button").click();
  const notice = card.locator("[data-error-code='shutting_down']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.serverUnavailable"));
  await expect(notice).toContainText(t("en", "Api.createErrors.shutting_down"));
  await expect(page.locator("[data-scenario='offline'] [data-scenario-error]")).toHaveCount(0);
  takeProblem(pageProblems, "HTTP 503: /api/v1/runs");
});

test("a guided 429 says how long to wait, disables every Run button until then and offers a recorded run", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    guidedRun: {
      status: 429,
      body: { code: "rate_limited", message: "too many requests from this client", retry_after_secs: 30 },
      retryAfter: "30",
    },
  });
  await page.clock.install();
  await page.goto("/en/new/");
  await hydrated(page);
  const card = page.locator("[data-scenario='offline']");
  await card.getByRole("button").click();
  const notice = card.locator("[data-scenario-error] [data-error-code='rate_limited']");
  // The headline says the wait once (counted down with the buttons); no
  // Retry-After line repeats it.
  await expect(notice.locator(".hc-error-title")).toHaveText(
    /^Too many requests from this connection\. Retry in (30|29|28) seconds\.$/u,
  );
  await expect(notice.locator("[data-error-wait]")).toHaveCount(0);
  await expect(notice.getByText(/Retry in/u)).toHaveCount(1);
  await expect(notice.locator("[data-error-fallback]")).toHaveText(t("en", "Api.fallback"));
  await expect(notice.locator("[data-error-fallback]")).toHaveAttribute("href", "/en/showcase/");
  const buttons = page.locator("[data-scenario] button");
  await expect(buttons).toHaveCount(5);
  for (const button of await buttons.all()) {
    await expect(button).toBeDisabled();
    await expect(button).toContainText(/^Try again in 0:[0-3]\d$/u);
  }
  await page.clock.fastForward("00:31");
  for (const button of await buttons.all()) {
    await expect(button).toBeEnabled();
    await expect(button).toHaveText(t("en", "NewDeliberation.guided.run"));
  }
  // The notice follows the buttons: once they are enabled, the wait reads as over.
  await expect(notice.locator(".hc-error-title")).toHaveText(
    "Too many requests from this connection. You can retry now.",
  );
  await expect(notice).not.toContainText("Retry in");
  takeProblem(pageProblems, "HTTP 429: /api/v1/runs");
});

test("a connection-wide 429 on a guided Run keeps live Start waiting too", async ({ page, pageProblems }) => {
  await mockApi(page, {
    liveEnabled: true,
    liveRequiresTurnstile: false,
    guidedRun: {
      status: 429,
      body: { code: "rate_limited", message: "too many requests from this client", retry_after_secs: 30 },
      retryAfter: "30",
    },
  });
  await page.clock.install();
  await page.goto("/en/new/");
  await hydrated(page);
  const submit = page.locator("[data-live='form'] button[type='submit']");
  await expect(submit).toHaveText(t("en", "NewDeliberation.live.start"));
  await expect(submit).not.toHaveAttribute("data-wait");
  await page.locator("[data-scenario='offline']").getByRole("button").click();
  await expect(page.locator("[data-scenario-error] [data-error-code='rate_limited']")).toBeVisible();
  // rate_limited counts this connection's requests: Start waits with the Run buttons.
  await expect(submit).toBeDisabled();
  await expect(submit).toContainText(/^Try again in 0:[0-3]\d$/u);
  await expect(submit).toHaveAttribute("data-wait", /^\d+$/u);
  await page.clock.fastForward("00:31");
  await expect(submit).toHaveText(t("en", "NewDeliberation.live.start"));
  await expect(submit).not.toHaveAttribute("data-wait");
  takeProblem(pageProblems, "HTTP 429: /api/v1/runs");
});

test("an endpoint's own 429 (a busy server) keeps only that endpoint waiting, with a counted Retry-After line", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    liveEnabled: true,
    liveRequiresTurnstile: false,
    guidedRun: {
      status: 429,
      body: { code: "too_many_runs", message: "too many runs", retry_after_secs: 30 },
      retryAfter: "30",
    },
  });
  await page.clock.install();
  await page.goto("/en/new/");
  await hydrated(page);
  await page.locator("[data-scenario='offline']").getByRole("button").click();
  const notice = page.locator("[data-scenario-error] [data-error-code='too_many_runs']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.busy"));
  // Not rate-limited: the Retry-After line carries the wait, counted down.
  await expect(notice.locator("[data-error-wait]")).toHaveText(/^Retry in (30|29|28) seconds\.$/u);
  await expect(page.locator("[data-scenario='offline'] button")).toBeDisabled();
  const submit = page.locator("[data-live='form'] button[type='submit']");
  await expect(submit).toHaveText(t("en", "NewDeliberation.live.start"));
  await expect(submit).not.toHaveAttribute("data-wait");
  await page.clock.fastForward("00:31");
  await expect(notice.locator("[data-error-wait]")).toHaveText("You can retry now.");
  takeProblem(pageProblems, "HTTP 429: /api/v1/runs");
});

test("pending: before the health probe settles, connection-dependent copy makes no claim either way", async ({
  page,
}) => {
  await mockApi(page);
  // The probe is held: it does not answer until the end of this test.
  const release = await holdRequests(page, "**/api/v1/health", "GET");
  await page.goto("/en/");
  await hydrated(page);
  await expect(
    page.locator("[data-connection-copy='pending']", { hasText: t("en", "Provenance.introPending") }),
  ).toHaveText(t("en", "Provenance.introPending"));
  await expect(page.getByText(t("en", "Provenance.intro"))).toHaveCount(0);
  await expect(page.getByText(t("en", "Provenance.introConnected"))).toHaveCount(0);
  await page.getByRole("button", { name: t("en", "Tour.start") }).click();
  const dialog = page.getByRole("dialog", { name: t("en", "Tour.steps.welcome.title") });
  await expect(dialog).toHaveAccessibleDescription(t("en", "Tour.steps.welcome.bodyPending"));
  await dialog.getByRole("button", { name: t("en", "Tour.next") }).click();
  await expect(page.getByRole("dialog")).toHaveAccessibleDescription(t("en", "Tour.steps.guided.bodyPending"));
  await expect(page.getByRole("dialog")).not.toContainText("Once the portal is connected");
  await page.keyboard.press("Escape");

  await page.goto("/en/docs/modes/");
  await hydrated(page);
  const build = page.locator("section", { has: page.locator("#modes-build") });
  await expect(build.locator("[data-connection-copy='pending']")).toHaveText(t("en", "Docs.modesBuild.pending"));

  await page.goto("/en/settings/");
  await hydrated(page);
  await expect(page.locator("[data-connection-copy='pending']")).toHaveText(
    t("en", "Settings.explanation.technicalConnected"),
  );
  // Once it answers, the same sentence is the connected variant.
  release();
  await expect(page.locator("[data-connection-copy='connected']")).toHaveText(
    t("en", "Settings.explanation.technicalConnected"),
  );
});

test("a refused cancel is headed by its code's class, never as a refused decision", async ({
  page,
  pageProblems,
}) => {
  const run = inProgressLiveRun(5);
  await mockApi(page, { detail: run.detail, frames: run.frames });
  await page.route("**/api/v1/runs/*/cancel", (route) =>
    route.fulfill({
      status: 409,
      contentType: "application/json",
      body: JSON.stringify({ code: "run_not_cancellable", message: "not cancellable" }),
    }),
  );
  await openRun(page, "en");
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await page
    .locator("section", { has: page.locator("#decision-cancel") })
    .getByRole("button", { name: t("en", "Run.decision.cancel") })
    .click();
  const notice = page.locator("[data-cancel-error] [data-error-code='run_not_cancellable']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.notAccepted"));
  await expect(notice).toContainText(t("en", "Api.errors.run_not_cancellable"));
  await expect(page.getByText(t("en", "Api.headlines.decision"))).toHaveCount(0);
  dropProblems(pageProblems, /HTTP 409: \/api\/v1\/runs\/.*\/cancel/u);
  expect(pageProblems).toEqual([]);
});

test("a decision answered with an unreadable 200 says it may have been recorded, and re-reads the run", async ({
  page,
  pageProblems,
}) => {
  const mock = await mockApi(page, {
    detail: awaitingDecisionDetail(),
    decision: { status: 200, body: { nope: true } },
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const detailReads = () =>
    mock.calls.filter((c) => c.method === "GET" && c.path === `/api/v1/runs/${RUN_ID}`).length;
  const before = detailReads();
  await page.locator("[data-decision-form]").getByRole("button", { name: t("en", "Run.decision.reject") }).click();
  const notice = page.locator("[data-error-code='bad_response']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.decisionUncertain"));
  await expect(notice).not.toContainText(t("en", "Api.headlines.decision"));
  await expect.poll(detailReads).toBeGreaterThan(before);
  dropProblems(pageProblems, /bad_response|does not match the API contract|\/decision/u);
});

test("a refused decision says it was not recorded, never that the server admits no new run", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    detail: awaitingDecisionDetail(),
    decision: { status: 503, body: { code: "shutting_down", message: "shutting down" } },
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  await page.locator("[data-decision-form]").getByRole("button", { name: t("en", "Run.decision.reject") }).click();
  const notice = page.locator("[data-error-code='shutting_down']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.decision"));
  await expect(notice).toContainText(t("en", "Api.errors.shutting_down"));
  await expect(notice).not.toContainText("admits");
  dropProblems(pageProblems, /HTTP 503: \/api\/v1\/runs\/.*\/decision/u);
  expect(pageProblems).toEqual([]);
});

test("a showcase that cannot be read is headed as an unavailable server, never with the run-creation sentence", async ({
  page,
  pageProblems,
}) => {
  await mockApi(page, {
    showcaseFailures: { count: 1, answer: { status: 503, body: { code: "shutting_down", message: "shutting down" } } },
  });
  await page.goto(localePath("en", "showcase"));
  await hydrated(page);
  const notice = page.locator("[data-showcase='error'] [data-error-code='shutting_down']");
  await expect(notice.locator(".hc-error-title")).toHaveText(t("en", "Api.headlines.serverUnavailable"));
  await expect(notice).toContainText(t("en", "Api.errors.shutting_down"));
  await expect(notice).not.toContainText("admits");
  takeProblem(pageProblems, "HTTP 503: /api/v1/showcase");
});
