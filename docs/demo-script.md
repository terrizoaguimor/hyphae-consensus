# Demo script / video storyboard (target ≤ 3 minutes)

Setup before recording: `cargo run -p consensus-api -- serve`, open
http://127.0.0.1:8787/, and paste the operator token the server prints at
startup into the decision panel (approvals are refused without it). For
the live cut, set `META_API_KEY` (and, for the Hyphae segment, the
container environment in `scripts/hyphae-container/README.md`) in the
server environment before starting. Record the fake-mode cut first as the
guaranteed take, and label it on screen as scripted with simulated proofs.

Live expectation, stated before recording: with the pinned Hyphae build,
proof generation fails upstream with a corruption error, so a live
`meta + hyphae` run that retrieves evidence ends **Blocked**. That is the
honest live result. Segments 2–4 (revision → Supported) exist only in the
scripted cut; never present them as live.

| # | Time | Screen | Narration beats |
|---|---|---|---|
| 1 | 0:00–0:15 | Console home | "Agents shouldn't grade their own homework. Hyphae Consensus makes three Meta-model roles argue — under a runtime that trusts none of them." |
| 2 | 0:15–0:40 | New run (fake or meta), task = payment timeouts | Proposer claims appear: "raise retries to 10". Point at explicit claims/assumptions — no hidden reasoning. |
| 3 | 0:40–1:10 | Timeline: challenge + evaluation | Critic finds the stored constraint (cap = 3 with jitter). Evaluator marks the claim contradicted by retrieved evidence. Show the evidence table: integrity / factual / relevance are separate chips. |
| 4 | 1:10–1:30 | Revision + second evaluation | Proposer revises to the compliant fix. Recommendation flips to **Supported**: the runtime's fixed policy aggregates the Evaluator's per-claim judgments and the proof statuses; no model writes or votes on the outcome. |
| 5 | 1:30–1:55 | Authorization panel | State waits at AwaitingHumanApproval. Approve on camera; stress that approval is *recorded only* — no code changes, no execution. Download the run receipt; zoom into model identity (`muse-wonder-1.0`), proof statuses, and the limitations line. |
| 6 | 1:55–2:25 | Failure gallery (scenario selector, three quick runs) | Memory offline → InsufficientEvidence. Invalid proof → Blocked (approving it now demands an override note). Hostile memory → not supported, still pending; say it is a *policy* demo, because the scripted model never reads the text. Then cut to the live hostile-memory run from `docs/evaluation.md`. "Uncertainty is surfaced, never averaged into confidence." |
| 7 | 2:25–2:45 | Terminal: `cargo test` + evaluate table + live table | 67 tests; scripted conformance 7/7 (author-defined outcomes, not a baseline: say so); then the live Meta + Hyphae battery table exactly as recorded. |
| 8 | 2:45–3:00 | README boundary section | Close on honesty: Hyphae is pre-existing and pinned; the consensus layer is the new work; proofs attest integrity, not truth. |

Reproducibility: every screen above is produced by committed code and
fixtures (`demo-repo/`, `crates/consensus-tests/examples/evaluate.rs`).
Delete `demo-run/` for a clean take.
