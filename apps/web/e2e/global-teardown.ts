// SPDX-License-Identifier: Apache-2.0
//
// Adds up the axe results of the run (test-results/axe/*.json) into
// test-results/axe-summary.json and prints the totals.

import { writeFile } from "node:fs/promises";

import { AXE_SUMMARY_FILE, blocking, readScans } from "./axe-results";

export default async function globalTeardown(): Promise<void> {
  const scans = await readScans();
  if (scans.length === 0) {
    return;
  }
  const byImpact: Record<string, number> = {};
  const incomplete: Record<string, number> = {};
  for (const scan of scans) {
    for (const rule of scan.violations) {
      const impact = rule.impact ?? "unknown";
      byImpact[impact] = (byImpact[impact] ?? 0) + 1;
    }
    for (const rule of scan.incomplete) {
      incomplete[rule.id] = (incomplete[rule.id] ?? 0) + 1;
    }
  }
  const summary = {
    scans: scans.length,
    rulePasses: scans.reduce((sum, scan) => sum + scan.passes, 0),
    elementChecksPassed: scans.reduce((sum, scan) => sum + scan.passedNodes, 0),
    violationsByImpact: byImpact,
    seriousOrCritical: blocking(scans),
    incompleteRules: incomplete,
  };
  await writeFile(AXE_SUMMARY_FILE, `${JSON.stringify(summary, null, 2)}\n`);
  console.log(
    `axe: ${summary.scans} scans, ${summary.rulePasses} rule passes, ` +
      `${summary.elementChecksPassed} element checks passed, ` +
      `violations by impact ${JSON.stringify(byImpact)}, ` +
      `serious or critical: ${summary.seriousOrCritical.length}`,
  );
}
