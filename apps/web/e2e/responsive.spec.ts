// SPDX-License-Identifier: Apache-2.0
//
// Every screen works at every width (the owner's 2026-11 report: on the run
// page's Playback tab the Proposer, Critic and Evidence Evaluator cards let
// their text spill past the frame, objection tags overflowed their own
// frames, 39-digit record ids in "contradicted by ..." did not wrap and the
// three columns overflowed). Each screen is opened once, every <details> on
// it opened, and measured at 1920, 1440, 1280, 1024, 768, 390, 360 and 320
// px (320 CSS px: a 1280 px screen at 400% zoom, WCAG 1.4.10 reflow), in
// English and in Spanish (longer words), on a guided run, on a live-like run
// whose model documents are as long as real Meta answers (long claims and
// objections, four objections, five 39-digit contradicting ids per claim,
// long plan steps, failed policy checks naming those ids, muse-spark-1.3 and
// muse-wonder-1.0, sealed-snapshot hashes) and on the recorded run R06. At
// each width:
//   (a) the document is no wider than the viewport, and no open dialog
//       scrolls sideways;
//   (b) nothing visible inside a framed box (any element drawing a border or
//       an outline, and the static-glass cards, whose border is transparent
//       and whose edge is a gradient: cards, panels, tags, callouts, code)
//       reaches past that box by more than 1 px, text included (measured per
//       line box, against the element holding it too), unless it sits in a
//       region that scrolls on purpose (overflow auto/scroll: the tables, the
//       JSON views; a scroll region's own box is still measured);
//   (c) on the Playback tab, the three role cards stand side by side, each
//       at least 280 px wide, at 1920 and 1440 px (the stage is then at least
//       60rem wide), and stack one per row at every narrower width;
//   (d) no text is cut by an overflow-hidden box, unless that box ellipsizes
//       and carries the full text as its title or accessible name; a tag's
//       label never ellipsizes (it wraps, .hc-tag).
// The mobile drawer is measured the same way at 320 and 360 px in all four
// locales, with a fine and with a coarse (touch) pointer. The stage tab of
// the live-like run is also captured at 1920, 1440, 1280, 1024 and 390 px,
// and the drawer at 320 px in Spanish, into test-results/responsive-shots/
// for a look by eye.

import type { Page } from "@playwright/test";

import { runDetailSchema } from "../src/lib/api/schemas";
import { docSlugs, localePath, runPath, showcaseRunPath } from "../src/lib/site";

import { awaitingDecisionDetail, mockApi, recordedRun, RUN_ID, sealedSnapshotDetail, storedRun } from "./api-mock";
import { expect, hydrated, type Locale, locales, t, test, useTheme } from "./fixtures";
import { archiveEntry, manifestRun, metaRequestIds } from "./research";

type Json = Record<string, unknown>;

const WIDTHS = [1920, 1440, 1280, 1024, 768, 390, 360, 320] as const;

/** A role card narrower than this is too narrow for a column of its own. */
const MIN_CARD = 280;

/**
 * The stage's rows at each width: one (three columns) from 1440 px up, where
 * the run's 80rem page leaves the stage at least 60rem; three below.
 */
const stageRows = (width: number) => (width >= 1440 ? 1 : 3);

// ---- a live-like run with long documents ------------------------------------

/** 39-digit record ids, as Hyphae numbers them. */
const LONG_IDS = [
  "186670067796945807561772405393844769735",
  "290412883957190475628110394857261038475",
  "318840279561029384756102938475610293847",
  "402938475610293847561029384756102938471",
  "571029384756102938475610293847561029384",
];

