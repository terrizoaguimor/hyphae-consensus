#!/bin/sh
# Seeds the two demo-repo memories via the writer credential (operator action).
export HYPHAE_NATIVE_API_KEY_FILE=/hyphae-home/.config/hyphae/credentials/memory-writer.key
SOCK=/hyphae-home/.local/share/hyphae/agent-memory/hyphae.sock
{
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"seed","version":"0"}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hyphae_memory_store","arguments":{"project":"demo-repo","kind":"constraint","text":"Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage."}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"hyphae_memory_store","arguments":{"project":"demo-repo","kind":"fact","text":"Fact: payment client timeouts correlate with retry bursts above 5 attempts."}}}'
sleep 3
} | timeout 25 hyphae mcp --profile memory --allow-write --endpoint "$SOCK"
echo "SEED_EXIT=$?"
