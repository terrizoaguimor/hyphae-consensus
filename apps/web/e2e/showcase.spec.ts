// SPDX-License-Identifier: Apache-2.0
//
// The research showcase over a mocked GET /showcase and /research
// (e2e/api-mock.ts, e2e/research.ts): entries built from the archive's own
// manifest and logs (run ids, batteries, titles, outcomes, files, pinned
// digests, record counts, heads) plus the server's own F03 entry from the
// contract, and the archive's real bytes at /research. The list reads as
// RECORDED, grouped by battery, every outcome next to its evidence axis; a
// recorded run opens in the run workspace in RECORDED mode (no stream, no
// stage, the Decision tab read-only) with the archive's labels: recorded
// proofs verified at run time, schema 1.0 pre-1.1 and unchained, a run
// blocked because no proof could be generated upstream with the archive's
// own note; a real archived revision (R06, its proposals from the log)
// shown as two lists, since its claims share no id, never as claims added
// or dropped; and nothing reads LIVE, in any locale. axe on the list and on
// the recorded run's tabs, in four locales and both themes.

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { IntlMessageFormat } from "intl-messageformat";

import { localePath, showcaseRunPath } from "../src/lib/site";

import { AXE_TAGS, blocking, type ScanSummary, summarize, writeScans } from "./axe-results";
import { mockApi, recordedRun } from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, type Locale, locales, t, test, useTheme } from "./fixtures";
import { expectNoLive, liveWord, takeProblem } from "./helpers";
import { archivedProposals, archiveEntry, logSegment, manifestRun, metaRequestIds, researchBytes } from "./research";

type Json = Record<string, unknown>;

/** The server's own showcase entry for F03 (contract fixture). */
const F03 = fixture<Json>("showcase-entry-F03.json");

/** Archived runs as the server lists them: fields from the archive, provenance as the server derives it. */
function entries() {
  const live = (label: string) => {
    const run = manifestRun(label);
    return {
      model: "live_meta",
      evidence: "hyphae_live",
      flags: ["mode_undeclared"],
      meta_request_ids: metaRequestIds(run.file, run.run_id),
    };
  };
  const c01 = archiveEntry("C01", {
    model: "scripted",
    evidence: "simulated",
    flags: ["mode_undeclared", "unchained"],
    meta_request_ids: [],
  });
  const l01 = archiveEntry("L01", live("L01"));
  const r06 = archiveEntry("R06", live("R06"));
  return { c01, l01, r06, list: [c01, l01, r06, F03] };
}

async function openShowcase(page: Page, locale: Locale) {
  await page.goto(localePath(locale, "showcase"));
  await hydrated(page);
  await expect(page.locator("[data-showcase='list']")).toBeVisible();
}

async function openRecorded(page: Page, locale: Locale, runId: string) {
  await page.goto(showcaseRunPath(locale, runId));
  await hydrated(page);
  await expect(page.locator("[data-run-mode='recorded']")).toBeVisible();
}

