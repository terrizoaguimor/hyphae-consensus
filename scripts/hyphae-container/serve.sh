#!/bin/sh
# Starts an Agent Memory daemon the way the systemd unit written by
# `hyphae agent setup` does: `agent recover`, then `serve` with the socket
# OUTSIDE the data directory (see hyphae-env.sh for why).
# Usage: docker exec [-e HYPHAE_HOME_DIR=...] hyphae-consensus sh /hyphae-home/serve.sh
. /hyphae-home/hyphae-env.sh
export HOME=$HYPHAE_HOME_DIR
[ -n "$HYPHAE_SOCK" ] || { echo "no canonical endpoint (run hyphae agent setup first)" >&2; exit 1; }
hyphae agent recover || exit 1
echo "--- serve start $(date -u +%Y-%m-%dT%H:%M:%SZ) endpoint $HYPHAE_SOCK" >> "$HYPHAE_HOME_DIR/serve.log"
nohup hyphae serve --data-dir "$HYPHAE_DATA" --endpoint "$HYPHAE_SOCK" --native-api-key-auth >> "$HYPHAE_HOME_DIR/serve.log" 2>&1 &
echo "SERVE_STARTED pid=$! endpoint=$HYPHAE_SOCK"