const LONG = {
  title:
    "Decide whether the payment client's retry policy should change to stop the intermittent checkout timeouts under peak load",
  summary:
    "Keep the payment client's retry limit at three attempts with full jitter, add a per-request deadline of 2.5 seconds and a circuit breaker on the upstream gateway, and measure before raising anything",
  claims: [
    "Payment timeouts during the 2026-10 peak correlate with retry bursts above five attempts per request, which the gateway's own rate limiter then answers with 429s",
    "Raising the retry limit to ten attempts would multiply the load on the upstream gateway during an incident and contradicts the documented constraint recorded as 186670067796945807561772405393844769735",
    "A per-request deadline of 2.5 seconds with exponential backoff and full jitter keeps the 99th-percentile checkout latency under the service-level objective while the breaker sheds load",
  ],
  assumptions: [
    "Upstream services tolerate the proposed change without a coordinated deployment of the gateway's configuration",
    "The 2026-10 incident report (record 290412883957190475628110394857261038475) describes the production topology as it stands today",
  ],
  uncertainties: [
    "No load-test data is available for the proposed deadline; the 99th-percentile figures come from a single week of production traces",
  ],
  plan: [
    "Add the per-request deadline behind the feature flag payments.client.deadline_ms and ship it disabled to every region",
    "Run the existing integration test suite and the payment-gateway contract tests against the staging gateway with the flag enabled",
    "Enable the flag for one percent of checkout traffic, compare the timeout rate and the 429 rate for 48 hours, then widen in steps",
  ],
  objections: [
    "The documented constraint 186670067796945807561772405393844769735 caps outbound retries at three attempts with jitter; the proposal's second claim cites it but the earlier revision raised the limit to ten",
    "No retrieved record ties the timeouts to insufficient retries; the only fact on record (290412883957190475628110394857261038475) ties them to retry bursts instead",
    "The deadline figure of 2.5 seconds is asserted without any retrieved measurement of the gateway's latency distribution under peak load",
    "Shipping a circuit breaker without a documented fallback path risks failing checkouts outright during a partial gateway outage, a regression the plan does not test",
  ],
  verdict:
    "record 186670067796945807561772405393844769735 caps retries at three attempts; the revision keeps that cap but its deadline claim still cites no measurement of the gateway's latency",
  rationale:
    "Contested: 1 of 3 claims corroborated; proofs: 2 valid, 0 invalid, 0 unavailable or error; 2 unresolved challenges remain open after the second round of evaluation.",
};

