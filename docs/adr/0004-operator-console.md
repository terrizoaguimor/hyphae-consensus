# ADR-0004: Operator console

Date: 2026-10-26 · Status: accepted · Scope: console milestone

## Decisions

1. **Axum + SSE in `consensus-api`**, one server-rendered page
   (`src/console.html`, vanilla JS, no build step, no frontend framework).
   PliegoRS/PliegoCSS were not verifiably available in this environment;
   per the brief's fallback, plain HTML/CSS was used.

2. **Explicit modes, honestly labeled.** `model=fake` runs a scripted
   `scenario` (`retry` reference, `offline`, `invalid-proof`, `injection`,
   `abstain`: the demo's failure gallery) and requires `memory=fake`; any
   other memory with `model=fake` is rejected with 400 instead of being
   ignored (amended 2026-10-26: it used to be silently ignored).
   `model=meta` calls the live Meta Model API and hard-fails with guidance
   when no credential is configured. `memory=fake` uses seeded records with
   **simulated** proofs: every verification record says `fake-verifier` and
   `SIMULATED proof`, so a receipt cannot pass them off as cryptography.
   `memory=offline` demonstrates documented degradation; `memory=hyphae`
   uses the real MCP adapter.

3. **One owning event log** behind a mutex; read paths parse file
   snapshots tolerating only a torn final line; any other unreadable line
   is reported (HTTP 500 / an SSE `replay_error` event) instead of being
   skipped, and a run whose history fails replay is shown as failed with
   the replay error. Server runs use `UuidIdGen` because the log is
   multi-run (the deterministic `SequentialIdGen` remains for the
   single-run CLI demo and tests). On startup the server closes runs left
   mid-deliberation by a dead process as `Failed{stage:"interrupted"}`.

4. **SSE is replay**: `GET /api/runs/{id}/events` streams the persisted
   event sequence (`replay_complete` terminates it). It performs no model
   calls and no external effects, consistent with the event-log contract.

5. **Approval remains record-only, and is gated.** The decision endpoint
   appends `HumanDecisionRecorded`; there is no executor in the server.
   Since 2026-10-26 it requires the random per-process operator token
   printed at startup (401 otherwise): a typed operator name is a label,
   not authentication. Approving a `blocked` or `abstained` run is an
   override and requires a note (400 otherwise). Idempotency keys are
   scoped per run, and the UI sends a stable key per run. The other
   endpoints remain unauthenticated and the server binds to loopback; it
   must not be exposed beyond the local machine.
