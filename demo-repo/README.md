# Synthetic demo repository

`payment-client/` is a tiny standalone Rust crate (not a workspace member)
containing the reproducible bug the consensus demo deliberates:

- Default `RetryPolicy` allows 1 attempt → transient hiccups become
  user-visible payment timeouts.
- The obvious fix (raise retries to 10) violates the documented
  constraint (cap 3 attempts with jitter; see `CONSTRAINTS.md`).

Run its tests with `cargo test` inside `payment-client/`. Hyphae Consensus
never modifies this fixture: through M3, human approval is recorded only
and the executor is deferred.