function isObject(value: unknown): value is Json {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Visits every object of a JSON tree, depth first. */
function walk(value: unknown, visit: (node: Json) => void) {
  if (Array.isArray(value)) {
    value.forEach((item) => walk(item, visit));
  } else if (isObject(value)) {
    visit(value);
    Object.values(value).forEach((item) => walk(item, visit));
  }
}

const OBJECTION_KINDS = ["constraint_conflict", "unsupported_claim", "contradictory_evidence", "regression_risk"];
const OBJECTION_CLAIMS = ["claim-2", "claim-1", "claim-3", null];

/**
 * The sealed-snapshot live-model run (api-mock.ts) with every model document
 * as long as a real answer: the Proposer on muse-spark-1.3, the Critic and
 * the Evaluator on muse-wonder-1.0, 39-digit evidence ids, three claims,
 * four objections, five contradicting ids per claim. Derived from the
 * fixture, so it stays valid against the contract.
 */
function longLiveDetail(): Json {
  let text = JSON.stringify(sealedSnapshotDetail());
  text = text.split("ev-constraint").join(LONG_IDS[0] ?? "").split("ev-fact").join(LONG_IDS[1] ?? "");
  const detail = JSON.parse(text) as Json;
  let call = 0;
  walk(detail, (node) => {
    if (Array.isArray(node.claims)) {
      node.claims = LONG.claims.map((statement, index) => {
        const existing = (node.claims as Json[])[index] ?? (node.claims as Json[])[0] ?? {};
        return { ...existing, id: `claim-${index + 1}`, statement };
      });
      node.summary = LONG.summary;
      node.assumptions = LONG.assumptions;
      node.uncertainties = LONG.uncertainties;
      node.plan_steps = LONG.plan;
    }
    if (Array.isArray(node.items) && typeof node.target_proposal === "string") {
      node.items = LONG.objections.map((description, index) => ({
        ...((node.items as Json[])[index] ?? {}),
        id: `item-${index + 1}`,
        kind: OBJECTION_KINDS[index],
        claim_id: OBJECTION_CLAIMS[index],
        description,
        evidence_refs: index < 2 ? [LONG_IDS[index]] : [],
      }));
    }
    if (Array.isArray(node.assessments)) {
      node.assessments = [1, 2, 3].map((n) => ({
        claim_id: `claim-${n}`,
        corroborated: n === 1,
        contradicting_evidence: n === 1 ? [] : LONG_IDS,
        relevant_evidence: LONG_IDS.slice(0, 2),
        note: "assessed against the retrieved constraint and the incident report",
      }));
    }
    if (Array.isArray(node.item_verdicts)) {
      const status = (node.item_verdicts as Json[])[0]?.status ?? "unresolved";
      node.item_verdicts = LONG.objections.map((_, index) => ({
        item_id: `item-${index + 1}`,
        status: index < 2 ? status : "unresolved",
        reason: LONG.verdict,
      }));
      if (Array.isArray(node.unresolved_items)) {
        node.unresolved_items = status === "unresolved" ? ["item-1", "item-2", "item-3", "item-4"] : ["item-3", "item-4"];
      }
    }
    if (isObject(node.invocation)) {
      call += 1;
      const role = typeof node.proposal === "object" || typeof node.revision === "object" ? "propose" : "other";
      node.invocation = {
        ...node.invocation,
        provider: "meta",
        model: role === "propose" ? "muse-spark-1.3" : "muse-wonder-1.0",
        scripted: false,
        request_id: `chatcmpl-${call.toString(16).padStart(4, "0")}${"9f3c2a71b0e45d68".repeat(2)}`,
      };
    }
    if (typeof node.rationale === "string") {
      node.rationale = LONG.rationale;
    }
    // The policy trace (the Decision tab's technical explanation): failed
    // checks that name every contradicting id.
    if (Array.isArray(node.reasons) && typeof node.rule === "string") {
      node.reasons = [
        ...(node.reasons as Json[]),
        { reason: "contradicted_evidence", evidence_ids: LONG_IDS },
        { reason: "invalid_proofs", evidence_ids: LONG_IDS.slice(0, 3) },
      ];
    }
  });
  const invocations: Json[] = [];
  walk(detail.events, (node) => {
    if (isObject(node.invocation)) {
      invocations.push(node.invocation);
    }
  });
  detail.model_invocations = invocations;
  const summary = detail.summary as Json;
  (summary.provenance as Json).meta_request_ids = invocations.map((i) => i.request_id);
  summary.title = LONG.title;
  (detail.task as Json).title = LONG.title;
  detail.challenge_items = LONG.objections.map((description, index) => ({
    id: `item-${index + 1}`,
    kind: OBJECTION_KINDS[index],
    claim_id: OBJECTION_CLAIMS[index],
    description,
    raised_in_round: 1,
    status: index < 2 ? "resolved" : "open",
    resolution_reason: index < 2 ? LONG.verdict : null,
  }));
  // The client reads only what its mirror of the contract accepts.
  runDetailSchema.parse(detail);
  return detail;
}

// ---- the screens --------------------------------------------------------------

/** The showcase list of layout-contrast.spec.ts: C01, L01, R06 (live model, Hyphae evidence). */
function showcase() {
  const live = (label: string) => {
    const run = manifestRun(label);
    return {
      model: "live_meta",
      evidence: "hyphae_live",
      flags: ["mode_undeclared"],
      meta_request_ids: metaRequestIds(run.file, run.run_id),
    };
  };
  const c01 = archiveEntry("C01", { model: "scripted", evidence: "simulated", flags: ["mode_undeclared", "unchained"], meta_request_ids: [] });
  const r06 = archiveEntry("R06", live("R06"));
  return { r06, list: [c01, archiveEntry("L01", live("L01")), r06] };
}

type RunKind = "guided" | "live-like" | "recorded";

const RUN_TABS = [
  { tab: "playback", marker: ".hc-stage-card" },
  { tab: "decision", marker: "[data-recommendation-kind]" },
  { tab: "explorer", marker: "[data-explorer] [data-sequence]" },
  { tab: "evidence", marker: "[data-evidence-id]" },
  { tab: "proofs", marker: "[data-proof-sequence]" },
  { tab: "audit", marker: "[data-audit-chain]" },
  { tab: "meta", marker: "[data-invocation-sequence]" },
] as const;

type RunTab = (typeof RUN_TABS)[number];

async function openRun(page: Page, locale: Locale, kind: RunKind) {
  if (kind === "recorded") {
    const { r06, list } = showcase();
    await mockApi(page, { showcase: list, recordedRuns: [recordedRun(r06, { base: "retry" })] });
    await page.goto(showcaseRunPath(locale, String(r06.run_id)));
    await hydrated(page);
    await expect(page.locator("[data-run-mode='recorded']")).toBeVisible();
    return;
  }
  const detail = kind === "guided" ? awaitingDecisionDetail() : longLiveDetail();
  await mockApi(page, kind === "guided" ? { detail } : { detail, frames: storedRun(detail).frames ?? [] });
  await page.goto(runPath(locale, RUN_ID));
  await hydrated(page);
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
}

async function openTab(page: Page, locale: Locale, tab: RunTab) {
  // A recorded run opens on its Decision tab, a guided or live one on its stage.
  const control = page.getByRole("tab", { name: t(locale, `Run.tabs.${tab.tab}`), exact: true });
  await control.click();
  await expect(control).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(tab.marker).first()).toBeVisible();
}

