# ADR-0001: M1 architecture — contracts, state machine, event log

Date: 2026-10-26 · Status: accepted · Scope: milestone M1

## Context

Hyphae Consensus must coordinate untrusted model participants over a verifiable memory
backend (Hyphae Agent Memory, pinned at commit
`418a3da717c2f9ba0cf095423f392b3d496cb607`, release 4.0.0) while keeping every decision
auditable and human-authorized. M1 must deliver a compiling workspace, typed contracts,
a deterministic state machine, event-log recovery, and passing tests — with no live
model or Hyphae dependency.

## Decisions

1. **Four crates.** `consensus-domain` (contracts, no I/O), `consensus-runtime`
   (state machine, event log, orchestration, policy, ports, fakes), `consensus-api`
   (operator entry point), `consensus-tests` (integration/adversarial scenarios).
   A 10-crate split was rejected: it slows the MVP without adding safety. Conceptual
   boundaries live on as modules.

2. **Models cannot write outcomes.** Model-produced types (`Proposal`, `Challenge`,
   `Evaluation`, `Revision`) contain no outcome, vote, or authorization fields. The
   runtime derives `Recommendation` in one pure function
   (`consensus_runtime::policy::derive_recommendation`). `AuthorizationState` changes
   only through a `HumanDecisionRecorded` event whose actor must be `Actor::Human`.

3. **Orthogonal evidence axes.** `EvidenceVerification` carries independent
   `integrity_status`, `factual_status`, and `relevance_status`. Integrity comes from
   the `ProofVerifier` port (Hyphae `proof verify` in M3); factual/relevance come from
   evaluator judgment. Policy: invalid proof → `blocked`; abstain → `abstained`;
   contradicted/unresolved → `contested`; uncorroborated claim or cited evidence not
   integrity-`valid` → `insufficient_evidence`; else `supported`. A valid proof over
   uncorroborated content therefore never yields `supported`.

4. **Deterministic state machine.** `Created → Proposed → EvidenceReview → Evaluated
   (⇄ Revised) | Abstained → AwaitingHumanApproval → Approved | Rejected`, plus
   `Failed` from any non-terminal state. Max 2 revision rounds; a no-progress exit
   stops revision when the unresolved-challenge count does not decrease.
   `RunAggregate::apply` is pure: replaying the event sequence rebuilds state exactly.

5. **Append-only JSONL event log** (no second database). Gap-free sequences validated
   on open; `fsync` per append; a truncated final record (crash mid-write) is
   discarded and reported via `recovered_truncated_tail`; mid-log corruption and
   sequence gaps are hard errors. Appends are idempotent on `event_id`; human
   decisions derive their event id from the authorization's `idempotency_key`, so
   decision retries are no-ops. Replay is read-only and triggers no external effects.
   The log is separate from Hyphae memory (application events vs. knowledge records).

6. **Ports and fakes first.** `ModelClient`, `MemoryClient` (read-only; maps to MCP
   `hyphae_memory_recall`/`hyphae_memory_status` under the `memory-reader` profile),
   `MemoryAdmin` (runtime-service-only writes; unreachable from agent code),
   `ProofVerifier`, and `ExecutorPort` (deferred: never invoked through M3 — human
   approval is recorded only). The runtime is synchronous in M1; M2 introduces the
   async Meta adapter behind the same contracts.

7. **Hyphae-offline degradation.** Memory unavailability records
   `evidence_unavailable` events and caps recommendations below `supported`;
   orchestration, replay, receipts, and authorization recording keep working.

8. **Receipts preserve exact model identity.** `ModelInvocation.model` stores the
   exact Model API string (production: `muse-wonder-1.0`). Fakes use
   `provider: "fake"` so test data can never be mistaken for live calls. The
   development model in Muse (Muse Wonder Full) is unrelated and never appears
   in receipts.

9. **Licensing posture.** Hyphae's root `LICENSE` at the pinned commit is Apache-2.0
   (verified by reading the file at that revision, 2026-10-26); its docs are
   CC-BY-SA-4.0. This repository's license is deliberately undecided pending the
   owner's choice. No Hyphae code is vendored or modified.

## Consequences

- M2 must implement structured-output validation against `schemas/` before any model
  payload becomes an event; malformed output already fails runs safely in M1 tests.
- M3 must verify the installed Hyphae binary's actual MCP tool list against the
  `MemoryClient` contract before writing the adapter, and must use an isolated
  Hyphae profile/data directory — never the owner's primary Agent Memory data.
  No isolated Hyphae runtime exists on the development machine yet, so the M1
  compatibility probe is deferred until one is approved and installed.
- The synchronous orchestrator will need an async boundary in M2; ports were kept
  small so this change stays local to the runtime crate.

## Amendment — 2026-10-26 audit-fix round (schema 1.1)

An independent audit of 2026-10-26 found
gaps in decisions 2, 3, and 5. Changes, each covered by tests:

- **Decision 2, stated precisely.** Models never *write* the outcome field,
  but the Evaluator's per-claim judgments (`corroborated`, relevance,
  contradictions, `unresolved_items`) are the policy's main inputs. The
  Critic affects the outcome only through the Evaluator's per-item
  unresolved judgment. "Models never decide" was true in form only.
- **Decision 3, policy (in priority order).** (1) An invalid proof
  *anywhere in the retrieved set* → `blocked`, even if no claim cites the
  record, and even when the Evaluator abstained. (2) Abstain → `abstained`.
  (3) A contradicted record among cited *or contradicting* evidence, or any
  unresolved challenge item → `contested`. (4) Any uncorroborated claim,
  any corroborated claim citing no evidence of its own, cited evidence not
  integrity-`valid`, or no checked evidence → `insufficient_evidence`.
  (5) Otherwise `supported`. A corruption signal from the backend's proof
  generation counts as an invalid proof.
