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
// or dropped; and nothing reads LIVE, in any locale, not even a live-model
// run with no flag (RECORDED mode gives the header no delivery). The
// Overview's figures are read as text against numbers counted here from two
// archives; hostile markup, markdown and bidi controls in any run or archive
// text render inert. axe on the list and on the recorded run's tabs, in four
// locales and both themes, on the failure and edge states and on a phone;
// the recorded tabs and the chain check by keyboard.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { IntlMessageFormat } from "intl-messageformat";

import { recordHash } from "../src/lib/chain/verify";
import { modelLabel } from "../src/lib/provenance";
import { KNOWN_PROOF_LIMITATIONS } from "../src/lib/run/receipt";
import { MODEL_AXES, OUTCOME_KEYS } from "../src/lib/showcase";
import { localePath, showcaseRunPath } from "../src/lib/site";

import { AXE_TAGS, blocking, type ScanSummary, summarize, writeScans } from "./axe-results";
import { mockApi, recordedRun } from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, type Locale, locales, t, test, useTheme } from "./fixtures";
import { dropProblems, expectNoLive, expectNoLiveOnPage, liveWord, liveWordHits, takeProblem } from "./helpers";
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

/**
 * The archive's own names a recorded view may interpolate into a sentence as
 * plain text (battery ids such as live-battery-2026-10-27, log paths): data,
 * not labels, so the whole-page LIVE scan cuts them out (and only them).
 */
const IDENTIFIERS = [
  ...new Set(entries().list.flatMap((entry) => [String(entry.battery), String(entry.file)])),
];

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
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
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
  // Beside that link: the receipt is this server's rebuild, in today's wording.
  await expect(page.locator("li:has([data-receipt-link]) [data-receipt-rebuilt]")).toHaveText(
    t("en", "Run.decision.receiptRebuilt"),
  );
  await expect(page.locator("[data-ndjson-link]")).toHaveAttribute(
    "href",
    "/research/2026-10-27/live-battery-2026-10-27.jsonl",
  );
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);

  // Evidence and proofs: verified at run time, re-verifiable chain, not these proofs.
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  const reading = page.locator("[data-proof-reading='accepted']").first();
  await expect(reading).toContainText("Verified at run time on");
  await expect(reading).toContainText("by hyphae proof verify");
  await expect(reading).toContainText("so the public can re-verify the event chain, not these proofs");
  await expect(page.locator("[data-coverage='unrecorded']").first()).toContainText("recorded before schema 1.3");
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
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
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
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
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
});

test("a recorded run's receipt says it is this server's rebuild and quotes the run record's own limitations", async ({
  page,
}) => {
  const { r06, list } = entries();
  const runId = String(r06.run_id);
  const record = String(r06.record_file);
  const mock = await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await openRecorded(page, "en", runId);

  // Proof inspector: next to the limitations, where the receipt comes from.
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  const limits = page.locator("[data-proof-limits='ready']");
  await expect(limits.locator("[data-proof-limitations]")).toBeVisible();
  const note = limits.locator("[data-run-time-receipt]");
  await expect(note.locator("[data-receipt-regenerated]")).toContainText(
    "This server built this receipt from the archived log when it was requested",
  );
  await expect(note).toContainText(t("en", "Run.runTimeReceipt.record"));
  await expect(note.locator("[data-run-record-link]")).toHaveAttribute("href", `/research/${record}`);
  // Nothing of the record is read before it is asked for.
  expect(mock.calls.filter((c) => c.path === `/research/${record}`)).toEqual([]);
  await note.getByRole("button", { name: t("en", "Run.runTimeReceipt.read") }).click();
  const quoted = note.locator("[data-run-record-limitations='differ']");
  await expect(quoted).toContainText("Integrity proofs, where valid, establish that a recalled record matches");
  await expect(note.locator("[data-run-record='ready']")).toContainText(t("en", "Run.runTimeReceipt.limitsDiffer"));
  await expect(note.locator("[data-run-record='ready']")).toContainText("The run record’s receipt was generated on");
  // The served receipt's own text stays as it is, beside it.
  await expect(limits.locator("[data-proof-limitations]")).toHaveText(KNOWN_PROOF_LIMITATIONS);
  await expectNoLive(page, "en");

  // Audit trail: next to the receipt download, read once per run; the receipt's limitations are today's wording.
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  const auditNote = page.locator("[data-audit-log] [data-run-time-receipt='ready']");
  await expect(auditNote.locator("[data-receipt-regenerated]")).toContainText(
    "This server built this receipt from the archived log when it was requested",
  );
  await expect(auditNote.locator("[data-run-record-limitations='differ']")).toContainText("Integrity proofs, where valid");
  await expect(page.locator("[data-receipt-limitations-today]")).toHaveText(t("en", "Run.runTimeReceipt.limitsToday"));
  expect(mock.calls.filter((c) => c.path === `/research/${record}`)).toHaveLength(1);
  await expectNoLive(page, "en");
});

test("a run record that cannot be read is said, and nothing of it is quoted", async ({ page, pageProblems }) => {
  const { r06, list } = entries();
  const record = String(r06.record_file);
  await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(r06, { base: "retry" })],
    research: { [record]: Buffer.from("not a run record") },
  });
  await openRecorded(page, "en", String(r06.run_id));
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  const note = page.locator("[data-run-time-receipt]");
  await note.getByRole("button", { name: t("en", "Run.runTimeReceipt.read") }).click();
  await expect(note.locator("[data-run-record='unreadable']")).toHaveText(t("en", "Run.runTimeReceipt.unreadable"));
  await expect(note.locator("[data-run-record-limitations]")).toHaveCount(0);
  expect(pageProblems).toEqual([]);
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
  // The sentence cites the archive's note only because the note is on the page.
  await expect(header.locator("[data-archive-note-reference]")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.shown"),
  );
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
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
});

test("a blocked run whose manifest does not match cites no note it does not show", async ({ page }) => {
  const { l01, list } = entries();
  const forged = Buffer.from(
    JSON.stringify({ created: "2026-10-27", files: [{ path: l01.file, sha256: "0", kind: "event_log", description: "forged note" }] }),
  );
  await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
    research: { "MANIFEST.json": forged },
  });
  await openRecorded(page, "en", String(l01.run_id));
  const sentence = page.locator("[data-recorded-run] [data-archive-label='generation-failed']");
  await expect(sentence.locator("[data-archive-note-reference='differs']")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.differs"),
  );
  await expect(sentence).toContainText("Blocked because no proof could be generated upstream");
  await expect(sentence).not.toContainText(t("en", "Showcase.run.generationFailedNote.shown"));
  await expect(sentence).not.toContainText("names the cause");
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  await expect(page.locator("main")).not.toContainText("forged note");
});

