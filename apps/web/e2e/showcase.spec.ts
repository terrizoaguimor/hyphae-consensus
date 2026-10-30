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

import { createHash } from "node:crypto";

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { IntlMessageFormat } from "intl-messageformat";

import { KNOWN_PROOF_LIMITATIONS } from "../src/lib/run/receipt";
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
  // The listed digest agrees with the checked manifest's own pin.
  await expect(battery.locator("[data-log-pin='matches']")).toHaveText(t("en", "Showcase.pin.matches"));
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
  const { l01, r06, list } = entries();
  const forged = Buffer.from(
    JSON.stringify({
      created: "2026-10-27",
      files: [
        { path: "2026-10-26/live-battery-2026-10-26.jsonl", sha256: "0", kind: "event_log", description: "forged note" },
        // A forged pin that parses and agrees with the listing: still not the checked manifest's.
        { path: String(r06.file), sha256: String(r06.sha256), kind: "event_log", description: "forged note on R06" },
      ],
    }),
  );
  await mockApi(page, {
    showcase: list,
    research: { "MANIFEST.json": forged },
    recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true }), recordedRun(r06, { base: "retry" })],
  });
  await openShowcase(page, "en");
  await expect(page.locator("[data-manifest-check='differs']")).toBeVisible();
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  await expect(page.locator("main")).not.toContainText("forged note");
  // The outcome is announced by the status region that was there, empty, while the manifest was checked.
  await expect(page.locator("[data-manifest-announcement]")).toContainText(
    "Checked in this browser: the manifest’s bytes hash to",
  );
  await expect(page.locator("[data-manifest-announcement]")).toContainText("not to the published SHA-256");
  // GATES-03: no log digest reads as the manifest's pin.
  await expect(page.locator("[data-log-pin='matches']")).toHaveCount(0);
  await expect(page.locator(`[data-archive-file='${String(r06.file)}'] [data-log-pin='unchecked']`)).toHaveText(
    t("en", "Showcase.pin.unchecked"),
  );

  // GATES-03: the recorded run header of the 2026-10-26 blocked battery lends nothing of it either.
  await openRecorded(page, "en", String(l01.run_id));
  const header = page.locator("[data-recorded-run]");
  await expect(header.locator("[data-manifest-problem='differs']")).toHaveText(
    t("en", "Showcase.run.manifestProblem.differs"),
  );
  await expect(header.locator("[data-archive-note-reference='differs']")).toBeVisible();
  // The header says why once; the blocked-run sentence points to it instead of repeating it.
  const said = (await header.innerText()).split("hash to the published SHA-256").length - 1;
  expect(said).toBe(1);
  await expect(header.locator("[data-archive-note-reference='differs']")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.differs"),
  );
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  await expect(page.locator("main")).not.toContainText("forged note");
  // And a run that is not blocked (R06), whose forged entry agrees with the listing.
  await openRecorded(page, "en", String(r06.run_id));
  await expect(page.locator("[data-recorded-run] [data-manifest-problem='differs']")).toBeVisible();
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  await expect(page.locator("main")).not.toContainText("forged note");
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-log] [data-log-pin='unchecked']")).toHaveText(t("en", "Showcase.pin.unchecked"));
  await expect(page.locator("[data-log-pin='matches']")).toHaveCount(0);
});

