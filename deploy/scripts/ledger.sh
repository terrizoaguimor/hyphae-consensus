#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# The operator's ledger subcommand (crates/consensus-api/src/ledger_cli.rs),
# run where the server runs: as the service user, in a transient unit
# (systemd-run) whose properties are the installed unit's own [Service]
# directives (User, Group, EnvironmentFile, sandbox, ReadWritePaths), so it
# opens the ledger directory of public.env with the server's caps, the way
# consensus-public does. No credential is loaded: the subcommand reads
# none. Runbook sections 4.4, 4.5, 8.8 and 9.3.
#
#   sudo deploy/scripts/ledger.sh status
#   sudo deploy/scripts/ledger.sh reset --by <operator>
#   sudo deploy/scripts/ledger.sh release <reservation-id> --by <operator>
#   sudo deploy/scripts/ledger.sh --unit <unit file> status
#
# Output is the subcommand's JSON on stdout (errors on stderr); the exit
# status is the subcommand's, propagated by systemd-run: 0 done, 1 usage,
# 2 refused or failed. Carried over from the unit and not carried: as
# sandbox-probe.sh (no Type, Exec*, Kill*, Timeout*, Restart*, *Credential*,
# RuntimeDirectory*).
set -euo pipefail

UNIT=/etc/systemd/system/consensus-public.service
if [ "${1:-}" = "--unit" ]; then
  [ $# -ge 2 ] || { echo "--unit needs a unit file" >&2; exit 64; }
  UNIT="$2"; shift 2
fi
[ $# -gt 0 ] || { echo "usage: ledger.sh [--unit <unit file>] status | reset --by <operator> | release <reservation-id> --by <operator>" >&2; exit 64; }
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo): systemd-run talks to the system manager" >&2; exit 1; }
[ -r "$UNIT" ] || { echo "unit file not found: $UNIT" >&2; exit 1; }
command -v systemd-run >/dev/null || { echo "systemd-run not found" >&2; exit 1; }

# The binary the unit starts (first word of ExecStart).
BIN="$(sed -n 's/^[[:space:]]*ExecStart=\([^[:space:]]*\).*/\1/p' "$UNIT" | head -n 1)"
[ -n "$BIN" ] || { echo "no ExecStart in $UNIT" >&2; exit 1; }

# The [Service] section of the unit, one -p per directive.
props=()
while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"
  case "$line" in ''|'#'*|'['*) continue ;; esac
  key="${line%%=*}"
  case "$key" in
    Type|Exec*|Kill*|Timeout*|Restart*|*Credential*|RuntimeDirectory*) continue ;;
  esac
  props+=(-p "$line")
done < <(sed -n '/^\[Service\]/,/^\[/p' "$UNIT")
[ ${#props[@]} -gt 0 ] || { echo "no [Service] directives found in $UNIT" >&2; exit 1; }

exec systemd-run --quiet --wait --pipe --collect --unit "consensus-ledger-cli-$$" \
  -p Type=exec "${props[@]}" -- "$BIN" ledger "$@"