test("a blocked run whose manifest cannot be read cites no note it does not show", async ({ page, pageProblems }) => {
  const { l01, list } = entries();
  await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
  });
  // Registered after mockApi, so it answers the manifest before the archive route does.
  await page.route("**/research/MANIFEST.json", (route) =>
    route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({ code: "shutting_down", message: "shutting down" }),
    }),
  );
  await openRecorded(page, "en", String(l01.run_id));
  const sentence = page.locator("[data-recorded-run] [data-archive-label='generation-failed']");
  await expect(sentence.locator("[data-archive-note-reference='unread']")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.unread"),
  );
  await expect(sentence).not.toContainText("names the cause");
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  takeProblem(pageProblems, "HTTP 503: /research/MANIFEST.json");
  dropProblems(pageProblems, /^console: Failed to load resource: the server responded with a status of 503/);
});

test("a blocked run cites nothing about the note while its manifest is still being checked", async ({ page }) => {
  const { l01, list } = entries();
  await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
  });
  // Registered after mockApi, so it holds the manifest open; fallback() then
  // hands the request to the archive route, which serves the real bytes.
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested: () => void = () => undefined;
  const asked = new Promise<void>((resolve) => {
    requested = resolve;
  });
  await page.route("**/research/MANIFEST.json", async (route) => {
    requested();
    await held;
    await route.fallback();
  });
  await openRecorded(page, "en", String(l01.run_id));
  await asked;
  const sentence = page.locator("[data-recorded-run] [data-archive-label='generation-failed']");
  await expect(sentence).toContainText("Blocked because no proof could be generated upstream");
  // Still checking: neither "shown" nor a failure the check has not reached.
  await expect(sentence.locator("[data-archive-note-reference]")).toHaveCount(0);
  await expect(sentence).not.toContainText("names the cause");
  await expect(sentence).not.toContainText(t("en", "Showcase.run.generationFailedNote.unread"));
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  release();
  await expect(sentence.locator("[data-archive-note-reference='shown']")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.shown"),
  );
  await expect(page.locator("[data-recorded-run] [data-archive-note] blockquote")).toContainText(
    "a deployment misconfiguration",
  );
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
  // The console runs have no run record: no run-time receipt to compare, and none offered.
  await expect(page.locator("[data-run-time-receipt='no-record'] [data-run-record='none']")).toHaveText(
    t("en", "Run.runTimeReceipt.noRecord"),
  );
  await expect(page.locator("[data-run-record-link]")).toHaveCount(0);
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
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
  await expect(page.locator("[data-run-record-link]")).toHaveCount(0);
  await expectNoLiveOnPage(page, "en", IDENTIFIERS);
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
    await expectNoLiveOnPage(page, locale, IDENTIFIERS);
    await openRecorded(page, locale, String(r06.run_id));
    await expect(page.locator("[data-run-field='state']")).toHaveText(t(locale, "Showcase.run.awaiting"));
    await expectNoLiveOnPage(page, locale, IDENTIFIERS);
    for (const tab of ["evidence", "proofs", "audit", "meta", "explorer"] as const) {
      await page.getByRole("tab", { name: t(locale, `Run.tabs.${tab}`) }).click();
      await expectNoLiveOnPage(page, locale, IDENTIFIERS);
    }
  });

  test(`no LIVE word in any text or accessible name of a recorded run, in every tab (${locale})`, async ({ page }) => {
    test.setTimeout(180_000);
    const { c01, l01, r06, list } = entries();
    await mockApi(page, {
      showcase: list,
      recordedRuns: [
        recordedRun(r06, { base: "retry" }),
        recordedRun(l01, { base: "invalid_proof", generationFailed: true }),
        recordedRun(c01, { base: "retry" }),
      ],
    });
    // A run with its archived log checked here, one blocked because no proof
    // could be generated (with the archive's note), and an unchained one.
    for (const [entry, marker] of [
      [r06, "[data-recorded-run]"],
      [l01, "[data-archive-label='generation-failed']"],
      [c01, "[data-archive-label='unchained']"],
    ] as const) {
      await openRecorded(page, locale, String(entry.run_id));
      await expect(page.locator(marker)).toBeVisible();
      await expect(page.getByRole("tab")).toHaveCount(6);
      await expectNoLiveOnPage(page, locale, IDENTIFIERS);
      for (const tab of ["decision", "explorer", "evidence", "proofs", "audit", "meta"] as const) {
        await page.getByRole("tab", { name: t(locale, `Run.tabs.${tab}`) }).click();
        await expect(page.locator(`[data-tab='${tab}']`)).toBeVisible();
        if (tab === "audit" && entry === r06) {
          // The chain checked in this browser, with its verdict on screen.
          await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
          await page.getByRole("button", { name: t(locale, "Run.chain.run") }).click();
          await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
        }
        await expectNoLiveOnPage(page, locale, IDENTIFIERS);
      }
    }
  });

  // GATES-02: every other recorded live-model run here carries the
  // mode_undeclared flag, which alone forbids LIVE. An archived run whose
  // task declares its mode carries no flag at all, so only RECORDED mode's
  // own guarantee (no stream, so the header is given no delivery) keeps it
  // from reading LIVE: no delivery badge of any kind (LIVE, STREAMING or
  // REPLAY), META MODEL, and no LIVE word anywhere, in every tab.
  test(`a recorded live-model run with no flag still never reads LIVE and has no delivery badge (${locale})`, async ({
    page,
  }) => {
    test.setTimeout(120_000);
    const { list } = entries();
    const run = manifestRun("R06");
    const unflagged = archiveEntry("R06", {
      model: "live_meta",
      evidence: "hyphae_live",
      flags: [],
      meta_request_ids: metaRequestIds(run.file, run.run_id),
    });
    const showcase = list.map((entry) => (entry.run_id === unflagged.run_id ? unflagged : entry));
    await mockApi(page, { showcase, recordedRuns: [recordedRun(unflagged, { base: "retry" })] });
    await openShowcase(page, locale);
    await expect(page.locator(`[data-showcase-run='${String(unflagged.run_id)}']`)).toBeVisible();
    await expectNoLiveOnPage(page, locale, IDENTIFIERS);
    await openRecorded(page, locale, String(unflagged.run_id));
    const header = page.locator("header.hc-run-header");
    // Model, evidence, source: and no delivery badge, no flag.
    await expect(header.locator("ul.hc-badges > li")).toHaveText([
      t(locale, "Provenance.metaModel.label"),
      t(locale, "Provenance.hyphaeLive.label"),
      t(locale, "Provenance.recorded.label"),
    ]);
    for (const key of ["live.label", "liveMeta.label", "streaming.label", "replay.label"]) {
      await expect(header.getByText(t(locale, `Provenance.${key}`), { exact: true })).toHaveCount(0);
    }
    await expect(header.locator("[data-provenance-note]")).toHaveAttribute("data-provenance-note", "liveMeta.recorded");
    await expect(page.getByRole("tab")).toHaveCount(6);
    await expectNoLiveOnPage(page, locale, IDENTIFIERS);
    for (const tab of ["explorer", "evidence", "proofs", "audit", "meta", "decision"] as const) {
      await page.getByRole("tab", { name: t(locale, `Run.tabs.${tab}`) }).click();
      await expect(page.locator(`[data-tab='${tab}']`)).toBeVisible();
      await expectNoLiveOnPage(page, locale, IDENTIFIERS);
    }
  });

  test(`the whole-page LIVE scan sees text, hidden badges and accessible names, and skips only identifiers (${locale})`, async ({
    page,
  }) => {
    const { r06, list } = entries();
    await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
    await openRecorded(page, locale, String(r06.run_id));
    const word = t(locale, "Provenance.live.label");
    expect(await liveWordHits(page.locator("body"), locale, IDENTIFIERS)).toEqual([]);
    // What the scan must not report: an interpolated battery id, code, the run's quoted words.
    await page.locator("[data-recorded-run]").evaluate((section, w) => {
      const add = (html: string) => section.insertAdjacentHTML("beforeend", html);
      add(`<p data-probe>battery live-battery-2026-10-27</p>`);
      add(`<p data-probe><code>${w} id</code></p>`);
      add(`<figure data-probe data-untrusted="model"><blockquote>${w} quoted</blockquote></figure>`);
    }, word);
    expect(await liveWordHits(page.locator("body"), locale, IDENTIFIERS)).toEqual([]);
    // What it must report: a sentence, a hidden badge, an aria-label, a title,
    // and the word split across text nodes by markup (EN</strong> VIVO), once.
    await page.locator("[data-recorded-run]").evaluate((section, w) => {
      const add = (html: string) => section.insertAdjacentHTML("beforeend", html);
      add(`<p data-probe>A run, ${w.toLowerCase()} here.</p>`);
      add(`<span data-probe hidden class="hc-badge">${w}</span>`);
      add(`<button data-probe type="button" aria-label="Open ${w}"></button>`);
      add(`<span data-probe title="${w} title"></span>`);
      add(`<p data-probe>Split <strong>${w.slice(0, 2)}</strong>${w.slice(2)} here</p>`);
    }, word);
    expect(await liveWordHits(page.locator("body"), locale, IDENTIFIERS)).toEqual([
      `text: A run, ${word.toLowerCase()} here.`,
      `text: ${word}`,
      `text: Split ${word} here`,
      `aria-label: Open ${word}`,
      `title: ${word} title`,
    ]);
  });

  test(`no LIVE word in any text or accessible name of the Overview's figures (${locale})`, async ({ page }) => {
    const { list } = entries();
    await mockApi(page, { showcase: list });
    await page.goto(localePath(locale, ""));
    await hydrated(page);
    const figures = page.locator("[data-figures='ready']");
    await expect(figures).toBeVisible();
    await expect(figures.locator("[data-figures-models]")).toContainText(t(locale, "Provenance.metaModel.label"));
    expect(await liveWordHits(figures, locale, IDENTIFIERS)).toEqual([]);
  });

  test(`the not-connected showcase carries no LIVE word but its one stated negation (${locale})`, async ({
    page,
  }) => {
    // No mock: the static server answers 404 (its HTML not-found page) for /api/v1.
    await page.goto(localePath(locale, "showcase"));
    await hydrated(page);
    await expect(page.locator("[data-api-gate='unreachable']")).toBeVisible();
    await expect(page.locator("[data-showcase-run]")).toHaveCount(0);
    await expectNoLive(page, locale);
    // Showcase.planned.recorded says a replay is never presented as a live
    // run (allow-listed in tests/recorded-no-live.test.ts); it is the only
    // text of this state that may carry the word, and only where it does.
    const planned = t(locale, "Showcase.planned.recorded");
    expect(await liveWordHits(page.locator("body"), locale, IDENTIFIERS)).toEqual(
      liveWord(locale).test(planned) ? [`text: ${planned}`] : [],
    );
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
  // The runs those ids were recorded in (the scripted C01 has none), named in the sentence.
  const requestRuns = entriesOf.filter((e) => (e.provenance.meta_request_ids ?? []).length > 0).length;
  expect(requestRuns).toBe(entriesOf.length - 1);
  await expect(figures.locator("[data-figures-requests]")).toHaveAttribute("data-figures-request-runs", String(requestRuns));
  await expect(figures.locator("[data-figures-requests]")).toContainText(
    `${requests} recorded Meta request ids, in ${requestRuns} runs: one per Meta model call recorded with its response`,
  );
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

// GATES-04: the figures as a visitor reads them. Every visible number is
// checked in its visible sentence or cell, formatted from the catalog with
// the numbers counted here from the entries the mock serves (never with
// archiveFigures), over two different archives, so a number typed into the
// page (or computed for one archive only) fails on the other.

/** A catalog message exactly as written (fixtures.ts t() fills simple arguments only). */
function rawMessage(locale: Locale, key: string): string {
  let node: unknown = JSON.parse(readFileSync(path.join(__dirname, "..", "messages", `${locale}.json`), "utf8"));
  for (const part of key.split(".")) {
    node = typeof node === "object" && node !== null ? (node as Record<string, unknown>)[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key} in ${locale}`);
  }
  return node;
}

/** A catalog message with its arguments and plurals, formatted as next-intl formats it. */
const icu = (locale: Locale, key: string, values: Record<string, string | number>) =>
  String(new IntlMessageFormat(rawMessage(locale, key), locale).format(values));

type FigureEntry = {
  outcome: string;
  battery: string;
  authorization: string;
  event_count: number;
  schema_version: string;
  head_hash: string | null;
  provenance: { evidence: string; model: string; flags?: string[]; meta_request_ids?: string[] };
};

/** Another archive: several runs per battery, a failed run, three authorizations, two unchained runs. */
function widerArchive(): Json[] {
  const live = (label: string) => {
    const run = manifestRun(label);
    return archiveEntry(label, {
      model: "live_meta",
      evidence: "hyphae_live",
      flags: ["mode_undeclared"],
      meta_request_ids: metaRequestIds(run.file, run.run_id),
    });
  };
  const scripted = (label: string) =>
    archiveEntry(label, { model: "scripted", evidence: "simulated", flags: ["mode_undeclared", "unchained"], meta_request_ids: [] });
  return [scripted("C02"), scripted("C04"), live("L01"), live("L02"), live("L06"), live("R01"), live("R06")];
}

const FIGURE_ARCHIVES = {
  "four runs": () => entries().list,
  "seven runs": widerArchive,
} as const;

/** The figures' outcome key of an entry's outcome (a kind, failed:<stage>, or other). */
const figureOutcome = (outcome: string) =>
  outcome.startsWith("failed:") ? "failed" : (OUTCOME_KEYS as readonly string[]).includes(outcome) ? outcome : "other";

for (const [archive, served] of Object.entries(FIGURE_ARCHIVES)) {
  for (const locale of ["en", "fr"] as const) {
    test(`the Overview's figures read, as text, the numbers counted from the archive (${archive}, ${locale})`, async ({
      page,
    }) => {
      const list = served();
      await mockApi(page, { showcase: list });
      await page.goto(localePath(locale, ""));
      await hydrated(page);
      const figures = page.locator("[data-figures='ready']");
      await expect(figures).toBeVisible();
      const runs = list as unknown as FigureEntry[];
      const events = runs.reduce((n, e) => n + e.event_count, 0);
      const ids = (e: FigureEntry) => (e.provenance.meta_request_ids ?? []).length;
      const requests = runs.reduce((n, e) => n + ids(e), 0);
      const requestRuns = runs.filter((e) => ids(e) > 0).length;
      const unchained = runs.filter(
        (e) => e.schema_version === "1.0" || (e.provenance.flags ?? []).includes("unchained") || e.head_hash === null,
      ).length;
      const manifestSha256 = createHash("sha256").update(researchBytes("MANIFEST.json")).digest("hex");
      expect(runs.length).toBeGreaterThan(0);
      // The lead sentence: runs, records, the manifest's digest.
      await expect(figures.locator("[data-figures-total]")).toHaveText(
        icu(locale, "Overview.figures.source", { runs: runs.length, events, sha256: manifestSha256.slice(0, 12) }),
      );
      // Each battery's runs and outcomes, in its row.
      const batteries = [...new Set(runs.map((e) => e.battery))];
      await expect(figures.locator("[data-figure-battery]")).toHaveCount(batteries.length);
      const outcomeText = (outcome: string) =>
        outcome === "failed"
          ? t(locale, "Overview.figures.failed")
          : outcome === "other"
            ? t(locale, "Overview.figures.other")
            : t(locale, `Run.kinds.${outcome}`);
      for (const battery of batteries) {
        const inBattery = runs.filter((e) => e.battery === battery);
        const cells = figures.locator(`[data-figure-battery='${battery}'] td`);
        await expect(cells.nth(0)).toHaveText(String(inBattery.length));
        const outcomes = OUTCOME_KEYS.filter((key) => inBattery.some((e) => figureOutcome(e.outcome) === key));
        await expect(cells.nth(2)).toHaveText(
          outcomes
            .map((outcome) =>
              icu(locale, "Overview.figures.outcomeCount", {
                outcome: outcomeText(outcome),
                count: inBattery.filter((e) => figureOutcome(e.outcome) === outcome).length,
              }),
            )
            .join(", "),
        );
      }
      // By model and by authorization, each with its count.
      const models = MODEL_AXES.filter((model) => runs.some((e) => e.provenance.model === model));
      await expect(figures.locator("[data-figures-models]")).toHaveText(
        icu(locale, "Overview.figures.models", {
          list: models
            .map((model) =>
              icu(locale, "Overview.figures.modelCount", {
                label: t(locale, `Provenance.${modelLabel({ model: model as "live_meta" }).key}`),
                count: runs.filter((e) => e.provenance.model === model).length,
              }),
            )
            .join(", "),
        }),
      );
      const authorizations = [...new Set(runs.map((e) => e.authorization))];
      await expect(figures.locator("[data-figures-authorizations]")).toHaveText(
        icu(locale, "Overview.figures.authorizations", {
          list: authorizations
            .map((authorization) =>
              icu(locale, "Overview.figures.authorizationCount", {
                authorization: t(locale, `Run.audit.authorization.${authorization}`),
                count: runs.filter((e) => e.authorization === authorization).length,
              }),
            )
            .join(", "),
        }),
      );
      // The recorded Meta request ids and the runs they were recorded in.
      await expect(figures.locator("[data-figures-requests]")).toHaveText(
        icu(locale, "Overview.figures.requests", { count: requests, runs: requestRuns }),
      );
      await expect(figures.locator("[data-figures-unchained]")).toHaveText(
        tCount(locale, "Overview.figures.unchained", unchained),
      );
    });
  }
}

