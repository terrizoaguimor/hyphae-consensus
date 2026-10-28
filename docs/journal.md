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

## 2026-10-27 — owner decisions: live proofs, policy 1.2, a live `supported`

- Headless run from `docs/reviews/2026-10-27-claude-fix-brief-2.md` (the prompt
  again arrived as "Read"). Report:
  `docs/reviews/2026-10-27-claude-fix-report-2.md`.
- **Hyphae proofs work live.** With owner authorization (container only,
  store backed up first), `hyphae agent doctor` reported the store healthy:
  "corruption" was Hyphae's generic code for a failed proof. Cause 1: the
  daemon's socket was inside its data directory, and the proof witness
  bundles that whole directory and rejects special files. Fixed by serving
  with `--endpoint` outside it, as `agent setup`'s systemd unit does. Cause 2,
  found after seeding richer memory: the witness travels inside one MCP
  response capped at 4 MiB, so stores past about 3.9 MB cannot be proven; the
  store was replaced by a fresh 3.3 MB one, and the hostile note moved to a
  separate lab store. `live_hyphae_probe` now verifies every recalled record
  (`hyphae proof verify`, semantic re-execution) and rejects a wrong anchor.
- **Schema 1.2.** A recorded contradiction outranks abstention; a revision may
  follow an abstention that lists open items; replay re-derives each run with
  its own version's rules, so the 1.1 logs still replay. Prompt guidance for
  memory-checkable claims and evidence-cited corroboration. Per-run token cap
  (`CONSENSUS_MAX_RUN_TOKENS`, 150,000), append-rollback fault injection,
  120 s model timeout, label-only Critic items rejected.
- **Two defects of this round, found live and fixed.** (1) The first
  Evaluator wording let confirmed objections count as resolved (pre-fix runs
  P01/P02; P02's `supported` does not count). (2) With `strict: false` the
  model renamed or dropped optional draft fields and the adapter read them as
  empty: fail-open for objections and contradictions. Raw drafts captured with
  the new `CONSENSUS_META_DEBUG_DIR` proved it; the fix is strict structured
  output plus required fields, and 5/5 replays then conformed.
- **Live results** (`docs/evaluation.md` §5, 18 runs, all pinned to an
  out-of-band anchor, every proof `valid`). Battery 1 (8 runs): 1 supported
  (R06: the revision dropped an unsupported 30 s timeout for the stored
  retry-policy fix, and each claim cited records that verified), 2 contested,
  5 insufficient evidence. Final battery (6 runs, strict output): 6 contested;
  every final claim corroborated, and the Critic's open objections (mostly
  missing tests) block `supported`. Hostile note with a valid proof (2 runs):
  flagged, recorded as contradicting, never obeyed; contested; then forgotten
  from every served store.
- Gates: `cargo fmt`, `cargo clippy --workspace --all-targets` (0 warnings),
  `cargo test --workspace` → 89 passed + 2 ignored live; schemas regenerate
  identically; evaluate 10/10; console HTTP smoke passed on the final binary.
- Left to the owner: the license; pushing; whether any class of Critic
  objection should stop blocking `supported`; carrying the proposal's
  uncertainties into the recommendation; pruning proof artifacts and archived
  stores in the container.

## 2026-10-27 (afternoon) — independent audit and the public-portal plan

- A read-only, multi-agent audit of `72e422a`: seven dimensions, each
  challenged by an adversarial verifier, four research sweeps, a completeness
  critic, and a three-lens red team of the resulting plan. Report:
  `docs/reviews/2026-10-27-independent-audit.md`. Every documented gate and
  battery number reproduced (89 passed + 2 ignored; evaluate 10/10; 585 chained
  records recomputed; 33 recommendations re-derived; no credential in the
  history or in `demo-run/`).
- 91 findings. Two fail-open paths toward `supported`: MA-1 (Evaluator ids
  that are not byte-exact are dropped silently) and RT-1 (earlier-round
  objections and contradictions are never re-judged; R06's 7 round-1
  objections were not). The console router must never be exposed publicly
  (SEC-1 to SEC-11).
- Owner decisions: Apache-2.0 for this repository (copyright Mario Gutiérrez;
  `NOTICE`; Hyphae is not relicensed); a public portal with guided (simulated),
  live (owner key, curated presets, caps) and recorded-showcase modes; no hosted
  BYOK; cross-round carry-forward (protocol 1.3); a sealed Hyphae snapshot for
  public live runs; daily and lifetime spend caps. Work continues on branch
  `feat/public-portal`, with one local commit per verified milestone.