test("the showcase lists the archive as RECORDED, by battery, every outcome next to its evidence @smoke", async ({
  page,
}) => {
  const { c01, l01, r06, list } = entries();
  await mockApi(page, { showcase: list });
  await openShowcase(page, "en");
  await expect(page.locator("[data-showcase-run]")).toHaveCount(4);
  await expect(page.locator("[data-battery]")).toHaveCount(4);
  // Every entry is RECORDED, with the model badge of a list: never LIVE META.
  await expect(page.getByText(t("en", "Provenance.recorded.label"), { exact: true })).toHaveCount(4);
  const f03 = page.locator(`[data-showcase-run='${String(F03.run_id)}']`);
  await expect(f03.locator("[data-showcase-outcome='contested']")).toHaveText(
    t("en", "Run.outcome", { kind: t("en", "Run.kinds.contested"), evidence: t("en", "Provenance.hyphaeLive.label") }),
  );
  await expect(f03.getByText(t("en", "Provenance.metaModel.label"), { exact: true })).toBeVisible();
  const scripted = page.locator(`[data-showcase-run='${String(c01.run_id)}']`);
  await expect(scripted).toContainText(t("en", "Showcase.unchained"));
  await expect(scripted.locator("[data-showcase-outcome='supported']")).toHaveText(
    "supported — on SIMULATED evidence",
  );
  await expect(page.locator(`[data-showcase-run='${String(l01.run_id)}'] [data-showcase-outcome='blocked']`)).toBeVisible();
  // The card opens the run in the workspace, in RECORDED mode.
  await expect(page.locator(`[data-showcase-run='${String(r06.run_id)}'] a`)).toHaveAttribute(
    "href",
    showcaseRunPath("en", String(r06.run_id)),
  );
  // Each battery's log, with the digest the manifest pins, and the manifest checked here.
  const battery = page.locator("[data-battery='live-battery-2026-10-26']");
  await expect(battery.locator("[data-archive-download]")).toHaveAttribute(
    "href",
    "/research/2026-10-26/live-battery-2026-10-26.jsonl",
  );
  await expect(battery).toContainText(String(l01.sha256).slice(0, 12));
  await expect(page.locator("[data-manifest-check='matches']")).toHaveText(t("en", "Showcase.manifest.matches"));
  // The archive's own note on the 2026-10-26 battery names the cause of its blocked runs.
  await expect(battery.locator("[data-archive-note] blockquote")).toContainText("a deployment misconfiguration");
  await expectNoLive(page, "en");
});

test("a recorded run opens read-only in RECORDED mode, never as running @smoke", async ({ page }) => {
  const { r06, list } = entries();
  const recorded = recordedRun(r06, { base: "retry" });
  const mock = await mockApi(page, { showcase: list, recordedRuns: [recorded] });
  const runId = String(r06.run_id);
  await openRecorded(page, "en", runId);
  // No stage and no stream: the tabs a recorded run has, Decision first.
  await expect(page.getByRole("tab")).toHaveCount(6);
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.playback") })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: t("en", "Run.tabs.decision") })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("[data-run-field='state']")).toHaveText(t("en", "Showcase.run.awaiting"));
  const header = page.locator("header.hc-run-header");
  await expect(header.getByText(t("en", "Provenance.recorded.label"), { exact: true }).first()).toBeVisible();
  await expect(header.getByText(t("en", "Provenance.metaModel.label"), { exact: true })).toBeVisible();
  await expect(header.locator("[data-provenance-note='liveMeta.recorded']")).toBeVisible();
  await expect(header.locator("[data-recorded-run]")).toContainText(
    t("en", "Showcase.run.banner", { label: "R06", battery: "live-battery-2026-10-27" }),
  );
  // Read-only decision: no form, the archive's receipt and log.
  await expect(page.locator("[data-decision-form]")).toHaveCount(0);
  await expect(page.locator("[data-decision-unavailable]")).toHaveText(t("en", "Run.decision.recordedReadOnly"));
  await expect(page.locator("[data-receipt-link]")).toHaveAttribute("href", `/api/v1/showcase/${runId}/receipt`);
  await expect(page.locator("[data-ndjson-link]")).toHaveAttribute(
    "href",
    "/research/2026-10-27/live-battery-2026-10-27.jsonl",
  );
  await expectNoLive(page, "en");

  // Evidence and proofs: verified at run time, re-verifiable chain, not these proofs.
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  const reading = page.locator("[data-proof-reading='accepted']").first();
  await expect(reading).toContainText("Verified at run time on");
  await expect(reading).toContainText("by hyphae proof verify");
  await expect(reading).toContainText("so the public can re-verify the event chain, not these proofs");
  await expect(page.locator("[data-coverage='unrecorded']").first()).toContainText("recorded before schema 1.3");
  await expectNoLive(page, "en");
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  await expect(page.locator("[data-limits-translated]")).toBeVisible();
  await expect(page.locator("[data-proof-plain='accepted'] [data-proof-reading='accepted']").first()).toBeVisible();
  // The audit trail: the archived log this run is a segment of, its pinned digest, the check.
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-chain-scope='log']")).toHaveText(t("en", "Run.audit.scopes.log"));
  await expect(page.locator("[data-log-link]")).toHaveAttribute("href", "/research/2026-10-27/live-battery-2026-10-27.jsonl");
  await expect(page.locator("[data-log-sha256]")).toHaveText(String(r06.sha256));
  await expect(page.locator("[data-cli]")).toHaveText(
    `consensus-api verify live-battery-2026-10-27.jsonl --run ${runId} --receipt receipt-${runId}.json`,
  );
  await expect(page.locator("[data-receipt-download]")).toHaveAttribute("href", `/api/v1/showcase/${runId}/receipt`);
  // Recorded Meta calls: request ids are the archive's own.
  await page.getByRole("tab", { name: t("en", "Run.tabs.meta") }).click();
  const ids = (r06.provenance as { meta_request_ids: string[] }).meta_request_ids;
  await expect(page.locator("[data-invocation-scripted='false']").first()).toContainText(String(ids[0]));
  await expectNoLive(page, "en");
  // Nothing streams and nothing of this server's runs is asked for.
  expect(mock.calls.filter((c) => c.path.includes("/events"))).toEqual([]);
  expect(mock.calls.filter((c) => c.path.startsWith("/api/v1/runs/"))).toEqual([]);
  expect(mock.calls.filter((c) => c.path === `/api/v1/showcase/${runId}`)).toHaveLength(1);
});