test("the two archives the Overview's text is checked over differ in every figure", () => {
  // Otherwise a number typed into the page could pass both.
  const [a, b] = Object.values(FIGURE_ARCHIVES).map((served) => served() as unknown as FigureEntry[]);
  const figuresOf = (runs: FigureEntry[]) => ({
    runs: runs.length,
    events: runs.reduce((n, e) => n + e.event_count, 0),
    requests: runs.reduce((n, e) => n + (e.provenance.meta_request_ids ?? []).length, 0),
    requestRuns: runs.filter((e) => (e.provenance.meta_request_ids ?? []).length > 0).length,
    unchained: runs.filter((e) => e.head_hash === null || e.schema_version === "1.0").length,
    authorizationPending: runs.filter((e) => e.authorization === "pending").length,
    liveMeta: runs.filter((e) => e.provenance.model === "live_meta").length,
    largestBattery: Math.max(...runs.map((e) => runs.filter((other) => other.battery === e.battery).length)),
  });
  const [fa, fb] = [figuresOf(a ?? []), figuresOf(b ?? [])];
  for (const key of Object.keys(fa) as Array<keyof typeof fa>) {
    expect(fa[key], key).not.toBe(fb[key]);
  }
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

// SECURITY-4: text that comes from the run or the archive (a retrieved
// record, a model's output, the archive manifest's note, the receipt's and
// the run record's own strings, a run's title and label) renders as text and
// nothing else: hostile markup (a script, an image with an onerror handler,
// an iframe, a javascript: link), markdown (links, emphasis) and Unicode bidi
// controls never become an element, a link, a request or a script, and a
// bidi control in the quoted text cannot reorder the page's own words around
// it (the quote is a block of its own). A future markdown renderer or
// auto-linker for any of these fails here.
const HOSTILE_MARKUP =
  '<script data-hostile>window.__hostile = "script"</script>' +
  '<img data-hostile src="/hostile.png" onerror="window.__hostile = 1">' +
  '<a data-hostile href="javascript:window.__hostile=1">open</a> ' +
  "[markdown link](javascript:window.__hostile=1) [sign in](https://example.invalid/login) " +
  "<https://example.invalid/auto> https://example.invalid/bare **bold** _em_ " +
  '<iframe data-hostile src="https://example.invalid/"></iframe> end';
/**
 * The same, with bidi controls: a closed right-to-left isolate, then a
 * right-to-left override (U+202E) that is never closed, the spoofing form: it
 * would reverse every letter after it up to the end of its paragraph.
 */
const HOSTILE = `${HOSTILE_MARKUP} ${String.fromCharCode(0x2067)}isolate${String.fromCharCode(0x2069)} ${String.fromCharCode(0x202e)}desrever liat`;

/** Free-text fields of a run's records (record content, model output, the server's own text). */
const FREE_TEXT = new Set([
  "content",
  "statement",
  "summary",
  "rationale",
  "description",
  "reason",
  "resolution_reason",
  "query",
  "abstain_reason",
  "note",
]);

/** Every free-text string of `value` (one with a space: an enumeration code has none) replaced with `text`. */
function withText(value: unknown, text: string): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => withText(item, text));
  }
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, child]) => [
        key,
        FREE_TEXT.has(key) && typeof child === "string" && child.includes(" ") ? text : withText(child, text),
      ]),
    );
  }
  return value;
}

