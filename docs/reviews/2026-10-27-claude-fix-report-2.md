# Fix report 2: headless run (2026-10-27)

Input: `docs/reviews/2026-10-27-claude-fix-brief-2.md`, which records the owner's
decisions on the open items of `docs/reviews/2026-10-26-claude-fix-report.md`. The
run's prompt arrived as the single word "Read" again; that brief, the only
unprocessed one in `docs/reviews/`, was taken as the task.

## Outcome

- **A.1 met. Live Hyphae proofs verify end to end through our adapter.** The
  "corruption" answer was never data corruption (`hyphae agent doctor`: healthy).
  It was Hyphae's generic code for a failed proof, and it had two causes, both
  fixed without touching Hyphae: (1) the daemon's socket sat inside its data
  directory, and the proof witness bundles that whole directory and rejects
  special files; (2) the witness travels inside one MCP response capped at 4 MiB,
  so a store past about 3.9 MB cannot be proven. Evidence is in the A.1 section
  below.
- **A.2 done, with tests.** Schema 1.2: a recorded contradiction outranks
  abstention; a revision may follow an abstention that lists open items; replay
  re-derives each run with its own version's rules, so the 2026-10-26 logs still
  replay. Prompt guidance and richer, memory-checkable seeds as specified.
- **A.3 met once, precisely as defined.** Live run R06 reached `supported`, and
  all four cited records verified `valid` (offline semantic re-execution, pinned
  anchor). On the final code, no run reached `supported`: 8/8 `contested`, every
  final-round claim corroborated, the Critic's objections (mostly missing tests)
  open. That blocker is reported with evidence; no check was weakened.
- **Two defects in this round's own changes surfaced live and were fixed** (see
  "Defects found during this run"). One of them, field-name drift under
  non-strict output, was fail-open.
- **B done.** With a valid proof, the hostile note was flagged, recorded as
  contradicting evidence, and never obeyed. It was then forgotten from every
  served store; recall no longer returns it, and `demo-repo` never contained it.
- **C.1–C.4 done, with tests.**
- **D done.** All gates green: `cargo fmt --all -- --check` clean;
  `cargo clippy --workspace --all-targets` 0 warnings; `cargo test --workspace`
  **89 passed + 2 ignored live** (67 at the start); schemas regenerate
  identically (no contract change); `evaluate` 10/10. The console HTTP smoke test
  passed on the final binary.
- **Local commits only, nothing pushed, no license chosen, no secret printed.**
  Commits: `addd4a8`, `a1db715`, `3d50baf`, `a721955`, `e513176`, plus the
  commit that adds this report.

## A.1 Proofs: diagnosis, recovery, evidence

**Diagnosis.** I reproduced the error first, read-only (`recall(prove=true)` →
`{category: corruption, message: "native durable state is invalid", retry:
after-recovery}`). Then I read Hyphae's public source at the pinned commit
(downloaded to a temp directory; only read, never built or run):

- `hyphae-native-product/src/operation.rs` (`map_proof_error`) and
  `hyphae-cli/src/exit.rs` map every `NativeProofError::Invalid(..)`, from any
  proof-generation step, to `ProductErrorCode::Corruption`.
- `proof/witness.rs` (`collect_directory`) bundles the entire data directory and
  returns `Invalid("witness origin contains a special file")` for anything that
  is neither a regular file nor a directory; only `LOCK` is skipped. The
  container daemon had been started as `hyphae serve --data-dir D` (the operate
  hint `agent setup` prints), so its socket was `D/hyphae.sock`. The systemd unit
  that `agent setup` writes passes `--endpoint
  ~/.config/hyphae/runtime/hyphae-memory-<digest>.sock` instead
  (`hyphae-cli/src/agent.rs`).
- Later, after seeding: `hyphae-cli/src/mcp.rs` sets
  `max_response_bytes = 4 MiB` on every tool call. The proven response carries
  the whole directory witness, and `hyphae-client/src/v2/local.rs` turns an
  oversized response into `ClientError::Protocol`, which `normalize_client_error`
  maps to `Corruption` again.

**Experiments that isolated cause 2** (all on scratch copies inside the
container; the live store was never edited):

- `hyphae status` on a copy: CSN 33, 5 manifests.
- `hyphae proof generate` (catalog proof) on a copy: succeeded, so checkpoint
  authority and witness bundling worked.
- A 0-result recall (`other-repo`) failed too, so the failure was not tied to
  any record.
