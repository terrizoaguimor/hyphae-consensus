// SPDX-License-Identifier: Apache-2.0
//
// In-browser verification of a verbatim event log, exactly as
// crates/consensus-runtime/src/event_log.rs verifies one from its bytes
// (`verify_bytes`): the file is split into lines on "\n" (a trailing "\r" is
// cut), every line parses as one record, sequences run gap-free from 1,
// event ids are unique, schema versions are readable and never go back
// within a run, and every record from schema 1.1 on carries its link and
// hash: `prev_hash` is "genesis" for the first record, `pre-1.1:<n>` after a
// legacy record, the previous record's `hash` otherwise; `hash` is the
// lowercase hex SHA-256 of the `prev_hash` string followed by the record's
// own line with its top-level hash value blanked (`"hash":"<hex>"` becomes
// `"hash":""`). The line is blanked textually, never re-serialized: the
// archive's lines re-serialize byte for byte (research/README.md), so the
// textual recipe agrees with the runtime's own check, and a re-serializer
// here could only hide a difference. Legacy records (schema 1.0, no hash)
// are a readable prefix that is not chain-verified.
//
// The digest function is passed in (WebCrypto's SHA-256 in the browser and
// in Node), so this module is pure and unit-tested over real archived bytes
// (tests/chain-verify.test.ts). A chain that verifies shows that the records
// are internally consistent and, against a head or digest obtained
// elsewhere (the receipt, the archive's manifest), unchanged since; it says
// nothing about whether their content is true, and it is unsigned.

import { type Digest, hex, sha256 } from "./digest";

export const READABLE_SCHEMA_VERSIONS: readonly string[] = ["1.0", "1.1", "1.2", "1.3"];

/** Why a line fails, in the order event_log.rs checks. */
export type ChainProblem =
  | { kind: "not_utf8" }
  | { kind: "empty_line" }
  | { kind: "not_json" }
  | { kind: "torn_tail" }
  | { kind: "not_a_record" }
  | { kind: "schema_version"; found: string }
  | { kind: "sequence_gap"; expected: number; found: number }
  | { kind: "duplicate_id"; eventId: string }
  | { kind: "legacy_not_1_0"; found: string }
  | { kind: "legacy_with_prev" }
  | { kind: "legacy_after_chain" }
  | { kind: "chained_1_0" }
  | { kind: "prev_mismatch"; stored: string; expected: string }
  | { kind: "no_hash_field" }
  | { kind: "hash_mismatch"; stored: string; computed: string }
  | { kind: "schema_downgrade"; found: string; earlier: string; runId: string };

export type LineFailure = {
  /** 1-based line number in the file. */
  line: number;
  /** The record's sequence, when the line got far enough to have one. */
  sequence: number | null;
  problem: ChainProblem;
};

/** One record that passed every check. */
export type CheckedRecord = {
  line: number;
  sequence: number;
  runId: string;
  eventId: string;
  schemaVersion: string;
  prevHash: string;
  /** Empty for a legacy (pre-1.1) record. */
  hash: string;
};

export type ChainResult = {
  /** Lowercase hex SHA-256 of the whole file, as downloaded. */
  sha256: string;
  bytes: number;
  /** Every record that passed, in order, up to the first failure. */
  records: CheckedRecord[];
  failure: LineFailure | null;
  /** The file ends without a newline after its last complete record (accepted, as event_log.rs does). */
  unterminated: boolean;
};

/** Order of a schema version `<major>.<minor>`, or null. */
function schemaOrder(version: string): [number, number] | null {
  const match = /^(\d+)\.(\d+)$/.exec(version);
  return match === null ? null : [Number(match[1]), Number(match[2])];
}

/** Whether `version` is older than `earlier` (a version without an order counts as older unless equal). */
function isDowngrade(version: string, earlier: string): boolean {
  const now = schemaOrder(version);
  const before = schemaOrder(earlier);
  if (now === null || before === null) {
    return version !== earlier;
  }
  return now[0] < before[0] || (now[0] === before[0] && now[1] < before[1]);
}

const WHITESPACE = new Set([" ", "\t", "\n", "\r"]);

/** The end (exclusive) of the JSON string token that starts at `start` (a quote). */
function stringEnd(text: string, start: number): number {
  let i = start + 1;
  while (i < text.length) {
    const c = text[i];
    if (c === "\\") {
      i += 2;
      continue;
    }
    if (c === '"') {
      return i + 1;
    }
    i += 1;
  }
  return -1;
}

/**
 * The span of the string value of the top-level member `key` of a JSON
 * object line (the quotes included), or null when the object has no such
 * member or its value is not a string. Only top-level keys are matched: a
 * nested `"hash"` key or the text `"hash":"…"` inside a string never is.
 */