/** A recorded run whose every free text, title, receipt string and run-record string is hostile. */
function hostileRun(entry: Json, options: Parameters<typeof recordedRun>[1]): { detail: Json; receipt: Json } {
  const recorded = recordedRun(entry, options);
  const detail = withText(recorded.detail, HOSTILE) as Json;
  // A retrieval that found nothing, with the model's query (shown as model output).
  const events = detail.events as Json[];
  const last = events.at(-1) as Json;
  events.push({
    ...last,
    sequence: (last.sequence as number) + 1,
    event_id: "evt-hostile-unavailable",
    actor: { kind: "runtime" },
    payload: { type: "evidence_unavailable", query: HOSTILE, message: "no record matched" },
  });
  (detail.summary as Json).title = HOSTILE_MARKUP;
  (detail.task as Json).title = HOSTILE_MARKUP;
  const receipt: Json = { ...recorded.receipt, proof_limitations: HOSTILE };
  receipt.chain = { ...(receipt.chain as Json), note: HOSTILE };
  return { detail, receipt };
}

/** The archive's real manifest with the note on `file` replaced by the hostile text. */
function hostileManifest(file: string): Buffer {
  const manifest = JSON.parse(researchBytes("MANIFEST.json").toString("utf8")) as { files: Json[] };
  const target = manifest.files.find((f) => f.path === file);
  expect(target, file).toBeDefined();
  for (const f of manifest.files) {
    if (f.path === file) {
      f.description = HOSTILE;
    }
  }
  return Buffer.from(JSON.stringify(manifest, null, 2));
}

