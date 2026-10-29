# Deployment runbook — Hyphae Consensus public portal

Date: 2026-10-28 · Status: written against `feat/public-portal` at `b62adc0`; nothing
is deployed yet · Audience: the owner-operator (Mario), who administers the server
and owns the Cloudflare zone · Companion files: `deploy/` (templates and scripts,
listed in `deploy/README.md`).

This runbook is the "how" of the release gates in `docs/security/release-checklist.md`:
G1 (old router never exposed), G3 (the edge rules), G4 (the credential file), G6 (the
edge settings), G9 (Hyphae on the public server), G10 (the geo rules) and G11 (ops).
The decisions it implements are in ADR-0006 (listener split), ADR-0007 (live mode on
the owner's key) and ADR-0008 (sealed snapshot). It changes none of them.

Conventions. Every placeholder is in angle brackets: `<hostname>` is the public
hostname chosen on Day 2, `<server>` the SSH alias of the Ubuntu host,
`<TUNNEL-UUID>` the tunnel id, `<snapshot-id>` the sealed snapshot's directory
name. "Owner machine" is the Windows development machine (Git Bash, Docker);
"server" is the Ubuntu 24.04 host. Code blocks are labelled `# owner machine` or
`# server`, and commands prefixed with `sudo` run on the server. **Every request to
`https://<hostname>` or `https://staging.<hostname>` is made from the owner machine**:
the server's egress policy (section 2.4) reaches Cloudflare's tunnel edge, Meta and
Siteverify only, not Cloudflare's HTTP edge, so the server cannot fetch its own
hostname, by design; the scripts that compare origin and edge read the origin over
SSH. During the staging phase those requests carry the Access service-token headers
(section 7.9; `"${ACCESS[@]}"` in section 8). Non-obvious claims about Cloudflare,
systemd or Ubuntu carry a source tag such as `[CF-1]` or `[SD-1]`; the tags resolve
in appendix B. Claims about this project's server cite the file in this repository
that implements them.

Topology (owner decisions, fixed): a Cloudflare Free zone; a **named** Tunnel
(Quick Tunnels have no uptime guarantee, cap in-flight requests at 200, rotate their
hostname and do not support Server-Sent Events `[CF-12]`); `cloudflared` on the server
whose only origin is the unix socket of `consensus-api serve-public`, so no public TCP
port exists and no local user can forge `CF-*` headers (ADR-0006 decision 1); an
Ubuntu 24.04 server with 2 GiB RAM plus swap; provider firewall and ufw: inbound
only SSH from an allowlist, outbound only to `api.meta.ai`, `challenges.cloudflare.com`
and Cloudflare's tunnel edge; Hyphae 4.0.0 installed under its own unprivileged user
and used only as the offline proof verifier; the operator console started on demand
over SSH on loopback with a token file; a dedicated Meta API key supplied through
systemd `LoadCredential`, revoked after judging (about 2026-11-10).

Parallel lanes this runbook depends on (named where used): `CONSENSUS_META_API_KEY_FILE`
(the credential-file way to supply the key; `config.rs` already has
`ApiKey::read_file` for "systemd `LoadCredential`, a secrets mount"); `CONSENSUS_SNAPSHOT_DIR`
with its boot verification (ADR-0008: on `feat/public-portal` since D2-B,
`crates/consensus-api/src/snapshot.rs`: `SnapshotMemoryClient::open` re-runs
`hyphae proof verify` on the two served captures at startup, code
`boot_verification_failed`; `CONSENSUS_SNAPSHOT_VERIFIER_DIR` stays unset with a native
binary); and the public live-mode wiring behind `AppState::live_gate`, with the sealed
snapshot as its evidence factory (`crates/consensus-api/src/live/snapshot_factory.rs`; ADR-0008
"Wiring": the snapshot is read only with live mode enabled, and one that does not boot leaves
the portal up with live mode unavailable, section 8.6). Build the deployed binary from a
commit that has them.

One fact shapes sections 4.4, 4.5, 8.8 and 9.3: **a trip lasts until an operator
resets it.** A trip is persistent across restarts (`budget.rs`, "Trips"), and neither
listener can clear one. The only caller of `Ledger::reset` is the operator's subcommand
`consensus-api ledger reset --by <operator>` (`crates/consensus-api/src/ledger_cli.rs`,
which also has `ledger status` and `ledger release <reservation-id> --by <operator>`),
run on the server through `deploy/scripts/ledger.sh` so it opens the ledger with the
unit's own user, environment and sandbox. This runbook still uses one ledger directory
per phase, for the accounts: staging live runs spend real money that must not count
against the production lifetime cap, so production starts on an empty ledger.

---

## 1. Prerequisites and owner actions

Day numbers are the approved plan's (`docs/reviews/2026-10-27-independent-audit.md`
§7 and `docs/journal.md`, 2026-10-27 night): Day 0 is the plan's approval day.

| Day | Owner action | Output this runbook consumes |
|---|---|---|
| 0 | Save the Official Rules and Credit Terms; back up `demo-run/` and the Docker volume off-machine (plan). | nothing here; precondition for the snapshot capture |
| 2 | **Server**: confirm the Ubuntu 24.04 host (2 GiB RAM), the SSH allowlist addresses, and that the owner has `sudo`. | `<server>`, `<admin-ip>` |
| 2 | **Hostname**: choose `<hostname>` in the Cloudflare zone; reserve `staging.<hostname>` for the Access-protected staging phase. | `<hostname>` |
| 2 | **Tunnel**: on the owner machine, `cloudflared tunnel login` and `cloudflared tunnel create consensus-public` (section 6.1); keep `cert.pem` on the owner machine only. | `<TUNNEL-UUID>`, the credentials file |
| 2 | **Turnstile widget**: create it for `<hostname>` and `staging.<hostname>`, mode Managed (section 7.6). | sitekey (web build), secret (credential file) |
| 2 | **Dedicated Meta key**: create a separate key for the public server on dev.meta.ai (never the development key); claim the hackathon credits on that account and set a spend alert (ADR-0007 consequences). | `meta.key` credential file |
| 3 | **Reproducible Linux build**: `scripts/linux-gates.sh` green in the pinned `ubuntu:24.04` container; release build of `consensus-api` (section 3.1). | the binary and its sha256 |
| captured 2026-10-29 (`b0375df`) | **Snapshot artifact**: the sealed snapshot directory (`manifest.json`, `SHA256SUMS`, `captures/*.proof`, `*.witness`, `*.response.json`, `*.verify.json`) and its pinned anchor, captured once per ADR-0008. Captured as `snap-2026-10-29-demo-repo`; its values are in section 3.3. Placing it on the server needs the owner's authorization. | `<snapshot-id>`, `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` |
| before go-live | **Judge codes**: on the server, `sudo ~/consensus/deploy/scripts/judge-codes.sh --count <judge seats>` generates the invite codes, prints them once and installs only their digests; copy the codes into the submission's testing instructions and nowhere else (section 4.3). | `/etc/consensus/judge-codes.sha256` |
| 5 | Legal texts approved in four languages; public contact email set (web lane). | nothing here |
| after G1–G12 PASS | Go-live (section 8). | |
| about 2026-11-10 | Teardown: revoke the key, export the ledger, archive the runs (section 9.7). | |

Tools on the owner machine: Git Bash, Docker Desktop, `ssh`/`scp`, `sha256sum`,
`curl`, `cloudflared` (for `tunnel login`/`create`/`route dns` only). Nothing on the
server needs Docker, Node or Rust.

---

## 2. Server preparation

### 2.1 Baseline and SSH

```sh
ssh <server>
lsb_release -a            # Ubuntu 24.04 LTS
ldd --version | head -1   # glibc 2.39: the binaries below were built against it
free -h; df -h /
```

SSH stays key-only on the owner's existing setup; the provider firewall admits TCP 22
from the allowlist addresses only and nothing else inbound (no 80/443: the tunnel is
outbound-only `[CF-11]`).

### 2.2 Swap (2 GiB)

The kernel needs a swap file without holes; `swapon(8)` says files created with
`fallocate(1)` "may be interpreted as files with holes too depending of the
filesystem" and that `dd(1)` with `/dev/zero` is "the most portable solution"
`[UB-4]`; `mkswap(8)` gives the same `dd` form `[UB-4]`. A swap entry in `fstab(5)`
uses mount point `none`, type `swap`, options `defaults`, and `0 0` `[UB-4]`.

```sh
sudo dd if=/dev/zero of=/swapfile bs=1MiB count=2048 status=progress
sudo chmod 0600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
echo '/swapfile none swap defaults 0 0' | sudo tee -a /etc/fstab
swapon --show && free -h
```

### 2.3 Users, groups, directories

`deploy/scripts/install.sh` (section 3.4) creates everything below idempotently; the
layout is listed here so it can be audited by hand.

| Principal / path | Owner:group, mode | Purpose |
|---|---|---|
| user `consensus` (system, `nologin`, home `/var/lib/consensus`) | | runs both servers |
| user `hyphae` (system, `nologin`, home `/opt/hyphae`) | | owns the Hyphae binary; never runs anything |
| group `consensus-sock` | | "may connect to the public socket": owns nothing but `/run/consensus` and `public.sock` |
| user `cloudflared` (system, `nologin`), supplementary group `consensus-sock` **only** | | runs the tunnel; reaches the socket through `consensus-sock`; is **not** in `consensus` (see below) |
| `/etc/consensus/` | `root:consensus 0750` | `public.env`, `operator.env`, `judge-codes.sha256` (`root:consensus 0640`) |
| `/etc/consensus/credentials/` | `root:root 0700` | `meta.key`, `turnstile.secret`, `operator.token` (`root:root 0600`), read by systemd (`LoadCredential=`), not by the service |
| `/etc/tmpfiles.d/consensus.conf` | `root:root 0644` | creates `/run/consensus` at boot (`deploy/tmpfiles.d/consensus.conf`) `[SD-8]` |
| `/var/lib/consensus/runs` | `consensus:consensus 0750` | the per-run store (writable) |
| `/var/lib/consensus/ledger/{staging,public}` | `consensus:consensus 0750` | one ledger directory per phase (section 4.5); writable |
| `/var/lib/consensus/ledger-archive/` | `root:root 0700` | retired ledgers (staging after the rehearsal, a suspect one after an incident), read-only to the services and invisible to `consensus` |
| `/var/lib/consensus/{research,web}` | `consensus:consensus 0750`, files `0640` | read-only for the services (`ProtectSystem=strict`, section 5) |
| `/var/lib/consensus/snapshots/` | `consensus:consensus 0700`, files `0640` | the sealed snapshot with its private witnesses: only the service user enters the tree; read-only for the services |
| `/run/consensus/` | `consensus:consensus-sock 2750` (tmpfiles.d, setgid) | `public.sock`, `consensus:consensus-sock 0660`: the group is inherited from the setgid directory `[UB-5]`, the mode is set by the server (`server.rs`, `serve_unix`) |
| `/opt/hyphae/` | `hyphae:hyphae 0755` | the 4.0.0 binary, executable by everyone, writable by nobody who runs |
| `/etc/cloudflared/` | `root:cloudflared 0750` | `config.yml`, `<TUNNEL-UUID>.json` (`root:cloudflared 0640`) |
| `/usr/local/bin/consensus-api` | `root:root 0755` | the server binary |
| `/root/consensus-exports/` | `root:root 0700` | ledger and run exports before `scp` |

**Least privilege of cloudflared.** cloudflared is the only process on the host that
talks to the internet, so it gets exactly one thing: the right to connect to the
socket. Had it joined the group `consensus`, it would read `/etc/consensus/*` (the env
files and the judge digests), and, because the service user owns the snapshot tree, the
private witnesses, and until this revision the operator bearer token too. Instead the
socket directory is created by tmpfiles.d as `2750 consensus:consensus-sock`: a node
created in a set-group-ID directory "will inherit the group ownership from its parent
directory" `[UB-5]`, so the socket the server binds there is `consensus:consensus-sock`,
and the server sets it to `0660` (`server.rs`). `RuntimeDirectory=` is not used for it:
systemd would own the directory `User=`/`Group=` of the unit and would reset the
ownership of an existing directory to that `[SD-1]`, which puts the socket back in the
group `consensus`. The checks are in section 5.1.

No user on this host is in a `docker` group and Docker is not installed (gate G9;
`install.sh` refuses to continue if it finds one).

### 2.4 Firewall (provider and ufw)

ufw starts disabled with a default outgoing policy of allow `[UB-3]`; the rule
grammar is `allow|deny|reject|limit [in|out] ... [proto PROTOCOL] [from ADDRESS
[port PORT]] [to ADDRESS [port PORT]] [comment COMMENT]`, the default policy is set
with `ufw default allow|deny|reject [incoming|outgoing|routed]`, and "the first match
wins" `[UB-3]`. The Ubuntu Server guide shows `ufw allow proto tcp from <ip> to any
port 22`, `ufw status numbered`, `ufw delete <rule>` and `ufw logging on` `[UB-2]`.
ufw has no hostname rules, so the HTTPS destinations are resolved at rule time
(re-run that block if `api.meta.ai` or `challenges.cloudflare.com` change addresses;
section 9.2 checks it daily).

cloudflared needs outbound TCP **and** UDP 7844 to `region1.v2.argotunnel.com` and
`region2.v2.argotunnel.com` (the `quic` default uses UDP and falls back to `http2` on
TCP `[CF-5]`); the published addresses are `198.41.192.{167,67,57,107,27,7,227,47,37,77}`,
`198.41.200.{13,193,33,233,53,63,113,73,43,23}` and `2606:4700:a0::1`–`::10`,
`2606:4700:a8::1`–`::10` `[CF-11]`. Its optional destinations (`api.cloudflare.com`,
`update.argotunnel.com`, `github.com`, all on 443) are for update checks: blocking
them "may prompt a log error, but `cloudflared` will still run correctly" `[CF-11]`,
and the service runs with `--no-autoupdate` anyway (section 6.4).

```sh
sudo ufw default deny incoming
sudo ufw default deny outgoing
sudo ufw allow proto tcp from <admin-ip>/32 to any port 22 comment 'ssh admin'
# DNS: the upstream resolvers the host uses (not the 127.0.0.53 stub)
resolvectl status | grep -E 'DNS Servers|Current DNS Server'
sudo ufw allow out to <resolver-ip> port 53 proto udp comment 'dns'
sudo ufw allow out to <resolver-ip> port 53 proto tcp comment 'dns'
# Time sync (the servers' addresses rotate: by port)
sudo ufw allow out to any port 123 proto udp comment 'ntp'
# Cloudflare tunnel edge, 7844 TCP (http2) and UDP (quic), published addresses [CF-11]
for ip in 198.41.192.167 198.41.192.67 198.41.192.57 198.41.192.107 198.41.192.27 \
          198.41.192.7 198.41.192.227 198.41.192.47 198.41.192.37 198.41.192.77 \
          198.41.200.13 198.41.200.193 198.41.200.33 198.41.200.233 198.41.200.53 \
          198.41.200.63 198.41.200.113 198.41.200.73 198.41.200.43 198.41.200.23; do
  sudo ufw allow out to "$ip" port 7844 proto tcp comment 'cloudflared edge'
  sudo ufw allow out to "$ip" port 7844 proto udp comment 'cloudflared edge'
done
# IPv6 edge addresses (only if the host has IPv6 connectivity)
for prefix in 2606:4700:a0 2606:4700:a8; do for n in 1 2 3 4 5 6 7 8 9 10; do
  sudo ufw allow out to "$prefix::$n" port 7844 proto tcp comment 'cloudflared edge v6'
  sudo ufw allow out to "$prefix::$n" port 7844 proto udp comment 'cloudflared edge v6'
done; done
# Meta Model API and Turnstile Siteverify over HTTPS: resolved now
for host in api.meta.ai challenges.cloudflare.com; do
  for ip in $(getent ahostsv4 "$host" | awk '{print $1}' | sort -u); do
    sudo ufw allow out to "$ip" port 443 proto tcp comment "$host"
  done
done
sudo ufw logging on
sudo ufw enable
sudo ufw status numbered
```

What this policy also closes, on purpose: the server cannot open
`https://<hostname>`. The proxied hostname resolves to Cloudflare's HTTP edge
addresses, which are not the tunnel edge addresses above and are not in the
allowlist, so `curl https://<hostname>/...` from the server fails. Every probe that
goes through the edge (gates G1, G2, G4 responses, G6, the SSE test, the daily edge
check) therefore runs from the owner machine; `verify-edge.sh` fetches the origin half
over SSH (section 8.4) and `canary-scan.sh` has a `--responses-only` half for the
owner machine (section 8.3). A probe that reports `UNREACHABLE` or `ERR` from the
server is this policy at work, not a failure of the portal.

Package installation and updates need egress to the Ubuntu archive, which the
policy above closes. Install `cloudflared` (section 6.2) **before** enabling ufw, and
patch in a maintenance window:

```sh
sudo ufw allow out to any port 80,443 proto tcp comment 'apt maintenance'
sudo apt-get update && sudo apt-get upgrade
sudo ufw status numbered          # find the 'apt maintenance' rule numbers
sudo ufw delete <number>          # once per rule (IPv4 and IPv6), highest number first
```

