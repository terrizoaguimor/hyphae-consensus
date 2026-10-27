# Hyphae (Linux) in Docker — operator runbook

Why: the pinned Windows 4.0.0 build's Agent Memory surface
(`hyphae agent *`, `hyphae mcp --profile memory`) exits with
`native product request is invalid` on this host, while the Linux build
works. This runbook runs the pinned Linux binary in a local container and
points the Consensus adapter at it. No Hyphae code is modified.

## Two rules that make proofs work (learned 2026-10-27)

`recall(prove=true)` answered every request with
`{category: "corruption", message: "native durable state is invalid",
retry: "after-recovery"}` for two different reasons. Neither was data
corruption: `hyphae agent doctor` reported the store healthy. Hyphae maps
every proof-generation failure, and an oversized response, to that generic
`corruption` code. Source reading at the pinned commit plus experiments on
restored copies found:

1. **The daemon socket must live outside the data directory.** Proof
   generation bundles the whole data directory into the witness and rejects
   special files (only `LOCK` is skipped). `hyphae serve --data-dir D`
   without `--endpoint` puts its socket at `D/hyphae.sock`, which is what the
   operate hint printed by `agent setup` shows. The systemd unit that
   `agent setup` writes passes `--endpoint
   ~/.config/hyphae/runtime/hyphae-memory-<digest>.sock` instead;
   `serve.sh` does the same (`agent recover`, then `serve`).
2. **The whole data directory must fit in one MCP response.** The proof
   carries the complete directory witness inline, and `hyphae mcp` caps a
   response at 4 MiB. Once the data directory approaches 3.9 MB (one copy
   failed there, another had just passed), the client rejects the response
   and reports `corruption` again. Each stored memory added 160–180 KB (WAL
   plus pages) in a fresh store; each checkpoint adds pages too, and a
   proven recall at a new commit checkpoints. The available roles cannot
   `vacuum`. So: keep each store small and avoid needless writes.
   `hyphae_store_bytes` (in `hyphae-env.sh`) prints the current size. A
   fresh `agent setup` is about 2.0 MB; the demo store with its 7 records
   is about 3.3 MB.

## Stores in the container (2026-10-27)

| Instance | `HYPHAE_HOME_DIR` | Contents |
|---|---|---|
| Demo store | `/hyphae-home` (default) | `demo-repo`: `seed.sh` (2 records) + `seed-demo-facts.sh` (5 records) |
| Hostile-note lab | `/hyphae-home/lab-injection` | `injection-lab`: `seed-injection-lab.sh` (3 records, one hostile; the hostile one was forgotten after the test) |

Each instance has its own data directory, credentials, daemon, and socket.
The previous store (grown past the size limit) is preserved, not served,
under `/hyphae-home/replaced-2026-10-27/`; the pre-recovery tarball and
logs are in `/hyphae-home/recovery-2026-10-27/`. Memory ids are derived
from project + layer + text, so re-seeding identical text reproduces the
same ids.

## Setup (already done on the dev machine)

```powershell
# Verified download (host): hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz
# sha256 must match the GitHub release asset digest (release-v4.0.0-crates).
docker pull debian:trixie-slim   # bookworm glibc 2.36 is too old; binary needs 2.39
docker volume create hyphae-home-vol
docker run -d --name hyphae-consensus -e HOME=/hyphae-home `
  -v hyphae-home-vol:/hyphae-home -v "<workspace>\tools:/opt/dist:ro" `
  -p 127.0.0.1:8791:8790 debian:trixie-slim sleep infinity
docker exec hyphae-consensus sh -c "mkdir -p /opt/hyphae && tar -xzf /opt/dist/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz -C /opt/hyphae && ln -sf /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae /usr/local/bin/hyphae"
# Copy the scripts in (all of them source /hyphae-home/hyphae-env.sh):
foreach ($f in Get-ChildItem scripts\hyphae-container\*.sh) { docker cp $f.FullName "hyphae-consensus:/hyphae-home/$($f.Name)" }
# Demo store:
docker exec hyphae-consensus hyphae agent setup --no-service
docker exec hyphae-consensus sh /hyphae-home/serve.sh
docker exec hyphae-consensus sh /hyphae-home/seed.sh
docker exec hyphae-consensus sh /hyphae-home/seed-demo-facts.sh
# Hostile-note lab (separate instance):
docker exec hyphae-consensus sh -c "mkdir -p /hyphae-home/lab-injection && HOME=/hyphae-home/lab-injection hyphae agent setup --no-service"
docker exec -e HYPHAE_HOME_DIR=/hyphae-home/lab-injection hyphae-consensus sh /hyphae-home/serve.sh
docker exec -e HYPHAE_HOME_DIR=/hyphae-home/lab-injection hyphae-consensus sh /hyphae-home/seed-injection-lab.sh
```

Notes:
- The container's PID 1 is `sleep infinity`, which never reaps children:
  a stopped daemon lingers as a zombie, harmlessly.
- Stop a daemon with SIGINT (`kill -INT <pid>`); it removes its socket.
- The MCP adapter allows ONE active tool call; `hyphae_call` opens one
  session per call.
- Proof artifacts accumulate under `<HOME>/.local/state/hyphae/proofs/`
  (each witness is a full copy of the data directory, ~3 MB); prune old
  ones when no receipt under review references them.

## Probe, verify, forget

```powershell
docker exec hyphae-consensus sh /hyphae-home/recall.sh demo-repo "retries attempts" true
# Verify a returned proof offline (paths and anchor from the response):
docker exec hyphae-consensus hyphae proof verify --proof <proof_path> --witness <witness_path> --anchor <anchor_hex>
# Forget one memory (writer credential; idempotent):
docker exec -e HYPHAE_HOME_DIR=/hyphae-home/lab-injection hyphae-consensus sh /hyphae-home/forget.sh injection-lab <id>
```

## Console environment (host)

```powershell
. .\scripts\hyphae-container\live-env.ps1                              # demo store
. .\scripts\hyphae-container\live-env.ps1 /hyphae-home/lab-injection   # lab store
cargo run -p consensus-api -- serve
$env:CONSENSUS_LIVE_TESTS = "1"
cargo test -p consensus-api --test hyphae_adapter live_ -- --ignored --nocapture
```

`scripts/hyphae-docker.cmd` forwards every invocation (MCP stdio and
`proof verify`) into the container via `docker exec -i`, so every path the
adapter passes is a container path, consistently. Reader keys never leave
the container volume; the host only holds their path strings.

From Git Bash, set `MSYS_NO_PATHCONV=1` for `docker exec` / `docker cp`, and
set container paths in environment variables from PowerShell only: MSYS
rewrites values such as `/hyphae-home/...` into Windows paths.
