// SPDX-License-Identifier: Apache-2.0
//
// Every status role, as the browser draws it, without hue
// (design/palette-mono.md 2.1; helpers.ts ROLE_ENCODING): border style,
// fill weight, polarity, the simulated hatch strip, the LIVE square, the
// pending bar and the role's icon, in both themes. The Overview legend
// carries SIMULATED, LIVE META, META MODEL, SEALED SNAPSHOT, RECORDED and
// REPLAY; My runs a run waiting for the visitor's decision (pending), its
// blocked outcome (failed), a supported one (info) and the SCRIPTED and
// THIS SERVER labels; the Evidence tab a real verifier's accepted proof
// (verified); a flagged run its flag (contested). The chain check
// ("checked") is asserted where it runs (api.spec.ts, showcase.spec.ts).
// The LIVE square pulses three times and stops, and never under reduced
// motion; the hatch tile it needs is served; tags are rectangles.

import type { Locator, Page } from "@playwright/test";

import { VARIANT_ROLES } from "../src/lib/provenance";

import {
  framesWith,
  liveModelDetail,
  mockApi,
  RUN_ID,
  sealedSnapshotDetail,
} from "./api-mock";
import { fixture } from "./contract";
import { expect, hydrated, t, test, useTheme } from "./fixtures";
import { EXPECTED_VARIANT_ROLES, expectEncoding, expectNotVerified, openRun, tagEncodings } from "./helpers";

type Json = Record<string, unknown>;

function badge(scope: Page | Locator, text: string): Locator {
  return scope.locator(".astryx-badge", { hasText: new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`) });
}

/** The icon each tag carries, in order (null for none). */
async function icons(badges: Locator): Promise<Array<string | null>> {
  return (await tagEncodings(badges)).map((encoding) => encoding.icon);
}

for (const theme of ["light", "dark"] as const) {
  test(`the provenance legend draws each role by shape, fill, hatch and icon (${theme})`, async ({ page }) => {
    await useTheme(page, theme);
    await mockApi(page);
    await page.goto("/en/");
    await hydrated(page);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const legend = page.locator(".hc-provenance-list");
    await expect(legend.locator(".astryx-badge").first()).toBeVisible();

    const simulated = badge(legend, t("en", "Provenance.simulated.label"));
    await expect(simulated).toHaveCount(2);
    await expectEncoding(simulated, "simulated");
    expect(await icons(simulated)).toEqual(["flask-conical", "flask-conical"]);

    const liveMeta = badge(legend, t("en", "Provenance.liveMeta.label"));
    await expectEncoding(liveMeta, "live");
    expect(await icons(liveMeta)).toEqual([null]);

    const metaModel = badge(legend, t("en", "Provenance.metaModel.label"));
    await expectEncoding(metaModel, "info");
    expect(await icons(metaModel)).toEqual(["bot", "bot"]);

    const sealed = badge(legend, t("en", "Provenance.liveMeta.evidence"));
    await expectEncoding(sealed, "info");
    expect(await icons(sealed)).toEqual(["package", "package"]);

    const recorded = badge(legend, t("en", "Provenance.recorded.label"));
    await expect(recorded).toHaveAttribute("data-tag-role", "recorded");
    await expectEncoding(recorded, "recorded");
    expect(await icons(recorded)).toEqual(["archive"]);

    const replay = badge(legend, t("en", "Provenance.replay.label"));
    await expectEncoding(replay, "info");
    expect(await icons(replay)).toEqual(["history"]);

    // RECORDED's mid tint is its own: no other legend tag shares it.
    const all = await tagEncodings(legend.locator(".astryx-badge"));
    const texts = (await legend.locator(".astryx-badge").allInnerTexts()).map((text) => text.trim());
    expect(texts.filter((_, index) => all[index]?.fill === "mid")).toEqual([t("en", "Provenance.recorded.label")]);
    // The hatch is SIMULATED's alone, the square LIVE META's alone, and nothing looks verified.
    expect(texts.filter((_, index) => all[index]?.hatch)).toEqual([
      t("en", "Provenance.simulated.label"),
      t("en", "Provenance.simulated.label"),
    ]);
    expect(texts.filter((_, index) => all[index]?.marker)).toEqual([t("en", "Provenance.liveMeta.label")]);
    for (const encoding of all) {
      expectNotVerified(encoding);
    }

    // The tile behind the hatch is served (a missing tile would leave a dashed tag without its strip).
    const tile = await page.request.get(`/brand/hatch-${theme}.png`);
    expect(tile.status()).toBe(200);
    expect(tile.headers()["content-type"]).toContain("image/png");

    // Tags are rectangles of radius 2, never pills.
    const radii = await legend
      .locator(".astryx-badge")
      .evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).borderTopLeftRadius));
    expect(new Set(radii)).toEqual(new Set(["2px"]));
  });
}

test("the LIVE square pulses three times and stops, and never moves under reduced motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await mockApi(page);
  await page.goto("/en/");
  await hydrated(page);
  const live = badge(page.locator(".hc-provenance-list"), t("en", "Provenance.liveMeta.label"));
  const pulse = () =>
    live.evaluate((node) => {
      const before = getComputedStyle(node, "::before");
      return { name: before.animationName, count: before.animationIterationCount, duration: before.animationDuration };
    });
  expect(await pulse()).toEqual({ name: "hc-live-pulse", count: "3", duration: "1.6s" });

  await page.emulateMedia({ reducedMotion: "reduce" });
  expect((await pulse()).name).toBe("none");

  // The in-app Reduced setting does the same without the OS preference.
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.evaluate(() => document.documentElement.setAttribute("data-motion", "reduce"));
  expect((await pulse()).name).toBe("none");
});

for (const theme of ["light", "dark"] as const) {
  test(`My runs: the pending decision, a blocked outcome and the run's labels each keep their role (${theme})`, async ({
    page,
  }) => {
    await useTheme(page, theme);
    const summaries = ["retry", "invalid_proof"].map((name) => fixture<Json>(`run-summary-${name}.json`));
    await mockApi(page, { runs: summaries });
    await page.goto("/en/runs/");
    await hydrated(page);
    const waiting = page.locator("[data-run-id='run-00000000-0000-4000-8000-000000000003']");
    await expect(waiting).toBeVisible();
    // Waiting for your decision: the strong tint, the 2px frame and the ink bar, the person icon.
    const pending = waiting.locator("[data-status-tag]");
    await expect(pending).toHaveAttribute("data-tag-role", "pending");
    await expectEncoding(pending, "pending");
    expect(await icons(pending)).toEqual(["user-round-check"]);
    // Blocked: full ink inside the double frame, the square cross.
    const blocked = waiting.locator("[data-outcome-tag='outcome']");
    await expect(blocked).toHaveAttribute("data-variant", "error");
    await expectEncoding(blocked, "error");
    expect(await icons(blocked)).toEqual(["square-x"]);
    // Its evidence: SIMULATED, dashed with the hatch and the flask, never verified.
    await expectEncoding(waiting.locator("[data-outcome-tag='evidence']"), "simulated");

    const supported = page.locator(`[data-run-id='${RUN_ID}'] [data-outcome-tag='outcome']`);
    await expectEncoding(supported, "info");
    expect(await icons(supported)).toEqual(["circle-dot"]);
    // SCRIPTED is simulated (the code file), THIS SERVER informational (the server).
    const scripted = badge(page, t("en", "Provenance.scripted.label"));
    await expect(scripted).toHaveCount(2);
    await expectEncoding(scripted, "simulated");
    expect(await icons(scripted)).toEqual(["file-code-corner", "file-code-corner"]);
    const server = badge(page, t("en", "Provenance.thisServer.label"));
    await expectEncoding(server, "info");
    expect(await icons(server)).toEqual(["server", "server"]);
    // The pending look is the waiting state's alone; nothing on the page looks verified.
    const all = await tagEncodings(page.locator(".astryx-badge"));
    expect(all.filter((encoding) => encoding.bar)).toHaveLength(1);
    for (const encoding of all) {
      expectNotVerified(encoding);
    }
  });
}