Whether to leave 80/443 open to the Ubuntu security mirrors so that unattended
upgrades run on their own is an owner decision; see the open questions at the end.

### 2.5 Unattended security updates

`unattended-upgrades` ships preinstalled; `/etc/apt/apt.conf.d/50unattended-upgrades`
allows the `${distro_id}:${distro_codename}-security` origin by default,
`/etc/apt/apt.conf.d/20auto-upgrades` holds `APT::Periodic::Update-Package-Lists "1";`
and `APT::Periodic::Unattended-Upgrade "1";`, automatic reboots are off
(`Unattended-Upgrade::Automatic-Reboot "false";`), and the logs are under
`/var/log/unattended-upgrades` `[UB-1]`.

```sh
dpkg -s unattended-upgrades | grep -E '^Status'
grep -E 'security";' /etc/apt/apt.conf.d/50unattended-upgrades
cat /etc/apt/apt.conf.d/20auto-upgrades
grep -n 'Automatic-Reboot ' /etc/apt/apt.conf.d/50unattended-upgrades   # must stay "false": the owner restarts
sudo unattended-upgrade -v --dry-run     # [UB-1]; needs the apt egress window of 2.4
```

### 2.6 Time synchronisation

`timedatectl status` shows whether network time synchronisation is active,
`timedatectl set-ntp true` "enables and starts the first existing network
synchronization service", and `timedatectl timesync-status` shows the state of
`systemd-timesyncd.service` `[SD-7]`. The servers come from `NTP=` / `FallbackNTP=`
in `timesyncd.conf(5)` or its `/etc/systemd/timesyncd.conf.d/*.conf` drop-ins, with
a compiled-in default list `[SD-7]`. Time matters here: the ledger's day files are
per UTC day and Turnstile refuses a `challenge_ts` older than 300 s
(`live/turnstile.rs`).

```sh
sudo timedatectl set-ntp true
timedatectl status
timedatectl timesync-status
```

### 2.7 journald retention

Both servers log to stdout/stderr, hence to the journal. `SystemMaxUse=` bounds the
disk the journal may use, `MaxRetentionSec=` the age of entries (default 0 = off),
`Storage=persistent` keeps them under `/var/log/journal`, and drop-ins live in
`/etc/systemd/journald.conf.d/*.conf` (suggested prefixes 60–90) `[SD-6]`.

```sh
sudo install -d -m 0755 /etc/systemd/journald.conf.d
printf '[Journal]\nStorage=persistent\nSystemMaxUse=500M\nMaxRetentionSec=2week\n' \
  | sudo tee /etc/systemd/journald.conf.d/60-consensus.conf
sudo systemctl restart systemd-journald
journalctl --disk-usage
```

Two weeks covers the exposure window (go-live to about 2026-11-10) with margin;
the ledger, not the journal, is the record of spend.

---

## 3. Installing the software

### 3.1 The `consensus-api` binary, built reproducibly

`scripts/linux-gates.sh` runs fmt, clippy, the workspace and demo-crate tests, the
evaluation and the snapshot tooling self-tests inside `ubuntu:24.04` pinned by digest
(glibc 2.39, the deployment target), with toolchain `1.99.1` pinned by
`rust-toolchain.toml`, the repository mounted read-only and the toolchain, registry
and target directory in named Docker volumes (the target volume is per checkout path).
It exits non-zero when any gate fails. With `--release` it then builds the release
binary in the same container and volumes (`cargo build --release --locked -p
consensus-api`), so the gates and the artifact come from one toolchain; it refuses a
checkout with uncommitted changes, so the recorded commit is the one built. cargo-deny
is not part of it (it needs the advisory database): run `cargo deny check` on the host
too. On the owner machine, from the repository root at the commit to deploy:

```sh
sh scripts/linux-gates.sh --release    # every gate must PASS; then dist/ holds the build
cat dist/consensus-api.sha256 dist/consensus-api.commit   # the digest (checked after the copy) and the commit
cargo deny check
```

Copy and check on the server (the digest travels separately from the file):

```sh
scp dist/consensus-api dist/consensus-api.sha256 <server>:/tmp/
ssh <server> 'cd /tmp && sha256sum -c consensus-api.sha256'
```

### 3.2 The Hyphae 4.0.0 Linux binary (verifier only)

Hyphae is the owner's earlier, pinned project (README, "Competition boundary"); the
server uses the released binary only to run `hyphae proof verify` against the
sealed snapshot's proofs and witnesses (`CONSENSUS_HYPHAE_BIN`, ADR-0008 decision 5).
No daemon, no store, no `hyphae agent`, no `hyphae mcp`, no key file (gate G9).

Release `release-v4.0.0-crates` (Hyphae 4.0.0, published 2026-09-23, immutable) carries
the asset `hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz` (11,661,966 bytes) with the
GitHub-recorded digest
`sha256:cf92ed6da52d7db634c2d0c8c82bae8f0a36f912ee0fc7c1aea047d65957a075`, and a
`SHA256SUMS` asset with digest
`sha256:61514af06cf1331b36ad7dd0e1e75b11a9a51e1328dd6f28de985b3016ada416` `[HY-1]`.
The server has no egress to GitHub: download and check on the owner machine, then
`scp`. The archive extracts to `hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae`
(`scripts/hyphae-container/README.md`, Setup) and needs glibc 2.39 (same file;
Ubuntu 24.04 has it).

```sh
# owner machine
base=https://github.com/Hyphae-Research-Foundation/hyphae/releases/download/release-v4.0.0-crates
curl -fsSLO "$base/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz"
curl -fsSLO "$base/SHA256SUMS"
echo "cf92ed6da52d7db634c2d0c8c82bae8f0a36f912ee0fc7c1aea047d65957a075  hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz" | sha256sum -c -
echo "61514af06cf1331b36ad7dd0e1e75b11a9a51e1328dd6f28de985b3016ada416  SHA256SUMS" | sha256sum -c -
sha256sum -c --ignore-missing SHA256SUMS      # the release's own checksum list agrees
scp hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz <server>:/tmp/
```

```sh
# server
echo "cf92ed6da52d7db634c2d0c8c82bae8f0a36f912ee0fc7c1aea047d65957a075  /tmp/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz" | sha256sum -c -
sudo tar -xzf /tmp/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz -C /opt/hyphae
sudo chown -R hyphae:hyphae /opt/hyphae
sudo chmod 0755 /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae
sudo ln -sfn /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae /opt/hyphae/bin/hyphae
sha256sum /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae    # record: CONSENSUS_HYPHAE_BIN_SHA256 (4.1), published out of band (ADR-0008 decision 2)
sudo -u consensus /opt/hyphae/bin/hyphae --version
rm /tmp/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz
```

The recorded digest of the extracted binary is the verifier pin,
`CONSENSUS_HYPHAE_BIN_SHA256` (section 4.1). At boot `serve-public` hashes the file
`CONSENSUS_HYPHAE_BIN` names (`/opt/hyphae/bin/hyphae`; the symbolic link is followed, as
`exec` follows it, so the digest is the extracted binary's) before any verifier run, and
refuses another file with `verifier_binary_mismatch` (section 8.6). The same digest must equal
the snapshot manifest's `hyphae.binary_sha256`, the binary that produced and verified the
captures (`capture_binary_mismatch` otherwise): compare them once the snapshot is on the
server (section 3.3). The README and the release tag publish it together with the snapshot
anchor and the manifest digest (ADR-0008 decision 2). The binary is hashed at boot only:
after replacing it, restart the service.

### 3.3 Web export, research archive, sealed snapshot

**Web export.** Built only through Docker on the owner machine (`apps/web/README.md`,
"Build only through Docker"); `pnpm build` runs `next build` and
`scripts/csp-manifest.mjs`, which writes `out/csp-manifest.json`
(`apps/web/package.json`; `static_files.rs` reads it per page).

```sh
# owner machine
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm build
( cd apps/web/out && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum ) > web.SHA256SUMS
tar -czf web.tar.gz -C apps/web/out .
scp web.tar.gz web.SHA256SUMS <server>:/tmp/
```

```sh
# server
sudo rm -rf /var/lib/consensus/web.new && sudo install -d -o consensus -g consensus -m 0750 /var/lib/consensus/web.new
sudo tar -xzf /tmp/web.tar.gz -C /var/lib/consensus/web.new
( cd /var/lib/consensus/web.new && sudo sha256sum -c --quiet /tmp/web.SHA256SUMS ) && echo "web export verified"
sudo chown -R consensus:consensus /var/lib/consensus/web.new
sudo find /var/lib/consensus/web.new -type d -exec chmod 0750 {} + -o -type f -exec chmod 0640 {} +
sudo rm -rf /var/lib/consensus/web.old
[ -d /var/lib/consensus/web ] && sudo mv /var/lib/consensus/web /var/lib/consensus/web.old
sudo mv /var/lib/consensus/web.new /var/lib/consensus/web
test -f /var/lib/consensus/web/csp-manifest.json && echo "csp manifest present"
rm /tmp/web.tar.gz /tmp/web.SHA256SUMS
```

(`sudo systemctl restart consensus-public` afterwards: the export and its CSP manifest
are read at startup.)

**Research archive.** `research/` is byte-for-byte and pinned by `MANIFEST.json` and
`SHA256SUMS` ("the same digests, for `sha256sum -c`", `research/README.md`); the
showcase (`showcase.rs`) reads `MANIFEST.json` and re-checks every file's sha256.

```sh
# owner machine, from the repository root
tar -czf research.tar.gz research
scp research.tar.gz <server>:/tmp/
```

```sh
# server
sudo tar -xzf /tmp/research.tar.gz -C /var/lib/consensus --no-same-owner
( cd /var/lib/consensus/research && sudo sha256sum -c --quiet SHA256SUMS ) && echo "research archive verified"
sudo chown -R consensus:consensus /var/lib/consensus/research
sudo find /var/lib/consensus/research -type d -exec chmod 0750 {} + -o -type f -exec chmod 0640 {} +
rm /tmp/research.tar.gz
```

**Sealed snapshot.** The capture (ADR-0008 decision 2) produces one directory holding
`manifest.json`, `SHA256SUMS` and `captures/*.proof|.witness|.response.json|.verify.json`.
Witnesses and proofs are private operator artifacts: never under the web root, never
in a backup that leaves the server, mode `0640`, owned by the service user, inside a
tree whose top directory is `0700` (only the service user enters it) and which the
service cannot write (section 5).

```sh
# owner machine (the snapshot directory is <snapshot-id>/ next to its manifest)
scp -r <snapshot-id> <server>:/tmp/
```

```sh
# server
sudo mv /tmp/<snapshot-id> /var/lib/consensus/snapshots/<snapshot-id>
( cd /var/lib/consensus/snapshots/<snapshot-id> && sudo sha256sum -c --quiet SHA256SUMS ) && echo "snapshot verified"
sudo sha256sum /var/lib/consensus/snapshots/<snapshot-id>/manifest.json            # must print the published manifest digest: CONSENSUS_SNAPSHOT_MANIFEST_SHA256
sudo grep -o '"binary_sha256": "[0-9a-f]*"' /var/lib/consensus/snapshots/<snapshot-id>/manifest.json   # must be the binary digest of 3.2: CONSENSUS_HYPHAE_BIN_SHA256
sudo chown -R consensus:consensus /var/lib/consensus/snapshots
sudo find /var/lib/consensus/snapshots -type d -exec chmod 0750 {} + -o -type f -exec chmod 0640 {} +
sudo chmod 0700 /var/lib/consensus/snapshots                      # nobody but the service user enters the tree
sudo ls -la /var/lib/consensus/snapshots/<snapshot-id> /var/lib/consensus/snapshots/<snapshot-id>/captures | head
sudo -u cloudflared ls /var/lib/consensus/snapshots 2>&1 | grep -q 'Permission denied' && echo "witnesses: unreachable for cloudflared"
```

The anchor for `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` is the shared anchor recorded in
the snapshot's `manifest.json` (64 hex digits; `config.rs` normalises case and
whitespace and refuses anything else). The value of `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`
is the SHA-256 of that `manifest.json`: the digest recorded on the owner machine after
the last `snapshot_manifest.py build` (`docs/architecture/sealed-snapshot.md` section 3,
step 5), equal to the `manifest.json` line of `SHA256SUMS` and to the digest the README
and the release tag publish; the `sha256sum` line above must print it. Take the value
from the publication, never from the copy in doubt: with it pinned, a `manifest.json` with
any other digest does not load (`manifest_pin_mismatch`, section 8.6), and since the
manifest holds the digest of every capture file, neither does a copy with one byte of a
record text changed, whatever `SHA256SUMS` says. `CONSENSUS_HYPHAE_BIN_SHA256` is the
binary digest of section 3.2, which the `grep` line above must show. Those four lines
(`CONSENSUS_SNAPSHOT_DIR`, `CONSENSUS_HYPHAE_EXPECTED_ANCHOR`,
`CONSENSUS_SNAPSHOT_MANIFEST_SHA256`, `CONSENSUS_HYPHAE_BIN_SHA256`) are commented out in
the `public.env` template until the snapshot is on the server; uncomment and fill them
now (with live mode still off they are only noticed, never read).

**The values for `snap-2026-10-29-demo-repo`** (captured 2026-10-29, `b0375df`; published
in the README, "Published snapshot digests"; not yet on the server):