- **Decision 4.** Challenge items carry ids; progress is measured per item,
  so the second revision round is reachable. An abstained run may be
  `blocked` (invalid proofs outrank abstention) but never upgraded.
- **Decision 5.** Every 1.1 record carries a SHA-256 hash-chain link
  (`prev_hash`/`hash`) validated on open; 1.0 records remain readable as a
  legacy prefix. Replay recomputes the recommendation from the recorded
  evaluation and verifications and rejects a stored kind that does not
  match. The orchestrator validates each event against the state machine
  *before* appending it, and a failed append is rolled back. Decision
  idempotency keys are scoped per run. On startup, runs left
  mid-deliberation by a dead process are closed as
  `Failed{stage:"interrupted"}`; a run whose history fails replay is
  reported and left untouched instead of blocking startup.
- **Limit.** The chain is tamper-*evident*, not tamper-proof: it is not
  signed, so someone with write access can rewrite and re-hash the whole
  file. Detection then relies on comparing against a previously exported
  receipt's `head_hash`.

## Amendment — 2026-10-27 owner decisions (schema 1.2)

The owner decided the open design items from the 2026-10-26 fix report. Each
change has tests; record shapes are unchanged, so the JSON Schemas regenerate
identically.

- **Decision 3, policy order (1.2).** (1) An invalid proof anywhere in the
  retrieved set → `blocked`. (2) **New:** a contradicted record among cited or
  contradicting evidence → `contested`, even when the Evaluator abstained: an
  abstention must not hide a contradiction the Evaluator itself recorded (live
  run S02 on 2026-10-26 did exactly that). (3) Abstain → `abstained`. (4) Any
  unresolved challenge item → `contested`. (5) `insufficient_evidence` as
  before. (6) Otherwise `supported`. Rules 1 and 3–6 keep their 1.1 meaning;
  only the contradiction moved above abstention.
- **Decision 4, revision after abstention.** An abstention that still lists
  unresolved items may be answered by a revision, bounded by the same 2-round
  limit and per-item no-progress exit. An abstention with nothing unresolved,
  or a `blocked` result, still ends deliberation. The state machine enforces
  the same rule (`AbstainedWithoutOpenItems`), and an abstained run may now
  receive `abstained`, `blocked`, or `contested`, never an upgrade.
- **Replay stays version-faithful.** Replay re-derives each recorded
  recommendation with the rules of the record's own schema version
  (`PolicyVersion::for_schema`): 1.1 runs keep the 1.1 order, so the
  2026-10-26 logs still replay and stay tamper-checked, and 1.2 runs use the
  new order. Checked against both local logs (`demo-run/events.jsonl`,
  `demo-run/live-battery-2026-10-26.jsonl`): every run replays.
- **Bounded cost.** A per-run token cap (`CONSENSUS_MAX_RUN_TOKENS`, default
  150,000 input + output tokens) is enforced by the runtime after every model
  call, for any `ModelClient`. Crossing it, or a call that reports no usage,
  fails the run with stage `token_budget`; the call that crossed it is still
  recorded, so the receipt accounts for every token. A run can overshoot by
  at most that one call.
- **Append rollback is now tested.** A test-only seam makes the next append
  write part of its record and fail; the test shows the file is rolled back
  byte-for-byte, the event is absent, a fresh open sees a valid log with no
  torn tail, and the same handle commits the retry.

## Amendment — 2026-10-27 independent audit (corrections; no code change)

The independent audit of `72e422a`
(`docs/threat-model.md` section 4 tracks the status of its findings) found statements in this ADR
that do not match the code. They are corrected here; the decisions above are
kept as written.

- **Decision 4 and both amendments: "progress is measured per item" and
  "per-item no-progress exit".** Not so. The no-progress exit compares the
  number of unresolved items in the new round with the previous round
  (`unresolved >= previous` in the orchestrator), and the Meta adapter mints new
  item ids every round, so no item is tracked across rounds (audit finding
  RT-7). Ids did make the second revision round reachable. Set-based progress
  is planned together with cross-round carry-forward (portal plan, work item
  A2).
- **Decisions 2 and 3: the policy sees only the final round.** Objections and
  contradictions recorded in earlier rounds are not carried into later rounds or
  into the policy's input (RT-1), and the Meta adapter silently drops Evaluator
  ids that are not byte-exact (MA-1). Both can lead to `supported` while an
  objection or contradiction is still open; see the threat model's "Known gaps"
  section. Planned fixes: A2 and A1.
- **Decision 6 and the Consequences: no async adapter.** M2 did not introduce an
  async Meta adapter. The adapter uses `reqwest::blocking`, the orchestrator is
  synchronous, and the console runs it on a blocking thread
  (`docs/adr/0002-meta-model-adapter.md`, decision 7; audit finding DOC-10).
- **The 1.1 amendment's "Limit": what a receipt's `head_hash` covers.** It is
  the hash of the run's last event at receipt time, not the head of the log
  file. The chain is log-wide, so verifying one run's receipt needs the whole
  file, and edits made after that event are outside the receipt (DOC-9, RT-4).
- **Decision 9: superseded.** On 2026-10-27 the owner chose the Apache License
  2.0 for this repository (`LICENSE`, `NOTICE`, README "Competition boundary").
  Hyphae keeps its own license and is not relicensed.
