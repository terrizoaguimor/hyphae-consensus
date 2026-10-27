# Recorded constraints (mirror of the Hyphae memory seed)

These are the records the demo seeds into memory, project `demo-repo`. They
are synthetic, operator-seeded demo memories that describe this fixture; they
are not production knowledge.

Fake mode (`memory=fake`, scripted scenarios) uses records 1–2. The live
Hyphae store is seeded with all seven (`scripts/hyphae-container/seed.sh`
and `seed-demo-facts.sh`), so each claim about the retry fix can be checked
against a stored record:

1. **constraint** — "Constraint: outbound retries must be capped at 3
   attempts with jitter; unbounded retries caused the 2026-08 outage."
2. **fact** — "Fact: payment client timeouts correlate with retry bursts
   above 5 attempts."
3. **fact** — "Fact: the payment-client default RetryPolicy is max_attempts =
   1 with jitter_ms = 0, so a single transient upstream timeout surfaces to
   users as a payment timeout."
4. **fact** — "Fact: send_with_retries in payment-client retries only
   SendError::Timeout, up to RetryPolicy.max_attempts; upstream errors are
   returned at once without a retry."
5. **fact** — "Fact: the payment-client test constraint_cap_is_three_attempts
   fails for any RetryPolicy with max_attempts above 3 or with jitter_ms
   equal to 0."
6. **decision** — "Decision (2026-09 payments review): fix payment timeouts
   by setting the payment-client RetryPolicy to max_attempts = 3 with
   jitter_ms of at least 50; raising the attempt cap above 3 is not allowed."
7. **fact** — "Fact: in the 2026-08 outage, retry bursts from payment clients
   amplified an upstream slowdown into a full outage."

Correction (2026-10-27 audit, DOC-8): record 5 overstates the test. The
test `constraint_cap_is_three_attempts` asserts only on its own literal
`RetryPolicy { max_attempts: 3, jitter_ms: 50 }`; it never checks
`RetryPolicy::default()` or a configured policy, so raising the default to
10 attempts would not make it fail. The record is kept as seeded, because
the recorded runs in `research/` cite it by id (live run R06 among them).

The reported bug: payment requests intermittently time out under load
because the default `RetryPolicy` allows a single attempt. The naive fix
(raise the limit to 10) violates constraint 1, record 5's description of the test (see the correction above), and decision
6, and fact 2 contradicts it. The compliant fix is 3 attempts with jitter.