- When this entry was written, the branch held `35297e7` (LICENSE, SPDX
  headers, Cargo license fields, `publish = false`, ignore rules), `e1304f2`
  (runtime refactor for plan item B2: `EventSink`, `create_task`/`deliberate`
  split, `RunControl` checkpoints) and `ff0d2fb` (byte-identical research
  archive in `research/` with MANIFEST and replay tests, plan item D1). The
  documentation change that adds this entry also adds `NOTICE`, the audit
  report, dated corrections and errata across the docs, and an index of
  `docs/reviews/`.
- Corrections to the entry above: pushing is no longer pending, because
  `origin/main` was pushed to `72e422a` on 2026-10-27; the license is decided.

## 2026-10-27 (night) — protocol 1.3 validated live; the public-portal lanes integrated

- Branch `feat/public-portal`, one local commit per verified milestone, nothing
  pushed. Landed since the plan was approved: Apache-2.0 license, SPDX headers and
  `NOTICE`; the byte-identical `research/` archive with MANIFEST and replay tests;
  `EventSink`/`RunControl`; `PolicyTrace`, receipts v2 and the hardened event log;
  protocol 1.3 (ADR-0010: fail-closed evaluation contract, carry-forward, explicit
  per-item verdicts); per-run `RunStore`, run index and `SharedRunLog`; hardened Meta
  and Hyphae adapters (ADR-0002 note); CI workflow and the Linux parity script;
  cargo-deny and third-party notices; ADR-0005 to ADR-0010 and the release checklist;
  the web shell, legal, docs and onboarding pages (`apps/web`); the keyed per-call
  Evaluator schema with one bounded post-send transport retry (tag `prompt-freeze-2`);
  lane S-b1 (public API v1, run manager, live SSE, sessions, listener split,
  contracts, `verify` CLI) and lane S-b2 (USD ledger, Turnstile, presets, geo, judge
  codes) merged as `9dca9f1` and `f52b68f`. Gates on the merged tree: fmt, clippy
  `-D warnings`, 494 tests passed / 2 ignored, evaluate 10/10.
- Live validation (`docs/evaluation.md` §6): the two smoke runs on `prompt-freeze-1`
  (K01, K02) exposed an empty-`assessments` hole in the strict schema and the cost of
  never retrying a sent request; the two on `prompt-freeze-2` (K03, K04) confirmed the
  keyed schema (one assessment per claim, one verdict per objection, Meta accepted the
  per-call schema) and both ended `supported` **for a revised proposal that departs
  from the task text**. That is a property of protocol 1.3 to show (the Decision
  center and the receipt must put the scope change next to the outcome), not a bug to
  hide; the runtime does not judge whether a revision still serves the task. Four of
  the eight authorized live runs are spent; the counted battery has four left.
- The one flaky test (`shared_log` stream gap test) was timing-dependent and is now
  deterministic (the writer waits until the stream filled the first gap).
- Open owner decisions: the judge lane's daily cap (default = the whole $15 slice),
  the ring's eviction of in-window visitor runs under load, the public contact email,
  the legal texts, and the Day-2 items (hostname, Tunnel, Turnstile, server key).
- In flight as this entry is written: the sealed Hyphae snapshot (capture tooling and
  the `SnapshotMemoryClient`), the public live-mode wiring (`LiveGate`, per-attempt
  metering in the Meta adapter), and the first run views of Mission Control (API
  client generated from the contract, SSE client, New deliberation, My runs, the
  Live/Playback, Explorer and Decision tabs).

## Backlog

*Status 2026-10-27 (independent audit):* M2, M3, the operator console and the
synthetic demo repo are done. The baseline comparison was never executed
(`docs/evaluation.md` §1); the portal plan offers an optional single-model
baseline battery that needs the owner's spend approval. The "submission pack" is
now the plan's work items F1–F5. The deferred "multi-run persistence beyond one
JSONL log per deployment" is replaced by per-run logs (plan item B1). The list
below is kept as written.

**Must have:** M2 Meta adapter (structured output vs. `schemas/`, timeouts/retries/
budgets, gated live test) · M3 Hyphae MCP adapter + proof verification + adversarial
proof case · operator console · synthetic demo repo · baseline comparison · submission
pack.

**Should have:** cost/latency rollups on receipts · run-list/replay view in console ·
more adversarial memory cases (expired TTL, anchor mismatch as supported by the real
proof contract).

**Deferred:** restricted executor (post-M3, separate review) · semantic-search memory
features · multi-run persistence beyond one JSONL log per deployment.
