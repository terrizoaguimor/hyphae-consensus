# ADR-0005: Per-run event logs, asynchronous runs and live streaming

Date: 2026-10-27 · Status: accepted; runtime part implemented (`e1304f2`, `200b0df`,
`a6911e4`, `745609b`), server part implemented in lane S-b1 and being integrated ·
Scope: `consensus-runtime` (`run_store.rs`, `shared_log.rs`, `sink.rs`), `consensus-api`
(`runs.rs`, `stream.rs`)

Numbering is not chronological: ADR-0007 to ADR-0010 were written first, while this
decision's lanes were still landing.

## Context

The prototype the independent audit examined (`docs/reviews/2026-10-27-independent-audit.md`)
kept every run of a server in one event log behind one `std::sync::Mutex` that the
orchestrator held for the whole deliberation (87–407 s live), inside async handlers;
`POST /api/runs` answered only when the run had finished, past Cloudflare's 125 s read
timeout; the SSE route replayed the file at 30 ms per event and could not follow a run;
the whole log was re-parsed on every request; a crash left no way to tell which runs were
cut short; and a single run's receipt could not be checked without the whole multi-run
chain (findings SEC-4/5/6, RT-2/3/6/9/13, UI-1/2, LC-07/12/15). The portal needs runs
that start in milliseconds, stream while they deliberate, survive a browser disconnect,
can be cancelled, and are isolated per owner.

## Decisions

1. **One hash-chained log per run (`RunStore`).** A run lives in `runs/<run_id>.jsonl`,
   opened with the existing `EventLog` so the genesis-anchored chain, sequence numbers
   from 1, fsync, rollback and torn-tail recovery are reused unchanged. Run ids are
   validated as `run-` + UUID before any path is joined; an unknown id is 404 and a read
   never creates a file. A sidecar `<run_id>.meta.json` records the owner
   (`sha256(session id)`, `operator` or `showcase`), the run kind and its retention; it is
   written atomically and cross-checked against the chain, and a disagreement surfaces as
   `provenance_inconsistent`. Each log is guarded by an exclusive lock file, so a second
   writer (another process, a stale handle) fails instead of forking the chain.

2. **A hash-chained run index.** `index.jsonl` records each run's creation and its
   terminal head, so a run file that disappears or is truncated is detectable, and
   restart recovery can mark deliberations that were cut short as `interrupted`
   (`RunFailed{code: "interrupted"}`) before anything is served. Purge follows the
   recorded retention; it removes whole runs and records what it removed.

3. **Legacy logs are read-only.** Multi-run logs written before this decision
   (`demo-run/events.jsonl`, the battery logs now archived in `research/`) are opened
   through `EventLog::open_read_only`, which verifies and never repairs, and through
   `verify_bytes` for downloaded copies. The writable `open` is reserved for the run
   store's own files.

4. **Validated, durable, then published (`SharedRunLog`).** Every append is validated
   against the run's current aggregate under the run's lock, appended, fsync'd, and only
   then handed to the `EventPublisher`; the lock is released before publication. A
   publisher that panics is caught (`publish_failures` counts it) and never unwinds into
   the writer, so a subscriber bug cannot abandon a run. Nothing a subscriber sees is
   absent from the log.

5. **Admission before durability, durability before spawning (`RunManager`).** A start
   takes a deliberation permit (guided and live pools are bounded separately) before
   anything durable exists, creates the run files, appends `TaskCreated` through the
   shared log, and only then spawns the deliberation on the blocking pool and answers
   `202` with the run's summary. Each deliberation runs under `catch_unwind`: a panic
   becomes `RunFailed{stage: "panic", code: "panic"}` through the run's own sink and
   poisons nothing else. Cancellation, the wall-clock deadline and shutdown trip the run's
   `RunControl`, which the orchestrator honours at its next checkpoint (before every model
   call and every recall); a browser disconnect never reaches the manager. While a run
   deliberates its writer is the only one, reads come from the writer's durable snapshot
   and a decision is refused (`409`); once it rests, the writer is dropped, the lock is
   free, and reads and decisions go through the store, owner-checked.

6. **Live SSE: subscribe, replay, follow (`stream.rs`).** A per-run bounded broadcast
   channel exists while the run is active. A stream subscribes first, replays the durable
   log from the client's `Last-Event-ID` (or `?after`), then follows the channel; it
   dedupes by sequence and, on a lag, a gap or a closed channel, re-reads the log and says
   so with a `resync` frame. Every `consensus` frame names its delivery, `replay` or
   `live`, so a client can never label a replayed record as live. A heartbeat comment
   (15 s) keeps the connection open through Cloudflare's read timeout, a `retry` hint is
   sent at the start, and `stream_end` closes the stream once the run awaits a human or
   is terminal. Streams are capped per run, per client (IPv4 /32 or IPv6 /64) and per
   process.

7. **Retention is per run.** Visitor runs stay reachable for 24 h and are deleted whole
   after 7 days; a bounded ring of visitor runs (`503 storage_full` beyond it) bounds
   disk use; operator runs are kept. Deleting a run never breaks another run's chain.

## Consequences

- A run's receipt and its verbatim log can be verified on their own
  (`consensus-api verify <log> [--run <id>] [--receipt <file>]` recomputes the chain and
  re-derives the policy), and the archive in `research/` stays byte-identical because
  nothing writable ever touches it.
- The old console's single log and blocking `POST /api/runs` are gone; the operator
  surface (ADR-0006) uses the same manager and store as the public one.
- Two writers on one run are impossible by construction; two processes on one store are
  refused by the lock files, which also means a crashed process's locks must be reaped by
  restart recovery before the store accepts writes again.
- The test that replays a reordered publication (`shared_log::tests`) holds the writer
  until the stream has filled the first gap, so it asserts exactly two fills; the first
  version of that test was timing-dependent and flaked under load.
