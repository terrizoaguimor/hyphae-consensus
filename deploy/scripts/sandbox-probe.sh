#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Runs one command under the sandbox of consensus-public.service, as the
# service user, in a transient unit (systemd-run) whose properties are the
# installed unit's own [Service] directives, read from the unit file, so the
# probe cannot drift from the unit. Default command: `hyphae proof verify` on
# one capture of the configured sealed snapshot against the pinned anchor,
# the exact invocation crates/consensus-api/src/hyphae.rs makes from inside
# the service. This is how gate G9 shows that the verifier works under
# ProtectSystem=strict, ProtectHome, PrivateTmp, MemoryDenyWriteExecute, the
# system-call filter and the empty capability set BEFORE a live run depends
# on it. Runbook section 8.6.
#
#   sudo deploy/scripts/sandbox-probe.sh [--unit <unit file>] [--env <public.env>] [--capture <id>]
#   sudo deploy/scripts/sandbox-probe.sh -- <command> [args...]
#
# Carried over from the unit: User, Group, UMask, EnvironmentFile and every
# sandbox and resource directive. Not carried: Type, Exec*, Kill*, Timeout*,
# Restart*, *Credential* (the probe needs no secret) and RuntimeDirectory*.
# Exit status: the command's (systemd-run propagates it); 2 when the default
# verifier run exited 0 without a "verified" re-execution report.
set -euo pipefail

UNIT=/etc/systemd/system/consensus-public.service
ENV_FILE=/etc/consensus/public.env
CAPTURE=""
CMD=()
while [ $# -gt 0 ]; do
  case "$1" in
    --unit) UNIT="$2"; shift 2 ;;
    --env) ENV_FILE="$2"; shift 2 ;;
    --capture) CAPTURE="$2"; shift 2 ;;
    --) shift; CMD=("$@"); break ;;
    -h|--help) sed -n '3,22p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo): systemd-run talks to the system manager" >&2; exit 1; }
[ -r "$UNIT" ] || { echo "unit file not found: $UNIT" >&2; exit 1; }
command -v systemd-run >/dev/null || { echo "systemd-run not found" >&2; exit 1; }

env_value() {
  # env_value <VAR>: the last uncommented assignment in the env file, or empty.
  [ -r "$ENV_FILE" ] || return 0
  grep -E "^[[:space:]]*$1=" "$ENV_FILE" | tail -n 1 | cut -d= -f2- | tr -d '"' || true
}

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

default_run=0
if [ ${#CMD[@]} -eq 0 ]; then
  default_run=1
  bin="$(env_value CONSENSUS_HYPHAE_BIN)"
  anchor="$(env_value CONSENSUS_HYPHAE_EXPECTED_ANCHOR)"
  snap="$(env_value CONSENSUS_SNAPSHOT_DIR)"
  if [ -z "$bin" ] || [ -z "$anchor" ] || [ -z "$snap" ]; then
    echo "CONSENSUS_HYPHAE_BIN, CONSENSUS_HYPHAE_EXPECTED_ANCHOR and CONSENSUS_SNAPSHOT_DIR must be set in $ENV_FILE, or pass a command after --" >&2
    exit 64
  fi
  if [ -z "$CAPTURE" ]; then
    first="$(find "$snap/captures" -maxdepth 1 -name '*.proof' 2>/dev/null | LC_ALL=C sort | head -n 1)"
    [ -n "$first" ] || { echo "no *.proof under $snap/captures" >&2; exit 1; }
    CAPTURE="$(basename "$first" .proof)"
  fi
  proof="$snap/captures/$CAPTURE.proof"; witness="$snap/captures/$CAPTURE.witness"
  if [ ! -f "$proof" ] || [ ! -f "$witness" ]; then
    echo "capture $CAPTURE: $proof or $witness is missing" >&2; exit 1
  fi
  CMD=("$bin" proof verify --proof "$proof" --witness "$witness" --anchor "$anchor")
  echo "capture $CAPTURE, anchor ${anchor:0:12}..., verifier $bin"
fi
run_as="$(printf '%s\n' "${props[@]}" | sed -n 's/^User=//p' | head -n 1)"
echo "sandbox: ${#props[@]} directives from $UNIT (Type=exec added); runs as ${run_as:-root}"

out="$(mktemp)"; err="$(mktemp)"
trap 'rm -f "$out" "$err"' EXIT
set +e
systemd-run --quiet --wait --pipe --collect --unit "consensus-sandbox-probe-$$" \
  -p Type=exec "${props[@]}" -- "${CMD[@]}" >"$out" 2>"$err"
rc=$?
set -e
echo "exit status: $rc"
if [ -s "$err" ]; then
  echo "stderr (last 10 lines):"; tail -n 10 "$err" | sed 's/^/   /'
fi
if [ "$default_run" -eq 1 ]; then
  if [ "$rc" -eq 0 ] \
     && grep -q -E '"status"[[:space:]]*:[[:space:]]*"verified"' "$out" \
     && grep -q -E '"semantic_reexecution_performed"[[:space:]]*:[[:space:]]*true' "$out"; then
    echo "report: status verified, semantic re-execution performed"
    echo "sandbox probe: PASS (hyphae proof verify ran inside the unit's sandbox)"
    exit 0
  fi
  echo "report (first 300 bytes of stdout): $(head -c 300 "$out" | tr '\n' ' ')"
  echo "sandbox probe: FAIL. 'Read-only file system' or 'Operation not permitted' in stderr means a sandbox directive stops the verifier"
  echo "(ProtectSystem=strict and ProtectHome=yes make HOME=/var/lib/consensus read-only; SystemCallFilter answers EPERM;"
  echo "an 'Unknown assignment' from systemd-run names a directive this systemd cannot set on a transient unit)."
  echo "Do not loosen the unit without recording the directive and the reason under gate G9."
  if [ "$rc" -ne 0 ]; then exit "$rc"; fi
  exit 2
fi
if [ -s "$out" ]; then echo "stdout (last 20 lines):"; tail -n 20 "$out" | sed 's/^/   /'; fi
exit "$rc"