/**
 * The recorded R06 detail with its real proposals: the original and the
 * revision exactly as the archived log records them (prop-1 with claim-2..4,
 * prop-13 with claim-14..16, its addressed items), in place of the
 * fixture's scripted prop-1 and prop-2.
 */
function withArchivedRevision(entry: Json): { detail: Json; receipt: Json } {
  const { proposal, revisions } = archivedProposals(String(entry.file), String(entry.run_id));
  const revision = revisions.at(-1);
  if (revision === undefined) {
    throw new Error(`${String(entry.label)} has no revision`);
  }
  const revisedId = String((revision.proposal as Json).id);
  const recorded = recordedRun(entry, { base: "retry" });
  const detail = JSON.parse(JSON.stringify(recorded.detail).split('"prop-2"').join(JSON.stringify(revisedId))) as Json;
  for (const event of detail.events as Json[]) {
    const payload = event.payload as Json;
    if (payload.type === "proposal_submitted") {
      payload.proposal = proposal;
    } else if (payload.type === "revision_submitted") {
      payload.revision = revision;
    }
  }
  detail.current_proposal = revision.proposal;
  return { detail, receipt: recorded.receipt };
}

for (const locale of locales) {
  test(`a real archived revision (R06) is shown as two lists, never as claims added or dropped (${locale})`, async ({
    page,
  }) => {
    const { r06, list } = entries();
    const { proposal, revisions } = archivedProposals(String(r06.file), String(r06.run_id));
    const revision = revisions.at(-1) as Json;
    const revisedProposal = revision.proposal as Json;
    await mockApi(page, { showcase: list, recordedRuns: [withArchivedRevision(r06)] });
    await openRecorded(page, locale, String(r06.run_id));
    const compare = page.locator("[data-superseded] [data-revision-compare]");
    await expect(compare).toHaveAttribute("data-claim-pairing", "none");
    await expect(compare.locator("h4")).toHaveText(
      t(locale, "Run.revision.heading", { original: "prop-1", revised: "prop-13" }),
    );
    // The records share no claim id: said in plain words, and nothing is paired.
    await expect(compare.locator("[data-claim-pairing-note]")).toContainText(
      t(locale, "Run.revision.unpaired", { original: "prop-1", revised: "prop-13" }),
    );
    // Its revision went through the Meta model adapter (a live-model run):
    // the adapter's numbering is named as the cause.
    await expect(compare.locator("[data-claim-pairing-cause]")).toHaveText(t(locale, "Run.revision.unpairedMeta"));
    await expect(compare.locator("table")).toHaveCount(0);
    await expect(compare.locator("[data-claim-change]")).toHaveCount(0);
    for (const kind of ["unchanged", "changed", "added", "removed"]) {
      await expect(compare.getByText(t(locale, `Run.revision.kinds.${kind}`), { exact: true })).toHaveCount(0);
    }
    // Each list as recorded, in its order, headed by its proposal's id.
    for (const [side, recordedProposal, heading] of [
      ["original", proposal, "Run.revision.originalColumn"],
      ["revised", revisedProposal, "Run.revision.revisedColumn"],
    ] as const) {
      const claims = recordedProposal.claims as { id: string; statement: string }[];
      const column = compare.locator(`[data-claim-list='${side}']`);
      await expect(column.locator("h5")).toHaveText(t(locale, heading, { id: String(recordedProposal.id) }));
      await expect(column.locator("li")).toHaveCount(claims.length);
      for (const [i, claim] of claims.entries()) {
        const item = column.locator("li").nth(i);
        await expect(item).toHaveAttribute("data-claim-id", claim.id);
        await expect(item.locator("[lang='en']")).toHaveText(claim.statement);
      }
    }
    expect((proposal.claims as unknown[]).length).toBe(3);
    expect((revisedProposal.claims as unknown[]).length).toBe(3);
    // The revision's own claim of what it addresses, from the archive.
    await expect(compare.locator("[data-revision-steps] li")).toHaveText(
      t(locale, "Run.revision.step", {
        id: "prop-13",
        supersedes: "prop-1",
        items: (revision.addressed_items as string[]).join(", "),
      }),
    );
    await expectNoLive(page, locale);
  });
}

