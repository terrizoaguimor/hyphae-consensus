#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Permanently forgets one memory (operator action, writer credential), via
# the documented hyphae_memory_forget tool. Idempotent.
# Usage: sh forget.sh <project> <memory-id>
. /hyphae-home/hyphae-env.sh
if [ -z "$1" ] || [ -z "$2" ]; then
  echo "usage: forget.sh <project> <memory-id>" >&2
  exit 2
fi
hyphae_call "$HYPHAE_WRITER_KEY" yes "$(printf '{"name":"hyphae_memory_forget","arguments":{"project":"%s","id":"%s"}}' "$1" "$2")"