type Screen = { name: string; open: (page: Page, locale: Locale) => Promise<void>; stage?: true };

const screens: Screen[] = [
  {
    name: "overview",
    open: async (page, locale) => {
      await mockApi(page, { showcase: showcase().list });
      await page.goto(localePath(locale, ""));
      await hydrated(page);
      await expect(page.locator("[data-figures='ready']")).toBeVisible();
    },
  },
  {
    name: "new deliberation",
    open: async (page, locale) => {
      await mockApi(page);
      await page.goto(localePath(locale, "new"));
      await hydrated(page);
      await expect(page.locator("[data-scenario]").first()).toBeVisible();
    },
  },
  {
    name: "new deliberation, live form open",
    open: async (page, locale) => {
      await mockApi(page, { liveEnabled: true, liveRequiresTurnstile: false });
      await page.goto(localePath(locale, "new"));
      await hydrated(page);
      await expect(page.locator("[data-live='form'][data-live-state='open']")).toBeVisible();
    },
  },
  {
    name: "runs",
    open: async (page, locale) => {
      const long = longLiveDetail();
      await mockApi(page, { runs: [long.summary as Json, (awaitingDecisionDetail().summary as Json)] });
      await page.goto(localePath(locale, "runs"));
      await hydrated(page);
      await expect(page.locator("[data-run-id]").first()).toBeVisible();
    },
  },
  {
    name: "showcase",
    open: async (page, locale) => {
      await mockApi(page, { showcase: showcase().list });
      await page.goto(localePath(locale, "showcase"));
      await hydrated(page);
      await expect(page.locator("[data-manifest-check='matches']")).toBeVisible();
    },
  },
  ...["docs", ...docSlugs.map((slug) => `docs/${slug}`), "settings", "terms", "privacy", "limitations", "about"].map(
    (path): Screen => ({
      name: path,
      open: async (page, locale) => {
        await page.goto(localePath(locale, path));
        await hydrated(page);
      },
    }),
  ),
  // A recorded run has no stage: its tabs start at Decision.
  ...(["guided", "live-like", "recorded"] as const).flatMap((kind) =>
    RUN_TABS.filter((tab) => kind !== "recorded" || tab.tab !== "playback").map(
      (tab): Screen => ({
        name: `${kind} run, ${tab.tab}`,
        open: async (page, locale) => {
          await openRun(page, locale, kind);
          await openTab(page, locale, tab);
        },
        ...(tab.tab === "playback" ? { stage: true as const } : {}),
      }),
    ),
  ),
];

// ---- the measurements -----------------------------------------------------------

/** Waits for fonts and two frames, so line breaks are final. */
async function settle(page: Page) {
  await page.evaluate(
    () =>
      document.fonts.ready.then(
        () => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))),
      ),
  );
}

/** Opens every <details>, so what it holds is measured too. */
async function openDetails(page: Page) {
  await page.evaluate(() => {
    for (const details of Array.from(document.querySelectorAll("details"))) {
      details.open = true;
    }
  });
}

/**
 * What breaks the layout at the current width: (a) a document wider than the
 * viewport or a dialog scrolling sideways, (b) anything past a framed box,
 * (d) text cut by an overflow-hidden box. One line per problem, with the
 * elements named. `scope` measures only what is inside that element (an
 * open dialog).
 */