test("disjoint claim ids from a scripted revision: the two lists, and no cause named for them", async ({ page }) => {
  const { r06, list } = entries();
  const recorded = withArchivedRevision(r06);
  // The same archived proposals, the revision now a scripted double's: the
  // Meta model adapter wrote nothing here, so it is not named as the cause.
  for (const event of recorded.detail.events as Json[]) {
    const payload = event.payload as Json;
    if (payload.type === "revision_submitted") {
      payload.invocation = { provider: "fake", model: "muse-wonder-1.0", request_id: "fake-revise-1", scripted: true };
    }
  }
  await mockApi(page, { showcase: list, recordedRuns: [recorded] });
  await openRecorded(page, "en", String(r06.run_id));
  const compare = page.locator("[data-superseded] [data-revision-compare]");
  await expect(compare).toHaveAttribute("data-claim-pairing", "none");
  await expect(compare.locator("[data-claim-pairing-note]")).toHaveText(
    t("en", "Run.revision.unpaired", { original: "prop-1", revised: "prop-13" }),
  );
  await expect(compare.locator("[data-claim-pairing-cause]")).toHaveCount(0);
  await expect(compare.getByText(t("en", "Run.revision.unpairedMeta"))).toHaveCount(0);
  await expect(compare.locator("[data-claim-change]")).toHaveCount(0);
});

test("a recorded run's archived log is checked in this browser against the pinned digest and the receipt's head @smoke", async ({
  page,
}) => {
  const { r06, list } = entries();
  const runId = String(r06.run_id);
  const segment = logSegment(String(r06.file), runId);
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await openRecorded(page, "en", runId);
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await expect(page.locator("[data-chain-intro='chained']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const verdict = page.locator("[data-chain-verdict='pass']");
  await expect(verdict).toBeVisible();
  const result = (name: string) => verdict.locator(`[data-chain-result='${name}']`);
  await expect(result("sha256")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("sha256")).toContainText(String(r06.sha256));
  await expect(result("chain")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("range")).toContainText(`sequences ${segment.first} to ${segment.last}`);
  await expect(result("head")).toContainText(String(r06.head_hash));
  await expect(result("head")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(result("count")).toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(verdict).toContainText(t("en", "Run.chain.limits"));
  await expectNoLive(page, "en");
});

test("a flipped byte in the archived log fails at the record that holds it", async ({ page }) => {
  const { r06, list } = entries();
  const runId = String(r06.run_id);
  const file = String(r06.file);
  const target = logSegment(file, runId).first + 3;
  // Flip the case of one letter inside that record's payload: still UTF-8, still JSON.
  const original = researchBytes(file);
  const text = original.toString("utf8");
  const lineStart = text.split("\n").slice(0, target - 1).reduce((n, line) => n + Buffer.byteLength(line) + 1, 0);
  const line = text.split("\n")[target - 1] ?? "";
  expect(line).toContain(`"sequence":${target},`);
  // The first letter of the payload's first string value (its "type").
  const payloadAt = line.indexOf('"payload":');
  const letter = /"[a-z_]+":"([a-z])/.exec(line.slice(payloadAt));
  expect(letter).not.toBeNull();
  const charIndex = payloadAt + (letter?.index ?? 0) + (letter?.[0].length ?? 1) - 1;
  const offset = lineStart + Buffer.byteLength(line.slice(0, charIndex));
  expect(String.fromCharCode(original[offset] ?? 0)).toBe(letter?.[1]);
  const tampered = Buffer.from(original);
  tampered[offset] = (tampered[offset] ?? 0) ^ 0x20;
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })], research: { [file]: tampered } });
  await openRecorded(page, "en", runId);
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const verdict = page.locator("[data-chain-verdict='fail']");
  await expect(verdict).toBeVisible();
  await expect(verdict.locator(`[data-chain-failure='hash_mismatch'][data-chain-failure-sequence='${target}']`)).toContainText(
    `The check stops at record ${target} (line ${target})`,
  );
  await expect(verdict.locator("[data-chain-result='sha256']")).toContainText(t("en", "Run.chain.verdicts.fail"));
});

