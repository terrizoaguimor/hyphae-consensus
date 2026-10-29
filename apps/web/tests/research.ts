// SPDX-License-Identifier: Apache-2.0
//
// The research archive (research/, verbatim and read-only) for the unit
// tests: on the host two directories above apps/web, inside the Node
// container at /research (scripts/in-docker.sh mounts it read-only there, as
// it mounts the contract at /contracts). Tests read its bytes; nothing writes.

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function researchDir(): string {
  const fromEnv = process.env.API_RESEARCH_DIR;
  if (fromEnv !== undefined && fromEnv !== "") {
    return path.resolve(fromEnv);
  }
  const appDir = fileURLToPath(new URL("..", import.meta.url));
  for (const candidate of ["../../research", "../research"]) {
    const resolved = path.resolve(appDir, candidate);
    if (existsSync(path.join(resolved, "MANIFEST.json"))) {
      return resolved;
    }
  }
  throw new Error(
    "research/ not found (set API_RESEARCH_DIR, or run through scripts/in-docker.sh, which mounts it at /research)",
  );
}

/** An archive file's bytes, exactly as stored. */
export function researchBytes(file: string): Uint8Array {
  return new Uint8Array(readFileSync(path.join(researchDir(), ...file.split("/"))));
}

export type ManifestFile = {
  path: string;
  bytes: number;
  sha256: string;
  kind: string;
  runs?: number;
  events?: number;
  schema_versions: string[];
  description?: string;
};

export type ManifestRun = {
  run_id: string;
  label: string;
  battery: string;
  outcome: string;
  authorization: string;
  schema_version: string;
  file: string;
  /** The run record (task, detail, receipt as downloaded at run time), when the archive has one. */
  record?: string | null;
};

export function manifest(): { created: string; files: ManifestFile[]; runs: ManifestRun[] } {
  return JSON.parse(new TextDecoder().decode(researchBytes("MANIFEST.json"))) as {
    created: string;
    files: ManifestFile[];
    runs: ManifestRun[];
  };
}

type LogRecord = { run_id: string; payload: { invocation?: unknown } };

const parsedLogs = new Map<string, LogRecord[]>();

/** An archived log's records, parsed once per test file. */
function logRecords(file: string): LogRecord[] {
  let records = parsedLogs.get(file);
  if (records === undefined) {
    records = new TextDecoder()
      .decode(researchBytes(file))
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => JSON.parse(line) as LogRecord);
    parsedLogs.set(file, records);
  }
  return records;
}

/**
 * The request ids of a run's Meta invocations in its archived log, in order:
 * every record whose payload carries an invocation with `provider: meta` and
 * a request id (the proposal, challenge, evaluation and revision records, and
 * a failure that recorded its call), as the server derives
 * `provenance.meta_request_ids` (crates/consensus-api/src/public_view.rs).
 */
export function metaRequestIds(file: string, runId: string): string[] {
  return logRecords(file)
    .filter((record) => record.run_id === runId)
    .flatMap((record) => {
      const invocation = record.payload.invocation as { provider?: unknown; request_id?: unknown } | null | undefined;
      return invocation !== undefined &&
        invocation !== null &&
        invocation.provider === "meta" &&
        typeof invocation.request_id === "string"
        ? [invocation.request_id]
        : [];
    });
}