async function layoutProblems(page: Page, scope = "body"): Promise<string[]> {
  return page.evaluate((scope) => {
    const root = document.querySelector(scope);
    if (root === null) {
      return [`nothing matches ${scope}`];
    }
    const problems = new Set<string>();
    const viewport = window.innerWidth;
    const name = (element: Element): string => {
      const classes = Array.from(element.classList)
        .filter((c) => !c.startsWith("x") || c.length > 8)
        .slice(0, 3)
        .join(".");
      const data = Array.from(element.attributes)
        .filter((a) => a.name.startsWith("data-") && a.name !== "data-hydrated")
        .slice(0, 1)
        .map((a) => `[${a.name}${a.value === "" ? "" : `=${a.value.slice(0, 30)}`}]`)
        .join("");
      return `${element.tagName.toLowerCase()}${classes === "" ? "" : `.${classes}`}${data}`;
    };
    const scrollWidth = document.scrollingElement?.scrollWidth ?? Number.POSITIVE_INFINITY;
    if (scrollWidth > viewport) {
      const wide = Array.from(document.querySelectorAll("body *"))
        .filter((element) => element.getBoundingClientRect().right > viewport + 0.5)
        .slice(0, 8)
        .map((element) => `${name(element)} (right ${Math.round(element.getBoundingClientRect().right)})`);
      problems.add(`(a) document ${scrollWidth}px wide in a ${viewport}px viewport: ${wide.join(", ")}`);
    }
    // A dialog scrolls on purpose, but only down: content wider than the
    // dialog (the drawer's theme control at 320 px) makes it scroll sideways.
    for (const dialog of Array.from(document.querySelectorAll("dialog[open]"))) {
      if (dialog.scrollWidth > dialog.clientWidth) {
        const wide = Array.from(dialog.querySelectorAll("*"))
          .filter((element) => element.getBoundingClientRect().right > dialog.getBoundingClientRect().right + 0.5)
          .slice(0, 8)
          .map((element) => name(element));
        problems.add(`(a) ${name(dialog)} scrolls sideways (${dialog.scrollWidth}px of content in ${dialog.clientWidth}px): ${wide.join(", ")}`);
      }
    }
    const styles = new Map<Element, CSSStyleDeclaration>();
    const style = (element: Element) => {
      let found = styles.get(element);
      if (found === undefined) {
        found = getComputedStyle(element);
        styles.set(element, found);
      }
      return found;
    };
    /**
     * A framed box: draws a border or an outline on its inline sides, or is
     * a card or a glass surface. Static glass (hc-mono-glass.css) draws its
     * edge as a border-box gradient behind a transparent border, so its
     * border colour says nothing; the floating decision card does the same
     * from 672 px.
     */
    const framed = (element: Element) => {
      const s = style(element);
      const border =
        parseFloat(s.borderLeftWidth) > 0 &&
        parseFloat(s.borderRightWidth) > 0 &&
        s.borderLeftStyle !== "none" &&
        !/rgba\(\d+, \d+, \d+, 0\)|transparent/.test(s.borderLeftColor);
      const outline =
        s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0 && !element.matches(":focus, :focus-visible");
      return (
        border ||
        outline ||
        element.matches(
          ".hc-glass, .hc-stage-card, .hc-panel, .astryx-badge, .hc-card, .hc-run-card, .hc-scenario-card, .hc-flow-step, .hc-decision-card, .hc-preset-item",
        )
      );
    };
    /** Not drawn: hidden, visually hidden, or in a closed <details> (whose content still has boxes). */
    const hidden = (element: Element) =>
      element.closest(".hc-visually-hidden") !== null ||
      !element.checkVisibility({ visibilityProperty: true, contentVisibilityAuto: true });
    /** The text a box holds in full: its title or accessible name. */
    const fullText = (element: Element) =>
      [element.getAttribute("title"), element.getAttribute("aria-label")]
        .concat(element.parentElement === null ? [] : [element.parentElement.getAttribute("title"), element.parentElement.getAttribute("aria-label")])
        .some((value) => value !== null && value.trim() !== "" && value.includes((element.textContent ?? "").trim()));
    const ellipsized = (element: Element) =>
      style(element).textOverflow === "ellipsis" || (style(element).webkitLineClamp || "none") !== "none";

    /**
     * Checks one box against the elements around it, from `start` out: an
     * element's box from its parent (so a scroll region or a clipping box is
     * still measured against the card around it), a text line's from the
     * element holding it (its own frame, or its own clipping box).
     */
    const check = (box: DOMRect, start: Element | null, what: () => string, isText: boolean) => {
      if (box.width === 0 && box.height === 0) {
        return;
      }
      for (let ancestor: Element | null = start; ancestor !== null && ancestor !== document.body; ancestor = ancestor.parentElement) {
        const s = style(ancestor);
        const scrollsX = s.overflowX === "auto" || s.overflowX === "scroll";
        const scrollsY = s.overflowY === "auto" || s.overflowY === "scroll";
        const clipsX = s.overflowX === "hidden" || s.overflowX === "clip";
        const clipsY = s.overflowY === "hidden" || s.overflowY === "clip";
        if (scrollsX || scrollsY) {
          // A region that scrolls on purpose (a table, a JSON view).
          return;
        }
        const own = ancestor.getBoundingClientRect();
        if ((clipsX || clipsY) && (own.width <= 1 || own.height <= 1)) {
          // Visually hidden on purpose (a 1 px clipped box: the phone's table header).
          return;
        }
        const past = {
          left: own.left - box.left,
          right: box.right - own.right,
          top: own.top - box.top,
          bottom: box.bottom - own.bottom,
        };
        const sides = Object.entries(past)
          .filter(([, by]) => by > 1)
          .map(([side, by]) => `${side} ${Math.round(by)}px`);
        if (sides.length > 0) {
          if ((clipsX || clipsY) && isText) {
            // A tag's label wraps (.hc-tag): an ellipsis there is a defect,
            // even though Astryx's Badge carries the label as its title.
            if (!(ellipsized(ancestor) && fullText(ancestor) && ancestor.closest(".astryx-badge") === null)) {
              problems.add(`(d) text cut by ${name(ancestor)} (${sides.join(", ")}): ${what()}`);
            }
          } else if (framed(ancestor)) {
            problems.add(`(b) ${what()} past ${name(ancestor)} (${sides.join(", ")})`);
          }
        }
        if (clipsX || clipsY || s.position === "fixed") {
          // Nothing beyond a clipping box (reported above) or a fixed layer.
          return;
        }
      }
    };

    for (const element of Array.from(root.querySelectorAll("*"))) {
      if (hidden(element) || element.closest("svg") !== null && element.tagName.toLowerCase() !== "svg") {
        continue;
      }
      const s = style(element);
      if (s.display === "contents" || s.display === "none" || s.position === "fixed") {
        continue;
      }
      check(element.getBoundingClientRect(), element.parentElement, () => name(element), false);
    }
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const range = document.createRange();
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      const parent = node.parentElement;
      const content = (node.textContent ?? "").trim();
      if (parent === null || content === "" || hidden(parent) || ["SCRIPT", "STYLE", "NOSCRIPT"].includes(parent.tagName)) {
        continue;
      }
      range.selectNodeContents(node);
      for (const line of Array.from(range.getClientRects())) {
        check(line, parent, () => `text "${content.slice(0, 50)}" in ${name(parent)}`, true);
      }
    }
    return Array.from(problems);
  }, scope);
}