test("a run blocked because no proof could be generated reads so, with the archive's own note", async ({ page }) => {
  const { l01, list } = entries();
  await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
  });
  await openRecorded(page, "en", String(l01.run_id));
  const header = page.locator("[data-recorded-run]");
  await expect(header.locator("[data-archive-label='generation-failed']")).toContainText(
    "Blocked because no proof could be generated upstream",
  );
  await expect(header.locator("[data-archive-label='generation-failed']")).toContainText(
    "not a check that found a record altered",
  );
  await expect(header.locator("[data-archive-note] blockquote")).toContainText("a deployment misconfiguration");
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  await expect(page.locator("[data-coverage='proof_error']").first()).toHaveText(
    t("en", "Run.evidence.coverage.proofError"),
  );
  await expect(page.locator("[data-proof-reading='generation_failed']").first()).toHaveText(
    t("en", "Run.proofs.reading.generationFailed"),
  );
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  await expect(page.locator("[data-proof-plain='generation_failed']").first()).toHaveText(
    t("en", "Run.proofs.plain.generationFailed"),
  );
  // The Decision tab's explanation agrees: nothing was checked, no check came back invalid.
  await page.getByRole("tab", { name: t("en", "Run.tabs.decision") }).click();
  const explanation = page.locator("[data-explanation]");
  await expect(explanation.locator("[data-explain-lead]")).toHaveText(
    t("en", "Run.explain.plain.rules.invalid_proof_ungenerated", { kind: t("en", "Run.kinds.blocked") }),
  );
  await expect(explanation.locator("[data-explain-reasons] li").first()).toHaveText(
    t("en", "Run.explain.plain.reasons.invalid_proofs_ungenerated"),
  );
  await expect(explanation).not.toContainText(t("en", "Run.explain.plain.reasons.invalid_proofs"));
  // The proofs count reads as integrity statuses, never as proofs that failed.
  await expect(explanation.locator("[data-explain-counts] li").nth(1)).toContainText(
    "Integrity statuses of the final round:",
  );
  await expect(explanation.locator("[data-explain-counts] li").nth(1)).toContainText("with no proof generated");
  await expect(explanation.locator("[data-explain-counts]")).not.toContainText("Proofs of the final round");
  await expect(explanation.locator("[data-explain-counts]")).not.toContainText("failed");
  await expectNoLive(page, "en");
});

test("a schema-1.0 run reads pre-1.1, unchained", async ({ page }) => {
  const { c01, list } = entries();
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(c01, { base: "retry" })] });
  await openRecorded(page, "en", String(c01.run_id));
  await expect(page.locator("[data-archive-label='unchained']")).toContainText(t("en", "Showcase.unchained"));
  await expect(page.locator("[data-archive-label='unchained']")).toContainText(t("en", "Showcase.run.unchainedNote"));
  await expect(page.getByText(t("en", "Provenance.flags.unchained"), { exact: true })).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-chain-scope='unchained']")).toHaveText(t("en", "Run.audit.scopes.unchained"));
  await expect(page.locator("[data-chain-field='head']")).toHaveText(t("en", "Run.audit.noHead"));
  await expectNoLive(page, "en");
});

