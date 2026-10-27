# Recorded constraints (mirror of the Hyphae memory seed)

These are the records the demo seeds into memory (fake mode) or expects in
the isolated Hyphae profile (live mode), project `demo-repo`:

1. **constraint** — "Constraint: outbound retries must be capped at 3
   attempts with jitter; unbounded retries caused the 2026-08 outage."
2. **fact** — "Fact: payment client timeouts correlate with retry bursts
   above 5 attempts."

The reported bug: payment requests intermittently time out under load
because the default `RetryPolicy` allows a single attempt. The naive fix
(raise the limit to 10) violates constraint 1 and is contradicted by
fact 2. The compliant fix is 3 attempts with jitter.
