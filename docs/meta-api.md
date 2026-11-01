# How Hyphae Consensus uses Meta's API

The [README](../README.md) has a summary of this page.

- **Model.** `muse-wonder-1.0`, the default model id (`DEFAULT_META_MODEL` in
  `crates/consensus-api/src/config.rs`; `CONSENSUS_MODEL` overrides it). Every recorded
  live run in `research/` used it, and every completed Meta call there is recorded with
  `provider: meta`, the model id and the API's request id (scripted calls record
  `provider: fake`; `research/README.md`).
- **Endpoint.** The Meta Model API's Chat Completions, `POST {base}/chat/completions` with
  base `https://api.meta.ai/v1` (`crates/consensus-api/src/meta.rs`, ADR-0002).
- **Which roles call it.** All three roles, in four kinds of call with one strict schema
  each (`Role` in `meta.rs`): Proposer (`DraftProposal`), Critic (`DraftChallenge`), Evidence
  Evaluator (`DraftEvaluation`) and the Proposer's revision (`DraftRevision`). The
  scripted doubles (`provider: fake`) replace all four in the guided runs and the tests.
- **Structured output.** `response_format: {type: "json_schema", json_schema: {name,
  schema, strict: true}}`. Every list field is required (a missing one is malformed
  output, since `3d50baf`). The Evaluator's schema is built per call and keyed by id:
  `assessments` has exactly one required property per claim id of the proposal and
  `item_verdicts` one per open objection, no other properties allowed, so an omitted,
  repeated or unknown id is `MalformedOutput`, never a silent default (tag
  `prompt-freeze-2`; `docs/evaluation.md` §6.2). Responses are parsed into typed drafts
  and cross-checked; ids, timestamps and schema versions are assigned by the runtime.
  Each invocation records the SHA-256 of the schema it sent and of its role's prompt
  template, `finish_reason`, the attempt count and the reasoning tokens.