// RECORDED-UX-1: a recorded run whose listing cannot be read says so, with
// a retry, and names what it leaves out; never "not read, or not listed".
test("a recorded run whose showcase listing cannot be read says what it leaves out, with a retry", async ({
  page,
  pageProblems,
}) => {
  const { r06, list } = entries();
  const runId = String(r06.run_id);
  const mock = await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(r06, { base: "retry" })],
    showcaseFailures: { count: 1, answer: { status: 503, body: { code: "shutting_down", message: "shutting down" } } },
  });
  await openRecorded(page, "en", runId);
  const header = page.locator("[data-recorded-run]");
  const failed = header.locator("[data-archive-error]");
  await expect(failed).toContainText(t("en", "Showcase.run.listingFailed"));
  await expect(failed.locator("[data-error-code='shutting_down']")).toContainText(t("en", "Api.errors.shutting_down"));
  // The Decision tab names the cause in place of the log link.
  await expect(page.locator("[data-ndjson-unavailable='error']")).toHaveText(t("en", "Run.decision.archiveLogFailed"));
  await expect(page.locator("[data-ndjson-link]")).toHaveCount(0);
  await expect(page.getByText(t("en", "Run.decision.archiveLogPending"))).toHaveCount(0);
  // So does the Audit trail: no log, digest, command or chain check, and why.
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-log='archive-error'] [data-log-unavailable]")).toHaveText(
    t("en", "Run.audit.archiveFailed"),
  );
  await expect(page.getByText(t("en", "Run.audit.archivePending"))).toHaveCount(0);
  await expect(page.locator("[data-cli]")).toHaveCount(0);
  await expect(page.locator("[data-chain-check]")).toHaveCount(0);
  expect(mock.calls.filter((c) => c.path === "/api/v1/showcase")).toHaveLength(1);

  // The retry reads the listing again: the entry, its log, digest, command and check appear.
  await failed.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(header.locator("[data-archive-error]")).toHaveCount(0);
  await expect(header).toContainText(t("en", "Showcase.run.banner", { label: "R06", battery: "live-battery-2026-10-27" }));
  await expect(page.locator("[data-audit-log='published'] [data-log-link]")).toHaveAttribute(
    "href",
    `/research/${String(r06.file)}`,
  );
  await expect(page.locator("[data-log-sha256]")).toHaveText(String(r06.sha256));
  await expect(page.locator("[data-log-pin='matches']")).toHaveText(t("en", "Showcase.pin.matches"));
  await expect(page.locator("[data-cli]")).toBeVisible();
  await expect(page.locator("[data-chain-check]")).toBeVisible();
  expect(mock.calls.filter((c) => c.path === "/api/v1/showcase")).toHaveLength(2);
  takeProblem(pageProblems, "HTTP 503: /api/v1/showcase");
});

test("a recorded run whose detail and listing both fail: the run's retry reads the listing again too", async ({
  page,
  pageProblems,
}) => {
  const { r06, list } = entries();
  const runId = String(r06.run_id);
  const mock = await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(r06, { base: "retry" })],
    showcaseFailures: { count: 1, answer: { status: 503, body: { code: "shutting_down", message: "shutting down" } } },
  });
  // Registered after mockApi: the first GET /showcase/{id} fails, then the mock answers.
  let detailFailures = 1;
  await page.route(`**/api/v1/showcase/${runId}`, async (route) => {
    if (detailFailures > 0) {
      detailFailures -= 1;
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "shutting_down", message: "shutting down" }),
      });
      return;
    }
    await route.fallback();
  });
  await page.goto(showcaseRunPath("en", runId));
  await hydrated(page);
  const error = page.locator("[data-run='error']");
  await expect(error.locator("[data-error-code='shutting_down']")).toBeVisible();
  await expect.poll(() => mock.calls.filter((c) => c.path === "/api/v1/showcase").length).toBe(1);
  await error.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(page.locator("[data-run-mode='recorded']")).toBeVisible();
  // The listing was read again with the detail: the run is named, with its log.
  await expect(page.locator("[data-recorded-run]")).toContainText(
    t("en", "Showcase.run.banner", { label: "R06", battery: "live-battery-2026-10-27" }),
  );
  await expect(page.locator("[data-archive-error]")).toHaveCount(0);
  await expect(page.locator("[data-ndjson-link]")).toHaveAttribute("href", `/research/${String(r06.file)}`);
  expect(mock.calls.filter((c) => c.path === "/api/v1/showcase")).toHaveLength(2);
  takeProblem(pageProblems, `HTTP 503: /api/v1/showcase/${runId}`);
  takeProblem(pageProblems, "HTTP 503: /api/v1/showcase");
});