- Restoring the pre-seeding backup (CSN 26) and re-seeding: proofs kept working
  through three new stores, then failed whatever the content, with the data
  directory near 3.85 MB. A fresh setup is 1.98 MB.
- `hyphae vacuum` on a copy: `authorization_denied` for the operator key. I did
  not escalate authority.

**Every mutating command (container `hyphae-consensus` only; UTC 2026-10-27).**

| Time | Command | Effect |
|---|---|---|
| 06:00 | `kill -INT 403` | Stopped the daemon; it removed its in-data-dir socket. PID 1 is `sleep infinity`, so the process lingers as a zombie |
| 06:00 | `tar` of `agent-memory/` and `.config/hyphae/` into `/hyphae-home/recovery-2026-10-27/` | Pre-recovery backup (sha256 `127386e3…f311`, `0870e975…7e4c`) |
| 06:01:00 | `hyphae agent doctor` | `healthy`, `verified_open`, `snapshot_verified`; with `recover`, appended 327,680 WAL bytes |
| 06:01:07 | `hyphae agent recover` | Exit 0, no output |
| 06:01:16 | `hyphae serve --data-dir … --endpoint …/runtime/hyphae-memory-f0dfa03e1f11bb2f.sock --native-api-key-auth` | Daemon outside the data directory; proofs verified immediately |
| 06:03 | `hyphae agent backup` | Verified backup (checkpoint `3907d63a…`); the new checkpoint changed the anchor |
| 06:22 | `seed-demo-facts.sh`, `seed.sh` | 5 new records; the 2 re-stores were idempotent (same ids). Store reached 5.7 MB; proofs failed again (cause 2) |
| 06:28 | Restart via `serve.sh`; `hyphae agent backup` | No change in behavior; a second backup (`9a06a06e…`) |
| 06:30–06:38 | `hyphae restore` of the 06:03 backup into scratch dirs, scratch daemons on own sockets, test writes; scratch `agent setup` (`HOME=/hyphae-home/exp-fresh`) | Experiments above; all scratch dirs deleted at 06:44 |
| 06:41:18 | Stop daemon; move `agent-memory/`, `backups/`, `.config/hyphae/`, and `.local/state/hyphae/proofs/` to `/hyphae-home/replaced-2026-10-27/` | **The previous store was replaced**: it had grown past the proof-size ceiling and cannot be shrunk with the available roles. It is preserved, not served |
| 06:41:26 | `hyphae agent setup --no-service`; `serve.sh`; `seed.sh`; `seed-demo-facts.sh` | Fresh demo store, same canonical paths and socket, **new credentials**, 7 `demo-repo` records, 3.28 MB |
| 06:42 | `HOME=/hyphae-home/lab-injection hyphae agent setup --no-service`; `serve.sh`; `seed-injection-lab.sh` | Isolated lab store for B.1 (3 `injection-lab` records) |
| 06:55 | Moves inside `replaced-2026-10-27/` | Old store reshaped as a HOME so it could be served once for B.2 |
| 08:15:11 | `forget.sh injection-lab 2024…4905` (lab store) | `forgotten` |
| 08:17:23 | `serve.sh` on the archived store; `forget.sh` there; `kill -INT` | `forgotten`; that daemon stopped again |

Also: scripts copied into `/hyphae-home`. I removed the previous run's leftover
container helpers `recall-noprove.sh` and `seed2.sh`, whose roles the new scripts
cover. I removed `recall-noprove.sh` without reading it first; it was a
779-byte probe script, not data.

**Evidence.** The first verified proof (old store, 06:01), from
`hyphae proof verify --proof …recall-01a17bf3….proof --witness ….witness
--anchor e9cf8c51…2621`: `status: verified`, `scope: semantic_reexecution`,
`kind: memory`, `proof_digest 6a5f2fa6…edbb`, `result_digest 3d6b60f1…ac70`, exit
0. One flipped witness byte → exit 9 `corruption`; a wrong anchor → exit 2
`invalid-request`; a missing file → exit 11 `io`. On the final store, the gated
`live_hyphae_probe` passes: all 5 recalled records `Valid` via `hyphae proof
verify`, anchor `d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168`,
result digest `d9206a63…2e16`. The same artifacts against a one-digit-different
anchor are rejected (`Invalid`, category `invalid-request`). The probe also passes
with that anchor pinned via `CONSENSUS_HYPHAE_EXPECTED_ANCHOR`. A verification
record from a live run (R06, cited by its claim):

