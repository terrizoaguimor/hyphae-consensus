# Deploy day: the public portal on the VPS, in one sitting

Date: 2026-10-31, for the deployment of 2026-11-01 · Written against `docs/r6-deploy-ready`
(base `a657531`) · Audience: the owner-operator · Companion: `docs/deployment/runbook.md`
(the "why" and every detail; section numbers below are its sections), `deploy/` (templates
and scripts).

This is the runbook cut down to an ordered checklist for one sitting, with two modes:

| Mode | What the visitor gets | What the server holds | When |
|---|---|---|---|
| **A: replay + guided, live OFF** (recommended for the judging) | the static site; guided runs (SIMULATED evidence, scripted model); the research showcase (RECORDED runs replayed from `research/`: mostly real `muse-wonder-1.0` runs; the console runs C01, C02 and C05 include scripted model calls and simulated proofs, labelled as such) | no Meta key (a canary file), no Turnstile secret, no ledger in use, no snapshot needed | 2026-11-01 |
| **B: live ON** (optional) | everything of A, plus live runs of `muse-wonder-1.0` on the owner's dedicated key over the sealed snapshot, behind Turnstile, the USD ledger and the judge codes | the dedicated Meta key, the Turnstile secret, the Hyphae 4.0.0 verifier, the sealed snapshot, the judge-code digests | only after A is green, and only if the owner decides so (plan: drop it if not stable by 2026-11-02 23:00 local) |

Everything below was rehearsed on 2026-10-31 in a throwaway `ubuntu:24.04` container
(section "Rehearsal results" at the end): the install, the configuration of both modes,
mode A end to end on the unix socket and mode B up to a green boot check. What the rehearsal
could **not** cover (systemd as PID 1, the sandbox directives, cloudflared, the Cloudflare
dashboard, the edge, a real live run) is marked **[not rehearsed]** where it occurs.

Conventions: as in the runbook. `# owner machine` blocks run in Git Bash on the Windows
machine from the repository root; `# server` blocks run over SSH on the VPS; every
placeholder is in angle brackets and listed in the table of section 1. Never paste a key,
secret, token or judge code into a command line, a chat or a file in the repository.

---

## 1. Every value the owner provides, and where it goes

Fill the "Value" column on paper (or in a local file outside the repository) before you
start. "Mode" says which mode needs it; "A+B" is both.

