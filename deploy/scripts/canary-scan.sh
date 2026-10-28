#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Gate G4, credentials: proves the Meta key is nowhere it must not be. Scans
# the public service's process environments (/proc/<pid>/environ of the main
# process and of every process in its cgroup, so a running hyphae verifier is
# covered), the run store, the ledger, the web export, the env files, the
# journal and a sample of public responses, for the documented key shape
# LLM_<digits>_<secret> and, optionally, a canary string planted in a staging
# key file. It prints locations and counts only, never a match, and it never
# reads /etc/consensus/credentials. Runbook section 8.3.
#
#   sudo deploy/scripts/canary-scan.sh [--canary <string>] [--hostname <hostname>] [--since '-7 days']
#
# Exit status 0 = no hit anywhere; 1 = at least one hit (listed as HIT).
set -euo pipefail

PATTERN='LLM_[0-9]+_[A-Za-z0-9_-]+'
CANARY=""; HOSTNAME_=""; SINCE="-7 days"
while [ $# -gt 0 ]; do
  case "$1" in
    --canary) CANARY="$2"; shift 2 ;;
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --since) SINCE="$2"; shift 2 ;;
    --pattern) PATTERN="$2"; shift 2 ;;
    -h|--help) sed -n '3,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo): the service's /proc/<pid>/environ is root-readable only" >&2; exit 1; }
if [ -n "$CANARY" ]; then
  escaped="$(printf '%s' "$CANARY" | sed 's/[][\.*^$|?+(){}\\/]/\\&/g')"
  PATTERN="$PATTERN|$escaped"
fi

hits=0
report() {
  # report <location> <count>
  if [ "$2" -gt 0 ]; then echo "HIT  $1 ($2 match(es))"; hits=$((hits + 1)); else echo "ok   $1"; fi
}

echo "== process environments (consensus-public.service cgroup)"
pid="$(systemctl show -p MainPID --value consensus-public.service)"
if [ -z "$pid" ] || [ "$pid" = "0" ]; then
  echo "consensus-public.service is not running; the environment scan is skipped"
else
  cg="$(systemctl show -p ControlGroup --value consensus-public.service)"
  procs="$(cat "/sys/fs/cgroup${cg}/cgroup.procs" 2>/dev/null || echo "$pid")"
  for p in $procs; do
    [ -r "/proc/$p/environ" ] || continue
    cmd="$(tr '\0' ' ' <"/proc/$p/cmdline" | cut -c1-60)"
    n="$(tr '\0' '\n' <"/proc/$p/environ" | grep -c -E "$PATTERN" || true)"
    report "/proc/$p/environ ($cmd)" "$n"
    if tr '\0' '\n' <"/proc/$p/environ" | grep -q -E '^(CONSENSUS_META_API_KEY|MODEL_API_KEY|META_API_KEY)='; then
      echo "HIT  /proc/$p/environ carries a key VARIABLE (CONSENSUS_META_API_KEY, MODEL_API_KEY or META_API_KEY): the key must come from CONSENSUS_META_API_KEY_FILE"
      hits=$((hits + 1))
    fi
  done
  cred="/run/credentials/consensus-public.service"
  if [ -d "$cred" ]; then
    echo "     credentials directory: $(stat -c '%A %U:%G %n' "$cred") (expected: accessible to consensus only)"
  fi
fi

echo "== files"
for d in /var/lib/consensus/runs /var/lib/consensus/ledger /var/lib/consensus/web \
         /etc/consensus/public.env /etc/consensus/operator.env /etc/cloudflared/config.yml; do
  [ -e "$d" ] || continue
  n="$(grep -r -I -l -E "$PATTERN" "$d" 2>/dev/null | wc -l)"
  report "$d" "$n"
done

echo "== journal (since $SINCE)"
n="$(journalctl -u consensus-public.service -u consensus-operator.service -u cloudflared.service \
      --since "$SINCE" --no-pager -o cat 2>/dev/null | grep -c -E "$PATTERN" || true)"
report "journalctl: consensus-public, consensus-operator, cloudflared" "$n"

if [ -n "$HOSTNAME_" ]; then
  echo "== public responses via https://$HOSTNAME_"
  for p in / /api/v1/health /api/v1/capabilities /api/v1/scenarios /api/v1/showcase; do
    n="$(curl -sS "https://$HOSTNAME_$p" 2>/dev/null | grep -c -E "$PATTERN" || true)"
    report "GET $p" "$n"
  done
fi

echo
if [ "$hits" -eq 0 ]; then
  echo "G4 canary scan: PASS (no match anywhere)"
  exit 0
fi
echo "G4 canary scan: FAIL ($hits location(s) with matches)"
exit 1