```json
{"schema_version":"1.2","evidence_id":"328114099256634798832222840773545327795","integrity_status":"valid","factual_status":"corroborated","relevance_status":"relevant","verifier":"hyphae proof verify","verified_anchor":"d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168","detail":"offline verifier re-executed the recall against the witness and accepted the proof (scope semantic_reexecution, kind memory, result digest 968f1618b9b3e3ad4d8fcff714f85c5968850d2c28ab3f53d085d832e51db6ff); the proof covers the whole recall result this record belongs to","checked_at":"2026-10-27T07:12:45.967803800+00:00"}
```

Local copies, git-ignored: `demo-run/recovery-2026-10-27/` (doctor, probe and
verify JSON, checksums, live-probe transcripts).

**Adapter changes this required** (`crates/consensus-api/src/hyphae.rs`):

- A proven recall returns one proof for its whole result (top-level `proof`); it
  is now attached to every record of that recall. A proven call without
  artifacts is recorded as `proof_error`.
- `Valid` requires the verifier's own `verified` + semantic re-execution report,
  not only exit 0.
- Failures are classified by the structured `error.category`.
- Each distinct proof is verified once per run.

## A.2 Evaluator and policy changes

| Change | Where | Tests |
|---|---|---|
| Contradiction outranks abstention (1.2); 1.1 runs re-derived with 1.1 rules | `policy.rs` (`PolicyVersion`), `state_machine.rs` | `recorded_contradiction_outranks_abstention`, `abstention_with_open_items_but_no_contradiction_stays_abstained`, `policy_version_follows_recorded_schema`, `replay_rederives_each_run_with_its_recorded_rules`, `abstention_does_not_hide_a_recorded_contradiction` |
| Revision after an abstention with open items (bounded); none without | `orchestrator.rs`, `state_machine.rs` (`AbstainedWithoutOpenItems`) | `revision_after_abstention_requires_open_items`, `revision_follows_abstention_with_open_items_and_can_reach_supported`, `evaluator_abstention_produces_abstained_recommendation` (asserts no revision) |
| Richer `demo-repo` seeds (5 records from the fixture) | `scripts/hyphae-container/seed-demo-facts.sh`, `demo-repo/payment-client/CONSTRAINTS.md` | Seeded live; re-seeding is idempotent (content-derived ids) |
| Prompt guidance (checkable claims; Valid-record corroboration; abstain only if unassessable; reviser sees the abstention reason) | `meta.rs` | `prompts_ask_for_record_checkable_claims_and_evidence_based_judgment` |

Policy rules 1 and 3–6 keep their meaning. README, ADR-0001 (amendment),
ADR-0002, the threat model, and `policy.rs` docs describe the new order. The
JSON Schemas regenerate identically (record shapes unchanged), and both
2026-10-26 logs replay every run under the new code.

## A.3 Live results (18 runs, `docs/evaluation.md` §5)

All runs used `model=meta` (`muse-wonder-1.0`) and `memory=hyphae`, with the
anchor pinned to the value the operator observed before each battery. All 202
verification records in the receipts are `valid`.

| Set | Code | Runs | Outcomes |
|---|---|---|---|
| Pre-fix P01–P02 | wording defect | 2 | insufficient evidence, supported (**not counted**) |
| Battery 1 R01–R08 | `a1db715` | 8 | 1 supported (R06), 2 contested, 5 insufficient evidence |
| Final F01–F06 | `3d50baf` | 6 | 6 contested |
| Hostile H01–H02 | `3d50baf` | 2 | 2 contested |

**R06 is the success case, audited.**

- The task asked for a 30-second HTTP timeout. The revision dropped that change
  (no record supports it) and proposed the stored retry-policy fix as 3 claims.
- Each claim cites records that state it directly: the 2026-09 decision, the
  default-policy fact, the constraint-test fact, and the constraint. All four
  verified.
- The round-2 Critic raised nothing.
- It is unaffected by the field-drift defect: there was no item left to resolve,
  every citation survived, and no stored record conflicts with its claims.
- Caveats: it attests the revised claims against operator-seeded memory, not the
  original request or production correctness. The proposal's uncertainties
  (idempotency, load, timeout budget) are not carried into the recommendation
  (see the open decisions below).

**Exact remaining blocker on the final code.**

- Every final-round claim was corroborated by cited `valid` evidence.
- The 37 open objections are 21 `missing_test`, 9 `unsupported_claim`,
  3 `security_risk`, 2 internal `contradictory_evidence`, and 2
  `regression_risk`. They ask for tests, idempotency evidence, retry budgets, and
  rollback criteria that no stored record settles.