// HONESTY-01: the chain check on a schema-1.0 run says there is no chain to
// check, in every locale: never the green "Chain checks out", never "All #
// records hash and link", never a green "matches" on the chain or the head.
/** A catalog plural with its count, formatted as next-intl formats it. */
const tCount = (locale: Locale, key: string, count: number) =>
  String(new IntlMessageFormat(t(locale, key), locale).format({ count }));
/** A catalog message with plural arguments, formatted as next-intl formats it. */
const tArgs = (locale: Locale, key: string, values: Record<string, string | number>) =>
  String(new IntlMessageFormat(t(locale, key), locale).format(values));

for (const locale of locales) {
  test(`a schema-1.0 run's chain check reads "no chain to check", never checks out (${locale})`, async ({ page }) => {
    const { c01, list } = entries();
    const runId = String(c01.run_id);
    const segment = logSegment(String(c01.file), runId);
    await mockApi(page, { showcase: list, recordedRuns: [recordedRun(c01, { base: "retry" })] });
    await openRecorded(page, locale, runId);
    await page.getByRole("tab", { name: t(locale, "Run.tabs.audit") }).click();
    await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
    // The method is described without recomputing any hash (CU2-01).
    const intro = page.locator("[data-chain-intro='unchained']");
    await expect(intro).toHaveText(t(locale, "Run.chain.introUnchained", { name: String(c01.file).split("/").at(-1) ?? "" }));
    await expect(page.locator("[data-chain-intro='chained']")).toHaveCount(0);
    await page.getByRole("button", { name: t(locale, "Run.chain.run") }).click();
    const verdict = page.locator("[data-chain-verdict='unchained']");
    await expect(verdict).toBeVisible();
    await expect(page.locator("[data-chain-verdict='pass']")).toHaveCount(0);
    await expect(verdict.locator(".hc-chain-verdict")).toContainText(t(locale, "Run.chain.unchainedVerdict"));
    await expect(verdict).not.toContainText(t(locale, "Run.chain.pass"));
    expect(segment.count).toBeGreaterThan(0);
    // Neither over the run's records nor over the whole file's.
    const counts = [segment.count, researchBytes(String(c01.file)).toString("utf8").trim().split("\n").length];
    for (const n of counts) {
      await expect(verdict).not.toContainText(tCount(locale, "Run.chain.passSentence", n));
      for (const legacy of [0, ...counts]) {
        await expect(verdict).not.toContainText(tArgs(locale, "Run.chain.passSentenceLegacy", { chained: n, legacy }));
      }
    }
    // Nor "checked in part": no record of this run carries a hash.
    await expect(verdict).not.toContainText(t(locale, "Run.chain.partialVerdict"));
    await expect(page.locator("[data-chain-verdict='partial']")).toHaveCount(0);
    await expect(verdict.locator(".hc-chain-verdict")).toContainText(
      tCount(locale, "Run.chain.unchainedSentence", segment.count),
    );
    const result = (name: string) => verdict.locator(`[data-chain-result='${name}']`);
    // What was compared reads as compared; what was not, as not.
    await expect(result("sha256")).toContainText(t(locale, "Run.chain.verdicts.pass"));
    await expect(result("chain")).toContainText(t(locale, "Run.chain.verdicts.not_checked"));
    await expect(result("chain")).not.toContainText(t(locale, "Run.chain.verdicts.pass"));
    await expect(result("head")).toContainText(t(locale, "Run.chain.verdicts.not_compared"));
    await expect(result("head")).not.toContainText(t(locale, "Run.chain.verdicts.pass"));
    await expect(result("count")).toContainText(t(locale, "Run.chain.verdicts.pass"));
    await expect(verdict).toContainText(t(locale, "Run.chain.limitsUnchained"));
    await expect(verdict).not.toContainText(t(locale, "Run.chain.limits"));
    await expectNoLive(page, locale);
  });
}

