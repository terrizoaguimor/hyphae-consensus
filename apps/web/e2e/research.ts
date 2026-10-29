// SPDX-License-Identifier: Apache-2.0
//
// The research archive (research/, verbatim and read-only) as the e2e suite
// sees it: on the host two directories above apps/web, inside the Playwright
// container at /research (playwright-docker.sh mounts it read-only there,
// as it mounts the contract). The mocked showcase serves its real bytes at
// /research/<path>, and builds showcase entries from its manifest and its
// logs, so the files, digests, run ids, outcomes, record counts and heads
// the views show are the archive's own.

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

type Json = Record<string, unknown>;

export function researchDir(): string {
  const fromEnv = process.env.API_RESEARCH_DIR;
  if (fromEnv !== undefined && fromEnv !== "") {
    return path.resolve(fromEnv);
  }
  // CommonJS under Playwright: __dirname is apps/web/e2e.
  for (const candidate of ["../../../research", "../../research"]) {
    const resolved = path.resolve(__dirname, candidate);
    if (existsSync(path.join(resolved, "MANIFEST.json"))) {
      return resolved;
    }
  }
  throw new Error("research/ not found (set API_RESEARCH_DIR, or run through scripts/playwright-docker.sh)");
}

/** An archive file's bytes, as the server serves them. */
export function researchBytes(file: string): Buffer {
  return readFileSync(path.join(researchDir(), ...file.split("/")));
}

type ManifestRun = {
  run_id: string;
  label: string;
  battery: string;
  task_title: string;
  outcome: string;
  authorization: string;
  schema_version: string;
  file: string;
  record: string | null;
};

type ManifestFile = { path: string; sha256: string; kind: string };

export function manifest(): { files: ManifestFile[]; runs: ManifestRun[] } {
  return JSON.parse(researchBytes("MANIFEST.json").toString("utf8")) as {
    files: ManifestFile[];
    runs: ManifestRun[];
  };
}

/** A run's records in its log: first and last sequence, count, the hash of its last record. */
export function logSegment(file: string, runId: string): { first: number; last: number; count: number; head: string | null } {
  const records = researchBytes(file)
    .toString("utf8")
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as Json)
    .filter((record) => record.run_id === runId);
  const last = records.at(-1);
  const head = typeof last?.hash === "string" && last.hash !== "" ? last.hash : null;
  return {
    first: (records[0]?.sequence as number | undefined) ?? 0,
    last: (last?.sequence as number | undefined) ?? 0,
    count: records.length,
    head,
  };
}

/** The request ids of a run's recorded Meta calls, in log order (what the server lists as meta_request_ids). */
export function metaRequestIds(file: string, runId: string): string[] {
  return researchBytes(file)
    .toString("utf8")
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as Json)
    .filter((record) => record.run_id === runId)
    .flatMap((record) => {
      const invocation = (record.payload as Json).invocation as Json | undefined | null;
      return invocation !== undefined &&
        invocation !== null &&
        invocation.provider === "meta" &&
        typeof invocation.request_id === "string"
        ? [invocation.request_id]
        : [];
    });
}

/** A run of the manifest, by its archive label. */
export function manifestRun(label: string): { run_id: string; file: string } {
  const run = manifest().runs.find((r) => r.label === label);
  if (run === undefined) {
    throw new Error(`no run ${label} in MANIFEST.json`);
  }
  return run;
}

/**
 * The showcase entry the server lists for an archived run, built from the
 * manifest (run id, battery, title, outcome, authorization, schema version,
 * file and its pinned digest, run record) and the run's log (record count,
 * head). The provenance axes are the server's derivation from the records;
 * the caller gives them.
 */
export function archiveEntry(label: string, provenance: Json): Json {
  const m = manifest();
  const run = m.runs.find((r) => r.label === label);
  if (run === undefined) {
    throw new Error(`no run ${label} in MANIFEST.json`);
  }
  const file = m.files.find((f) => f.path === run.file);
  if (file === undefined) {
    throw new Error(`no file ${run.file} in MANIFEST.json`);
  }
  const segment = logSegment(run.file, run.run_id);
  return {
    run_id: run.run_id,
    label: run.label,
    battery: run.battery,
    title: run.task_title,
    outcome: run.outcome,
    authorization: run.authorization,
    schema_version: run.schema_version,
    provenance: { source: "research_archive", ...provenance },
    file: run.file,
    sha256: file.sha256,
    head_hash: segment.head,
    record_file: run.record,
    event_count: segment.count,
  };
}

/**
 * A run's first proposal and every revision of it, as its archived log
 * records them (the proposal_submitted and revision_submitted payloads,
 * verbatim), in log order.
 */
export function archivedProposals(file: string, runId: string): { proposal: Json; revisions: Json[] } {
  const payloads = researchBytes(file)
    .toString("utf8")
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as Json)
    .filter((record) => record.run_id === runId)
    .map((record) => record.payload as Json);
  const proposal = payloads.find((payload) => payload.type === "proposal_submitted")?.proposal as Json | undefined;
  if (proposal === undefined) {
    throw new Error(`no proposal for ${runId} in ${file}`);
  }
  const revisions = payloads
    .filter((payload) => payload.type === "revision_submitted")
    .map((payload) => payload.revision as Json);
  return { proposal, revisions };
}
