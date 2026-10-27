#!/bin/sh
# Seeds richer, memory-checkable records for project demo-repo (operator
# action, writer credential), on top of seed.sh. Each record states a value,
# rule, or fact taken from the synthetic fixture in demo-repo/payment-client
# (see its CONSTRAINTS.md), so a claim about the retry fix can be checked
# against stored records. These are operator-seeded demo memories, not
# production knowledge. Idempotent: identical text maps to the same memory.
. /hyphae-home/hyphae-env.sh
P=demo-repo
hyphae_store $P fact "Fact: the payment-client default RetryPolicy is max_attempts = 1 with jitter_ms = 0, so a single transient upstream timeout surfaces to users as a payment timeout."
hyphae_store $P fact "Fact: send_with_retries in payment-client retries only SendError::Timeout, up to RetryPolicy.max_attempts; upstream errors are returned at once without a retry."
hyphae_store $P fact "Fact: the payment-client test constraint_cap_is_three_attempts fails for any RetryPolicy with max_attempts above 3 or with jitter_ms equal to 0."
hyphae_store $P decision "Decision (2026-09 payments review): fix payment timeouts by setting the payment-client RetryPolicy to max_attempts = 3 with jitter_ms of at least 50; raising the attempt cap above 3 is not allowed."
hyphae_store $P fact "Fact: in the 2026-08 outage, retry bursts from payment clients amplified an upstream slowdown into a full outage."
echo "SEED_DEMO_FACTS_DONE"
