# Implementation journal

## 2026-10-26 — Discovery + M1

- Verified the workspace (`Documents/meta-hackathon`) was empty and independent of
  Hyphae Core; not a Git repository (git init pending owner instruction).
- Read Hyphae's public Agent Memory contract (`docs/product/agent-memory.md` at commit
  `418a3da717c2f9ba0cf095423f392b3d496cb607`). Confirmed MCP tools
  `hyphae_memory_store/journal/recall/forget/status`, permission profiles
  (`memory-reader` exposes recall+status only), project+`_global` isolation, and
  `recall(prove)` sealed proofs verified via `hyphae proof verify`.
- License evidence at the pinned commit: root `LICENSE` is Apache-2.0; docs are
  CC-BY-SA-4.0. Competition-repository license left undecided per owner.
- Found no Rust/MSVC toolchain and no Meta API credential on the machine. With the
  owner's approval, installed via winget: Visual Studio Community 2022 (MSVC 14.44
  tools + Windows 11 SDK) and Rustup 1.29.1 with `stable-x86_64-pc-windows-msvc`
  (rustc/cargo 1.99.1). Verified with a hello-world build.
- Implemented M1: 4-crate workspace, typed contracts + generated JSON Schemas,
  deterministic state machine (max 2 revisions, no-progress exit, human-only
  authorization), crash-safe JSONL event log (sequence validation, fsync,
  truncated-tail recovery, idempotent appends, read-only replay), orthogonal evidence
  statuses, recommendation policy, ports+fakes, reference scenario, demo binary.
- Tests: `cargo test --workspace` → 31 passed (17 runtime unit, 14 integration).
  `cargo clippy` warnings fixed to zero; `cargo fmt` applied. Demo binary verified:
  challenge → revision → `supported` → approval recorded with no execution; replay on
  re-run.
- Not done at M1 (by design): Meta API adapter (M2), Hyphae MCP adapter +
  compatibility probe (M3), operator console, executor (deferred through M3).

## 2026-10-26 (later) — M2, M3, console, evaluation ("continua hasta el final")

- Owner set the session goal to continue to completion. Approval-gated items
  (Hyphae install, credentials, remotes, publishing, codex) remain untouched.
- **M2**: verified Meta's structured-output contract at dev.meta.ai
  (Chat Completions `response_format.json_schema {name, schema, strict}`;
  Responses uses `text.format`). Implemented `MetaModelClient`
  (draft schemas via schemars, id assignment adapter-side, unknown-id
  filtering, omitted-claim = uncorroborated, timeouts, bounded 429/5xx
  retries, exact model identity from the response). 4 tests vs. a local fake
  HTTP server; live test gated (`CONSENSUS_LIVE_TESTS=1` + key), not run.
  ADR-0002.
- **M3**: verified Hyphae's MCP contract at the pinned commit
  (`hyphae mcp --profile memory`, JSON-RPC 2.0 NDJSON stdio, rev 2025-06-18;
  read-only without `--allow-write`; `hyphae proof verify --proof/--witness/
  --anchor`). Implemented injectable-transport MCP client + proof verifier
  with status mapping. 5 tests (in-memory transport, scripted runner);
  live probe gated and not run (no isolated runtime). ADR-0003.
  `EvidenceReference` gained optional `proof_file`/`witness_file`.
- **Console**: Axum + SSE replay, single-page operator UI with explicit
  fake/meta × fake/offline/hyphae modes, decision + receipt endpoints.
  Verified live over HTTP: run → `supported` → approve → receipt; `meta`
  without a key returns an explicit 400. ADR-0004.
- **Demo fixture**: `demo-repo/payment-client` (standalone crate, retry bug
  + constraint test, 2 tests green). Consensus never modifies it.
- **Evaluation**: 7-case deterministic battery — baseline unsafe 6/7,
  consensus safe 7/7 (`docs/evaluation.md`). Pitch + demo storyboard in
  `docs/pitch.md`, `docs/demo-script.md`.
- Remaining blockers (all owner-gated): Meta API key for live runs; Hyphae
  4.0.0 install + isolated profile for the live probe and adversarial proof
  case; git init/remote; repository license selection; demo video recording.

## 2026-10-26 (evening) — live Meta verified; Hyphae Agent Memory surface blocked upstream

- Owner supplied the Meta key via User-scope environment variable (never
  through chat; never printed). **Live Meta test passed**: `muse-wonder-1.0`,
  real request id, 16 s.
- **Live console run** (`model=meta`, `memory=fake`, 70 s, 3 calls,
  8,627 output tokens total): the real Proposer suggested idempotency-gated
  retries "at most twice"; the real Critic raised 9 substantive objections
  (including a `constraint_conflict` demanding proof of ≤3 total attempts
  against the stored constraint); the real Evaluator **abstained** — 0/6
  claims corroborated, noting verbatim that valid integrity does not
  establish factual correctness. Recommendation: `abstained`, authorization
  left pending for the human. This is the protocol working as designed
  against a live model, not a scripted outcome.
- **Hyphae installed** (owner-approved): official 4.0.0 Windows zip,
  SHA-256 verified against the GitHub asset digest. Engine, security
  bootstrap, catalog/search provisioning, HTTP serve, and the full-profile
  MCP all work on an isolated in-workspace directory with role-ceilinged
  keys (owner → reader principal, built via documented commands only).
