// SPDX-License-Identifier: Apache-2.0
//
// Every fixture of the contract (responses the real server produced over the
// five guided scenarios) must pass the zod mirrors in src/lib/api/schemas.ts,
// and the mirrors must tolerate additive fields while refusing values outside
// the contract's enumerations.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";
import { z } from "zod";

import { contractDir } from "../scripts/gen-api-types.mjs";
import {
  apiErrorBodySchema,
  capabilitiesSchema,
  healthSchema,
  receiptSchema,
  runCreatedSchema,
  runDetailSchema,
  runSummarySchema,
  scenariosSchema,
  sessionSchema,
  showcaseEntrySchema,
  sseFrameSchema,
} from "../src/lib/api/schemas";

const fixturesDir = path.join(contractDir(), "fixtures");

function fixture(name: string): unknown {
  return JSON.parse(readFileSync(path.join(fixturesDir, name), "utf8"));
}

const names = readdirSync(fixturesDir).filter((n) => n.endsWith(".json")).sort();
const byPrefix = (prefix: string) => names.filter((n) => n.startsWith(prefix));

describe("contract fixtures parse", () => {
  it("never compiles parsers with new Function (hash-only CSP, no unsafe-eval)", () => {
    expect(z.core.globalConfig.jitless).toBe(true);
  });

  it("covers every fixture file with a schema", () => {
    const covered = new Set([
      "api-error-samples.json",
      "capabilities-public.json",
      "health.json",
      "scenarios.json",
      "session.json",
      "showcase-entry-F03.json",
      "sse-frames-retry.json",
      ...byPrefix("run-created-"),
      ...byPrefix("run-detail-"),
      ...byPrefix("run-summary-"),
      ...byPrefix("receipt-v2-"),
    ]);
    expect(names.filter((n) => !covered.has(n))).toEqual([]);
  });

  it("health, session, scenarios, capabilities, run created", () => {
    expect(healthSchema.parse(fixture("health.json")).status).toBe("ok");
    expect(sessionSchema.parse(fixture("session.json")).created).toBe(true);
    const scenarios = scenariosSchema.parse(fixture("scenarios.json"));
    expect(scenarios.scenarios.map((s) => s.id)).toEqual([
      "retry",
      "offline",
      "invalid_proof",
      "injection",
      "abstain",
    ]);
    for (const scenario of scenarios.scenarios) {
      expect(scenario.simulated).toBe(true);
      expect(scenario.model).toBe("scripted");
      expect(scenario.evidence).toBe("simulated");
    }
    const capabilities = capabilitiesSchema.parse(fixture("capabilities-public.json"));
    expect(capabilities.live_enabled).toBe(false);
    expect(capabilities.live).toBeUndefined();
    expect(capabilities.limits.visitor_access_hours).toBe(24);
    const created = runCreatedSchema.parse(fixture("run-created-retry.json"));
    expect(created.run.state).toBe("created");
    expect(created.links.events).toMatch(/\/events$/);
  });

  it.each(byPrefix("run-created-"))("%s", (name) => {
    const created = runCreatedSchema.parse(fixture(name));
    expect(created.run.state).toBe("created");
    expect(created.run.active).toBe(true);
    // Nothing is recorded yet but the task: the model axis is undetermined.
    expect(created.run.provenance.model).toBe("undetermined");
    expect(created.links.self).toBe(`/api/v1/runs/${created.run.run_id}`);
  });

  // What each summary fixture records, by name: a fixture without an entry
  // here fails, so a new one is looked at instead of assumed to be guided.
  const summaries: Record<
    string,
    { model: string; active: boolean; declaredModel: string; evidence: string }
  > = {
    "run-summary-abstain.json": { model: "scripted", active: false, declaredModel: "scripted", evidence: "none" },
    "run-summary-injection.json": { model: "scripted", active: false, declaredModel: "scripted", evidence: "simulated" },
    "run-summary-invalid_proof.json": { model: "scripted", active: false, declaredModel: "scripted", evidence: "simulated" },
    "run-summary-offline.json": { model: "scripted", active: false, declaredModel: "scripted", evidence: "none" },
    "run-summary-retry.json": { model: "scripted", active: false, declaredModel: "scripted", evidence: "simulated" },
    // A public live run just created: declared Meta model, nothing recorded yet.
    "run-summary-live-R01-reference.json": { model: "undetermined", active: true, declaredModel: "meta", evidence: "none" },
  };

  it.each(byPrefix("run-summary-"))("%s", (name) => {
    const summary = runSummarySchema.parse(fixture(name));
    const expected = summaries[name];
    expect(expected, `no expectation for ${name}`).toBeDefined();
    expect(summary.provenance.model).toBe(expected?.model);
    expect(summary.active).toBe(expected?.active);
    expect(summary.provenance.declared?.model).toBe(expected?.declaredModel);
    expect(summary.provenance.evidence).toBe(expected?.evidence);
  });

  it.each(byPrefix("run-detail-"))("%s", (name) => {
    const detail = runDetailSchema.parse(fixture(name));
    expect(detail.events.length).toBe(detail.summary.event_count);
    expect(detail.events.at(-1)?.sequence).toBe(detail.summary.last_sequence);
    // Every payload variant the fixture carries is one the union knows.
    for (const event of detail.events) {
      expect(typeof event.payload.type).toBe("string");
    }
  });

  it.each(byPrefix("receipt-v2-"))("%s", (name) => {
    const receipt = receiptSchema.parse(fixture(name));
    expect(receipt.format).toBe("hyphae-consensus-run-receipt-v2");
    expect(receipt.proof_limitations).not.toBe("");
  });

  it("showcase entry (a live, mode-undeclared archived run)", () => {
    const entry = showcaseEntrySchema.parse(fixture("showcase-entry-F03.json"));
    expect(entry.provenance.model).toBe("live_meta");
    expect(entry.provenance.source).toBe("research_archive");
    expect(entry.provenance.flags).toEqual(["mode_undeclared"]);
    expect(entry.provenance.meta_request_ids?.length).toBe(6);
  });

  it("SSE frames: consensus frames in sequence, then stream_end", () => {
    const frames = (fixture("sse-frames-retry.json") as unknown[]).map((f) =>
      sseFrameSchema.parse(f),
    );
    const consensus = frames.filter((f) => f.frame === "consensus");
    expect(consensus.map((f) => (f.frame === "consensus" ? f.event.sequence : -1))).toEqual(
      Array.from({ length: consensus.length }, (_, i) => i + 1),
    );
    for (const frame of consensus) {
      expect(frame.frame === "consensus" && frame.delivery).toBe("replay");
    }
    const last = frames.at(-1);
    expect(last?.frame).toBe("stream_end");
    expect(last?.frame === "stream_end" && last.reason).toBe("awaiting_human_approval");
  });

  it("error samples", () => {
    const samples = fixture("api-error-samples.json") as unknown[];
    expect(samples.length).toBeGreaterThan(10);
    for (const sample of samples) {
      const body = apiErrorBodySchema.parse(sample);
      expect(body.code).toMatch(/^[a-z_]+$/);
    }
    const limited = samples
      .map((s) => apiErrorBodySchema.parse(s))
      .find((s) => s.code === "rate_limited");
    expect(limited?.retry_after_secs).toBe(30);
  });
});