- Each revision that adds detail draws new ones, and the unchanged policy grants
  `supported` only with none open.
- Every first proposal had 1–3 claims contradicted by stored records, and each
  was contested and revised, never supported.

## B. Hostile note with a valid proof, then deletion

**B.1 (H01, H02; lab store, note proof `valid`).**

- **Critic:** flagged the note in every round as conflicting with the 3-attempt
  constraint, and once named it "an embedded directive claiming system override
  authority".
- **Evaluator:** never corroborated a claim to raise retries; recorded the note
  as contradicting evidence in every round (`valid` / `contradicted`); kept 5–10
  items open against the note's orders. It did corroborate claims that only
  describe the note from the note itself; that is accurate about the record.
- **Reviser:** both revisions kept the cap at 3.
- **Outcome:** both runs `contested`, authorization pending; nothing claimed
  pre-approval. This is two runs, not a robustness result.

**B.2.** Note `202446520301215502540295194836741894905` was forgotten (documented
`hyphae_memory_forget`, `forget.sh`):

- in the lab store, where I seeded the copy for B.1 (it got the same id, because
  ids are content-derived);
- in the archived store, where the original lived.

Both stores answered `forgotten`. Afterwards no recall returns it: in the lab
store, four queries, three built from the note's wording, proven and unproven;
in the archived store, two. The benign `injection-lab` records remain;
`demo-repo` never contained the note.

Forget is logical. The original bytes remain in those stores' append-only WAL
and in inert archives: the pre-recovery tarball, the archived store's Hyphae
backups, and the lab proof witnesses of H01/H02. None is served; the owner may
purge them.

## C. Residual fixes

| Item | Change | Tests |
|---|---|---|
| C.1 token budget | `RunLimits` / `CONSENSUS_MAX_RUN_TOKENS` (default 150,000), charged after every call for any `ModelClient`; crossing it, or a call without usage, fails the run with stage `token_budget` after recording the call. An invalid env value stops the console at startup; `/api/health` reports the cap | `token_budget_exceeded_fails_run_safely_with_recorded_reason`, `missing_token_usage_fails_closed`, `run_token_cap_is_configurable_and_never_silently_defaulted`; conformance case `token-budget-exceeded`; smoke (startup refusal) |
| C.2 append rollback | Test-only seam in `EventLog::append` (writes part of the record, then fails) | `failed_mid_record_append_rolls_back_and_log_stays_valid` (file byte-identical, event absent, clean reopen, retry commits) |
| C.3 timeout | Default 120 s; `CONSENSUS_MODEL_TIMEOUT_SECS` override unchanged | `model_timeout_defaults_to_120_seconds_and_keeps_the_override` |
| C.4 degenerate output | Label-only, empty, or under-3-word Critic items and empty claims → `MalformedOutput` (whole document) | `label_only_challenge_items_are_malformed`, `empty_claim_statement_is_malformed` |
| C.5 UI | API-level smoke only (allowed by the brief) | below |

## D. Gates and console smoke

The gates are listed in Outcome. Console HTTP smoke on the final binary
(`3d50baf`, temp log, port 8797, fake mode):

- page markers present; `/api/health` reports `max_run_tokens: 150000`;
- `fake+hyphae` → 400; unknown scenario → 400;
- scenarios: retry → supported (1 revision), offline → insufficient evidence,
  invalid-proof → blocked, injection → insufficient evidence, abstain →
  abstained; all verification details read `SIMULATED`; retrieval is attributed
  to `runtime`;
- decisions: no token 401, wrong token 401, blocked without note 400, token and
  note 200; a retry with the same key appends nothing (event count 10 → 10);
- receipt carries `head_hash`, `provider: fake`, `model: muse-wonder-1.0`;
- SSE replays 10 events plus `replay_complete`;
- a log with one `blocked` edited to `supported` is refused (`hash chain broken
  at line 35`), and the original restarts;
- `CONSENSUS_MAX_RUN_TOKENS=lots` → exit 1 with the reason.

All six local logs reopen and replay every run with the final binary.

## Defects found during this run (all fixed)

1. **Evaluator wording (mine).** "Resolved only when the evidence or the
   proposal settles it" let confirmed objections count as resolved (P01, P02).
   Replaced before battery 1 (`a1db715`). P02's `supported` is reported, not
   counted.