// RECORDED-UX-3: a manifest that cannot be downloaded has its own retry,
// and a recorded run says its note could not be read instead of dropping it.
test("a manifest that cannot be downloaded is said, with a retry of the manifest alone", async ({ page, pageProblems }) => {
  const { list } = entries();
  const mock = await mockApi(page, { showcase: list });
  // Answers the first manifest request with an error, then hands the rest to the archive route.
  let failures = 1;
  let manifestRequests = 0;
  await page.route("**/research/MANIFEST.json", async (route) => {
    manifestRequests += 1;
    if (failures > 0) {
      failures -= 1;
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "shutting_down", message: "shutting down" }),
      });
      return;
    }
    await route.fallback();
  });
  await openShowcase(page, "en");
  const line = page.locator("[data-manifest='error']");
  await expect(line.locator("[data-error-code='shutting_down']")).toContainText(t("en", "Api.errors.shutting_down"));
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  await expect(page.locator("[data-log-pin='unchecked']").first()).toHaveText(t("en", "Showcase.pin.unchecked"));
  await line.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(page.locator("[data-manifest-check='matches']")).toHaveText(t("en", "Showcase.manifest.matches"));
  await expect(
    page.locator("[data-battery='live-battery-2026-10-26'] [data-archive-note] blockquote"),
  ).toContainText("a deployment misconfiguration");
  await expect(page.locator("[data-log-pin='unchecked']")).toHaveCount(0);
  // Only the manifest was read again, not the listing.
  expect(manifestRequests).toBe(2);
  expect(mock.calls.filter((c) => c.path === "/research/MANIFEST.json")).toHaveLength(1);
  expect(mock.calls.filter((c) => c.path === "/api/v1/showcase")).toHaveLength(1);
  takeProblem(pageProblems, "HTTP 503: /research/MANIFEST.json");
  dropProblems(pageProblems, /^console: Failed to load resource: the server responded with a status of 503/);
});

test("a recorded run whose manifest cannot be downloaded says its note is not shown, with a retry", async ({
  page,
  pageProblems,
}) => {
  const { l01, list } = entries();
  const mock = await mockApi(page, {
    showcase: list,
    recordedRuns: [recordedRun(l01, { base: "invalid_proof", generationFailed: true })],
  });
  // Answers the first manifest request with an error, then hands the rest to the archive route.
  let failures = 1;
  let manifestRequests = 0;
  await page.route("**/research/MANIFEST.json", async (route) => {
    manifestRequests += 1;
    if (failures > 0) {
      failures -= 1;
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "shutting_down", message: "shutting down" }),
      });
      return;
    }
    await route.fallback();
  });
  await openRecorded(page, "en", String(l01.run_id));
  const header = page.locator("[data-recorded-run]");
  const problem = header.locator("[data-manifest-problem='unread']");
  await expect(problem).toContainText(t("en", "Showcase.run.manifestProblem.unread"));
  await expect(problem.locator("[data-error-code='shutting_down']")).toContainText(t("en", "Api.errors.shutting_down"));
  await expect(page.locator("[data-archive-note]")).toHaveCount(0);
  // The header says why once; the blocked-run sentence points to it instead of repeating it.
  await expect(header.locator("[data-archive-note-reference='unread']")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.unread"),
  );
  const said = (await header.innerText()).split("manifest could not be read").length - 1;
  expect(said).toBe(1);
  await problem.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(header.locator("[data-manifest-problem]")).toHaveCount(0);
  await expect(header.locator("[data-archive-note] blockquote")).toContainText("a deployment misconfiguration");
  await expect(header.locator("[data-archive-note-reference='shown']")).toHaveText(
    t("en", "Showcase.run.generationFailedNote.shown"),
  );
  expect(manifestRequests).toBe(2);
  expect(mock.calls.filter((c) => c.path === "/research/MANIFEST.json")).toHaveLength(1);
  expect(mock.calls.filter((c) => c.path === "/api/v1/showcase")).toHaveLength(1);
  takeProblem(pageProblems, "HTTP 503: /research/MANIFEST.json");
  dropProblems(pageProblems, /^console: Failed to load resource: the server responded with a status of 503/);
});