/**
 * Nothing hostile became anything but text: no element it carries, no link
 * to a javascript: URL or to the hostile host, no image, frame or script it
 * would load, no element at all inside a quoted text, no handler that ran;
 * and no bidi control reorders the page's own words: in the paragraph a
 * control is in (its nearest block, unless an isolating element closes it
 * first), every word of the page's own that comes after it still reads left
 * to right on screen (its first letter left of its last, measured).
 */
async function expectInert(page: Page, where: string) {
  await expect(page.locator("[data-hostile]"), where).toHaveCount(0);
  await expect(page.locator("a[href^='javascript:' i], [href*='example.invalid'], [src*='example.invalid']"), where).toHaveCount(0);
  await expect(page.locator("main iframe, main img[src='/hostile.png'], main script"), where).toHaveCount(0);
  await expect(page.locator("main strong, main em"), where).toHaveCount(0);
  await expect(page.locator("[data-untrusted] blockquote *, .hc-untrusted-text *"), where).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { __hostile?: unknown }).__hostile), where).toBeUndefined();
  const leaks = await page.locator("main").evaluate((main) => {
    // U+202A to U+202E (embeddings, overrides) and U+2066 to U+2069 (isolates).
    const char = String.fromCharCode;
    const bidi = new RegExp(`[${char(0x202a)}-${char(0x202e)}${char(0x2066)}-${char(0x2069)}]`);
    /** The paragraph a control in `element` reaches: its nearest block; null when an isolating element closes it first. */
    const paragraphOf = (element: Element | null): Element | null => {
      for (let node = element; node !== null && node !== main; node = node.parentElement) {
        const style = getComputedStyle(node);
        if (style.display !== "inline" && style.display !== "contents") {
          return node;
        }
        if (node.tagName === "BDI" || ["isolate", "isolate-override", "plaintext"].includes(style.unicodeBidi)) {
          return null;
        }
      }
      return main;
    };
    const box = (node: Text, index: number) => {
      const range = document.createRange();
      range.setStart(node, index);
      range.setEnd(node, index + 1);
      return range.getBoundingClientRect();
    };
    /** The letters of node[from, to) are drawn right to left: the first one right of the last one, on one line. */
    const reversed = (node: Text, from: number, to: number) => {
      const letters = [...(node.textContent ?? "").slice(from, to).matchAll(/\p{L}/gu)].map((m) => from + (m.index ?? 0));
      if (letters.length < 2) {
        return false;
      }
      const first = box(node, letters[0] ?? 0);
      const last = box(node, letters.at(-1) ?? 0);
      return first.width > 0 && Math.abs(first.top - last.top) < 2 && first.left > last.left;
    };
    const texts = (root: Node) => {
      const all: Text[] = [];
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
        all.push(node as Text);
      }
      return all;
    };
    const found: string[] = [];
    for (const node of texts(main)) {
      if (!bidi.test(node.textContent ?? "")) {
        continue;
      }
      const paragraph = paragraphOf(node.parentElement);
      if (paragraph === null) {
        continue;
      }
      // The page's own text after the hostile text in the same paragraph: the
      // rest of this text node past the quote's last word, then every later node.
      const text = node.textContent ?? "";
      const end = text.lastIndexOf("liat");
      if (end >= 0 && reversed(node, end + 4, text.length)) {
        found.push(`${paragraph.tagName}: ${text.slice(end + 4, end + 84)}`);
      }
      const later = texts(paragraph).filter(
        (other) => other !== node && node.compareDocumentPosition(other) & Node.DOCUMENT_POSITION_FOLLOWING,
      );
      for (const other of later) {
        if (!bidi.test(other.textContent ?? "") && reversed(other, 0, (other.textContent ?? "").length)) {
          found.push(`${paragraph.tagName}: ${(other.textContent ?? "").slice(0, 80)}`);
        }
      }
    }
    return found;
  });
  expect(leaks, `${where}: a bidi control in quoted text reverses the page's own words`).toEqual([]);
}

