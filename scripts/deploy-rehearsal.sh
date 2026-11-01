#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Deploy-day rehearsal (docs/deployment/deploy-day.md, "Rehearsal"): the
# server half of the deployment, run in a throwaway ubuntu:24.04 container
# (the image linux-gates.sh pins), with the release binary of
# `sh scripts/linux-gates.sh --release` (dist/), the web export
# (apps/web/out, from `pnpm build`) and research/ of THIS checkout, all
# mounted read-only. Nothing on the host is written except the log on
# stdout; the container is removed at the end.
#
# What it does, in the order of deploy-day.md:
#   - the runbook 3.1 copy check of the binary (sha256sum -c)
#   - deploy/scripts/install.sh --dry-run (must change nothing), then
#     --no-systemd (the container has no running systemd), then again
#     (idempotence); the layout of runbook 2.3 checked path by path;
#     the tmpfiles.d line applied with systemd-tmpfiles; systemd-analyze
#     verify on the two units
#   - the web export and research/ packed and unpacked with the runbook 3.3
#     commands, checksums verified on the "server" side
#   - mode A (live OFF, no Meta key): public.env from the template,
#     credentials as deploy-day.md says, serve-public started by hand as
#     the service user with exactly the env file's variables (systemd's
#     EnvironmentFile= has no expansion: neither has this) and the
#     credentials where LoadCredential= puts them; then over the socket
#     with curl --unix-socket: health, ready, capabilities, scenarios, the
#     static pages and their CSP, the 404s of gate G1, the geo and proxy
#     header refusals, a guided run of every scenario to rest, its receipt,
#     two-session isolation (gate G2), the showcase and its receipt, the
#     research files, cloudflared's least privilege, /proc/<pid>/environ
#     (gate G4), a graceful SIGTERM stop
#   - the configuration traps of mode A (each must stop startup naming the
#     variable, or must not)
#   - with --snapshot and --hyphae: mode B's BOOT only (live ON; no request
#     ever reaches Meta or Siteverify: the container has NO network while
#     the server runs, and no live run is created): the Hyphae binary
#     installed as runbook 3.2 says and compared with the manifest, the
#     snapshot copied as runbook 3.3 says, snapshot_manifest.py check
#     --reverify with the installed binary, judge codes generated (never
#     printed), the pins, the boot check's journal line, /ready
#     evidence_ok, /capabilities, the ledger subcommand, and the startup
#     refusals of a missing pin
#
# Usage, from the repository root (Git Bash, Linux or macOS):
#   sh scripts/linux-gates.sh --release && sh apps/web/scripts/in-docker.sh pnpm build
#   bash scripts/deploy-rehearsal.sh [--snapshot <full snapshot dir> --hyphae <hyphae 4.0.0 linux binary>]
#
# The snapshot directory must be the FULL capture (proofs and witnesses
# included: a git checkout lacks them, runbook 3.3); it is mounted
# read-only and copied inside the container only. The container installs
# curl, ca-certificates, openssl, python3, systemd (for systemd-analyze
# and systemd-tmpfiles, not as PID 1) and sudo from the Ubuntu archive,
# then its network is disconnected before anything else runs.
#
# The server blocks of deploy-day.md A3 (install and verify), A4, A7 and B2
# (the snapshot) are read out of the document itself and run as an
# UNPRIVILEGED SSH user with sudo, as the owner will type them: a check
# that only works as root fails here.
#
# Exit status: 0 when every check passed, 1 otherwise (the summary lists
# the failures).

set -euo pipefail

IMAGE="ubuntu:24.04@sha256:24737cef17f4e22b1977df7d9a05e77c4e28b7da5b8911f8d8ccda136bf3573c"

if [ "${1:-}" != "--inside" ]; then
  SNAPSHOT=""
  HYPHAE=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --snapshot) SNAPSHOT="$2"; shift 2 ;;
      --hyphae) HYPHAE="$2"; shift 2 ;;
      -h|--help) sed -n '3,55p' "$0"; exit 0 ;;
      *) echo "unknown argument: $1" >&2; exit 64 ;;
    esac
  done
  if { [ -n "$SNAPSHOT" ] && [ -z "$HYPHAE" ]; } || { [ -z "$SNAPSHOT" ] && [ -n "$HYPHAE" ]; }; then
    echo "--snapshot and --hyphae go together" >&2; exit 64
  fi
  repo=$(cd "$(dirname "$0")/.." && pwd)
  for f in dist/consensus-api dist/consensus-api.sha256 dist/consensus-api.commit \
           apps/web/out/csp-manifest.json research/MANIFEST.json research/SHA256SUMS; do
    [ -f "$repo/$f" ] || { echo "missing $f: build it first (see the usage above)" >&2; exit 1; }
  done
  winpath() { if p=$(cd "$1" 2>/dev/null && pwd -W 2>/dev/null); then printf %s "$p"; else (cd "$1" && pwd); fi; }
  mounts=(--volume "$(winpath "$repo"):/src:ro")
  args=(--inside)
  if [ -n "$SNAPSHOT" ]; then
    [ -f "$SNAPSHOT/manifest.json" ] || { echo "no manifest.json in $SNAPSHOT" >&2; exit 1; }
    [ -f "$HYPHAE" ] || { echo "no file $HYPHAE" >&2; exit 1; }
    mounts+=(--volume "$(winpath "$SNAPSHOT"):/in/snapshot/$(basename "$SNAPSHOT"):ro")
    mounts+=(--volume "$(winpath "$(dirname "$HYPHAE")"):/in/hyphae:ro")
    args+=(--mode-b "$(basename "$SNAPSHOT")" "/in/hyphae/$(basename "$HYPHAE")")
  fi
  export MSYS_NO_PATHCONV=1
  name="hc-deploy-rehearsal-$$"
  trap 'docker rm -f "$name" >/dev/null 2>&1 || true' EXIT
  echo "== rehearsal container $name ($IMAGE); binary from commit $(cat "$repo/dist/consensus-api.commit")"
  docker run -d --name "$name" "${mounts[@]}" --env DEBIAN_FRONTEND=noninteractive "$IMAGE" sleep infinity >/dev/null
  docker exec "$name" sh -c 'apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends curl ca-certificates openssl python3 systemd sudo >/dev/null' \
    || { echo "FAIL apt-get install in the container" >&2; exit 1; }
  docker network disconnect bridge "$name"
  echo "== network disconnected: $(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' "$name")(empty = none)"
  docker exec "$name" bash /src/scripts/deploy-rehearsal.sh "${args[@]}"
  exit $?
fi

# ---------------------------------------------------------------- inside ----
shift
# Every check below reports its own result; a failed command must not end
# the run before the summary.
set +e
MODE_B_ID=""
MODE_B_HYPHAE=""
if [ "${1:-}" = "--mode-b" ]; then MODE_B_ID="$2"; MODE_B_HYPHAE="$3"; fi

H=portal.rehearsal.test
SOCK=/run/consensus/public.sock
CRED=/run/credentials/consensus-public.service
PASSED=0
FAILURES=()
pass() { PASSED=$((PASSED + 1)); echo "PASS $*"; }
fail() { FAILURES+=("$*"); echo "FAIL $*"; }
expect() { if [ "$2" = "$3" ]; then pass "$1 ($3)"; else fail "$1: expected $2, got $3"; fi; }
expect_match() { if printf '%s' "$3" | grep -qE -- "$2"; then pass "$1"; else fail "$1: no match for /$2/ in: $(printf '%s' "$3" | head -c 400)"; fi; }
section() { echo; echo "== $*"; }