// CRITIC-2: the listed digest of a log is compared with the checked
// manifest's own pin, and the chain check compares the file with that pin.
test("a listed log digest that differs from the checked manifest's pin is said, and the file is checked against the manifest", async ({
  page,
}) => {
  const { c01, l01, r06 } = entries();
  const file = String(r06.file);
  // A rewritten log served with the listing re-hashed to it; the manifest is the genuine one.
  const original = researchBytes(file);
  const at = original.indexOf(Buffer.from('"type":"'));
  expect(at).toBeGreaterThanOrEqual(0);
  const rewritten = Buffer.from(original);
  rewritten[at + 8] = (rewritten[at + 8] ?? 0) ^ 0x20;
  const rewrittenSha = createHash("sha256").update(rewritten).digest("hex");
  const pinned = String(r06.sha256);
  expect(rewrittenSha).not.toBe(pinned);
  const listed = { ...r06, sha256: rewrittenSha };
  await mockApi(page, {
    showcase: [c01, l01, listed, F03],
    recordedRuns: [recordedRun(listed, { base: "retry" })],
    research: { [file]: rewritten },
  });
  await openShowcase(page, "en");
  await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
  const pin = page.locator(`[data-archive-file='${file}'] [data-log-pin='differs']`);
  await expect(pin).toContainText(t("en", "Showcase.pin.differsBadge"));
  await expect(pin).toContainText(pinned.slice(0, 12));
  await expect(pin).toContainText(rewrittenSha.slice(0, 12));
  // The digest said to be pinned by the manifest is the manifest's own, not the listing's.
  const logFile = page.locator(`[data-archive-file='${file}'] [data-log-file-sha256]`);
  await expect(logFile).toHaveAttribute("data-log-file-sha256", pinned);
  await expect(logFile).toHaveText(t("en", "Showcase.logFile", { file, sha256: pinned.slice(0, 12) }));
  // The other battery's log agrees with the manifest.
  await expect(page.locator("[data-archive-file='2026-10-26/live-battery-2026-10-26.jsonl'] [data-log-pin='matches']")).toHaveText(
    t("en", "Showcase.pin.matches"),
  );

  await openRecorded(page, "en", String(r06.run_id));
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  // Under "pinned by MANIFEST.json", the checked manifest's own pin; the listing's digest under its own label.
  await expect(page.locator("[data-log-sha256]")).toHaveText(pinned);
  await expect(page.locator("[data-audit-log] dt").first()).toHaveText(t("en", "Run.audit.pinnedSha256"));
  const listedRow = page.locator("[data-audit-log] .hc-kv-wide").filter({ has: page.locator("[data-log-listed-sha256]") });
  await expect(listedRow.locator("dt")).toHaveText(t("en", "Run.audit.listedSha256"));
  await expect(listedRow.locator("[data-log-listed-sha256]")).toHaveText(rewrittenSha);
  await expect(page.locator("[data-audit-log]")).toContainText(t("en", "Run.audit.logArchive"));
  const auditPin = page.locator("[data-audit-log] [data-log-pin='differs']");
  await expect(auditPin).toContainText(pinned.slice(0, 12));
  await expect(auditPin).toContainText(rewrittenSha.slice(0, 12));
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const verdict = page.locator("[data-chain-verdict='fail']");
  await expect(verdict).toBeVisible();
  const sha256 = verdict.locator("[data-chain-result='sha256']");
  // The file is the listed one, not the one the checked manifest pins: a mismatch.
  await expect(sha256).toContainText(t("en", "Run.chain.verdicts.fail"));
  await expect(sha256).not.toContainText(t("en", "Run.chain.verdicts.pass"));
  await expect(sha256).toContainText(rewrittenSha);
  await expect(sha256).toContainText("against the digest the archive’s manifest pins");
  await expect(sha256).toContainText(t("en", "Run.chain.sha256", { sha256: rewrittenSha, bytes: rewritten.length }));
  await expect(sha256).not.toContainText(t("en", "Run.chain.sha256Listing", { sha256: rewrittenSha, bytes: rewritten.length }));
});

