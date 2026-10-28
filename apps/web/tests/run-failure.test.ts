// SPDX-License-Identifier: Apache-2.0
//
// A failed run is explained from its failure CODE (src/lib/run/failure.ts),
// in every locale, never by the server's English message: every code
// `public_view::failure_message` knows and the live ledger's
// `budget_refused` / `budget_unsettled` have a sentence; any other code gets
// the generic sentence, which still names the code; no code names the stage.
// The same for the codes of an evidence_unavailable record.

import { readFileSync } from "node:fs";

import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";

import { routing } from "../src/i18n/routing";
import {
  FAILURE_CODES,
  failureSentence,
  UNAVAILABLE_CODES,
  unavailableSentence,
} from "../src/lib/run/failure";

// crates/consensus-api/src/public_view.rs failure_message (each `Some("…")`
// arm), and the live ledger's codes (crates/consensus-api/src/meta.rs codes).
// The Rust sources are not mounted in the test container, so the list is
// copied here; FAILURE_CODES must cover it.
const SERVER_FAILURE_CODES = [
  "cancelled",
  "shutdown",
  "deadline",
  "interrupted",
  "panic",
  "model_timeout",
  "model_unavailable",
  "model_malformed_output",
  "evaluation_contract",
  "challenge_contract",
  "proposal_contract",
  "token_budget_exceeded",
  "token_usage_missing",
  "persistence",
];
const LEDGER_FAILURE_CODES = ["budget_refused", "budget_unsettled"];

type Catalog = { [key: string]: string | Catalog };

function runCatalog(locale: string): Catalog {
  const catalog = JSON.parse(
    readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), "utf8"),
  ) as Catalog;
  return catalog.Run as Catalog;
}

function lookup(catalog: Catalog, key: string): string {
  let node: string | Catalog | undefined = catalog;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  if (typeof node !== "string") {
    throw new Error(`no message ${key}`);
  }
  return node;
}

describe("failureSentence", () => {
  it("covers every code the server's failure_message knows and the ledger's", () => {
    for (const code of [...SERVER_FAILURE_CODES, ...LEDGER_FAILURE_CODES]) {
      expect(FAILURE_CODES as readonly string[], code).toContain(code);
      expect(failureSentence({ code, stage: "evaluate" })).toEqual({
        key: `codes.${code}`,
        values: {},
      });
    }
  });

  it("names an unknown code, and the stage when there is no code", () => {
    expect(failureSentence({ code: "a_new_code", stage: "critique" })).toEqual({
      key: "unknownCode",
      values: { code: "a_new_code" },
    });
    expect(failureSentence({ code: null, stage: "critique" })).toEqual({
      key: "noCode",
      values: { stage: "critique" },
    });
    expect(failureSentence({ stage: "propose" }).key).toBe("noCode");
  });

  it("reads evidence_unavailable codes the same way", () => {
    expect(unavailableSentence("memory_unavailable").key).toBe("codes.memory_unavailable");
    expect(unavailableSentence("foreign_record")).toEqual({
      key: "unknownCode",
      values: { code: "foreign_record" },
    });
    expect(unavailableSentence(undefined).key).toBe("noCode");
  });
});

describe.each(routing.locales)("%s failure sentences", (locale) => {
  const run = runCatalog(locale);

  it("has a sentence for every code", () => {
    for (const code of FAILURE_CODES) {
      expect(lookup(run, `failure.codes.${code}`).trim(), code).not.toBe("");
    }
    expect(Object.keys((run.failure as Catalog).codes as Catalog).sort()).toEqual(
      [...FAILURE_CODES].sort(),
    );
    for (const code of UNAVAILABLE_CODES) {
      expect(lookup(run, `unavailable.codes.${code}`).trim(), code).not.toBe("");
    }
  });

  it("the generic sentences still show the code or the stage", () => {
    const format = (key: string, values: Record<string, string>) =>
      String(new IntlMessageFormat(lookup(run, key), locale).format(values));
    expect(format("failure.unknownCode", { code: "x_code_1" })).toContain("x_code_1");
    expect(format("failure.noCode", { stage: "x_stage_1" })).toContain("x_stage_1");
    expect(format("failure.detail", { stage: "x_stage_1", code: "x_code_1" })).toContain("x_code_1");
    expect(format("unavailable.unknownCode", { code: "x_code_2" })).toContain("x_code_2");
  });
});
