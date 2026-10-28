# deploy/ — templates and scripts for the public portal

The runbook is `docs/deployment/runbook.md`. Read it first: it is the "how" of
the release gates G1, G3 (edge rules), G4 (credential file), G6 (edge settings),
G9, G10 (geo rules) and G11 of `docs/security/release-checklist.md`, and every
file here is referenced from a numbered section of it. Nothing in this
directory holds a secret or a real hostname: every placeholder is written in
angle brackets (`<hostname>`, `<TUNNEL-UUID>`, `<snapshot-id>`, ...).

Where a script runs matters: the server's egress policy (runbook 2.4) does not
reach Cloudflare's HTTP edge, so anything that fetches `https://<hostname>` runs
on the owner machine, and anything that reads `/proc`, the socket or the
ledger runs on the server.

| Path | Installed as | Runs on | Runbook |
|---|---|---|---|
| `systemd/consensus-public.service` | `/etc/systemd/system/consensus-public.service` | | 5.1 |
| `systemd/consensus-operator.service` | `/etc/systemd/system/consensus-operator.service` (never enabled; no `[Install]`) | | 5.2 |
| `systemd/cloudflared.service.d/10-consensus.conf` | `/etc/systemd/system/cloudflared.service.d/10-consensus.conf` | | 6.4 |
| `tmpfiles.d/consensus.conf` | `/etc/tmpfiles.d/consensus.conf` (`/run/consensus` as `2750 consensus:consensus-sock`) | | 2.3, 5.1 |
| `cloudflared/config.yml.example` | `/etc/cloudflared/config.yml` | | 6.3 |
| `etc/consensus/public.env.example` | `/etc/consensus/public.env` | | 4.1 |
| `etc/consensus/operator.env.example` | `/etc/consensus/operator.env` | | 4.1 |
| `scripts/install.sh` | run once as root | server | 2–5 |
| `scripts/verify-edge.sh` | gate G6: origin (socket, over SSH) vs edge, byte for byte; `cf-cache-status` | owner machine | 8.4 |
| `scripts/sse-heartbeat-test.sh` | SSE heartbeat and resume test through the tunnel | owner machine | 8.5 |
| `scripts/canary-scan.sh` | gate G4: environments, files, journal (server); public responses (`--responses-only`, owner machine) | both | 8.3 |
| `scripts/sandbox-probe.sh` | gate G9: `hyphae proof verify` inside the unit's own sandbox via `systemd-run` | server | 8.6 |
| `scripts/kill-switch.sh` | live-mode kill switch on / off / status (removing the file clears no trip) | server | 9.3 |
| `scripts/ledger.sh` | the operator's `consensus-api ledger` subcommand (status, reset a trip, release a reservation a crash left open) as the unit runs it | server | 4.4, 8.8, 9.3 |
| `scripts/judge-codes.sh` | judge-lane invite codes: generates them (140 random bits each), prints them once, installs only their SHA-256 digests (`root:consensus 0640`); never overwrites a digests file without `--force` | server | 1, 4.3, 9.8 |
| `scripts/export-ledger.sh` | checksummed export of a ledger directory (and optionally the runs) | server | 4.5, 9.5, 9.7 |

Conventions: every script is bash with `set -euo pipefail`; destructive steps
ask for confirmation unless `--yes` is given; scripts print locations and
counts, never secrets. On staging behind Cloudflare Access, the owner-machine
scripts read `CF_ACCESS_CLIENT_ID` and `CF_ACCESS_CLIENT_SECRET` from the
environment and send the service-token headers (runbook 7.9). Validation that
works on any machine:

```sh
for f in deploy/scripts/*.sh; do bash -n "$f" && echo "syntax ok: $f"; done
grep -rhoE 'CONSENSUS_[A-Z0-9_]+' deploy docs/deployment | sort -u   # cross-check against the variable tables
```

`systemd-analyze verify` is the server-side check of the unit files
(`systemd-analyze verify /etc/systemd/system/consensus-public.service`); the
runbook's appendix A lists the directives and the man pages they were checked
against.
