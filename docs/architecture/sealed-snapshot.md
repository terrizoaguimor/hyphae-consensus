# The sealed Hyphae snapshot: capture, artifacts, verification, runtime

Status: authoritative description of plan item B11 (ADR-0008). Written 2026-10-27 for the
capture session of 2026-10-29. Lane A (this document, `scripts/hyphae-container/capture-snapshot.sh`,
`scripts/snapshot_manifest.py`) produces the artifact; lane B (`SnapshotMemoryClient` in
`consensus-api`) consumes it. The artifact layout and the manifest schema in section 4 are the
contract between the two lanes and must not drift.

## 1. Why (ADR-0008 in short)

Public live runs need evidence with real integrity proofs, but the Hyphae container that served
the recorded batteries cannot sit behind a public server: the adapter reaches it through
`docker exec` (Docker-daemon access is root-equivalent and sits next to the writer and operator
keys); proven recall through `hyphae mcp` fails once a store passes about 3.9 MB because the
4 MiB MCP response carries the whole directory witness; every proven recall writes a ~3.3 MB
witness with no retention; and a witness is a full copy of the data directory, including the WAL
with forgotten records and the access-control catalog with API-key verifiers.

The decision: **capture once, in one authorized container session**, a new, purpose-built Agent
Memory instance that holds only the 7 synthetic `demo-repo` facts; capture proven results through
the released operator surface `hyphae agent ui`; verify every capture with `hyphae proof verify`;
write a manifest; destroy that instance's credentials so the store is never served again. The
public runtime (`sealed_snapshot` evidence mode) answers from the captured artifacts, re-verifying
the proofs with the same verifier at boot and per run, and never reaches the container. Witnesses
and proofs stay private on the server; the public sees digests. The honest label on every run:
"Evidence: sealed Hyphae snapshot captured <date>; integrity re-verified now with
`hyphae proof verify` against pinned anchor <prefix>; this proves the records are the snapshot's
records, not that they are true or that a model used them."

## 2. What the container told us (read-only, 2026-10-27)

Facts established with the permitted read-only commands against the running `hyphae-consensus`
container (debian:trixie-slim, `/bin/sh` is dash, `hyphae 4.0.0` at
`/opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae`):

- `hyphae agent ui --help` prints only: "One bounded JSON operator request on stdin; used by the
  Omarchy plugin. Usage: hyphae agent ui". No options, nothing about a daemon, nothing about the
  output format. `hyphae agent --help` describes the agent surface as "the bounded read-only MCP
  adapter over managed Native HTTP v2" with the lifecycle commands `setup`, `status`, `doctor`,
  `backup`, `remove` ("Remove generated credentials while preserving data and backups"),
  `restore`, `upgrade`, `configure`, `hook`, `purge-data`, `maintain`, `recover`, `ui`.
- `hyphae proof verify --help`: `hyphae proof verify --proof <PROOF> --witness <WITNESS>
  --anchor <ANCHOR>` where the anchor is the "independently trusted 32-byte native anchor
  digest". This is the exact CLI the runtime already uses (module doc of
  `crates/consensus-api/src/hyphae.rs`). `hyphae proof generate` exists but cannot produce
  memory proofs (independent audit, HY-11); only `recall`/`list` with `prove` do.
- Tools: `sha256sum`, `awk` (mawk 1.3.4), `sed`, `grep`, `timeout`, `cmp`, `stat`, `find`,
  `mktemp`, `du`, `nohup` exist; `jq`, `python3`, `b3sum` and `ps` do not. The capture script
  therefore reads JSON with a small embedded awk tokenizer that fails closed, and digests with
  `sha256sum`. BLAKE3 digests (`proof_blake3`) are copied from Hyphae's own response, never
  recomputed in the container; the host-side `check` compares them to the response as well.
- `hyphae agent setup --no-service` is the flag that created the demo and lab stores on
  2026-10-27 (`scripts/hyphae-container/README.md`, `docs/reviews/2026-10-27-claude-fix-report-2.md`).
  The script records `hyphae agent setup --help` in `capture.log` at the start of the session so
  the flag is evidenced in the artifact itself.