/**
 * (c) The role cards at this width: side by side, each at least 280 px
 * wide, from 1440 px up; one per row below.
 */
async function stageProblems(page: Page, width: number): Promise<string[]> {
  const cards = await page.locator(".hc-stage-card").evaluateAll((elements) =>
    elements.map((element) => {
      const box = element.getBoundingClientRect();
      return { top: Math.round(box.top), width: box.width };
    }),
  );
  if (cards.length !== 3) {
    return [`(c) ${cards.length} role cards, not 3`];
  }
  const stage = await page.locator(".hc-stage-tab").evaluate((element) => element.clientWidth);
  const rows = new Set(cards.map((card) => card.top)).size;
  const widths = cards.map((c) => Math.round(c.width)).join("/");
  const expected = stageRows(width);
  if (rows !== expected) {
    return [`(c) cards in ${rows} row(s), not ${expected}, on a ${stage}px stage (${widths} px)`];
  }
  const narrow = cards.filter((card) => card.width < MIN_CARD);
  return rows === 1 && narrow.length > 0 ? [`(c) cards side by side at ${widths} px`] : [];
}

for (const locale of ["en", "es"] as const satisfies readonly Locale[]) {
  test.describe(`every screen at every width (${locale})`, () => {
    test.use({ viewport: { width: 1440, height: 900 } });

    for (const screen of screens) {
      // The stage of each run kind also runs in Firefox and WebKit.
      const tag = screen.stage === true && locale === "en" ? " @smoke" : "";
      test(`${screen.name} (${locale}) fits ${WIDTHS.join(", ")} px${tag}`, async ({ page }) => {
        await useTheme(page, "light");
        await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
        await screen.open(page, locale);
        await openDetails(page);
        const found: string[] = [];
        for (const width of WIDTHS) {
          await page.setViewportSize({ width, height: 900 });
          await settle(page);
          const problems = [
            ...(await layoutProblems(page)),
            ...(screen.stage === true ? await stageProblems(page, width) : []),
          ];
          found.push(...problems.map((problem) => `${width}px ${problem}`));
        }
        expect(found, found.join("\n")).toEqual([]);
      });
    }
  });
}

