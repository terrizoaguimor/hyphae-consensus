#!/bin/sh
# Recall with proof, via the reader credential (validates the adapter shape).
SOCK=/hyphae-home/.local/share/hyphae/agent-memory/hyphae.sock
export HYPHAE_NATIVE_API_KEY_FILE=/hyphae-home/.config/hyphae/credentials/memory-reader.key
{
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"recall-probe","version":"0"}}}'
sleep 1
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
sleep 1
printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hyphae_memory_recall","arguments":{"project":"demo-repo","query":"retries attempts","limit":5,"prove":true}}}'
sleep 5
} | timeout 30 hyphae mcp --profile memory --endpoint "$SOCK"
echo "RECALL_EXIT=$?"
