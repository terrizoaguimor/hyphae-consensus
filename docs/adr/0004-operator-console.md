# ADR-0004: Operator console

Date: 2026-10-26 · Status: accepted · Scope: console milestone

## Decisions

1. **Axum + SSE in `consensus-api`**, one server-rendered page
   (`src/console.html`, vanilla JS, no build step, no frontend framework).
   PliegoRS/PliegoCSS were not verifiably available in this environment;
   per the brief's fallback, plain HTML/CSS was used.

2. **Explicit modes, honestly labeled.** `model=fake` runs the scripted
   reference scenario; `model=meta` calls the live Meta Model API and hard-
   fails with guidance when no credential is configured. `memory=fake` uses
   seeded records; `memory=offline` demonstrates documented degradation;
   `memory=hyphae` uses the real MCP adapter. Nothing silently substitutes
   one mode for another.

3. **One owning event log** behind a mutex; read paths parse file
   snapshots tolerating a partial tail. Server runs use `UuidIdGen`
   because the log is multi-run (the deterministic `SequentialIdGen`
   remains for the single-run CLI demo and tests).

4. **SSE is replay**: `GET /api/runs/{id}/events` streams the persisted
   event sequence (`replay_complete` terminates it). It performs no model
   calls and no external effects, consistent with the event-log contract.

5. **Approval remains record-only.** The decision endpoint appends
   `HumanDecisionRecorded`; there is no executor in the server.
