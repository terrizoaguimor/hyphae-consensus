# Demo script / video storyboard (target ≤ 3 minutes)

Setup before recording: `cargo run -p consensus-api -- serve`, open
http://127.0.0.1:8787/. For the live cut, set `META_API_KEY` (and, for the
Hyphae segment, an isolated Hyphae profile) in the server environment
before starting. Record the fake-mode cut first as the guaranteed-take;
swap in live footage per segment when credentials are in place.

| # | Time | Screen | Narration beats |
|---|---|---|---|
| 1 | 0:00–0:15 | Console home | "Agents shouldn't grade their own homework. Hyphae Consensus makes three Meta-model roles argue — under a runtime that trusts none of them." |
| 2 | 0:15–0:40 | New run (fake or meta), task = payment timeouts | Proposer claims appear: "raise retries to 10". Point at explicit claims/assumptions — no hidden reasoning. |
| 3 | 0:40–1:10 | Timeline: challenge + evaluation | Critic finds the stored constraint (cap = 3 with jitter). Evaluator marks the claim contradicted by retrieved evidence. Show the evidence table: integrity / factual / relevance are separate chips. |
| 4 | 1:10–1:30 | Revision + second evaluation | Proposer revises to the compliant fix. Recommendation flips to **Supported** — derived by the runtime, not voted by models. |
| 5 | 1:30–1:55 | Authorization panel | State waits at AwaitingHumanApproval. Approve on camera; stress that approval is *recorded only* — no code changes, no execution. Download the run receipt; zoom into model identity (`muse-wonder-1.0`), proof statuses, and the limitations line. |
| 6 | 1:55–2:25 | Failure gallery (three quick runs) | Memory offline → InsufficientEvidence. Invalid proof → Blocked. Prompt-injection memory → not supported, still pending. "Uncertainty is surfaced, never averaged into confidence." |
| 7 | 2:25–2:45 | Terminal: `cargo test` + evaluate table | 42 tests; baseline unsafe 6/7 vs consensus safe 7/7 (deterministic doubles — say so). |
| 8 | 2:45–3:00 | README boundary section | Close on honesty: Hyphae is pre-existing and pinned; the consensus layer is the new work; proofs attest integrity, not truth. |

Reproducibility: every screen above is produced by committed code and
fixtures (`demo-repo/`, `crates/consensus-tests/examples/evaluate.rs`).
Delete `demo-run/` for a clean take.
