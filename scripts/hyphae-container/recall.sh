#!/bin/sh
# Recall via the reader credential (validates the adapter shape).
# Usage: sh recall.sh [project] [query] [prove:true|false]
# Prints only the tools/call response line.
. /hyphae-home/hyphae-env.sh
hyphae_call "$HYPHAE_READER_KEY" no "$(printf '{"name":"hyphae_memory_recall","arguments":{"project":"%s","query":"%s","limit":5,"prove":%s}}' "${1:-demo-repo}" "${2:-retries attempts}" "${3:-true}")" 8