test("hostile markup, markdown and bidi controls in a recorded run render as inert text", async ({ page }) => {
  test.setTimeout(180_000);
  const { l01, r06, list } = entries();
  const record = String(r06.record_file);
  const showcase = list.map((entry) =>
    entry.run_id === r06.run_id || entry.run_id === l01.run_id ? { ...entry, title: HOSTILE_MARKUP, label: HOSTILE_MARKUP } : entry,
  );
  const hostileR06 = showcase.find((entry) => entry.run_id === r06.run_id) as Json;
  const hostileL01 = showcase.find((entry) => entry.run_id === l01.run_id) as Json;
  await mockApi(page, {
    showcase,
    recordedRuns: [
      hostileRun(hostileR06, { base: "retry" }),
      hostileRun(hostileL01, { base: "invalid_proof", generationFailed: true }),
    ],
    research: {
      "MANIFEST.json": hostileManifest(String(l01.file)),
      [record]: Buffer.from(
        JSON.stringify({ receipt: { run_id: r06.run_id, generated_at: "2026-10-27T10:00:00Z", proof_limitations: HOSTILE } }),
      ),
    },
    publishServedManifest: true,
  });

  // The list: titles and labels, and the manifest's note (its digest matches, so it is shown).
  await openShowcase(page, "en");
  await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
  const battery = page.locator("[data-battery='live-battery-2026-10-26']");
  await expect(battery.locator("[data-archive-note] blockquote")).toHaveText(HOSTILE);
  await expect(page.locator(`[data-showcase-run='${String(r06.run_id)}']`)).toContainText(HOSTILE_MARKUP);
  await expectInert(page, "/showcase");

  // A recorded run blocked because no proof could be generated: the header's
  // title, label and manifest note, and every tab with the run's own text.
  await openRecorded(page, "en", String(l01.run_id));
  await expect(page.locator("h2.hc-run-title")).toHaveText(HOSTILE_MARKUP);
  await expect(page.locator("[data-recorded-run]")).toContainText(HOSTILE_MARKUP);
  await expect(page.locator("[data-recorded-run] [data-archive-note] blockquote")).toHaveText(HOSTILE);
  await expectInert(page, "l01 decision");
  for (const tab of ["explorer", "evidence", "proofs", "audit", "meta", "decision"] as const) {
    await page.getByRole("tab", { name: t("en", `Run.tabs.${tab}`) }).click();
    await expect(page.locator(`[data-tab='${tab}']`)).toBeVisible();
    if (tab === "evidence") {
      // A record's content, and the query of a proof that could not be generated (model output).
      await expect(page.locator("[data-untrusted='record'] blockquote").first()).toHaveText(HOSTILE);
      await expect(page.locator("[data-untrusted='model'] blockquote").first()).toHaveText(HOSTILE);
    }
    await expectInert(page, `l01 ${tab}`);
  }

  // A recorded run with its receipt strings, its run record and its chain check.
  await openRecorded(page, "en", String(r06.run_id));
  await expectInert(page, "r06 decision");
  await page.getByRole("tab", { name: t("en", "Run.tabs.proofs") }).click();
  await expect(page.locator("[data-proof-limitations]")).toHaveText(HOSTILE);
  await page.locator("[data-run-time-receipt]").getByRole("button", { name: t("en", "Run.runTimeReceipt.read") }).click();
  await expect(page.locator("[data-run-record-limitations]")).toHaveText(HOSTILE);
  await expectInert(page, "r06 proofs");
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-receipt-chain-note]")).toHaveText(HOSTILE);
  await expect(page.locator("[data-receipt-limitations]")).toHaveText(HOSTILE);
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
  await expectInert(page, "r06 audit");
  for (const tab of ["explorer", "evidence", "meta"] as const) {
    await page.getByRole("tab", { name: t("en", `Run.tabs.${tab}`) }).click();
    await expect(page.locator(`[data-tab='${tab}']`)).toBeVisible();
    await expectInert(page, `r06 ${tab}`);
  }
});