test("a recorded run the listing does not name offers no file it cannot name", async ({ page }) => {
  const { r06 } = entries();
  await mockApi(page, { showcase: [], recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await openRecorded(page, "en", String(r06.run_id));
  await expect(page.locator("[data-recorded-run]")).toContainText(t("en", "Showcase.run.bannerNoEntry"));
  await expect(page.locator("[data-ndjson-unavailable]")).toHaveText(t("en", "Run.decision.archiveLogPending"));
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-log='archive-pending'] [data-log-unavailable]")).toHaveText(
    t("en", "Run.audit.archivePending"),
  );
  await expect(page.locator("[data-cli]")).toHaveCount(0);
  await expect(page.locator("[data-chain-check]")).toHaveCount(0);
  await expectNoLive(page, "en");
});

test("a showcase that cannot be read is said, with a retry", async ({ page, pageProblems }) => {
  const { list } = entries();
  await mockApi(page, {
    showcase: list,
    showcaseFailures: { count: 1, answer: { status: 503, body: { code: "shutting_down", message: "shutting down" } } },
  });
  await page.goto(localePath("en", "showcase"));
  await hydrated(page);
  const error = page.locator("[data-showcase='error']");
  await expect(error.locator("[data-error-code='shutting_down']")).toContainText(t("en", "Api.errors.shutting_down"));
  await expect(page.getByText(t("en", "Showcase.loading"))).toHaveCount(0);
  await error.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(page.locator("[data-showcase-run]")).toHaveCount(4);
  takeProblem(pageProblems, "HTTP 503: /api/v1/showcase");
});

test("a manifest that does not match its published digest lends no note", async ({ page }) => {
  const { list } = entries();
  const forged = Buffer.from(
    JSON.stringify({ created: "2026-10-27", files: [{ path: "2026-10-26/live-battery-2026-10-26.jsonl", sha256: "0", kind: "event_log", description: "forged note" }] }),
  );
  await mockApi(page, { showcase: list, research: { "MANIFEST.json": forged } });
  await openShowcase(page, "en");
  await expect(page.locator("[data-manifest-check='differs']")).toBeVisible();
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  await expect(page.locator("main")).not.toContainText("forged note");
});

for (const locale of locales) {
  test(`no LIVE word on the showcase or a recorded run (${locale})`, async ({ page }) => {
    const { r06, list } = entries();
    await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
    await openShowcase(page, locale);
    await expect(page.locator("[data-showcase-run]")).toHaveCount(4);
    await expectNoLive(page, locale);
    await openRecorded(page, locale, String(r06.run_id));
    await expect(page.locator("[data-run-field='state']")).toHaveText(t(locale, "Showcase.run.awaiting"));
    await expectNoLive(page, locale);
    for (const tab of ["evidence", "proofs", "audit", "meta", "explorer"] as const) {
      await page.getByRole("tab", { name: t(locale, `Run.tabs.${tab}`) }).click();
      await expectNoLive(page, locale);
    }
  });
}

