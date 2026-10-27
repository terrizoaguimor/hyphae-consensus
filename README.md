# Hyphae Consensus

**Verifiable Multi-Agent Reasoning and Decision Accountability**

Meta Global AI Developer Hackathon 2026 · Indie Developer track · Push the Frontier
Development window: October 26 – November 4, 2026 · Owner: Mario (independent developer)

Hyphae Consensus coordinates three Meta-AI agent roles — **Proposer**, **Critic**, and
**Evidence Evaluator** — that propose solutions to software-engineering problems,
challenge each other's assumptions, and revise conclusions. The **runtime** (not a
model) retrieves evidence from a verifiable memory backend and checks its integrity
proofs; the **Evaluator** judges relevance and factual corroboration per claim; a fixed,
deterministic runtime policy turns those judgments and the proof statuses into an
auditable recommendation; and only a **human** authorizes. No model writes the outcome
or can approve, but the Evaluator's judgments are the policy's main input, so "models
never decide" holds in form, not in substance. Nothing is executed before a human
approves (the executor is deferred: approval records a decision only).

> **Status (2026-10-27, owner-decision round; schema 1.2).**
> Typed contracts, deterministic state machine, hash-chained crash-safe event log,
> Meta Model API adapter (strict structured output), Hyphae MCP read-only adapter +
> offline proof verifier, Axum/SSE operator console, synthetic demo repository, a
> 10-case scripted protocol conformance check, and live batteries. **Live status:**
> Hyphae proofs now verify end to end. `hyphae proof verify` re-executes each recall
> against its sealed witness, checked against an operator-pinned anchor. The earlier
> "corruption" answers were never data corruption: the daemon's socket sat inside
> its data directory, and later the store outgrew the 4 MiB MCP response that
> carries the witness (ADR-0003). Across 18 live runs on 2026-10-27 (real Meta API,
> `muse-wonder-1.0`, and real Hyphae memory), one reached `supported` with every
> cited record verified (R06). Two defects in this round's own changes surfaced
> live and were fixed. On the final code all 8 runs ended `contested`: every
> final-round claim was corroborated by verified memory, but the Critic's
> objections about missing tests and unproven risks stay open, and the unchanged
> policy grants `supported` only when none is open. A hostile note with a valid
> proof was treated as contradicting data, never as instructions
> (`docs/evaluation.md` §5). Hyphae runs
> its pinned Linux build in a local Docker container, because the Windows build's
> Agent Memory surface does not start on this host (`docs/journal.md`). Demo modes
> are labeled everywhere (`fake` with simulated proofs vs. live).

## Competition boundary: pre-existing vs. newly developed work

This repository is an **independent** project created for the hackathon. It consumes
pre-existing open-source technology through documented public interfaces only.

**Pre-existing (NOT built for this hackathon, NOT claimed as new work):**

