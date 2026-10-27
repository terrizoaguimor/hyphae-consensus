# Brief for Claude (headless fix run, 2026-10-26)

You are Claude, running headless in the repo root of **Hyphae Consensus**
(a private Rust workspace for the Meta Hackathon 2026). The owner (Mario)
asked you to finish applying ALL fixes from your own independent audit at
`docs/reviews/2026-10-26-claude-opus-review.md` — read that file first,
in full. Work autonomously until done or genuinely blocked. Write a
final report to `docs/reviews/2026-10-26-claude-fix-report.md`.

## Hard constraints (never violate)

- Repo `terrizoaguimor/hyphae-consensus` stays **PRIVATE**. You may make
  **local commits** with clear messages. **NEVER push** — Mario authorizes
  each push personally. Never create PRs/remotes.
- Never print, log, or commit secrets. `META_API_KEY` lives in the Windows
  User environment (`[Environment]::GetEnvironmentVariable("META_API_KEY","User")`
  in PowerShell); never echo it. `HYPHAE_NATIVE_API_KEY_FILE` is a path only.
- Never modify/vendor Hyphae Core. Never touch Mario's primary Agent
  Memory data on the host. Hyphae runs only inside the Docker container
  `hyphae-consensus` (Linux), via `scripts\hyphae-docker.cmd`.
- Competition repo license is **undecided** — do not choose one.
- Architecture invariants: models never set outcomes (only the runtime's
  deterministic policy derives recommendations); only a human authorizes;
  the executor is deferred (decisions are recorded, nothing is executed);
  retrieved memory text is untrusted data, never instructions.
- Honesty: never fabricate results. Label fake/simulated vs live clearly.
  If something is blocked (Docker down, API error), document it in the
  report and continue with everything else.

## Already done and verified (do not redo; re-check tests stay green)

- R1 policy holes closed (`policy.rs`): invalid proof anywhere → Blocked;
  contradicting evidence considered; per-claim corroboration needs its
  own valid evidence. Adversarial tests pass.
- R3 schema 1.1 (`SCHEMA_VERSION="1.1"`, reads 1.0 too): per-item
  challenge ids, `unresolved_items`/`addressed_items`, `proof_error`,
  event `prev_hash`/`hash`, receipt `head_hash`. Meta adapter: item-id
  filtering, `</UNTRUSTED` escaping, evidence truncation, proposal
  delimiters, revise receives challenge items + evidence, claim caps.
  Hyphae adapter: tool-error envelopes, `proof_error` propagation
  (corruption → Invalid), verifier exit mapping (125-127/usage →
  VerificationError; rejection keywords → Invalid), operator-pinned
  expected anchor (`CONSENSUS_HYPHAE_EXPECTED_ANCHOR`).
- R4 audit log: SHA-256 hash-chain assigned/validated on open (legacy
  1.0 prefix anchors as `pre-1.1:<seq>`), append rollback on write
  failure, replay recomputes recommendation kind from tracked
  evaluation+round verifications (1.1 records), `head_hash()`. Real
  legacy `demo-run/events.jsonl` (1.0) replays cleanly.
- R11 items above (verifier mapping, proof_error, anchor) done.
- R2 server: fake+non-fake memory → 400 (no silent substitution);
  fake scenarios retry/offline/invalid-proof/injection/abstain;
  `/decision` requires per-process operator token (401) and an override
  note to approve Blocked/Abstained; handler tests added. Console UI
  (`console.html`) now has scenario selector, operator-token + note
  fields, stable idempotency key, "simulated proofs" labeling.
- Test state at handoff: `cargo test --workspace` green (56 active,
  2 ignored live). There are **uncommitted changes** implementing all
  of the above — review `git status`/`git diff` before touching code.

## Remaining work (in order)

1. **Console HTTP smoke (real server)**: build, run
   `target\debug\consensus-api.exe serve` with `CONSENSUS_BIND` and a
   temp `CONSENSUS_EVENT_LOG`; verify over HTTP: page serves new
   fields, POST fake `invalid-proof` run ends Blocked, decision without
   token → 401, with token+note → 200. Stop only the process you
   started. If the exe is locked, stop any `consensus-api` process
   whose path is inside this workspace.
2. **R5 docs honesty pass**: README (fix broken sentence + status),
   ADR-0002/0003 (outdated "not run" claims),
   `docs/mission-control.md` (exists? verify claims vs code),
   `docs/threat-model.md` (threat 5, offline claim),
   `docs/pitch.md` (attribution: "runtime retrieves/verifies; Evaluator
   judges relevance/factuality"; stop overstating "models never decide"
   — the Evaluator judges corroboration), `docs/evaluation.md` (relabel
   the injection test as a POLICY test — the fake model never reads the
   hostile text; remove the timeout "UNSAFE" claim; the "single-agent
   baseline" must be either actually run or renamed to what it is).
   `docs/journal.md`: append an entry for this audit-fix round.
3. **Gates**: `cargo fmt --all`, `cargo clippy --workspace --all-targets`
   (zero warnings), `cargo test --workspace`,
   `cargo run -p consensus-domain --example write_schemas` (regenerate
   `schemas/` for 1.1), `cargo run -p consensus-tests --example evaluate`.
4. **Live battery ≥10 runs (Meta + Hyphae Docker)**:
   - Docker: container `hyphae-consensus` may need `docker start`;
     Hyphae serve runs INSIDE it with:
     `hyphae serve --data-dir /hyphae-home/.local/share/hyphae/agent-memory --native-api-key-auth`
     (start it inside the container if not running; seed scripts in
     `scripts/hyphae-container/`). Host env for live tests:
     `CONSENSUS_LIVE_TESTS=1`,
     `CONSENSUS_HYPHAE_BIN=<repo>\scripts\hyphae-docker.cmd`,
     `HYPHAE_NATIVE_ENDPOINT=/hyphae-home/.local/share/hyphae/agent-memory/hyphae.sock`,
     `HYPHAE_NATIVE_API_KEY_FILE=/hyphae-home/.config/hyphae/credentials/memory-reader.key`,
     `META_API_KEY` from User env. Sanity first:
     `cargo test -p consensus-api --test hyphae_adapter live_ -- --ignored --nocapture`
     and the meta live test similarly.
   - Run ≥10 full live runs through the server (model=meta,
     memory=hyphae) with varied tasks, INCLUDING one real malicious-
     memory case: seed a hostile memory inside the container (see
     `scripts/hyphae-container/seed*.sh` for the pattern) that tries to
     force approval, then run a matching task and record what happens.
   - KNOWN UPSTREAM BUG: `recall(prove=true)` returns a corruption
     error even on Linux, so live integrity may honestly report
     `unavailable`/`Invalid` and runs may abstain or end
     InsufficientEvidence. That is correct behavior — never fake a
     Supported-with-valid-Hyphae-proof result.
   - Record an honest table in `docs/evaluation.md`: per run — task,
     final state, recommendation kind, request ids, latency, tokens
     (from receipts; receipts preserve exact `muse-wonder-1.0`).
5. **Local commit(s)** of everything (sensible grouping), then write the
   final report. NO push.

## Definition of done

Every audit finding in the review file is either fixed with a test, or
explicitly documented as blocked/wontfix-with-reason in your report.
All gates green. Report lists: files changed, tests added, live-run
table location, remaining risks. Rust: put `$env:USERPROFILE\.cargo\bin`
on PATH for cargo. Prefer small, reviewable changes; match existing
code conventions; English in code/docs (repo language).