test("the Overview's figures are counted from the archive the server lists @smoke", async ({ page }) => {
  const { list } = entries();
  await mockApi(page, { showcase: list });
  await page.goto(localePath("en", ""));
  await hydrated(page);
  const figures = page.locator("[data-figures='ready']");
  await expect(figures).toBeVisible();
  // Every expected figure is counted here from the same entries the mock serves.
  type Entry = { outcome: string; battery: string; provenance: { evidence: string; model: string; meta_request_ids?: string[] } };
  const entriesOf = list as unknown as Entry[];
  const count = (outcome: string, evidence: string) =>
    entriesOf.filter((e) => e.outcome === outcome && e.provenance.evidence === evidence).length;
  await expect(figures.locator("[data-figures-total]")).toHaveAttribute("data-figures-total", String(entriesOf.length));
  // Every outcome is counted next to its evidence axis.
  await expect(figures.locator("[data-figures-outcomes] thead th")).toHaveText([
    t("en", "Overview.figures.outcome"),
    t("en", "Overview.figures.onEvidence", { evidence: t("en", "Provenance.simulated.label") }),
    t("en", "Overview.figures.onEvidence", { evidence: t("en", "Provenance.hyphaeLive.label") }),
    t("en", "Overview.figures.total"),
  ]);
  for (const outcome of ["supported", "contested", "blocked"]) {
    const row = figures.locator(`[data-figure-outcome='${outcome}']`);
    await expect(row.locator("[data-figure-evidence='simulated']")).toHaveText(String(count(outcome, "simulated")));
    await expect(row.locator("[data-figure-evidence='hyphae_live']")).toHaveText(String(count(outcome, "hyphae_live")));
    await expect(row.locator("[data-figure-total]")).toHaveText(
      String(entriesOf.filter((e) => e.outcome === outcome).length),
    );
  }
  // By battery, with the schema and policy rules each was recorded under.
  const batteries = [...new Set(entriesOf.map((e) => e.battery))];
  await expect(figures.locator("[data-figure-battery]")).toHaveCount(batteries.length);
  await expect(figures.locator("[data-figure-battery='console-2026-10-26']")).toContainText(
    t("en", "Overview.figures.schemaNoRules", { schema: "1.0" }),
  );
  await expect(figures.locator("[data-figure-battery='live-battery-2026-10-26']")).toContainText(
    t("en", "Overview.figures.schemaRules", { schema: "1.1", rules: "1.1" }),
  );
  const requests = entriesOf.reduce((n, e) => n + (e.provenance.meta_request_ids ?? []).length, 0);
  await expect(figures.locator("[data-figures-requests]")).toHaveAttribute("data-figures-requests", String(requests));
  await expect(figures.locator("[data-figures-unchained]")).toHaveAttribute("data-figures-unchained", "1");
  // A list of models never reads LIVE: META MODEL, from provenance.ts. (The
  // Overview's provenance legend, which explains what LIVE META means, is
  // outside the figures; battery ids such as live-battery-2026-10-26 are the
  // archive's identifiers, shown as code, not labels.)
  await expect(figures.locator("[data-figures-models]")).toContainText(t("en", "Provenance.metaModel.label"));
  const labels = await figures.evaluate((element) => {
    const copy = element.cloneNode(true) as HTMLElement;
    copy.querySelectorAll("code").forEach((code) => code.remove());
    return copy.textContent ?? "";
  });
  expect(labels).not.toMatch(liveWord("en"));
});

test("without an API the Overview's figures are the not-connected placeholder", async ({ page }) => {
  // No mock: the static server answers 404 (its HTML not-found page) for /api/v1.
  await page.goto(localePath("en", ""));
  await hydrated(page);
  const placeholder = page.locator("[data-figures='unreachable']");
  await expect(placeholder).toContainText(t("en", "NotConnected.message"));
  await expect(placeholder).toContainText(t("en", "Overview.metricsPlanned.computed"));
  await expect(page.locator("[data-figures='ready']")).toHaveCount(0);
});

const themes = ["light", "dark"] as const;

async function scan(page: Page, name: string): Promise<ScanSummary> {
  const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
  return summarize(name, results);
}

for (const locale of locales) {
  for (const theme of themes) {
    test(`axe: the Overview's figures, /showcase and a recorded run (${locale}, ${theme})`, async ({ page }) => {
      test.setTimeout(240_000);
      await useTheme(page, theme);
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      const { l01, list } = entries();
      await mockApi(page, {
        showcase: list,
        recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
      });
      const scans: ScanSummary[] = [];
      await page.goto(localePath(locale, ""));
      await hydrated(page);
      await expect(page.locator("[data-figures='ready']")).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} / (figures)`));
      await openShowcase(page, locale);
      await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      scans.push(await scan(page, `${locale} ${theme} /showcase (connected)`));
      await openRecorded(page, locale, String(l01.run_id));
      await expect(page.locator("[data-archive-note]")).toBeVisible();
      scans.push(await scan(page, `${locale} ${theme} /run recorded decision`));
      for (const tab of ["explorer", "evidence", "proofs", "audit", "meta"] as const) {
        await page.getByRole("tab", { name: t(locale, `Run.tabs.${tab}`) }).click();
        await expect(page.locator(`[data-tab='${tab}']`)).toBeVisible();
        if (tab === "audit") {
          // The chain checked in this browser, with its result on screen.
          await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
          await page.getByRole("button", { name: t(locale, "Run.chain.run") }).click();
          await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
        }
        scans.push(await scan(page, `${locale} ${theme} /run recorded ${tab}`));
      }
      await writeScans(`showcase-${locale}-${theme}`, scans);
      expect(scans).toHaveLength(8);
      expect(blocking(scans)).toEqual([]);
    });
  }
}