| Component | Pinned revision | Evidence |
|---|---|---|
| Hyphae Core + Hyphae Agent Memory | Commit `418a3da717c2f9ba0cf095423f392b3d496cb607` (2026-09-24), release Hyphae 4.0.0, tag `release-v4.0.0-crates` (2026-09-23) | [github.com/Hyphae-Research-Foundation/hyphae](https://github.com/Hyphae-Research-Foundation/hyphae) |
| Hyphae root `LICENSE` at that commit | Apache License 2.0 (full text fetched and read 2026-10-26) | `docs/product/agent-memory.md` carries `SPDX-License-Identifier: CC-BY-SA-4.0` |

Hyphae already provides: local shared agent memory, the MCP tools
`hyphae_memory_store / journal / recall / forget / status`, permission profiles
(`memory-reader`, `memory-writer`, `auditor`, `operator`), per-project memory isolation,
TTL/expiry, backup/restore, and sealed recall proofs verifiable offline with
`hyphae proof verify`. None of Hyphae's code, internals, or history is vendored, copied,
forked, or modified by this project.

**Newly developed for this hackathon (this repository):**

- The three-role deliberation protocol and deterministic consensus state machine.
- Typed, versioned contracts for proposals, claims, challenges, evaluations,
  recommendations, human authorizations, events, and run receipts (`crates/consensus-domain`,
  JSON Schemas in `schemas/`).
- Orthogonal evidence status modeling (`integrity_status` / `factual_status` /
  `relevance_status`) and the recommendation-derivation policy.
- The append-only, crash-safe, replayable application event log.
- Meta Model API adapter (`muse-wonder-1.0`, Chat Completions structured output per
  Meta's docs) with timeouts, bounded retries, and strict server-side validation
  (`crates/consensus-api/src/meta.rs`, ADR-0002).
- Hyphae MCP adapter over `hyphae mcp --profile memory` (stdio JSON-RPC, read-only
  tools only) plus `hyphae proof verify` integrity checking
  (`crates/consensus-api/src/hyphae.rs`, ADR-0003).
- Operator console (Axum + SSE replay), synthetic demo repository (`demo-repo/`),
  adversarial integration tests, a scripted protocol conformance check, and a live
  Meta + Hyphae battery (`docs/evaluation.md`).

**Licensing of this repository: undecided.** No license is granted by this repository
until the owner explicitly selects one. Hyphae is consumed only via its public
interfaces; no Hyphae code is redistributed here.

## What works when Hyphae is offline

The event log, state machine, orchestration, receipts, and human-authorization recording
are fully operational without Hyphae. When the memory backend is unreachable, evidence
retrieval fails per claim and an `evidence_unavailable` event records the reason. No
evidence is retrieved, so no verification record exists at all, and with no checked
evidence the policy can never return `supported`: the run ends `insufficient_evidence`,
`contested` (unresolved Critic items), or `abstained`, depending on the Evaluator.
Uncertainty is surfaced, never converted into confidence.

## Integrity proofs: what they do and do not establish

A valid Hyphae proof establishes that a recalled record matches its anchored snapshot
under Hyphae's documented assumptions. It does **not** establish that the content is
true, relevant, or complete, or that a model reasoned from it. This is why
`EvidenceVerification` keeps integrity, factual corroboration, and relevance as three
independent fields. A `supported` recommendation requires **every** claim to be
corroborated by at least one record that the claim itself cites, a valid proof on
every cited record, no contradicting record (cited or not), no Critic item the
Evaluator still judges unresolved, and no invalid proof anywhere in the retrieved
set. A recorded contradiction outranks an abstention (`contested`, schema 1.2), so
abstaining cannot hide it. `Valid` means `hyphae proof verify` re-executed the
recall against its sealed witness and reproduced the result; one proof covers a
whole recall. By default the proof anchor comes from the same server response as
the proof, which shows internal consistency only; pin an out-of-band anchor with
`CONSENSUS_HYPHAE_EXPECTED_ANCHOR` to verify against a trusted one.

## Workspace layout

| Crate | Responsibility |
|---|---|
| `crates/consensus-domain` | Typed contracts only; no I/O |
| `crates/consensus-runtime` | State machine, event log, orchestration, policy, ports, fakes |
| `crates/consensus-api` | Meta + Hyphae adapters, Axum/SSE operator console, scripted CLI demo |
| `crates/consensus-tests` | Deterministic integration and adversarial scenarios |

Architecture decisions: `docs/adr/0001-m1-architecture.md`. Threat model:
`docs/threat-model.md`. Implementation journal: `docs/journal.md`.

## Setup and verification (M1)

Prerequisites: Rust 1.99.1 via [rustup](https://rust-lang.org/tools/install) with the
MSVC build tools on Windows (see the
[rustup book](https://rust-lang.github.io/rustup/installation/windows-msvc.html)).
The toolchain is pinned in `rust-toolchain.toml`.

```powershell
cargo test --workspace                                        # 89 tests (+2 ignored live), deterministic
cargo run -p consensus-domain --example write_schemas         # regenerate schemas/ (1.2)
cargo run -p consensus-tests --example evaluate               # scripted protocol conformance check
cargo run -p consensus-api -- serve                           # operator console → http://127.0.0.1:8787/
cargo run -p consensus-api                                    # scripted CLI demo (alternative)
cargo run -p consensus-api -- --approve                       # record approval (no execution occurs)
```

The console prints a per-process **operator token** at startup; the decision panel
needs it to record an approval or rejection, and approving a `blocked` or `abstained`
run additionally requires a note. Live modes (console `model=meta` / `memory=hyphae`,
and the gated tests) read credentials from the environment — see `.env.example` and
`scripts/hyphae-container/README.md`. Without them the API returns an explicit
configuration error instead of silently falling back; `model=fake` with a non-fake
memory is rejected rather than ignored.

The demo persists events to `demo-run/events.jsonl` (git-ignored). Re-running replays
the log instead of calling models again; delete `demo-run/` for a fresh run.

No credentials are needed for M1. From M2 onward, copy `.env.example` to your
environment configuration; secrets must come from environment variables, never files
committed to this repository.

## Safety invariants (enforced in code, covered by tests)

- No self-approval: only a human actor can record an authorization decision, and the
  console requires the operator token printed at startup (a typed name is a label, not
  authentication).
- No execution before approval: the executor is deferred through M3; approval currently
  only records the decision.
- Bounded debate and cost: at most 2 revision rounds plus a no-progress exit measured
  per Critic item (a revision may also follow an abstention that still lists
  unresolved items); at most 9 model calls, 20 distinct evidence records, and
  `CONSENSUS_MAX_RUN_TOKENS` (default 150,000) input + output tokens per run, enforced
  by the runtime; at most 12 claims per proposal from the Meta adapter. A run that
  crosses the token cap fails with stage `token_budget`.
- Tamper-evident history: events are SHA-256 hash-chained and verified on open; replay
  recomputes every recommendation and rejects a forged one; receipts carry the chain
  head. The chain is unsigned, so it is evident, not proof, against a full rewrite.
- Replay never repeats external effects: replay is read-only state reconstruction.
- Retrieved memory and other models' outputs are untrusted data, never instructions:
  they are delimited, escaped, and truncated in prompts, and no model output can
  authorize. That is a mitigation, not a guarantee. The scripted injection test is a
  *policy* test; prompt-level behavior was observed live (`docs/evaluation.md`).
- Agents receive read-only memory access by contract; writes are a separate
  runtime-service trait (`MemoryAdmin`) that agent-facing code cannot reach.
