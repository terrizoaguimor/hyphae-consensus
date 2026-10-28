// SPDX-License-Identifier: Apache-2.0
//
// The API-connected views over a mocked /api/v1 (e2e/api-mock.ts, the
// contract fixtures): New deliberation -> run -> frames -> decision ->
// receipt link; the honesty rules (a replayed run never reads LIVE, a
// provenance_inconsistent run never reads LIVE, simulated evidence named next
// to the outcome); the live section's disabled state and its form; My runs
// with and without a session; errors with their code's sentence and
// Retry-After; the runtime NotConnected state against the static server; and
// axe on /new, /runs and /run in every locale and theme.

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";

import { localePath, runPath } from "../src/lib/site";

import { AXE_TAGS, blocking, type ScanSummary, summarize, writeScans } from "./axe-results";
import {
  awaitingDecisionDetail,
  framesWith,
  liveModelDetail,
  mockApi,
  PUBLIC_DEMO_NOTICE,
} from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, type Locale, locales, t, test, useTheme } from "./fixtures";

const RUN_ID = "run-00000000-0000-4000-8000-000000000001";

/** Removes an expected API error response from the page's problem list, asserting it happened. */
function takeProblem(problems: string[], text: string): void {
  const index = problems.indexOf(text);
  expect(index, `expected ${text} among ${JSON.stringify(problems)}`).toBeGreaterThanOrEqual(0);
  problems.splice(index, 1);
}

/** The exact LIVE badge text of a locale; never present on a replayed or flagged run. */
function liveBadge(page: Page, locale: Locale) {
  return page.getByText(t(locale, "Provenance.live.label"), { exact: true });
}

async function openRun(page: Page, locale: Locale) {
  await page.goto(runPath(locale, RUN_ID));
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
  await expect(page.getByText(t("en", "Provenance.liveMeta.label"), { exact: true }).first()).toBeVisible();
  await expect(page.getByText(t("en", "Provenance.liveMeta.evidence"), { exact: true }).first()).toBeVisible();
  await expect(page.locator("[data-provenance-note='liveMeta.recorded']")).toContainText("Meta model");
  await expect(page.getByText(t("en", "Provenance.replay.label"), { exact: true }).first()).toBeVisible();
  await expect(liveBadge(page, "en")).toHaveCount(0);
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

test("the live section shows the form when capabilities.live is present", async ({
  page,
  pageProblems,
}) => {
  const mock = await mockApi(page, {
    liveEnabled: true,
    liveRun: {
      status: 422,
      body: { code: "invalid_request", message: "invalid request: unknown field `attest_adult`" },
    },
  });
  await page.goto("/en/new/");
  await hydrated(page);
  const form = page.locator("[data-live='form']");
  await expect(form).toBeVisible();
  await expect(page.getByText(t("en", "NewDeliberation.live.modelBadge"), { exact: true })).toBeVisible();
  await expect(page.getByText(t("en", "NewDeliberation.live.evidenceBadge"), { exact: true })).toBeVisible();
  await expect(form.locator("[data-turnstile='missing']")).toHaveText(
    t("en", "NewDeliberation.live.turnstileMissing"),
  );
  await expect(form).toContainText("daily_cap_usd");
  const start = page.getByRole("button", { name: t("en", "NewDeliberation.live.start") });
  await expect(start).toBeDisabled();
  await form.getByRole("radio", { name: "Retry budget (live)" }).check();
  await form.getByRole("checkbox", { name: t("en", "NewDeliberation.live.attest") }).check();
  await page.getByText(t("en", "NewDeliberation.live.inviteSummary")).click();
  await form.getByRole("textbox", { name: t("en", "NewDeliberation.live.inviteLabel") }).fill("JUDGE-1");
  await expect(start).toBeEnabled();
  await start.click();
  // The server (without the live-gate fields yet) answers 422; shown honestly.
  const notice = page.locator("[data-error-code='invalid_request']");
  await expect(notice).toContainText(t("en", "Api.errors.invalid_request"));
  await expect(notice).toContainText(t("en", "Api.errorDetail", { code: "invalid_request", status: 422 }));
  const call = mock.calls.find((c) => c.method === "POST" && c.path === "/api/v1/live-runs");
  expect(call?.body).toEqual({ preset_id: "retry-cap-3", attest_adult: true, judge_code: "JUDGE-1" });
  expect(call?.headers["cf-turnstile-response"]).toBeUndefined();
  takeProblem(pageProblems, "HTTP 422: /api/v1/live-runs");
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
  await expect(rows.last()).toContainText("approved by visitor:fixture0000000 (public_demo)");
  await page.getByLabel(t("en", "Run.explorer.filter")).selectOption("evidence_verified");
  await expect(rows).toHaveCount(4);
  await expect(rows.first()).toContainText("integrity valid");
  await rows.first().getByText(t("en", "Run.explorer.json")).click();
  await expect(rows.first().locator("pre")).toContainText('"type": "evidence_verified"');
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