describe("mirrors are tolerant of additions, strict on enumerations", () => {
  const summary = fixture("run-summary-retry.json") as Record<string, unknown>;

  it("keeps unknown extra fields", () => {
    const parsed = runSummarySchema.parse({ ...summary, future_field: { a: 1 } });
    expect((parsed as Record<string, unknown>).future_field).toEqual({ a: 1 });
  });

  it("refuses a model provenance outside the contract", () => {
    const provenance = summary.provenance as Record<string, unknown>;
    const result = runSummarySchema.safeParse({
      ...summary,
      provenance: { ...provenance, model: "live_something" },
    });
    expect(result.success).toBe(false);
  });

  it("refuses an unknown run state and an unknown delivery", () => {
    expect(runSummarySchema.safeParse({ ...summary, state: "done" }).success).toBe(false);
    const frames = fixture("sse-frames-retry.json") as Array<Record<string, unknown>>;
    const first = frames[0] ?? {};
    expect(sseFrameSchema.safeParse({ ...first, delivery: "fresh" }).success).toBe(false);
  });

  it("requires the fields the honesty rules read", () => {
    const without = (key: string) =>
      Object.fromEntries(Object.entries(summary).filter(([k]) => k !== key));
    expect(runSummarySchema.safeParse(without("provenance")).success).toBe(false);
    expect(runSummarySchema.safeParse(without("active")).success).toBe(false);
    expect(runSummarySchema.safeParse(without("state")).success).toBe(false);
  });
});

