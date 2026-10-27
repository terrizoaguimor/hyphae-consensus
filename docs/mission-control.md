# Mission Control frontend — implementation notes

Date: 2026-10-27  
Branch: `feat/mission-control`  
Status: initial operator UI delivered; live streaming and true Hyphae proof integration remain future work.

## Scope implemented

- Replaces the legacy single-page console with a high-density, responsive dark operator dashboard.
- Preserves the existing Axum API routes and payload contracts; no backend or Hyphae Core modifications.
- Shows all three logical roles, persisted event timeline, orthogonal evidence statuses, recommendation, human authorization, model telemetry and receipts.
- Exposes explicit `fake`/`meta` model and `fake`/`offline`/`hyphae` memory modes.
- Renders remote model and evidence content through `textContent` / DOM node creation instead of HTML interpolation.
- Labels SSE replay as persisted replay; does not claim live streaming.
- Preserves record-only approval; no executor is introduced.

## Verification

- JavaScript extracted from `console.html` passes Node.js `vm.Script` syntax parsing.
- UI has **not** yet been exercised against a running Rust server in the contributor's Windows environment.
- No live API, Hyphae or Meta calls were made for this UI change.

## Existing backend constraints

1. `POST /api/runs` blocks until the orchestration finishes and returns the completed run; it does not issue an early run ID.
2. `GET /api/runs/{id}/events` replays persisted events and ends with `replay_complete`. It is not a real-time subscription.
3. The event-log mutex is currently held by `execute_run` during the full orchestration; concurrent human/stream operations are constrained.
4. `memory=hyphae` remains blocked by the upstream Windows 4.0.0 Agent Memory initialization failure documented in `docs/journal.md`.
5. Authentication is not implemented. The operator API binds to loopback by default and **must not be exposed publicly** until authorization and CSRF protections exist.

## Next: actual streaming contract

Before claiming live-agent visualization, implement:

- `POST /api/runs` returns `202 Accepted` with `run_id` immediately.
- Spawn a cancellable orchestration task with a bounded per-run lifecycle; release any process-wide event-log mutex between model/memory calls.
- Persist each event before publication and emit it to subscribers; include monotonically increasing per-run sequence numbers.
- SSE support for replay-then-tail, `Last-Event-ID` resume, heartbeat, terminal events, backpressure and gap recovery.
- Preserve a compatibility endpoint or version the API so the UI can migrate cleanly.
- Tests: concurrent runs, disconnect/reconnect, no duplicate events, truncation/restart recovery, crash before/after persist, approval race, invalid transitions and no external mutation.
- Only after this contract is tested should the frontend label active inference as `LIVE`.

## Presentation plan

Final demo must fit **three minutes**, not the older four-minute storyboard. Show a truthful, reproducible fake-mode capture plus separately labeled real Meta/model integrations. Never represent scripted scenarios as live model output. The live Hyphae integration is a release gate for claims about end-to-end cryptographic proofs.
