// SPDX-License-Identifier: Apache-2.0
//
// What the views read from a receipt (RunReceiptV2): the plain-language
// limits of what proofs establish, and the exact command a third party runs
// to check a downloaded log against it. The Proof inspector words its plain
// view from the receipt's own `proof_limitations`: the catalogs translate
// exactly the text the server writes (RunReceipt::DEFAULT_LIMITATIONS,
// pinned below and checked against every receipt fixture of the contract),
// and a receipt that carries any other text is shown as written, never
// paraphrased. Pure functions, unit-tested in tests/run-receipt.test.ts.

/**
 * The limitations text every receipt v2 carries
 * (crates/consensus-domain/src/receipt.rs, RunReceipt::DEFAULT_LIMITATIONS).
 * The translated sentences of `Run.proofs.limits` restate exactly this text.
 */
export const KNOWN_PROOF_LIMITATIONS =
  "A proof marked valid means that Hyphae's verifier re-executed the request the proof sealed against its witness and accepted it at the anchor it was given (the pinned anchor wherever one is configured, always for the public snapshot). It covers that request's whole result (for the sealed snapshot, the complete record list it captured), not the claim that led to the retrieval, and a record's text is not byte-compared with the proof. A simulated proof checks nothing. Proofs do not establish that the content is true, relevant, or complete, or that a model reasoned from it.";

/** Whether the catalogs' translation of the limitations applies to this receipt's text. */
export function limitationsTranslated(proofLimitations: string): boolean {
  return proofLimitations.trim() === KNOWN_PROOF_LIMITATIONS;
}

/**
 * The receipt a run record of the research archive holds: the one the
 * battery driver downloaded after the run (`scripts/live_battery.py`), with
 * the limitations text the software wrote then. A recorded run's served
 * receipt (GET /showcase/{id}/receipt) is not that file: the server builds
 * it from the archived log on each request, so its `generated_at` is the
 * request's time and its `proof_limitations` are today's text.
 */
export type RunTimeReceipt = {
  generatedAt: string | null;
  proofLimitations: string | null;
};

/**
 * The run-time receipt of a run record's bytes, or null when they are not a
 * run record of this run (not UTF-8 JSON, no `receipt` object, or a receipt
 * of another run). Fields the receipt does not carry as text are null.
 */
export function runTimeReceipt(bytes: Uint8Array, runId: string): RunTimeReceipt | null {
  let record: unknown;
  try {
    record = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    return null;
  }
  if (!isObject(record) || !isObject(record.receipt)) {
    return null;
  }
  const receipt = record.receipt;
  if (receipt.run_id !== runId) {
    return null;
  }
  const text = (value: unknown) => (typeof value === "string" && value.trim() !== "" ? value : null);
  return {
    generatedAt: text(receipt.generated_at),
    proofLimitations: text(receipt.proof_limitations),
  };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The download names the server gives a run's log and receipt (Content-Disposition). */
export function ownLogFileName(runId: string): string {
  return `events-${runId}.jsonl`;
}

export function receiptFileName(runId: string): string {
  return `receipt-${runId}.json`;
}

/** The last path segment of an archive file (`2026-10-27/x.jsonl` -> `x.jsonl`). */
export function baseName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

/**
 * The read-only check of a downloaded log and receipt
 * (crates/consensus-api/src/verify_cli.rs): `consensus-api verify <log.jsonl>
 * --run <id> --receipt <receipt.json>`. File names are the ones the browser
 * saves; a name that needs quoting is quoted for a POSIX shell.
 */
export function verifyCommand(logFile: string, runId: string, receiptFile: string): string {
  const quote = (value: string) => (/^[A-Za-z0-9._:/-]+$/.test(value) ? value : `'${value.replace(/'/g, "'\\''")}'`);
  return `consensus-api verify ${quote(logFile)} --run ${quote(runId)} --receipt ${quote(receiptFile)}`;
}
