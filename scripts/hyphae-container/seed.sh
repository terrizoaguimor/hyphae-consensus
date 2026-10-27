#!/bin/sh
# Seeds the two original demo-repo memories (operator action, writer
# credential). Idempotent: identical text maps to the same memory.
. /hyphae-home/hyphae-env.sh
hyphae_store demo-repo constraint "Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage."
hyphae_store demo-repo fact "Fact: payment client timeouts correlate with retry bursts above 5 attempts."
echo "SEED_DONE"
