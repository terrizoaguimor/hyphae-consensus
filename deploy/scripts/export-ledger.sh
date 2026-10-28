#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Exports the USD ledger (and, optionally, the run store) as a checksummed
# tarball for the owner's machine. The ledger directory holds one append-only
# JSONL file per UTC day, lifetime.json and ledger.lock (budget.rs); it holds
# no credential and no witness. The run store export excludes *.witness and
# *.proof as a safeguard: no witness leaves the server (ADR-0008). Nothing is
# deleted or modified. Runbook sections 9.5 and 9.7.
#
#   sudo deploy/scripts/export-ledger.sh [--out-dir /root/consensus-exports] [--include-runs]
#
# Then, from the owner's machine:
#   scp '<server>:/root/consensus-exports/<name>.tar.gz*' .
#   sha256sum -c <name>.tar.gz.sha256
set -euo pipefail

OUT_DIR=/root/consensus-exports
LEDGER_DIR=/var/lib/consensus/ledger
RUNS_DIR=/var/lib/consensus/runs
INCLUDE_RUNS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --ledger-dir) LEDGER_DIR="$2"; shift 2 ;;
    --runs-dir) RUNS_DIR="$2"; shift 2 ;;
    --include-runs) INCLUDE_RUNS=1; shift ;;
    -h|--help) sed -n '3,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }
[ -d "$LEDGER_DIR" ] || { echo "no ledger directory at $LEDGER_DIR" >&2; exit 1; }

install -d -o root -g root -m 0700 "$OUT_DIR"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
name="consensus-ledger-$stamp"
parent="$(dirname "$LEDGER_DIR")"
members=("$(basename "$LEDGER_DIR")")
if [ "$INCLUDE_RUNS" -eq 1 ]; then
  [ -d "$RUNS_DIR" ] || { echo "no run store at $RUNS_DIR" >&2; exit 1; }
  if [ "$(dirname "$RUNS_DIR")" != "$parent" ]; then
    echo "--include-runs expects the runs and ledger directories to share a parent" >&2; exit 1
  fi
  members+=("$(basename "$RUNS_DIR")")
  name="consensus-ledger-runs-$stamp"
fi

# tar reads the append-only files as they are; a record appended during the
# export appears in the next one. Witnesses and proofs are never included.
tar --exclude='*.witness' --exclude='*.proof' -C "$parent" -czf "$OUT_DIR/$name.tar.gz" "${members[@]}"
( cd "$OUT_DIR" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256" )
chmod 0600 "$OUT_DIR/$name.tar.gz" "$OUT_DIR/$name.tar.gz.sha256"
echo "wrote $OUT_DIR/$name.tar.gz"
cat "$OUT_DIR/$name.tar.gz.sha256"
echo "contents (first 50 entries):"
tar -tzf "$OUT_DIR/$name.tar.gz" | head -n 50 | sed 's/^/   /'
echo "from the owner's machine: scp '<server>:$OUT_DIR/$name.tar.gz*' . && sha256sum -c $name.tar.gz.sha256"