// A checked manifest that does not name the listed file: the digest is the
// server's word alone, and nothing calls it pinned by the manifest.
test("a listed log the checked manifest does not name is labelled as the server lists it", async ({ page }) => {
  const { c01, l01, r06 } = entries();
  const file = "2026-10-27/live-battery-2026-10-27-unnamed.jsonl";
  const bytes = researchBytes(String(r06.file));
  const listed = { ...r06, file };
  await mockApi(page, {
    showcase: [c01, l01, listed, F03],
    // The run's records are R06's own (read from its archived file); the listing names the same bytes under another path.
    recordedRuns: [recordedRun(r06, { base: "retry" })],
    research: { [file]: bytes },
  });
  await openShowcase(page, "en");
  await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
  const archived = page.locator(`[data-archive-file='${file}']`);
  await expect(archived.locator("[data-log-pin='unlisted']")).toHaveText(t("en", "Showcase.pin.unlisted"));
  await expect(archived.locator("[data-log-file-sha256]")).toHaveText(
    t("en", "Showcase.logFileListed", { file, sha256: String(r06.sha256).slice(0, 12) }),
  );
  await expect(archived).not.toContainText(t("en", "Showcase.logFile", { file, sha256: String(r06.sha256).slice(0, 12) }));

  await openRecorded(page, "en", String(r06.run_id));
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  const log = page.locator("[data-audit-log='published']");
  await expect(log).toContainText(t("en", "Run.audit.logArchiveUnlisted"));
  await expect(log).not.toContainText(t("en", "Run.audit.logArchive"));
  await expect(log.locator("[data-log-sha256]")).toHaveCount(0);
  await expect(log).not.toContainText(t("en", "Run.audit.pinnedSha256"));
  await expect(log.locator("[data-log-listed-sha256]")).toHaveText(String(r06.sha256));
  await expect(log.locator("[data-log-pin='unlisted']")).toHaveText(t("en", "Showcase.pin.unlisted"));
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const sha256 = page.locator("[data-chain-result='sha256']");
  await expect(sha256).toContainText(t("en", "Run.chain.sha256Unlisted", { sha256: String(r06.sha256), bytes: bytes.length }));
  await expect(sha256).not.toContainText(t("en", "Run.chain.sha256Listing", { sha256: String(r06.sha256), bytes: bytes.length }));
  await expect(sha256).toContainText(t("en", "Run.chain.verdicts.pass"));
});

// A finished chain check keeps saying what it compared the file with, even
// when the manifest is read afterwards and would lend its own pin.
test("a chain check run before the manifest was read keeps its listing wording after the manifest's retry", async ({
  page,
  pageProblems,
}) => {
  const { r06, list } = entries();
  await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
  let failures = 1;
  await page.route("**/research/MANIFEST.json", async (route) => {
    if (failures > 0) {
      failures -= 1;
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "shutting_down", message: "shutting down" }),
      });
      return;
    }
    await route.fallback();
  });
  await openRecorded(page, "en", String(r06.run_id));
  const problem = page.locator("[data-recorded-run] [data-manifest-problem='unread']");
  await expect(problem).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.audit") }).click();
  await expect(page.locator("[data-audit-log] [data-log-pin='unchecked']")).toBeVisible();
  await expect(page.locator("[data-audit-receipt='ready']")).toBeVisible();
  await page.getByRole("button", { name: t("en", "Run.chain.run") }).click();
  const sha256 = page.locator("[data-chain-result='sha256']");
  const bytes = researchBytes(String(r06.file)).length;
  const listing = t("en", "Run.chain.sha256Listing", { sha256: String(r06.sha256), bytes });
  const manifest = t("en", "Run.chain.sha256", { sha256: String(r06.sha256), bytes });
  await expect(sha256).toContainText(listing);
  // The manifest is read on its retry and pins this file: the finished check still names what it compared with.
  await problem.getByRole("button", { name: t("en", "Api.retry") }).click();
  await expect(page.locator("[data-audit-log] [data-log-pin='matches']")).toHaveText(t("en", "Showcase.pin.matches"));
  await expect(sha256).toContainText(listing);
  await expect(sha256).not.toContainText(manifest);
  takeProblem(pageProblems, "HTTP 503: /research/MANIFEST.json");
  dropProblems(pageProblems, /^console: Failed to load resource: the server responded with a status of 503/);
});

