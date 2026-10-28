# deploy/ — templates and scripts for the public portal

The runbook is `docs/deployment/runbook.md`. Read it first: it is the "how" of
the release gates G1, G3 (edge rules), G4 (credential file), G6 (edge settings),
G9, G10 (geo rules) and G11 of `docs/security/release-checklist.md`, and every
file here is referenced from a numbered section of it. Nothing in this
directory holds a secret or a real hostname: every placeholder is written in
angle brackets (`<hostname>`, `<TUNNEL-UUID>`, `<snapshot-id>`, ...).

| Path | Installed as | Runbook |
|---|---|---|
| `systemd/consensus-public.service` | `/etc/systemd/system/consensus-public.service` | 5.1 |
| `systemd/consensus-operator.service` | `/etc/systemd/system/consensus-operator.service` (never enabled; no `[Install]`) | 5.2 |
| `systemd/cloudflared.service.d/10-consensus.conf` | `/etc/systemd/system/cloudflared.service.d/10-consensus.conf` | 6.4 |
| `cloudflared/config.yml.example` | `/etc/cloudflared/config.yml` | 6.3 |
| `etc/consensus/public.env.example` | `/etc/consensus/public.env` | 4.1 |
| `etc/consensus/operator.env.example` | `/etc/consensus/operator.env` | 4.1 |
| `scripts/install.sh` | run once as root on the server | 2–5 |
| `scripts/verify-edge.sh` | gate G6: origin (socket) vs edge, byte for byte; `cf-cache-status` | 8.4 |
| `scripts/sse-heartbeat-test.sh` | SSE heartbeat and resume test through the tunnel | 8.5 |
| `scripts/canary-scan.sh` | gate G4: the key is in no environment, file, log or response | 8.3 |
| `scripts/kill-switch.sh` | live-mode kill switch on / off / status | 9.3 |
| `scripts/export-ledger.sh` | checksummed export of the ledger (and optionally the runs) | 9.5, 9.7 |

Conventions: every script is bash with `set -euo pipefail`; destructive steps
ask for confirmation unless `--yes` is given; scripts print locations and
counts, never secrets. Validation that works on any machine:

```sh
for f in deploy/scripts/*.sh; do bash -n "$f" && echo "syntax ok: $f"; done
grep -rhoE 'CONSENSUS_[A-Z0-9_]+' deploy docs/deployment | sort -u   # cross-check against the variable tables
```

`systemd-analyze verify` is the server-side check of the unit files
(`systemd-analyze verify /etc/systemd/system/consensus-public.service`); the
runbook's appendix A lists the directives and the man pages they were checked
against.
