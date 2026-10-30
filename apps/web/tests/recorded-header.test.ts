// SPDX-License-Identifier: Apache-2.0
//
// RECORDED mode never reads LIVE (release gate G7), at the component that
// draws the header: the run workspace (src/components/run/RunWorkspace.tsx)
// rendered to HTML with React's server renderer, in RECORDED mode
// (?id=<run_id>&source=showcase), over a recorded run whose provenance alone
// would allow LIVE (model live_meta, no flag at all; an archived run whose
// task declares its mode carries no mode_undeclared flag). A recorded run
// has no stream, so the header is given no delivery: its badges are the
// model as META MODEL, the evidence and RECORDED, with no delivery badge
// (no LIVE, no STREAMING, no REPLAY) and the "Meta model · recorded" note.
// A header handed a live delivery (`delivery={recorded ? "live" : ...}`) or
// any delivery at all fails here, in every locale. The same badges given a
// live delivery do read LIVE and LIVE META, so the absence asserted here is
// not an artefact of the harness.
//
// The data hook (useRecordedRun) is replaced by its recorded answer, with
// the empty frame list the hook always returns; next/navigation answers the
// RECORDED-mode query, and the preferences context is given plain values.
// Nothing else is mocked.

import { readFileSync } from "node:fs";
import path from "node:path";

import { NextIntlClientProvider } from "next-intl";
import { type DefaultTreeAdapterMap, parseFragment } from "parse5";
import { type ComponentProps, createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeAll, describe, expect, it, vi } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import type { Preferences } from "../src/components/preferences";
import type { Locale } from "../src/i18n/routing";
import { routing } from "../src/i18n/routing";
import type { ProvenanceV1, RunDetailV1 } from "../src/lib/api/types";

type Catalog = { [key: string]: string | Catalog };
type Node = DefaultTreeAdapterMap["childNode"] | DefaultTreeAdapterMap["documentFragment"];
type Element = DefaultTreeAdapterMap["element"];

const fixtures = vi.hoisted(() => ({ runId: "", detail: null as unknown }));

vi.mock("next/navigation", () => ({
  useSearchParams: () => new URLSearchParams(`id=${fixtures.runId}&source=showcase`),
  usePathname: () => "/en/run/",
  useRouter: () => ({ push: () => undefined, replace: () => undefined, prefetch: () => undefined }),
}));

// Only the hook is replaced: the module's other exports (missingEntry, which
// the workspace derives the archive's missing parts from) stay the real ones.
vi.mock("../src/components/run/useRecordedRun", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useRecordedRun: () => {
    const detail = fixtures.detail as RunDetailV1;
    return {
      detail,
      detailError: null,
      // What useRecordedRun always answers: a recorded run has no stream.
      frames: [],
      summary: detail.summary,
      reloadDetail: () => undefined,
      archive: { status: "loading" },
      reloadArchive: () => undefined,
      retryManifest: () => undefined,
      entry: null,
    };
  },
}));

const read = (locale: string) =>
  JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8")) as Catalog;

