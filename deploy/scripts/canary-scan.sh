#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Gate G4, credentials: proves the Meta key is nowhere it must not be. Two
# halves, because the server cannot reach its own public hostname (runbook
# 2.4: no egress to Cloudflare's HTTP edge):
#
#   server (root):   sudo deploy/scripts/canary-scan.sh [--canary <string>] [--since '-7 days']
#       scans the public service's process environments (/proc/<pid>/environ
#       of the main process and of every process in its cgroup, so a running
#       hyphae verifier is covered), the run store, the ledger tree, the web
#       export, the env files and the journal
#   owner machine:   deploy/scripts/canary-scan.sh --responses-only --hostname <hostname> [--canary <string>]
#       scans a sample of public responses fetched through Cloudflare
#       (staging behind Access: export CF_ACCESS_CLIENT_ID/CF_ACCESS_CLIENT_SECRET)
#
# Both look for the documented key shape LLM_<digits>_<secret> and,
# optionally, a canary string planted in the key file during the staging
# phase (runbook 4.2; the dedicated key is the only real key). They print
# locations and counts only, never a match, and never read
# /etc/consensus/credentials. An unreachable response counts as a failure
# (the scan is incomplete), never as "ok". Runbook section 8.3.
# Exit status 0 = no hit anywhere; 1 = at least one HIT or UNREACHABLE.
set -euo pipefail

PATTERN='LLM_[0-9]+_[A-Za-z0-9_-]+'
CANARY=""; HOSTNAME_=""; SINCE="-7 days"; RESPONSES_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --canary) CANARY="$2"; shift 2 ;;
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --responses-only) RESPONSES_ONLY=1; shift ;;
    --since) SINCE="$2"; shift 2 ;;
    --pattern) PATTERN="$2"; shift 2 ;;
    -h|--help) sed -n '3,23p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
if [ "$RESPONSES_ONLY" -eq 1 ]; then
  [ -n "$HOSTNAME_" ] || { echo "--responses-only needs --hostname <hostname>" >&2; exit 64; }
else
  [ "$(id -u)" -eq 0 ] || {
    echo "run as root (sudo) on the server: the service's /proc/<pid>/environ is root-readable only." >&2
    echo "From the owner machine use: $0 --responses-only --hostname <hostname>" >&2; exit 1; }
fi
if [ -n "$CANARY" ]; then
  escaped="$(printf '%s' "$CANARY" | sed 's/[][\.*^$|?+(){}\\/]/\\&/g')"
  PATTERN="$PATTERN|$escaped"
fi

hits=0
report() {
  # report <location> <count>
  if [ "$2" -gt 0 ]; then echo "HIT  $1 ($2 match(es))"; hits=$((hits + 1)); else echo "ok   $1"; fi
}

scan_server() {
  echo "== process environments (consensus-public.service cgroup)"
  local pid cg procs p cmd n cred
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
  local d
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
}

scan_responses() {
  local access=() tmp p code n
  if [ -n "${CF_ACCESS_CLIENT_ID:-}" ] && [ -n "${CF_ACCESS_CLIENT_SECRET:-}" ]; then
    access=(-H "CF-Access-Client-Id: $CF_ACCESS_CLIENT_ID" -H "CF-Access-Client-Secret: $CF_ACCESS_CLIENT_SECRET")
  fi
  tmp="$(mktemp)"
  echo "== public responses via https://$HOSTNAME_"
  for p in / /api/v1/health /api/v1/capabilities /api/v1/scenarios /api/v1/showcase; do
    code="$(curl -sS -o "$tmp" -w '%{http_code}' "${access[@]}" "https://$HOSTNAME_$p" 2>/dev/null)" || code=ERR
    if [ "$code" = ERR ] || [ "$code" = 000 ]; then
      echo "UNREACHABLE  GET $p (curl failed; run this half from a machine that reaches https://$HOSTNAME_: the server's egress policy does not)"
      hits=$((hits + 1)); continue
    fi
    n="$(grep -c -E "$PATTERN" "$tmp" || true)"
    report "GET $p (HTTP $code)" "$n"
  done
  rm -f "$tmp"
}

if [ "$RESPONSES_ONLY" -eq 0 ]; then scan_server; fi
if [ -n "$HOSTNAME_" ]; then scan_responses; fi

echo
if [ "$hits" -eq 0 ]; then
  echo "G4 canary scan: PASS (no match anywhere)"
  exit 0
fi
echo "G4 canary scan: FAIL ($hits location(s) with matches or unreachable)"
exit 1