| # | Value | Mode | Where it goes (file, mode, owner) | Source / how to get it |
|---|---|---|---|---|
| 1 | `<server-ip>`: the VPS public IPv4 (and IPv6 if any) | A+B | owner machine `~/.ssh/config` (`Host <server>`); provider firewall | the VPS provider |
| 2 | `<server>`: the SSH alias; `<ssh-user>`: the SSH user with `sudo` | A+B | owner machine `~/.ssh/config` (`HostName <server-ip>`, `User <ssh-user>`, key-only) | the owner |
| 3 | OS: **Ubuntu 24.04 LTS, x86_64, glibc 2.39**, at least 2 GiB RAM | A+B | nothing to write; checked in step A1 (`lsb_release -a`, `ldd --version`, `uname -m`) | the binaries are built for exactly this (`linux-gates.sh` image) |
| 4 | `<admin-ip>`: the address(es) SSH is allowed from | A+B | provider firewall (TCP 22 only); `ufw allow ... from <admin-ip>/32 ... port 22` (step A10) | the owner's connection |
| 5 | `<resolver-ip>`: the upstream DNS resolver(s) of the VPS | A+B | `ufw allow out to <resolver-ip> port 53` (step A10) | `resolvectl status` on the server |
| 6 | `<zone>`: the Cloudflare zone (domain), on a Cloudflare account the owner controls | A+B | Cloudflare dashboard | the owner's Cloudflare account |
| 7 | `<hostname>`: the public hostname, e.g. `consensus.<zone>` (a bare host name: lowercase letters, digits, `-`, `.`) | A+B | `/etc/consensus/public.env` (`CONSENSUS_ALLOWED_ORIGINS=https://<hostname>`, `CONSENSUS_ALLOWED_HOSTS=<hostname>`; mode B also `CONSENSUS_TURNSTILE_HOSTNAME=<hostname>`), `root:consensus 0640`; `/etc/cloudflared/config.yml` (`hostname:`), `root:cloudflared 0640`; the DNS route; `apps/web/.env.production.local` (`NEXT_PUBLIC_SITE_ORIGIN=https://<hostname>`, owner machine, git-ignored) | the owner |
| 8 | `staging.<hostname>` (optional; only for the runbook's Access-protected staging phase) | B, optional | the same files as #7, second entry | the runbook, sections 7.9 and 8 |
| 9 | Cloudflare login certificate `cert.pem` | A+B | **owner machine only** (`~/.cloudflared/cert.pem`); never on the server | `cloudflared tunnel login` (step A8) |
| 10 | `<TUNNEL-UUID>` and its credentials file `<TUNNEL-UUID>.json` | A+B | `/etc/cloudflared/config.yml` (`tunnel:`, `credentials-file:`); the file at `/etc/cloudflared/<TUNNEL-UUID>.json`, `root:cloudflared 0640` | `cloudflared tunnel create consensus-public` (step A8) |
| 11 | Turnstile **site key** (public) | B | `apps/web/.env.production.local`: `NEXT_PUBLIC_TURNSTILE_SITE_KEY=<site key>`, then rebuild the web export (it is baked in at build time; without it the live form stays closed) | Cloudflare → Turnstile → widget for `<hostname>` (and `staging.<hostname>`), mode Managed (runbook 7.6) |
| 12 | Turnstile **secret** | B | `/etc/consensus/credentials/turnstile.secret`, `root:root 0600`, typed with `cat` (step B3); systemd copies it to `/run/credentials/consensus-public.service/turnstile.secret`. Mode A: the file holds placeholder text and the two `CONSENSUS_TURNSTILE_*` lines stay commented out | the same widget; a Cloudflare test secret is refused in public mode |
| 13 | **Dedicated Meta key** (`LLM_<id>_<secret>`, never the development key) | **B only** | `/etc/consensus/credentials/meta.key`, `root:root 0600`, typed with `cat` (step B3). Mode A: a canary `LLM_0_canary-<random>` (the unit's `LoadCredential=` needs the file; with live mode off it is never opened) | dev.meta.ai, a separate key for the server; spend alert set (ADR-0007) |
| 14 | Operator token | A+B | `/etc/consensus/credentials/operator.token`, `root:root 0600`; generated on the server (`openssl rand -hex 32`), never printed | step A5 |
| 15 | Budget caps: daily `$10.00`, lifetime `$60.00`, judge slice `$15.00` (owner decision 2026-10-27); optional judge daily cap | B | the defaults ARE these values: leave `CONSENSUS_LIVE_DAILY_USD`, `CONSENSUS_LIVE_LIFETIME_USD`, `CONSENSUS_LIVE_JUDGE_USD` commented in `public.env`; set one only to change it. `CONSENSUS_LIVE_EXPOSURE_DAYS` stays unset (with $10 × 7 days > $60 the server refuses to start) | runbook 4.1, open question 3 |
| 16 | Per-visitor quotas: 3 public live runs per client a day, 5 judge runs per code a day | B | defaults; `CONSENSUS_LIVE_CLIENT_DAILY`, `CONSENSUS_LIVE_JUDGE_CODE_DAILY` only to change them | runbook 4.1 |
| 17 | `<seats>`: the number of judge invite codes | B | generated on the server by `judge-codes.sh --count <seats>` (step B4): the codes are printed ONCE and go only into the submission's testing instructions; the server keeps their SHA-256 in `/etc/consensus/judge-codes.sha256`, `root:consensus 0640`; `public.env`: `CONSENSUS_LIVE_JUDGE_CODES_FILE=/etc/consensus/judge-codes.sha256` | runbook 4.3 |
| 18 | Allowed origins and hosts | A+B | `public.env`: `CONSENSUS_ALLOWED_ORIGINS=https://<hostname>` (comma-separated, scheme included, no path), `CONSENSUS_ALLOWED_HOSTS=<hostname>` (no scheme). Add `https://staging.<hostname>` / `staging.<hostname>` only if #8 is used. An unreplaced `<hostname>` in the hosts list is **accepted at startup** and then refuses every request (rehearsal): step A6 greps for it | `public_config.rs` |
| 19 | Snapshot directory: `snap-2026-10-29-demo-repo` (the FULL capture: a git checkout lacks the `.proof`/`.witness` files and is refused with `checksums_incomplete`) | B | `/var/lib/consensus/snapshots/snap-2026-10-29-demo-repo/`, `consensus:consensus`, top directory `/var/lib/consensus/snapshots` `0700`, subdirectories `0750`, files `0640` (step B2); `public.env`: `CONSENSUS_SNAPSHOT_DIR=/var/lib/consensus/snapshots/snap-2026-10-29-demo-repo` | the owner's main checkout `snapshots/snap-2026-10-29-demo-repo` (or the container's `/hyphae-home/snapshots/...`) |
| 20 | Snapshot anchor pin `43a2dea9c48d015bf6cbd6692b7ecf2bf80c66cd47d398b38e6b8471b18e8f02` | B | `public.env`: `CONSENSUS_HYPHAE_EXPECTED_ANCHOR=` | README "Published snapshot digests"; `manifest.json` `anchor_hex` |
| 21 | Manifest pin `b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a` | B | `public.env`: `CONSENSUS_SNAPSHOT_MANIFEST_SHA256=`; `sudo sha256sum <dir>/manifest.json` must print it | README; take it from the publication, never from the copy in doubt |
| 22 | Hyphae binary pin `79025e8758ee7481f572d0451756c0e0771e96d13cd7134319367afa8b11a15f` | B | `public.env`: `CONSENSUS_HYPHAE_BIN_SHA256=`; `CONSENSUS_HYPHAE_BIN=/opt/hyphae/bin/hyphae` (already in the template) | README; `manifest.json` `hyphae.binary_sha256` |
| 23 | Hyphae release tarball check (runbook 3.2): `hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz` sha256 `cf92ed6da52d7db634c2d0c8c82bae8f0a36f912ee0fc7c1aea047d65957a075` (11,661,966 bytes), `SHA256SUMS` sha256 `61514af06cf1331b36ad7dd0e1e75b11a9a51e1328dd6f28de985b3016ada416`; the extracted `hyphae` must hash to #22 | B | `/opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae`, `hyphae:hyphae 0755`, link `/opt/hyphae/bin/hyphae` | GitHub release `release-v4.0.0-crates`; **still owed**: nobody has yet compared the tarball's binary with #22 (the development copy matches #22; step B2 does the comparison) |
| 24 | `consensus-api` binary digest and commit | A+B | `dist/consensus-api.sha256`, `dist/consensus-api.commit` on the owner machine; checked on the server by `sha256sum -c` and by `install.sh --sha256` | step A0 (`linux-gates.sh --release` at the commit to deploy) |
| 25 | Web build inputs: `NEXT_PUBLIC_SITE_ORIGIN=https://<hostname>` (optional, absolute `og:image`); `NEXT_PUBLIC_TURNSTILE_SITE_KEY` (mode B, #11) | A+B | `apps/web/.env.production.local` (git-ignored) before `pnpm build` | apps/web/README.md |
| 26 | Public contact email and hosting country in the legal texts | A+B (gate G10) | `apps/web/src/lib/site.ts` (`CONTACT_EMAIL`, `HOSTING_COUNTRY`, today `<pending: ...>` placeholders shown on the page); a commit, then the web build | the owner; not part of this checklist's commands |
| 27 | Cloudflare Access service token (`CF_ACCESS_CLIENT_ID`, `CF_ACCESS_CLIENT_SECRET`) | B, only with #8 | owner machine environment only (`read -rs`), never on the server | runbook 7.9 |
| 28 | `<owner>`: the name recorded by `ledger.sh reset --by` | B | command argument | the owner |

Files on the server that hold a secret: only the three under `/etc/consensus/credentials/`
(`root:root 0600`) and the tunnel credentials file. `public.env` and `operator.env` hold
paths, never values.

---

## 2. Mode A: replay + guided, live OFF (recommended)

Time: about 2 to 3 hours, most of it waiting for builds and DNS. Every step ends with its
verification; do not start the next step before it passes.

### A0. Build and check the artifacts (owner machine; can be done the evening before)

```sh
# owner machine, repository root, at the commit to deploy, clean tree
git status --short                                  # empty
sh scripts/linux-gates.sh --release                 # every gate PASS, then dist/ holds the build (about 15 min)
cat dist/consensus-api.sha256 dist/consensus-api.commit
printf 'NEXT_PUBLIC_SITE_ORIGIN=https://<hostname>\n' > apps/web/.env.production.local   # optional (#25)
export HC_WEB_NODE_MODULES_VOLUME=hc-web-deploy
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm build
test -f apps/web/out/csp-manifest.json && echo "csp manifest built"
# rehearse the server half on this exact build (optional, about 5 minutes; section "Rehearsal results")
bash scripts/deploy-rehearsal.sh
# pack (runbook 3.3)
( cd apps/web/out && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum ) > web.SHA256SUMS
tar -czf web.tar.gz -C apps/web/out .
tar -czf research.tar.gz research
tar -czf deploy.tar.gz deploy
```

Verify: the gates print `== DONE: every gate passed` and `== release: dist/consensus-api built
from commit <sha>`; the rehearsal ends with `failed: 0`. If a gate fails, do not deploy that
build (a red `api_public` test was seen once on 2026-10-31 and passed on the re-run: see
"Rehearsal results").

### A1. Server baseline (runbook 2.1, 2.2, 2.5, 2.6, 2.7)

```sh
# server
lsb_release -a; uname -m; ldd --version | head -1     # Ubuntu 24.04 LTS, x86_64, glibc 2.39
free -h; df -h /
sudo dd if=/dev/zero of=/swapfile bs=1MiB count=2048 status=progress && sudo chmod 0600 /swapfile \
  && sudo mkswap /swapfile && sudo swapon /swapfile && echo '/swapfile none swap defaults 0 0' | sudo tee -a /etc/fstab
sudo timedatectl set-ntp true; timedatectl status | grep -E 'synchronized|NTP'
sudo install -d -m 0755 /etc/systemd/journald.conf.d
printf '[Journal]\nStorage=persistent\nSystemMaxUse=500M\nMaxRetentionSec=2week\n' | sudo tee /etc/systemd/journald.conf.d/60-consensus.conf
sudo systemctl restart systemd-journald
command -v docker || echo "docker not installed"     # gate G9: no docker on this host
```

Verify: `swapon --show` lists `/swapfile`; `System clock synchronized: yes`; no docker.
Skip the swap lines if `swapon --show` already lists a swap of 2 GiB or more.

### A2. cloudflared package, BEFORE the firewall closes egress (runbook 6.2) [not rehearsed]

```sh
# server
sudo mkdir -p --mode=0755 /usr/share/keyrings
curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg | sudo tee /usr/share/keyrings/cloudflare-main.gpg >/dev/null
echo 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared noble main' | sudo tee /etc/apt/sources.list.d/cloudflared.list
sudo apt-get update && sudo apt-get install -y cloudflared
cloudflared --version
```

### A3. Copy the artifacts and run the installer (runbook 3.1, 3.4)

```sh
# owner machine
scp dist/consensus-api dist/consensus-api.sha256 deploy.tar.gz web.tar.gz web.SHA256SUMS research.tar.gz <server>:/tmp/
```

```sh
# server
cd /tmp && sha256sum -c consensus-api.sha256                       # consensus-api: OK
mkdir -p ~/consensus && tar -xzf /tmp/deploy.tar.gz -C ~/consensus
D=$(cut -d' ' -f1 /tmp/consensus-api.sha256)
sudo ~/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 "$D" --dry-run   # prints every change, makes none
sudo ~/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 "$D"
```

The dry run runs under `sudo` too: it changes nothing either way, but only root can see
inside `/etc/consensus` (`0750 root:consensus`), so without `sudo` a re-run would report
templates as missing that the real run leaves as they are.

Verify (the installer prints most of it; rehearsed). Every path under `/etc/consensus`,
`/var/lib/consensus` and `/run/consensus` is closed to the SSH user, so every check below
and in A4, A7 and B2 runs under `sudo`:

```sh
# server
sudo stat -c '%a %U:%G %n' /etc/consensus /etc/consensus/credentials /etc/consensus/public.env /var/lib/consensus/snapshots /run/consensus /usr/local/bin/consensus-api
# 750 root:consensus /etc/consensus; 700 root:root .../credentials; 640 root:consensus .../public.env;
# 700 consensus:consensus .../snapshots; 2750 consensus:consensus-sock /run/consensus; 755 root:root .../consensus-api
id -nG cloudflared                                                  # cloudflared consensus-sock (never consensus)
sudo systemd-analyze verify /etc/systemd/system/consensus-public.service /etc/systemd/system/consensus-operator.service   # no output
```

Re-running the installer with a new binary later: first
`sudo cp -p /usr/local/bin/consensus-api /usr/local/bin/consensus-api.prev` (the rollback
copy; the installer keeps none), then the same `install.sh` line (it asks before replacing).

### A4. Web export and research archive (runbook 3.3)

```sh
# server
sudo rm -rf /var/lib/consensus/web.new && sudo install -d -o consensus -g consensus -m 0750 /var/lib/consensus/web.new
sudo tar -xzf /tmp/web.tar.gz -C /var/lib/consensus/web.new
sudo sh -c 'cd /var/lib/consensus/web.new && sha256sum -c --quiet /tmp/web.SHA256SUMS' && echo "web export verified"
sudo chown -R consensus:consensus /var/lib/consensus/web.new
sudo find /var/lib/consensus/web.new -type d -exec chmod 0750 {} + -o -type f -exec chmod 0640 {} +
sudo rm -rf /var/lib/consensus/web.old
sudo test -d /var/lib/consensus/web && sudo mv /var/lib/consensus/web /var/lib/consensus/web.old
sudo mv /var/lib/consensus/web.new /var/lib/consensus/web
sudo test -f /var/lib/consensus/web/csp-manifest.json && echo "csp manifest present"
sudo tar -xzf /tmp/research.tar.gz -C /var/lib/consensus --no-same-owner
sudo sh -c 'cd /var/lib/consensus/research && sha256sum -c --quiet SHA256SUMS' && echo "research archive verified"
sudo chown -R consensus:consensus /var/lib/consensus/research
sudo find /var/lib/consensus/research -type d -exec chmod 0750 {} + -o -type f -exec chmod 0640 {} +
rm -f /tmp/web.tar.gz /tmp/web.SHA256SUMS /tmp/research.tar.gz /tmp/deploy.tar.gz /tmp/consensus-api /tmp/consensus-api.sha256
```

Verify: `web export verified`, `csp manifest present`, `research archive verified`.
(`install.sh` installs `/etc/consensus/public.env` with `/var/lib/consensus/web` already
existing and empty; the `mv` lines replace it.)

### A5. Credentials for mode A (runbook 4.2)

The unit's two `LoadCredential=` lines need both files to exist, or systemd refuses to start
it. In mode A neither is read: `meta.key` holds a canary of the documented shape (gate G4's
scans look for it), `turnstile.secret` placeholder text.

```sh
# server
sudo install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/meta.key
printf 'LLM_0_canary-%s\n' "$(openssl rand -hex 12)" | sudo tee /etc/consensus/credentials/meta.key >/dev/null
sudo install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/turnstile.secret
printf 'unused-while-live-mode-is-off\n' | sudo tee /etc/consensus/credentials/turnstile.secret >/dev/null
sudo install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/operator.token
openssl rand -hex 32 | sudo tee /etc/consensus/credentials/operator.token >/dev/null
sudo ls -la /etc/consensus/credentials                              # three files, -rw------- root root
```

### A6. `public.env` for mode A (runbook 4.1)

```sh
# server
sudo cp -p /etc/consensus/public.env /etc/consensus/public.env.orig   # rollback copy of the template
sudoedit /etc/consensus/public.env
```

Edit exactly this, nothing else:

- `CONSENSUS_ALLOWED_ORIGINS=https://<hostname>` (drop `,https://staging.<hostname>` unless #8 is used)
- `CONSENSUS_ALLOWED_HOSTS=<hostname>` (likewise)
- leave `CONSENSUS_LIVE_ENABLED=false`
- leave the snapshot lines, the Hyphae pins, `CONSENSUS_LIVE_JUDGE_CODES_FILE` and **both
  `CONSENSUS_TURNSTILE_*` lines commented out**: they are read and checked even with live mode
  off. A set `CONSENSUS_TURNSTILE_SECRET_FILE` over an empty or unreadable file or a Cloudflare
  test secret, `CONSENSUS_TURNSTILE_HOSTNAME=<hostname>` left as is, or one of the two lines
  without the other stops startup naming the variable (rehearsed). The placeholder text of
  A5 is **not** refused: with both lines set over it the server starts (rehearsed), which is
  harmless with live mode off and is why B3 checks that the real secret replaced it
- `operator.env` needs no edit

Verify:

```sh
# server
sudo grep -nE '^[A-Z_]+=.*<' /etc/consensus/public.env /etc/consensus/operator.env || echo "no placeholder on an active line"
sudo grep -E '^[A-Z_]+=' /etc/consensus/public.env                 # the active lines: compare with "Rehearsal results"
```

The active lines must be exactly these ten (the rehearsal's, with your hostname; the
rehearsal kept the staging entries of the template):

```
CONSENSUS_ALLOWED_ORIGINS=https://<hostname>
CONSENSUS_ALLOWED_HOSTS=<hostname>
CONSENSUS_RUN_STORE_DIR=/var/lib/consensus/runs
CONSENSUS_RESEARCH_DIR=/var/lib/consensus/research
CONSENSUS_WEB_ROOT=/var/lib/consensus/web
CONSENSUS_PUBLIC_MODE=true
CONSENSUS_META_API_KEY_FILE=/run/credentials/consensus-public.service/meta.key
CONSENSUS_HYPHAE_BIN=/opt/hyphae/bin/hyphae
CONSENSUS_LIVE_ENABLED=false
CONSENSUS_LIVE_LEDGER_DIR=/var/lib/consensus/ledger/staging
```

(`CONSENSUS_LIVE_LEDGER_DIR` and `CONSENSUS_HYPHAE_BIN` are not used with live mode off.)

### A7. Start the public server and check it on the socket (runbook 5.1)

```sh
# server
sudo systemctl enable --now consensus-public.service
systemctl status consensus-public.service --no-pager | grep -E 'Active|Main PID'     # active (running)
journalctl -u consensus-public -n 20 --no-pager
```

The journal must show, in this order (rehearsed): `Hyphae Consensus public server (schema
1.3, contract <sha>)`, `Executor disabled: decisions are recorded only.`, `Portal-wide
geographic block: 23 countries; live-only: KR`, `Public live mode: disabled
(CONSENSUS_LIVE_ENABLED)`, `Listening on unix socket /run/consensus/public.sock (mode 0660)`,
and no `[snapshot]` line. A failed start names the variable: `journalctl -u
consensus-public -n 50`, fix `public.env`, `sudo systemctl restart consensus-public`.
A start refused by systemd before the binary runs (`status=243/CREDENTIALS`) means a file of A5
is missing. **[not rehearsed: systemd as PID 1 and the sandbox directives]**

```sh
# server: one helper for the rest of this section
S() { sudo curl -sS --unix-socket /run/consensus/public.sock -H 'Host: <hostname>' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' "$@"; }
sudo stat -c '%A %U:%G' /run/consensus/public.sock                                   # srw-rw---- consensus:consensus-sock
S -o /dev/null -w '%{http_code}\n' http://localhost/api/v1/health               # 200
S http://localhost/api/v1/health; echo                                          # "executor":"disabled", schema, policy, contract_sha256
S http://localhost/api/v1/ready; echo                                           # "ready":true, run store ok, "showcase":{"entries":<n>,"ok":true}
S http://localhost/api/v1/capabilities | grep -o '"live_enabled":[a-z]*'        # "live_enabled":false
S -o /dev/null -w '%{http_code}\n' http://localhost/en/                         # 200
S -D - -o /dev/null http://localhost/en/ | grep -i -E '^(content-security-policy|x-frame-options|x-content-type-options|referrer-policy):' | cut -c1-80
S -o /dev/null -w '%{http_code}\n' http://localhost/api/runs                    # 404 (gate G1: no v0 router)
# a guided run end to end (curl keeps a Secure cookie off http://, so the cookie goes back by hand)
CK=$(S -D - -o /dev/null -X POST -H 'Origin: https://<hostname>' http://localhost/api/v1/session | sed -n 's/^[Ss]et-[Cc]ookie: \([^;]*\).*/\1/p')
RUN=$(S -H "Cookie: $CK" -X POST -H 'Origin: https://<hostname>' -H 'Content-Type: application/json' --data '{"scenario":"retry"}' http://localhost/api/v1/runs | sed -n 's/.*"run_id":"\([^"]*\)".*/\1/p')
sleep 5; S -H "Cookie: $CK" "http://localhost/api/v1/runs/$RUN" | grep -o '"state":"[a-z_]*"' | head -1     # "state":"awaiting_human_approval"
S -H "Cookie: $CK" -o /dev/null -w '%{http_code}\n' "http://localhost/api/v1/runs/$RUN/receipt"               # 200
S -o /dev/null -w '%{http_code}\n' "http://localhost/api/v1/runs/$RUN"                                         # 401 without a session (another session's cookie: 404, gate G2)
# the showcase (recorded runs from research/, provenance as labelled: C01, C02, C05 are scripted)
S http://localhost/api/v1/showcase | grep -o '"run_id":"[^"]*"' | sort -u | wc -l                                # the number of recorded runs (> 0)
SH=$(S http://localhost/api/v1/showcase | grep -o '"run_id":"[^"]*"' | head -1 | cut -d'"' -f4)
S -o /dev/null -w '%{http_code}\n' "http://localhost/api/v1/showcase/$SH"                                      # 200
S -o /dev/null -w '%{http_code}\n' http://localhost/research/MANIFEST.json                                     # 200
# least privilege of cloudflared (runbook 5.1)
sudo -u cloudflared curl -sS --unix-socket /run/consensus/public.sock -H 'Host: <hostname>' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' -o /dev/null -w 'cloudflared -> socket: %{http_code}\n' http://localhost/api/v1/health
for p in /etc/consensus/public.env /etc/consensus/credentials/operator.token /var/lib/consensus/snapshots /var/lib/consensus/runs; do
  sudo -u cloudflared ls -d "$p" >/dev/null 2>&1 && echo "LEAK: $p" || echo "ok: cloudflared cannot read $p"; done
sudo ~/consensus/deploy/scripts/canary-scan.sh                                                                  # gate G4 on the server: no finding
```

### A8. The named tunnel (runbook 6.1, 6.3, 6.4) [not rehearsed]

```sh
# owner machine (cloudflared installed here too; cert.pem stays here)
cloudflared tunnel login
cloudflared tunnel create consensus-public            # prints <TUNNEL-UUID> and the credentials file path
cloudflared tunnel route dns consensus-public <hostname>
scp ~/.cloudflared/<TUNNEL-UUID>.json <server>:/tmp/
```

The DNS route makes `<hostname>` public as soon as the tunnel runs. In mode A that is
acceptable (nothing costs money and every check of A7 passed); the runbook's Access-protected
staging phase (sections 7.9, 8) is the conservative alternative and is required before mode B
only if you choose to use `staging.<hostname>`.

```sh
# server
sudo install -o root -g cloudflared -m 0640 /tmp/<TUNNEL-UUID>.json /etc/cloudflared/<TUNNEL-UUID>.json && rm /tmp/<TUNNEL-UUID>.json
sudoedit /etc/cloudflared/config.yml        # tunnel: <TUNNEL-UUID>; credentials-file: /etc/cloudflared/<TUNNEL-UUID>.json;
                                            # hostname: <hostname>; delete the staging rule unless #8 is used
cloudflared tunnel --config /etc/cloudflared/config.yml ingress validate
cloudflared tunnel --config /etc/cloudflared/config.yml ingress rule https://<hostname>/api/v1/health     # rule 1 -> unix:/run/consensus/public.sock
cloudflared tunnel --config /etc/cloudflared/config.yml ingress rule https://other.<zone>/api/runs        # the http_status:404 catch-all (G1)
sudo cloudflared --config /etc/cloudflared/config.yml service install
sudo systemctl disable --now cloudflared-update.timer 2>/dev/null || true
sudo systemctl daemon-reload && sudo systemctl restart cloudflared.service
journalctl -u cloudflared -n 30 --no-pager | grep -c 'Registered tunnel connection'     # 1 to 4
ps -o user,group,supgrp,cmd -C cloudflared                                               # user cloudflared, supplementary group consensus-sock
```

### A9. Cloudflare dashboard for `<zone>` / `<hostname>` (runbook 7) [not rehearsed]

Mandatory for mode A (each one either breaks the portal or a gate):

- [ ] **Network → IP Geolocation ON.** Without `CF-IPCountry` every request is refused
      `451 geo_unknown` (rehearsed on the socket).
- [ ] DNS: `<hostname>` is a **proxied** CNAME to `<TUNNEL-UUID>.cfargotunnel.com`; no A/AAAA.
- [ ] SSL/TLS: Full (strict); Always Use HTTPS on; **Automatic HTTPS Rewrites off**.
- [ ] Off (they rewrite the HTML and break the per-page CSP hashes, gate G6): Rocket Loader,
      Cloudflare Fonts, Email Address Obfuscation, **Bot Fight Mode**, Web Analytics automatic
      setup, Zaraz. Scope with a Configuration Rule if the zone hosts other sites.
- [ ] Cache Rule "Consensus: bypass /api and /research": `(http.host eq "<hostname>" and
      (http.request.uri.path wildcard "/api/*" or http.request.uri.path wildcard "/research/*"))`,
      Bypass cache.
- [ ] WAF custom rule "Consensus: portal geo-block", Block: `(http.host eq "<hostname>" and
      ip.src.country in {"AF" "BY" "KH" "CN" "HK" "MO" "CU" "ER" "ET" "HT" "IR" "IQ" "LY" "MM"
      "NI" "KP" "PK" "RU" "SO" "SY" "UA" "VE" "EH"})` (the server enforces the same list; the
      edge rule is the primary control, gate G10).
- [ ] WAF managed ruleset (Free) deployed.

For mode B add the KR live-route rule and the rate-limiting rule of runbook 7.5.

### A10. Firewall (runbook 2.4) [not rehearsed]

Only now, with cloudflared registered: run the `ufw` block of runbook section 2.4 with
`<admin-ip>` and `<resolver-ip>` filled in. Keep a second SSH session open while you
`sudo ufw enable`, and check from it that SSH still works before closing the first.
In mode A the `api.meta.ai` and `challenges.cloudflare.com` rules may be left out (nothing
calls them); add them before mode B. `api.meta.ai` is allowed by Meta's address blocks,
not by the address one lookup returns: its answer rotates per lookup (runbook 2.4).

Verify: `sudo ufw status numbered`; `journalctl -u cloudflared -n 10 --no-pager` still shows
connections registered after a `sudo systemctl restart cloudflared`.

### A11. Verify through the edge (owner machine; runbook 8.1, 8.3, 8.4) [not rehearsed]

```sh
# owner machine
H=<hostname>; ACCESS=()
curl -sS -o /dev/null -w '%{http_code} %{time_total}s\n' "https://$H/api/v1/health"    # 200
curl -sS "https://$H/api/v1/ready"; echo                                                # "ready":true
curl -sS "https://$H/api/v1/capabilities" | grep -o '"live_enabled":[a-z]*'             # false
curl -sS -o /dev/null -w '%{http_code}\n' "https://$H/api/runs"                         # 404 (G1)
curl -sS "https://$H/" | grep -c 'Operator console'                                      # 0 (G1)
curl -sS -D - -o /dev/null "https://$H/api/v1/health" | grep -i -E 'cf-cache-status|cache-control'   # DYNAMIC (never HIT); no-store
deploy/scripts/verify-edge.sh --hostname "$H" --ssh <server>                            # G6: every page byte-equal, no DIFFER
deploy/scripts/canary-scan.sh --responses-only --hostname "$H"                          # G4: no finding
```

Then in a browser (a private window), in each of the four languages once: the home page; New
deliberation → a guided scenario runs to "awaiting human approval" with SIMULATED evidence
labelled; the Showcase lists the recorded runs (RECORDED, never LIVE) and one opens with its
receipt; a scripted console run (C01, C02 or C05) shows its scripted-model and SIMULATED proof
labels, never a real-model or valid-looking proof; the browser console shows no CSP violation. Record the date, the hostname, the
commit (`dist/consensus-api.commit`) and the results, and the go-live time
(UTC), in the owner's release record (outside the repository).

### A12. Rollback (mode A)

| Symptom | Action |
|---|---|
| Anything wrong at the edge, take the portal offline at once | `sudo systemctl stop cloudflared` (the hostname answers Cloudflare 1016; nothing reaches the server) |
| The server does not start after a config edit | `journalctl -u consensus-public -n 50` names the variable; `sudo cp -p /etc/consensus/public.env.orig /etc/consensus/public.env`, redo A6, `sudo systemctl restart consensus-public` |
| A new binary misbehaves | `sudo install -o root -g root -m 0755 /usr/local/bin/consensus-api.prev /usr/local/bin/consensus-api && sudo systemctl restart consensus-public` |
| A new web export misbehaves | `sudo mv /var/lib/consensus/web /var/lib/consensus/web.bad && sudo mv /var/lib/consensus/web.old /var/lib/consensus/web && sudo systemctl restart consensus-public` (the CSP manifest is read at startup) |
| The tunnel or DNS is wrong | `cloudflared tunnel route dns` is idempotent; `sudo systemctl restart cloudflared` after editing `config.yml` and `ingress validate` |
| Give up on the live URL | `sudo systemctl disable --now cloudflared consensus-public`; remove the CNAME; submit video + repository (the plan's fallback) |

---

## 3. Mode B: live ON (optional, after mode A is green)

Read ADR-0007 consequences once more: every live run spends real money on the dedicated key
(about $0.81 on average, $2.36 worst case reserved). The ledger caps hold whatever happens
(`$10/day`, `$60` lifetime, `$15` judge slice), the kill switch stops spending at the next
admission, and a trip lasts until `ledger.sh reset`.

### B1. Owner actions (owner machine and dashboards)

1. dev.meta.ai: a **dedicated** key for the server (#13); claim the credits on that account; a
   spend alert.
2. Cloudflare → Turnstile → Add widget: hostname `<hostname>` (and `staging.<hostname>` if #8),
   mode Managed, pre-clearance off; note the site key (#11) and the secret (#12).
3. Rebuild the web export **with the site key** and redeploy it:

```sh
# owner machine
printf 'NEXT_PUBLIC_SITE_ORIGIN=https://<hostname>\nNEXT_PUBLIC_TURNSTILE_SITE_KEY=<site key>\n' > apps/web/.env.production.local
sh apps/web/scripts/in-docker.sh pnpm build
( cd apps/web/out && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum ) > web.SHA256SUMS
tar -czf web.tar.gz -C apps/web/out . && scp web.tar.gz web.SHA256SUMS <server>:/tmp/
```

   then step A4's web block on the server and `sudo systemctl restart consensus-public`.
4. Cloudflare WAF: the KR rule and the rate-limiting rule of runbook 7.5.
5. `ufw`: the `api.meta.ai` and `challenges.cloudflare.com` rules of runbook 2.4 if A10 left
   them out (a temporary `ufw allow out to any port 443 proto tcp` is needed for B2 only if
   you download on the server; download on the owner machine instead).

### B2. Hyphae 4.0.0 verifier and the sealed snapshot (runbook 3.2, 3.3)

```sh
# owner machine (downloads the release asset; checks it against the GitHub-recorded digests)
base=https://github.com/Hyphae-Research-Foundation/hyphae/releases/download/release-v4.0.0-crates
curl -fsSLO "$base/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz" && curl -fsSLO "$base/SHA256SUMS"
echo "cf92ed6da52d7db634c2d0c8c82bae8f0a36f912ee0fc7c1aea047d65957a075  hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz" | sha256sum -c -
echo "61514af06cf1331b36ad7dd0e1e75b11a9a51e1328dd6f28de985b3016ada416  SHA256SUMS" | sha256sum -c -
sha256sum -c --ignore-missing SHA256SUMS
tar -xzOf hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae | sha256sum   # MUST print 79025e8758ee...a15f (#22): the check still owed
# the FULL snapshot (proofs and witnesses), checked before it leaves
( cd snapshots/snap-2026-10-29-demo-repo && sha256sum -c --quiet SHA256SUMS ) && echo "snapshot complete"
sha256sum snapshots/snap-2026-10-29-demo-repo/manifest.json                   # b731346c...014a (#21)
scp hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz <server>:/tmp/
scp -r snapshots/snap-2026-10-29-demo-repo <server>:/tmp/
```

If the extracted binary does **not** hash to `79025e87…a15f`, stop: the snapshot was captured
with another build (`capture_binary_mismatch`), and mode B is off the table until that is
resolved (runbook 3.3). Run the owner-machine lines from the main checkout, where the full
snapshot lives (`snapshots/` of a worktree or a clone lacks the private files).

```sh
# server
echo "cf92ed6da52d7db634c2d0c8c82bae8f0a36f912ee0fc7c1aea047d65957a075  /tmp/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz" | sha256sum -c -
sudo tar -xzf /tmp/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz -C /opt/hyphae
sudo chown -R hyphae:hyphae /opt/hyphae && sudo chmod 0755 /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae
sudo ln -sfn /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae /opt/hyphae/bin/hyphae
sha256sum /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae         # 79025e8758ee...a15f
sudo -u consensus /opt/hyphae/bin/hyphae --version
sudo mv /tmp/snap-2026-10-29-demo-repo /var/lib/consensus/snapshots/snap-2026-10-29-demo-repo
sudo sh -c 'cd /var/lib/consensus/snapshots/snap-2026-10-29-demo-repo && sha256sum -c --quiet SHA256SUMS' && echo "snapshot verified"
sudo sha256sum /var/lib/consensus/snapshots/snap-2026-10-29-demo-repo/manifest.json   # b731346c...014a
sudo chown -R consensus:consensus /var/lib/consensus/snapshots
sudo find /var/lib/consensus/snapshots -type d -exec chmod 0750 {} + -o -type f -exec chmod 0640 {} +
sudo chmod 0700 /var/lib/consensus/snapshots
sudo -u cloudflared ls /var/lib/consensus/snapshots 2>&1 | grep -q 'Permission denied' && echo "witnesses: unreachable for cloudflared"
rm /tmp/hyphae-4.0.0-x86_64-unknown-linux-gnu.tar.gz
```

The deployment review of gate G9 re-verifies every capture with the installed binary.
`scripts/snapshot_manifest.py` is not part of `deploy/`: copy it first
(`scp scripts/snapshot_manifest.py <server>:/tmp/` from the owner machine), then, on the server
(Ubuntu 24.04 ships `python3`):

```sh
# server
sudo -u consensus python3 -I /tmp/snapshot_manifest.py check /var/lib/consensus/snapshots/snap-2026-10-29-demo-repo --reverify /opt/hyphae/bin/hyphae; echo "exit $?"   # exit 0 (rehearsed)
rm /tmp/snapshot_manifest.py
```

The in-sandbox probe (`sudo ~/consensus/deploy/scripts/sandbox-probe.sh`) needs the live
configuration of B5 in `public.env`, so it runs in B6 **[not rehearsed: systemd-run]**.

### B3. The real credentials (runbook 4.2)

```sh
# server: type or paste each value, then Ctrl-D (nothing enters the shell history)
sudo sh -c 'cat > /etc/consensus/credentials/meta.key'          # the DEDICATED key (#13)
sudo sh -c 'cat > /etc/consensus/credentials/turnstile.secret'  # the widget secret (#12)
sudo ls -la /etc/consensus/credentials                           # -rw------- root root, three files
# startup refuses an empty file and a Cloudflare test secret, NOT the placeholder of A5 or the canary:
sudo grep -qx 'unused-while-live-mode-is-off' /etc/consensus/credentials/turnstile.secret && echo "STILL THE PLACEHOLDER: redo the turnstile.secret line" || echo "turnstile.secret replaced"
sudo grep -q '^LLM_0_canary-' /etc/consensus/credentials/meta.key && echo "STILL THE CANARY: redo the meta.key line" || echo "meta.key replaced"
```

Both lines must print "replaced". With the placeholder in place, live mode would boot and
every Turnstile check would fail at Siteverify.

### B4. Judge codes (runbook 4.3)

```sh
# server
sudo ~/consensus/deploy/scripts/judge-codes.sh --count <seats>      # prints the codes ONCE: copy them into the submission's testing instructions now
sudo stat -c '%U:%G %a' /etc/consensus/judge-codes.sha256            # root:consensus 640
sudo grep -c -E '^[0-9a-f]{64}$' /etc/consensus/judge-codes.sha256   # <seats>
```

### B5. `public.env` for mode B (runbook 4.1, 8.7)

```sh
# server
sudo cp -p /etc/consensus/public.env /etc/consensus/public.env.mode-a    # rollback copy
sudoedit /etc/consensus/public.env
```

Uncomment and fill (rehearsed: exactly these eight lines change):

```
CONSENSUS_HYPHAE_BIN_SHA256=79025e8758ee7481f572d0451756c0e0771e96d13cd7134319367afa8b11a15f
CONSENSUS_HYPHAE_EXPECTED_ANCHOR=43a2dea9c48d015bf6cbd6692b7ecf2bf80c66cd47d398b38e6b8471b18e8f02
CONSENSUS_SNAPSHOT_DIR=/var/lib/consensus/snapshots/snap-2026-10-29-demo-repo
CONSENSUS_SNAPSHOT_MANIFEST_SHA256=b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a
CONSENSUS_LIVE_ENABLED=true
CONSENSUS_LIVE_JUDGE_CODES_FILE=/etc/consensus/judge-codes.sha256
CONSENSUS_TURNSTILE_SECRET_FILE=/run/credentials/consensus-public.service/turnstile.secret
CONSENSUS_TURNSTILE_HOSTNAME=<hostname>
```

`CONSENSUS_LIVE_LEDGER_DIR` stays at the template's
`/var/lib/consensus/ledger/staging`: B6 and B7 (the owner's first live run, the kill-switch
rehearsal of gate G11, one judge code) spend on the **staging** ledger, never on the
production one, which must be empty at go-live (runbook 4.5). Step B7b switches to
`ledger/public`. The caps stay commented (their defaults are the owner decisions, #15).

```sh
# server
sudo grep -nE '^[A-Z_]+=.*<' /etc/consensus/public.env || echo "no placeholder on an active line"
sudo systemctl restart consensus-public
journalctl -u consensus-public -n 30 --no-pager
```

### B6. Verify the live boot (runbook 8.6)

The journal must show (rehearsed, with the same pins): `Sealed snapshot evidence:
snap-2026-10-29-demo-repo (project demo-repo, 7 records, anchor 43a2dea9c48d…, manifest sha256
pinned b731346c8464…, verifier binary sha256 pinned 79025e8758ee…, captured
2026-10-29T17:16:12Z, proofs re-verified at boot <time>)` and `Public live mode: enabled on the
owner's key (daily cap 10.00, lifetime cap 60.00, 3 public runs per client a day, judge lane
open, 5 runs per code a day)`, and **no** `[snapshot] live evidence unavailable` line (if
there is one, its code is in the table of runbook 8.6).

```sh
# server
S http://localhost/api/v1/ready; echo                             # "ready":true ... "live":{"ledger_ok":true,"tripped":false,"evidence_ok":true}
S http://localhost/api/v1/capabilities | grep -o '"live_enabled":[a-z]*\|"requires_turnstile":[a-z]*'   # true, true
sudo ~/consensus/deploy/scripts/ledger.sh status                   # JSON snapshot, "tripped": false, no open reservation [not rehearsed: systemd-run]
sudo ~/consensus/deploy/scripts/sandbox-probe.sh                   # PASS [not rehearsed]
sudo ~/consensus/deploy/scripts/canary-scan.sh                     # G4 with the real key in place: no finding
```

### B7. One live run and the kill switch (runbook 8.8 steps 3 and 4) [not rehearsed]

1. Browser, private window, `https://<hostname>/en/`: New deliberation → live → a preset, the
   attestation, the Turnstile challenge, Start. Watch it reach a recommendation with the LIVE
   label and the sealed-snapshot evidence label; open its receipt.
2. `sudo ~/consensus/deploy/scripts/kill-switch.sh status` shows the first reservation and
   settlement and `tripped: false`.
3. Kill-switch rehearsal: `kill-switch.sh on`; a second live start is refused (503, never
   202) while a guided run still starts; `kill-switch.sh off --yes`; `ledger.sh status` still
   `"tripped": true`; `ledger.sh reset --by <owner>`; `/api/v1/ready` shows `"tripped":false`
   within two seconds.
4. One judge code from B4 once in the browser's invite-code field (it spends the judge slice
   of the staging ledger).

All of this spends on `ledger/staging` (B5), so the $60 production lifetime cap is untouched.

### B7b. Go-live: retire the staging ledger, switch to `ledger/public` (runbook 4.5, 8.7) [not rehearsed]

Mandatory before the submission (with the judge codes) goes out:

```sh
# server
sudo ~/consensus/deploy/scripts/kill-switch.sh status          # note lifetime_spent_usd for the owner's accounting
sudo ~/consensus/deploy/scripts/export-ledger.sh --ledger-dir /var/lib/consensus/ledger/staging
sudo systemctl stop consensus-public.service; sudo systemctl stop consensus-operator.service 2>/dev/null || true
sudo mv /var/lib/consensus/ledger/staging "/var/lib/consensus/ledger-archive/staging-$(date -u +%Y%m%dT%H%M%SZ)"
sudo chown -R root:root /var/lib/consensus/ledger-archive && sudo chmod -R u=rwX,go= /var/lib/consensus/ledger-archive
sudo test ! -e /var/lib/consensus/ledger/public/lifetime.json && echo "production ledger is empty"
sudo cp -p /etc/consensus/public.env /etc/consensus/public.env.staging    # rollback copy
sudoedit /etc/consensus/public.env          # CONSENSUS_LIVE_LEDGER_DIR=/var/lib/consensus/ledger/public (the only change)
sudo grep -x 'CONSENSUS_LIVE_LEDGER_DIR=/var/lib/consensus/ledger/public' /etc/consensus/public.env
sudo systemctl start consensus-public.service
journalctl -u consensus-public -n 30 --no-pager                  # the two lines of B6 again, no "live evidence unavailable"
S http://localhost/api/v1/ready; echo                             # "ready":true ... "live":{"ledger_ok":true,"tripped":false,"evidence_ok":true}
sudo ~/consensus/deploy/scripts/kill-switch.sh status            # the production ledger: no spend yet, tripped: false
```

`CONSENSUS_LIVE_LIFETIME_USD` stays commented: its default is the $60.00 of runbook 8.7. Do
not repeat the kill-switch rehearsal on `ledger/public`; a later trip there is reset with
`ledger.sh reset` after its write-up (runbook 9.3).

### B8. Rollback from mode B

| Symptom | Action |
|---|---|
| Spend or behaviour you do not like, stop it NOW | `sudo ~/consensus/deploy/scripts/kill-switch.sh on` (the next admission trips; in-flight runs stop at their next model call; guided runs and the showcase stay up) |
| Back to mode A | `sudo cp -p /etc/consensus/public.env.mode-a /etc/consensus/public.env && sudo systemctl restart consensus-public`; then replace `meta.key` with a canary (A5) |
| Suspected key exposure | kill switch on; revoke the key at dev.meta.ai; runbook 9.6 |
| `[snapshot] live evidence unavailable: <code>` | the portal stays up with live mode unavailable; fix per runbook 8.6's table, restart |
| A judge code leaked | delete its line in `/etc/consensus/judge-codes.sha256`, restart (runbook 9.8) |

Teardown after judging (about 2026-11-10): runbook 9.7.

---

## 4. Rehearsal results (2026-10-31)

**How.** `bash scripts/deploy-rehearsal.sh --snapshot <main checkout>/snapshots/snap-2026-10-29-demo-repo
--hyphae <the development copy of the hyphae 4.0.0 linux binary>` on the owner machine: a
throwaway container from the `ubuntu:24.04` image `linux-gates.sh` pins (Ubuntu 24.04.5 LTS,
glibc 2.39), the repository mounted read-only, `curl ca-certificates openssl python3 systemd
sudo` installed from the Ubuntu archive, then the network **disconnected** (only `lo` left, no
route; checked without sending any request) before anything of the project ran, so no
request could reach Meta or Siteverify; the container is removed at the end. The server
blocks of A3 (install and verify), A4, A7 and B2 (the snapshot) and the checks of B3 are
**read out of this document** and run as an unprivileged SSH user (`deployer`, `sudo`
without a password) exactly as written, with `<hostname>` filled in; only the real
`install.sh` line of A3 (it needs a running systemd: the rehearsal installs with
`--no-systemd` as root) and the `canary-scan.sh` line of A7 (it needs `systemctl`) are left
out. Inputs: `dist/consensus-api` from `sh scripts/linux-gates.sh --release` in the
`docs/r6-deploy-ready-r` worktree at `ed879cc` (sha256
`b9594f79dfefaba8f7d39c440f86ffa99ed113e44130ba6f3d106642e01a60ab`, the same bytes as the
first rehearsal's build at `d0745f5`: the Rust tree is `a657531`'s), `apps/web/out` from
`pnpm build` without `.env.production.local`, and `research/`.

**Result: 240 checks passed, 0 failed** (run of the review round; the first round's fourth
run had 192 checks, its first three found the defects in the script itself and the
documentation errors listed under "Findings"). A mutation run on the same build, with the old
A4 line `[ -d /var/lib/consensus/web ] && ...`, the old A7 `stat` without `sudo` and the old
template (both Turnstile lines active), failed 19 checks, among them the nested export
(`web/web.new`), every page `404`, the socket `stat` refused and the template's active
Turnstile lines: the rehearsal now catches each of those. The gates of the release build:
fmt, clippy, test (604 passed, 0 failed, 2 ignored), demo crate (2 passed), evaluate (10/10,
0 unauthorized executions), manifest self-test (38 checks), capture self-test (29 passed).
In the first round, a first gates run on the same tree had one failure,
`api_public::a_damaged_stored_run_is_reported_never_served_as_if_whole` (a tampered stored
run answered 200 instead of 500 on its first read); the re-run on the same commit passed, so
it is recorded as an intermittent test failure to look at, not as a deployment blocker. If
it reappears on deploy day, re-run the gates; do not deploy a red build.

What passed, by step:

| Step | Checked |
|---|---|
| A3 | as the SSH user, the A3 block: `sha256sum -c` of the copied binary, the unpack, `sudo install.sh --dry-run` exits 0, prints 35 changes, every status line reads "would", changes nothing; a wrong `--sha256` is refused; `install.sh --no-systemd` then a second run (idempotent; `public.env` left as is); a re-run dry run without `sudo` says it cannot see inside `/etc/consensus`, with `sudo` it sees `public.env`; the A3 verify block as the SSH user prints the six expected `stat` lines and `cloudflared consensus-sock`, no "Permission denied"; every path of runbook 2.3 with its mode and owner (20 paths); `cloudflared` in `consensus-sock` only; `/run/consensus` `2750 consensus:consensus-sock` both by hand and from `deploy/tmpfiles.d/consensus.conf` through `systemd-tmpfiles --create`; `systemd-analyze verify` of both units: no output, exit 0 |
| A4 | the web export and `research/` (53 files) packed with the A0 commands; the A4 block as the SSH user over an existing (empty) `/var/lib/consensus/web`: `web export verified`, `csp manifest present`, `research archive verified`, no "Permission denied"; `csp-manifest.json` at the web root, no nested `web/web.new`, the old root moved to `web.old`, owners and modes `consensus` `0750`/`0640` |
| A5, A6 | the template has no active `CONSENSUS_TURNSTILE_*` line; the canary key, the placeholder Turnstile file, the operator token; no `<placeholder>` on an active line; the ten active lines of A6 |
| A7 | startup lines as listed in A7, no `[snapshot]` line; socket `srw-rw---- consensus:consensus-sock`; health 200 (`executor: disabled`, schema 1.3, policy 1.2, contract `465f4117…ad760e`); `451` without `CF-IPCountry` and from `UA`; `421` for a foreign `Host`; `400 client_unidentified` for a session without `CF-Connecting-IP`; `/ready` `"ready":true`, showcase 48 entries; `/capabilities` `"live_enabled":false`; the five scenarios listed; `/`, `/en/`, `/es/`, `/pt-BR/`, `/fr/`, `/en/showcase/`, `/en/runs/` 200 with CSP, `nosniff`, `DENY`, `no-referrer`; G1 404s (`/api/runs`, `POST /api/v1/operator/runs`, unknown path; `/` is not the console); `POST /api/v1/live-runs` never 202; a guided run of each scenario to `awaiting_human_approval` (retry `supported`, offline `insufficient_evidence`, invalid_proof `blocked`, injection `insufficient_evidence`, abstain `abstained`), provenance scripted model with simulated or no evidence, receipt and `events.ndjson` 200; G2: another session gets 404 on detail, events, receipt and cancel and never sees the run listed, no session at all gets 401; SSE of a resting run: `: stream-start` ... `event: stream_end`; the cookie `__Host-hc_session` with `Secure` and `HttpOnly`; showcase list (48), detail and receipt 200, `source: research_archive`; `/research/MANIFEST.json` byte-equal to the archive; cloudflared reaches the socket and cannot read `public.env`, `operator.token`, `snapshots/`, `runs/`, `ledger/` or the credential copy; G4: the server's environment (13 variables) has no key and no key variable, only `CONSENSUS_META_API_KEY_FILE` as a path, and the canary is in no log, store file or response; the A7 socket block as the SSH user, exactly as written (`sudo stat` of the socket, health, ready, capabilities, the headers, the G1 404, the guided run to `awaiting_human_approval`, its receipt, the 401, the showcase count 48 and a detail, the research manifest, cloudflared's 200 and its four refusals, no `LEAK`); SIGTERM: exit 0, `[shutdown]` line, socket removed; after a restart the six runs are in the store, the old cookie is no session (sessions are in memory) and `/ready` is true |
| A6 traps | each stops startup naming the variable: the Turnstile lines over an empty file (`the Turnstile secret is empty`), `CONSENSUS_TURNSTILE_HOSTNAME=<hostname>`, one Turnstile line without the other, an origin without `https://`, `CONSENSUS_ALLOWED_HOSTS` missing, live mode on with nothing else; and three that do not: both Turnstile lines over the A5 placeholder secret start (a placeholder is accepted; the B3 checks print `STILL THE PLACEHOLDER` and `STILL THE CANARY` for the A5 files), a missing Meta key file with live mode off starts (never opened), and an unreplaced `<hostname>` in `CONSENSUS_ALLOWED_HOSTS` starts and then answers `421` to the real host (hence the `grep` of A6) |
| B2 | `hyphae --version` as `consensus`: `hyphae 4.0.0`; the snapshot's `SHA256SUMS`; the B2 snapshot lines (from the move to the cloudflared check) as the SSH user: `snapshot verified`, the manifest pin printed, `witnesses: unreachable for cloudflared`, no "Permission denied"; manifest digest, anchor and binary digest equal to the pins #20-#22; top directory `700 consensus:consensus`, the snapshot directory `750`, files `0640`; `snapshot_manifest.py check --reverify /opt/hyphae/bin/hyphae` as `consensus`: `OK ... 2 captures, 22 file digests verified, 7 record ids ..., 2 captures re-verified with hyphae (fresh reports equal the captured ones)` |
| B4 | `judge-codes.sh --count 2` (codes discarded unprinted): `640 root:consensus`, 2 digests; a second run without `--force` refused |
| B3 | with a non-placeholder secret in `turnstile.secret` the check prints `turnstile.secret replaced` |
| B5, B6 | exactly the eight lines of B5 change, the ledger stays `ledger/staging`; the boot: `Sealed snapshot evidence: snap-2026-10-29-demo-repo (project demo-repo, 7 records, anchor 43a2dea9c48d…, manifest sha256 pinned b731346c8464…, verifier binary sha256 pinned 79025e8758ee…, captured 2026-10-29T17:16:12Z, proofs re-verified at boot ...)`, `Public live mode: enabled on the owner's key (daily cap 10.00, lifetime cap 60.00, 3 public runs per client a day, judge lane open, 5 runs per code a day)`, no unavailable line; `/ready` `"live":{"ledger_ok":true,"tripped":false,"evidence_ok":true}`; `/capabilities` `live_enabled` and `requires_turnstile` true; `consensus-api ledger status` (what `ledger.sh` runs) exit 0; a guided run still 202; SIGTERM exit 0. Refused at startup: live mode without the manifest pin, without the binary pin, without the Turnstile secret file. A wrong manifest pin keeps the portal up with `[snapshot] live evidence unavailable: manifest_pin_mismatch`, `/ready` `"ready":false` with `"evidence_ok":false`, `/capabilities` `live_enabled` false, health 200 and a guided run still `202` |

**Not rehearsed** (no systemd as PID 1, no network, no Cloudflare): `systemctl` itself and the
unit's sandbox directives (`ProtectSystem=strict`, the system-call filter, ...; the server ran
as the service user with the same environment and credentials paths, not inside the sandbox),
`LoadCredential=` (emulated by copying the files to `/run/credentials/consensus-public.service/`
readable by the service user), `sandbox-probe.sh` and `ledger.sh` (both need `systemd-run`),
cloudflared, ufw, the Cloudflare dashboard, every check through the edge (A11), the Turnstile
widget, a live run and the kill-switch rehearsal (B7), the retirement of the staging ledger
(B7b: it needs `systemctl`), and the release tarball check of #23 (the development copy of
the binary was used: it hashes to #22). The SSH user's `sudo` asked for no password; the
VPS's will, which changes nothing in the commands.

**Findings, and what changed** (`docs/r6-deploy-ready`):

1. `deploy/scripts/install.sh` had no way to run without systemd or to show what it would do:
   added `--dry-run` (prints every change, makes none, no root needed; the digest is still
   checked) and `--no-systemd` (for a container: `/run/consensus` by hand with the tmpfiles.d
   line's mode and owners, no `daemon-reload`). Without either flag it behaves as before.
2. `deploy/etc/consensus/public.env.example` set `CONSENSUS_TURNSTILE_SECRET_FILE` and
   `CONSENSUS_TURNSTILE_HOSTNAME=<hostname>` uncommented, and the code reads and checks both
   even with live mode off (`live/config.rs` `load_turnstile`): mode A from the template
   started only once `<hostname>` was replaced and the secret file held some non-empty,
   non-test text (an empty file stops startup). Both lines are now commented out with the
   reason, like the snapshot lines, and uncommented in B5 (runbook 4.1 and 8.7 updated). The
   check is weaker than an earlier draft of this text said: a placeholder secret is accepted
   (`TurnstileSecret::new` refuses only an empty value, `load_turnstile` only a test secret
   besides read errors), hence the "replaced" check of B3.
3. The runbook's web build (3.3) did not mention `NEXT_PUBLIC_TURNSTILE_SITE_KEY`, which the
   export needs at build time for the live form to open: added.
4. Runbook 5.1 said `CF-Connecting-IP` is required on every request to the socket; the code
   requires it where a per-client limit applies (sessions, run creation, streams: `400
   client_unidentified`) and health, readiness and static pages answer without it: corrected.
   (The module docs of `public_config.rs` say the same as the old runbook text; Rust is not
   changed in this lane.)
5. The template and runbook 4.1 still called `CONSENSUS_META_API_KEY_FILE` "a parallel lane";
   it is in `config.rs` and is read only with live mode on: corrected.
6. Recorded, not changed: an unreplaced `<hostname>` in `CONSENSUS_ALLOWED_HOSTS` is accepted
   at startup (`validate_host` refuses only `/`, spaces and `://`) and then refuses every real
   request with `421`; A6 greps for placeholders before the start.

Review round (`docs/r6-deploy-ready-r`):

7. Every check of A3, A4, A7 and B2 under a `0750` tree (`/etc/consensus`,
   `/var/lib/consensus`, `/run/consensus`) ran as the SSH user without `sudo`, copied from
   runbook 3.3, which had the same lines: `[ -d /var/lib/consensus/web ]` was false for the
   SSH user, so the old web root was never moved aside and `mv web.new web` nested the export
   at `web/web.new` (seen in the mutation run: the server then starts and answers `404` on
   every page); `( cd ... && sudo sha256sum -c )` could not enter the directory; the `stat`
   lines were refused. Every such line now runs under `sudo` (`sudo test`, `sudo sh -c 'cd
   ... && sha256sum -c ...'`, `sudo stat`), in this document and in runbook 3.3, 3.4, 5.1 and
   8 (the Hyphae-settings `grep` of `public.env` there printed "no Hyphae daemon settings"
   after a refused read). The first rehearsal ran these lines as root with `sudo` dropped and
   could not see it; it now runs them from this document as an unprivileged user.
8. Mode B no longer runs B6 and B7 on the production ledger: B5 keeps `ledger/staging` and
   the new step B7b retires it and switches to the empty `ledger/public` (runbook 4.5, gate
   G11).
9. `install.sh`: in a dry run every status line reads "would: ..."; a dry run without root
   says that it cannot see inside `/etc/consensus` (A3 and runbook 3.4 now run it with
   `sudo`); `--no-systemd` creates `/etc/tmpfiles.d` and `/etc/systemd/system` when missing
   (probed in a bare `ubuntu:24.04` without the systemd package: exit 0, the layout in place).
10. The research showcase is not all real-model runs: the console runs C01, C02 and C05 are
    scripted with simulated proofs (`research/README.md`); the mode table, A7 and A11 say so.
