# Mission Control frontend — implementation notes

> **Superseded (2026-10-27).** These notes describe the loopback operator console
> (`crates/consensus-api/src/console.html`) as merged into `main` in `c3ba8a1`.
> The approved portal plan replaces this UI with a new Mission Control frontend
> (work items E1–E9) and the backend contract below with an asynchronous,
> session-scoped API (B1–B5, C1–C8); the legacy console is planned to survive only
> as a loopback operator tool. The text below is kept as written, with two
> corrections: the branch `feat/mission-control` was merged (`c3ba8a1`), and Hyphae
> proofs verify live since 2026-10-27 (constraint 4 below), so "true Hyphae proof
> integration" is no longer future work. The independent audit of 2026-10-27
> (`docs/reviews/2026-10-27-independent-audit.md`) also found that the result views
> show simulated proofs with the same `valid` badge as real ones (UI-4), that
> Approve and Reject act on a global selection that can differ from the run on
> screen (UI-3), and several accessibility gaps (UI-8).

Date: 2026-10-27  
Branch: `feat/mission-control`  
Status: initial operator UI delivered; live streaming and true Hyphae proof integration remain future work.

## Scope implemented

- Replaces the legacy single-page console with a high-density, responsive dark operator dashboard.
- Preserves the existing Axum API routes and payload contracts; no backend or Hyphae Core modifications.
- Shows all three logical roles, persisted event timeline, orthogonal evidence statuses, recommendation, human authorization, model telemetry and receipts.
- Exposes explicit `fake`/`meta` model and `fake`/`offline`/`hyphae` memory modes, plus a scripted-scenario selector for `model=fake` (reference, offline, invalid proof, hostile memory, abstention) labeled "simulated proofs".
- Decision panel: operator-token field (the token is printed by the server at startup) and a decision note, required to approve a blocked or abstained run; the idempotency key is stable per run.
- Renders remote model and evidence content through `textContent` / DOM node creation instead of HTML interpolation.
- Labels SSE replay as persisted replay; does not claim live streaming.
- Preserves record-only approval; no executor is introduced.

## Verification

- JavaScript extracted from `console.html` passes Node.js `vm.Script` syntax parsing.
- 2026-10-26 (audit-fix round): HTTP smoke test against the real `consensus-api serve` binary on Windows (temp event log). The page is served with the scenario selector, token, and note fields. Over HTTP: `fake+hyphae` → 400; the `invalid-proof` scenario → `blocked`; a decision with no or a wrong token → 401; approving `blocked` without a note → 400; with token + note → 200; retrying the same key appends nothing; receipt and SSE replay work; `retry`/`offline`/`injection`/`abstain` scenarios reach their documented outcomes; a hand-edited log is refused at startup (`hash chain broken`).
- That smoke test drove the API the page calls; it did **not** automate clicks in a real browser. Live `meta`/`hyphae` runs were driven through the same API (see `docs/evaluation.md`).

## Existing backend constraints

1. `POST /api/runs` blocks until the orchestration finishes and returns the completed run; it does not issue an early run ID.
2. `GET /api/runs/{id}/events` replays persisted events and ends with `replay_complete`. It is not a real-time subscription.
3. The event-log mutex is currently held by `execute_run` during the full orchestration; concurrent human/stream operations are constrained.
4. `memory=hyphae` works through the pinned Linux build in a local Docker container (`scripts/hyphae-container/`); the Windows 4.0.0 Agent Memory surface still does not start on this host. Since 2026-10-27 proofs verify live; the daemon must listen outside its data directory and the store must stay below about 3.9 MB, or proven recall fails (ADR-0003).
5. Only `/decision` is authenticated, by a per-process operator token printed at startup. Run creation, listing, and receipts are unauthenticated, and there is no CSRF protection. The operator API binds to loopback by default and **must not be exposed publicly**.

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