// While the manifest is still being checked, a log's pin reads as a loading line, not as a result.
test("a log digest reads as being checked while the manifest is still read", async ({ page }) => {
  const { list } = entries();
  await mockApi(page, { showcase: list });
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested: () => void = () => undefined;
  const asked = new Promise<void>((resolve) => {
    requested = resolve;
  });
  // Registered after mockApi, so it holds the manifest open; fallback() then
  // hands the request to the archive route, which serves the real bytes.
  await page.route("**/research/MANIFEST.json", async (route) => {
    requested();
    await held;
    await route.fallback();
  });
  await page.goto(localePath("en", "showcase"));
  await hydrated(page);
  await asked;
  const battery = page.locator("[data-battery='live-battery-2026-10-26']");
  const checking = battery.locator("[data-log-pin='checking']");
  await expect(checking).toHaveText(t("en", "Showcase.pin.checking"));
  await expect(checking).toHaveAttribute("role", "status");
  await expect(page.locator("[data-log-pin='unchecked']")).toHaveCount(0);
  release();
  await expect(battery.locator("[data-log-pin='matches']")).toHaveText(t("en", "Showcase.pin.matches"));
  await expect(page.locator("[data-log-pin='checking']")).toHaveCount(0);
});

// A11Y-I18N-2: the manifest check's status region is mounted empty while the
// manifest is checked, and only the outcome is written into it (once); the
// visible "checking" line and the outcome line are not live regions.
test("the manifest check announces its outcome through a region that was there before", async ({ page }) => {
  const { list } = entries();
  await mockApi(page, { showcase: list });
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested: () => void = () => undefined;
  const asked = new Promise<void>((resolve) => {
    requested = resolve;
  });
  // Registered after mockApi, so it holds the manifest open; fallback() then
  // hands the request to the archive route, which serves the real bytes.
  await page.route("**/research/MANIFEST.json", async (route) => {
    requested();
    await held;
    await route.fallback();
  });
  await openShowcase(page, "en");
  await asked;
  const announcement = page.locator("[data-manifest-announcement]");
  await expect(announcement).toHaveAttribute("role", "status");
  await expect(announcement).toHaveText("");
  await expect(page.locator("[data-manifest-checking]")).toHaveText(t("en", "Showcase.manifest.checking"));
  await expect(page.locator("[data-manifest-checking]")).not.toHaveAttribute("role", /.+/);
  const region = await announcement.elementHandle();
  release();
  await expect(page.locator("[data-manifest-check='matches']")).toHaveText(t("en", "Showcase.manifest.matches"));
  await expect(announcement).toHaveText(t("en", "Showcase.manifest.matches"));
  expect(await region?.evaluate((el) => el.isConnected)).toBe(true);
  await expect(page.locator("[data-manifest-check='matches']")).not.toHaveAttribute("role", /.+/);
  const live = await page
    .locator("[data-manifest]")
    .evaluate((root) =>
      [...root.querySelectorAll("[role='status'], [role='alert'], [aria-live]")].map((el) => el.textContent ?? ""),
    );
  expect(live).toEqual([t("en", "Showcase.manifest.matches")]);
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