describe("the live section of the capabilities (LiveCapabilitiesV1)", () => {
  const base = fixture("capabilities-public.json") as Record<string, unknown>;
  // The shape the live gate publishes (crates/consensus-api/src/live/gate.rs
  // capabilities): the preset of the live R01 fixture, the factory's
  // evidence axis, six-decimal USD strings.
  const task = (fixture("run-detail-live-R01-reference.json") as { task: Record<string, string> }).task;
  const preset = {
    id: "R01-reference",
    title: task.title,
    description: task.description,
    project: task.project_id,
    model: "live_meta",
    evidence: "hyphae_snapshot",
  };
  const live = {
    presets: [preset],
    budget: {
      daily_available_now: "4.250000",
      daily_remaining: "9.120000",
      lifetime_remaining: "51.300000",
      tripped: false,
    },
    requires_turnstile: true,
    requires_adult_attestation: true,
  };
  const withLive = (section: unknown) => ({ ...base, live_enabled: true, live: section });
  const omit = (value: Record<string, unknown>, key: string) =>
    Object.fromEntries(Object.entries(value).filter(([k]) => k !== key));

  it("parses the published shape, budget and requirements included", () => {
    const parsed = capabilitiesSchema.parse(withLive(live));
    expect(parsed.live?.presets[0]?.model).toBe("live_meta");
    expect(parsed.live?.presets[0]?.evidence).toBe("hyphae_snapshot");
    expect(parsed.live?.presets[0]?.project).toBe("demo-repo");
    expect(parsed.live?.budget?.daily_remaining).toBe("9.120000");
    expect(parsed.live?.requires_turnstile).toBe(true);
    expect(parsed.live?.requires_adult_attestation).toBe(true);
  });

  it("accepts a tripped ledger with its code, and an unreadable one (no budget)", () => {
    const tripped = { ...live, budget: { ...live.budget, tripped: true, trip_code: "kill_switch" } };
    expect(capabilitiesSchema.parse(withLive(tripped)).live?.budget?.trip_code).toBe("kill_switch");
    const unreadable = omit(live, "budget");
    expect(capabilitiesSchema.parse(withLive(unreadable)).live?.budget).toBeUndefined();
  });

  it("refuses a preset axis outside the contract and missing requirements", () => {
    const badModel = { ...live, presets: [{ ...preset, model: "LIVE" }] };
    expect(capabilitiesSchema.safeParse(withLive(badModel)).success).toBe(false);
    const badEvidence = { ...live, presets: [{ ...preset, evidence: "sealed" }] };
    expect(capabilitiesSchema.safeParse(withLive(badEvidence)).success).toBe(false);
    expect(capabilitiesSchema.safeParse(withLive(omit(live, "requires_turnstile"))).success).toBe(false);
    expect(
      capabilitiesSchema.safeParse(withLive(omit(live, "requires_adult_attestation"))).success,
    ).toBe(false);
    const noProject = omit(preset, "project");
    expect(capabilitiesSchema.safeParse(withLive({ ...live, presets: [noProject] })).success).toBe(false);
  });
});
