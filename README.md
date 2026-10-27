# Hyphae Consensus

**Verifiable Multi-Agent Reasoning and Decision Accountability**

Meta Global AI Developer Hackathon 2026 · Indie Developer track · Push the Frontier
Development window: October 26 – November 4, 2026 · Owner: Mario (independent developer)

Hyphae Consensus coordinates three Meta-AI agent roles — **Proposer**, **Critic**, and
**Evidence Evaluator** — that propose solutions to software-engineering problems, challenge
each other's assumptions, retrieve evidence from a verifiable memory backend, check
integrity proofs, revise conclusions, and present an auditable recommendation for explicit
human authorization. Models never decide outcomes and nothing is executed before a human
approves.

> **Status: M1–M3 + console complete (2026-10-26).** Typed contracts, deterministic
> state machine, crash-safe event log, Meta Model API adapter
> (structured output, tested against a local fake server), Hyphae MCP read-only adapter
> + offline proof verifier (tested against in-memory/scripted transports), Axum/SSE
> operator console, synthetic demo repository, and a 7-case baseline-vs-consensus
> protocol evaluation. **Live status:** Meta and Hyphae both verified end-to-end;
> Hyphae runs its pinned Linux build in a local Docker container because the
> Windows build's Agent Memory surface does not start on this host. Meta verified against the real
> API (one full console run; the live Evaluator abstained — see
> `docs/evaluation.md`). The Hyphae live probe passes against the container; one
> full Meta+Hyphae run retrieved real evidence and abstained. Upstream Windows
> 4.0.0 Agent Memory surface, which fails to start on this machine even via its
> official `agent setup` (evidence in `docs/journal.md`), and upstream
> `recall(prove=true)` currently fails while plain recall works, so integrity is
> reported `unavailable` live; the adapter is tested
> against scripted transports and the gated probe runs unchanged once upstream
> starts. Demo modes are labeled (`fake` vs live) everywhere.

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
  adversarial integration tests, and the baseline-vs-consensus evaluation
  (`docs/evaluation.md`).

**Licensing of this repository: undecided.** No license is granted by this repository
until the owner explicitly selects one. Hyphae is consumed only via its public
interfaces; no Hyphae code is redistributed here.

## What works when Hyphae is offline

The event log, state machine, orchestration, receipts, and human-authorization recording
are fully operational without Hyphae. When the memory backend is unreachable, evidence
retrieval fails per-claim, an `evidence_unavailable` event is recorded, affected evidence
carries `integrity_status: unavailable`, and the recommendation is capped at
`insufficient_evidence` or `abstained` — never `supported`. Uncertainty is surfaced,
never converted into confidence.

## Integrity proofs: what they do and do not establish

A valid Hyphae proof establishes that a recalled record matches its anchored snapshot
under Hyphae's documented assumptions. It does **not** establish that the content is
true, relevant, or complete, or that a model reasoned from it. This is why
`EvidenceVerification` keeps integrity, factual corroboration, and relevance as three
independent fields, and why a `supported` recommendation requires corroborated claims
*and* valid proofs on every cited record.

## Workspace layout

| Crate | Responsibility |
|---|---|
| `crates/consensus-domain` | Typed contracts only; no I/O |
| `crates/consensus-runtime` | State machine, event log, orchestration, policy, ports, fakes |
| `crates/consensus-api` | Operator entry point (M1: scripted demo binary) |
| `crates/consensus-tests` | Deterministic integration and adversarial scenarios |

Architecture decisions: `docs/adr/0001-m1-architecture.md`. Threat model:
`docs/threat-model.md`. Implementation journal: `docs/journal.md`.

## Setup and verification (M1)

Prerequisites: Rust 1.99.1 via [rustup](https://rust-lang.org/tools/install) with the
MSVC build tools on Windows (see the
[rustup book](https://rust-lang.github.io/rustup/installation/windows-msvc.html)).
The toolchain is pinned in `rust-toolchain.toml`.

```powershell
cargo test --workspace                                        # 42 tests, deterministic
cargo run -p consensus-domain --example write_schemas         # regenerate schemas/
cargo run -p consensus-tests --example evaluate               # baseline vs consensus battery
cargo run -p consensus-api -- serve                           # operator console → http://127.0.0.1:8787/
cargo run -p consensus-api                                    # scripted CLI demo (alternative)
cargo run -p consensus-api -- --approve                       # record approval (no execution occurs)
```

Live modes (console `model=meta` / `memory=hyphae`, and the gated tests) read
credentials from the environment — see `.env.example`. Without them the API
returns an explicit configuration error instead of silently falling back.

The demo persists events to `demo-run/events.jsonl` (git-ignored). Re-running replays
the log instead of calling models again; delete `demo-run/` for a fresh run.

No credentials are needed for M1. From M2 onward, copy `.env.example` to your
environment configuration; secrets must come from environment variables, never files
committed to this repository.

## Safety invariants (enforced in code, covered by tests)

- No self-approval: only a human actor can record an authorization decision.
- No execution before approval: the executor is deferred through M3; approval currently
  only records the decision.
- Bounded debate: at most 2 revision rounds, plus a no-progress exit.
- Replay never repeats external effects: replay is read-only state reconstruction.
- Retrieved memory is untrusted data, never instructions (prompt-injection scenario in
  `crates/consensus-tests/tests/m1_scenarios.rs`).
- Agents receive read-only memory access by contract; writes are a separate
  runtime-service trait (`MemoryAdmin`) that agent-facing code cannot reach.
