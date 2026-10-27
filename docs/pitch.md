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
   missing tests, and constraint conflicts, as individually tracked items.
3. **Evidence Evaluator** — judges, per claim, whether the retrieved records
   are relevant and whether they corroborate or contradict it.

The **runtime** retrieves the records from Hyphae Agent Memory and verifies
their sealed integrity proofs offline. That is not a model capability, and
integrity, factual corroboration, and relevance stay three independent axes,
because a valid proof attests integrity, never truth. A fixed runtime policy,
not a vote, turns the Evaluator's judgments and the proof statuses into the
recommendation (`supported / contested / insufficient_evidence / abstained /
blocked`). No model writes that field or can approve anything, but the
Evaluator's judgments are its main input, and the Critic counts through the
Evaluator's per-item verdict. Every step lands in a hash-chained, crash-safe,
append-only event log that replays deterministically and re-derives each
recommendation. Then the run stops and a human authorizes. Nothing executes
before that authorization; in this build the executor is deliberately absent,
so approval is an auditable record, not an action.

**Why it matters.** Disagreement becomes data. Abstention becomes a valid,
visible outcome instead of silent guessing. Corrupted evidence blocks
instead of being averaged away. And every decision ships with a redacted
run receipt: the recommendation and its unresolved assumptions, per-record
proof statuses, model identity (`muse-wonder-1.0`), request ids, latency,
tokens, the hash-chain head — and a plain statement of what the proofs do
*not* establish.

**What the live system does today** (`docs/evaluation.md`). Across 12 runs
with the real Meta API and real Hyphae memory: the Critic flagged the stored
retry constraint in 5 of 11 critiques. Two first proposals (a single Proposer
call) would have violated that constraint, and neither came out `supported`. A
planted "SYSTEM OVERRIDE" memory was treated as contradicting evidence, not
obeyed. Hyphae's project isolation held. Every run ended blocked (9),
abstained (1), or failed safely on a timeout (2): none was `supported`.
Upstream proof generation currently reports corruption, and the live
Evaluator abstained every time.

**Built for the hackathon** (Oct 26 – Nov 4, 2026): the deliberation
protocol, deterministic state machine, orthogonal evidence model,
hash-chained event log with crash recovery, Meta structured-output adapter,
Hyphae MCP read-only adapter, operator console, a 7-case scripted protocol
conformance check (7/7 author-defined outcomes; it is not a baseline
comparison), and the live battery above.

**Pre-existing and explicitly not claimed:** Hyphae Core and Hyphae Agent
Memory (memory engine, MCP tools, sealed proofs) — independent open-source
work pinned at commit `418a3da…` / release 4.0.0, consumed only through
its public interfaces.