export function topLevelStringSpan(line: string, key: string): { start: number; end: number } | null {
  let depth = 0;
  let i = 0;
  while (i < line.length) {
    const c = line[i];
    if (c === '"') {
      const end = stringEnd(line, i);
      if (end === -1) {
        return null;
      }
      if (depth === 1) {
        let j = end;
        while (j < line.length && WHITESPACE.has(line[j] ?? "")) {
          j += 1;
        }
        if (line[j] === ":") {
          const name = JSON.parse(line.slice(i, end)) as string;
          j += 1;
          while (j < line.length && WHITESPACE.has(line[j] ?? "")) {
            j += 1;
          }
          if (name === key) {
            if (line[j] !== '"') {
              return null;
            }
            const valueEnd = stringEnd(line, j);
            return valueEnd === -1 ? null : { start: j, end: valueEnd };
          }
          i = j;
          continue;
        }
      }
      i = end;
      continue;
    }
    if (c === "{" || c === "[") {
      depth += 1;
    } else if (c === "}" || c === "]") {
      depth -= 1;
    }
    i += 1;
  }
  return null;
}

/** The line as the chain hashes it: its top-level hash value blanked, nothing else touched. */
export function blankedLine(line: string): string | null {
  const span = topLevelStringSpan(line, "hash");
  return span === null ? null : `${line.slice(0, span.start)}""${line.slice(span.end)}`;
}

/** The record's own hash, recomputed from its line. */
export async function recordHash(prevHash: string, line: string, digest: Digest = sha256): Promise<string | null> {
  const blanked = blankedLine(line);
  if (blanked === null) {
    return null;
  }
  return hex(await digest(new TextEncoder().encode(`${prevHash}${blanked}`)));
}

type Parsed = {
  schema_version: string;
  sequence: number;
  event_id: string;
  run_id: string;
  prev_hash: string;
  hash: string;
};

function asRecord(value: unknown): Parsed | null {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const text = (field: string, fallback?: string): string | null => {
    const v = record[field];
    if (v === undefined && fallback !== undefined) {
      return fallback;
    }
    return typeof v === "string" ? v : null;
  };
  const schema = text("schema_version");
  const eventId = text("event_id");
  const runId = text("run_id");
  // Missing chain fields read as empty, as `#[serde(default)]` reads them.
  const prev = text("prev_hash", "");
  const hash = text("hash", "");
  const sequence = record.sequence;
  if (
    schema === null ||
    eventId === null ||
    runId === null ||
    prev === null ||
    hash === null ||
    typeof sequence !== "number" ||
    !Number.isSafeInteger(sequence) ||
    sequence < 0
  ) {
    return null;
  }
  return { schema_version: schema, sequence, event_id: eventId, run_id: runId, prev_hash: prev, hash };
}

/** Splits the bytes into lines on 0x0A, keeping each line's bytes. */
function byteLines(bytes: Uint8Array): Uint8Array[] {
  const lines: Uint8Array[] = [];
  let start = 0;
  for (let i = 0; i < bytes.length; i += 1) {
    if (bytes[i] === 0x0a) {
      lines.push(bytes.subarray(start, i));
      start = i + 1;
    }
  }
  lines.push(bytes.subarray(start));
  return lines;
}

