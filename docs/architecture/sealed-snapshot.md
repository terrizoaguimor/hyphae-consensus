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
  recomputed: no BLAKE3 implementation is available anywhere in this project (no `b3sum` in the
  container, none in the Python standard library, none in `Cargo.lock`, which has `sha2` only),
  so the host-side `check` compares the value to the response and nothing else. The bindings
  that can be recomputed are the sha256 digests the session records (section 6).
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

   Before that, the script's own checks, which need no Hyphae: `dash scripts/hyphae-container/capture-snapshot.sh --selftest`
   on the host (dash + gawk) and the same under the container's shell and awk in a throwaway
   `debian:trixie-slim` container with the script mounted read-only (dash + mawk 1.3.4). It
   feeds the store response shapes `hyphae mcp` can print (a result object, `isError`, a
   tool-error envelope, a JSON-RPC error object) through the seeding check, the tokenizer with
   raw control characters inside strings, and the daemon stop and the signal traps with sleep
   processes as the daemon; every line must say `ok`.

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
   | 1 | `mkdir /hyphae-home/sealed/<id>`; `HOME=/hyphae-home/sealed/<id> hyphae agent setup --no-service` (its stdout goes to the script's scratch directory under `/tmp`, which is removed at exit on every path, success or failure, because whether it prints credential material is unverified; only its byte count is logged); then `hyphae-env.sh` is sourced for the new HOME, which runs `hyphae agent configure opencode` to learn the canonical endpoint | the data directory and the credentials directory must appear where `hyphae-env.sh` derives them; every file `setup` and `configure` left under the HOME outside the data directory is listed (names and sizes); `configure` must not change the data directory; the canonical socket must be one plain absolute path outside the data directory, and no socket may already exist there (another daemon would answer the captures) |
   | 2 | `hyphae agent recover`, then `nohup hyphae serve --data-dir <data> --endpoint <socket> --native-api-key-auth` for the new HOME only (the two commands of `serve.sh`) | waits for the socket; the pid is kept so the daemon is stopped and reaped by the script |
   | 3 | 7 × `hyphae_store demo-repo <kind> "<text>"` (writer credential of the new instance, MCP writer path) | each text must appear verbatim in `seed.sh` or `seed-demo-facts.sh`; only a response line that is a tools/call result object without an error marker counts as stored: an empty response, `isError`, a tool-error envelope or a JSON-RPC error object (`{"jsonrpc":"2.0","id":2,"error":{…}}`, what `hyphae mcp` prints when a request is refused) aborts with the raw line in the log; nothing is ever forgotten |
   | 4 | two `hyphae agent ui` requests (below); the proof and witness files Hyphae writes (under `<HOME>/.local/state/hyphae/proofs/` for the demo store) are copied verbatim to `captures/` and compared with `cmp`; the sha256 of the response, the proof copy and the witness copy is computed the moment they are written and goes into the log and `captures.index` | `proof` must not be null; `proof.scope` must be `memory-snapshot-v1`; the proof and witness paths must lie under the new HOME (and not inside its data directory), otherwise `agent ui` answered from another store; the list must hold exactly 7 records, all of project `demo-repo`; the `other-repo` recall must be empty; both proofs must carry the same `anchor_hex` and must not be byte-identical; an anchor equal to the demo store's `d91ac10d…` is logged as a warning |
   | 5 | none (reads) | `hyphae proof verify --proof captures/<id>.proof --witness captures/<id>.witness --anchor <anchor_hex of the list response>`: exit 0 and a report with `status = verified`, `kind = memory`, `scope = semantic_reexecution`, `semantic_reexecution_performed = true`, `anchor_digest` equal to the anchor given; the report's own digests (`proof_digest`, `witness_digest`, `request_digest`, `result_digest`, …) and the sha256 of the report file are logged, the latter also recorded in `captures.index` |
   | 6 | none (reads) | `hyphae --version` and `sha256sum` of `readlink -f $(command -v hyphae)` |
   | 7 | `kill -INT <daemon pid>` (waits up to 30 s for the socket to vanish; a daemon still running 10 s later gets `SIGTERM`, one still running 10 s after that gets `SIGKILL`, each logged, so the wait is bounded and this step always runs; then the process is reaped); `rm -rf <HOME>/.config/hyphae/credentials` after listing every file in it (names and sizes only, never contents); `chmod -R a-w <data directory>` | the credentials directory must be gone afterwards; the files left under the HOME are listed again (the original proofs and witnesses, `serve.log`) |
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

   The same cleanup runs when the script is interrupted (`SIGINT`) or the `docker exec` session
   is torn down (`SIGTERM`, `SIGHUP`): the daemon is stopped with the bounded escalation of
   step 7, the scratch directory is removed, and the signal is re-raised (exit 130/143/129).
   What no trap can catch is `SIGKILL`, and a signal arriving while `hyphae agent ui` or
   `hyphae proof verify` runs in the foreground is acted on only when that child returns. If
   the session died that way, a daemon of the sealed HOME may still be running; the container
   has no `ps`, so find it through `/proc` (only the one whose `--data-dir` lies under
   `/hyphae-home/sealed/<id>`; the demo and lab daemons are not touched):

   ```sh
   for p in /proc/[0-9]*; do
     c=$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null) || continue
     case $c in *"hyphae serve"*) echo "${p#/proc/}: $c" ;; esac
   done
   kill -INT <pid>      # SIGINT makes it remove its socket; then destroy the credentials as above
   ```

4. **Copy the output to the host** (the witness copies included; they are private files on the
   dev machine and are git-ignored):

   ```powershell
   New-Item -ItemType Directory -Force snapshots | Out-Null
   docker cp hyphae-consensus:/hyphae-home/snapshots/<id> snapshots/
   ```

5. **Build and check the manifest** (standard-library Python, always with `-I`; PowerShell, because
   the `--verifier-dir` argument is a container path that Git Bash would rewrite):

   ```powershell
   python -I scripts/snapshot_manifest.py build snapshots/<id> --expect-ids research-ids.txt
   python -I scripts/snapshot_manifest.py check snapshots/<id> --reverify scripts\hyphae-docker.cmd --verifier-dir /hyphae-home/snapshots/<id>
   ```

   `build` refuses any capture file whose sha256 differs from the digest the session recorded in
   `captures.index` the moment the file was written (section 6: the only binding of a proof, a
   witness or a report to its capture once the directory has left the container), and requires
   the two required captures to carry different proofs. `--reverify` runs `hyphae proof verify`
   again on every capture, exactly as the runtime does (`hyphae proof verify --proof F --witness F
   --anchor HEX`, Meta key variables stripped from the child's environment), and requires the
   fresh report to equal the captured `captures/<id>.verify.json` field by field; on the dev
   machine the verifier is the container wrapper, which sees the captures where the session left
   them, hence `--verifier-dir`. A report the verifier does not reproduce means the proof or
   witness in hand is not the pair verified at capture time.

   `build` also copies the capture-time digests of each response and verifier report from
   `captures.index` into the manifest (`response_sha256`, `verify_sha256`, section 4): the
   runtime never reads `captures.index`, so these are what bind the record texts at runtime.

   **The manifest digest.** Once `check` passes after the last `build`, `manifest.json` is
   final: record its SHA-256 (`sha256sum snapshots/<id>/manifest.json` in Git Bash, or
   `(Get-FileHash snapshots\<id>\manifest.json -Algorithm SHA256).Hash` in PowerShell, whose
   uppercase hex the runtime accepts). It equals the `manifest.json` line of `SHA256SUMS`, it is
   the value of `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` on the server (step 7), and it is published
   with the anchor and the binary digest (step 8). Any later `build` or edit changes it.

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
   $env:CONSENSUS_SNAPSHOT_MANIFEST_SHA256 = (Get-FileHash snapshots\<id>\manifest.json -Algorithm SHA256).Hash
   ```

   The manifest pin is optional here and harmless to set. `CONSENSUS_HYPHAE_BIN_SHA256` stays
   unset on the dev machine: the wrapper is not the Hyphae binary (its digest is a script's),
   and the startup line then says `verifier binary not pinned`.

7. **Server (Linux).** Copy the whole directory to `/var/lib/consensus/snapshots/<id>`
   (runbook 3.3: `scp -r` to `/tmp`, `sha256sum -c SHA256SUMS`, then moved into place), owned by
   the service user, the tree closed to everyone else: the witnesses are private. Install the
   native Linux `hyphae` 4.0.0 whose
   `sha256sum` equals `manifest.hyphae.binary_sha256` (the tarball digest is matched against the
   GitHub release asset; the binary digest is what the manifest records). Configuration
   (runbook 3.2, 3.3 and 4.1 give the server's paths):
   `CONSENSUS_SNAPSHOT_DIR=/var/lib/consensus/snapshots/<id>`,
   `CONSENSUS_HYPHAE_EXPECTED_ANCHOR=<manifest.anchor_hex>`,
   `CONSENSUS_SNAPSHOT_MANIFEST_SHA256=<the manifest digest of step 5>`,
   `CONSENSUS_HYPHAE_BIN=/opt/hyphae/bin/hyphae` (an absolute path; the symbolic link is
   followed when the binary is hashed, as `exec` follows it),
   `CONSENSUS_HYPHAE_BIN_SHA256=<sha256sum of the installed binary, equal to manifest.hyphae.binary_sha256>`,
   `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS` as today; `CONSENSUS_SNAPSHOT_VERIFIER_DIR` unset (the
   native binary sees the same path). Both pins are required on the server's unix socket with
   live mode enabled (section 7, "Pins"). No daemon, no credentials, no writer path on the server
   (release checklist G9). If Python 3 is on the server, `python3 -I scripts/snapshot_manifest.py
   check /var/lib/consensus/snapshots/<id> --reverify /opt/hyphae/bin/hyphae` is the
   deployment review step (no `--verifier-dir`: the native binary sees the directory itself).

8. **What gets committed.** `git add snapshots/<id>`: the `.proof` and `.witness` files are
   already ignored (`.gitignore`), everything else (`manifest.json`, `SHA256SUMS`, the
   `.response.json` and `.verify.json` files, `captures.index`, `session.json`, `capture.log`,
   `hyphae-version.txt`, `hyphae-binary.sha256`) may be committed. Confirm with `git status
   --short snapshots/` and `git check-ignore -v snapshots/<id>/captures/*.proof`. The committed
   subset must pass `python -I scripts/snapshot_manifest.py check snapshots/<id> --public-only`.
   Git stores and checks out `snapshots/` byte for byte (`.gitattributes`: `-text`, as for
   `research/`): with `core.autocrlf=true` a checkout would otherwise rewrite the line endings
   of `manifest.json`, `SHA256SUMS` and the responses and break every digest above.
   The anchor, the manifest digest (step 5) and the binary digest are additionally published
   out of band: the README and the release tag carry all three, as ADR-0008 decides, and they
   are the values the server pins (step 7). The public live mode is pinned to the
   **snapshot's** anchor, which is a new value: it is not the demo store's `d91ac10d…` anchor of
   the recorded batteries.

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
`SHA256SUMS`, and `captures.index` is required by `check` because it holds the capture-time
digests (the runtime does not read it). `captures/<id>.response.json` is the exact stdout of `hyphae agent ui`: if that
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
     "response_sha256": "<64 hex>", "verify_sha256": "<64 hex>",              // the response and the verifier report, as captures.index recorded them
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
`response_sha256`, `verify_sha256`, `memory_ids`, `verification`); `arguments` has exactly
`project`, `query`, `limit`, `prove`; `verification` has exactly `verified_at`, `status`,
`kind`, `scope`, `anchor_hex`. The manifest is written only by `scripts/snapshot_manifest.py
build` (`json.dump`, indent 2, ASCII, trailing newline); `check` enforces the schema, the
patterns, the key order, the two required captures, `memory_ids` equal to the response ids,
`store` equal to the list response's `snapshot`, one anchor everywhere, and recomputes every
digest.

The four file digests of a capture are all required (a manifest without `response_sha256` or
`verify_sha256` is refused by `check` and by the runtime, `manifest_invalid`; no manifest
exists without them). `response_sha256` and `verify_sha256` are the digests the session
recorded in `captures.index` (section 5), copied by `build`, which refuses a file that differs
from them. They are the binding of the record texts: the runtime reads neither
`captures.index` nor `capture.log`, and checks each response and report against these
digests, on the very bytes it then parses, before parsing it (section 7).

## 5. Session files written in the container (input of the manifest builder)

`captures.index`: one JSON object per line, one line per capture, in capture order:

```json
{"id":"list-demo-repo","operation":"list","arguments":{"project":"demo-repo","query":"","limit":64,"prove":true},"files":{"response":"captures/list-demo-repo.response.json","proof":"captures/list-demo-repo.proof","witness":"captures/list-demo-repo.witness","verify":"captures/list-demo-repo.verify.json"},"sha256":{"response":"<hex>","proof":"<hex>","witness":"<hex>","verify":"<hex>"},"proof_blake3":"<hex>","anchor_hex":"<hex>","memory_ids":["<decimal id>","..."],"captured_at":"<RFC 3339 UTC>","verified_at":"<RFC 3339 UTC>"}
```

`arguments` is the exact object sent to `hyphae agent ui`; `sha256` holds the digests of the
four capture files computed in the container the moment each was written (the response as
`agent ui` printed it; the proof and witness copies right after `cmp` confirmed them equal to the
files the response names; the verifier report as `proof verify` printed it), also printed in
`capture.log`; `memory_ids` are the ids the script extracted from the response, in order;
`captured_at` is the time the request was sent and `verified_at` the time `hyphae proof verify`
was started (both required: the manifest's `verification.verified_at` is this value, never a
substitute). The builder trusts none of it: every value is compared with the response file and
the verifier report, every file's sha256 must equal the recorded one, and a mismatch aborts.
The four recorded digests then become the manifest's `response_sha256`, `proof_sha256`,
`witness_sha256` and `verify_sha256` (section 4), which is how they reach the runtime.

`session.json`: `{"schema":"hyphae-consensus-capture-session-v1","snapshot_id","project",
"captured_at" (session start),"finished_at","anchor_hex","record_count","hyphae_version",
"binary_path","binary_sha256","control_schema","ui_mode","instance_home"}`. The builder takes
`captured_at`, `project`, `hyphae_version` and `binary_sha256` from it (falling back to
`hyphae-version.txt` and `hyphae-binary.sha256`) and ignores the rest.

`capture.log`: the complete session transcript with UTC timestamps, including the help texts
recorded at the start, every `+ <command>` line, every store response, every memory id, the
anchor, the verifier fields, the capture-time sha256 of every capture file, the names and sizes
of the files `setup` and `configure` created and of the credential files that were removed, and
the list of output files. It contains paths inside the container and no key material (the
`agent setup` output is discarded with the scratch directory, see section 3 step 1).

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
`SHA256SUMS`), nor have the response files the record texts are read from or the verifier
reports (their digests in the manifest, below); the empty `other-repo` recall is a proven
absence at the same anchor.

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

**What binds which file to which capture.** Every proof of one session carries the same anchor
and verifies against the same witness contents, so the verifier alone cannot tell whether
`captures/list-demo-repo.proof` is the proof of the list capture or of the isolation capture.
The bindings, and who checks them:

| Binding | Established by | Checked by |
|---|---|---|
| response ↔ proof and witness copies | the session: the copies are made from the paths the response names, compared with `cmp`, and the sha256 of the response, the proof copy and the witness copy is recorded at once in `captures.index` and `capture.log` | `build` and `check`: every file's sha256 must equal the recorded one (a swapped pair, a swapped report or an altered file is refused even though it would verify); the two required captures must carry different proofs (`proof_sha256`, `proof_blake3`, and the report's `proof_digest`/`request_digest`/`result_digest` when present) |
| every capture file (response, proof, witness, report) ↔ manifest | `build`: `response_sha256`, `proof_sha256`, `witness_sha256`, `verify_sha256`, equal to the recorded digests | `check`, and the runtime at load on the very bytes it parses (`digest_mismatch`), then again for the two served captures at every status read and admission (section 7); the runtime never reads `captures.index`, so for the record texts this is the binding |
| manifest ↔ published digest | the owner: the manifest digest published in the README and the release tag (section 3, steps 5 and 8) | the runtime at load, before parsing the manifest, when `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` is set (`manifest_pin_mismatch`); unpinned, nothing |
| verifier binary ↔ capture build | the session: `hyphae.binary_sha256` (the sha256 of the binary that produced and verified the captures); the owner publishes it | the runtime at boot, when `CONSENSUS_HYPHAE_BIN_SHA256` is set: the file `CONSENSUS_HYPHAE_BIN` names must hash to the pin (`verifier_binary_mismatch`) and the pin must be the manifest's (`capture_binary_mismatch`); unpinned, nothing |
| proof and witness ↔ verifier report | `hyphae proof verify` at capture time; the report's `proof_digest`, `witness_digest`, `request_digest`, `result_digest` are deterministic functions of the two files | `check --reverify` (the fresh report must equal the captured one field by field) and the runtime's boot check (section 7) |
| proof ↔ anchor | the proof itself; `anchor_digest` in the report | the verifier, every time |
| `proof_blake3` ↔ proof bytes | Hyphae's response | nobody here: no BLAKE3 implementation is available (section 2); the value is carried as an opaque identifier and compared only with the response it came from |

What stays beyond offline checking: without the manifest pin, a rewrite of the whole directory
that keeps every file consistent with every other (`manifest.json` and `SHA256SUMS` included)
loads, and can only be caught by the shape checks (the list capture holds exactly 7 `demo-repo`
records, the isolation recall is empty, one anchor) and by a manual comparison with the
digests published out of band and the committed `manifest.json`/`SHA256SUMS` in git history.
With `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` set to the published digest, the runtime refuses it
at load (`manifest_pin_mismatch`): the manifest holds the digest of every capture file, so every
byte served is then bound to the published value.

**What each check catches, and what it does not.**

| Check (when) | Catches | Does not catch |
|---|---|---|
| `SHA256SUMS` (load) | a file altered, added or removed with the listing left as it was | anything regenerated together with the listing (it lives in the directory) |
| the manifest's four file digests (load; the two served captures again at every status read and admission) | a proof, witness, record text or verifier report altered with `SHA256SUMS` regenerated, a file swapped between captures, a byte changed after boot | a rewrite of `manifest.json` to match, unless the manifest is pinned |
| `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` (load, before parsing) | any other manifest, so any other byte of any capture file: a snapshot that is not the published one | a wrong value pinned (it binds to whatever is pinned: a manifest pinned to its own rewrite loads); a manifest changed after boot is caught by the re-check above, not by the pin |
| pinned anchor `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` (boot, every run) | artifacts of another store | another capture of the same session (every proof shares the anchor) |
| boot verification and `BootBoundVerifier` (boot, every run) | a proof or witness that does not verify, or that is not the capture-time pair; a served pair swapped after boot | a lying verifier: the verifier is trusted, hence the binary pin |
| `CONSENSUS_HYPHAE_BIN_SHA256` (boot, before any verifier run) | a replaced binary or a wrapper (one that prints the capture-time reports included: it never runs), and a snapshot captured with another Hyphae build | a binary replaced after boot (hashed once, until the next start), the time between the hash and each run, a wrong value pinned, the dev machine's container wrapper (not pinnable: unset there) |
| none | | whether the texts are byte-identical to what the proofs seal (HY-1: not byte-compared); the capture session, its script and the published digests are trusted, so a text that was wrong when captured is served as captured |

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
- **Boot check.** At start, with public live mode enabled (with it disabled the snapshot is not
  read at all), the runtime checks `SHA256SUMS`, reads `manifest.json` (refusing it before
  parsing when its SHA-256 is not the pinned `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`,
  `manifest_pin_mismatch`), checks every capture file against the manifest's digests before
  parsing a response or a report (`digest_mismatch`: an edited record text is refused here),
  requires the manifest's project to be the presets' `demo-repo` (`project_mismatch`), checks
  the pinned verifier binary ("Pins" below: `verifier_binary_mismatch`,
  `capture_binary_mismatch`), requires `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` (mandatory) to equal
  `manifest.anchor_hex` (`pinned_anchor_mismatch`), all before any verifier run, and runs
  `hyphae proof verify` on both
  required captures with the verifier's own view of the directory
  (`CONSENSUS_SNAPSHOT_VERIFIER_DIR` when set, else `CONSENSUS_SNAPSHOT_DIR`), requiring the same
  report fields as section 6 and the `proof_digest` and `result_digest` of the captured
  `captures/<id>.verify.json` (its digests are deterministic, section 6), which binds the proof
  and witness in hand to the pair verified at capture time; `proof_blake3` cannot be recomputed
  (no BLAKE3 in the workspace) and is an opaque value from the response. A snapshot that fails
  any of this does not stop the server: live mode is unavailable with the stable code (see
  "Status" below). Per run the runtime re-verifies (cached by content digest within the run,
  HY-13), stamps the verification time, and lets a `Valid` stand only when the report's
  `proof_digest` and `result_digest` are the ones boot accepted for the record's `proof_ref`
  (`BootBoundVerifier`): every proof of a session shares the anchor, so the verifier alone
  would accept the isolation capture's pair copied over the list capture's.
- **Pins.** Two variables, 64 hex digits each (case and surrounding whitespace are
  normalized; anything else is a configuration error naming the variable, and live-enabled
  startup stops on it as on a malformed anchor). With live mode enabled on the unix socket
  (the deployment) both are required: a missing one stops `serve-public` naming it
  (`server::require_snapshot_pins`); a development TCP bind may leave them unset, and its
  startup line says what is not pinned. Production sets both to the values published with the
  snapshot (section 3, step 8).
  `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` pins the SHA-256 of `manifest.json`: a manifest with
  another digest is refused at load, before it is parsed (`manifest_pin_mismatch`), after the
  `SHA256SUMS` check as before (a manifest altered with the listing left alone is still
  `checksums_mismatch`). `CONSENSUS_HYPHAE_BIN_SHA256` pins the verifier binary: with it,
  `CONSENSUS_HYPHAE_BIN` must be an absolute path (configuration error otherwise: a name looked
  up on `PATH` could run another file than the one hashed); at boot, before any verifier run,
  that file (symbolic links followed, as `exec` follows them; a regular file of at most 1 GiB)
  must hash to the pin (`verifier_binary_mismatch`: missing, unreadable, not a regular file, or
  another build or a wrapper, which then never runs), and the pin must be the manifest's
  `hyphae.binary_sha256` (`capture_binary_mismatch`: the snapshot was captured with another
  Hyphae build). Each of these refusals makes live mode unavailable like the other boot
  failures. The binary is hashed at boot only: one replaced while the server runs is not
  detected until the next start. Unset, nothing is compared, and the startup line says so
  (`manifest not pinned`, `verifier binary not pinned`).
- **Configuration.** `CONSENSUS_SNAPSHOT_DIR` = path of the snapshot directory (`manifest.json`
  inside); `CONSENSUS_SNAPSHOT_VERIFIER_DIR` (optional) = the same directory as the verifier
  binary sees it (Windows dev machine: the container path, because `CONSENSUS_HYPHAE_BIN` is
  `scripts/hyphae-docker.cmd`; Linux server: unset); `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` mandatory
  and equal to the manifest anchor: with live mode enabled and the directory set, a missing or
  malformed anchor is a configuration error naming the variable and startup aborts, and so does
  live mode enabled without `CONSENSUS_SNAPSHOT_DIR` (`SnapshotConfig::try_from_env` /
  `from_vars` are the only readers; none folds that error into "snapshot mode not configured");
  with live mode disabled none of these is read (a set `CONSENSUS_SNAPSHOT_DIR` earns one
  `[snapshot]` notice); `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` and `CONSENSUS_HYPHAE_BIN_SHA256`
  as in "Pins" above; `CONSENSUS_HYPHAE_BIN` (an absolute path when its digest is pinned) and
  `CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS` as today
  (`crates/consensus-api/src/config.rs`, `hyphae.rs`). The directory is read without following
  symbolic links (a link anywhere under it, or a directory nested deeper than `captures/`, is
  refused with `symlink_refused` / `layout_too_deep`).
- **Status: `/health` versus `/ready` and the log.** `/api/v1/health` stays static facts
  (version, schema, policy, contract digest, executor). The evidence state lives in three
  places. The server log carries the startup line of a snapshot that booted (snapshot id,
  project, record count, anchor prefix, `manifest sha256 pinned <12 hex>…` or `manifest not
  pinned (CONSENSUS_SNAPSHOT_MANIFEST_SHA256 unset)`, `verifier binary sha256 pinned <12 hex>…`
  or `verifier binary not pinned (CONSENSUS_HYPHAE_BIN_SHA256 unset)`, capture and verification
  times; digest prefixes only, no path) or, for one that did
  not, `[snapshot] live evidence unavailable: <code>` (`snapshot_dir_missing`,
  `checksums_mismatch`, `digest_mismatch`, `manifest_pin_mismatch`, `pinned_anchor_mismatch`,
  `verifier_binary_mismatch`, `capture_binary_mismatch`, `boot_verification_failed`,
  `project_mismatch`, ...), and the same line, or `[snapshot] live evidence ok again`, on every
  later change between ok and not ok. `/api/v1/ready` carries the boolean: `ready: false` with
  `live.evidence_ok: false` while the evidence is not ok. `/api/v1/capabilities` then reports
  `live_enabled: false` without a live section, so the web app shows live mode as disabled; live
  admissions are `503 live_unavailable`, while guided mode, the showcase and the static site keep
  serving. After boot the status re-hashes `manifest.json` and every file of the two served
  captures (response, proof, witness, capture-time report) against the digests the boot check
  accepted (`SnapshotMemoryClient::check_artifacts`), at most once per two seconds
  (`READY_CACHE_TTL`) for the routes and in full at every admission, so a file deleted,
  truncated or replaced after boot (an edited record text included) makes live mode unavailable
  at once (`file_unreadable`, `digest_mismatch`, `file_too_large`, `symlink_refused`) and
  restoring it makes it ok again. The records themselves are served from memory as boot read
  them. The verifier binary is not part of this re-check (see "Pins"). No public response
  carries the code, a filesystem path, witness contents or container details (G9: no witness
  or container path in
  any public response); the snapshot errors name files relative to the snapshot directory, and
  only the error of the strict entry point `SnapshotFactory::from_env`, which `serve-public`
  does not use, quotes the verifier's own message, which may name the paths it was given.
- **Wiring.** `SnapshotFactory` (`crates/consensus-api/src/live/snapshot_factory.rs`) is the
  public live gate's evidence factory: with live mode enabled `serve-public` boots the client
  from `CONSENSUS_SNAPSHOT_DIR` through `SnapshotFactory::live_evidence_from_env` before it
  listens (a snapshot that does not boot becomes `UnavailableEvidence`, which builds nothing),
  refuses admissions before Turnstile while the status is not ok, re-checks the files in full
  at every admission, and hands each admitted run a copy of the client with a fresh, pinned
  verifier bound to the boot check (ADR-0008, "Wiring").

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
sizes with `find` and `stat` before removing the directory), discards `agent setup`'s stdout
with its scratch directory at exit (success or failure; only the byte count is logged), and
never carries the Meta key (none is set in the container). What the sealed HOME keeps after the
session: the read-only data directory, the original proof and witness files, the daemon log
`serve.log`, and whatever `agent setup`/`agent configure` wrote under it (listed in
`capture.log`); no credential file and no copy of the setup output.

## 9. Unverified until the capture session (2026-10-29)

Each item is guarded by the script (it aborts with a precise message) or by the builder; none is
assumed silently.

1. Whether `hyphae agent ui` needs the daemon or opens the store itself (section 2); default
   `--ui-via-daemon`, alternative `--ui-direct`.
2. Whether `agent ui` prints the result object itself or an envelope with `result`, and whether
   it prints compact or pretty-printed JSON; both shapes are handled (the tokenizer was exercised
   under dash + mawk 1.3.4 in a throwaway debian:trixie-slim container on compact and
   pretty-printed samples, including the live `proof verify` report shape of 2026-10-27, and on
   strings holding a raw newline, a raw tab or a NUL byte, which JSON forbids and the tokenizer
   refuses as a hard failure instead of splitting the leaf; `--selftest` repeats these checks,
   and `--inspect` on such a file exits 4 saying so).
3. Whether `agent ui` accepts the request as given (`schema`, `id`, `operation`, `arguments`
   with `project`, `query`, `limit`, `prove`), honours `prove` for `list`, and returns `proof`
   with `scope = memory-snapshot-v1`, `proof_path`, `witness_path`, `proof_blake3` (64 hex) and
   `anchor_hex` (64 hex) exactly as the MCP recall response does; `query = ""` for `list`.
4. Whether `hyphae agent setup --no-service` prints credential material on stdout (discarded
   either way, section 3 step 1), what `hyphae agent configure opencode` writes under the new
   HOME (listed by the script; it must not change the data directory), and whether `hyphae agent
   remove` would be a cleaner credential destruction than removing
   `<HOME>/.config/hyphae/credentials` (the layout `hyphae-env.sh` documents; the script asserts
   the directory exists before and is gone after).
5. Whether the ids of the 7 re-seeded records equal the recorded research ids (section 3 step 5;
   `--expect-ids`).
6. The proof and witness sizes of a fresh 7-record store (expected about 3.3 MB for the witness)
   and the time `hyphae proof verify` takes on the server.
7. Whether `proof_blake3` is the BLAKE3 of the proof file's bytes (it cannot be recomputed here,
   section 2) and whether the verifier report is reproduced field by field by a second run on
   the same files (expected: its digests are deterministic; `check --reverify` and the runtime's
   boot check depend on it, and the 2026-10-27 live report had no time-dependent field).
8. Whether the sealed instance's anchor differs from the demo store's `d91ac10d…` (expected: a
   fresh store with its own catalog and key verifiers; the script warns if they are equal).
9. The receipt wording is lane B's implementation; section 7 is the contract it is written
   against (the evidence status itself is implemented: section 7, "Status").
