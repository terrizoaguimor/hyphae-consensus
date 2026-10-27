#!/bin/sh
# Second demo memory (sequential: the adapter allows one active tool call).
SOCK=/hyphae-home/.local/share/hyphae/agent-memory/hyphae.sock
export HYPHAE_NATIVE_API_KEY_FILE=/hyphae-home/.config/hyphae/credentials/memory-writer.key
{
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"seed","version":"0"}}}'
sleep 1
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
sleep 1
printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hyphae_memory_store","arguments":{"project":"demo-repo","kind":"fact","text":"Fact: payment client timeouts correlate with retry bursts above 5 attempts."}}}'
sleep 4
} | timeout 25 hyphae mcp --profile memory --allow-write --endpoint "$SOCK"
echo "SEED2_EXIT=$?"