test("the inertness check sees an injected element, a javascript: link and a bidi control among the page's words", async ({
  page,
}) => {
  // The control: what expectInert must report if any of it ever rendered.
  const { r06, list } = entries();
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await openRecorded(page, "en", String(r06.run_id));
  await expectInert(page, "untouched");
  for (const html of [
    "<b data-hostile>x</b>",
    '<a href="javascript:void(0)">x</a>',
    "<strong>x</strong>",
    '<figure data-untrusted="record"><blockquote>a <em>b</em></blockquote></figure>',
    `<p>The run <span>${String.fromCharCode(0x202e)}R06</span> of battery</p>`,
  ]) {
    await page.locator("[data-recorded-run]").evaluate((section, markup) => {
      section.insertAdjacentHTML("beforeend", `<div data-probe>${markup}</div>`);
    }, html);
    const caught = await expectInert(page, html).then(
      () => null,
      (error: unknown) => error,
    );
    expect(caught, `expectInert missed ${html}`).not.toBeNull();
    await page.locator("[data-probe]").evaluate((probe) => probe.remove());
  }
  await expectInert(page, "cleaned");
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

// A11Y-I18N-9: axe over the W2 states the loops above do not reach (a chain
// check that fails, one checked in part, a schema-1.0 run and its "no chain
// to check", a manifest that does not match its digest, the listing that
// cannot be read, a recorded run that cannot be read, a run the listing does
// not name), in both themes, then the Overview's figures, the list and a
// recorded run on a phone (390x844, the suite's phone convention); and the
// recorded run's tabs and chain check driven by the keyboard alone.

/** The archived log with one letter of the record `offset` records into the run's segment flipped in case. */
function flippedLog(file: string, runId: string, offset = 3): { bytes: Buffer; sequence: number } {
  const sequence = logSegment(file, runId).first + offset;
  const original = researchBytes(file);
  const lines = original.toString("utf8").split("\n");
  const line = lines[sequence - 1] ?? "";
  const start = lines.slice(0, sequence - 1).reduce((n, l) => n + Buffer.byteLength(l) + 1, 0);
  const payloadAt = line.indexOf('"payload":');
  const letter = /"[a-z_]+":"([a-z])/.exec(line.slice(payloadAt));
  if (letter === null) {
    throw new Error(`no letter to flip in record ${sequence}`);
  }
  const at = start + Buffer.byteLength(line.slice(0, payloadAt + letter.index + letter[0].length - 1));
  const bytes = Buffer.from(original);
  bytes[at] = (bytes[at] ?? 0) ^ 0x20;
  return { bytes, sequence };
}

/**
 * A recorded run that began before schema 1.1, as a test log: two records
 * without a hash, then one chained to them (chained as event_log.rs chains,
 * src/lib/chain/verify.ts), the entry's digest, the detail's chain and the
 * receipt's head and count set to match. No archived run spans the
 * boundary (tests/chain-verify.test.ts), so the bytes are made here.
 */
async function spanningRun(entry: Json): Promise<{ entry: Json; recorded: { detail: Json; receipt: Json }; bytes: Buffer }> {
  const runId = String(entry.run_id);
  const record = (sequence: number, schema: string, prev: string) =>
    `{"schema_version":"${schema}","sequence":${sequence},"event_id":"evt-span-${sequence}","run_id":"${runId}","actor":{"kind":"runtime"},"occurred_at":"2026-10-28T00:00:00Z","prev_hash":"${prev}","hash":"","payload":{"type":"note"}}`;
  const legacy = [record(1, "1.0", ""), record(2, "1.0", "")];
  const blank = record(3, "1.3", "pre-1.1:2");
  const head = await recordHash("pre-1.1:2", blank);
  if (head === null) {
    throw new Error("no hash field in the record");
  }
  const bytes = Buffer.from(`${[...legacy, blank.replace('"hash":""', `"hash":"${head}"`)].join("\n")}\n`);
  const spanning = { ...entry, sha256: createHash("sha256").update(bytes).digest("hex"), head_hash: head, event_count: 3 };
  const recorded = recordedRun(spanning, { base: "retry" });
  recorded.detail.chain = { scope: "run", first_sequence: 1, last_sequence: 3, head_hash: head };
  recorded.receipt.event_count = 3;
  recorded.receipt.chain = { ...(recorded.receipt.chain as Json), scope: "run", first_sequence: 1, last_sequence: 3, head_hash: head };
  return { entry: spanning, recorded, bytes };
}

async function runChainCheck(page: Page, locale: Locale, verdict: string) {
  await page.getByRole("tab", { name: t(locale, "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t(locale, "Run.chain.run") }).click();
  await expect(page.locator(`[data-chain-verdict='${verdict}']`)).toBeVisible();
}

for (const theme of themes) {
  test(`axe: the failure and edge states of the showcase and a recorded run (fr, ${theme})`, async ({
    page,
    pageProblems,
  }) => {
    test.setTimeout(300_000);
    const locale = "fr" as const;
    await useTheme(page, theme);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    const { c01, l01, r06, list } = entries();
    const scans: ScanSummary[] = [];
    const remock = async (options: Parameters<typeof mockApi>[1]) => {
      await page.unrouteAll({ behavior: "ignoreErrors" });
      return mockApi(page, options);
    };

    // A chain check that fails at a flipped byte.
    const flipped = flippedLog(String(r06.file), String(r06.run_id));
    await remock({ showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })], research: { [String(r06.file)]: flipped.bytes } });
    await openRecorded(page, locale, String(r06.run_id));
    await runChainCheck(page, locale, "fail");
    await expect(page.locator(`[data-chain-failure-sequence='${flipped.sequence}']`)).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /run recorded audit chain fails`));

    // A chain checked in part: a run that began before schema 1.1.
    const spanning = await spanningRun(r06);
    await remock({
      showcase: list.map((entry) => (entry.run_id === r06.run_id ? spanning.entry : entry)),
      recordedRuns: [spanning.recorded],
      research: { [String(r06.file)]: spanning.bytes },
    });
    await openRecorded(page, locale, String(r06.run_id));
    await runChainCheck(page, locale, "partial");
    scans.push(await scan(page, `${locale} ${theme} /run recorded audit chain partial`));

    // A schema-1.0 run: its header, and its chain check with no chain to check.
    await remock({ showcase: list, recordedRuns: [recordedRun(c01, { base: "retry" })] });
    await openRecorded(page, locale, String(c01.run_id));
    await expect(page.locator("[data-archive-label='unchained']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /run recorded schema 1.0 decision`));
    await runChainCheck(page, locale, "unchained");
    scans.push(await scan(page, `${locale} ${theme} /run recorded schema 1.0 audit`));

    // A manifest that does not match its digest: the list, and a blocked run that cites no note.
    const forged = Buffer.from(
      JSON.stringify({ created: "2026-10-27", files: [{ path: l01.file, sha256: "0", kind: "event_log", description: "forged note" }] }),
    );
    await remock({
      showcase: list,
      recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
      research: { "MANIFEST.json": forged },
    });
    await openShowcase(page, locale);
    await expect(page.locator("[data-manifest-check='differs']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /showcase manifest differs`));
    await openRecorded(page, locale, String(l01.run_id));
    await expect(page.locator("[data-archive-note-reference='differs']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /run recorded manifest differs`));

    // The listing cannot be read: the error with its retry.
    await remock({
      showcase: list,
      showcaseFailures: { count: 1, answer: { status: 503, body: { code: "shutting_down", message: "shutting down" } } },
    });
    await page.goto(localePath(locale, "showcase"));
    await hydrated(page);
    await expect(page.locator("[data-showcase='error'] [data-error-code='shutting_down']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /showcase listing error`));
    takeProblem(pageProblems, "HTTP 503: /api/v1/showcase");

    // A recorded run that cannot be read: the error, its retry and the way back.
    await remock({ showcase: list, recordedRuns: [] });
    await page.goto(showcaseRunPath(locale, String(r06.run_id)));
    await hydrated(page);
    await expect(page.locator("[data-run='error'] [data-error-code='not_found']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /run recorded detail error`));
    takeProblem(pageProblems, `HTTP 404: /api/v1/showcase/${String(r06.run_id)}`);

    // A recorded run the listing does not name: no file it cannot name.
    await remock({ showcase: [], recordedRuns: [recordedRun(r06, { base: "retry" })] });
    await openRecorded(page, locale, String(r06.run_id));
    await expect(page.locator("[data-recorded-run]")).toContainText(t(locale, "Showcase.run.bannerNoEntry"));
    scans.push(await scan(page, `${locale} ${theme} /run recorded not listed decision`));
    await page.getByRole("tab", { name: t(locale, "Run.tabs.audit") }).click();
    await expect(page.locator("[data-audit-log='archive-pending']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} /run recorded not listed audit`));

    await writeScans(`showcase-states-${locale}-${theme}`, scans);
    expect(scans).toHaveLength(10);
    expect(blocking(scans)).toEqual([]);
  });

  test(`axe: the Overview's figures, the showcase and a recorded run on a phone (fr, ${theme})`, async ({ page }) => {
    test.setTimeout(240_000);
    const locale = "fr" as const;
    await useTheme(page, theme);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await page.setViewportSize({ width: 390, height: 844 });
    const { r06, list } = entries();
    await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
    const scans: ScanSummary[] = [];
    await page.goto(localePath(locale, ""));
    await hydrated(page);
    await expect(page.locator("[data-figures='ready']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} mobile / (figures)`));
    await openShowcase(page, locale);
    await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
    scans.push(await scan(page, `${locale} ${theme} mobile /showcase`));
    await openRecorded(page, locale, String(r06.run_id));
    scans.push(await scan(page, `${locale} ${theme} mobile /run recorded decision`));
    for (const tab of ["explorer", "evidence", "proofs", "audit", "meta"] as const) {
      await page.getByRole("tab", { name: t(locale, `Run.tabs.${tab}`) }).click();
      await expect(page.locator(`[data-tab='${tab}']`)).toBeVisible();
      if (tab === "audit") {
        await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
        await page.getByRole("button", { name: t(locale, "Run.chain.run") }).click();
        await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
      }
      scans.push(await scan(page, `${locale} ${theme} mobile /run recorded ${tab}`));
    }
    await writeScans(`showcase-mobile-${locale}-${theme}`, scans);
    expect(scans).toHaveLength(8);
    expect(blocking(scans)).toEqual([]);
  });
}

test("a recorded run's tabs follow the keyboard: arrows, Home and End move focus and selection", async ({ page }) => {
  const { r06, list } = entries();
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await openRecorded(page, "en", String(r06.run_id));
  const tab = (name: string) => page.getByRole("tab", { name: t("en", `Run.tabs.${name}`) });
  const order = ["explorer", "evidence", "proofs", "audit", "meta", "decision"] as const;
  await expect(page.getByRole("tab")).toHaveText(order.map((name) => t("en", `Run.tabs.${name}`)));
  /** The focused tab is the selected one, the only one in the tab order, and the panel shows it. */
  const expectOn = async (name: (typeof order)[number]) => {
    await expect(tab(name)).toBeFocused();
    await expect(tab(name)).toHaveAttribute("aria-selected", "true");
    await expect(tab(name)).toHaveAttribute("tabindex", "0");
    await expect(page.locator("[role='tab'][tabindex='0']")).toHaveCount(1);
    await expect(page.locator("[role='tab'][aria-selected='true']")).toHaveCount(1);
    await expect(page.locator("#run-tabpanel")).toHaveAttribute("data-tab", name);
    await expect(page.locator("#run-tabpanel")).toHaveAttribute("aria-labelledby", `run-tabpanel-tab-${name}`);
  };
  // Decision is selected first.
  await tab("decision").focus();
  await expectOn("decision");
  // Right from the last tab wraps to the first; Left from the first wraps to the last.
  await page.keyboard.press("ArrowRight");
  await expectOn("explorer");
  await page.keyboard.press("ArrowRight");
  await expectOn("evidence");
  await page.keyboard.press("ArrowLeft");
  await expectOn("explorer");
  await page.keyboard.press("ArrowLeft");
  await expectOn("decision");
  await page.keyboard.press("Home");
  await expectOn("explorer");
  await page.keyboard.press("End");
  await expectOn("decision");
  await page.keyboard.press("Home");
  for (const next of ["evidence", "proofs", "audit"] as const) {
    await page.keyboard.press("ArrowRight");
    await expectOn(next);
  }
  // Tab leaves the tab list: the other tabs are out of the tab order.
  await page.keyboard.press("Tab");
  await expect(page.locator("[role='tab']:focus")).toHaveCount(0);
  await expect(page.locator("#run-tabpanel")).toHaveAttribute("data-tab", "audit");
});

test("a recorded run's chain check runs from the keyboard", async ({ page }) => {
  const { r06, list } = entries();
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  await openRecorded(page, "en", String(r06.run_id));
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  // Enter on the button runs the check.
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
  // Space on "check again" runs it again. (Where focus goes once a check is
  // done is A11Y-I18N-1, fixed with its own regression test.)
  const again = page.getByRole("button", { name: t("en", "Run.chain.again") });
  await again.focus();
  await page.keyboard.press("Space");
  await expect(page.locator("[data-chain-verdict='pass']")).toBeVisible();
  await expect(again).toBeVisible();
});
