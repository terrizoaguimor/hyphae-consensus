# Demo script / video storyboard (target ≤ 3 minutes)

Setup before recording: `cargo run -p consensus-api -- serve`, open
http://127.0.0.1:8787/, and paste the operator token the server prints at
startup into the decision panel (approvals are refused without it). For
the live cut, set `META_API_KEY` (and, for the Hyphae segment, the
container environment in `scripts/hyphae-container/README.md`) in the
server environment before starting. Record the fake-mode cut first as the
guaranteed take, and label it on screen as scripted with simulated proofs.

Live expectation, stated before recording (2026-10-27): Hyphae proofs verify
live, and the revision loop runs live. With the final code every live
`meta + hyphae` run in the last battery ended **Contested**: the revised claims
were corroborated by verified memory, but the Critic's objections (mostly
missing tests) stayed open. One earlier live run (R06 in `docs/evaluation.md`
§5.2) reached **Supported**; show its receipt, not a re-run, if you claim it.
Segment 4's flip to Supported is guaranteed only in the scripted cut; never
present a scripted run as live.

| # | Time | Screen | Narration beats |
|---|---|---|---|
| 1 | 0:00–0:15 | Console home | "Agents shouldn't grade their own homework. Hyphae Consensus makes three Meta-model roles argue — under a runtime that trusts none of them." |
| 2 | 0:15–0:40 | New run (fake or meta), task = payment timeouts | Proposer claims appear: "raise retries to 10". Point at explicit claims/assumptions — no hidden reasoning. |
| 3 | 0:40–1:10 | Timeline: challenge + evaluation | Critic finds the stored constraint (cap = 3 with jitter). Evaluator marks the claim contradicted by retrieved evidence. Show the evidence table: integrity / factual / relevance are separate chips. |
| 4 | 1:10–1:30 | Revision + second evaluation | Proposer revises to the compliant fix. Recommendation flips to **Supported**: the runtime's fixed policy aggregates the Evaluator's per-claim judgments and the proof statuses; no model writes or votes on the outcome. |
| 5 | 1:30–1:55 | Authorization panel | State waits at AwaitingHumanApproval. Approve on camera; stress that approval is *recorded only* — no code changes, no execution. Download the run receipt; zoom into model identity (`muse-wonder-1.0`), proof statuses, and the limitations line. |
| 6 | 1:55–2:25 | Failure gallery (scenario selector, three quick runs) | Memory offline → InsufficientEvidence. Invalid proof → Blocked (approving it now demands an override note). Hostile memory → not supported, still pending; say it is a *policy* demo, because the scripted model never reads the text. Then cut to the live hostile-memory run from `docs/evaluation.md`. "Uncertainty is surfaced, never averaged into confidence." |
| 7 | 2:25–2:45 | Terminal: `cargo test` + evaluate table + live table | 89 tests; scripted conformance 10/10 (author-defined outcomes, not a baseline: say so); then the live Meta + Hyphae battery tables exactly as recorded, including the contested final battery. |
| 8 | 2:45–3:00 | README boundary section | Close on honesty: Hyphae is pre-existing and pinned; the consensus layer is the new work; proofs attest integrity, not truth. |

Reproducibility: every screen above is produced by committed code and
fixtures (`demo-repo/`, `crates/consensus-tests/examples/evaluate.rs`).
Delete `demo-run/` for a clean take.
