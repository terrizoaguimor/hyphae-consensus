#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# The restart gate (runbook 9.9): restarts consensus-public only once the
# server reports no run in progress and none queued, then checks that the
# restart left no live reservation open. Never restart the public server any
# other way (a restart during a model call costs a settled attempt, and a
# stop that systemd has to kill strands the run's reservation at its worst
# case until an operator releases it).
#
#   sudo deploy/scripts/restart-when-idle.sh
#   sudo deploy/scripts/restart-when-idle.sh --host <hostname> --interval 15 --max-wait 3600
#
# 1. Polls GET /api/v1/ready on the unix socket, as cloudflared would send it
#    (Host, CF-Connecting-IP, CF-IPCountry), every --interval seconds, until
#    it answers "active_runs":0 and "queued_runs":0. A /ready that cannot be
#    read, or that names neither figure, counts as busy. After --max-wait
#    seconds without an idle answer it gives up WITHOUT restarting (exit 2).
# 2. systemctl restart <unit> (the unit's TimeoutStopSec=280 covers the
#    250 s shutdown grace at a 240 s call budget: a run admitted between the
#    idle answer and the stop signal is still settled and released).
# 3. Waits up to --ready-wait seconds for "ready":true again (exit 4 when it
#    does not come), prints the boot's `[startup] limits:` line when
#    journalctl has it, then runs `ledger.sh status` and prints every open
#    (unreleased) reservation: exit 3 when there is one (each counts at the
#    run's worst case until `ledger.sh release <id> --by <operator>`, runbook
#    9.3), 0 when there is none.
#
# Nothing it prints holds a secret: /ready and the ledger status carry
# figures, codes and reservation ids only.
#
# --host defaults to the first entry of CONSENSUS_ALLOWED_HOSTS in the env
# file (--env, /etc/consensus/public.env). HC_LEDGER_SH overrides the ledger
# command (deploy/scripts/ledger.sh next to this script), for the self-test.
#
# Exit status: 0 restarted and no reservation open; 1 setup error; 2 never
# idle within --max-wait (not restarted); 3 restarted, a reservation is open;
# 4 restarted, /ready never became ready; 64 usage.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
ENV_FILE=/etc/consensus/public.env
SOCKET=/run/consensus/public.sock
UNIT=consensus-public
HOST=""
INTERVAL=15
MAX_WAIT=3600
READY_WAIT=120

while [ $# -gt 0 ]; do
  case "$1" in
    --host) HOST="$2"; shift 2 ;;
    --env) ENV_FILE="$2"; shift 2 ;;
    --socket) SOCKET="$2"; shift 2 ;;
    --unit) UNIT="$2"; shift 2 ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    --max-wait) MAX_WAIT="$2"; shift 2 ;;
    --ready-wait) READY_WAIT="$2"; shift 2 ;;
    *) echo "usage: restart-when-idle.sh [--host <hostname>] [--env <file>] [--socket <path>] [--unit <name>] [--interval <s>] [--max-wait <s>] [--ready-wait <s>]" >&2; exit 64 ;;
  esac
done
for n in "$INTERVAL" "$MAX_WAIT" "$READY_WAIT"; do
  case "$n" in ''|*[!0-9]*) echo "--interval, --max-wait and --ready-wait take whole seconds" >&2; exit 64 ;; esac
done

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo): the socket and systemctl need it" >&2; exit 1; }
command -v curl >/dev/null || { echo "curl not found" >&2; exit 1; }
command -v systemctl >/dev/null || { echo "systemctl not found" >&2; exit 1; }

if [ -z "$HOST" ] && [ -r "$ENV_FILE" ]; then
  HOST="$(grep -E '^[[:space:]]*CONSENSUS_ALLOWED_HOSTS=' "$ENV_FILE" | tail -n 1 | cut -d= -f2- | tr -d '"' | cut -d, -f1 | tr -d '[:space:]' || true)"
fi
[ -n "$HOST" ] || { echo "no host: pass --host or set CONSENSUS_ALLOWED_HOSTS in $ENV_FILE" >&2; exit 1; }
LEDGER_SH="${HC_LEDGER_SH:-$here/ledger.sh}"

ready_body() {
  # The /ready body, without a newline, or empty when it cannot be read.
  curl -sS --max-time 10 --unix-socket "$SOCKET" \
    -H "Host: $HOST" -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' \
    http://localhost/api/v1/ready 2>/dev/null | tr -d '\r\n' || true
}

figure() { # figure <name> <body>: the integer value of "<name>":N, or empty
  printf '%s' "$2" | tr -d ' ' | grep -o "\"$1\":[0-9]*" | head -n 1 | cut -d: -f2 || true
}

started=$(date +%s)
while :; do
  body="$(ready_body)"
  active="$(figure active_runs "$body")"
  queued="$(figure queued_runs "$body")"
  if [ "$active" = 0 ] && [ "$queued" = 0 ]; then
    echo "[restart-when-idle] idle: active_runs 0, queued_runs 0"
    break
  fi
  if [ -z "$active" ] || [ -z "$queued" ]; then
    echo "[restart-when-idle] /ready unreadable or incomplete; waiting"
  else
    echo "[restart-when-idle] busy: active_runs $active, queued_runs $queued; waiting"
  fi
  if [ $(( $(date +%s) - started )) -ge "$MAX_WAIT" ]; then
    echo "[restart-when-idle] not idle within $MAX_WAIT s: NOT restarted" >&2
    exit 2
  fi
  sleep "$INTERVAL"
done

echo "[restart-when-idle] systemctl restart $UNIT"
systemctl restart "$UNIT"

waited=0
until printf '%s' "$(ready_body)" | tr -d ' ' | grep -q '"ready":true'; do
  if [ "$waited" -ge "$READY_WAIT" ]; then
    echo "[restart-when-idle] /ready not ready $READY_WAIT s after the restart" >&2
    exit 4
  fi
  sleep 1
  waited=$((waited + 1))
done
echo "[restart-when-idle] ready again"
if command -v journalctl >/dev/null; then
  journalctl -u "$UNIT" -n 200 --no-pager 2>/dev/null | grep -F '[startup] limits:' | tail -n 1 || true
fi

status_json="$("$LEDGER_SH" status)" || { echo "[restart-when-idle] ledger.sh status failed" >&2; exit 1; }
open="$(printf '%s' "$status_json" | tr -d ' \r\n' | grep -o '"open_reservations":\[[^]]*\]' | head -n 1 || true)"
if [ "$open" = '"open_reservations":[]' ]; then
  echo "[restart-when-idle] ledger: no open reservation"
  exit 0
fi
echo "[restart-when-idle] ledger: OPEN RESERVATION(S) after the restart (each counts at its worst case until released, runbook 9.3):" >&2
printf '%s\n' "${open:-"(open_reservations not found in the ledger status)"}" >&2
exit 3