**Does `hyphae agent ui` need a running daemon?** The help texts do not say. The audit's reading
of the source says `agent ui` forwards `recall` and `list` "to the same proven path with the
reader key", the path the MCP adapter uses, and the MCP adapter is a daemon client
(`--endpoint`/`--base-url` are mandatory). The script therefore keeps the daemon it started for
the new HOME running through the captures (default). If the session shows `agent ui` opening the
data directory itself and refusing a directory held by a daemon, rerun with `--ui-direct`, which
stops the daemon after seeding and before the captures. Seeding itself always needs the daemon:
the only write path available to us is the MCP writer path of `seed.sh`/`seed-demo-facts.sh`
(`hyphae_store` in `hyphae-env.sh`), so the script starts the daemon before seeding (its step 2)
rather than after it. This is the one deliberate reordering against the plan text; it changes
nothing about what is captured.

## 3. The capture procedure (host commands, 2026-10-29)

Everything below is run by the integrator from the repository root on the Windows dev machine.
PowerShell is shown; from Git Bash prefix `docker` commands with `MSYS_NO_PATHCONV=1` and never
put container paths into environment variables from Git Bash (MSYS rewrites them). `<id>` is the
snapshot id, `snap-2026-10-29-demo-repo` unless decided otherwise; it must match
`^[a-z0-9][a-z0-9-]{0,63}$` and becomes the directory name everywhere.

1. **Preconditions.** The container runs (`docker ps`), the demo daemon and the lab daemon may
   keep running (they are not touched). Copy the current scripts in; the capture script sources
   `/hyphae-home/hyphae-env.sh` and cross-checks the seed texts against `/hyphae-home/seed.sh`
   and `/hyphae-home/seed-demo-facts.sh`, so all of them must be the current versions:

   ```powershell
   foreach ($f in Get-ChildItem scripts\hyphae-container\*.sh) { docker cp $f.FullName "hyphae-consensus:/hyphae-home/$($f.Name)" }
   docker exec hyphae-consensus hyphae --version
   ```

2. **Rehearse without touching anything** (prints every command, creates nothing):

   ```powershell
   docker exec hyphae-consensus sh /hyphae-home/capture-snapshot.sh --dry-run <id>
   ```

   Optional, recommended by the audit ("confirmed on a scratch store first"): run a real session
   with a scratch id (`scratch-2026-10-29-a`), read its `capture.log`, then run the real one.
   The script never deletes an instance; a scratch instance stays under
   `/hyphae-home/sealed/scratch-…` (credentials destroyed, data read-only) until the owner
   removes it.