- **Request bounds and retries** (`meta.rs`, `config.rs`). `max_completion_tokens`
  16,000 by default; one 120 s time budget per call (`CONSENSUS_MODEL_TIMEOUT_SECS`, a
  whole number of seconds from 1 to 3600: any other value, `180s` say, stops the server
  at boot naming the variable; the public server warns at boot when the budget is below
  `max_completion_tokens / 80` seconds) shared by up to 3 attempts. 429 and 503 are
  retried up to that budget, after `Retry-After` when it fits or after a full-jitter
  backoff from 500 ms; a connection that failed before the request was sent may be retried
  too. HTTP 500 and 502 are retried **once** per call (since 2026-11-01, after live run
  M02's HTTP 500 at critique), under the same `Retry-After` and backoff rules, and only
  when 30 s of the call budget are still left after the wait (a quarter of a budget
  shorter than 120 s): a 500 late in the budget fails at once, since its retry could not
  finish a generation. A connection that broke after the request was sent is retried
  exactly once too. Both failed attempts may have been billed, so each of these retries
  can bill one duplicate generation (at most two per call, one per retry class) that no
  invocation records: the failed attempt is charged at its full
  estimate on the live ledger, and the answering call records `attempts: 2` (an invocation
  can now show `attempts: 2` after a 5xx). Timeouts, 504 (the gateway gave up on a
  generation that may have completed upstream) and every other 4xx are never retried.
  Pooled connections idle for more than 5 s are dropped, so a call never reuses a
  connection a middlebox has silently closed (smoke run K02). `finish_reason: length` and
  `refusal` are recorded as failures with stable codes; no response body or URL reaches an
  error, event or receipt.
- **Run deadline and shutdown** (`meta.rs`, `runs.rs`). The server's run manager binds each
  run's model client to the run's control: a call's budget is cut to the run's deadline
  (`CONSENSUS_RUN_DEADLINE_SECS`), a call with less than 30 s left before it is not made
  (code **`run_deadline_reached`**, nothing drawn on the ledger or sent; a call the deadline
  cuts short fails with the same code and is charged like a timeout), and at shutdown a
  call still in progress when the grace runs out (the public server's grace is one call
  budget plus 10 s, at least 60 s: 250 s at a 240 s budget) is abandoned, charged at its
  estimate and fails its run with code `shutdown`, so the run's reservation is released
  before the process exits. This only works when the service's stop timeout
  (`TimeoutStopSec`) is longer than the grace plus the 10 s the aborted runs get to settle
  (`ABORT_SETTLE_WAIT`) plus the HTTP drain: 280 s at a 240 s budget. With a shorter one,
  systemd kills the process first and the reservation stays open until the next start's
  recovery, so the timeout and the unit's stop timeout are deployed together. The public
  server prints its effective limits at boot (one `[startup] limits:` line on stderr:
  per-call and connect timeouts, attempts, output bound, `reasoning_effort` set or unset,
  run deadline, run token cap, shutdown grace and the stop timeout it needs, live
  concurrency, burst fraction, daily quotas; no key).
- **Prompts.** Everything a role did not write itself sits in separate `<UNTRUSTED_*>`
  blocks; a mitigation, not a boundary (details in [verification.md](verification.md#safety-invariants-enforced-in-code-covered-by-tests)).
- **The public live mode (on the owner's key; not deployed).** Decided in ADR-0007,
  operated per `docs/deployment/runbook.md`:
  - **No hosted BYOK.** The portal never accepts a visitor's Meta key (Meta's Model API
    terms make keys confidential and forbid gateway access; ADR-0007). Visitors who want
    their own key self-host this repository.
  - **Presets only, no free text.** A live run takes one of five preset ids whose task
    texts are the recorded battery tasks, verbatim, against project `demo-repo`
    (`R01-reference`, `R03-raise-to-10`, `R05-add-jitter`, `R06-timeout-30s`,
    `R07-exp-backoff`; `crates/consensus-api/src/live/presets.rs`). Nothing a visitor
    types reaches a model.
  - **Evidence** comes from a sealed Hyphae snapshot whose files are re-checked at every
    admission (ADR-0008, `docs/architecture/sealed-snapshot.md`). The snapshot
    `snap-2026-10-29-demo-repo` was captured on 2026-10-29 (`b0375df`; its public files
    are in `snapshots/`, its digests in the README, "Published snapshot digests"); the
    runtime loads it since the boot fix `0515f29` (merged as `7e26866`), and it is not on
    any server yet (see the status in [roadmap.md](roadmap.md#status-on-2026-10-31)).
  - **Two-phase gate** (`live/gate.rs`): preset, geography, an 18+ attestation, the
    evidence status and Turnstile before the single live slot is taken; then the client's
    daily quota (default 3 public-lane runs per IPv4 /32 or IPv6 /64 per UTC day) and a
    ledger reservation of the run's worst case, before any model call.
  - **USD ledger** (`live/budget.rs`, `live/config.rs`): append-only, fsynced, shared by
    every process that holds the key. Caps by owner decision: $10 per UTC day, $60
    lifetime, of which $15 is the slice of the judge lane (runs admitted with an invite
    code, with their own daily limit). A run's worst case is (70,000-token
    run cap + 16,000 output + 32,000 input cap) × $20/M = $2.36. Every HTTP attempt is
    drawn on the reservation and settled once; timeouts, 5xx, post-send transport errors
    and responses without usage are charged at the attempt's worst case and never
    refunded. The ledger trips on an overrun, on 401/402/403 from Meta or at the lifetime
    cap, and the kill switch is the same state. The operator console's `model=meta` runs
    are metered on the same ledger when `CONSENSUS_LIVE_LEDGER_DIR` is set. From a shell
    on the server the operator reads it, clears a trip and releases a crash-stranded
    reservation with `consensus-api ledger status | reset | release` (reconciliation with
    the provider dashboard: runbook §9.3).
  - Each public live request carries the session's pseudonym (`visitor:<16 hex>`) as
    `safety_identifier`. Measured cost on the final code (ADR-0007): $0.81 mean, $0.98
    maximum per run.