/** Verifies a log file from its bytes; stops at the first failing line. */
export async function verifyLog(bytes: Uint8Array, digest: Digest = sha256): Promise<ChainResult> {
  const fileSha256 = hex(await digest(bytes));
  const records: CheckedRecord[] = [];
  const ids = new Set<string>();
  const runSchema = new Map<string, string>();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const endsWithNewline = bytes.length > 0 && bytes[bytes.length - 1] === 0x0a;
  const result = (failure: LineFailure | null, unterminated = false): ChainResult => ({
    sha256: fileSha256,
    bytes: bytes.length,
    records,
    failure,
    unterminated,
  });
  if (bytes.length === 0) {
    return result(null);
  }
  const lines = byteLines(bytes);
  for (let index = 0; index < lines.length; index += 1) {
    const lineNo = index + 1;
    const isLast = index === lines.length - 1;
    let text: string;
    try {
      text = decoder.decode(lines[index]);
    } catch {
      return result({ line: lineNo, sequence: null, problem: { kind: "not_utf8" } });
    }
    const line = text.endsWith("\r") ? text.replace(/\r+$/, "") : text;
    if (line === "") {
      if (isLast) {
        break;
      }
      return result({ line: lineNo, sequence: null, problem: { kind: "empty_line" } });
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      const problem: ChainProblem = isLast && !endsWithNewline ? { kind: "torn_tail" } : { kind: "not_json" };
      return result({ line: lineNo, sequence: null, problem });
    }
    const record = asRecord(parsed);
    if (record === null) {
      return result({ line: lineNo, sequence: null, problem: { kind: "not_a_record" } });
    }
    const fail = (problem: ChainProblem) =>
      result({ line: lineNo, sequence: record.sequence, problem });
    if (!READABLE_SCHEMA_VERSIONS.includes(record.schema_version)) {
      return fail({ kind: "schema_version", found: record.schema_version });
    }
    const expectedSequence = records.length + 1;
    if (record.sequence !== expectedSequence) {
      return fail({ kind: "sequence_gap", expected: expectedSequence, found: record.sequence });
    }
    if (ids.has(record.event_id)) {
      return fail({ kind: "duplicate_id", eventId: record.event_id });
    }
    ids.add(record.event_id);
    const previous = records.at(-1);
    if (record.hash === "") {
      // A legacy (pre-1.1) prefix: schema 1.0, no link, nothing chained before it.
      if (record.schema_version !== "1.0") {
        return fail({ kind: "legacy_not_1_0", found: record.schema_version });
      }
      if (record.prev_hash !== "") {
        return fail({ kind: "legacy_with_prev" });
      }
      if (previous !== undefined && previous.hash !== "") {
        return fail({ kind: "legacy_after_chain" });
      }
    } else {
      if (record.schema_version === "1.0") {
        return fail({ kind: "chained_1_0" });
      }
      const expectedPrev =
        previous === undefined
          ? "genesis"
          : previous.hash === ""
            ? `pre-1.1:${previous.sequence}`
            : previous.hash;
      if (record.prev_hash !== expectedPrev) {
        return fail({ kind: "prev_mismatch", stored: record.prev_hash, expected: expectedPrev });
      }
      const computed = await recordHash(record.prev_hash, line, digest);
      if (computed === null) {
        return fail({ kind: "no_hash_field" });
      }
      if (computed !== record.hash) {
        return fail({ kind: "hash_mismatch", stored: record.hash, computed });
      }
    }
    const earlier = runSchema.get(record.run_id);
    if (earlier !== undefined && isDowngrade(record.schema_version, earlier)) {
      return fail({
        kind: "schema_downgrade",
        found: record.schema_version,
        earlier,
        runId: record.run_id,
      });
    }
    if (earlier === undefined || earlier !== record.schema_version) {
      runSchema.set(record.run_id, record.schema_version);
    }
    records.push({
      line: lineNo,
      sequence: record.sequence,
      runId: record.run_id,
      eventId: record.event_id,
      schemaVersion: record.schema_version,
      prevHash: record.prev_hash,
      hash: record.hash,
    });
  }
  return result(null, !endsWithNewline);
}

/** What the run's records in the file are expected to be, from the data the views hold. */
export type SegmentExpectation = {
  runId: string;
  /** The file's pinned SHA-256 (ShowcaseEntryV1.sha256), when the file comes from the archive. */
  sha256?: string | null;
  /** The receipt's head hash (the hash of the run's last record). */
  receiptHead?: string | null;
  /** The receipt's event count. */
  receiptEventCount?: number | null;
  /** The detail's chain: first and last sequence of the run's records. */
  firstSequence?: number | null;
  lastSequence?: number | null;
};

export type Verdict = "pass" | "fail" | "not_compared";

export type SegmentResult = {
  /** The run's records in the file (only those before a failure, if any). */
  records: CheckedRecord[];
  first: number | null;
  last: number | null;
  /** The hash of the run's last record; null when it is unchained or absent. */
  head: string | null;
  chain: Verdict;
  sha256: Verdict;
  head_matches: Verdict;
  count: Verdict;
  range: Verdict;
  /** Every comparison that could be made passed, and the chain verified. */
  pass: boolean;
  /** The run's records carry no hash: nothing in them is chain-verified. */
  unchained: boolean;
};

const compare = <T>(expected: T | null | undefined, actual: T | null): Verdict =>
  expected === undefined || expected === null ? "not_compared" : expected === actual ? "pass" : "fail";

/** The run's segment of a verified file, compared with what the receipt, the detail and the archive say. */
export function checkSegment(result: ChainResult, expected: SegmentExpectation): SegmentResult {
  const records = result.records.filter((r) => r.runId === expected.runId);
  const first = records[0]?.sequence ?? null;
  const last = records.at(-1)?.sequence ?? null;
  const lastHash = records.at(-1)?.hash ?? "";
  const head = lastHash === "" ? null : lastHash;
  const unchained = records.length > 0 && records.every((r) => r.hash === "");
  const chain: Verdict = result.failure === null && records.length > 0 ? "pass" : "fail";
  const sha256 = compare(expected.sha256, result.sha256);
  const headMatches =
    expected.receiptHead === undefined
      ? "not_compared"
      : (expected.receiptHead ?? null) === head
        ? "pass"
        : "fail";
  const count = compare(expected.receiptEventCount, records.length);
  const range =
    (expected.firstSequence === undefined || expected.firstSequence === null) &&
    (expected.lastSequence === undefined || expected.lastSequence === null)
      ? "not_compared"
      : (expected.firstSequence ?? first) === first && (expected.lastSequence ?? last) === last
        ? "pass"
        : "fail";
  const verdicts = [chain, sha256, headMatches, count, range];
  return {
    records,
    first,
    last,
    head,
    chain,
    sha256,
    head_matches: headMatches,
    count,
    range,
    pass: verdicts.every((v) => v !== "fail"),
    unchained,
  };
}
