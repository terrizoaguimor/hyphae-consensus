#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Exports a USD ledger directory (and, optionally, the run store) as a
# checksummed tarball for the owner's machine. A ledger directory holds one
# append-only JSONL file per UTC day, lifetime.json, ledger.lock and perhaps
# the kill-switch file (budget.rs); it holds no credential and no witness.
# The run store export excludes *.witness and *.proof as a safeguard: no
# witness leaves the server (ADR-0008). It also excludes the session table
# (sessions.json and its .sessions.json.<pid>.tmp rewrite, session.rs): the
# Privacy page says a session record stays only on the server and only until
# the session ends, so no export carries one. Nothing is deleted or modified.
# Runbook sections 4.5, 9.5 and 9.7.
#
#   sudo deploy/scripts/export-ledger.sh [--ledger-dir <dir>] [--out-dir /root/consensus-exports] [--include-runs]
#
# --ledger-dir defaults to CONSENSUS_LIVE_LEDGER_DIR from /etc/consensus/public.env
# (the active ledger). Pass /var/lib/consensus/ledger to export every ledger
# (staging and public) at once, or /var/lib/consensus/ledger/staging before
# the staging ledger is archived.
# Then, from the owner's machine:
#   scp '<server>:/root/consensus-exports/<name>.tar.gz*' .
#   sha256sum -c <name>.tar.gz.sha256
set -euo pipefail

ENV_FILE=/etc/consensus/public.env
OUT_DIR=/root/consensus-exports
LEDGER_DIR=""
RUNS_DIR=/var/lib/consensus/runs
INCLUDE_RUNS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --ledger-dir) LEDGER_DIR="$2"; shift 2 ;;
    --runs-dir) RUNS_DIR="$2"; shift 2 ;;
    --env) ENV_FILE="$2"; shift 2 ;;
    --include-runs) INCLUDE_RUNS=1; shift ;;
    -h|--help) sed -n '3,23p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }
if [ -z "$LEDGER_DIR" ] && [ -r "$ENV_FILE" ]; then
  LEDGER_DIR="$(grep -E '^[[:space:]]*CONSENSUS_LIVE_LEDGER_DIR=' "$ENV_FILE" | tail -n 1 | cut -d= -f2- | tr -d '"' || true)"
fi
LEDGER_DIR="${LEDGER_DIR:-/var/lib/consensus/ledger}"
[ -d "$LEDGER_DIR" ] || { echo "no ledger directory at $LEDGER_DIR" >&2; exit 1; }

install -d -o root -g root -m 0700 "$OUT_DIR"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
name="consensus-ledger-$(basename "$LEDGER_DIR")-$stamp"
# tar accepts several -C: each changes the directory for the members that
# follow, so the ledger and the runs need not share a parent.
members=(-C "$(dirname "$LEDGER_DIR")" "$(basename "$LEDGER_DIR")")
if [ "$INCLUDE_RUNS" -eq 1 ]; then
  [ -d "$RUNS_DIR" ] || { echo "no run store at $RUNS_DIR" >&2; exit 1; }
  members+=(-C "$(dirname "$RUNS_DIR")" "$(basename "$RUNS_DIR")")
  name="consensus-ledger-$(basename "$LEDGER_DIR")-runs-$stamp"
fi

# tar reads the append-only files as they are; a record appended during the
# export appears in the next one. Witnesses, proofs and the session table are
# never included.
tar --exclude='*.witness' --exclude='*.proof' \
  --exclude='sessions.json' --exclude='.sessions.json.*.tmp' -czf "$OUT_DIR/$name.tar.gz" "${members[@]}"
( cd "$OUT_DIR" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256" )
chmod 0600 "$OUT_DIR/$name.tar.gz" "$OUT_DIR/$name.tar.gz.sha256"
echo "wrote $OUT_DIR/$name.tar.gz (ledger: $LEDGER_DIR)"
cat "$OUT_DIR/$name.tar.gz.sha256"
echo "contents (first 50 entries):"
tar -tzf "$OUT_DIR/$name.tar.gz" | head -n 50 | sed 's/^/   /'
echo "from the owner's machine: scp '<server>:$OUT_DIR/$name.tar.gz*' . && sha256sum -c $name.tar.gz.sha256"