3. **The session** (one command; about two minutes: setup, daemon, 7 stores of about 6 s each,
   two proven captures, two verifications):

   ```powershell
   docker exec hyphae-consensus sh /hyphae-home/capture-snapshot.sh <id>
   ```

   What it does, in order, each step logged with a UTC timestamp to
   `/hyphae-home/snapshots/<id>/capture.log` and every mutating command printed as `+ <command>`
   before it runs:

   | Step | Mutation in the container | Guard |
   |---|---|---|
   | 0 | creates `/hyphae-home/snapshots/<id>/captures/` and `capture.log`; records `hyphae --version`, `agent ui --help`, `agent setup --help`, `proof verify --help` | refuses if the instance HOME or the output directory exists; refuses the demo and lab HOMEs; requires the sibling scripts and the tools |
   | 1 | `mkdir /hyphae-home/sealed/<id>`; `HOME=/hyphae-home/sealed/<id> hyphae agent setup --no-service` (its stdout stays in `<HOME>/setup.out`, inside the container, because whether it prints credential material is unverified) | the data directory and the credentials directory must appear where `hyphae-env.sh` derives them; the canonical socket printed by `hyphae agent configure` must lie outside the data directory |
   | 2 | `hyphae agent recover`, then `nohup hyphae serve --data-dir <data> --endpoint <socket> --native-api-key-auth` for the new HOME only (the two commands of `serve.sh`) | waits for the socket; the pid is kept so the daemon is stopped and reaped by the script |
   | 3 | 7 × `hyphae_store demo-repo <kind> "<text>"` (writer credential of the new instance, MCP writer path) | each text must appear verbatim in `seed.sh` or `seed-demo-facts.sh`; a store error or an empty response aborts; nothing is ever forgotten |
   | 4 | two `hyphae agent ui` requests (below); the proof and witness files Hyphae writes under `<HOME>/.local/state/hyphae/proofs/` are copied verbatim to `captures/` and compared with `cmp` | `proof` must not be null; `proof.scope` must be `memory-snapshot-v1`; the list must hold exactly 7 records, all of project `demo-repo`; the `other-repo` recall must be empty; both proofs must carry the same `anchor_hex` |
   | 5 | none (reads) | `hyphae proof verify --proof captures/<id>.proof --witness captures/<id>.witness --anchor <anchor_hex of the list response>`: exit 0 and a report with `status = verified`, `kind = memory`, `scope = semantic_reexecution`, `semantic_reexecution_performed = true`, `anchor_digest` equal to the anchor given |
   | 6 | none (reads) | `hyphae --version` and `sha256sum` of `readlink -f $(command -v hyphae)` |
   | 7 | `kill -INT <daemon pid>` (waits for the socket to vanish, reaps the process); `rm -rf <HOME>/.config/hyphae/credentials` after listing every file in it (names and sizes only, never contents); `chmod -R a-w <data directory>` | the credentials directory must be gone afterwards |
   | 8 | writes `captures.index` and `session.json` into the output directory | |

   The two requests (`id` is any string; the response is written verbatim):

   ```json
   {"schema":"hyphae-omarchy-control-v1","id":"capture-list-demo-repo","operation":"list","arguments":{"project":"demo-repo","query":"","limit":64,"prove":true}}
   {"schema":"hyphae-omarchy-control-v1","id":"capture-recall-other-repo-isolation","operation":"recall","arguments":{"project":"other-repo","query":"Fix payment client timeouts","limit":8,"prove":true}}
   ```

   If the session fails, the exit code says where (2 usage, 3 precondition/setup/daemon/seeding,
   4 capture checks, 5 verification, 6 credential destruction), the daemon it started is stopped,
   and the log ends with a note. Nothing is deleted. The instance HOME may still hold
   credentials: destroy them by hand as the owner (`ls` the credentials directory, `rm -rf` it,
   `chmod -R a-w` the data directory, exactly what step 7 does) or leave the instance as a
   scratch and rerun with a new id. `sh /hyphae-home/capture-snapshot.sh --inspect <file>`
   prints what the script extracts from a saved response or verifier report, for diagnosis.

4. **Copy the output to the host** (the witness copies included; they are private files on the
   dev machine and are git-ignored):

   ```powershell
   New-Item -ItemType Directory -Force snapshots | Out-Null
   docker cp hyphae-consensus:/hyphae-home/snapshots/<id> snapshots/
   ```

5. **Build and check the manifest** (standard-library Python, always with `-I`):

   ```powershell
   python -I scripts/snapshot_manifest.py build snapshots/<id> --expect-ids research-ids.txt
   python -I scripts/snapshot_manifest.py check snapshots/<id>
   ```

   `--expect-ids` is optional but recommended: save the list below as `research-ids.txt` (a
   scratch file, not committed). It holds the 7 memory ids every recorded `demo-repo` run
   retrieved; ids are derived from project + layer + text, so identical texts must reproduce
   them, and a difference means a text or kind changed and must be explained before the
   snapshot is used:

   ```text
   186670067796945807561772405393844769735
   8864874238917673930644756936982232317
   328114099256634798832222840773545327795
   295729039551322334710406856728557559035
   287567405605917891851290763137356115373
   237412834382378687445660534362297845298
   161896895843658017381255677281559699870
   ```

   (from `research/2026-10-27/live-battery-2026-10-27-final/F01-reference.json`; the first two
   are the `seed.sh` records as the adapter test fixture names them).