# curl on the socket with the headers cloudflared adds (runbook 5.1)
c() { curl -sS --unix-socket "$SOCK" -H "Host: $H" -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' "$@"; }
code() { c -o /dev/null -w '%{http_code}' "$@"; }
json() { python3 -I -c "import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1], {'d': d}))" "$1"; }

# serve-public as consensus-public.service would run it: the service user,
# umask 0027, ONLY the variables of the env file (no expansion), the
# credentials where LoadCredential= puts them. Not emulated: the sandbox
# directives. Every launch execs, so $! is the server's own PID.
SERVER_PID=""
env_args() { grep -E '^[A-Z_][A-Z0-9_]*=' "$1"; }
build_cmd() {
  CMD=(setpriv --reuid=consensus --regid=consensus --init-groups
       env -i PATH=/usr/local/bin:/usr/bin:/bin HOME=/var/lib/consensus CREDENTIALS_DIRECTORY="$CRED")
  while IFS= read -r line; do CMD+=("$line"); done < <(env_args "$1")
  CMD+=(/usr/local/bin/consensus-api)
}
run_as_service() {
  build_cmd "$1"; shift
  ( umask 0027; cd /var/lib/consensus && exec "${CMD[@]}" "$@" )
}
start_server() {
  rm -f "$SOCK"
  build_cmd "$1"
  ( umask 0027; cd /var/lib/consensus && exec "${CMD[@]}" serve-public ) >"$2" 2>&1 &
  SERVER_PID=$!
  for _ in $(seq 1 300); do
    [ -S "$SOCK" ] && return 0
    kill -0 "$SERVER_PID" 2>/dev/null || { wait "$SERVER_PID"; return 1; }
    sleep 0.2
  done
  return 1
}
stop_server() {
  kill -TERM "$SERVER_PID" 2>/dev/null
  local rc=0
  wait "$SERVER_PID" || rc=$?
  SERVER_PID=""
  return $rc
}
# startup_refused <label> <env file> <regex>: serve-public must exit non-zero
# within 20 s with a line matching the regex (the variable's name).
startup_refused() {
  local rc=0 out
  build_cmd "$2"
  out=$(cd /var/lib/consensus && timeout 20 "${CMD[@]}" serve-public 2>&1) || rc=$?
  rm -f "$SOCK"
  if [ $rc -ne 0 ] && [ $rc -ne 124 ] && printf '%s' "$out" | grep -qE -- "$3"; then
    pass "$1: startup refused (exit $rc): $(printf '%s' "$out" | grep -E -- "$3" | head -1 | cut -c1-220)"
  else
    fail "$1: expected a refused startup matching /$3/, got exit $rc: $(printf '%s' "$out" | tail -3 | cut -c1-300)"
  fi
}
# startup_ok <label> <env file>: serve-public must still be listening after 3 s.
startup_ok() {
  if start_server "$2" /tmp/startup-ok.log && sleep 3 && kill -0 "$SERVER_PID" 2>/dev/null; then
    pass "$1: starts and listens"
    stop_server || true
  else
    fail "$1: did not start: $(tail -3 /tmp/startup-ok.log | cut -c1-300)"
  fi
}

# The owner's SSH user: no root, sudo without a password (the VPS's sudo
# asks for one; that changes nothing below).
ADMIN=deployer
DOC=/src/docs/deployment/deploy-day.md
useradd -m -s /bin/bash "$ADMIN"
printf '%s ALL=(ALL) NOPASSWD: ALL\n' "$ADMIN" > /etc/sudoers.d/90-rehearsal && chmod 0440 /etc/sudoers.d/90-rehearsal
as_admin() {
  setpriv --reuid="$ADMIN" --regid="$ADMIN" --init-groups \
    env -i PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin HOME="/home/$ADMIN" \
    bash -c "cd /home/$ADMIN && $1" 2>&1
}
# doc_block <heading prefix> <n>: the n-th ```sh block under the "### "
# heading that starts with the prefix, exactly as deploy-day.md has it.
doc_block() {
  tr -d '\r' < "$DOC" | awk -v h="$1" -v n="$2" '
    index($0, h) == 1 { inh = 1; k = 0; next }
    inh && /^### / { inh = 0 }
    inh && /^```sh$/ { k++; inb = (k == n); next }
    inb && /^```$/ { inb = 0; next }
    inb { print }'
}
# no_denied <label> <output>: an unprivileged check that silently failed
expect_no_denied() { expect "$1: no 'Permission denied' / 'cannot'" "" "$(printf '%s' "$2" | grep -E 'Permission denied|cannot (statx|access|open)|can.t cd' | head -3)"; }

section "baseline (runbook 2.1)"
. /etc/os-release
echo "$PRETTY_NAME; $(ldd --version | head -1)"
expect "Ubuntu 24.04" "24.04" "$VERSION_ID"
expect_match "glibc 2.39" '2\.39' "$(ldd --version | head -1)"
echo "network interfaces: $(ls /sys/class/net | tr '\n' ' ')"
# No request leaves this container to prove it: only the loopback interface
# and no route at all (the header line of /proc/net/route aside).
expect "no network: the only interface is lo (nothing can reach Meta or Siteverify)" "lo" "$(ls /sys/class/net | tr '\n' ' ' | sed 's/ $//')"
expect "no network: no route in /proc/net/route" 0 "$(tail -n +2 /proc/net/route | wc -l)"

section "binary copy check (runbook 3.1)"
cp /src/dist/consensus-api /src/dist/consensus-api.sha256 /tmp/
tar -czf /tmp/deploy.tar.gz -C /src deploy
# the owner scp's these as the SSH user, so the SSH user owns them
chown "$ADMIN:$ADMIN" /tmp/consensus-api /tmp/consensus-api.sha256 /tmp/deploy.tar.gz
expect_match "sha256sum -c consensus-api.sha256" ': OK$' "$(cd /tmp && sha256sum -c consensus-api.sha256)"
echo "binary sha256 $(cut -d' ' -f1 /tmp/consensus-api.sha256), commit $(cat /src/dist/consensus-api.commit)"

section "install.sh (runbook 3.4; deploy-day.md A3 as the SSH user)"
mkdir -p /root/consensus
tar -xzf /tmp/deploy.tar.gz -C /root/consensus
DIGEST=$(cut -d' ' -f1 /tmp/consensus-api.sha256)
# A3's server block, every line but the real install (that one needs a
# running systemd: the rehearsal installs with --no-systemd below).
a3=$(doc_block "### A3." 2 | grep -v 'install.sh --binary /tmp/consensus-api --sha256 "\$D"$')
echo "$a3" | sed 's/^/   $ /'
expect_match "A3 block read from deploy-day.md holds the sudo dry run" '^sudo .*install\.sh .*--dry-run' "$a3"
out=$(as_admin "$a3") && rc=0 || rc=$?
echo "$out" | grep -v 'would run' | sed 's/^/   | /' | head -30
expect "A3 server block as the SSH user (sha256sum -c, unpack, sudo install.sh --dry-run) exit" 0 "$rc"
expect_match "A3: consensus-api: OK" 'consensus-api: OK' "$out"
expect_match "A3: the dry run completes" 'dry run complete: nothing was changed' "$out"
expect "A3: every status line of the dry run reads 'would'" "" "$(printf '%s' "$out" | grep -E '^   /' | head -3)"
echo "$out" | grep -c 'would run' | sed 's/^/   dry run: changes it would make: /'
if [ -e /etc/consensus ] || getent passwd consensus >/dev/null || [ -e /usr/local/bin/consensus-api ]; then
  fail "install.sh --dry-run changed the system"
else
  pass "install.sh --dry-run changed nothing"
fi
out=$(/root/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 0000 --dry-run 2>&1) && rc=0 || rc=$?
expect "install.sh refuses a wrong digest (dry run too)" 1 "$rc"
/root/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 "$DIGEST" --no-systemd >/tmp/install.log 2>&1 && rc=0 || rc=$?
expect "install.sh --no-systemd exit" 0 "$rc"
sed 's/^/   | /' /tmp/install.log | head -40
/root/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 "$DIGEST" --no-systemd >/tmp/install2.log 2>&1 && rc=0 || rc=$?
expect "install.sh second run (idempotent) exit" 0 "$rc"
expect_match "second run leaves public.env as is" 'public.env exists; left as is' "$(cat /tmp/install2.log)"
out=$(as_admin "~/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 $DIGEST --dry-run") && rc=0 || rc=$?
expect_match "a re-run dry run WITHOUT sudo says it cannot see inside /etc/consensus" 'cannot inspect /etc/consensus without root' "$out"
out=$(as_admin "sudo ~/consensus/deploy/scripts/install.sh --binary /tmp/consensus-api --sha256 $DIGEST --dry-run") && rc=0 || rc=$?
expect_match "a re-run dry run with sudo sees the existing public.env" 'public.env exists; left as is' "$out"

section "layout (runbook 2.3)"
check_path() { expect "$1" "$2" "$(stat -c '%a %U:%G' "$1" 2>&1)"; }
check_path /etc/consensus "750 root:consensus"
check_path /etc/consensus/credentials "700 root:root"
check_path /etc/consensus/public.env "640 root:consensus"
check_path /etc/consensus/operator.env "640 root:consensus"
check_path /var/lib/consensus "750 consensus:consensus"
for d in runs ledger ledger/staging ledger/public research web; do check_path "/var/lib/consensus/$d" "750 consensus:consensus"; done
check_path /var/lib/consensus/ledger-archive "700 root:root"
check_path /var/lib/consensus/snapshots "700 consensus:consensus"
check_path /opt/hyphae "755 hyphae:hyphae"
check_path /opt/hyphae/bin "755 hyphae:hyphae"
check_path /etc/cloudflared "750 root:cloudflared"
check_path /etc/cloudflared/config.yml "640 root:cloudflared"
check_path /usr/local/bin/consensus-api "755 root:root"
check_path /run/consensus "2750 consensus:consensus-sock"
check_path /etc/tmpfiles.d/consensus.conf "644 root:root"
expect "cloudflared's groups" "cloudflared consensus-sock" "$(id -nG cloudflared)"
expect "consensus's login shell" "/usr/sbin/nologin" "$(getent passwd consensus | cut -d: -f7)"
rm -rf /run/consensus
systemd-tmpfiles --create /etc/tmpfiles.d/consensus.conf
check_path /run/consensus "2750 consensus:consensus-sock"
echo "   (the line above: created by systemd-tmpfiles from deploy/tmpfiles.d/consensus.conf, as at boot)"
out=$(systemd-analyze verify /etc/systemd/system/consensus-public.service /etc/systemd/system/consensus-operator.service 2>&1) && rc=0 || rc=$?
echo "$out" | sed 's/^/   | /'
expect "systemd-analyze verify (both units)" 0 "$rc"
cmp -s /src/deploy/etc/consensus/public.env.example /etc/consensus/public.env && pass "public.env is the template" || fail "public.env differs from the template"
# Mode A starts from the template only with both Turnstile lines commented
# out (an empty secret file stops startup; deploy-day.md A6).
expect "the template has no active CONSENSUS_TURNSTILE_* line" 0 "$(grep -c '^CONSENSUS_TURNSTILE_' /etc/consensus/public.env)"
v=$(doc_block "### A3." 3)
out=$(as_admin "$v") && rc=0 || rc=$?
echo "$out" | sed 's/^/   | /'
expect "A3 verify block as the SSH user exit" 0 "$rc"
for l in "750 root:consensus /etc/consensus" "700 root:root /etc/consensus/credentials" "640 root:consensus /etc/consensus/public.env" \
         "700 consensus:consensus /var/lib/consensus/snapshots" "2750 consensus:consensus-sock /run/consensus" "755 root:root /usr/local/bin/consensus-api" \
         "cloudflared consensus-sock"; do
  expect "A3 verify as the SSH user prints '$l'" 1 "$(printf '%s\n' "$out" | grep -cx -- "$l")"
done
expect_no_denied "A3 verify as the SSH user" "$out"

section "web export and research archive (runbook 3.3)"
( cd /src/apps/web/out && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum ) > /tmp/web.SHA256SUMS
tar -czf /tmp/web.tar.gz -C /src/apps/web/out .
echo "   web export: $(wc -l </tmp/web.SHA256SUMS) files, $(du -h /tmp/web.tar.gz | cut -f1) packed"
tar -czf /tmp/research.tar.gz -C /src research
chown "$ADMIN:$ADMIN" /tmp/web.SHA256SUMS /tmp/web.tar.gz /tmp/research.tar.gz
expect "before A4: /var/lib/consensus/web exists (install.sh made it)" 1 "$([ -d /var/lib/consensus/web ] && echo 1 || echo 0)"
# deploy-day.md A4, exactly as written, as the SSH user
a4=$(doc_block "### A4." 1)
out=$(as_admin "$a4") && rc=0 || rc=$?
echo "$out" | sed 's/^/   | /'
expect_match "A4: web export verified" '^web export verified$' "$out"
expect_match "A4: csp manifest present" '^csp manifest present$' "$out"
expect_match "A4: research archive verified" '^research archive verified$' "$out"
expect_no_denied "A4 as the SSH user" "$out"
test -f /var/lib/consensus/web/csp-manifest.json && pass "csp manifest at the web root" || fail "csp manifest missing at /var/lib/consensus/web"
[ -e /var/lib/consensus/web/web.new ] && fail "the export was nested at web/web.new" || pass "the export is not nested (no web/web.new)"
[ -e /var/lib/consensus/web.new ] && fail "web.new left behind" || pass "web.new moved into place"
expect "the old (empty) web root moved aside to web.old" 1 "$([ -d /var/lib/consensus/web.old ] && echo 1 || echo 0)"
echo "   research: $(wc -l </var/lib/consensus/research/SHA256SUMS) files"
expect "web export: owner and modes" "" "$(find /var/lib/consensus/web /var/lib/consensus/research \( ! -user consensus -o \( -type f ! -perm 0640 \) -o \( -type d ! -perm 0750 \) \) | head -3)"

section "mode A configuration (deploy-day.md A3, A4)"
install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/meta.key
printf 'LLM_0_canary-%s\n' "$(openssl rand -hex 12)" > /etc/consensus/credentials/meta.key
CANARY=$(cat /etc/consensus/credentials/meta.key)
install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/turnstile.secret
printf 'unused-while-live-mode-is-off\n' > /etc/consensus/credentials/turnstile.secret
install -o root -g root -m 0600 /dev/null /etc/consensus/credentials/operator.token
openssl rand -hex 32 > /etc/consensus/credentials/operator.token
sed -i "s/<hostname>/$H/g" /etc/consensus/public.env
expect "no <placeholder> left on an active line of public.env" "" "$(grep -E '^[A-Z_]+=.*<' /etc/consensus/public.env || true)"
echo "   the active lines serve-public receives:"
env_args /etc/consensus/public.env | sed 's/^/   | /'
# LoadCredential=: systemd copies each file into /run/credentials/<unit>/,
# readable by the unit's user only.
install -d -o consensus -g consensus -m 0500 "$CRED"
for f in meta.key turnstile.secret; do install -o consensus -g consensus -m 0400 "/etc/consensus/credentials/$f" "$CRED/$f"; done

section "mode A: serve-public by hand (live OFF)"
if start_server /etc/consensus/public.env /tmp/serve-a.log; then
  pass "serve-public listens on $SOCK"
else
  fail "serve-public did not start: $(tail -5 /tmp/serve-a.log)"
fi
sleep 1
sed 's/^/   | /' /tmp/serve-a.log
expect_match "startup line: live mode disabled" 'Public live mode: disabled \(CONSENSUS_LIVE_ENABLED\)' "$(cat /tmp/serve-a.log)"
expect_match "startup line: geo block" 'Portal-wide geographic block: 23 countries; live-only: KR' "$(cat /tmp/serve-a.log)"
expect "no [snapshot] line with the snapshot lines commented out" "" "$(grep '\[snapshot\]' /tmp/serve-a.log || true)"
check_path "$SOCK" "660 consensus:consensus-sock"

expect "GET /api/v1/health" 200 "$(code http://localhost/api/v1/health)"
health=$(c http://localhost/api/v1/health); echo "   health: $health"
expect_match "health: executor disabled" '"executor":"disabled"' "$health"
expect "GET /api/v1/health without CF-IPCountry (geo unknown)" 451 "$(curl -sS --unix-socket "$SOCK" -H "Host: $H" -H 'CF-Connecting-IP: 203.0.113.10' -o /dev/null -w '%{http_code}' http://localhost/api/v1/health)"
expect "GET /api/v1/health from a blocked country (UA)" 451 "$(curl -sS --unix-socket "$SOCK" -H "Host: $H" -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: UA' -o /dev/null -w '%{http_code}' http://localhost/api/v1/health)"
# CF-Connecting-IP is the client key: required wherever a per-client limit
# applies (sessions, run creation, streams), not on health or static pages.
expect "GET /api/v1/health without CF-Connecting-IP (no per-client limit there)" 200 "$(curl -sS --unix-socket "$SOCK" -H "Host: $H" -H 'CF-IPCountry: US' -o /dev/null -w '%{http_code}' http://localhost/api/v1/health)"
noip=$(curl -sS --unix-socket "$SOCK" -H "Host: $H" -H 'CF-IPCountry: US' -H "Origin: https://$H" -X POST -o /tmp/noip.json -w '%{http_code}' http://localhost/api/v1/session)
if [ "$noip" -ge 400 ]; then pass "POST /api/v1/session without CF-Connecting-IP refused ($noip $(cat /tmp/noip.json | head -c 120))"; else fail "POST /api/v1/session without CF-Connecting-IP answered $noip"; fi
badhost=$(curl -sS --unix-socket "$SOCK" -H 'Host: other.example' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' -o /dev/null -w '%{http_code}' http://localhost/api/v1/health)
if [ "$badhost" -ge 400 ]; then pass "a Host outside CONSENSUS_ALLOWED_HOSTS refused ($badhost)"; else fail "a foreign Host answered $badhost"; fi
ready=$(c http://localhost/api/v1/ready); echo "   ready: $ready"
expect_match "ready: true" '"ready":true' "$ready"
caps=$(c http://localhost/api/v1/capabilities); echo "   capabilities: $(printf '%s' "$caps" | head -c 600)"
expect_match "capabilities: live_enabled false" '"live_enabled":false' "$caps"
scen=$(c http://localhost/api/v1/scenarios)
SCENARIOS=$(printf '%s' "$scen" | python3 -I -c "import json,sys,re; print(' '.join(sorted(set(re.findall(r'\"id\":\"([a-z_]+)\"', sys.stdin.read())))))")
echo "   scenario ids: $SCENARIOS"
for s in retry offline invalid_proof injection abstain; do expect_match "scenario $s listed" "(^| )$s( |$)" "$SCENARIOS"; done

for p in / /en/ /es/ /pt-BR/ /fr/ /en/showcase/ /en/runs/; do
  expect "GET $p" 200 "$(code "http://localhost$p")"
done
hdr=$(c -D - -o /dev/null http://localhost/en/)
expect_match "/en/ has a Content-Security-Policy" '^[Cc]ontent-[Ss]ecurity-[Pp]olicy: ' "$hdr"
expect_match "/en/ X-Content-Type-Options nosniff" '[Xx]-[Cc]ontent-[Tt]ype-[Oo]ptions: nosniff' "$hdr"
expect_match "/en/ X-Frame-Options DENY" '[Xx]-[Ff]rame-[Oo]ptions: DENY' "$hdr"
expect_match "/en/ Referrer-Policy no-referrer" '[Rr]eferrer-[Pp]olicy: no-referrer' "$hdr"
expect "G1: GET /api/runs (no v0 router)" 404 "$(code http://localhost/api/runs)"
expect "G1: / is not the operator console" 0 "$(c http://localhost/ | grep -c 'Operator console' || true)"
expect "G1: POST /api/v1/operator/runs" 404 "$(code -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{}' http://localhost/api/v1/operator/runs)"
expect "G1: an unknown path" 404 "$(code http://localhost/no-such-path-$RANDOM)"
expect "POST /api/v1/live-runs with live mode off is never 202" 1 "$([ "$(code -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{}' http://localhost/api/v1/live-runs)" != 202 ] && echo 1 || echo 0)"

section "mode A: a guided run of every scenario, two-session isolation (G2)"
session() { c -D "$1" -o /dev/null -X POST -H "Origin: https://$H" http://localhost/api/v1/session; grep -i '^set-cookie:' "$1" | head -1 | sed 's/^[^:]*: *//; s/;.*//' | tr -d '\r'; }
# curl keeps a Secure cookie off a plain-http URL, so the cookie goes back by hand
CA=$(session /tmp/a.h); CB=$(session /tmp/b.h)
expect_match "session cookie is __Host-hc_session" '^__Host-hc_session=' "$CA"
expect_match "session cookie flags: Secure, HttpOnly, SameSite" '[Ss]ecure.*[Hh]ttp[Oo]nly|[Hh]ttp[Oo]nly.*[Ss]ecure' "$(grep -i '^set-cookie:' /tmp/a.h)"
RUNS=()
for s in retry offline invalid_proof injection abstain; do
  created=$(c -H "Cookie: $CA" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data "{\"scenario\":\"$s\"}" -w '\n%{http_code}' http://localhost/api/v1/runs)
  status=$(printf '%s' "$created" | tail -1)
  run=$(printf '%s' "$created" | head -n -1 | json "d['run']['run_id']" 2>/dev/null || echo "")
  expect "POST /api/v1/runs {scenario:$s}" 202 "$status"
  [ -n "$run" ] && RUNS+=("$s:$run")
done
for entry in "${RUNS[@]}"; do
  s=${entry%%:*}; run=${entry#*:}
  state=""
  for _ in $(seq 1 100); do
    detail=$(c -H "Cookie: $CA" "http://localhost/api/v1/runs/$run")
    active=$(printf '%s' "$detail" | json "d['summary']['active']" 2>/dev/null || echo "?")
    state=$(printf '%s' "$detail" | json "d['summary']['state']" 2>/dev/null || echo "?")
    [ "$active" = "False" ] && break
    sleep 0.3
  done
  rec=$(printf '%s' "$detail" | json "d['summary'].get('recommendation')" 2>/dev/null || echo "?")
  prov=$(printf '%s' "$detail" | json "d['summary']['provenance']['evidence'] + '/' + d['summary']['provenance']['model']" 2>/dev/null || echo "?")
  if [ "$active" = "False" ]; then pass "guided $s at rest: state $state, recommendation $rec, provenance $prov"; else fail "guided $s never came to rest (state $state)"; fi
  # evidence simulated (none for abstain, which retrieves nothing), model scripted: never live
  expect_match "guided $s provenance: scripted model, simulated or no evidence (never live)" '^(simulated|none)/scripted$' "$prov"
  expect "guided $s receipt" 200 "$(code -H "Cookie: $CA" "http://localhost/api/v1/runs/$run/receipt")"
  expect "guided $s events.ndjson" 200 "$(code -H "Cookie: $CA" "http://localhost/api/v1/runs/$run/events.ndjson")"
done
run=${RUNS[0]#*:}
for p in "" /events.ndjson /receipt; do
  expect "G2: another session reads /api/v1/runs/<run>$p" 404 "$(code -H "Cookie: $CB" "http://localhost/api/v1/runs/$run$p")"
done
expect "G2: another session cancels" 404 "$(code -H "Cookie: $CB" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{}' "http://localhost/api/v1/runs/$run/cancel")"
expect "G2: the run is not in another session's list" 0 "$(c -H "Cookie: $CB" http://localhost/api/v1/runs | grep -c "$run" || true)"
expect "the owner's list holds the five runs" 5 "$(c -H "Cookie: $CA" http://localhost/api/v1/runs | python3 -I -c "import json,sys; print(len(json.load(sys.stdin)['runs']))")"
sse=$(c -H "Cookie: $CA" -m 5 "http://localhost/api/v1/runs/$run/events" || true)
expect_match "SSE of a resting run: stream-start, replayed frames, stream_end" '^: stream-start' "$sse"
expect_match "SSE ends with stream_end" 'event: stream_end' "$sse"
expect "a decision POST without Origin is refused" 1 "$([ "$(code -H "Cookie: $CA" -X POST -H 'Content-Type: application/json' --data '{"decision":"approve"}' "http://localhost/api/v1/runs/$run/decision")" -ge 400 ] && echo 1 || echo 0)"

section "mode A: the showcase and the research files"
show=$(c http://localhost/api/v1/showcase)
SHOW_IDS=$(printf '%s' "$show" | python3 -I -c "import sys,re; ids=sorted(set(re.findall(r'\"run_id\":\"([^\"]+)\"', sys.stdin.read()))); print(len(ids), ids[0] if ids else '')")
echo "   showcase: $SHOW_IDS (entries, first id)"
n=${SHOW_IDS%% *}; first=${SHOW_IDS#* }
if [ "${n:-0}" -gt 0 ]; then pass "showcase lists $n recorded runs"; else fail "showcase is empty"; fi
expect "showcase detail of $first" 200 "$(code "http://localhost/api/v1/showcase/$first")"
expect "showcase receipt of $first" 200 "$(code "http://localhost/api/v1/showcase/$first/receipt")"
expect_match "showcase detail: provenance source research_archive (a recorded run, served from research/)" '"source":"research_archive"' "$(c "http://localhost/api/v1/showcase/$first" | head -c 20000)"
expect "GET /research/MANIFEST.json" 200 "$(code http://localhost/research/MANIFEST.json)"
expect "GET /research/SHA256SUMS" 200 "$(code http://localhost/research/SHA256SUMS)"
expect "/research/MANIFEST.json is the archive byte for byte" "$(sha256sum </var/lib/consensus/research/MANIFEST.json | cut -d' ' -f1)" "$(c http://localhost/research/MANIFEST.json | sha256sum | cut -d' ' -f1)"
ready=$(c http://localhost/api/v1/ready)
expect_match "ready: showcase ok" '"showcase":\{[^}]*"ok":true' "$ready"

section "mode A: least privilege of cloudflared (runbook 5.1), the key (G4)"
expect "cloudflared -> socket" 200 "$(setpriv --reuid=cloudflared --regid=cloudflared --init-groups curl -sS --unix-socket "$SOCK" -H "Host: $H" -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' -o /dev/null -w '%{http_code}' http://localhost/api/v1/health)"
for p in /etc/consensus/public.env /etc/consensus/credentials/operator.token /var/lib/consensus/snapshots /var/lib/consensus/runs /var/lib/consensus/ledger "$CRED/meta.key"; do
  if setpriv --reuid=cloudflared --regid=cloudflared --init-groups ls -d "$p" >/dev/null 2>&1 && setpriv --reuid=cloudflared --regid=cloudflared --init-groups cat "$p" >/dev/null 2>&1; then
    fail "LEAK: cloudflared reads $p"
  else
    pass "cloudflared cannot read $p"
  fi
done
pid=$(pgrep -f 'consensus-api serve-public' | head -1)
# Docker withholds CAP_SYS_PTRACE, so container root cannot read another
# user's environ: read it as the service user (on the server: sudo, runbook 8.3).
setpriv --reuid=consensus --regid=consensus --init-groups cat "/proc/$pid/environ" | tr '\0' '\n' > /tmp/environ.txt
if [ -s /tmp/environ.txt ]; then pass "G4: /proc/<pid>/environ read ($(wc -l </tmp/environ.txt) variables)"; else fail "G4: /proc/<pid>/environ could not be read"; fi
expect "G4: no LLM_ key in /proc/<pid>/environ" 0 "$(grep -c -E 'LLM_[0-9]+_' /tmp/environ.txt)"
expect "G4: no key variable in /proc/<pid>/environ" 0 "$(grep -c -E '^(CONSENSUS_META_API_KEY|MODEL_API_KEY|META_API_KEY)=' /tmp/environ.txt)"
expect "G4: the key FILE variable is a path only" 1 "$(grep -c -E '^CONSENSUS_META_API_KEY_FILE=/run/credentials/' /tmp/environ.txt)"
leaks=0
for f in /tmp/serve-a.log; do grep -qF "$CANARY" "$f" && leaks=$((leaks + 1)); done
grep -rqF "$CANARY" /var/lib/consensus/runs 2>/dev/null && leaks=$((leaks + 1))
for p in /api/v1/health /api/v1/ready /api/v1/capabilities /en/; do c "http://localhost$p" | grep -qF "$CANARY" && leaks=$((leaks + 1)); done
expect "G4: the canary key in no log, store file or response" 0 "$leaks"
expect "the run store is the service user's, files 0640" "" "$(find /var/lib/consensus/runs -type f ! -perm 0640 | head -3)"

section "mode A: deploy-day.md A7's socket block, exactly as written, as the SSH user"
# <hostname> filled in as the owner will; the canary-scan.sh line needs
# systemctl (the service's MainPID) and is not rehearsed here (G4 is covered
# above from /proc/<pid>/environ).
a7=$(doc_block "### A7." 2 | grep -v 'canary-scan\.sh' | sed "s/<hostname>/$H/g")
expect_match "A7 block read from deploy-day.md defines the sudo curl helper" "^S\\(\\) \\{ sudo curl " "$a7"
out=$(as_admin "$a7") && rc=0 || rc=$?
echo "$out" | cut -c1-200 | sed 's/^/   | /'
expect_no_denied "A7 as the SSH user" "$out"
expect "A7: socket owner and mode (sudo stat)" 1 "$(printf '%s\n' "$out" | grep -cx 'srw-rw---- consensus:consensus-sock')"
expect_match "A7: health JSON, executor disabled" '"executor":"disabled"' "$out"
expect_match "A7: ready true" '"ready":true' "$out"
expect_match "A7: capabilities live_enabled false" '^"live_enabled":false$' "$out"
expect_match "A7: the guided run reached awaiting_human_approval" '^"state":"awaiting_human_approval"$' "$out"
expect_match "A7: the CSP header of /en/" '^[Cc]ontent-[Ss]ecurity-[Pp]olicy: ' "$out"
expect "A7: G1 404 and the 401 without a session, each once" "1 1" "$(printf '%s\n' "$out" | grep -cx 404) $(printf '%s\n' "$out" | grep -cx 401)"
expect "A7: the 200 lines (health, /en/, receipt, showcase detail, research manifest)" 5 "$(printf '%s\n' "$out" | grep -cx 200)"
n=$(printf '%s\n' "$out" | grep -xE '[0-9]+' | grep -vxE '200|401|404' | head -1)
if [ "${n:-0}" -gt 0 ]; then pass "A7: the showcase count ($n)"; else fail "A7: the showcase count (${n:-none})"; fi
expect_match "A7: cloudflared reaches the socket" '^cloudflared -> socket: 200$' "$out"
expect "A7: cloudflared cannot read any of the four paths" 4 "$(printf '%s\n' "$out" | grep -c '^ok: cloudflared cannot read ')"
expect "A7: no LEAK line" 0 "$(printf '%s\n' "$out" | grep -c '^LEAK')"

section "mode A: graceful stop (SIGTERM)"
stop_server && rc=0 || rc=$?
expect "serve-public exits 0 on SIGTERM" 0 "$rc"
expect_match "shutdown line" '\[shutdown\]' "$(cat /tmp/serve-a.log)"
[ -e "$SOCK" ] && fail "the socket is left behind" || pass "the socket is removed on exit"
if start_server /etc/consensus/public.env /tmp/serve-a2.log; then
  # sessions live in memory (a restart ends them); the runs live in the store
  expect "restart: the old session cookie is no longer a session" 401 "$(code -H "Cookie: $CA" http://localhost/api/v1/runs)"
  expect "restart: the six runs of the first start are still in the store" 6 "$(find /var/lib/consensus/runs -name 'run-*.jsonl' | wc -l)"
  expect_match "restart: ready" '"ready":true' "$(c http://localhost/api/v1/ready)"
  stop_server || true
else
  fail "restart did not start: $(tail -3 /tmp/serve-a2.log)"
fi

section "mode A: configuration traps"
cp /etc/consensus/public.env /tmp/t.env
printf 'CONSENSUS_TURNSTILE_SECRET_FILE=%s/empty.secret\nCONSENSUS_TURNSTILE_HOSTNAME=%s\n' /tmp "$H" >> /tmp/t.env
install -o consensus -g consensus -m 0400 /dev/null /tmp/empty.secret
startup_refused "Turnstile lines set, secret file empty, live OFF" /tmp/t.env 'CONSENSUS_TURNSTILE_SECRET_FILE'
cp /etc/consensus/public.env /tmp/t.env
printf 'CONSENSUS_TURNSTILE_SECRET_FILE=%s/turnstile.secret\nCONSENSUS_TURNSTILE_HOSTNAME=<hostname>\n' "$CRED" >> /tmp/t.env
startup_refused "Turnstile hostname left as <hostname>" /tmp/t.env 'CONSENSUS_TURNSTILE_HOSTNAME'
cp /etc/consensus/public.env /tmp/t.env
printf 'CONSENSUS_TURNSTILE_SECRET_FILE=%s/turnstile.secret\n' "$CRED" >> /tmp/t.env
startup_refused "only one of the two Turnstile lines" /tmp/t.env 'CONSENSUS_TURNSTILE'
# Recorded behaviour, not a refusal (deploy-day.md A6, B3): the placeholder
# text of A5 is accepted as a secret, so B3 checks that it was replaced.
cp /etc/consensus/public.env /tmp/t.env
printf 'CONSENSUS_TURNSTILE_SECRET_FILE=%s/turnstile.secret\nCONSENSUS_TURNSTILE_HOSTNAME=%s\n' "$CRED" "$H" >> /tmp/t.env
expect "the credential copy holds the A5 placeholder" "unused-while-live-mode-is-off" "$(cat "$CRED/turnstile.secret")"
startup_ok "Turnstile lines set over the A5 placeholder secret, live OFF: ACCEPTED (hence B3's check)" /tmp/t.env
b3=$(doc_block "### B3." 1 | grep -E 'STILL THE (PLACEHOLDER|CANARY)')
expect "B3: the two 'replaced' checks read from deploy-day.md" 2 "$(printf '%s\n' "$b3" | grep -c .)"
expect_match "B3: the checks flag the A5 files as not replaced" 'STILL THE PLACEHOLDER.*STILL THE CANARY' "$(as_admin "$b3" | tr '\n' ' ')"
sed 's#^CONSENSUS_META_API_KEY_FILE=.*#CONSENSUS_META_API_KEY_FILE=/nonexistent/meta.key#' /etc/consensus/public.env > /tmp/t.env
startup_ok "live OFF: the Meta key file is never opened (a missing path starts)" /tmp/t.env
sed "s#^CONSENSUS_ALLOWED_ORIGINS=.*#CONSENSUS_ALLOWED_ORIGINS=$H#" /etc/consensus/public.env > /tmp/t.env
startup_refused "an origin without https://" /tmp/t.env 'CONSENSUS_ALLOWED_ORIGINS'
grep -v '^CONSENSUS_ALLOWED_HOSTS=' /etc/consensus/public.env > /tmp/t.env
startup_refused "CONSENSUS_ALLOWED_HOSTS missing" /tmp/t.env 'CONSENSUS_ALLOWED_HOSTS'
sed 's#^CONSENSUS_LIVE_ENABLED=.*#CONSENSUS_LIVE_ENABLED=true#' /etc/consensus/public.env > /tmp/t.env
startup_refused "live ON without the snapshot pins" /tmp/t.env 'CONSENSUS_SNAPSHOT_MANIFEST_SHA256|CONSENSUS_HYPHAE_BIN_SHA256|CONSENSUS_TURNSTILE|CONSENSUS_SNAPSHOT_DIR'
sed "s#^CONSENSUS_ALLOWED_HOSTS=.*#CONSENSUS_ALLOWED_HOSTS=<hostname>#" /etc/consensus/public.env > /tmp/t.env
if start_server /tmp/t.env /tmp/t.log; then
  st=$(code http://localhost/api/v1/health)
  stop_server || true
  # the documented behaviour (deploy-day.md #18, A6): it starts, then 421
  expect "an unreplaced <hostname> in CONSENSUS_ALLOWED_HOSTS starts, then answers the real Host" 421 "$st"
else
  fail "an unreplaced <hostname> in CONSENSUS_ALLOWED_HOSTS: documented as accepted at startup, but startup was refused: $(tail -3 /tmp/t.log | cut -c1-300)"
fi

if [ -n "$MODE_B_ID" ]; then
  section "mode B: the Hyphae binary (runbook 3.2) and the snapshot (runbook 3.3)"
  SNAP=/var/lib/consensus/snapshots/$MODE_B_ID
  install -d -o hyphae -g hyphae -m 0755 /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu
  install -o hyphae -g hyphae -m 0755 "$MODE_B_HYPHAE" /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae
  ln -sfn /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae /opt/hyphae/bin/hyphae
  BIN_SHA=$(sha256sum /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae | cut -d' ' -f1)
  echo "   hyphae --version as consensus: $(setpriv --reuid=consensus --regid=consensus --init-groups /opt/hyphae/bin/hyphae --version 2>&1 | head -1)"
  cp -r "/in/snapshot/$MODE_B_ID" /tmp/
  ( cd "/tmp/$MODE_B_ID" && sha256sum -c --quiet SHA256SUMS ) && pass "snapshot SHA256SUMS before the move" || fail "snapshot SHA256SUMS"
  chown -R "$ADMIN:$ADMIN" "/tmp/$MODE_B_ID"     # scp'd by the SSH user
  # deploy-day.md B2's server lines for the snapshot, exactly as written
  # (from the move to the cloudflared check), as the SSH user
  b2=$(doc_block "### B2." 2 | sed -n '/^sudo mv \/tmp\/snap-/,/witnesses: unreachable/p' | sed "s/snap-2026-10-29-demo-repo/$MODE_B_ID/g")
  expect "B2 snapshot lines read from deploy-day.md" 7 "$(printf '%s\n' "$b2" | grep -c .)"
  out=$(as_admin "$b2") && rc=0 || rc=$?
  echo "$out" | sed 's/^/   | /'
  expect_no_denied "B2 as the SSH user" "$out"
  expect_match "B2: snapshot verified" '^snapshot verified$' "$out"
  expect_match "B2: sudo sha256sum manifest.json prints the pin" '^b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a ' "$out"
  expect_match "B2: witnesses unreachable for cloudflared" '^witnesses: unreachable for cloudflared$' "$out"
  expect "B2: the snapshot is in place" 1 "$([ -f "$SNAP/manifest.json" ] && echo 1 || echo 0)"
  MAN_SHA=$(sha256sum "$SNAP/manifest.json" | cut -d' ' -f1)
  ANCHOR=$(python3 -I -c "import json,sys; print(json.load(open(sys.argv[1]))['anchor_hex'])" "$SNAP/manifest.json")
  MAN_BIN=$(python3 -I -c "import json,sys; print(json.load(open(sys.argv[1]))['hyphae']['binary_sha256'])" "$SNAP/manifest.json")
  expect "manifest digest = the published pin" b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a "$MAN_SHA"
  expect "anchor = the published pin" 43a2dea9c48d015bf6cbd6692b7ecf2bf80c66cd47d398b38e6b8471b18e8f02 "$ANCHOR"
  expect "installed hyphae binary = manifest hyphae.binary_sha256" "$MAN_BIN" "$BIN_SHA"
  check_path /var/lib/consensus/snapshots "700 consensus:consensus"
  check_path "$SNAP" "750 consensus:consensus"
  expect "snapshot files 0640, consensus-owned" "" "$(find /var/lib/consensus/snapshots \( ! -user consensus -o \( -type f ! -perm 0640 \) \) | head -3)"
  setpriv --reuid=cloudflared --regid=cloudflared --init-groups ls /var/lib/consensus/snapshots >/dev/null 2>&1 && fail "cloudflared lists the snapshots" || pass "witnesses: unreachable for cloudflared"
  out=$(setpriv --reuid=consensus --regid=consensus --init-groups env -i PATH=/usr/bin:/bin HOME=/tmp python3 -I /src/scripts/snapshot_manifest.py check "$SNAP" --reverify /opt/hyphae/bin/hyphae 2>&1) && rc=0 || rc=$?
  echo "$out" | tail -4 | sed 's/^/   | /'
  expect "snapshot_manifest.py check --reverify with the installed binary (G9 deployment review)" 0 "$rc"

  section "mode B: judge codes (runbook 4.3), credentials, public.env"
  /root/consensus/deploy/scripts/judge-codes.sh --count 2 >/dev/null 2>&1 && rc=0 || rc=$?
  expect "judge-codes.sh --count 2 (codes not printed here)" 0 "$rc"
  check_path /etc/consensus/judge-codes.sha256 "640 root:consensus"
  expect "judge digests" 2 "$(grep -c -E '^[0-9a-f]{64}$' /etc/consensus/judge-codes.sha256)"
  /root/consensus/deploy/scripts/judge-codes.sh --count 2 >/dev/null 2>&1 && rc=0 || rc=$?
  expect "judge-codes.sh refuses to overwrite without --force" 1 "$([ "$rc" -ne 0 ] && echo 1 || echo 0)"
  # A non-test secret that is not a real one: no request leaves this container.
  printf 'rehearsal-%s\n' "$(openssl rand -hex 16)" > /etc/consensus/credentials/turnstile.secret
  install -o consensus -g consensus -m 0400 /etc/consensus/credentials/turnstile.secret "$CRED/turnstile.secret"
  expect_match "B3: the check now reads 'turnstile.secret replaced'" 'turnstile.secret replaced' "$(as_admin "$b3")"
  cp /etc/consensus/public.env /tmp/b.env
  sed -i \
    -e "s#^\# CONSENSUS_HYPHAE_BIN_SHA256=.*#CONSENSUS_HYPHAE_BIN_SHA256=$BIN_SHA#" \
    -e "s#^\# CONSENSUS_HYPHAE_EXPECTED_ANCHOR=.*#CONSENSUS_HYPHAE_EXPECTED_ANCHOR=$ANCHOR#" \
    -e "s#^\# CONSENSUS_SNAPSHOT_DIR=.*#CONSENSUS_SNAPSHOT_DIR=$SNAP#" \
    -e "s#^\# CONSENSUS_SNAPSHOT_MANIFEST_SHA256=.*#CONSENSUS_SNAPSHOT_MANIFEST_SHA256=$MAN_SHA#" \
    -e "s#^\# CONSENSUS_TURNSTILE_SECRET_FILE=#CONSENSUS_TURNSTILE_SECRET_FILE=#" \
    -e "s#^\# CONSENSUS_TURNSTILE_HOSTNAME=.*#CONSENSUS_TURNSTILE_HOSTNAME=$H#" \
    -e "s#^\# CONSENSUS_LIVE_JUDGE_CODES_FILE=#CONSENSUS_LIVE_JUDGE_CODES_FILE=#" \
    -e "s#^CONSENSUS_LIVE_ENABLED=.*#CONSENSUS_LIVE_ENABLED=true#" \
    /tmp/b.env
  echo "   mode B: the active lines that differ from mode A:"
  diff <(env_args /etc/consensus/public.env) <(env_args /tmp/b.env) | grep '^>' | sed 's/^/   | /'
  expect "mode B (B5): exactly eight active lines change" 8 "$(diff <(env_args /etc/consensus/public.env) <(env_args /tmp/b.env) | grep -c '^>')"
  # B6 and B7 run on the staging ledger; B7b switches to ledger/public
  expect "mode B (B5): the ledger stays the staging one" 1 "$(grep -cx 'CONSENSUS_LIVE_LEDGER_DIR=/var/lib/consensus/ledger/staging' /tmp/b.env)"
  for v in CONSENSUS_HYPHAE_BIN_SHA256 CONSENSUS_HYPHAE_EXPECTED_ANCHOR CONSENSUS_SNAPSHOT_DIR CONSENSUS_SNAPSHOT_MANIFEST_SHA256 CONSENSUS_TURNSTILE_SECRET_FILE CONSENSUS_TURNSTILE_HOSTNAME CONSENSUS_LIVE_JUDGE_CODES_FILE; do
    expect "mode B: $v uncommented by the documented edit" 1 "$(grep -c "^$v=" /tmp/b.env)"
  done

  section "mode B: BOOT with live ON (no network: no Meta, no Siteverify)"
  if start_server /tmp/b.env /tmp/serve-b.log; then pass "serve-public (live ON) listens"; else fail "serve-public (live ON) did not start: $(tail -5 /tmp/serve-b.log)"; fi
  sleep 1
  sed 's/^/   | /' /tmp/serve-b.log
  log=$(cat /tmp/serve-b.log)
  expect_match "journal: Sealed snapshot evidence line" "Sealed snapshot evidence: $MODE_B_ID \\(project demo-repo, 7 records, anchor 43a2dea9c48d" "$log"
  expect_match "journal: manifest pinned b731346c8464" 'manifest sha256 pinned b731346c8464' "$log"
  expect_match "journal: verifier binary pinned 79025e8758ee" 'verifier binary sha256 pinned 79025e8758ee' "$log"
  expect_match "journal: proofs re-verified at boot" 'proofs re-verified at boot' "$log"
  expect_match "journal: live mode enabled, judge lane open" 'Public live mode: enabled on the owner.s key \(daily cap \$?10\.00.*lifetime cap \$?60\.00.*judge lane open, 5 runs per code a day\)' "$log"
  expect "journal: no [snapshot] unavailable line" "" "$(grep 'live evidence unavailable' /tmp/serve-b.log || true)"
  ready=$(c http://localhost/api/v1/ready); echo "   ready: $ready"
  expect_match "ready: true, evidence_ok true, ledger_ok true, not tripped" '"evidence_ok":true' "$ready"
  expect_match "ready: ledger_ok" '"ledger_ok":true' "$ready"
  expect_match "ready: not tripped" '"tripped":false' "$ready"
  caps=$(c http://localhost/api/v1/capabilities); echo "   capabilities: $(printf '%s' "$caps" | head -c 900)"
  expect_match "capabilities: live_enabled true" '"live_enabled":true' "$caps"
  expect_match "capabilities: requires_turnstile true" '"requires_turnstile":true' "$caps"
  out=$(run_as_service /tmp/b.env ledger status 2>&1) && rc=0 || rc=$?
  echo "$out" | head -c 600 | sed 's/^/   | /'; echo
  expect "consensus-api ledger status (what ledger.sh runs under systemd-run)" 0 "$rc"
  out=$(/root/consensus/deploy/scripts/kill-switch.sh status --env /tmp/b.env 2>&1) && rc=0 || rc=$?
  echo "$out" | head -8 | sed 's/^/   | /'
  expect "guided runs still work with live ON" 202 "$(code -H "Cookie: $(session /tmp/c.h)" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{"scenario":"retry"}' http://localhost/api/v1/runs)"
  stop_server && rc=0 || rc=$?
  expect "serve-public (live ON) exits 0 on SIGTERM" 0 "$rc"

  section "mode B: refusals at startup"
  grep -v '^CONSENSUS_SNAPSHOT_MANIFEST_SHA256=' /tmp/b.env > /tmp/t.env
  startup_refused "live ON, manifest pin missing" /tmp/t.env 'CONSENSUS_SNAPSHOT_MANIFEST_SHA256'
  grep -v '^CONSENSUS_HYPHAE_BIN_SHA256=' /tmp/b.env > /tmp/t.env
  startup_refused "live ON, verifier pin missing" /tmp/t.env 'CONSENSUS_HYPHAE_BIN_SHA256'
  grep -v '^CONSENSUS_TURNSTILE_SECRET_FILE=' /tmp/b.env > /tmp/t.env
  startup_refused "live ON, Turnstile secret file missing" /tmp/t.env 'CONSENSUS_TURNSTILE_SECRET_FILE'
  sed 's#^CONSENSUS_SNAPSHOT_MANIFEST_SHA256=.*#CONSENSUS_SNAPSHOT_MANIFEST_SHA256=0000000000000000000000000000000000000000000000000000000000000000#' /tmp/b.env > /tmp/t.env
  if start_server /tmp/t.env /tmp/t.log; then
    sleep 1
    expect_match "a wrong manifest pin: portal up, [snapshot] manifest_pin_mismatch" 'live evidence unavailable: manifest_pin_mismatch' "$(cat /tmp/t.log)"
    ready=$(c http://localhost/api/v1/ready); echo "   ready: $ready"
    expect_match "a wrong manifest pin: evidence_ok false" '"evidence_ok":false' "$ready"
    expect_match "a wrong manifest pin: ready false" '^\{"ready":false' "$ready"
    expect_match "a wrong manifest pin: capabilities live_enabled false" '"live_enabled":false' "$(c http://localhost/api/v1/capabilities)"
    expect "a wrong manifest pin: health still 200" 200 "$(code http://localhost/api/v1/health)"
    expect "a wrong manifest pin: guided runs still served (POST /api/v1/runs)" 202 "$(code -H "Cookie: $(session /tmp/d.h)" -X POST -H "Origin: https://$H" -H 'Content-Type: application/json' --data '{"scenario":"retry"}' http://localhost/api/v1/runs)"
    stop_server || true
  else
    fail "a wrong manifest pin stopped the portal: $(tail -3 /tmp/t.log)"
  fi
fi

section "summary"
echo "passed: $PASSED; failed: ${#FAILURES[@]}"
for f in "${FAILURES[@]}"; do echo "  FAILED: $f"; done
[ ${#FAILURES[@]} -eq 0 ]