test("a real verifier's accepted proof is the only verified tag, with the shield", async ({ page }) => {
  const detail = sealedSnapshotDetail();
  const provenance = (detail.summary as { provenance: Json }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  await page.getByRole("tab", { name: t("en", "Run.tabs.evidence") }).click();
  const accepted = page.locator("[data-proof-simulated='false'][data-proof-status='valid'] .astryx-badge[data-variant='success']");
  await expect(accepted.first()).toBeVisible();
  await expectEncoding(accepted, "verified");
  for (const icon of await icons(accepted)) {
    expect(icon).toBe("shield-check");
  }
  // Every other tag of the tab is something else.
  const others = page.locator("[role=tabpanel] .astryx-badge:not([data-variant='success'])");
  for (const encoding of await tagEncodings(others)) {
    expectNotVerified(encoding);
  }
});

test("a flag is contested: the double frame on a tint and the warning triangle", async ({ page }) => {
  const detail = liveModelDetail(["provenance_inconsistent"]);
  const provenance = (detail.summary as { provenance: Json }).provenance;
  await mockApi(page, { detail, frames: framesWith(provenance, "replay") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openRun(page, "en");
  const header = page.locator("header.hc-run-header");
  const flag = badge(header.locator(".hc-badges"), t("en", "Provenance.flags.provenance_inconsistent"));
  await expect(flag).toBeVisible();
  await expectEncoding(flag, "contested");
  expect(await icons(flag)).toEqual(["triangle-alert"]);
});

test("the app draws each badge variant as the role the e2e checks expect, written out independently", () => {
  // helpers.ts writes the variant-to-role table out instead of reading the
  // app's VARIANT_ROLES, so the encoding checks above cannot move with a
  // regression of the app's mapping; this pins the two together.
  expect(VARIANT_ROLES).toEqual(EXPECTED_VARIANT_ROLES);
});