6. **Dev-machine runtime (Windows).** `CONSENSUS_HYPHAE_BIN` is `scripts\hyphae-docker.cmd`,
   which runs `hyphae` inside the container, so the verifier sees container paths: copy the two
   generated files next to the captures that already live in the container, then configure from
   PowerShell only:

   ```powershell
   docker cp snapshots\<id>\manifest.json hyphae-consensus:/hyphae-home/snapshots/<id>/manifest.json
   docker cp snapshots\<id>\SHA256SUMS   hyphae-consensus:/hyphae-home/snapshots/<id>/SHA256SUMS
   $env:CONSENSUS_SNAPSHOT_DIR = (Resolve-Path snapshots\<id>).Path
   $env:CONSENSUS_SNAPSHOT_VERIFIER_DIR = "/hyphae-home/snapshots/<id>"
   $env:CONSENSUS_HYPHAE_BIN = (Resolve-Path scripts\hyphae-docker.cmd).Path
   $env:CONSENSUS_HYPHAE_EXPECTED_ANCHOR = (Get-Content snapshots\<id>\manifest.json | ConvertFrom-Json).anchor_hex
   ```

7. **Server (Linux).** Copy the whole directory with `scp -r snapshots/<id>
   <server>:/var/lib/hyphae-consensus/snapshots/`, owned by the service user, directory `0700`,
   files `0600`: the witnesses are private. Install the native Linux `hyphae` 4.0.0 whose
   `sha256sum` equals `manifest.hyphae.binary_sha256` (the tarball digest is matched against the
   GitHub release asset; the binary digest is what the manifest records). Configuration:
   `CONSENSUS_SNAPSHOT_DIR=/var/lib/hyphae-consensus/snapshots/<id>`,
   `CONSENSUS_HYPHAE_EXPECTED_ANCHOR=<manifest.anchor_hex>`, `CONSENSUS_HYPHAE_BIN=/usr/local/bin/hyphae`,
   `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS` as today; `CONSENSUS_SNAPSHOT_VERIFIER_DIR` unset (the
   native binary sees the same path). No daemon, no credentials, no writer path on the server
   (release checklist G9). If Python 3 is on the server, `python3 -I scripts/snapshot_manifest.py
   check /var/lib/hyphae-consensus/snapshots/<id>` is the deployment review step.

8. **What gets committed.** `git add snapshots/<id>`: the `.proof` and `.witness` files are
   already ignored (`.gitignore`), everything else (`manifest.json`, `SHA256SUMS`, the
   `.response.json` and `.verify.json` files, `captures.index`, `session.json`, `capture.log`,
   `hyphae-version.txt`, `hyphae-binary.sha256`) may be committed. Confirm with `git status
   --short snapshots/` and `git check-ignore -v snapshots/<id>/captures/*.proof`. The committed
   subset must pass `python -I scripts/snapshot_manifest.py check snapshots/<id> --public-only`.
   The anchor and the binary digest are additionally published out of band (README, submitted
   tag) as ADR-0008 decides. The public live mode is pinned to the **snapshot's** anchor, which
   is a new value: it is not the demo store's `d91ac10d…` anchor of the recorded batteries.

## 4. Artifact layout and manifest (contract between the lanes)

A snapshot directory, e.g. `snapshots/snap-2026-10-29-demo-repo/`:

```text
manifest.json
SHA256SUMS                 (sha256sum format, "<64 hex>  <relative path>", one line per file under the directory except SHA256SUMS itself; manifest.json included; paths relative to the snapshot directory with forward slashes)
captures/<capture id>.response.json   (the exact JSON result object of the agent ui operation, verbatim)
captures/<capture id>.proof           (bytes copied verbatim from the response's proof.proof_path)
captures/<capture id>.witness         (bytes copied verbatim from proof.witness_path; PRIVATE)
captures/<capture id>.verify.json     (the exact stdout of "hyphae proof verify" for that capture at capture time)
```

