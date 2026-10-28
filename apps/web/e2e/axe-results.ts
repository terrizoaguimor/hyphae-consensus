// SPDX-License-Identifier: Apache-2.0
//
// axe results on disk: each axe test writes one JSON file per locale and
// theme into test-results/axe/, and global-teardown.ts adds them up into
// test-results/axe-summary.json (the numbers reported for the run).

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import type AxeBuilder from "@axe-core/playwright";

// axe-core's result type, through the pinned @axe-core/playwright (axe-core
// itself is not a direct dependency).
type AxeResults = Awaited<ReturnType<AxeBuilder["analyze"]>>;

// Playwright loads these files as CommonJS (package.json has no "type"),
// so paths are built from __dirname rather than import.meta.url.
export const AXE_RESULTS_DIR = path.join(__dirname, "..", "test-results", "axe");

export const AXE_SUMMARY_FILE = path.join(
  __dirname,
  "..",
  "test-results",
  "axe-summary.json",
);

/** The WCAG 2.2 A/AA rules plus axe's best practices. */
export const AXE_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21a",
  "wcag21aa",
  "wcag22aa",
  "best-practice",
];

export type ScanSummary = {
  scan: string;
  url: string;
  /** Rules that ran and found nothing wrong. */
  passes: number;
  /** Rules axe could not decide (e.g. text over a gradient). */
  incomplete: Array<{ id: string; nodes: number; targets: string[] }>;
  violations: Array<{
    id: string;
    impact: string | null;
    nodes: number;
    targets: string[];
  }>;
  /** Element checks: passed nodes summed over the passed rules. */
  passedNodes: number;
};

export function summarize(scan: string, results: AxeResults): ScanSummary {
  return {
    scan,
    url: results.url,
    passes: results.passes.length,
    passedNodes: results.passes.reduce((sum, rule) => sum + rule.nodes.length, 0),
    incomplete: results.incomplete.map((rule) => ({
      id: rule.id,
      nodes: rule.nodes.length,
      targets: rule.nodes.slice(0, 3).map((node) => node.target.join(" ")),
    })),
    violations: results.violations.map((rule) => ({
      id: rule.id,
      impact: rule.impact ?? null,
      nodes: rule.nodes.length,
      targets: rule.nodes.slice(0, 5).map((node) => node.target.join(" ")),
    })),
  };
}

export async function writeScans(name: string, scans: ScanSummary[]) {
  await mkdir(AXE_RESULTS_DIR, { recursive: true });
  await writeFile(
    path.join(AXE_RESULTS_DIR, `${name}.json`),
    `${JSON.stringify(scans, null, 2)}\n`,
  );
}

export async function readScans(): Promise<ScanSummary[]> {
  let files: string[];
  try {
    files = await readdir(AXE_RESULTS_DIR);
  } catch {
    return [];
  }
  const scans: ScanSummary[] = [];
  for (const file of files.filter((name) => name.endsWith(".json")).sort()) {
    const content = await readFile(path.join(AXE_RESULTS_DIR, file), "utf8");
    scans.push(...(JSON.parse(content) as ScanSummary[]));
  }
  return scans;
}

/** Violating rules of serious or critical impact: the suite allows none. */
export function blocking(scans: ScanSummary[]) {
  return scans.flatMap((scan) =>
    scan.violations
      .filter((rule) => rule.impact === "serious" || rule.impact === "critical")
      .map((rule) => ({ scan: scan.scan, ...rule })),
  );
}
