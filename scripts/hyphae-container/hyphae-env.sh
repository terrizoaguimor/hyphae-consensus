#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Shared paths and helpers for the container scripts (paths only, never
# key values). Copied to /hyphae-home/hyphae-env.sh and sourced.
#
# HYPHAE_HOME_DIR selects the Agent Memory instance (default /hyphae-home,
# the demo store; /hyphae-home/lab-injection is the isolated hostile-note
# lab). Each instance has its own data directory, credentials, and socket.
#
# The socket MUST live outside the data directory: proof generation bundles
# the whole data directory into the witness and rejects special files, so a
# socket inside it makes every recall(prove=true) fail with the generic
# "corruption" error. The path used here is the one `hyphae agent setup`
# writes into its systemd unit and MCP host config; `agent configure`
# prints it, so it is read from there rather than hard-coded.
HYPHAE_HOME_DIR=${HYPHAE_HOME_DIR:-/hyphae-home}
HYPHAE_DATA=$HYPHAE_HOME_DIR/.local/share/hyphae/agent-memory
HYPHAE_READER_KEY=$HYPHAE_HOME_DIR/.config/hyphae/credentials/memory-reader.key
HYPHAE_WRITER_KEY=$HYPHAE_HOME_DIR/.config/hyphae/credentials/memory-writer.key
HYPHAE_SOCK=$(HOME=$HYPHAE_HOME_DIR hyphae agent configure opencode 2>/dev/null | sed -n 's/.*"endpoint": "\(.*\)".*/\1/p')

# One MCP session per tool call: the adapter allows one active call, and a
# fresh process per call never races a previous response. Prints only the
# tools/call response line.
# Usage: hyphae_call <key-file> <write:yes|no> <tools/call params JSON> [wait-seconds]
hyphae_call() {
  key=$1
  flag=""
  [ "$2" = yes ] && flag="--allow-write"
  {
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"consensus-ops","version":"0"}}}'
    sleep 1
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
    sleep 1
    printf '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":%s}\n' "$3"
    sleep "${4:-4}"
  } | HOME=$HYPHAE_HOME_DIR HYPHAE_NATIVE_API_KEY_FILE=$key timeout 40 hyphae mcp --profile memory $flag --endpoint "$HYPHAE_SOCK" | grep '"id":2'
}

# Stores one memory with the writer credential. Text must not contain
# double quotes or backslashes. Identity is derived from project + layer +
# text, so storing identical text again maps to the same memory id.
# Usage: hyphae_store <project> <kind> <text>
hyphae_store() {
  hyphae_call "$HYPHAE_WRITER_KEY" yes "$(printf '{"name":"hyphae_memory_store","arguments":{"project":"%s","kind":"%s","text":"%s"}}' "$1" "$2" "$3")"
}

# Proven recall needs the WHOLE data directory to fit in one MCP response:
# the proof carries the complete directory witness, and `hyphae mcp` caps a
# response at 4 MiB. Proofs failed once the data directory reached about
# 3.85 MB (the documented ceiling is "about 3.9 MB"); the oversized response
# is refused and reported, again, as "corruption". Keep the store small.
hyphae_store_bytes() {
  du -b -s "$HYPHAE_DATA" | cut -f1
}
