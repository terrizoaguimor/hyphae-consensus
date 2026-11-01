// SPDX-License-Identifier: Apache-2.0
//
// The live form's rules (src/lib/live/form.ts) over capabilities in the
// contract's shape (LiveCapabilitiesV1, as crates/consensus-api/src/live/
// gate.rs publishes it): when the whole form is closed, and when Start waits
// for the visitor (a preset, the attestation, an invite code that fits, a
// Turnstile token).

import { readFileSync } from "node:fs";
import path from "node:path";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { contractDir } from "../scripts/gen-api-types.mjs";
import { capabilitiesSchema } from "../src/lib/api/schemas";
import type { LiveCapabilitiesV1 } from "../src/lib/api/types";
import {
  focusAfterRefusal,
  isTripCode,
  liveClosedReason,
  parseUsd,
  presetFromQuery,
  quotaRuleValues,
  startBlocker,
  startNativelyDisabled,
  type StartInput,
  TRIP_CODES,
} from "../src/lib/live/form";

const task = (
  JSON.parse(
    readFileSync(
      path.join(contractDir(), "fixtures", "run-detail-live-R01-reference.json"),
      "utf8",
    ),
  ) as { task: { title: string; description: string; project_id: string } }
).task;

const budget = {
  daily_available_now: "4.250000",
  daily_remaining: "9.120000",
  lifetime_remaining: "51.300000",
  tripped: false,
};

const live: LiveCapabilitiesV1 = {
  presets: [
    {
      id: "R01-reference",
      title: task.title,
      description: task.description,
      project: task.project_id,
      model: "live_meta",
      evidence: "hyphae_snapshot",
    },
  ],
  budget,
  requires_turnstile: true,
  requires_adult_attestation: true,
};

const SITE_KEY = "0x4AAAAAAA-test-site-key";

const ready: StartInput = {
  live,
  presetId: "R01-reference",
  attested: true,
  judgeCode: "",
  token: "tok",
  starting: false,
};

describe("the live section fixture is in the contract's shape", () => {
  it("parses with the capabilities mirror", () => {
    const base = JSON.parse(
      readFileSync(path.join(contractDir(), "fixtures", "capabilities-public.json"), "utf8"),
    ) as Record<string, unknown>;
    expect(capabilitiesSchema.safeParse({ ...base, live_enabled: true, live }).success).toBe(true);
  });
});

describe("liveClosedReason", () => {
  it("is open with presets, a readable untripped ledger and a site key", () => {
    expect(liveClosedReason(live, SITE_KEY)).toBeNull();
  });

  it("is closed when the ledger is tripped, whatever else holds", () => {
    const tripped = { ...live, budget: { ...budget, tripped: true, trip_code: "kill_switch" } };
    expect(liveClosedReason(tripped, SITE_KEY)).toBe("tripped");
    expect(liveClosedReason(tripped, undefined)).toBe("tripped");
  });

  it("is closed when the ledger could not be read (no budget published)", () => {
    expect(liveClosedReason({ ...live, budget: null }, SITE_KEY)).toBe("budget_unknown");
    expect(liveClosedReason({ ...live, budget: undefined }, SITE_KEY)).toBe("budget_unknown");
  });

  it("is closed without a preset", () => {
    expect(liveClosedReason({ ...live, presets: [] }, SITE_KEY)).toBe("no_presets");
  });

  it("is closed when Turnstile is required and this build has no site key", () => {
    expect(liveClosedReason(live, undefined)).toBe("turnstile_unconfigured");
    expect(liveClosedReason(live, "")).toBe("turnstile_unconfigured");
    // A server that does not require the token needs no key.
    expect(liveClosedReason({ ...live, requires_turnstile: false }, undefined)).toBeNull();
  });
});

describe("startBlocker", () => {
  it("lets a complete request start", () => {
    expect(startBlocker(ready)).toBeNull();
  });

  it("waits for a listed preset", () => {
    expect(startBlocker({ ...ready, presetId: "" })).toBe("preset");
    expect(startBlocker({ ...ready, presetId: "R02-unknown" })).toBe("preset");
  });

  it("waits for the attestation when the server requires it", () => {
    expect(startBlocker({ ...ready, attested: false })).toBe("attestation");
    expect(
      startBlocker({ ...ready, attested: false, live: { ...live, requires_adult_attestation: false } }),
    ).toBeNull();
  });

  it("refuses an invite code longer than the contract allows, and accepts a blank one", () => {
    expect(startBlocker({ ...ready, judgeCode: "x".repeat(257) })).toBe("judge_code");
    expect(startBlocker({ ...ready, judgeCode: "   " })).toBeNull();
    expect(startBlocker({ ...ready, judgeCode: " JUDGE-1 " })).toBeNull();
  });

  it("keeps Start disabled until a Turnstile token exists", () => {
    expect(startBlocker({ ...ready, token: null })).toBe("turnstile");
    expect(startBlocker({ ...ready, token: "" })).toBe("turnstile");
    expect(
      startBlocker({ ...ready, token: null, live: { ...live, requires_turnstile: false } }),
    ).toBeNull();
  });

  it("is blocked while a start is in flight", () => {
    expect(startBlocker({ ...ready, starting: true })).toBe("starting");
  });
});

