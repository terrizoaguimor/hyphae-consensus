# Hyphae Consensus — technical pitch

**Problem.** AI coding agents act on their own conclusions. A single model
can propose a plausible fix that violates a constraint recorded months ago,
and nothing in the loop forces it to check — or lets a human audit why it
decided what it decided.

**Approach.** Three Meta-model roles deliberate under a runtime that does
not trust any of them:

1. **Proposer** — emits explicit, checkable claims, assumptions, and
   uncertainties (schema-constrained structured output, `muse-wonder-1.0`).
2. **Critic** — independently hunts contradictions, unsupported claims,
   missing tests, and constraint conflicts.
3. **Evidence Evaluator** — retrieves records from Hyphae Agent Memory,
   verifies sealed integrity proofs offline, and judges corroboration and
   relevance — three independent axes, because a valid proof attests
   integrity, never truth.

The runtime (not a model) derives the recommendation —
`supported / contested / insufficient_evidence / abstained / blocked` —
from evaluated evidence, records every step in a crash-safe append-only
event log that replays deterministically, and stops. A human authorizes.
Nothing executes before that authorization; in this build the executor is
deliberately absent, so approval is an auditable record, not an action.

**Why it matters.** Disagreement becomes data. Abstention becomes a valid,
visible outcome instead of silent guessing. Corrupted evidence blocks
instead of being averaged away. And every decision ships with a redacted
run receipt: claims, proof statuses, model identity (`muse-wonder-1.0`),
latency, tokens, unresolved assumptions — and a plain statement of what
the proofs do *not* establish.

**Built for the hackathon** (Oct 26 – Nov 4, 2026): the deliberation
protocol, deterministic state machine, orthogonal evidence model,
event log with crash recovery, Meta structured-output adapter, Hyphae MCP
read-only adapter, operator console, and a 7-case protocol evaluation
(baseline unsafe on 6/7; consensus safe on 7/7, deterministic doubles).

**Pre-existing and explicitly not claimed:** Hyphae Core and Hyphae Agent
Memory (memory engine, MCP tools, sealed proofs) — independent open-source
work pinned at commit `418a3da…` / release 4.0.0, consumed only through
its public interfaces.