The session also leaves `captures.index`, `session.json`, `capture.log`, `hyphae-version.txt`
and `hyphae-binary.sha256` in the directory (section 5); they are ordinary files listed in
`SHA256SUMS`. `captures/<id>.response.json` is the exact stdout of `hyphae agent ui`: if that
stdout wraps the result in an envelope (`{"schema": …, "id": …, "result": {…}}`), the file keeps
the envelope and every reader (the manifest builder, the runtime) takes the result from
`result`; the help text does not say which shape is printed (section 8).

`manifest.json` (UTF-8, keys in this order, all required unless marked optional):

```jsonc
{
  "schema": "hyphae-consensus-snapshot-manifest-v1",
  "snapshot_id": "snap-2026-10-29-demo-repo",            // ^[a-z0-9][a-z0-9-]{0,63}$ and equal to the directory name
  "captured_at": "2026-10-29T15:04:05Z",                  // RFC 3339 UTC, seconds
  "project": "demo-repo",                                 // the one project the snapshot serves
  "anchor_hex": "<64 lowercase hex>",                     // the single anchor every capture's proof.anchor_hex equals
  "hyphae": {"version": "<text of hyphae --version>", "binary_sha256": "<64 hex of the binary that produced the captures>", "control_schema": "hyphae-omarchy-control-v1"},
  "store": {"record_count": 7, "root_digest": "<from the list response snapshot.root_digest>", "visible_csn": <int>, "logical_time_micros": <int>},
  "captures": [
    {"id": "list-demo-repo", "operation": "list", "arguments": {"project": "demo-repo", "query": "", "limit": 64, "prove": true},
     "response_file": "captures/list-demo-repo.response.json", "proof_file": "captures/list-demo-repo.proof", "witness_file": "captures/list-demo-repo.witness", "verify_file": "captures/list-demo-repo.verify.json",
     "proof_blake3": "<hex from the response>", "proof_sha256": "<64 hex>", "witness_sha256": "<64 hex>",
     "memory_ids": ["<decimal id>", "..."],                                   // exactly the ids in the response, in response order
     "verification": {"verified_at": "<RFC 3339 UTC>", "status": "verified", "kind": "memory", "scope": "semantic_reexecution", "anchor_hex": "<64 hex>"}},
    {"id": "recall-other-repo-isolation", "operation": "recall", "arguments": {"project": "other-repo", "query": "Fix payment client timeouts", "limit": 8, "prove": true}, ... same fields ..., "memory_ids": []}
  ]
}
```

Capture ids match `^[a-z0-9][a-z0-9-]{0,63}$`. Exactly these two captures are REQUIRED (list of
the served project with limit 64; the empty proven recall of `other-repo`); additional captures
are allowed but the runtime uses only these two. The snapshot directory is a DEPLOYMENT artifact
copied to the server with `scp`; in git, `.proof` and `.witness` files are already ignored
(`.gitignore`), everything else in `snapshots/` may be committed.

Key order inside each capture is as shown (`id`, `operation`, `arguments`, `response_file`,
`proof_file`, `witness_file`, `verify_file`, `proof_blake3`, `proof_sha256`, `witness_sha256`,
`memory_ids`, `verification`); `arguments` has exactly `project`, `query`, `limit`, `prove`;
`verification` has exactly `verified_at`, `status`, `kind`, `scope`, `anchor_hex`. The manifest
is written only by `scripts/snapshot_manifest.py build` (`json.dump`, indent 2, ASCII, trailing
newline); `check` enforces the schema, the patterns, the key order, the two required captures,
`memory_ids` equal to the response ids, `store` equal to the list response's `snapshot`, one
anchor everywhere, and recomputes every digest.

## 5. Session files written in the container (input of the manifest builder)

`captures.index`: one JSON object per line, one line per capture, in capture order:

```json
{"id":"list-demo-repo","operation":"list","arguments":{"project":"demo-repo","query":"","limit":64,"prove":true},"files":{"response":"captures/list-demo-repo.response.json","proof":"captures/list-demo-repo.proof","witness":"captures/list-demo-repo.witness","verify":"captures/list-demo-repo.verify.json"},"proof_blake3":"<hex>","anchor_hex":"<hex>","memory_ids":["<decimal id>","..."],"captured_at":"<RFC 3339 UTC>","verified_at":"<RFC 3339 UTC>"}
```

