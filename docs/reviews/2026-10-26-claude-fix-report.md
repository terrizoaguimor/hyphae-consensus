# Fix report — headless audit-fix run (2026-10-26/27)

Input: `docs/reviews/2026-10-26-claude-fix-brief.md`, which implements
`docs/reviews/2026-10-26-claude-opus-review.md`. The run's prompt arrived as the
single word "Read"; this brief, written minutes before launch for this exact run,
was taken as the task.

## Outcome

- **All 14 audit findings are closed or explicitly documented** (table below).
  Every code fix has a test; design-level findings are documented with reasons.
- **The earlier "done" list held up, with two defects found while verifying it.**
  Both would have hit live runs, and both are fixed with regression tests (see
  "New defects"). In short: (1) the live Hyphae corruption signal was still being
  classified as `unavailable`; (2) an abstaining Evaluator plus an invalid proof
  wrote a record that failed replay, after which the console refused to start.
- **Gates green:** `cargo fmt --all --check` clean;
  `cargo clippy --workspace --all-targets` 0 warnings; `cargo test --workspace`
  **67 passed + 2 ignored live** (56 at handoff); schemas regenerated for 1.1
  (reproducible); `cargo run -p consensus-tests --example evaluate` 7/7.
- **Console HTTP smoke against the real binary:** passed (details below).
- **Live battery:** 12 Meta + Hyphae runs through the real console, including a
  real hostile-memory case, plus 3 clearly labeled simulated-proof runs. The table
  is in `docs/evaluation.md` §2–§3. Result: 9 blocked, 1 abstained, 2 failed
  safely, 0 supported.
- **Local commits only, no push:** `4faa6af` (code + schemas), `161196d` (Hyphae
  container tooling), `89b03ca` (docs), plus the commit that adds this report.

## Finding-by-finding status

"Prior" = implemented in the uncommitted work handed to this run and verified
here. "This run" = added in this run.