// A run's page has its final width from the first paint: the title does not
// move when the run arrives (it jumped about 224 px on a 1920 px screen while
// the 80rem width followed the loaded run), and a page without a run id is
// as wide.
test("a run's page keeps its width while the run loads", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  const header = async () =>
    page.locator(".hc-page-header").evaluate((element) => {
      const box = element.getBoundingClientRect();
      return { left: Math.round(box.left), width: Math.round(box.width) };
    });
  await mockApi(page, { detail: longLiveDetail(), frames: storedRun(longLiveDetail()).frames ?? [] });
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  // Every read of the run (its detail and its stream) waits.
  await page.route(
    (url) => url.pathname.startsWith(`/api/v1/runs/${RUN_ID}`),
    async (route) => {
      await held;
      await route.fallback();
    },
  );
  await page.goto(runPath("en", RUN_ID));
  await hydrated(page);
  await expect(page.locator("[data-run='loading']")).toBeVisible();
  const loading = await header();
  release();
  await expect(page.locator("[data-stream-state='ended']")).toBeVisible();
  expect(await header()).toEqual(loading);
  expect(loading.width).toBeGreaterThan(52 * 16);
  await page.goto(localePath("en", "run"));
  await hydrated(page);
  await expect(page.locator("[data-run='no-id']")).toBeVisible();
  expect(await header()).toEqual(loading);
});

// The mobile drawer (MobileNav): its links, the language list and the theme
// control, at 320 and 360 px in every locale. Once with the desktop
// project's fine pointer and once on a phone (@mobile: coarse pointer, where
// the supporting text is 14 px instead of 12).
for (const pointer of ["fine", "coarse"] as const) {
  test.describe(`the mobile drawer (${pointer} pointer)`, () => {
    for (const locale of locales) {
      for (const width of [320, 360]) {
        const tag = pointer === "coarse" ? " @mobile" : "";
        test(`fits ${width} px (${locale})${tag}`, async ({ page }) => {
          await page.setViewportSize({ width, height: 800 });
          await useTheme(page, "light");
          await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
          await page.goto(localePath(locale, "settings"));
          await hydrated(page);
          await page.getByRole("button", { name: t(locale, "Shell.menuButton"), exact: true }).click();
          const drawer = page.getByRole("dialog", { name: t(locale, "Shell.menuTitle") });
          await expect(drawer).toBeVisible();
          await expect(drawer).toHaveCSS("opacity", "1");
          await settle(page);
          const problems = await layoutProblems(page, "dialog[open]");
          if (width === 320 && locale === "es") {
            await drawer.screenshot({
              path: `test-results/responsive-shots/drawer-320-es${pointer === "coarse" ? "-touch" : ""}.png`,
            });
          }
          expect(problems, problems.join("\n")).toEqual([]);
        });
      }
    }
  });
}

test.describe("the stage of a live-like run, by eye", () => {
  for (const width of [1920, 1440, 1280, 1024, 390]) {
    test(`stage tab at ${width} px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await useTheme(page, "light");
      await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
      await openRun(page, "en", "live-like");
      await settle(page);
      // The sticky top bar would sit over the scrolled-to stage (hidden
      // through the CSSOM: the page's CSP refuses an injected stylesheet).
      await page.locator(".hc-topbar").evaluate((bar) => {
        (bar as HTMLElement).style.visibility = "hidden";
      });
      await page.locator(".hc-stage").screenshot({ path: `test-results/responsive-shots/stage-${width}.png` });
    });
  }
});