`arguments` is the exact object sent to `hyphae agent ui`; `memory_ids` are the ids the script
extracted from the response, in order; `captured_at` is the time the request was sent and
`verified_at` the time `hyphae proof verify` was started. The builder trusts none of it: every
value is compared with the response file and the verifier report, and a mismatch aborts.

`session.json`: `{"schema":"hyphae-consensus-capture-session-v1","snapshot_id","project",
"captured_at" (session start),"finished_at","anchor_hex","record_count","hyphae_version",
"binary_path","binary_sha256","control_schema","ui_mode","instance_home"}`. The builder takes
`captured_at`, `project`, `hyphae_version` and `binary_sha256` from it (falling back to
`hyphae-version.txt` and `hyphae-binary.sha256`) and ignores the rest.

`capture.log`: the complete session transcript with UTC timestamps, including the help texts
recorded at the start, every `+ <command>` line, every store response, every memory id, the
anchor, the verifier fields, the names and sizes of the credential files that were removed, and
the list of output files. It contains paths inside the container and no key material (the
`agent setup` output stays in the container, see section 3 step 1).

## 6. Verification semantics: what "verified" proves and does not prove

`hyphae proof verify --proof P --witness W --anchor A` exits 0 and prints a JSON report whose
`status` is `verified`, `kind` is `memory`, `scope` is `semantic_reexecution`,
`semantic_reexecution_performed` is `true` and `anchor_digest` equals `A` when the verifier has
re-executed the sealed request against the witness (a complete copy of the data directory at
capture time) and reproduced the sealed result, and when the proof is bound to the anchor it was
given. That is what the runtime requires for `Valid` (ADR-0003 decision 3, ADR-0008 decision 5),
and what the capture script and `snapshot_manifest.py` require before an artifact is accepted.

What this proves: the records in the capture are the records of that store at that anchor; the
proof and witness bytes have not changed since capture (digests in the manifest and in
`SHA256SUMS`); the empty `other-repo` recall is a proven absence at the same anchor.

What this does not prove: that the records are true (they are operator-seeded synthetic demo
facts); that a model read or used them; that the text handed to a model is byte-identical to the
proof's content (the runtime documents that the text is not byte-compared unless the layout
binding of ADR-0008 decision 5 is confirmed); that the anchor is independently trustworthy (it is
the value the store reported at capture, pinned out of band by the operator; pinning detects
change, not a lying server). A proof seals **its own request** (`list(project = demo-repo,
limit = 64)`), never the claim query that triggers retrieval at run time.

Verification is rerun by the runtime (same binary, same arguments) at boot and for every public
run, so each receipt carries its own verification time; a snapshot that fails its boot check
disables the mode instead of falling back silently.

## 7. Runtime behaviour (lane B implements; this is the documented contract)

- **Evidence mode `sealed_snapshot`** is fixed server-side for the public live route; `simulated`
  (guided mode) and `live_hyphae` (operator lab) are the other two, never a silent fallback.
- **Query-independent complete set.** Every recall of the served project returns the complete
  proven list set (the 7 records of `list-demo-repo`, in response order), whatever the query or
  limit; `limit` is accepted and recorded, the proven set is indivisible. The orchestrator's round
  context holds at most 20 records, so 7 always fit.
- **Isolation evidence.** A recall of any other project returns no records with a recorded
  reason; the captured empty proven recall of `other-repo` is reported as that project's
  isolation evidence (`RecallProven` with `empty = true`, carrying its own proof reference).
- **Proof request next to the live query.** Each evidence reference records the proof's real
  request (`EvidenceReference.proof_request`, schema 1.3) next to the claim query; the UI and the
  receipt say "proof covers: complete record list of `demo-repo` at anchor <prefix>", never
  "recall of <claim>". A proof is attached to a record only if that record is in the proof's
  captured result (tested).