| # | Finding | Status | Evidence |
|---|---|---|---|
| 1 | Policy ignored uncited contradictions; an uncited invalid proof did not block | **Fixed** (prior) | `contradicting_evidence_prevents_supported_even_if_not_relevant`, `invalid_proof_anywhere_in_retrieved_evidence_blocks` |
| 2 | A corroborated claim with no evidence of its own could ride on another claim | **Fixed** (prior); README wording corrected (this run) | `corroborated_claim_without_own_evidence_is_not_supported` |
| 3 | "Models never decide" true only in form; the Critic has no mechanical effect | **Documented; design kept.** README, pitch, ADR-0001 amendment, and threat model now say the Evaluator's judgments drive the policy and the Critic counts through the Evaluator's per-item verdict. Changing who decides is a design decision for the owner. | docs only |
| 4a | Injection test is theater (fake model never reads the text) | **Fixed**: relabeled `policy_never_authorizes_from_memory_content` (prior) and documented as a policy test everywhere (this run) | `docs/evaluation.md` §1, threat model #2 |
| 4b | `</UNTRUSTED_EVIDENCE>` breakout | **Fixed** (prior); **tested** (this run) | `untrusted_text_cannot_close_its_delimiter_block` |
| 4c | Proposer output undelimited in Critic/Evaluator prompts | **Fixed** (prior); **tested** (this run) | same test (asserts `<UNTRUSTED_PROPOSAL>` escaping) |
| 4d | A valid proof does not protect against injection | **Documented residual risk** (threat model #2). Live hostile-memory runs I01/I02 recorded. The valid-proof variant is **untestable live** while upstream proofs fail. | `docs/evaluation.md` §2 |
| 5a | `model=fake` silently ignored the memory selector | **Fixed** (prior) | `fake_model_with_real_memory_is_rejected_not_substituted`; HTTP smoke |
| 5b | `meta+fake` used simulated Valid proofs without a receipt label | **Fixed**: UI label (prior); every fake-verifier record now says `SIMULATED proof` (this run) | `every_console_scenario_reaches_its_documented_outcome`; smoke receipt check |
| 5c | Failure gallery not reproducible from the console | **Fixed** (prior); **all scenarios tested** (this run) | `every_console_scenario_reaches_its_documented_outcome`, `invalid_proof_scenario_ends_blocked`; HTTP smoke |
| 6 | Second revision unreachable live; no round-limit test | **Fixed**: per-item challenge ids (prior) | `two_revision_rounds_are_reachable_and_bounded`, `revision_beyond_round_limit_is_rejected`. Live: no run reached a revision because the Evaluator abstained (documented) |
| 7 | Revision blind to Critic items and evidence | **Fixed** (prior); **tested** (this run) | `revise_sees_critic_items_and_evidence_and_filters_item_ids` |
| 8a | Log not tamper-evident; replay trusted the stored kind | **Fixed** (prior) | `tampered_record_breaks_hash_chain`, `legacy_1_0_prefix_opens_and_chains_to_new_records`, `replay_recomputes_recommendation_and_rejects_forged_kind`; the smoke test refused a hand-edited log. **Limit:** unsigned chain (documented) |
| 8b | Retrieval attributed to the Evaluator | **Fixed** (prior); **asserted** (this run) | `evidence_cap_is_strict_recorded_and_retrieval_is_runtime_attributed`; smoke (`actor: runtime`) |
| 9a | Orphaned runs after a crash | **Fixed** (prior); **tested** (this run); recovery no longer aborts on an unreplayable run (this run) | `startup_recovery_closes_crashed_runs_and_reports_broken_ones` |
| 9b | A partial write corrupts the whole log | **Fixed** (prior: rollback to the previous length). **Not fault-injection tested**: there is no seam to make `write_all` fail mid-record. Documented as residual. | code review only |
| 9c | Console silently dropped bad lines and transition errors | **Fixed**: transition errors surfaced (prior); corrupt lines now reported as 500 / SSE `replay_error` (this run) | `corrupt_log_line_is_reported_never_skipped` |
| 10a | Approval authenticated by a free-text name | **Fixed**: per-process operator token (prior) | `decision_requires_operator_token_and_override_note`; HTTP smoke (401) |
| 10b | Blocked approvable without an override note | **Fixed** (prior); check moved to the owning log so a failed file read cannot skip it (this run) | same test; HTTP smoke (400 → 200) |
| 10c | Idempotency key global, not per run | **Fixed** (prior); **tested** (this run) | `reusing_an_idempotency_key_across_runs_records_both_decisions` |
| 10d | UI sent a fresh UUID per click | **Fixed** (prior) | served page checked in the smoke test |
| 11a | Anchor taken from the same untrusted response | **Mitigated**: optional pinned anchor (prior); default trust stated in docs (this run) | `pinned_anchor_mismatch_is_invalid_without_running_verifier`; ADR-0003 §6 |
| 11b | Unproven retry hid the corruption signal | **Fixed this run.** Prior work added `proof_error`, but the live envelope carries "corruption" only in `error.category`, which the adapter dropped, so live records still came out `unavailable`. The category is now kept. | `recall_falls_back_to_unproven_when_proof_generation_fails` (extended, live envelope), `upstream_corruption_signal_blocks_and_is_never_downgraded`, `non_corruption_proof_failure_is_unavailable_not_supported`; confirmed live |
| 11c | Any nonzero verifier exit mapped to Invalid | **Fixed** (prior) | `transport_exit_codes_are_verification_errors_not_invalid` |
| 11d | Live Supported impossible (proofs fail) | **Blocked upstream.** Documented in README, ADR-0003, evaluation, and demo script | live battery |
| 12 | Baseline never executed; circular 6/7 vs 7/7; timeout counted as unsafe | **Fixed** (this run): `evaluate.rs` is now a labeled conformance check with no baseline claim and no "UNSAFE" timeout; docs rewritten. A real executed comparison exists only as a small live sample: first proposals vs. outcomes, manually read and labeled. | `docs/evaluation.md` §1–§2 |
| 13 | Outdated docs and overclaims | **Fixed** (this run): ADR-0002/0003 "not run", mission-control "UI not tested" and "hyphae blocked", the broken README sentence, `strict: false`, and the offline claim. **License: not chosen**, per the hard constraint. Owner should check the hackathon rules (a public OSI license may be required). | docs |
| 14 | Unbounded memory/verification cost | **Fixed**: claim cap 12 (prior; tested this run), evidence truncation (prior; tested this run), runtime evidence cap made strict and recorded (this run). **Not done:** per-run token budget (documented residual). | `proposal_claim_count_is_bounded`, `untrusted_text_cannot_close_its_delimiter_block`, `evidence_cap_is_strict_recorded_and_retrieval_is_runtime_attributed` |

## New defects found while verifying (fixed this run)

1. **Live corruption signal still downgraded.** See 11b. Without this fix, all
   12 live runs would have reported integrity `unavailable` instead of honoring
   the backend's corruption report.
2. **Poisoned log on abstain + invalid proof.** The policy ranks an invalid proof
   above abstention, but the state machine rejected `blocked` in the Abstained
   state (`AbstainKindMismatch`). `emit` appended before validating, so the log
   kept a record that failed replay; at the next start, `mark_interrupted_runs`
   aborted, and the console refused to start. Reproduced first, then fixed in
   three layers:
   - `blocked` is allowed from Abstained (never an upgrade);
   - every event is validated against a clone of the aggregate before it is
     persisted;
   - recovery reports unreplayable runs instead of aborting.

   With fix 1 alone, every live run where the Evaluator abstained over
   retrieved evidence would have triggered this: 9 of the 10 completed live
   runs (all except L10, which retrieved nothing).

   Test: `abstention_with_invalid_proof_is_blocked_and_replayable`.
3. Smaller fixes:
   - `/decision` read the override rule from a file snapshot, so a read failure
     skipped the check. It now uses the owning log.
   - The evidence cap could overshoot by 4 records and degraded silently. It is
     now strict and recorded.
   - The fake-verifier detail now carries the SIMULATED label.
   - The Meta test server read requests with a single `read()`, which could
     truncate prompts. It now reads to `Content-Length`.
   - A pre-existing mojibake character in `tests/hyphae_adapter.rs` is fixed.

## Console HTTP smoke (real `consensus-api.exe serve`, temp log, port 8799)

- **Page and health:** `GET /` → 200, serving the scenario selector, `opToken`,
  `decNote`, the "simulated proofs" label, and the stable `mission-control-<run>`
  idempotency key. `/api/health` is ok.
- **Run creation:** `fake+hyphae` → 400 ("would be silently ignored"); unknown
  scenario → 400; the `invalid-proof` run → 201, `blocked`, with evidence actors
  `runtime` and verification details reading `SIMULATED proof`.
- **Decisions:** no token → 401; wrong token → 401; approving `blocked` with the
  token but no note → 400; token + note → 200 (`approved`). Retrying with the
  same key → 200 and no extra event (receipt `event_count` 10).
- **Receipt and replay:** the receipt carries `head_hash`,
  `provider: fake`, and `model: muse-wonder-1.0`. SSE replays 10 `consensus`
  events plus `replay_complete`.
- **Other scenarios:** `retry` → supported (1 revision), `offline` →
  insufficient_evidence, `injection` → insufficient_evidence, `abstain` →
  abstained.
- **Tamper and restart:** a copy of the log with one `"kind":"blocked"` edited
  to `"supported"` is refused at startup (`hash chain broken at line 9`); the
  original log restarts cleanly. Only the process this run started was stopped
  (path checked).
- The smoke test drove the API the page calls; no browser clicks were
  automated.

## Live battery

- **Sanity first:** `live_hyphae_probe` passed (tools: recall + status);
  `live_meta_propose` passed (`muse-wonder-1.0`,
  `chatcmpl-01a17b9a-a9a0-75f7-8828-0158895f7e61`, 13 s).
- **Table and analysis:** `docs/evaluation.md` §2 (Meta + Hyphae), §3
  (simulated-proof supplement, labeled), and request ids. Raw data, local and
  git-ignored: `demo-run/live-battery-2026-10-26.jsonl` (reopens and replays all
  15 runs cleanly) and `demo-run/live-battery/*.json` (run detail + receipt per
  run).
- **Headline:** 12 runs, 33 Meta calls → 9 blocked (upstream corruption signal),
  1 abstained (no evidence in `other-repo`: isolation held), 2 failed safely on
  the 60 s model timeout, 0 supported. The Evaluator abstained in 10/10
  completed runs (0 of 59 claims corroborated). The Critic flagged the stored
  constraint in 5 of 11 critiques. The 2 first proposals that violated the cap
  were never `supported`.
- **Hostile memory:** stored in project `injection-lab` via
  `scripts/hyphae-container/seed-injection-lab.sh` and treated as contradicting
  evidence by the live Critic and Evaluator. Caveat: it was labeled
  `integrity=Invalid` in the prompts.

## Files changed

- **`4faa6af`** (code; this commit also contains the prior session's
  uncommitted work in the same files, which could not be split without
  interactive staging):
  - Root: `Cargo.toml`, `Cargo.lock`.
  - `crates/consensus-domain/src/{events,evidence,lib,proposal,receipt}.rs`.
  - `crates/consensus-runtime/{Cargo.toml, src/event_log.rs, src/fakes.rs,
    src/orchestrator.rs, src/policy.rs, src/ports.rs, src/scenarios.rs,
    src/state_machine.rs}`.
  - `crates/consensus-api/src/{config,hyphae,meta,server}.rs`,
    `crates/consensus-api/src/console.html`,
    `crates/consensus-api/tests/{hyphae_adapter,meta_adapter}.rs`.
  - `crates/consensus-tests/{examples/evaluate.rs, tests/m1_scenarios.rs}`.
  - `schemas/{challenge,consensus-event,evaluation,evidence-reference,revision,run-receipt}.schema.json`.
  - This run's code changes, within those files:
    - `state_machine.rs`: Abstained → Blocked.
    - `orchestrator.rs`: validate before append, `Recovery`, strict and
      recorded evidence cap.
    - `fakes.rs`: SIMULATED label.
    - `hyphae.rs`: error category kept; comment corrected.
    - `server.rs`: snapshot errors, override check against the owning log,
      recovery reporting, module docs.
    - `evaluate.rs`: relabel.
    - All new tests.
- **`161196d`**: `.gitattributes` (new), `.gitignore` (`/claude-*.log`),
  `scripts/hyphae-container/seed-injection-lab.sh` (new),
  `scripts/hyphae-container/README.md`.
- **`89b03ca`**: `README.md`; `docs/adr/0001…0004`; `docs/demo-script.md`;
  `docs/evaluation.md`; `docs/journal.md`; `docs/mission-control.md`;
  `docs/pitch.md`; `docs/threat-model.md`; `docs/reviews/` (review + brief).

## Tests added in this run (11 new, 1 extended; 56 → 67)

- `consensus-tests/tests/m1_scenarios.rs`:
  - `abstention_with_invalid_proof_is_blocked_and_replayable`
  - `upstream_corruption_signal_blocks_and_is_never_downgraded`
  - `non_corruption_proof_failure_is_unavailable_not_supported`
  - `reusing_an_idempotency_key_across_runs_records_both_decisions`
  - `startup_recovery_closes_crashed_runs_and_reports_broken_ones`
  - `evidence_cap_is_strict_recorded_and_retrieval_is_runtime_attributed`
- `consensus-api/tests/meta_adapter.rs`:
  - `untrusted_text_cannot_close_its_delimiter_block`
  - `revise_sees_critic_items_and_evidence_and_filters_item_ids`
  - `proposal_claim_count_is_bounded`
- `consensus-api/src/server.rs`:
  - `every_console_scenario_reaches_its_documented_outcome`
  - `corrupt_log_line_is_reported_never_skipped`
- Extended: `consensus-api/tests/hyphae_adapter.rs`
  `recall_falls_back_to_unproven_when_proof_generation_fails` now uses the exact
  live envelope and asserts the `proof_error` category.

## Remaining risks and open decisions (owner)

1. **Live `supported` is unreachable today, for two independent reasons.**
   (a) Upstream `recall(prove=true)` reports corruption, so every live run with
   evidence is blocked. (b) The live Evaluator abstained in every completed
   run, even with simulated-valid proofs (S01–S03), and abstention ends the run
   before any revision.

   Design options (not implemented): allow a revision after an abstention that
   still lists unresolved items; ask for memory-checkable claims; seed richer
   memory; rank `contested` above `abstained`. In S02 the Evaluator found the
   real contradiction, but abstention hid it.
2. **Hyphae recovery not attempted.** The error says `retry: after-recovery`, and
   `hyphae agent doctor` / `recover` exist, but they can mutate the container
   store. The decision is left to the owner.
3. **The hostile note remains in the container store** (project `injection-lab`,
   id `202446520301215502540295194836741894905`). Remove it with
   `hyphae_memory_forget` if unwanted; `demo-repo` is unaffected.
4. **Untested:** injection with a *valid*-proof hostile record; fault injection
   for append rollback; real browser interaction with the UI.
5. **No per-run token budget.** The 12-claim cap applies only to the Meta
   adapter. Live runs used ~2k input / ~8–12k output tokens each.
6. **Console security:** only `/decision` is authenticated, and anyone who can
   read server stdout has the token. There is no CSRF protection, so the
   console must stay loopback-only. The event-log mutex is held for a whole live
   run (~1–2 min); pre-existing.
7. **The hash chain is unsigned:** tamper-evident against an exported
   `head_hash`, not against a full re-hash.
8. **Model timeouts:** 2 of 12 runs hit the 60 s default; consider
   `CONSENSUS_MODEL_TIMEOUT_SECS=120` for demos.
9. **Degenerate model output passes validation:** L02's Critic returned
   label-only items.
10. **License still undecided.** Check whether the hackathon requires a public
    repository with an OSI license.

## Deviations from the brief, and incidents

- **Order:** code fixes came before the console smoke test, so the smoke test
  exercised the final server code (the fixes touched `server.rs`).
- **Supplementary runs:** 3 runs with `model=meta`, `memory=fake` (simulated
  proofs) were added beyond the Meta + Hyphae battery, to separate model
  behavior from the upstream proof failure. They are labeled everywhere.
- **Hostile memory placement:** seeded into a separate project, not `demo-repo`,
  to keep the demo store clean.
- **Incident:** the live-battery server's ephemeral per-process operator token
  appeared once in this session's own log (`claude-fix-run.log`). PowerShell
  wrote the startup output as UTF-16, and the redaction filter missed it. That
  server process was stopped after the battery, so the token is void.
  `META_API_KEY` and the Hyphae key file contents were never printed. The log
  is git-ignored and not committed.
- **Not done, by constraint:** push, PR, or remote changes; license choice;
  modifying Hyphae.
