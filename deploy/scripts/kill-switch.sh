#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Live-mode kill switch (ADR-0007; crates/consensus-api/src/live/budget.rs):
# the presence of the kill-switch file trips the USD ledger at the next
# admission; a tripped ledger refuses every reservation and every draw, so
# in-flight runs stop at their next model call. The trip is persistent: it
# survives a restart and a file still present re-trips it. REMOVING THE FILE
# NEVER RE-OPENS LIVE MODE: only an operator reset clears the trip
# (deploy/scripts/ledger.sh reset --by <operator>, which refuses while the
# file is present), after the reason for the stop is written up.
# Runbook sections 4.4 and 9.3.
#
#   sudo deploy/scripts/kill-switch.sh status
#   sudo deploy/scripts/kill-switch.sh on
#   sudo deploy/scripts/kill-switch.sh off [--yes]
#
# The path comes from CONSENSUS_LIVE_KILL_SWITCH_FILE in /etc/consensus/public.env,
# else <CONSENSUS_LIVE_LEDGER_DIR>/kill-switch, else /var/lib/consensus/ledger/public/kill-switch.
set -euo pipefail

ENV_FILE=/etc/consensus/public.env
ACTION="${1:-status}"
if [ $# -gt 0 ]; then shift; fi
ASSUME_YES=0
while [ $# -gt 0 ]; do
  case "$1" in
    --yes) ASSUME_YES=1; shift ;;
    --env) ENV_FILE="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done

env_value() {
  # env_value <VAR>: the last uncommented assignment in the env file, or empty.
  [ -r "$ENV_FILE" ] || return 0
  grep -E "^[[:space:]]*$1=" "$ENV_FILE" | tail -n 1 | cut -d= -f2- | tr -d '"' || true
}
LEDGER_DIR="$(env_value CONSENSUS_LIVE_LEDGER_DIR)"
LEDGER_DIR="${LEDGER_DIR:-/var/lib/consensus/ledger/public}"
FILE="$(env_value CONSENSUS_LIVE_KILL_SWITCH_FILE)"
FILE="${FILE:-$LEDGER_DIR/kill-switch}"

show_status() {
  echo "ledger: $LEDGER_DIR"
  if [ -e "$FILE" ]; then
    echo "kill switch: ON  ($FILE exists; $(stat -c '%U:%G %a, modified %y' "$FILE"))"
  else
    echo "kill switch: off ($FILE is absent)"
  fi
  if [ -r "$LEDGER_DIR/lifetime.json" ]; then
    echo "ledger summary ($LEDGER_DIR/lifetime.json): trip and spend fields"
    tr -d '\n' <"$LEDGER_DIR/lifetime.json" \
      | grep -o -E '"(tripped|trip_code|trip_reason|lifetime_spent_usd)"[[:space:]]*:[[:space:]]*("[^"]*"|[^,}]*)' \
      | sed 's/^/   /' || true
  else
    echo "no ledger summary yet at $LEDGER_DIR/lifetime.json (no live admission has happened on this ledger)"
  fi
}

case "$ACTION" in
  status)
    show_status ;;
  on)
    [ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }
    install -d -o consensus -g consensus -m 0750 "$LEDGER_DIR"
    install -o root -g consensus -m 0640 /dev/null "$FILE"
    echo "created $FILE: the ledger trips at the next admission (code kill_switch); in-flight runs stop at their next call."
    echo "The trip persists until an operator reset (deploy/scripts/ledger.sh reset --by <operator>, after this file is removed): see runbook 9.3 before re-opening live mode."
    show_status ;;
  off)
    [ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }
    if [ ! -e "$FILE" ]; then echo "nothing to do: $FILE is absent"; show_status; exit 0; fi
    if [ "$ASSUME_YES" -ne 1 ]; then
      read -r -p "remove $FILE? The ledger stays tripped until deploy/scripts/ledger.sh reset (runbook 9.3). [y/N] " answer
      if [ "$answer" != "y" ] && [ "$answer" != "Y" ]; then echo "kept the kill switch"; exit 1; fi
    fi
    rm -f "$FILE"
    echo "removed $FILE. If the ledger has tripped, it remains tripped until: sudo deploy/scripts/ledger.sh reset --by <operator> (runbook 9.3)."
    show_status ;;
  *)
    echo "usage: kill-switch.sh status|on|off [--yes] [--env <file>]" >&2; exit 64 ;;
esac
