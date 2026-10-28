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
// against the static server; and axe on /new, /runs and /run in every locale
// and theme.

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";

import { CONTRACT_SHA256 } from "../src/lib/api/types";
import { localePath, runPath } from "../src/lib/site";

import { AXE_TAGS, blocking, type ScanSummary, summarize, writeScans } from "./axe-results";
import {
  awaitingDecisionDetail,
  failedDetail,
  framesWith,
  inProgressLiveRun,
  liveModelDetail,
  liveR01Run,
  mockApi,
  PUBLIC_DEMO_NOTICE,
  RUN_ID,
} from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, type Locale, locales, t, test, useTheme } from "./fixtures";

const LIVE_RUN_ID = "run-00000000-0000-4000-8000-000000000006";

/** Removes an expected API error response from the page's problem list, asserting it happened. */
function takeProblem(problems: string[], text: string): void {
  const index = problems.indexOf(text);
  expect(index, `expected ${text} among ${JSON.stringify(problems)}`).toBeGreaterThanOrEqual(0);
  problems.splice(index, 1);
}

/** Removes the browser's own console lines about a failure a test provokes on purpose. */
function dropProblems(problems: string[], pattern: RegExp): void {
  for (let i = problems.length - 1; i >= 0; i -= 1) {
    if (pattern.test(problems[i] ?? "")) {
      problems.splice(i, 1);
    }
  }
}

/** The exact LIVE badge text of a locale; never present on a replayed or flagged run. */
function liveBadge(page: Page, locale: Locale) {
  return page.getByText(t(locale, "Provenance.live.label"), { exact: true });
}

/** The locale's LIVE word (LIVE, EN VIVO, AO VIVO, EN DIRECT) as a whole word. */
function liveWord(locale: Locale): RegExp {
  const word = t(locale, "Provenance.live.label").replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(^|[^\\p{L}])${word}([^\\p{L}]|$)`, "iu");
}

/**
 * Nothing of the LIVE family is on the page: no LIVE delivery badge, no LIVE
 * META model badge, no "Meta model · live" note, no tab named Live, and no
 * badge or provenance note carrying the locale's LIVE word at all.
 */
async function expectNoLive(page: Page, locale: Locale) {
  await expect(liveBadge(page, locale)).toHaveCount(0);
  await expect(page.getByText(t(locale, "Provenance.liveMeta.label"), { exact: true })).toHaveCount(0);
  await expect(page.getByText(t(locale, "Provenance.liveMeta.live"), { exact: true })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: t(locale, "Run.tabs.live"), exact: true })).toHaveCount(0);
  await expect(page.locator("[data-frame-label='live.label']")).toHaveCount(0);
  const labels = await page
    .locator(".hc-badges, .hc-frame-badge, [data-provenance-note], [role=tab]")
    .allInnerTexts();
  for (const text of labels) {
    expect(text, `a label with the LIVE word: ${text}`).not.toMatch(liveWord(locale));
  }
}

async function openRun(page: Page, locale: Locale, runId = RUN_ID) {
  await page.goto(runPath(locale, runId));
  await hydrated(page);
  await expect(page.locator("[data-run-id]")).toBeVisible();
}

test("new -> run -> frames -> decision -> receipt link", async ({ page, pageProblems }) => {
  const mock = await mockApi(page, { detail: awaitingDecisionDetail() });
  await page.goto("/en/new/");
  await hydrated(page);
  await expect(page.locator("[data-api-gate='ok']")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: t("en", "NewDeliberation.guided.heading") }),
  ).toBeVisible();
  const cards = page.locator("[data-scenario]");
  await expect(cards).toHaveCount(5);
  await expect(
    page.getByText(t("en", "NewDeliberation.guided.modelBadge"), { exact: true }),
  ).toHaveCount(5);
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
  await expect(page.locator("[data-lane]")).toContainText("4 proofs checked");
  await expect(page.locator("[data-proof-simulated='true']")).toHaveCount(4);
  await expect(page.locator("[data-recommendation]")).toContainText(
    "supported — on SIMULATED evidence",
  );
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
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Run.states.approved"));
  expect(pageProblems).toEqual([]);
});

test("a replayed finished run never shows LIVE and is named Playback", async ({ page }) => {
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

    // A flagged run with live-delivered frames: the flag, a warning model badge, no LIVE.
    await mockApi(page, { detail: flagged, frames: framesWith(flaggedProvenance, "live") });
    await openRun(page, locale);
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    await expect(
      page.getByText(t(locale, "Provenance.flags.mode_undeclared"), { exact: true }).first(),
    ).toBeVisible();
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
  await expect(page.locator("[data-provenance-note='liveMeta.live']")).toHaveText(
    t("en", "Provenance.liveMeta.live"),
  );
  // Live frames are never paced, and a live model is never labelled simulated.
  await expect(page.locator("[data-pacing]")).toHaveCount(0);
});

for (const delivery of ["live", "replay"] as const) {
  test(`a live-model run is never paced or labelled simulated (${delivery} frames)`, async ({ page }) => {
    const detail = liveModelDetail();
    const provenance = (detail.summary as { provenance: Record<string, unknown> }).provenance;
    await mockApi(page, { detail, frames: framesWith(provenance, delivery) });
    // Pacing is on (no reduced motion): it must still not apply.
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await openRun(page, "en");
    await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
    // Every frame shows at once: nothing was held back.
    await expect(page.locator("[data-lane]")).toContainText("4 proofs checked");
    await expect(page.locator("[data-pacing]")).toHaveCount(0);
    await expect(page.getByText(t("en", "Run.pacing.label"), { exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: t("en", "Run.pacing.skip") })).toHaveCount(0);
  });
}

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
  // A blank code is not sent at all: the run takes the public lane.
  await invite.fill("   ");
  await start.click();
  const liveRuns = () => mock.calls.filter((c) => c.method === "POST" && c.path === "/api/v1/live-runs");
  await expect.poll(() => liveRuns().length).toBe(2);
  await expect(notice).toBeVisible();
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
  await expect(first).toContainText("supported — on SIMULATED evidence");
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
  await expect(page.locator("[data-lane]")).toContainText("4 proofs checked");
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
      await page.getByRole("tab", { name: t(locale, "Run.tabs.decision") }).click();
      await expect(page.locator("[data-decision-form]")).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run decision`));

      await writeScans(`api-${locale}-${theme}`, scans);
      expect(scans).toHaveLength(5);
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