- **Blocked upstream**: the Agent Memory surface does not start on this
  machine. `hyphae agent setup|status|doctor|configure` and
  `hyphae mcp --profile memory` all exit 2 with
  `native product request is invalid` — reproduced in the owner's own
  terminal (no sandbox involved) and after complete manual provisioning
  (memory-schema collections 13/16/19, provisioned, reader key). The
  full-profile MCP `initialize` succeeds with the same transport and keys,
  so the failure is specific to the memory profile's startup validation
  in the Windows 4.0.0 build (or an undocumented provisioning step only
  `agent setup` performs). The gated `live_hyphae_probe` remains ready to
  run unchanged once that surface starts. The owner's primary Agent
  Memory directory does not exist on this machine and was never touched;
  all Hyphae state created here lives under `hyphae-isolated/` (git-ignored).
  Follow-up probes (same evening): the memory profile was also tried over
  the native local endpoint (named pipe) and with no collection flags —
  six variants total, all dying at startup with the identical error before
  answering `initialize`, while the full MCP profile answers normally with
  the same keys. 4.0.0 remains the latest published release. The console
  now runs with full Hyphae environment configured, so `memory=hyphae`
  surfaces the true upstream connection failure (HTTP 502) instead of a
  configuration error, and will work unchanged once the upstream surface
  starts.
- **Docker unblock (same night, owner-directed):** the pinned Linux build
  runs in a local container (`debian:trixie-slim`, named volume) where
  `agent setup`, the daemon, seeding, and `mcp --profile memory` all work.
  The adapter gained an honest degradation: tool-error envelopes inside
  `structuredContent` are errors, and failed proof generation retries
  unproven (integrity `unavailable`, never fabricated). A `docker exec`
  stdio wrapper (`scripts/hyphae-docker.cmd`) lets the Windows host drive
  the container's MCP and `proof verify` with container paths throughout.
  `live_hyphae_probe` passed; a full live Meta+Hyphae run retrieved the
  real seeded evidence and abstained. Runbook:
  `scripts/hyphae-container/README.md`.

## 2026-10-26 (night) — independent audit and fix round (schema 1.1)

- An independent read-only audit (`docs/reviews/2026-10-26-claude-opus-review.md`)
  found policy holes, an unverifiable log, silent mode substitution in the
  console, unauthenticated approval, a blind revision step, and overstated docs.
  An earlier session implemented the policy, schema 1.1, hash-chain, and console
  fixes; a headless run then finished and verified the rest from
  `docs/reviews/2026-10-26-claude-fix-brief.md`. Report:
  `docs/reviews/2026-10-26-claude-fix-report.md`.
- **Two defects surfaced while verifying the earlier fixes.** Both are fixed
  with regression tests.
  1. The live Hyphae tool-error envelope says "corruption" only in
     `error.category`. The adapter kept only `message` ("native durable state
     is invalid"), so the live corruption signal would still have degraded to
     `unavailable`, and audit finding 11 was not actually fixed live. The
     category is now kept, and live records come back `invalid` → `blocked`.
  2. The policy ranks an invalid proof above abstention, but the state machine
     rejected `blocked` in the Abstained state. The orchestrator then appended
     a record that failed replay, and on restart the console refused to start.
     Once (1) was fixed, every live run where the Evaluator abstains over
     retrieved evidence would have hit this (9 of the 10 completed live runs). Fixes: `blocked` is allowed from Abstained (never an upgrade);
     every event is validated before it is persisted; startup recovery reports
     unreplayable runs instead of aborting.
- Also: console snapshots report a corrupt log line (500) instead of skipping
  it; `/decision` checks the override rule against the owning log; the
  per-run evidence cap is strict and recorded; simulated proofs say
  `SIMULATED` in every verification record; the evaluate example is now a
  conformance check (no baseline claim, no "UNSAFE" timeout); Hyphae
  container scripts are pinned to LF line endings.
- Gates: `cargo fmt`, `cargo clippy --workspace --all-targets` (0 warnings),
  `cargo test --workspace` → 67 passed + 2 ignored live tests (56 at handoff);
  schemas regenerated for 1.1; evaluate 7/7.
- Console smoke test against the real binary over HTTP. Page fields, the 400 on
  silent substitution, invalid proof → blocked, 401 without the token, 400 for
  an override without a note, 200 with both, idempotent retry, receipt
  `head_hash`, and SSE replay all behaved; a hand-edited log was refused at
  startup.
- **Live battery** (`docs/evaluation.md`): 12 Meta + Hyphae runs, 33 calls →
  9 blocked, 1 abstained, 2 failed safely on 60 s timeouts, 0 supported. The
  Evaluator abstained in every completed run. The Critic flagged the stored
  constraint in 5 of 11 critiques. A planted "SYSTEM OVERRIDE" memory
  (`injection-lab` project) was treated as contradicting evidence, not obeyed.
  Isolation held for `other-repo`. Three supplementary `meta + fake` runs
  (simulated proofs, labeled) also abstained 3/3, so live `supported` is
  currently out of reach for model-behavior reasons as well as the upstream
  proof failure.
- **Corrections to earlier entries.** In the evening `meta + fake` run, the
  "valid integrity" the Evaluator mentioned was simulated (`fake-verifier`).
  The "honest degradation (integrity `unavailable`)" described above was
  replaced: a corruption report now blocks.
- Left to the owner: repository license; pushing; running Hyphae's
  `agent doctor` / `recover` on the container store (the error says
  `retry: after-recovery`, but those commands can mutate the store); and the
  design options in `docs/evaluation.md` §3.

## Backlog

**Must have:** M2 Meta adapter (structured output vs. `schemas/`, timeouts/retries/
budgets, gated live test) · M3 Hyphae MCP adapter + proof verification + adversarial
proof case · operator console · synthetic demo repo · baseline comparison · submission
pack.

**Should have:** cost/latency rollups on receipts · run-list/replay view in console ·
more adversarial memory cases (expired TTL, anchor mismatch as supported by the real
proof contract).

**Deferred:** restricted executor (post-M3, separate review) · semantic-search memory
features · multi-run persistence beyond one JSONL log per deployment.
