# Hyphae (Linux) in Docker — operator runbook

Why: the pinned Windows 4.0.0 build's Agent Memory surface
(`hyphae agent *`, `hyphae mcp --profile memory`) exits with
`native product request is invalid` on this host, while the Linux build
works. This runbook runs the pinned Linux binary in a local container and
points the Consensus adapter at it. No Hyphae code is modified.

## One-time setup (already done on the dev machine 2026-10-26)

```powershell
# Verified download (host): hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz
# sha256 must match the GitHub release asset digest (release-v4.0.0-crates).
docker pull debian:trixie-slim   # bookworm glibc 2.36 is too old; binary needs 2.39
docker volume create hyphae-home-vol
docker run -d --name hyphae-consensus -e HOME=/hyphae-home `
  -v hyphae-home-vol:/hyphae-home -v "<workspace>\tools:/opt/dist:ro" `
  -p 127.0.0.1:8791:8790 debian:trixie-slim sleep infinity
docker exec hyphae-consensus sh -c "mkdir -p /opt/hyphae && tar -xzf /opt/dist/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz -C /opt/hyphae && ln -sf /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae /usr/local/bin/hyphae"
docker exec hyphae-consensus hyphae agent setup --no-service
docker exec hyphae-consensus sh -c "nohup hyphae serve --data-dir /hyphae-home/.local/share/hyphae/agent-memory --native-api-key-auth > /hyphae-home/serve.log 2>&1 &"
# Seed demo memories (writer credential, sequential calls):
docker cp scripts/hyphae-container/seed.sh  hyphae-consensus:/hyphae-home/seed.sh
docker cp scripts/hyphae-container/seed2.sh hyphae-consensus:/hyphae-home/seed2.sh
docker exec hyphae-consensus sh /hyphae-home/seed.sh
docker exec hyphae-consensus sh /hyphae-home/seed2.sh
```

Notes learned the hard way:
- Serve must use the canonical command (local UDS next to the data dir).
  Adding `--http-bind 0.0.0.0:...` made startup fail in this setup.
- Use a named Docker volume for HOME, not a Windows bind mount.
- The MCP adapter allows ONE active tool call; drivers must be sequential.
- `recall(prove=true)` currently fails upstream with
  `corruption: native durable state is invalid` even on Linux/ext4 while
  plain recall works. The Consensus adapter retries without `prove` and
  reports integrity `unavailable` — it never fabricates verification.

## Console environment (host)

```powershell
$env:META_API_KEY = [Environment]::GetEnvironmentVariable("META_API_KEY","User")
$env:CONSENSUS_HYPHAE_BIN = "$PWD\scripts\hyphae-docker.cmd"
$env:HYPHAE_NATIVE_ENDPOINT = "/hyphae-home/.local/share/hyphae/agent-memory/hyphae.sock"
$env:HYPHAE_NATIVE_API_KEY_FILE = "/hyphae-home/.config/hyphae/credentials/memory-reader.key"
cargo run -p consensus-api -- serve
```

`scripts/hyphae-docker.cmd` forwards every invocation (MCP stdio and
`proof verify`) into the container via `docker exec -i`, so every path the
adapter passes is a container path, consistently. The reader key never
leaves the container volume; the host only holds its path string.
