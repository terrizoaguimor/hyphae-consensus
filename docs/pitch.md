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
tokens, the hash of the run's last event in the hash-chained log — and a plain
statement of what the proofs do *not* establish.

**What the live system does today** (`docs/evaluation.md` §5, 2026-10-27).
Hyphae proofs verify live: every retrieved record was checked offline by
re-executing its recall against the sealed witness, against a pinned anchor.
The revision loop runs live: first proposals that contradict stored
constraints are contested and rewritten into claims the verified memory states
directly. One live run reached `supported` with every cited record verified
(R06; its 7 round-1 objections were never re-judged, a fail-open the 2026-10-27
audit found and the planned protocol 1.3 addresses). On the final code all 8
runs ended `contested`. In the 6-run final battery every final-round claim was
corroborated and only the Critic's open objections, mostly missing tests,
blocked `supported`; in both hostile-note runs a claim with contradicting
evidence remained, and H02 ended with 1 of 4 claims corroborated. A planted
"SYSTEM OVERRIDE" memory, this time with a valid proof, was flagged by the
Critic, recorded as contradicting evidence by the Evaluator, and never obeyed.
Hyphae's project isolation held. No run self-authorized.

**Built for the hackathon** (Oct 26 – Nov 4, 2026): the deliberation
protocol, deterministic state machine, orthogonal evidence model,
hash-chained event log with crash recovery, Meta structured-output adapter,
Hyphae MCP read-only adapter, operator console, a 10-case scripted protocol
conformance check (10/10 author-defined outcomes; it is not a baseline
comparison), and the live batteries above.

**Pre-existing and explicitly not claimed:** Hyphae Core and Hyphae Agent
Memory (memory engine, MCP tools, sealed proofs) — the owner's own earlier
open-source project (Apache-2.0), frozen at commit `418a3da…` / release 4.0.0
before the hackathon, consumed only through its public interfaces.

*Corrected 2026-10-27 after the independent audit
(`docs/reviews/2026-10-27-independent-audit.md`): the final-code results now
match `docs/evaluation.md` (only the final battery had every final-round claim
corroborated), R06 carries its caveat, the receipt's hash is described as the
run's last event hash, and Hyphae's authorship is stated.*