- **Boot check.** At start the runtime reads `manifest.json`, requires
  `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` to be set and equal to `manifest.anchor_hex` (otherwise boot
  fails), checks the digests, and runs `hyphae proof verify` on both required captures with the
  verifier's own view of the directory (`CONSENSUS_SNAPSHOT_VERIFIER_DIR` when set, else
  `CONSENSUS_SNAPSHOT_DIR`), requiring the same report fields as section 6. Per run it re-verifies
  (cached by content digest per the existing verifier, HY-13) and stamps the verification time.
- **Configuration.** `CONSENSUS_SNAPSHOT_DIR` = path of the snapshot directory (`manifest.json`
  inside); `CONSENSUS_SNAPSHOT_VERIFIER_DIR` (optional) = the same directory as the verifier
  binary sees it (Windows dev machine: the container path, because `CONSENSUS_HYPHAE_BIN` is
  `scripts/hyphae-docker.cmd`; Linux server: unset); `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` mandatory
  and equal to the manifest anchor; `CONSENSUS_HYPHAE_BIN` and
  `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS` as today (`crates/consensus-api/src/config.rs`,
  `hyphae.rs`).
- **`/health`** reports the evidence mode, the snapshot id, `captured_at`, the anchor prefix, the
  Hyphae version from the manifest, the time and result of the last verification, and, when the
  boot check failed, the mode as disabled with a stable reason code (digest mismatch, verifier
  rejection, verifier error, anchor mismatch, manifest invalid). It never reports paths, witness
  contents or container details (G9: no witness or container path in any public response).

## 8. Privacy

A witness is a full copy of the sealed instance's data directory at capture time: its records,
its write-ahead log, its catalog and the access-control catalog with the verifiers of the
(now destroyed) reader and writer keys. The sealed instance never held anything but the 7
synthetic facts and its own credentials' verifiers, so a witness of it leaks nothing about the
owner's demo, lab or archived stores; it still stays private (owner decision: no downloadable
witnesses), is git-ignored, is copied to the server with `0600` permissions and is never served.
Proofs are private for the same reason. What the public sees: the manifest's digests, the
record ids and texts (synthetic), the anchor prefix, the verification status and time.

The capture script never prints or reads the content of a credential file (it lists names and
sizes with `find` and `stat` before removing the directory), never copies `agent setup`'s stdout
out of the container, and never carries the Meta key (none is set in the container).

## 9. Unverified until the capture session (2026-10-29)

Each item is guarded by the script (it aborts with a precise message) or by the builder; none is
assumed silently.

1. Whether `hyphae agent ui` needs the daemon or opens the store itself (section 2); default
   `--ui-via-daemon`, alternative `--ui-direct`.
2. Whether `agent ui` prints the result object itself or an envelope with `result`, and whether
   it prints compact or pretty-printed JSON; both shapes are handled (the tokenizer was exercised
   under dash + mawk 1.3.4 in a throwaway debian:trixie-slim container on compact and
   pretty-printed samples, including the live `proof verify` report shape of 2026-10-27).
3. Whether `agent ui` accepts the request as given (`schema`, `id`, `operation`, `arguments`
   with `project`, `query`, `limit`, `prove`), honours `prove` for `list`, and returns `proof`
   with `scope = memory-snapshot-v1`, `proof_path`, `witness_path`, `proof_blake3` (64 hex) and
   `anchor_hex` (64 hex) exactly as the MCP recall response does; `query = ""` for `list`.
4. Whether `hyphae agent setup --no-service` prints credential material on stdout (kept in the
   container either way) and whether `hyphae agent remove` would be a cleaner credential
   destruction than removing `<HOME>/.config/hyphae/credentials` (the layout `hyphae-env.sh`
   documents; the script asserts the directory exists before and is gone after).
5. Whether the ids of the 7 re-seeded records equal the recorded research ids (section 3 step 5;
   `--expect-ids`).
6. The proof and witness sizes of a fresh 7-record store (expected about 3.3 MB for the witness)
   and the time `hyphae proof verify` takes on the server.
7. The behaviour of `/health` and the receipt wording are lane B's implementation; section 7
   is the contract they are written against.
