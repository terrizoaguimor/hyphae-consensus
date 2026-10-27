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