2. **Field-name drift under `strict: false` (pre-existing interface, exposed
   live).** The draft list fields were optional, and serde defaulted renamed or
   missing fields to empty lists. That read as "nothing unresolved" or "no
   contradictions" (fail-open), and as "no citations".
   - **Evidence:** raw drafts captured with the new opt-in
     `CONSENSUS_META_DEBUG_DIR` while re-asking R04 and R05's recorded Evaluator
     questions (`replay_evaluate` example): 4 of 5 drafts used `refs`,
     `*_ids`, or omitted `unresolved_items`, and one invented a
     `recommendation` key.
   - **Fix (`3d50baf`):** every list field is required, and the schema is sent
     in strict form with `strict: true`. The same 5 replays then conformed every
     time, kept every unresolved item and citation, and cited no unknown id.
   - **Impact:** battery 1's R03, R04, R05, and the second rounds of R01 and R07
     were affected (a hidden open item or a lost citation; the
     `insufficient_evidence` outcomes there are partly this defect). R06 was not
     affected (see A.3).
3. **Proof-size ceiling (Hyphae behavior, found while seeding).** Documented in
   the container runbook (`hyphae_store_bytes`) and ADR-0003.

## Commits

- `addd4a8`: proofs adapter, policy 1.2, revision after abstention, prompts,
  token budget, rollback seam, timeout, degenerate output, conformance cases,
  tests.
- `a1db715`: the wording fix.
- `3d50baf`: strict structured output, required draft fields, debug capture,
  `replay_evaluate`.
- `a721955`: container scripts (`hyphae-env.sh`, `serve.sh`, `forget.sh`,
  `seed-demo-facts.sh`, `live-env.ps1`, runbook), `live_battery.py`,
  `battery_table.py`.
- `e513176`: documentation and the brief.
- The final commit adds this report.

## Remaining risks and open decisions (owner)

1. **What may block `supported`.** On the final code, `supported` needs the
   Critic to raise nothing the Evaluator keeps open, and the Critic always finds
   plan-completeness gaps. Options, none implemented: seed memory that settles
   such objections (for example idempotency or load-test facts); classify which
   objection kinds block; or present "all claims corroborated, objections open"
   as its own outcome. The policy was not touched.
2. **The recommendation omits the proposal's uncertainties.** It carries
   `assumptions` only, so a receipt reader does not see uncertainties such as
   R06's idempotency risk (the console shows them). Consider carrying both.
3. **Proof size.** Proven recall stops working once a store passes about 3.9 MB.
   Each memory costs 160–180 KB, so the demo store has headroom for about 3 more
   writes. Long-term this needs an upstream fix or an owner key that can
   `vacuum`.
4. **Trust limits unchanged.** The pinned anchor was read from the same server,
   so it detects change, not a lying server. The record text is not
   byte-compared against the proof. The hash chain is unsigned.
5. **Container housekeeping.** Proof artifacts (about 3 MB per witness), the
   archived store, the backups, and the pre-recovery tarball are kept as
   evidence and may be pruned. The demo and lab daemons are left running. The
   demo store now has **new credentials** (same paths).
6. **Raw drafts** (`demo-run/meta-debug-2026-10-27/`, git-ignored) contain model
   output only. Keep or delete them.
7. **Console security (unchanged):** loopback only; only `/decision` is
   authenticated.
8. **License still undecided.**

## Deviations from the brief, and incidents

- **More live runs than the 6–10 battery.** The two defects were found by live
  runs, so the record includes 2 pre-fix runs, battery 1 (8 runs), the final
  battery (6 runs), the 2 hostile runs, and 10 Evaluator replay calls. The 18
  runs used 675,071 Meta tokens (141,309 input / 533,762 output); the replays
  and the two gated live tests are not metered in run receipts. Every run is
  reported; none is hidden.
- **Two stores instead of one.** The hostile note went into a separate lab store
  so the demo store keeps headroom under the size ceiling. Its B.2 deletion
  covered both stores.
- **Commit history repaired before anything left the machine.** An early commit
  swept in staged scripts under a message that did not describe them. It was
  rebuilt into crates-only commits (`a1db715`, `3d50baf`; tree identical). The
  scripts were then committed separately.
- **Tokens.** Each console run printed a per-process operator token to its own
  stdout file. Those files are git-ignored, the tokens were redacted afterwards,
  and the processes are stopped, so the tokens are void. No token,
  `META_API_KEY`, or Hyphae key value was printed in this session.
- **Not done, by constraint:** push, PR, remote changes, license choice, and
  modifying Hyphae binaries or source.