| Variable | Value | Where it is recorded |
|---|---|---|
| `CONSENSUS_SNAPSHOT_DIR` | `/var/lib/consensus/snapshots/snap-2026-10-29-demo-repo` | the directory name must equal the manifest's `snapshot_id` (`snapshot_id_mismatch` otherwise) |
| `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` | `43a2dea9c48d015bf6cbd6692b7ecf2bf80c66cd47d398b38e6b8471b18e8f02` | `manifest.json` `anchor_hex`, `session.json` |
| `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` | `b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a` | `sha256sum snapshots/snap-2026-10-29-demo-repo/manifest.json`, the `manifest.json` line of its `SHA256SUMS` |
| `CONSENSUS_HYPHAE_BIN_SHA256` | `79025e8758ee7481f572d0451756c0e0771e96d13cd7134319367afa8b11a15f` | `hyphae-binary.sha256` and `manifest.json` `hyphae.binary_sha256` (the container's `/opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae`) |

The binary digest has not yet been compared with the binary extracted from the release
tarball checked against `cf92ed6d…` (section 3.2); the `sha256sum` line of section 3.2 on
the server must print the same value, or the snapshot was captured with another build
(`capture_binary_mismatch`) and nothing is pinned until that is resolved.

Two things to know when copying it (observed in the boot check of 2026-10-29, `0515f29`):

- A git checkout of `snapshots/snap-2026-10-29-demo-repo/` does not boot: the `.proof` and
  `.witness` files are git-ignored, and the runtime refuses the directory with
  `checksums_incomplete`. Copy the full directory from the container
  (`/hyphae-home/snapshots/snap-2026-10-29-demo-repo`) or from the owner's main checkout,
  and check it with `sha256sum -c SHA256SUMS` before the `scp`.
- Keep the directory name `snap-2026-10-29-demo-repo`; a copy under another name is
  refused with `snapshot_id_mismatch`.

The runtime at `6e202eb` refuses this snapshot at boot (`response_invalid: …
no memories array`): `hyphae agent ui` 4.0.0 wraps the result object in an envelope with
`result`. The fix is `0515f29` (branch `test/snapshot-boot`); build the server binary of
section 3.1 from a commit that contains it.

### 3.4 Users, directories, binary, units, templates: `install.sh`

Copy the repository's `deploy/` directory to the server and run the installer once
(it is idempotent and starts nothing; section 2.3 lists what it creates):

```sh
# owner machine
tar -czf deploy.tar.gz deploy
scp deploy.tar.gz <server>:/tmp/
```

```sh
# server
mkdir -p ~/consensus && tar -xzf /tmp/deploy.tar.gz -C ~/consensus
sudo ~/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 "$(cut -d' ' -f1 /tmp/consensus-api.sha256)"
rm /tmp/consensus-api /tmp/consensus-api.sha256 /tmp/deploy.tar.gz
```

Re-running it with a new binary replaces `/usr/local/bin/consensus-api` after asking
(or with `--yes`); it never overwrites `/etc/consensus/*.env` or
`/etc/cloudflared/config.yml`.

---

## 4. Configuration

### 4.1 `/etc/consensus/public.env` and `operator.env`

The templates are `deploy/etc/consensus/public.env.example` and
`deploy/etc/consensus/operator.env.example`; `install.sh` placed them at
`/etc/consensus/public.env` and `/etc/consensus/operator.env` (`root:consensus 0640`).
systemd reads them as newline-separated `VAR=value` lines and the files "will be
read shortly before the process is executed" `[SD-1]`; `$VAR` references are not
expanded, so the credential paths are written out in full.

Fill in every `<placeholder>`:

| Variable | Value | Source of the rule |
|---|---|---|
| `CONSENSUS_ALLOWED_ORIGINS` | `https://<hostname>,https://staging.<hostname>` | required with the socket (`public_config.rs`) |
| `CONSENSUS_ALLOWED_HOSTS` | `<hostname>,staging.<hostname>` | required with the socket |
| `CONSENSUS_RUN_STORE_DIR` | `/var/lib/consensus/runs` | default is a relative `consensus-store` |
| `CONSENSUS_RESEARCH_DIR` | `/var/lib/consensus/research` | default is relative `research` |
| `CONSENSUS_WEB_ROOT` | `/var/lib/consensus/web` | default is relative `apps/web/out`; the CSP manifest is found at `<web root>/csp-manifest.json` |
| `CONSENSUS_PUBLIC_MODE` | `true` | refuses `CONSENSUS_META_DEBUG_DIR` and development-only behaviour (`config.rs`) |
| `CONSENSUS_META_API_KEY_FILE` | `/run/credentials/consensus-public.service/meta.key` | parallel lane; the path systemd gives a `LoadCredential=meta.key:...` for this unit (`/run/credentials/UNITNAME`, `[SD-1]`) |
| `CONSENSUS_HYPHAE_BIN` | `/opt/hyphae/bin/hyphae` | section 3.2; an absolute path, which it must be with `CONSENSUS_HYPHAE_BIN_SHA256` set (a relative path or a bare name then stops startup naming it) |
| `CONSENSUS_HYPHAE_BIN_SHA256` | the extracted binary's sha256, 64 hex (section 3.2) | required with live mode enabled on the unix socket (missing, startup stops naming it; a development TCP bind may leave it unset); commented out in the template until the snapshot is on the server (section 3.3); must equal the manifest's `hyphae.binary_sha256`; at boot another file is `verifier_binary_mismatch` and a snapshot of another build `capture_binary_mismatch`, either leaving live mode unavailable (section 8.6); with live mode enabled a malformed value stops startup naming it |
| `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` | the snapshot's anchor, 64 hex | mandatory for every non-simulated mode (ADR-0008 decision 5); commented out in the template until the snapshot is on the server (section 3.3); with live mode enabled a missing or malformed value stops startup naming it |
| `CONSENSUS_SNAPSHOT_DIR` | `/var/lib/consensus/snapshots/<snapshot-id>` | commented out in the template until the snapshot is on the server; read only with `CONSENSUS_LIVE_ENABLED=true` (with live mode off a set value earns one `[snapshot]` notice and nothing is loaded); unset with live mode on stops startup; a snapshot that does not boot leaves the portal up with live mode unavailable (section 8.6); `CONSENSUS_SNAPSHOT_VERIFIER_DIR` stays unset (native verifier, same paths) |
| `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` | the published sha256 of the snapshot's `manifest.json`, 64 hex (section 3.3) | required with live mode enabled on the unix socket (missing, startup stops naming it; a development TCP bind may leave it unset); commented out in the template until the snapshot is on the server; a manifest with another digest does not load (`manifest_pin_mismatch`, live mode unavailable, section 8.6); with live mode enabled a malformed value stops startup naming it |
| `CONSENSUS_LIVE_ENABLED` | `false` until section 8.7, then `true` | `live/config.rs` |
| `CONSENSUS_LIVE_LEDGER_DIR` | `/var/lib/consensus/ledger/staging` during staging; `/var/lib/consensus/ledger/public` from go-live | required when enabled; one directory per phase, so production starts at zero (section 4.5) |
| `CONSENSUS_LIVE_JUDGE_CODES_FILE` | `/etc/consensus/judge-codes.sha256`, set only once the file has at least one digest | an empty digest file is a configuration error (`live/judge.rs`) |
| `CONSENSUS_LIVE_CLIENT_DAILY` | unset: the default `3` | public-lane live runs per client key (IPv4 /32, IPv6 /64) per UTC day; over it `429 live_client_quota` until 00:00 UTC; the judge lane is exempt; process-local, so a restart of `consensus-public` forgets the counts (`live/config.rs`, `live/gate.rs`) |
| `CONSENSUS_LIVE_JUDGE_CODE_DAILY` | unset: the default `5` | judge-lane live runs per invite code per UTC day; over it the judge lane's daily refusal `429 live_budget_judge_daily`; process-local (`live/config.rs`, `live/gate.rs`) |
| `CONSENSUS_TURNSTILE_SECRET_FILE` | `/run/credentials/consensus-public.service/turnstile.secret` | required when enabled |
| `CONSENSUS_TURNSTILE_HOSTNAME` | `staging.<hostname>` during staging, `<hostname>` from go-live | one bare hostname; Siteverify's `hostname` must equal it (`live/turnstile.rs`) |

Everything else keeps its default, and the defaults are the owner decisions: $10.00 a
UTC day, $60.00 lifetime, $15.00 judge slice (`CONSENSUS_LIVE_DAILY_USD`,
`CONSENSUS_LIVE_LIFETIME_USD`, `CONSENSUS_LIVE_JUDGE_USD`), a 32,000-token per-call input
cap (`CONSENSUS_LIVE_INPUT_CAP_TOKENS`; with the 70,000-token run cap and the 16,000-token
output bound a run's worst case is $2.36, inside the $2.50 the day offers at 00:00 UTC;
8,000 refused every snapshot-backed run at its first evaluation), the 23 portal-blocked
codes and `KR` live-only (`CONSENSUS_GEO_BLOCKED_COUNTRIES`,
`CONSENSUS_GEO_LIVE_ONLY_BLOCKED`), the Turnstile actions `live-run` and `session`,
the 15 s SSE heartbeat, the socket path and `CONSENSUS_TRUST_PROXY_HEADERS=true`.
`CONSENSUS_LIVE_EXPOSURE_DAYS` stays unset: when set, `live/config.rs` enforces
`daily × days ≤ lifetime`, and `$10 × 7 or more days` exceeds `$60`; the owner either
lowers the daily cap or leaves the variable unset and accepts that the lifetime cap can
be reached before judging ends (open question). Never set `CONSENSUS_META_API_KEY`,
`MODEL_API_KEY`, `META_API_KEY`, `HYPHAE_NATIVE_API_KEY_FILE`, `HYPHAE_NATIVE_ENDPOINT`
or `HYPHAE_BASE_URL` on this server.

`operator.env` points `CONSENSUS_OPERATOR_TOKEN_FILE` and `CONSENSUS_META_API_KEY_FILE`
at `/run/credentials/consensus-operator.service/...`, that unit's own credentials
directory (the token is a `LoadCredential=` like the key, section 4.2); the operator unit
loads no Turnstile secret, since `serve-operator` reads the live configuration
credential-free. `CONSENSUS_OPERATOR_BIND` keeps its default `127.0.0.1:8787`
(`public_config.rs`; a non-loopback address is refused), and `CONSENSUS_LIVE_LEDGER_DIR`
is inherited from `public.env`, never overridden: `serve-operator` opens that ledger with
the same caps as `consensus-public` and meters every console `model=meta` run on it, on
the operator lane (gate G3; section 9.2). It reads that configuration without any
credential (the live switch, the Turnstile secret file and the judge-codes file are
hidden from the load, as for `ledger.sh`), and refuses to start when the directory does
not exist. A console run's `CONSENSUS_META_MAX_COMPLETION_TOKENS` (default 16000) must not
exceed `CONSENSUS_LIVE_MAX_COMPLETION_TOKENS` (default 16000), or the metered run is
refused before anything is reserved.

The server validates every variable at startup and stops with the variable's name on
an invalid value (`public_config.rs`, `live/config.rs`); a failed start is visible with
`journalctl -u consensus-public -n 50`. A snapshot that does not boot is not a failed
start: the portal comes up with live mode unavailable and says so (section 8.6).

### 4.2 Credential files (read by systemd, never by the shell history)

`LoadCredential=ID:PATH` copies `PATH` into the unit's credentials directory, "only
accessible to the user associated with the unit", exported as `$CREDENTIALS_DIRECTORY`
and, for system services, reachable as `/run/credentials/UNITNAME` `[SD-1]`; the
service process therefore never has the key in its environment (gate G4). The files
under `/etc/consensus/credentials/` are read by systemd as root, so they stay
`root:root 0600`. Both the key and the secret are trimmed on read (`config.rs`
`ApiKey`, `live/turnstile.rs` `TurnstileSecret::from_file`), so a trailing newline is
harmless. Type or paste the value into the editor-less `cat` (end with Ctrl-D) so it
never appears in a command line or shell history:

```sh
sudo install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/meta.key
sudo sh -c 'cat > /etc/consensus/credentials/meta.key'        # paste the dedicated key (LLM_<id>_<secret>), Ctrl-D
sudo install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/turnstile.secret
sudo sh -c 'cat > /etc/consensus/credentials/turnstile.secret' # paste the widget secret, Ctrl-D
sudo ls -la /etc/consensus/credentials
```

Both files must exist before the first `systemctl start`: the unit's two
`LoadCredential=` lines name them with absolute paths. For a staging phase without
live mode the key file may hold a canary of the documented shape (for example
`LLM_0_canary-<random>`), which section 8.3 then scans for; replace it with the real
key before enabling live mode.

The operator token (at least 32 characters, `server.rs` `operator_token`) is a
credential like the other two: `root:root 0600` on disk, handed to the console by
`LoadCredential=operator.token:...` in `consensus-operator.service` and read from
`/run/credentials/consensus-operator.service/operator.token` (`operator.env`). No
group-readable copy exists anywhere. With the file configured the token is never
printed:

```sh
sudo install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/operator.token
openssl rand -hex 32 | sudo tee /etc/consensus/credentials/operator.token >/dev/null
```

Cloudflare's always-pass / always-fail test secrets are refused in public mode
(`live/turnstile.rs` `TEST_SECRETS`; the published values are on `[CF-21]`), so a
test configuration cannot reach production by mistake.

### 4.3 Judge invite codes

The file holds the SHA-256 of each code as 64 lowercase hex characters, one per line,
`#` comments and blank lines allowed; the server never sees a code in plain text and
reports a malformed line by number only (`live/judge.rs`). The digests are unsalted, so
the codes must be long and random: `deploy/scripts/judge-codes.sh` makes them on the
server, 140 random bits each from `/dev/urandom` (28 symbols of `0-9 A-Z` without
`I L O U`, in seven groups of four joined by hyphens), prints them once, and installs
only their digests, `root:consensus 0640`, after one comment line. It refuses to
overwrite an existing digests file unless given `--force`, which replaces the file and
so revokes every code it held. The codes themselves go only into the submission's
testing instructions (release checklist, owner step 4); a judge enters one exactly as
printed, hyphens included (the server hashes what it receives, trimmed).

```sh
# server: one code per judge seat
sudo ~/consensus/deploy/scripts/judge-codes.sh --count 3      # prints the codes ONCE: copy them into the testing instructions now
sudo stat -c '%U:%G %a' /etc/consensus/judge-codes.sha256      # root:consensus 640
sudo grep -c -E '^[0-9a-f]{64}$' /etc/consensus/judge-codes.sha256   # the number of codes
```

Then uncomment `CONSENSUS_LIVE_JUDGE_CODES_FILE` in `public.env` and restart
`consensus-public`: the file is read at startup only. Each code admits at most
`CONSENSUS_LIVE_JUDGE_CODE_DAILY` (default 5) judge-lane runs per UTC day, refused over it
with the judge lane's own daily refusal (`429 live_budget_judge_daily`), so a code's holder
learns nothing new from it (`live/gate.rs`). Revocation is deleting the code's line from
the file (`sudoedit /etc/consensus/judge-codes.sha256`) and restarting `consensus-public`,
or the kill switch for every lane at once. What a code cannot hide: the judge lane is
observable (its refusals differ from the public lane's, it has its own hourly window and
slot, and `/capabilities`' `daily_remaining` leaves judge reservations out), so someone who
solves one Turnstile challenge per guess can tell a valid code from an invalid one, which
is why the codes are long and random; and a leaked code drains the judge slice and the
judge daily cap at up to its per-code limit a day until it is revoked.

### 4.4 The kill-switch file and the reset

Path: `CONSENSUS_LIVE_KILL_SWITCH_FILE`, default `<ledger dir>/kill-switch`
(`live/config.rs`), so `/var/lib/consensus/ledger/staging/kill-switch` during staging
and `/var/lib/consensus/ledger/public/kill-switch` from go-live. Its presence trips
the ledger at the next admission or snapshot (`budget.rs` `check_kill_switch`); the
trip is written to the day file and to `lifetime.json`, is restored at every reopen,
and a file still present trips again (`live/budget.rs`, "Trips"). Removing the file
therefore never re-opens live mode: the trip lasts until
`sudo ~/consensus/deploy/scripts/ledger.sh reset --by <operator>` clears it
(`ledger_cli.rs`). The reset refuses while the kill-switch file is present (the next
admission would trip again), records nothing when there is no trip, and is written to
the day file as `reset {by, at}`; the running server reads it at its next admission
(`/ready` and `/capabilities` within their cache age), so no restart is needed. Write
the reason for the stop up before resetting (section
9.3). `deploy/scripts/kill-switch.sh` operates the file, `deploy/scripts/ledger.sh` the
trip; both say so in their output.

### 4.5 Ledger lifecycle: one directory per phase

Gate G11 requires the kill switch "exercised on staging" (release checklist), and
staging live runs spend real money on the dedicated key. The ledger is a directory
(`CONSENSUS_LIVE_LEDGER_DIR`: day files, `lifetime.json`, `ledger.lock`; `Ledger::open`
creates a missing directory, `budget.rs`), so the phases get separate directories under
the single writable path `/var/lib/consensus/ledger`, and the production lifetime cap
counts from zero at go-live:

| Phase | `CONSENSUS_LIVE_LEDGER_DIR` | Fate |
|---|---|---|
| staging (sections 8.1–8.6, 8.8) | `/var/lib/consensus/ledger/staging` | tripped and reset by the rehearsal (8.8); exported, then moved to `/var/lib/consensus/ledger-archive/` before go-live |
| production (from 8.7) | `/var/lib/consensus/ledger/public` | must be empty at go-live (no `lifetime.json`); a trip is reset after its write-up (9.3); exported at teardown |
| incident (9.6) | the same `ledger/public` after a reset, or a new `/var/lib/consensus/ledger/public-<n>` only when the ledger itself is suspect (an `inconsistent` report, a file touched by hand) | the suspect one is archived the same way |

Rules that follow from the code:

1. **Staging live runs spend real money on the dedicated key** (the only key allowed
   on the server; there is no "staging key"), recorded in the staging ledger. Read
   `lifetime_spent_usd` from its `lifetime.json` before archiving it
   (`kill-switch.sh status` prints it) and carry the figure into the owner's accounting.
2. **The production ledger starts at zero**, so its $60 lifetime cap counts from
   go-live: the $60 is the public phase's budget, staging is paid from the rest of the
   credit (open question 6, answered by the plan).
3. **Never edit or delete `lifetime.json` or a day file to clear a trip**
   (`ledger.sh reset` is the way). The summary
   is derived from the day files and re-derived at open; a summary trip the day files
   do not show is restored anyway and reported as an inconsistency (`budget.rs`
   `apply_summary`). An archived ledger is the evidence of what was spent and why it
   stopped; it leaves the writable tree so neither server can touch it again.
4. **Both units read the same `public.env`**, so a ledger change is one edit plus a
   restart of `consensus-public` (and of the operator console if it is running); the
   operator console must never point at a different ledger (`operator.env`).

Retiring the staging ledger (after section 8.8, before 8.7):

```sh
# server
sudo ~/consensus/deploy/scripts/kill-switch.sh status                                  # tripped: true, trip_code: kill_switch, lifetime_spent_usd
sudo ~/consensus/deploy/scripts/export-ledger.sh --ledger-dir /var/lib/consensus/ledger/staging
sudo systemctl stop consensus-public.service; sudo systemctl stop consensus-operator.service 2>/dev/null || true
sudo mv /var/lib/consensus/ledger/staging "/var/lib/consensus/ledger-archive/staging-$(date -u +%Y%m%dT%H%M%SZ)"
sudo chown -R root:root /var/lib/consensus/ledger-archive && sudo chmod -R u=rwX,go= /var/lib/consensus/ledger-archive
sudo test ! -e /var/lib/consensus/ledger/public/lifetime.json && echo "production ledger is empty"
sudoedit /etc/consensus/public.env          # CONSENSUS_LIVE_LEDGER_DIR=/var/lib/consensus/ledger/public (and the go-live values of 8.7)
```

The restart happens in section 8.7, step 2. `install.sh` creates both directories and
the archive directory; a missing ledger directory would be created by the server
anyway.

---

## 5. systemd units

Templates: `deploy/systemd/consensus-public.service`,
`deploy/systemd/consensus-operator.service`,
`deploy/systemd/cloudflared.service.d/10-consensus.conf`; `install.sh` installs
them under `/etc/systemd/system/`. Appendix A lists every directive with the man-page
definition it was checked against. The choices:

### 5.1 `consensus-public.service`

- `Type=exec`: the unit counts as started once the binary has been executed, so a
  missing binary or user fails the start instead of succeeding silently `[SD-2]`.
- `User=consensus`, `Group=consensus`, `UMask=0027`: run files and ledger files are
  created `0640`; `UMask=` "Defaults to 0022 for system units" otherwise `[SD-1]`.
- `EnvironmentFile=/etc/consensus/public.env` (section 4.1) `[SD-1]`.
- `LoadCredential=meta.key:/etc/consensus/credentials/meta.key` and
  `LoadCredential=turnstile.secret:/etc/consensus/credentials/turnstile.secret`
  (section 4.2) `[SD-1]`.
- The socket directory `/run/consensus` comes from `/etc/tmpfiles.d/consensus.conf`
  (`d /run/consensus 2750 consensus consensus-sock - -`), applied at boot by
  `systemd-tmpfiles` ("creates volatile files and directories during boot") and by
  `install.sh` immediately (`systemd-tmpfiles --create /etc/tmpfiles.d/consensus.conf`)
  `[SD-8]`; it is listed in `ReadWritePaths=` so the server can bind there under
  `ProtectSystem=strict`. The setgid bit makes the socket inherit the group
  `consensus-sock` `[UB-5]` and the server sets it to `0660` (`server.rs`), so only
  `consensus` and the members of `consensus-sock` (cloudflared, section 6.4) can
  connect. `RuntimeDirectory=` is deliberately not used: it would own the directory
  `User=`/`Group=` (`consensus:consensus`) and re-apply that ownership to an existing
  directory `[SD-1]`, putting the socket into the group `consensus` (section 2.3).
- `ExecStart=/usr/local/bin/consensus-api serve-public` (`main.rs`).
- `KillMode=mixed`, `KillSignal=SIGTERM`, `TimeoutStopSec=90`: on stop SIGTERM goes to
  the main process only and SIGKILL to the rest of the control group after the
  timeout `[SD-3]`; the server handles SIGTERM by cancelling active runs and waiting
  for their checkpoints with a 60 s grace (`server.rs` `graceful`, `BuildOptions`),
  so 90 s is enough.
- `Restart=on-failure`, `RestartSec=5s`: restart on a non-zero exit or a signal, "the
  recommended choice for long-running services"; `systemctl stop` never triggers it
  `[SD-2]`. Startup recovery then closes runs left mid-deliberation as failed,
  stage `interrupted` (`server.rs` `print_recovery`; `docs/threat-model.md` v2, T37 and T43).
- Sandbox: `NoNewPrivileges=yes`; `ProtectSystem=strict` (the whole hierarchy read-only
  except the API file systems) with `ReadWritePaths=/var/lib/consensus/runs
  /var/lib/consensus/ledger /run/consensus` (the ledger tree holds the per-phase
  ledger directories of section 4.5); `ProtectHome=yes`; `PrivateTmp=yes` (the verifier's
  temporary directories are private and swept on stop); `PrivateDevices=yes`;
  `ProtectKernelTunables=`, `ProtectKernelModules=`, `ProtectKernelLogs=`,
  `ProtectControlGroups=`, `ProtectClock=`, `ProtectHostname=yes`;
  `ProtectProc=invisible`; `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6` (the
  socket, Meta and Siteverify over IPv4/IPv6); `RestrictNamespaces=`,
  `RestrictRealtime=`, `RestrictSUIDSGID=`, `LockPersonality=`,
  `MemoryDenyWriteExecute=`, `RemoveIPC=yes`; `CapabilityBoundingSet=` (empty: the
  bounding set is reset to nothing); `SystemCallFilter=@system-service` with
  `SystemCallErrorNumber=EPERM`, the man page's "relatively safe basic choice for the
  majority of system services", and `SystemCallArchitectures=native` `[SD-1]`.
  `research/`, `snapshots/` and `web/` are therefore read-only to the service: no
  writer path near the proofs (gate G9).
- Resources: `MemoryHigh=768M` (throttling), `MemoryMax=1G` (the OOM killer acts
  inside the unit), `TasksMax=1024` (threads count individually; tokio's blocking pool
  needs headroom) `[SD-4]`; `LimitNOFILE=65536` for 512 concurrent streams plus run
  files (the man page's caution concerns `select(2)`, which tokio does not use) `[SD-1]`.
- `[Install] WantedBy=multi-user.target`: `systemctl enable` creates the symlink that
  starts it at boot `[SD-5]`.

```sh
# server
stat -c '%A %U:%G %n' /run/consensus                      # drwxr-s--- consensus:consensus-sock (else: sudo systemd-tmpfiles --create /etc/tmpfiles.d/consensus.conf)
sudo systemctl enable --now consensus-public.service
systemctl status consensus-public.service --no-pager
sudo ss -lxp | grep /run/consensus/public.sock          # the socket, owned by consensus-api
stat -c '%A %U:%G %n' /run/consensus/public.sock          # srw-rw---- consensus:consensus-sock (the group inherited from the setgid directory)
sudo curl -sS --unix-socket /run/consensus/public.sock -H 'Host: <hostname>' \
     -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' http://localhost/api/v1/health   # without CF-IPCountry: 451 geo_unknown
# least privilege of cloudflared: the socket yes, everything else no
sudo -u cloudflared curl -sS --unix-socket /run/consensus/public.sock -H 'Host: <hostname>' \
     -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' -o /dev/null -w 'cloudflared -> socket: %{http_code}\n' http://localhost/api/v1/health   # 200
for p in /etc/consensus/public.env /etc/consensus/judge-codes.sha256 /etc/consensus/credentials/operator.token \
         /var/lib/consensus/snapshots /var/lib/consensus/runs /var/lib/consensus/ledger; do
  sudo -u cloudflared ls -d "$p" >/dev/null 2>&1 && echo "LEAK: cloudflared reaches $p" || echo "ok: cloudflared cannot read $p"
done
id -nG cloudflared | tr ' ' '\n' | grep -qx consensus && echo "WRONG: cloudflared is in group consensus (gpasswd -d cloudflared consensus)" || echo "ok: cloudflared not in group consensus"
```

(`CF-Connecting-IP` is required on every request to the socket: the listener serves
cloudflared only, `api/mod.rs`. If the socket's group is not `consensus-sock`, the
directory lost its setgid bit or its group: re-run the `systemd-tmpfiles --create`
line and restart the service; never fall back to adding cloudflared to `consensus`.)

### 5.2 `consensus-operator.service` (on demand, never enabled)

Same sandbox and `EnvironmentFile=` chain (`public.env`, then `operator.env`: "the
later setting will override the earlier setting" `[SD-1]`), its own two
`LoadCredential=` lines (`meta.key` and `operator.token`, so
`/run/credentials/consensus-operator.service/` holds both while it runs; no Turnstile
secret: `serve-operator` reads the live configuration credential-free),
`ReadWritePaths=` without the socket directory (the console has no socket),
`ExecStart=/usr/local/bin/consensus-api serve-operator`, no `Restart=` and **no
`[Install]` section**: enabling consists solely of creating the symlinks `[Install]`
describes `[SD-5]`, so this unit cannot be enabled and never starts at boot. The
console binds `127.0.0.1:8787` only (`public_config.rs`) and requires the bearer
token on every `/api` route (`api/mod.rs`). Usage in section 9.2.

### 5.3 Checks on the server

```sh
sudo systemd-analyze verify /etc/systemd/system/consensus-public.service /etc/systemd/system/consensus-operator.service
systemctl is-enabled consensus-operator.service || true     # "static": no [Install], cannot be enabled
systemctl cat cloudflared.service                            # the packaged unit plus the drop-in (section 6.4)
systemd-tmpfiles --cat-config | grep -A1 'consensus.conf'    # the socket directory line is seen by systemd-tmpfiles
```

---

## 6. cloudflared: the named tunnel

### 6.1 Create the tunnel and route DNS (owner machine)

`cloudflared tunnel login` opens a browser, and after login generates "the `cert.pem`
file, in the default cloudflared directory"; `cloudflared tunnel create <NAME>`
generates "a tunnel credentials file in the default cloudflared directory" and a
`<UUID>.cfargotunnel.com` subdomain; `cloudflared tunnel route dns <UUID or NAME>
<hostname>` creates a CNAME to `<UUID>.cfargotunnel.com` `[CF-8]` `[CF-10]`. The
account certificate "allows users to create, delete, and manage all tunnels for the
account"; the credentials file "only allows the user to run that specific tunnel, and
do nothing else" and does not expire `[CF-9]`. Only the credentials file goes to the
server; `cert.pem` stays on the owner machine.

```sh
cloudflared tunnel login
cloudflared tunnel create consensus-public            # note the UUID and the credentials file path
cloudflared tunnel list
# Staging first: create the Access application (section 7.8) BEFORE this route,
# otherwise the hostname is public until the policy exists [CF-31].
cloudflared tunnel route dns consensus-public staging.<hostname>
# Production hostname: at go-live (section 8.7), not before.
# cloudflared tunnel route dns consensus-public <hostname>
scp ~/.cloudflared/<TUNNEL-UUID>.json <server>:/tmp/
```

The DNS record is a CNAME to `<TUNNEL-UUID>.cfargotunnel.com`; the record and the
tunnel are independent, and a stopped tunnel leaves the record answering with
Cloudflare error 1016 `[CF-10]`. In the DNS app, confirm the record is proxied
(orange cloud): a tunnel hostname only works through Cloudflare's proxy.

### 6.2 Install cloudflared on the server (before ufw blocks egress)

Cloudflare's package repository `[CF-13]`:

```sh
sudo mkdir -p --mode=0755 /usr/share/keyrings
curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg | sudo tee /usr/share/keyrings/cloudflare-main.gpg >/dev/null
echo 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared noble main' | sudo tee /etc/apt/sources.list.d/cloudflared.list
sudo apt-get update && sudo apt-get install cloudflared
cloudflared --version
```

(The alternative is the `.deb` from the releases page, downloaded on the owner
machine and copied with `scp` `[CF-13]`.)

### 6.3 Configuration file and credentials

Template: `deploy/cloudflared/config.yml.example`, installed by `install.sh` at
`/etc/cloudflared/config.yml` (`root:cloudflared 0640`). The file needs `tunnel`
(the UUID) and `credentials-file`, and "Configuration files that contain ingress
rules must always include a catch-all rule that concludes the file"; the documented
catch-all is `- service: http_status:404` `[CF-1]`. In cloudflared's ingress parser a
service beginning with `unix:` is taken as a unix socket path with scheme http
("No validation necessary for unix socket filepath services"), `unix+tls:` the same
over https, `http_status:<code>` answers that status, and "The last ingress rule must
match all URLs (i.e. it should not have a hostname or path filter)" `[CF-2]`. The
`originRequest` keys are `connectTimeout`, `noTLSVerify`, `http2Origin`,
`httpHostHeader`, and so on, and a rule-level value wins over the ingress-wide one,
which wins over the defaults `[CF-3]`; `connectTimeout` defaults to `30s`,
`noTLSVerify` to `false` (TLS verification on), `http2Origin` to `false` (HTTP/1.1)
and `httpHostHeader` to empty `[CF-4]`. For a plain socket only `connectTimeout` is
set; `noTLSVerify` and `http2Origin` stay unset, and `httpHostHeader` stays unset so
the origin receives the public hostname in `Host`, which the server checks against
`CONSENSUS_ALLOWED_HOSTS`.

```yaml
tunnel: <TUNNEL-UUID>
credentials-file: /etc/cloudflared/<TUNNEL-UUID>.json
ingress:
  - hostname: <hostname>
    service: unix:/run/consensus/public.sock
    originRequest:
      connectTimeout: 10s
  - hostname: staging.<hostname>
    service: unix:/run/consensus/public.sock
    originRequest:
      connectTimeout: 10s
  - service: http_status:404
```

```sh
sudo install -o root -g cloudflared -m 0640 /tmp/<TUNNEL-UUID>.json /etc/cloudflared/<TUNNEL-UUID>.json && rm /tmp/<TUNNEL-UUID>.json
sudoedit /etc/cloudflared/config.yml        # replace <TUNNEL-UUID> (twice) and <hostname> (twice)
sudo cat /etc/cloudflared/config.yml
cloudflared tunnel --config /etc/cloudflared/config.yml ingress validate                       # [CF-1]
cloudflared tunnel --config /etc/cloudflared/config.yml ingress rule https://<hostname>/api/v1/health   # rule 1
cloudflared tunnel --config /etc/cloudflared/config.yml ingress rule https://other.<zone>/api/runs      # the http_status:404 catch-all (gate G1)
```

### 6.4 The service and the drop-in

`cloudflared service install` installs the systemd service; when run with `sudo`,
`$HOME` is `/root`, so pass the configuration explicitly:
`sudo cloudflared --config <path> service install` `[CF-6]`. The installer copies the
configuration to `/etc/cloudflared/config.yml` (refusing if a different file already
sits there) and writes `/etc/systemd/system/cloudflared.service` from this template
`[CF-7]`:

```ini
[Unit]
Description=Cloudflare Tunnel client
After=network-online.target
Wants=network-online.target

[Service]
TimeoutStartSec=15
Type=notify
ExecStart=/usr/bin/cloudflared --no-autoupdate --config /etc/cloudflared/config.yml tunnel run
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
```

It also writes `cloudflared-update.service` and `cloudflared-update.timer` (daily
`cloudflared update`) unless told not to `[CF-7]`; on this host the updates could not
reach the network and an unattended binary swap is unwanted, so the timer is disabled
(`--no-autoupdate` on the `ExecStart` line already stops in-process updates `[CF-5]`).
The packaged unit has no `User=`, so cloudflared would run as root; the drop-in
`deploy/systemd/cloudflared.service.d/10-consensus.conf` (installed by `install.sh`)
sets `User=cloudflared`, `Group=cloudflared`, `SupplementaryGroups=consensus-sock`
("extends the list of supplementary groups configured in the system group database"
`[SD-1]`) and a few sandbox directives. `consensus-sock` owns nothing but the socket
directory and the socket (section 2.3); cloudflared is **not** in `consensus`, and
`install.sh` removes it from that group if an earlier layout added it (`gpasswd -d`
"Remove the user from the named group" `[UB-5]`). Drop-ins in `UNIT.d/*.conf` are
merged into the unit after the main file and take precedence over it `[SD-5]`.

```sh
sudo cloudflared --config /etc/cloudflared/config.yml service install
systemctl list-unit-files 'cloudflared*'
sudo systemctl disable --now cloudflared-update.timer 2>/dev/null || true
sudo systemctl daemon-reload
sudo systemctl enable --now cloudflared.service
systemctl status cloudflared.service --no-pager
journalctl -u cloudflared -n 30 --no-pager             # "Registered tunnel connection" lines; errors about api.cloudflare.com are the blocked update checks
ps -o user,group,supgrp,cmd -C cloudflared             # user cloudflared, supplementary group consensus-sock (never consensus)
```

`--loglevel debug` prints request and response headers (session cookies among them)
`[CF-5]`; never use it in production.

### 6.5 Changing the configuration

Edit `/etc/cloudflared/config.yml`, `ingress validate`, then
`sudo systemctl restart cloudflared` ("restart the service to load the new
configuration" `[CF-6]`). A restart drops the open SSE streams; clients resume with
`Last-Event-ID` (section 8.5). cloudflared stops accepting new requests on SIGTERM and
waits up to `grace-period` (default `30s`) for in-flight ones `[CF-5]`.

---

## 7. Cloudflare dashboard checklist for `<hostname>`

Work through the list before go-live and record the result in `docs/reviews/`
(gate G10 asks for the owner's sign-off). The zone is Free; limits quoted below are
the Free plan's.

### 7.1 DNS

- [ ] `staging.<hostname>` and, at go-live, `<hostname>`: CNAME to
      `<TUNNEL-UUID>.cfargotunnel.com`, created by `cloudflared tunnel route dns`
      `[CF-10]`, shown as Proxied.
- [ ] No A/AAAA record for either name (the server has no public port).

### 7.2 SSL/TLS

- [ ] Encryption mode **Full (strict)**: "Similar to Full Mode, but with added
      validation of the origin server's certificate" `[CF-30]`. The tunnel's origin is a
      unix socket reached by cloudflared, so this mode governs nothing today; it is
      the safe value should an A record ever be added by mistake. Do not choose
      Flexible or Off.
- [ ] Edge Certificates → **Always Use HTTPS** on (redirects http to https for all
      hosts of the zone) `[CF-30]`.
- [ ] Edge Certificates → **Automatic HTTPS Rewrites** off: it rewrites HTTP URLs to
      HTTPS "in page responses" `[CF-26]`, which breaks byte-equality (gate G6).

### 7.3 Network

- [ ] **IP Geolocation** on: "IP geolocation adds the `CF-IPCountry` header to all
      requests to your origin server" `[CF-16]`; the server's geo check reads that
      header and treats a missing one as unknown, which blocks live mode
      (`live/geo.rs`). The header carries `XX` for clients without country data and
      `T1` for Tor `[CF-15]`; both count as unknown server-side.

### 7.4 WAF managed rules

- [ ] Security → Settings → Web application exploits: the managed ruleset available on
      the Free plan (the Free Managed Ruleset, "Provides protection against
      high-impact and widely exploited vulnerabilities") is deployed; the Security rules
      page then shows a rule with the *Execute* action `[CF-20]`.

### 7.5 WAF custom rules (geo, gate G10) and the rate-limiting rule (gate G3)

Free zones get 5 custom rules, no regular expressions, every action except Log
`[CF-17]`. `ip.src.country` is "The 2-letter country code in ISO 3166-1 Alpha 2
format" (`ip.geoip.country` is deprecated) `[CF-18]`; set membership is written
`in {"CN" "TH"}` with spaces, keywords in lowercase, `and` binding tighter than `or`
`[CF-18]`. The 23 portal codes are `geo::DEFAULT_PORTAL_BLOCKED` and `KR` is
`geo::DEFAULT_LIVE_ONLY_BLOCKED` (`live/geo.rs`); the server enforces the same lists
from `CF-IPCountry` on every request, so the edge rule is the primary control and
the origin the second (`live/geo.rs` module docs).

- [ ] Rule **"Consensus: portal geo-block (Meta Restricted Territories + UA)"**, action
      Block, expression (23 codes, exactly these):

```
(http.host in {"<hostname>" "staging.<hostname>"} and ip.src.country in {"AF" "BY" "KH" "CN" "HK" "MO" "CU" "ER" "ET" "HT" "IR" "IQ" "LY" "MM" "NI" "KP" "PK" "RU" "SO" "SY" "UA" "VE" "EH"})
```

- [ ] Rule **"Consensus: live route KR block"**, action Block, expression:

```
(http.host in {"<hostname>" "staging.<hostname>"} and http.request.uri.path eq "/api/v1/live-runs" and ip.src.country eq "KR")
```

- [ ] Rate limiting rule (Free: 1 rule, counting by IP, a 10 s period, a 10 s
      mitigation timeout, and an expression limited to Path and Verified Bot `[CF-19]`,
      so the method cannot be named; GET on that path is `405` anyway). Security rules →
      Create rule → Rate limiting rules `[CF-19]`: name "Consensus: live-run creations",
      expression `(http.request.uri.path eq "/api/v1/live-runs")`, same characteristics
      IP, when rate exceeds 5 requests per 10 seconds, action Block, duration 10 seconds.
      The server's own limits (6 run creations per minute per client, 3 public live
      runs per client a day, one live slot taken only after Turnstile passed) remain the
      fine-grained control (`public_config.rs`, `live/gate.rs`, ADR-0007).

Verification: from a blocked country (or a VPN exit there) `curl -sS -o /dev/null -w
'%{http_code} %{header_json}' https://<hostname>/` answers `403` with a `cf-ray`
header and no origin headers; eleven fast `POST`s to `/api/v1/live-runs` from one
address answer `429` at the edge within the window (checklist G3: "`cf-ray`, 429 at
the edge").

### 7.6 Turnstile

- [ ] Turnstile → **Add widget**: name "Hyphae Consensus portal", hostnames
      `<hostname>` and `staging.<hostname>`, widget mode **Managed**, pre-clearance
      left off; copy the sitekey (web build) and the secret key (section 4.2) `[CF-21]`.
- [ ] Actions are set by the page, not the widget: `data-action` is "A customer value
      that can be used to differentiate widgets under the same sitekey", up to 32
      alphanumeric characters plus `_` and `-`, returned on validation `[CF-21]`. The
      web client sends `live-run` on the live-run form and `session` on session
      creation; the server checks `action` and `hostname` in every Siteverify answer and
      refuses a token older than 300 s (`live/turnstile.rs`). Siteverify is
      `POST https://challenges.cloudflare.com/turnstile/v0/siteverify` with `secret`,
      `response`, optional `remoteip` and `idempotency_key`; "Each token can only be
      validated once" `[CF-21]` (the egress rule of section 2.4 covers it). At most 8
      Siteverify calls are in flight at once, for all clients together
      (`SITEVERIFY_MAX_IN_FLIGHT`, `live/gate.rs`); past that a live-run request is
      answered `503 turnstile_unavailable` with `Retry-After: 2` and its token is not
      sent, so the visitor can retry with it.

### 7.7 Features that must stay OFF for `<hostname>` (gate G6: edge HTML byte-equal to origin)

Each of these changes the HTML Cloudflare returns, which breaks the per-page CSP
hashes and the byte comparison of section 8.4. Zone-wide toggles affect every
hostname in the zone; if the zone hosts other sites, scope the settings to this
hostname with a Configuration Rule instead (expression `http.host in {"<hostname>"
"staging.<hostname>"}`; the rule can set Automatic HTTPS Rewrites, Email
Obfuscation, Fonts, Rocket Loader, Disable Zaraz and Disable Real User Monitoring
`[CF-29]`).

- [ ] **Rocket Loader** off (Speed → Settings → Content Optimization) `[CF-23]`: it
      defers scripts and needs CSP changes `[CF-23]`.
- [ ] **Cloudflare Fonts** off (same page): "It removes Google Fonts links and replaces
      them with inline CSS" `[CF-25]`.
- [ ] **Email Address Obfuscation** off (Security → Settings → Client-side abuse): it
      is on at signup and "injects a small decode script (`email-decode.min.js`)" `[CF-24]`.
- [ ] **Automatic HTTPS Rewrites** off (section 7.2) `[CF-26]`.
- [ ] **Bot Fight Mode** off (Security → Settings → Bot traffic): "For Bot Fight Mode
      customers, JavaScript Detections is automatically enabled and cannot be disabled",
      and JavaScript Detections inserts "a script tag in all HTML pages served on your
      website" `[CF-27]`; it also "Cannot be customized, adjusted, or reconfigured via WAF
      custom rules" `[CF-27]`. The owner's decision list did not name it; it is listed
      here because it would break G6 silently.
- [ ] **Web Analytics**: do not add the hostname with the automatic setup, which "is
      enabled by default" when a site is added and injects the beacon; if it was added,
      Manage Site → Disable `[CF-28]`.
- [ ] **Zaraz**: no tool configured for the zone; Zaraz loads only when "at least one
      enabled tool" exists and Auto Injection is on `[CF-28]`.

### 7.8 Cache

- [ ] Cache Rules: one rule "Consensus: bypass /api and /research", expression
      `(http.host in {"<hostname>" "staging.<hostname>"} and (http.request.uri.path wildcard "/api/*" or http.request.uri.path wildcard "/research/*"))`,
      Cache eligibility **Bypass cache** `[CF-22]` (`wildcard` matches the whole path
      case-insensitively `[CF-18]`). The server already sends `Cache-Control: no-store`
      on `/api` (`api/mod.rs` `security_headers`). On a bypass "the response header may
      return DYNAMIC rather than explicitly indicating a bypass" `[CF-22]`; `DYNAMIC`
      means the asset was not eligible for cache at request time and went to the origin
      without a cache lookup, `HIT` that it came from Cloudflare's cache `[CF-22]`. Gate
      G6 requires `cf-cache-status` never `HIT` on `/api`; section 8.4 checks it.

### 7.9 Cloudflare Access (staging hostname only)

- [ ] Zero Trust → Access controls → Applications → Create new application →
      Self-hosted and private → Add public hostname: domain `staging.<hostname>`, a
      session duration of the owner's choice, an Allow policy that matches only the
      owner's identity; "All Access applications are deny by default" `[CF-31]`.
      Create it **before** routing `staging.<hostname>` to the tunnel ("We recommend
      creating an Access application before setting up the tunnel route", otherwise
      "the published application will be available to anyone on the Internet") `[CF-31]`.
- [ ] **A service token for the curl probes.** Without one, every unauthenticated
      request to `staging.<hostname>` is answered by Access itself (a `302` to the
      login page, or `401`/`403` in strict mode), so no probe of section 8 could reach
      the origin during the staging phase. Zero Trust → Access controls → Service
      credentials → Service Tokens → Create Service Token: name "consensus staging
      probes", the shortest duration that covers the staging phase; "This is the only
      time Cloudflare Access will display the Client Secret" `[CF-32]`. Keep both values
      on the owner machine only (never on the server).
- [ ] In the staging application, add a second policy: action **Service Auth** ("Make
      sure to set the policy action to Service Auth; otherwise, Access will prompt for
      an identity provider login"), Include → selector **Service Token** → the token
      just created ("The request will need to present the correct service token headers
      configured for the specific application"; the selector "Requires the Service Auth
      action") `[CF-32]`. "Bypass and Service Auth policies are evaluated first" `[CF-32]`;
      the owner's Allow policy stays for the browser.
- [ ] The probes send `CF-Access-Client-Id: <CLIENT_ID>` and
      `CF-Access-Client-Secret: <CLIENT_SECRET>` on every request `[CF-32]`; the scripts
      read them from `CF_ACCESS_CLIENT_ID` and `CF_ACCESS_CLIENT_SECRET` (section 8).
      In strict service-token authentication (on by default for organisations created
      on or after 2026-10-05) Access "does not return a CF_Authorization cookie" and
      refuses with `401`/`403` instead of redirecting `[CF-32]`, which is what the scripts
      report as "edge refused". Browser checks (the Turnstile flow, a guided run) use the
      owner's identity login, not the token.
- [ ] Delete the service token at go-live or at teardown (section 9.7): "To revoke
      access, you must delete the service token" `[CF-32]`.
- [ ] No Access application in front of `<hostname>`: the public portal is anonymous
      by design (ADR-0006 decision 2).

---

## 8. Go-live and verification

Order: staging phase on `staging.<hostname>` behind Access: sections 8.1–8.6 with live
mode off, then on; then the G11 rehearsal of 8.8 (it trips the staging ledger); then
the ledger retirement of section 4.5; then go-live (8.7); then 8.1, 8.3 and 8.4 again
against `<hostname>`. Record every result with its date in `docs/reviews/` (gates G1,
G2, G4, G6, G9, G11 ask for evidence).

Every `https://` request below runs on the **owner machine** (section 2.4 explains why
the server cannot make them). Set these once per shell and keep `H`, `A` and `ACCESS`
for the later sections:

```sh
# owner machine; staging phase: the Access service token of 7.9 (read -rs keeps the values out of the shell history)
H=staging.<hostname>
read -rs -p 'CF-Access-Client-Id: ' CF_ACCESS_CLIENT_ID; echo
read -rs -p 'CF-Access-Client-Secret: ' CF_ACCESS_CLIENT_SECRET; echo
export CF_ACCESS_CLIENT_ID CF_ACCESS_CLIENT_SECRET            # read by verify-edge.sh, canary-scan.sh and sse-heartbeat-test.sh
ACCESS=(-H "CF-Access-Client-Id: $CF_ACCESS_CLIENT_ID" -H "CF-Access-Client-Secret: $CF_ACCESS_CLIENT_SECRET")
curl -sS -o /dev/null -w '%{http_code}\n' "${ACCESS[@]}" "https://$H/api/v1/health"   # 200; a 302 or 401/403 is Access: token or Service Auth policy wrong
# production (8.7 onwards): H=<hostname>; ACCESS=(); unset CF_ACCESS_CLIENT_ID CF_ACCESS_CLIENT_SECRET
```

### 8.1 Gate G1: nothing of the old router, operator never exposed

```sh
# owner machine: the v0 router does not exist in the binary (ADR-0006); every answer is the v1 server's or cloudflared's
curl -sS -o /dev/null -w '%{http_code}\n' "${ACCESS[@]}" "https://$H/api/runs"                 # 404 (JSON "code":"not_found" from the v1 fallback)
curl -sS "${ACCESS[@]}" "https://$H/api/runs"; echo                                              # {"code":"not_found",...}: not a run list
curl -sS "${ACCESS[@]}" "https://$H/" | grep -c 'Operator console'                               # 0: the portal's HTML, not console.html
curl -sS -o /dev/null -w '%{http_code}\n' "${ACCESS[@]}" -X POST -H "Origin: https://$H" \
     -H 'Content-Type: application/json' --data '{}' "https://$H/api/v1/operator/runs"          # 404: no operator route on the public listener (never 401 or 202)
curl -sS -o /dev/null -w '%{http_code}\n' "${ACCESS[@]}" "https://$H/no-such-path-$RANDOM"      # 404 (the export's 404.html)
```

```sh
# server: listeners and ingress
sudo ss -ltnp                                       # TCP: sshd only (and 127.0.0.1:8787 only while the operator console runs)
sudo ss -lxp | grep public.sock                     # the unix socket
cloudflared tunnel --config /etc/cloudflared/config.yml ingress rule https://<hostname>/           # rule 1 -> unix socket
cloudflared tunnel --config /etc/cloudflared/config.yml ingress rule https://other.<zone>/api/runs  # the final http_status:404 rule
systemctl is-enabled consensus-operator.service || true   # static: cannot be enabled
```

### 8.2 Gate G2: two-session isolation

Two cookie jars stand for two browsers. Sessions are minted by
`POST /api/v1/session` (an allowed `Origin` is required on state-changing requests,
`api/mod.rs`); the cookie is `__Host-hc_session` (`Secure`, so only over https). Guided
runs take `{"scenario":"<id>"}` with ids `retry`, `offline`, `invalid_proof`,
`injection`, `abstain` (`guided.rs`, `api/types.rs`). If the live wiring adds a
Turnstile token to session creation, mint session A in a browser and export its
cookie instead.

```sh
# owner machine ($H and ACCESS from the start of section 8)
A=$(mktemp); B=$(mktemp)
curl -sS -c "$A" "${ACCESS[@]}" -X POST -H "Origin: https://$H" "https://$H/api/v1/session"; echo
curl -sS -c "$B" "${ACCESS[@]}" -X POST -H "Origin: https://$H" "https://$H/api/v1/session"; echo
curl -sS -b "$A" "${ACCESS[@]}" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' \
     --data '{"scenario":"retry"}' "https://$H/api/v1/runs" | tee /tmp/run-a.json; echo
RUN=$(sed -n 's/.*"run_id":"\([^"]*\)".*/\1/p' /tmp/run-a.json)   # or read run_id from the 202 body
# owner (A) reads everything; a stranger (B) gets 404 on every route and never sees the run in a list
for p in "" /events.ndjson /receipt; do
  printf 'A %-16s %s\n' "$p" "$(curl -sS -o /dev/null -w '%{http_code}' -b "$A" "${ACCESS[@]}" "https://$H/api/v1/runs/$RUN$p")"
  printf 'B %-16s %s\n' "$p" "$(curl -sS -o /dev/null -w '%{http_code}' -b "$B" "${ACCESS[@]}" "https://$H/api/v1/runs/$RUN$p")"   # 404
done
curl -sS -o /dev/null -w 'B events %{http_code}\n' -b "$B" "${ACCESS[@]}" --max-time 5 "https://$H/api/v1/runs/$RUN/events"   # 404
curl -sS -o /dev/null -w 'B cancel %{http_code}\n' -b "$B" "${ACCESS[@]}" -X POST -H "Origin: https://$H" \
     -H 'Content-Type: application/json' --data '{}' "https://$H/api/v1/runs/$RUN/cancel"                 # 404
curl -sS -b "$B" "${ACCESS[@]}" "https://$H/api/v1/runs" | grep -c "$RUN"                                  # 0
rm -f "$B" /tmp/run-a.json          # keep $A: sections 8.5 and 8.8 reuse it
```

### 8.3 Gate G4: the key is in no environment, file, log or response

systemd keeps the credential in `/run/credentials/consensus-public.service/`,
accessible to the service user only `[SD-1]`; the server reads it from
`CONSENSUS_META_API_KEY_FILE` and keeps it in memory, zeroized on drop, never
printed (`config.rs`). The scan below never prints a match and never reads
`/etc/consensus/credentials/`.

```sh
# server
pid=$(systemctl show -p MainPID --value consensus-public.service)
sudo tr '\0' '\n' < /proc/$pid/environ | grep -c -E 'LLM_[0-9]+_'                      # 0
sudo tr '\0' '\n' < /proc/$pid/environ | grep -E '^(CONSENSUS_META_API_KEY|MODEL_API_KEY|META_API_KEY)=' ; echo "exit $? (1 = none set, as required)"
sudo tr '\0' '\n' < /proc/$pid/environ | grep -E '^CREDENTIALS_DIRECTORY='              # the path only
sudo ls -ld /run/credentials/consensus-public.service                                   # accessible to consensus only
sudo ~/consensus/deploy/scripts/canary-scan.sh                                          # environments, store, ledger tree, web, env files, journal
# staging only, with a canary key file (section 4.2): add --canary '<the canary string>'
```

```sh
# owner machine: the public responses (the server cannot fetch its own hostname, section 2.4)
deploy/scripts/canary-scan.sh --responses-only --hostname "$H"                          # on staging add --canary '<the canary string>'
```

The response half reports `UNREACHABLE` and fails when a page cannot be fetched (an
earlier version printed `ok` for a failed fetch, because zero matches in no body look
like zero matches); run it from a machine that reaches the hostname.

Also on the owner machine: `git check-ignore -v .env .env.local meta.key` lists the
ignore rules (`.gitignore`: `.env*`, `*.key`).

### 8.4 Gate G6: edge versus origin, byte for byte; `cf-cache-status`

`deploy/scripts/verify-edge.sh` runs on the owner machine. It fetches `/`, `/en/`,
`/es/`, `/pt-BR/`, `/fr/`, an unknown path, `/api/v1/health`, `/api/v1/capabilities`,
`/api/v1/scenarios`, `/research/MANIFEST.json` and `/research/SHA256SUMS` twice: from
the origin over SSH (`ssh <server> sudo -n curl --unix-socket ...` with the `Host`,
`CF-Connecting-IP` and `CF-IPCountry` headers the origin expects; `sudo -n` "will
display an error message and exit" instead of prompting `[UB-5]`) and from the hostname
through Cloudflare, compares with `cmp`, and prints `cf-cache-status` and the security
headers (`X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`,
`X-Frame-Options: DENY`, `Content-Security-Policy` from the manifest; `api/mod.rs`,
`static_files.rs`). Each side is one request whose body is followed by curl's
three-digit status, split with `head -c -3` / `tail -c 3` ("with the leading '-',
print all but the last NUM bytes" `[UB-5]`), so nothing touches the bytes; for the same
reason the SSH session has no pseudo-terminal (`ssh -t` would translate line endings).

```sh
# owner machine
deploy/scripts/verify-edge.sh --hostname "$H" --ssh <server>
# by hand, one page:
ssh <server> "sudo -n curl -sS --unix-socket /run/consensus/public.sock -H 'Host: $H' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' http://localhost/en/" > /tmp/origin.html
curl -sS "${ACCESS[@]}" "https://$H/en/" -o /tmp/edge.html -D /tmp/edge.h
cmp /tmp/origin.html /tmp/edge.html && echo "byte-equal"
grep -i cf-cache-status /tmp/edge.h                     # DYNAMIC (not HIT) for HTML too: not a default-cached type [CF-22]
curl -sS -D - -o /dev/null "${ACCESS[@]}" "https://$H/api/v1/health" | grep -i -E 'cf-cache-status|cache-control'   # DYNAMIC; no-store
```

If `sudo -n` fails over SSH ("a password is required"), add the SSH user to
`consensus-sock` for the duration of the checks (`sudo gpasswd -a <user> consensus-sock`
on the server, `gpasswd -d` afterwards `[UB-5]`; a new SSH session picks up the group)
and pass `--no-sudo`: the group means "may connect to the public socket", which is
exactly what the probe does (open question 7).

Any `DIFFER` means an edge feature of section 7.7 is still on (or a challenge page was
served); fix the setting and re-run. The Playwright part of G6 (CSP violations across
pages × locales × themes, the Turnstile flow) is the web lane's.

### 8.5 SSE through the tunnel: heartbeat and resume

Cloudflare's Proxy Read Timeout is 125 s, answered with `524`, configurable only for
Enterprise zones `[CF-14]`; the stream sends a `: heartbeat` comment every 15 s
(`CONSENSUS_SSE_HEARTBEAT_SECS`, `stream.rs`), so the edge never waits that long.
A stream begins with `: stream-start` and a `retry: 3000` hint, replays the durable
log from the client's `Last-Event-ID` (or `?after`), follows live frames, re-syncs on
a gap, marks every frame `replay` or `live`, and ends with `event: stream_end` once
the run awaits a decision or is terminal (`stream.rs`, `api/v1.rs` `resume_point`).
Because the stream lives only while the run deliberates, the 8-minute test needs a
live run on staging (guided runs finish in seconds). Live runs cost about $0.81 on
average (ADR-0007) **on the dedicated key** (there is no separate staging key: the
dedicated key is the only one allowed on the server), and every one of them is a
reservation and a settlement in the staging ledger (section 4.5, rule 1). The test runs
on the owner machine; the cloudflared restart of the resume step is the one command
that runs on the server.

```sh
# owner machine
# 1. a session that owns a live run (judge lane or public lane on staging), its cookie jar in $A, the run id in $RUN (8.2 shows the pattern;
#    the live-run body is {"preset_id":"<preset>"} plus the Turnstile and attestation fields the wiring adds; mint it in the browser if needed)
# 2. follow it for 8 minutes with timestamps; PASS = no silence over 35 s and no disconnect before stream_end or the deadline
deploy/scripts/sse-heartbeat-test.sh --hostname "$H" --run "$RUN" --cookie-jar "$A" --seconds 480
# 3. resume: with a second stream open in the background...
deploy/scripts/sse-heartbeat-test.sh --hostname "$H" --run "$RUN" --cookie-jar "$A" --seconds 600 --log /tmp/sse-before.log &
ssh <server> sudo systemctl restart cloudflared          # ...the open stream drops (curl exits early: expected)
wait; grep -c 'id: ' /tmp/sse-before.log               # the last id is printed by the script
deploy/scripts/sse-heartbeat-test.sh --hostname "$H" --run "$RUN" --cookie-jar "$A" --seconds 120 --resume <last-id> --log /tmp/sse-after.log
grep -m3 -E 'event: (consensus|resync)|"delivery"' /tmp/sse-after.log   # replay from <last-id>+1 ("delivery":"replay"), then live frames
```

(The script reads `CF_ACCESS_CLIENT_ID`/`CF_ACCESS_CLIENT_SECRET` for the staging
hostname and reports "no SSE frame at all" when Access or the WAF answered instead of
the origin.)

If the run ends before eight minutes, start another and add the durations: the
requirement is one continuous stream of at least 480 s or, failing that, several
streams each crossing a model-call silence longer than the 125 s edge limit with
heartbeats only.

### 8.6 Gate G9: Hyphae on the public server

```sh
# server
pgrep -a hyphae || echo "no hyphae process"                          # no daemon
sudo ss -lxp | grep -c hyphae                                          # 0
grep -E '^(HYPHAE_|CONSENSUS_HYPHAE_(PERSONAL|WORK|JOURNAL))' /etc/consensus/public.env || echo "no Hyphae daemon settings"
sudo find / -xdev \( -name 'hyphae.sock' -o -path '*/.config/hyphae*' -o -path '*/.local/state/hyphae*' \) 2>/dev/null   # nothing
getent group docker || echo "no docker group"; command -v docker || echo "docker not installed"
ls -la /opt/hyphae/bin/ /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae      # owned by hyphae, 0755
stat -c '%A %U:%G %n' /var/lib/consensus/snapshots                                     # drwx------ consensus:consensus: witnesses reachable by the service user only
# no writer path: inside the service's mount namespace the snapshot directory is read-only (ProtectSystem=strict)
pid=$(systemctl show -p MainPID --value consensus-public.service)
sudo nsenter -t "$pid" -m -- touch /var/lib/consensus/snapshots/.probe 2>&1 | grep -q 'Read-only' && echo "snapshots: read-only in the sandbox"
sudo nsenter -t "$pid" -m -- touch /var/lib/consensus/runs/.probe && sudo rm /var/lib/consensus/runs/.probe && echo "runs: writable (expected)"
# the verifier INSIDE the sandbox: hyphae proof verify under the unit's own directives (deploy/scripts/sandbox-probe.sh)
sudo ~/consensus/deploy/scripts/sandbox-probe.sh                                       # PASS: exit 0, report status "verified", semantic_reexecution_performed true
sudo ~/consensus/deploy/scripts/sandbox-probe.sh -- /opt/hyphae/bin/hyphae --version   # any other command under the same sandbox
# boot verification (live mode enabled) and readiness
journalctl -u consensus-public -b --no-pager | grep -E 'Sealed snapshot evidence|\[snapshot\]|Public live mode'
sudo curl -sS --unix-socket /run/consensus/public.sock -H 'Host: <hostname>' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' http://localhost/api/v1/ready; echo
```

**What a good boot shows.** With live mode enabled, the journal has `Sealed snapshot evidence:
<snapshot-id> (project demo-repo, 7 records, anchor <12 hex>…, manifest sha256 pinned <12
hex>…, verifier binary sha256 pinned <12 hex>…, captured <time>, proofs re-verified at boot
<time>)` and `Public live mode: enabled on the owner's key (...)`, no `[snapshot]` line, and
`/api/v1/ready` answers `"ready":true` with `"evidence_ok":true`. The two pinned prefixes must
be those of the published manifest and binary digests (sections 3.2, 3.3); the line shows
prefixes only, never a whole digest or a path. For `snap-2026-10-29-demo-repo` the
prefixes are anchor `43a2dea9c48d…`, manifest `b731346c8464…` and binary `79025e8758ee…`;
the boot check of 2026-10-29 (`0515f29`, development machine, verifier binary not pinned)
printed the first two and `captured 2026-10-29T17:16:12Z`. `manifest not pinned
(CONSENSUS_SNAPSHOT_MANIFEST_SHA256 unset)` or `verifier binary not pinned
(CONSENSUS_HYPHAE_BIN_SHA256 unset)` in its place means that pin is missing from
`public.env`: live mode runs, but nothing at runtime ties the directory or the binary to the
published digests; set it before go-live. With
live mode disabled there is no snapshot line at all, or, when `CONSENSUS_SNAPSHOT_DIR` is set
anyway, the one notice `[snapshot] CONSENSUS_SNAPSHOT_DIR is set, but public live mode is
disabled (CONSENSUS_LIVE_ENABLED): the snapshot is not loaded or verified`.

**What a failed boot check shows.** A snapshot that does not load, verify or match does not
stop the server. The journal has `[snapshot] live evidence unavailable: <code>` (the only
line that names the code) and `Public live mode: enabled on the owner's key (...); its
evidence is UNAVAILABLE (the [snapshot] line says why): every live run is refused (503
live_unavailable) until the snapshot is fixed and the server restarted`; refused admissions
add `[live-gate] refused a live run: the evidence source is not ok`; `/api/v1/ready` answers
`"ready":false` with
`"live":{"ledger_ok":true,"tripped":false,"evidence_ok":false}`; `/api/v1/capabilities`
answers `"live_enabled":false` without a live section, so the web app shows live mode as
disabled; `POST /api/v1/live-runs`
answers `503 live_unavailable` before Turnstile (no token spent, nothing reserved). Guided
runs, the showcase, the research files and the static site keep serving. The code says what
to fix, then `sudo systemctl restart consensus-public`:

| Code | Meaning | Remedy |
|---|---|---|
| `snapshot_dir_missing` | `CONSENSUS_SNAPSHOT_DIR` names no directory (not copied yet) | copy the snapshot (section 3.3) |
| `checksums_invalid`, `checksums_incomplete`, `checksums_mismatch`, `digest_mismatch`, `file_unreadable`, `file_too_large`, `symlink_refused`, `layout_too_deep` | the directory is not the captured one byte for byte (`digest_mismatch` while `sha256sum -c SHA256SUMS` passes: a capture file, a record text say, was changed and the listing regenerated over it) | re-copy it from the owner machine and run `sha256sum -c SHA256SUMS` in it (section 3.3) |
| `manifest_pin_mismatch` | the SHA-256 of `manifest.json` is not `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`: this is not the published snapshot, or the pin is wrong | re-copy the published snapshot (section 3.3), or correct the pin to the digest the README and the release tag publish; never pin a digest computed from the copy in doubt |
| `pinned_anchor_mismatch` | `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` is not the manifest's `anchor_hex` | set the anchor published with the snapshot |
| `verifier_binary_mismatch` | the file `CONSENSUS_HYPHAE_BIN` names is missing, unreadable, not a regular file, or does not hash to `CONSENSUS_HYPHAE_BIN_SHA256` (nothing ran it) | reinstall the release binary (section 3.2) and compare its `sha256sum` with the pin |
| `capture_binary_mismatch` | the pinned binary is not the build the snapshot was captured with (`manifest.hyphae.binary_sha256`) | install the capture's build (section 3.2), or deploy a snapshot captured with the installed one |
| `project_mismatch` | the snapshot serves another project than the presets' `demo-repo` | deploy the `demo-repo` snapshot |
| `boot_verification_failed` | `hyphae proof verify` refused a served capture, or its digests are not the capture-time ones | run `sandbox-probe.sh` (above) on the deployed directory; a sandbox error is a unit change under gate G9 |
| any other `manifest_*`, `capture_*`, `response_invalid`, `memory_ids_mismatch`, `foreign_record`, `store_mismatch`, `verify_record_invalid`, `proof_shared` | the manifest or a capture is not the documented one | re-copy the snapshot; `python -I scripts/snapshot_manifest.py check <dir>` on the owner machine names the file |

The same state follows any change to the served files after boot (a response, proof,
witness, capture-time report or `manifest.json` deleted, truncated or replaced; an edited
record text included): the next status read (`/ready`,
`/capabilities` or an admission; the routes reuse a read for at most two seconds, an
admission always re-hashes) logs `[snapshot] live evidence unavailable: <code>` and turns
live mode unavailable, and the first read after the files are the verified ones again logs
`[snapshot] live evidence ok again`, without a restart. A run whose files are swapped
under it records its proofs `invalid` and is blocked. The verifier binary is not re-hashed
after boot: replacing it takes a restart to be checked. A configuration
error (live mode enabled with `CONSENSUS_SNAPSHOT_DIR` unset, a missing or malformed
`CONSENSUS_HYPHAE_EXPECTED_ANCHOR`, a `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` or
`CONSENSUS_HYPHAE_BIN_SHA256` that is not 64 hex digits, a relative `CONSENSUS_HYPHAE_BIN`
with the binary pinned) still stops startup with `error: <VARIABLE>: ...`
(`journalctl -u consensus-public -n 50`).

**Why the probe exists.** Guided runs never call Hyphae (`guided.rs` uses
`FakeProofVerifier`); the real `hyphae proof verify` runs for live runs
(`HyphaeProofVerifier`, `api/v1.rs`, `hyphae.rs`) and, with live mode enabled, at boot
(`live/snapshot_factory.rs`, `snapshot.rs` `SnapshotMemoryClient::from_snapshot`: the two
served captures are re-verified and the journal carries `Sealed snapshot evidence: <id>
(... proofs re-verified at boot <time>)`, or `[snapshot] live evidence unavailable:
boot_verification_failed` with live mode unavailable, as above).
Without the probe, the first time the verifier would meet `ProtectSystem=strict`,
`ProtectHome=yes` (`$HOME=/var/lib/consensus` is read-only to it), `PrivateTmp`,
`MemoryDenyWriteExecute`, the empty capability set and `SystemCallFilter=@system-service`
would be the first paying live run. `sandbox-probe.sh` reads the installed unit's
`[Service]` section and starts a transient unit with `systemd-run --wait --pipe --collect`
and one `-p` per directive ("Sets a property on the scope or service unit that is
created"; "the return value will be propagated from the service" `[SD-8]`), all but
`Type`, `Exec*`, `Kill*`, `Timeout*`, `Restart*`, the credentials and
`RuntimeDirectory*`, as the service user, and runs the exact
`hyphae proof verify --proof <capture>.proof --witness <capture>.witness --anchor <anchor>`
of `hyphae.rs` on one capture of `CONSENSUS_SNAPSHOT_DIR`. PASS requires exit 0 and a
report with `"status":"verified"` and `"semantic_reexecution_performed":true`, the same
two facts `hyphae.rs` demands before it calls a proof `Valid`. A FAIL prints the
verifier's stderr: `Read-only file system` or `Operation not permitted` names a
sandbox directive; the remedy is a unit change recorded under gate G9 (for example a
dedicated writable directory for the verifier's state), never a quiet loosening.
`systemd-run` answering `Unknown assignment` would name a directive this systemd cannot
set on a transient unit; none of the unit's is expected to be one.

`/api/v1/ready` reports the run store (index head, tampering, tail repair), the
showcase status, active and queued runs and `shutting_down` (`api/v1.rs` `ready`), and,
with live mode enabled, `live.ledger_ok`, `live.tripped` and `live.evidence_ok`: the
boot check of ADR-0008 decision 1 (kind, scope, anchor, binding, the digest pins) and the
files still being the verified ones. It carries the boolean only (a pin that does not hold
reads `"evidence_ok":false` like any other boot failure); the reason code is in the journal
(`[snapshot] ...`, above). `/api/v1/health` is static facts only: version, schema,
policy, `contract_sha256`, `executor: disabled` (`api/types.rs`).

### 8.7 Go-live

1. Every gate G1–G12 reads PASS with evidence (`docs/security/release-checklist.md`),
   the G11 rehearsal of 8.8 included, and the staging ledger has been retired (4.5).
2. `public.env`: `CONSENSUS_LIVE_LEDGER_DIR=/var/lib/consensus/ledger/public` (empty:
   no `lifetime.json` yet), `CONSENSUS_TURNSTILE_HOSTNAME=<hostname>`,
   `CONSENSUS_LIVE_ENABLED=true` (if live mode launches with the portal),
   `CONSENSUS_LIVE_JUDGE_CODES_FILE` set, `CONSENSUS_LIVE_LIFETIME_USD=60.00` (open
   question 6), `CONSENSUS_SNAPSHOT_DIR` and `CONSENSUS_HYPHAE_EXPECTED_ANCHOR`
   uncommented (section 3.3); the real key in `meta.key`; `sudo systemctl restart
   consensus-public` and read `journalctl -u consensus-public -n 30` for a clean start:
   the `Sealed snapshot evidence:` line of 8.6 and no `[snapshot] live evidence
   unavailable` line (a snapshot that did not boot keeps the portal up with live mode
   off: fix it per the table in 8.6 and restart), then `/api/v1/ready` with
   `"evidence_ok":true`.
3. Owner machine: `cloudflared tunnel route dns consensus-public <hostname>`.
4. Owner machine: `H=<hostname>; ACCESS=()` (no Access on the production hostname),
   then repeat 8.1, 8.3 (`--responses-only`) and 8.4 (`--ssh <server>`) against
   `<hostname>`; one guided run end to end in a browser; one live run if enabled, after
   which `kill-switch.sh status` on the server shows the production ledger's first
   records and `tripped: false`.
5. Delete the staging service token (7.9) unless staging stays in use, and record the
   go-live time (UTC) in `docs/journal.md`.

### 8.8 Gate G11: shutdown, recovery, kill switch, export (staging rehearsal)

Run this **last** in the staging phase, on the staging ledger: the kill-switch step
trips that ledger, the reset step clears the trip (section 4.4), and section 4.5
retires the ledger before go-live anyway.
Live mode must be on (`CONSENSUS_LIVE_ENABLED=true` on staging, with a binary that
has the live wiring) for the kill-switch step to mean anything: before the wiring,
`POST /api/v1/live-runs` answers `503 live_disabled` whether or not a ledger is tripped,
`serve-public` opens no ledger, and G11 stays open. Two terminals: an SSH session on
the server and a shell on the owner machine with `$H`, `$A` and `"${ACCESS[@]}"` from
the start of section 8.

```sh
# 1. graceful stop during a run: the server cancels active runs and exits within TimeoutStopSec
curl -sS -b "$A" "${ACCESS[@]}" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{"scenario":"retry"}' "https://$H/api/v1/runs" >/dev/null   # owner machine
sudo systemctl stop consensus-public.service                                                                  # server, at once
journalctl -u consensus-public -n 20 --no-pager | grep -E 'shutdown|cancell'      # server: "[shutdown] stopping: cancelling active runs"
sudo systemctl start consensus-public.service                                     # server
# 2. crash recovery: a SIGKILL mid-run; Restart=on-failure brings it back and recovery closes the run as failed, stage interrupted
curl -sS -b "$A" "${ACCESS[@]}" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{"scenario":"retry"}' "https://$H/api/v1/runs" | tee /tmp/run-k.json >/dev/null   # owner machine
sudo systemctl kill --signal=SIGKILL consensus-public.service; sleep 8            # server, at once
journalctl -u consensus-public -n 20 --no-pager | grep recovery                    # server: "[recovery] closed 1 interrupted run(s) as failed"
curl -sS -b "$A" "${ACCESS[@]}" "https://$H/api/v1/runs/$(sed -n 's/.*"run_id":"\([^"]*\)".*/\1/p' /tmp/run-k.json)" | grep -o '"stage":"[a-z_]*"'   # owner machine: interrupted
# 3. kill switch: trips the STAGING ledger at the next admission; live-run creation is refused; guided runs and the showcase stay up
sudo ~/consensus/deploy/scripts/kill-switch.sh on                                 # server
curl -sS -o /dev/null -w '%{http_code}\n' -b "$A" "${ACCESS[@]}" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' \
     --data '{"preset_id":"<preset>"}' "https://$H/api/v1/live-runs"              # owner machine: 503 with a stable code, never 202
curl -sS -o /dev/null -w '%{http_code}\n' -b "$A" "${ACCESS[@]}" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' \
     --data '{"scenario":"retry"}' "https://$H/api/v1/runs"                       # owner machine: 202, guided runs unaffected
sudo ~/consensus/deploy/scripts/kill-switch.sh status                             # server: tripped: true, trip_code: kill_switch, lifetime_spent_usd
sudo ~/consensus/deploy/scripts/export-ledger.sh                                  # server: today's day file of the staging ledger holds the trip record
sudo ~/consensus/deploy/scripts/kill-switch.sh off                                # server: removes the file; the staging ledger stays tripped
sudo ~/consensus/deploy/scripts/ledger.sh status | grep -E '"tripped"|"trip_code"'   # server: still "tripped": true, "trip_code": "kill_switch"
# 4. reset: refused while the file is present, then clears the trip without a restart
sudo ~/consensus/deploy/scripts/kill-switch.sh on && sudo ~/consensus/deploy/scripts/ledger.sh reset --by <owner>; echo "exit $?"   # server: exit 2, "the kill-switch file ... is present"
sudo ~/consensus/deploy/scripts/kill-switch.sh off --yes
sudo ~/consensus/deploy/scripts/ledger.sh reset --by <owner>                         # server: "reset": true, "cleared": {"trip_code": "kill_switch", ...}, "snapshot": {"tripped": false, ...}
curl -sS -b "$A" "${ACCESS[@]}" "https://$H/api/v1/ready"                         # owner machine: live.tripped false within the cache age
```

Then retire the staging ledger exactly as section 4.5 shows (export, stop, move to
`ledger-archive/`, point `public.env` at `ledger/public`) and continue with 8.7. The
written result for G11 names the archived directory, its `lifetime_spent_usd` and the
`reset` record of step 4.

Retention (bounded guided-run storage) shows as `[retention] purged N expired run(s)`
in the journal every 10 minutes when something expired (`server.rs` `run_server`).

---

## 9. Operations

### 9.1 Logs

```sh
journalctl -u consensus-public.service -f                   # live
journalctl -u consensus-public.service -u cloudflared.service --since -1d -p warning --no-pager
journalctl -u consensus-operator.service --since -7d --no-pager
```

What the server writes: startup facts (schema, contract digest, listener), recovery
and retention counts, `[shutdown]` notices, and internal-error details with a stable
code for the client (`api/error.rs` `internal`). Never a key (`config.rs`: the key is
`<redacted>` in every `Debug` output and no `Display` exists), never a Turnstile
secret or a judge code (`live/turnstile.rs`, `live/judge.rs`), and with
`CONSENSUS_OPERATOR_TOKEN_FILE` set the operator token is never printed
(`server.rs`). Public responses carry fixed codes and messages, never upstream bodies
or paths (ADR-0006 decision 5). cloudflared at `--loglevel info` logs connections,
not headers `[CF-5]`.

### 9.2 Daily check (five minutes)

```sh
# server
systemctl status consensus-public.service cloudflared.service --no-pager | grep -E 'Active|Main PID'
sudo curl -sS --unix-socket /run/consensus/public.sock -H 'Host: <hostname>' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' http://localhost/api/v1/ready; echo   # "ready":true; "evidence_ok":false is section 8.6
journalctl -u consensus-public.service --since -1d --no-pager | grep '\[snapshot\]'     # nothing, or the evidence's transitions (8.6)
df -h / && free -h && journalctl --disk-usage
journalctl -u consensus-public.service --since -1d -p warning --no-pager | tail -n 20
sudo ~/consensus/deploy/scripts/kill-switch.sh status                                   # tripped: false; lifetime_spent_usd
ledger=$(grep -E '^CONSENSUS_LIVE_LEDGER_DIR=' /etc/consensus/public.env | cut -d= -f2-)
sudo tail -n 5 "$ledger/$(date -u +%F).jsonl" 2>/dev/null                               # today's reservations and settlements (no PII: ids and amounts)
# egress allowlist still valid? (section 2.4)
for host in api.meta.ai challenges.cloudflare.com; do getent ahostsv4 "$host" | awk '{print $1}' | sort -u | sed "s/^/$host /"; done
sudo ufw status | grep -E 'api.meta.ai|challenges.cloudflare.com'
```

```sh
# owner machine: the edge (the server cannot reach its own hostname, section 2.4)
curl -sS -o /dev/null -w 'edge %{http_code} %{time_total}s\n' https://<hostname>/api/v1/health
```

The ledger through the operator console, over SSH (never through the tunnel):

```sh
# owner machine: forward the loopback console
ssh -L 8787:127.0.0.1:8787 <server>
# on the server, in that session
sudo systemctl start consensus-operator.service
sudo cat /etc/consensus/credentials/operator.token   # paste as the bearer token in the console at http://127.0.0.1:8787/
# ... when done
sudo systemctl stop consensus-operator.service
```

The console accepts `Host` values `127.0.0.1:8787` and `localhost:8787` and nothing
else (`public_config.rs`), so the forwarded port must be 8787. Every console
`model=meta` run is metered on the same ledger, on the operator lane (gate G3,
`server.rs` `operator_ledger`): its worst case is reserved before it starts and counts
against the daily and lifetime caps the public lane shares, every Meta call is drawn and
settled, and the kill switch or any trip refuses it with the ledger's code
(`live_tripped`, `live_budget_daily`, ...). Check the console's startup line:

```sh
journalctl -u consensus-operator.service -n 20 --no-pager | grep 'Operator model=meta runs'
# "metered on the live ledger at /var/lib/consensus/ledger/<phase> (...)"; an "UNMETERED" line
# means CONSENSUS_LIVE_LEDGER_DIR did not reach the unit: stop the console and fix public.env
```

Until the console exposes the ledger snapshot, the files above are the record: one JSONL
per UTC day plus `lifetime.json` (`live/budget.rs`); an operator run's reservation carries
`"lane":"operator"`.

### 9.3 Kill switch, reset and reconciliation

```sh
sudo ~/consensus/deploy/scripts/kill-switch.sh on        # creates <ledger dir>/kill-switch (root:consensus 0640) in the active ledger of public.env
sudo ~/consensus/deploy/scripts/kill-switch.sh status
sudo ~/consensus/deploy/scripts/kill-switch.sh off       # asks; removes the file only: the ledger stays tripped
sudo ~/consensus/deploy/scripts/ledger.sh status         # JSON: the operator snapshot (trip reason included) and every open reservation
sudo ~/consensus/deploy/scripts/ledger.sh reset --by <operator>                     # clears a trip; refused while the kill-switch file exists
sudo ~/consensus/deploy/scripts/ledger.sh release <reservation-id> --by <operator>  # ends a reservation a crash left open
```

Semantics (`live/budget.rs`, `ledger_cli.rs`): the file's presence trips the ledger at
the next admission; a tripped ledger refuses every reservation and every draw, so
in-flight runs stop at their next model call, the operator console's `model=meta` runs
included (they are metered on the same ledger, section 9.2); the trip is persistent,
survives restarts, and a file still present re-trips it. **Only an operator reset clears a
trip** (section 4.4): `off` only removes the file; `ledger.sh reset` re-opens live mode
on the same ledger once the reason for the stop is written up. Guided runs and the
showcase are unaffected. The ledger also trips on its own on 401/402/403 from Meta, on
an overrun of a bound, and at the lifetime cap; reset those the same way once the cause
is fixed (a replaced key, a corrected bound). A lifetime-cap trip re-opens nothing: at
the cap every reservation is refused anyway. While live mode is disabled no ledger is
open, so a file created then takes effect at the first admission after live mode is
enabled.

Reconciliation after a crash: a run killed mid-call leaves its reservation open,
counting against the caps at its worst case (`ledger.sh status` lists it under
`open_reservations`, with what it drew so far); restart recovery closes the run as
failed but does not release the reservation. Once the run is closed and the provider
dashboard shows what it really spent, `ledger.sh release <id> --by <operator>` ends
the reservation at the spend the ledger recorded (settled charges plus unsettled
draws, never refunded). Never release the reservation of a run still in progress: its
next Meta call is refused.

### 9.4 Rotating the operator token

```sh
sudo systemctl stop consensus-operator.service 2>/dev/null || true
openssl rand -hex 32 | sudo tee /etc/consensus/credentials/operator.token >/dev/null      # the file keeps root:root 0600
sudo ls -l /etc/consensus/credentials/operator.token
```

The token is read at the console's start (from the `LoadCredential=` copy, which exists
only while the unit runs), so stop it first; nothing else holds it.

### 9.5 Backups

Back up `/var/lib/consensus/runs` and `/var/lib/consensus/ledger` only. Never the
snapshot directory (its witnesses are private operator artifacts and the owner
already holds the capture), never `/etc/consensus/credentials`.
`deploy/scripts/export-ledger.sh --include-runs` writes a tarball of the active ledger
directory (`CONSENSUS_LIVE_LEDGER_DIR` from `public.env`; `--ledger-dir
/var/lib/consensus/ledger` takes every ledger at once) and the run store, excluding
`*.witness` and `*.proof` as a safeguard, with its sha256, under
`/root/consensus-exports/` (`0700`); the archive name carries the ledger directory's
name (`consensus-ledger-public-runs-<stamp>.tar.gz`):

```sh
sudo ~/consensus/deploy/scripts/export-ledger.sh --include-runs
# owner machine
scp '<server>:/root/consensus-exports/consensus-ledger-public-runs-*.tar.gz*' . && sha256sum -c consensus-ledger-public-runs-*.tar.gz.sha256
```

Run records are per-run hash-chained logs plus a run index (ADR-0005); a visitor's run
is deleted whole after 7 days except its one-line index entry and its ledger entry
(ADR-0007 decision 7), so a backup taken later holds less by design. Daily during
exposure is enough; the ledger is append-only.

### 9.6 Incident response

Suspected key exposure, unexpected spend, or a compromised host:

1. `sudo ~/consensus/deploy/scripts/kill-switch.sh on` — spending stops at the next
   admission; in-flight runs stop at their next call. The trip holds until an operator
   reset (section 4.4); live mode re-opens, if at all, after the write-up: with
   `ledger.sh reset` on the same ledger, or on a new `ledger/public-<n>` when the ledger
   itself is suspect, the old one archived (section 4.5).
2. Revoke the dedicated key at dev.meta.ai (release checklist, owner step 5). The ledger
   also trips by itself on the first 401/403 the revoked key receives.
3. If the host is suspect: `sudo systemctl stop consensus-public.service cloudflared.service`
   (the portal goes dark; the DNS record answers error 1016 while the tunnel is down `[CF-10]`).
4. Keep everything: `sudo ~/consensus/deploy/scripts/export-ledger.sh --include-runs`,
   then `journalctl -u consensus-public.service -u cloudflared.service --since '<time>' -o short-iso --no-pager > /root/consensus-exports/incident-$(date -u +%Y%m%dT%H%M%SZ).log`
   (the journal holds no secrets, section 9.1).
5. Rotate what the host held: a new dedicated key (section 4.2), a new operator token
   (9.4), a new Turnstile secret (Turnstile → widget → Settings), a new tunnel credentials
   file (`cloudflared tunnel delete` and `create` from the owner machine, which holds
   `cert.pem` `[CF-9]`), then re-run sections 8.1–8.4.
6. Write the incident up in `docs/reviews/` with the ledger figures.

### 9.7 Post-judging teardown (about 2026-11-10)

1. Revoke the dedicated key at dev.meta.ai; confirm the ledger's last day file and
   `lifetime.json` (`kill-switch.sh status`).
2. `sudo ~/consensus/deploy/scripts/export-ledger.sh --include-runs` (the production
   ledger and the runs) and `sudo ~/consensus/deploy/scripts/export-ledger.sh
   --ledger-dir /var/lib/consensus/ledger-archive` (the retired staging ledger, if its
   export of section 4.5 was not kept); copy the tarballs to the owner machine (section
   9.5): the ledger export the checklist asks for, and the run archive.
3. `sudo systemctl disable --now consensus-public.service`;
   `sudo systemctl disable --now cloudflared.service`.
4. `sudo shred -u /etc/consensus/credentials/meta.key /etc/consensus/credentials/turnstile.secret /etc/consensus/credentials/operator.token /etc/consensus/judge-codes.sha256`
   and `sudo rm -rf /run/credentials/consensus-public.service 2>/dev/null || true`.
5. Owner machine: `cloudflared tunnel delete consensus-public` (the account certificate
   manages tunnels `[CF-9]`); remove the two CNAME records, the Access application and
   its service token (7.9), the Turnstile widget and, if wanted, the WAF, rate-limit and
   cache rules.
6. Keep `research/` (it lives in the repository) and the exported ledger; publish the
   ledger summary with the submission notes if the owner wishes. The snapshot directory
   on the server may be deleted (`sudo rm -rf /var/lib/consensus/snapshots`): the owner
   holds the capture.

### 9.8 Judge codes and the daily quotas

```sh
# a code leaked or a judge seat ends: delete its digest line, then restart (the file is read at startup only)
sudoedit /etc/consensus/judge-codes.sha256
sudo systemctl restart consensus-public.service
# every code at once: new codes replace the file (the old ones stop working at the restart)
sudo ~/consensus/deploy/scripts/judge-codes.sh --count <seats> --force && sudo systemctl restart consensus-public.service
journalctl -u consensus-public.service --since -1d --no-pager | grep -E 'refused a (public|judge) run: the (client|code) started'
```

A digest line does not say whose code it is: the lines follow the order in which the
script printed the codes, so note which judge seat got the first, the second, ... code
(the seat, never the code). The gate's daily counts (`live/gate.rs`) are kept in the
`consensus-public` process only: `CONSENSUS_LIVE_CLIENT_DAILY` (default 3) public-lane
runs per client key and `CONSENSUS_LIVE_JUDGE_CODE_DAILY` (default 5) runs per judge code,
per UTC day, taken with the ledger reservation and given back when the ledger refuses, the
run cannot be built or its start fails, so a refusal consumes nothing. A restart forgets
them, so a client or a code may start its share again the same day; the ledger's caps are
unaffected (they live in its files). A refusal logs one line without the client's address
or the code (the `grep` above); the visitor sees `429 live_client_quota` with
`Retry-After` until 00:00 UTC, a judge `429 live_budget_judge_daily`.

---

## Appendix A. Unit directives and the man-page definitions they were checked against

`systemd-analyze verify` runs on the server (section 5.3). Every directive used in
`deploy/systemd/*` was checked against the current man pages on 2026-10-28:

| Directive | Page | Checked statement |
|---|---|---|
| `Description=`, `After=`, `Wants=`, `[Install] WantedBy=`, drop-in `UNIT.d/*.conf` | systemd.unit(5) `[SD-5]` | `After=` orders after the listed unit; `Wants=` is the weak dependency; enable creates the `.wants/` symlink; drop-ins are merged after the unit file and take precedence |
| `Type=exec`, `ExecStart=`, `Restart=on-failure`, `RestartSec=`, `TimeoutStopSec=` | systemd.service(5) `[SD-2]` | `exec` waits for a successful `execve()`; `on-failure` covers non-zero exits, signals and timeouts and is "the recommended choice for long-running services"; `TimeoutStopSec=` bounds the stop before SIGKILL |
| `KillMode=mixed`, `KillSignal=SIGTERM` | systemd.kill(5) `[SD-3]` | `mixed`: SIGTERM to the main process, SIGKILL to the remaining control group after the timeout; `KillSignal=` defaults to SIGTERM |
| `User=`, `Group=`, `SupplementaryGroups=`, `UMask=` | systemd.exec(5) `[SD-1]` | user/group of the processes; supplementary groups extend the database's; `UMask=` defaults to 0022 |
| `EnvironmentFile=` | systemd.exec(5) | newline-separated `VAR=value`, read shortly before execution, later files override earlier ones |
| `LoadCredential=ID:PATH` (`meta.key` and `turnstile.secret` in the public unit; `meta.key` and `operator.token` in the operator unit) | systemd.exec(5) | copied read-only, accessible to the unit's user only, `$CREDENTIALS_DIRECTORY`, `/run/credentials/UNITNAME` for system services, 1 MB total |
| *(not used)* `RuntimeDirectory=` — replaced by `/etc/tmpfiles.d/consensus.conf` (`d /run/consensus 2750 consensus consensus-sock - -`) and `ReadWritePaths=/run/consensus` | systemd.exec(5); tmpfiles.d(5), systemd-tmpfiles(8) `[SD-8]` | `RuntimeDirectory=` owns the directory `User=`/`Group=` and re-applies that ownership to an existing directory, which would put the socket in the group `consensus`; a `d` line creates the directory at boot with the given mode and ownership, setgid included (the page's own examples use `1777`) |
| `NoNewPrivileges=`, `ProtectSystem=strict`, `ReadWritePaths=`, `ProtectHome=`, `PrivateTmp=`, `PrivateDevices=` | systemd.exec(5) | as quoted in section 5.1 |
| `ProtectKernelTunables=`, `ProtectKernelModules=`, `ProtectKernelLogs=`, `ProtectControlGroups=`, `ProtectClock=`, `ProtectHostname=`, `ProtectProc=invisible` | systemd.exec(5) | kernel tunables read-only; module loading denied; kernel log inaccessible; cgroup fs read-only; clock writes denied; UTS namespace; other users' processes hidden from `/proc` (effective with `User=`) |
| `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6` | systemd.exec(5) | allow-list of socket families; `AF_UNIX` should be included |
| `RestrictNamespaces=`, `RestrictRealtime=`, `RestrictSUIDSGID=`, `LockPersonality=`, `MemoryDenyWriteExecute=`, `RemoveIPC=` | systemd.exec(5) | namespaces, realtime scheduling, SUID/SGID bits, personality changes and W+X mappings refused; IPC objects removed on stop |
| `CapabilityBoundingSet=` (empty) | systemd.exec(5) | "the bounding set is reset to the empty capability set" |
| `SystemCallFilter=@system-service`, `SystemCallErrorNumber=EPERM`, `SystemCallArchitectures=native` | systemd.exec(5) | the recommended allow-list starting point; EPERM instead of SIGSYS; native ABI only |
| `LimitNOFILE=` | systemd.exec(5) | resource-limit table (`ulimit -n`); the hard default is 524288 |
| `MemoryHigh=`, `MemoryMax=`, `TasksMax=` | systemd.resource-control(5) `[SD-4]` | throttling limit; absolute limit with the OOM killer inside the unit; task count including threads |

The packaged `cloudflared.service` (`Type=notify`, `TimeoutStartSec=15`,
`Restart=on-failure`, `RestartSec=5s`) is Cloudflare's `[CF-7]` and is not edited.

## Appendix B. Sources

Cloudflare (fetched 2026-10-28; the Tunnel documentation moved under
`cloudflare-one/networks/connectors/cloudflare-tunnel/` and the former "Ingress rules"
page could not be fetched at any URL, so the ingress syntax is cited from cloudflared's
source):

- `[CF-1]` Configuration file — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/configuration-file/
- `[CF-2]` cloudflared source, `ingress/ingress.go` (service parsing: `unix:`, `unix+tls:`, `http_status:`; catch-all validation) — https://github.com/cloudflare/cloudflared/blob/master/ingress/ingress.go
- `[CF-3]` cloudflared source, `ingress/config.go` (`originRequest` keys and precedence) — https://github.com/cloudflare/cloudflared/blob/master/ingress/config.go
- `[CF-4]` Origin configuration parameters — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/origin-parameters/
- `[CF-5]` Tunnel run parameters — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/run-parameters/
- `[CF-6]` Run as a service, Linux — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/as-a-service/linux/
- `[CF-7]` cloudflared source, `cmd/cloudflared/linux_service.go` (unit templates, `/etc/cloudflared`, update timer) — https://github.com/cloudflare/cloudflared/blob/master/cmd/cloudflared/linux_service.go
- `[CF-8]` Create a locally-managed tunnel — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/create-local-tunnel/
- `[CF-9]` Tunnel permissions — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/tunnel-permissions/
- `[CF-10]` DNS records for tunnels — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/routing-to-tunnel/dns/
- `[CF-11]` Tunnel with firewall — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/tunnel-with-firewall/
- `[CF-12]` Quick Tunnels — https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/
- `[CF-13]` Downloads — https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/downloads/ ; package repository — https://pkg.cloudflare.com/index.html
- `[CF-14]` Connection limits — https://developers.cloudflare.com/fundamentals/reference/connection-limits/ ; Error 524 — https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-5xx-errors/error-524/
- `[CF-15]` HTTP request headers — https://developers.cloudflare.com/fundamentals/reference/http-headers/
- `[CF-16]` IP Geolocation — https://developers.cloudflare.com/network/ip-geolocation/ ; Managed Transforms reference — https://developers.cloudflare.com/rules/transform/managed-transforms/reference/
- `[CF-17]` WAF custom rules — https://developers.cloudflare.com/waf/custom-rules/
- `[CF-18]` Field `ip.src.country` — https://developers.cloudflare.com/ruleset-engine/rules-language/fields/reference/ip.src.country/ ; Operators — https://developers.cloudflare.com/ruleset-engine/rules-language/operators/
- `[CF-19]` Rate limiting rules — https://developers.cloudflare.com/waf/rate-limiting-rules/ ; Create in the dashboard — https://developers.cloudflare.com/waf/rate-limiting-rules/create-zone-dashboard/
- `[CF-20]` Managed rules — https://developers.cloudflare.com/waf/managed-rules/ ; Deploy in the dashboard — https://developers.cloudflare.com/waf/managed-rules/deploy-zone-dashboard/
- `[CF-21]` Turnstile server-side validation — https://developers.cloudflare.com/turnstile/get-started/server-side-validation/ ; Testing — https://developers.cloudflare.com/turnstile/troubleshooting/testing/ ; Widget management — https://developers.cloudflare.com/turnstile/get-started/widget-management/dashboard/ ; Widget configurations — https://developers.cloudflare.com/turnstile/get-started/client-side-rendering/widget-configurations/
- `[CF-22]` Cache Rules settings — https://developers.cloudflare.com/cache/how-to/cache-rules/settings/ ; Cache responses — https://developers.cloudflare.com/cache/concepts/cache-responses/
- `[CF-23]` Rocket Loader — https://developers.cloudflare.com/speed/optimization/content/rocket-loader/ and https://developers.cloudflare.com/speed/optimization/content/rocket-loader/enable/
- `[CF-24]` Email Address Obfuscation — https://developers.cloudflare.com/waf/tools/scrape-shield/email-address-obfuscation/
- `[CF-25]` Cloudflare Fonts — https://developers.cloudflare.com/speed/optimization/content/fonts/
- `[CF-26]` Automatic HTTPS Rewrites — https://developers.cloudflare.com/ssl/edge-certificates/additional-options/automatic-https-rewrites/
- `[CF-27]` JavaScript detections — https://developers.cloudflare.com/cloudflare-challenges/challenge-types/javascript-detections/ ; Bot Fight Mode — https://developers.cloudflare.com/bots/get-started/bot-fight-mode/
- `[CF-28]` Web Analytics — https://developers.cloudflare.com/web-analytics/get-started/ ; Zaraz FAQ — https://developers.cloudflare.com/zaraz/faq/
- `[CF-29]` Configuration Rules settings — https://developers.cloudflare.com/rules/configuration-rules/settings/
- `[CF-30]` Encryption modes — https://developers.cloudflare.com/ssl/origin-configuration/ssl-modes/ ; Always Use HTTPS — https://developers.cloudflare.com/ssl/edge-certificates/additional-options/always-use-https/
- `[CF-31]` Self-hosted Access application — https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/
- `[CF-32]` Access service tokens (creation, the `CF-Access-Client-Id` / `CF-Access-Client-Secret` headers, Service Auth, strict mode, revocation) — https://developers.cloudflare.com/cloudflare-one/access-controls/service-credentials/service-tokens/ ; Access policies (actions, the Service Token selector, evaluation order) — https://developers.cloudflare.com/cloudflare-one/access-controls/policies/

systemd (manual pages for systemd 262, fetched 2026-10-28; Ubuntu 24.04 ships systemd 255,
and every directive used here predates it, the newest being `LoadCredential=` from v247):

- `[SD-1]` systemd.exec(5) — https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html
- `[SD-2]` systemd.service(5) — https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html
- `[SD-3]` systemd.kill(5) — https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html
- `[SD-4]` systemd.resource-control(5) — https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html
- `[SD-5]` systemd.unit(5) — https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html
- `[SD-6]` journald.conf(5) — https://www.freedesktop.org/software/systemd/man/latest/journald.conf.html
- `[SD-7]` timesyncd.conf(5) — https://www.freedesktop.org/software/systemd/man/latest/timesyncd.conf.html ; timedatectl(1) — https://www.freedesktop.org/software/systemd/man/latest/timedatectl.html
- `[SD-8]` tmpfiles.d(5) (the `d` line, the mode field, boot-time application) — https://www.freedesktop.org/software/systemd/man/latest/tmpfiles.d.html ; systemd-tmpfiles(8) (`--create`, a configuration file as argument) — https://www.freedesktop.org/software/systemd/man/latest/systemd-tmpfiles.html ; systemd-run(1) (`--property=`, `--wait`, `--pipe`, `--collect`, exit-status propagation) — https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html

Ubuntu:

- `[UB-1]` Ubuntu Server documentation, Automatic updates — https://ubuntu.com/server/docs/how-to/software/automatic-updates/
- `[UB-2]` Ubuntu Server documentation, Firewalls — https://ubuntu.com/server/docs/how-to/security/firewalls/
- `[UB-3]` ufw(8), Ubuntu 24.04 manual page — https://manpages.ubuntu.com/manpages/noble/man8/ufw.8.html
- `[UB-4]` swapon(8), mkswap(8), fstab(5), Ubuntu 24.04 manual pages — https://manpages.ubuntu.com/manpages/noble/man8/swapon.8.html , https://manpages.ubuntu.com/manpages/noble/man8/mkswap.8.html , https://manpages.ubuntu.com/manpages/noble/man5/fstab.5.html
- `[UB-5]` mknod(2) ("If the directory containing the node has the set-group-ID bit set ... the new node will inherit the group ownership from its parent directory"; `S_IFSOCK` is one of the node types), gpasswd(1) (`-a`, `-d`), sudo(8) (`-n`: "If a password is required for the command to run, sudo will display an error message and exit"), head(1) (`-c -NUM`), Ubuntu 24.04 manual pages — https://manpages.ubuntu.com/manpages/noble/man2/mknod.2.html , https://manpages.ubuntu.com/manpages/noble/man1/gpasswd.1.html , https://manpages.ubuntu.com/manpages/noble/man8/sudo.8.html , https://manpages.ubuntu.com/manpages/noble/man1/head.1.html

Hyphae:

- `[HY-1]` Release Hyphae 4.0.0, tag `release-v4.0.0-crates` — https://github.com/Hyphae-Research-Foundation/hyphae/releases/tag/release-v4.0.0-crates ; asset digests read from the GitHub API — https://api.github.com/repos/Hyphae-Research-Foundation/hyphae/releases/tags/release-v4.0.0-crates

This repository: `docs/adr/0006-public-api-sessions-listener-split.md`,
`docs/adr/0007-public-live-mode-owner-key.md`, `docs/adr/0008-public-evidence-sealed-snapshot.md`,
`docs/security/release-checklist.md`, `docs/threat-model.md`,
`docs/reviews/2026-10-27-independent-audit.md` (§7, the plan's owner actions),
`crates/consensus-api/src/public_config.rs`, `crates/consensus-api/src/live/config.rs`,
`crates/consensus-api/src/live/geo.rs`, `crates/consensus-api/src/live/budget.rs`,
`crates/consensus-api/src/live/judge.rs`, `crates/consensus-api/src/live/turnstile.rs`,
`crates/consensus-api/src/config.rs`, `crates/consensus-api/src/server.rs`,
`crates/consensus-api/src/stream.rs`, `crates/consensus-api/src/api/mod.rs`,
`crates/consensus-api/src/api/v1.rs`, `crates/consensus-api/src/static_files.rs`,
`crates/consensus-api/src/showcase.rs`, `crates/consensus-api/src/guided.rs`,
`crates/consensus-api/src/hyphae.rs`, `crates/consensus-api/src/snapshot.rs` (on
`feat/public-portal`), `scripts/linux-gates.sh`, `scripts/hyphae-container/README.md`,
`research/README.md`, `apps/web/README.md`, `apps/web/scripts/in-docker.sh`, and the
templates and scripts under `deploy/`.

## Open questions for the owner

1. **Egress and updates.** The decided egress policy (Meta, Siteverify, tunnel edge only)
   stops unattended security updates and NTP unless 123/udp and the Ubuntu mirrors'
   80/443 are allowed. This runbook allows 123/udp outright and opens 80/443 only in
   maintenance windows (section 2.4). Decide whether to allow the security mirrors
   permanently.
2. **Resolved-address allowlist.** `api.meta.ai` and `challenges.cloudflare.com` are
   allowed by the addresses they resolve to at rule time (ufw has no hostname rules). If
   they rotate, live runs fail until the rules are refreshed (daily check, 9.2). The
   alternative is `ufw allow out to any port 443 proto tcp`, which is looser than the
   decision.
3. **`CONSENSUS_LIVE_EXPOSURE_DAYS`.** With $10 a day and $60 lifetime, the server refuses
   to start with the variable set to 7 or more days (`daily × days ≤ lifetime`), and the
   exposure from go-live to 2026-11-10 is longer than six days. Either leave it unset and
   accept that the lifetime cap can be reached before judging ends, or lower the daily cap.
4. **Bot Fight Mode** must stay off for byte-equal HTML (7.7); it was not in the decided
   list of features to turn off.
5. **Staging after go-live.** `staging.<hostname>` keeps serving the same socket behind
   Access; remove its ingress rule and DNS record when staging is no longer needed, or at
   teardown.
6. **Lifetime cap after staging.** Answered by the approved plan ("Budget (owner
   decision)"): the $60 is the public live mode's budget, judge lane included; the
   validation battery, staging and the video come out of the rest of the $150 credit. The
   production ledger therefore keeps `CONSENSUS_LIVE_LIFETIME_USD=60.00` from zero, and
   the staging ledger's `lifetime_spent_usd` goes into the owner's accounting
   (section 4.5, rule 1).
7. **Origin probes over SSH.** `verify-edge.sh --ssh` runs `sudo -n curl` on the server.
   Either the owner's SSH user may run `sudo` without a password (the usual cloud-image
   default), or the user joins `consensus-sock` for the duration of the checks and the
   script runs with `--no-sudo`. The second widens the socket group by one trusted
   account; the first widens nothing. Pick one and note it in the G6 evidence.
8. **A ledger reset in the binary.** Done: `consensus-api ledger status | reset --by |
   release <id> --by` (`crates/consensus-api/src/ledger_cli.rs`), run through
   `deploy/scripts/ledger.sh`; sections 4.4, 4.5, 8.8, 9.3 and 9.6 use it. It is a
   subcommand that needs a shell on the server, not an operator-console route, so it
   adds nothing to either listener. The per-phase ledgers stay, for the accounts
   (section 4.5).
