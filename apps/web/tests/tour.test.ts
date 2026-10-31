// SPDX-License-Identifier: Apache-2.0
//
// The tour's body per connection state (src/lib/tour.ts, HE #8): a step that
// speaks about the connection has a variant for each state of the health
// probe, the pending one makes no claim either way, and every key it picks
// exists in the four catalogs.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import { CONNECTION_STEPS, tourBodyKey, tourSteps } from "../src/lib/tour";

type Catalog = { [key: string]: string | Catalog };

function tourOf(locale: string): Catalog {
  const catalog = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  return catalog.Tour as Catalog;
}

function at(catalog: Catalog, path: string): unknown {
  return path.split(".").reduce<unknown>((node, key) => (node as Catalog | undefined)?.[key], catalog);
}

const states = ["pending", "connected", "unreachable"] as const;

describe("tourBodyKey", () => {
  it("gives the welcome and guided steps one variant per connection state", () => {
    for (const step of CONNECTION_STEPS) {
      expect(tourBodyKey(step, "connected")).toBe(`steps.${step}.bodyConnected`);
      expect(tourBodyKey(step, "pending")).toBe(`steps.${step}.bodyPending`);
      expect(tourBodyKey(step, "unreachable")).toBe(`steps.${step}.body`);
    }
  });

  it("keeps one body for the steps that make no claim about the connection", () => {
    for (const step of tourSteps.filter((s) => !(CONNECTION_STEPS as readonly string[]).includes(s))) {
      for (const state of states) {
        expect(tourBodyKey(step, state), `${step} ${state}`).toBe(`steps.${step}.body`);
      }
    }
  });

  it.each(routing.locales)("%s has every body the tour can pick, and pending claims nothing", (locale) => {
    const tour = tourOf(locale);
    for (const step of tourSteps) {
      for (const state of states) {
        const text = at(tour, tourBodyKey(step, state));
        expect(typeof text, `${locale} Tour.${tourBodyKey(step, state)}`).toBe("string");
        expect(String(text).trim()).not.toBe("");
      }
    }
    for (const step of CONNECTION_STEPS) {
      const pending = String(at(tour, `steps.${step}.bodyPending`));
      // Neither the planned wording nor the connected one.
      expect(pending, `${locale} ${step}`).not.toBe(String(at(tour, `steps.${step}.body`)));
      expect(pending, `${locale} ${step}`).not.toBe(String(at(tour, `steps.${step}.bodyConnected`)));
    }
    const enPending = String(at(tourOf("en"), "steps.guided.bodyPending"));
    expect(enPending).not.toMatch(/connected|did not answer|Once/u);
  });

  it("the connected guided step does not promise a guided example the server may not offer", () => {
    // capabilities.guided_enabled can be false on a connected server.
    expect(String(at(tourOf("en"), "steps.guided.bodyConnected"))).toMatch(/^When this server offers guided examples/u);
  });
});