function message(catalog: Catalog, key: string): string {
  let node: string | Catalog | undefined = catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key}`);
  }
  return node;
}

/** A recorded run that, by its provenance alone, could read LIVE: live_meta, no flag. */
function recordedDetail(): RunDetailV1 {
  const detail = JSON.parse(
    readFileSync(path.join(contractDir(), "fixtures", "run-detail-retry.json"), "utf8"),
  ) as RunDetailV1;
  const provenance: ProvenanceV1 = {
    ...detail.summary.provenance,
    model: "live_meta",
    evidence: "hyphae_live",
    source: "research_archive",
    flags: [],
    meta_request_ids: ["chatcmpl-recorded-1"],
  };
  detail.summary = { ...detail.summary, provenance, active: false, state: "awaiting_human_approval" };
  return detail;
}

const isElement = (node: Node): node is Element => "tagName" in node;
const children = (node: Node): Node[] => ("childNodes" in node ? (node.childNodes as Node[]) : []);
const attr = (node: Element, name: string) => node.attrs.find((a) => a.name === name)?.value ?? null;
const hasClass = (node: Element, name: string) => (attr(node, "class") ?? "").split(/\s+/).includes(name);

function all(node: Node, match: (element: Element) => boolean): Element[] {
  const found: Element[] = [];
  for (const child of children(node)) {
    if (isElement(child) && match(child)) {
      found.push(child);
    }
    found.push(...all(child, match));
  }
  return found;
}

function text(node: Node): string {
  if (node.nodeName === "#text") {
    return (node as DefaultTreeAdapterMap["textNode"]).value;
  }
  return children(node).map(text).join("");
}

function render(locale: Locale, catalog: Catalog, element: ReactElement): DefaultTreeAdapterMap["documentFragment"] {
  return parseFragment(
    renderToStaticMarkup(
      createElement(
        NextIntlClientProvider,
        { locale, messages: catalog, timeZone: "UTC" } as ComponentProps<typeof NextIntlClientProvider>,
        element,
      ),
    ),
  );
}

let RunWorkspace: typeof import("../src/components/run/RunWorkspace").RunWorkspace;
let ProvenanceBadges: typeof import("../src/components/ProvenanceBadges").ProvenanceBadges;
let PreferencesProvider: typeof import("../src/components/preferences").PreferencesProvider;

beforeAll(async () => {
  ({ RunWorkspace } = await import("../src/components/run/RunWorkspace"));
  ({ ProvenanceBadges } = await import("../src/components/ProvenanceBadges"));
  ({ PreferencesProvider } = await import("../src/components/preferences"));
}, 120_000);

const preferences: Preferences = {
  hydrated: true,
  colorMode: "light",
  theme: "system",
  motion: "system",
  explanation: "simple",
  tour: null,
  tourOpen: false,
  setTheme: () => undefined,
  setMotion: () => undefined,
  setExplanation: () => undefined,
  openTour: () => undefined,
  closeTour: () => undefined,
  resetAll: () => undefined,
};

describe("a recorded run's header is given no delivery: never LIVE, no delivery badge", () => {
  const catalogs = routing.locales.map((locale) => ({ locale, catalog: read(locale) }));

  it.each(catalogs)("$locale", ({ locale, catalog }) => {
    const detail = recordedDetail();
    fixtures.runId = detail.summary.run_id;
    fixtures.detail = detail;
    const tree = render(
      locale,
      catalog,
      createElement(
        PreferencesProvider,
        { value: preferences },
        createElement(RunWorkspace, { locale }),
      ),
    );
    const run = all(tree, (e) => attr(e, "data-run-mode") !== null);
    expect(run.map((e) => attr(e, "data-run-mode"))).toEqual(["recorded"]);
    const header = all(tree, (e) => e.tagName === "header" && hasClass(e, "hc-run-header"));
    expect(header).toHaveLength(1);
    const [head] = header as [Element];
    // The header's provenance badges, exactly: model, evidence, source, and nothing else.
    const lists = all(head, (e) => e.tagName === "ul" && hasClass(e, "hc-badges"));
    expect(lists).toHaveLength(1);
    const badges = all(lists[0] as Element, (e) => e.tagName === "li").map((li) => text(li).trim());
    expect(badges).toEqual([
      message(catalog, "Provenance.metaModel.label"),
      message(catalog, "Provenance.hyphaeLive.label"),
      message(catalog, "Provenance.recorded.label"),
    ]);
    // No delivery badge of any kind, and nothing of the LIVE family.
    const headerText = text(head);
    for (const key of [
      "Provenance.live.label",
      "Provenance.liveMeta.label",
      "Provenance.liveMeta.live",
      "Provenance.streaming.label",
      "Provenance.replay.label",
    ]) {
      expect(badges, key).not.toContain(message(catalog, key));
    }
    expect(headerText).not.toContain(message(catalog, "Provenance.liveMeta.live"));
    expect(headerText).not.toContain(message(catalog, "Provenance.liveMeta.label"));
    // The model note of a recorded live-model run.
    const notes = all(head, (e) => attr(e, "data-provenance-note") !== null).map((e) => attr(e, "data-provenance-note"));
    expect(notes).toEqual(["liveMeta.recorded"]);
    // No tab of a stream either.
    const tabs = all(tree, (e) => attr(e, "role") === "tab").map((e) => text(e));
    expect(tabs).toHaveLength(6);
    expect(tabs).not.toContain(message(catalog, "Run.tabs.live"));
    expect(tabs).not.toContain(message(catalog, "Run.tabs.playback"));
  });

  it.each(catalogs)("the same provenance with a live delivery does read LIVE ($locale)", ({ locale, catalog }) => {
    // The control: what the header above would show if it were handed a live delivery.
    const provenance = recordedDetail().summary.provenance;
    const tree = render(locale, catalog, createElement(ProvenanceBadges, { provenance, delivery: "live", withNote: true }));
    const badges = all(tree, (e) => e.tagName === "li").map((li) => text(li).trim());
    expect(badges).toContain(message(catalog, "Provenance.live.label"));
    expect(badges).toContain(message(catalog, "Provenance.liveMeta.label"));
    const replay = render(
      locale,
      catalog,
      createElement(ProvenanceBadges, { provenance, delivery: "replay", withNote: true }),
    );
    expect(all(replay, (e) => e.tagName === "li").map((li) => text(li).trim())).toContain(
      message(catalog, "Provenance.replay.label"),
    );
  });
});