describe("the Start button and the keyboard focus", () => {
  it("is busy, never natively disabled, while a start is in flight", () => {
    expect(startNativelyDisabled(null, "starting")).toBe(false);
    expect(startNativelyDisabled(null, null)).toBe(false);
    expect(startNativelyDisabled(null, "turnstile")).toBe(true);
    expect(startNativelyDisabled("tripped", null)).toBe(true);
  });

  it("hands the focus to the error notice when a refusal leaves Start waiting for a fresh Turnstile token", () => {
    // A request spends the single-use token (the form sets it to null), so
    // the refused form that comes back is blocked on "turnstile" and Start is
    // natively disabled again: left there, the focus would fall to <body>.
    const after = startBlocker({ ...ready, token: null, starting: false });
    expect(after).toBe("turnstile");
    expect(focusAfterRefusal(null, after)).toBe("notice");
  });

  it("leaves the focus on Start when Start can be pressed again after a refusal", () => {
    const noTurnstile = { ...ready, token: null, live: { ...live, requires_turnstile: false } };
    expect(focusAfterRefusal(null, startBlocker(noTurnstile))).toBe("start");
    // A fresh token arrived while the request was in flight.
    expect(focusAfterRefusal(null, startBlocker(ready))).toBe("start");
  });
});

describe("budget amounts and trip codes", () => {
  it("reads the ledger's six-decimal USD strings cut to cents, and nothing else", () => {
    expect(parseUsd("4.250000")).toBe(4.25);
    expect(parseUsd("9.120000")).toBe(9.12);
    // Never rounded up: what remains is shown at most as it is.
    expect(parseUsd("9.129999")).toBe(9.12);
    expect(parseUsd("0.000024")).toBe(0);
    expect(parseUsd("60")).toBe(60);
    expect(parseUsd("-1.000000")).toBeNull();
    expect(parseUsd("4,25")).toBeNull();
    expect(parseUsd("")).toBeNull();
    expect(parseUsd("NaN")).toBeNull();
  });

  it("knows the ledger's trip codes", () => {
    expect([...TRIP_CODES].sort()).toEqual(
      ["charge_bound", "inconsistent", "kill_switch", "lifetime_cap", "output_bound", "provider_refused_key"],
    );
    expect(isTripCode("kill_switch")).toBe(true);
    expect(isTripCode("a_new_reason")).toBe(false);
    expect(isTripCode(null)).toBe(false);
  });
});

// Round 8 (lane C): the daily rule with the server's own figures (C3), the
// cost of a failed run (C6), the expectation that a live run can fail (C8),
// and the preset a failed run's "Start another live run" link preselects (C2).
const withQuotas: LiveCapabilitiesV1 = { ...live, client_daily: 5, invite_code_daily: 7 };

type Catalog = { [key: string]: string | Catalog };

function message(locale: string, key: string): string {
  let node: string | Catalog | undefined = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key} in ${locale}`);
  }
  return node;
}

describe("the live form's round 8 rules", () => {
  it("parses capabilities that publish the daily quotas, and those that do not", () => {
    const base = JSON.parse(
      readFileSync(path.join(contractDir(), "fixtures", "capabilities-public.json"), "utf8"),
    ) as Record<string, unknown>;
    const parsed = capabilitiesSchema.safeParse({ ...base, live_enabled: true, live: withQuotas });
    expect(parsed.success).toBe(true);
    expect(parsed.data?.live?.client_daily).toBe(5);
    expect(parsed.data?.live?.invite_code_daily).toBe(7);
  });

  it("states the daily rule only with both figures the server publishes", () => {
    expect(quotaRuleValues(withQuotas)).toEqual({ clientDaily: 5, codeDaily: 7 });
    expect(quotaRuleValues(live)).toBeNull();
    expect(quotaRuleValues({ ...live, client_daily: 3 })).toBeNull();
    expect(quotaRuleValues({ ...live, client_daily: 3, invite_code_daily: null })).toBeNull();
  });

  it("preselects only a preset the server lists, and nothing else from the address", () => {
    expect(presetFromQuery("?preset=R01-reference", live)).toBe("R01-reference");
    expect(presetFromQuery("?preset=R09-unknown", live)).toBe("");
    expect(presetFromQuery("?preset=", live)).toBe("");
    expect(presetFromQuery("", live)).toBe("");
    expect(presetFromQuery("?attest_adult=true&judge_code=x", live)).toBe("");
    expect(presetFromQuery("?preset=R01-reference", { ...live, presets: [] })).toBe("");
  });
});

describe.each(["en", "es", "fr", "pt-BR"])("the live form's copy (%s)", (locale) => {
  const format = (key: string, values: Record<string, string | number> = {}) =>
    String(new IntlMessageFormat(message(locale, key), locale).format(values));

  it("renders the daily rule with the capability figures", () => {
    const values = quotaRuleValues(withQuotas);
    expect(values).not.toBeNull();
    const rule = format("NewDeliberation.live.quota.rule", values ?? {});
    expect(rule).toContain("5");
    expect(rule).toContain("7");
    expect(rule).toContain("UTC");
    expect(rule).not.toContain("{");
    const one = format("NewDeliberation.live.quota.rule", { clientDaily: 1, codeDaily: 1 });
    expect(one).not.toBe(rule);
  });

  it("says what a failed run costs, and that a live run can fail", () => {
    expect(message(locale, "NewDeliberation.live.budget.noteFailed").trim()).not.toBe("");
    expect(message(locale, "NewDeliberation.live.failNote").trim()).not.toBe("");
    // The note says the rest of the reservation is returned, never that the cost is.
    expect(format("NewDeliberation.live.budget.noteFailed")).not.toContain("{");
  });
});
