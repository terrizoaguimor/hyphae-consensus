# Threat model — Hyphae Consensus (v2)

Date: 2026-10-28 · Version 2 · Describes the code at `25c7867` (branch
`feat/public-portal`: `e915b6d` plus the review fixes of `25c7867` and the template
fix of `0c473a6`, see the verification note) · Supersedes v1 (2026-10-26/27, which described `72e422a`)

Companions: ADR-0005 (per-run logs, asynchronous runs, live SSE), ADR-0006 (public
API v1, sessions, listener split), ADR-0007 (public live mode on the owner's key),
ADR-0008 (sealed Hyphae snapshot), ADR-0009 (frontend), ADR-0010 (protocol 1.3),
ADR-0002 and ADR-0003 with their 2026-10-27 notes; the independent audit of `72e422a`
(`docs/reviews/2026-10-27-independent-audit.md`); the security release checklist
(`docs/security/release-checklist.md`, gates G1–G12); the sealed-snapshot architecture
(`docs/architecture/sealed-snapshot.md`); the deployment runbook and its templates
(`docs/deployment/runbook.md`, `deploy/`).

**What changed since v1.** The operator console of v1 (one loopback router, one
multi-run log behind one mutex, a synchronous `POST /api/runs`, an operator token in the
request body) no longer exists. The binary has two listeners: `serve-public`, a unix
socket for cloudflared (or a loopback TCP bind for development) serving anonymous cookie
sessions, guided and live runs, the static web export and the research archive; and
`serve-operator`, a loopback TCP listener behind a bearer token, started on demand. Every
run has its own hash-chained log, index entry and lock; runs start with `202` and stream
over SSE; a cancel, a deadline and a graceful shutdown exist. The evaluation protocol is
1.3 (fail-closed contract, carry-forward) and the Meta and Hyphae adapters are hardened.
The public live mode runs on the owner's key behind a two-phase gate (preset, geography,
attestation, the evidence status and Turnstile first; then a per-client daily quota and a
USD ledger reservation), over a sealed Hyphae snapshot whose files are pinned, re-hashed
and bound to every run's proof checks. The operator console meters its own `model=meta`
runs on the same ledger when it is given the ledger directory, and `consensus-api ledger`
lets an operator read, reset and reconcile the ledger from a shell. The deployment exists
as templates only. **The public portal is not exposed.** Nothing is tunneled until every
gate G1–G12 reads PASS. At `e915b6d` live mode can be enabled only over a sealed snapshot.
The one sealed snapshot, `snap-2026-10-29-demo-repo`, was captured on 2026-10-29
(`b0375df`); the code at `e915b6d` (and at `6e202eb`) refuses it at boot until the fix
`0515f29` (merged as `7e26866`) is in (section 5).

**How this document was verified.** Every mitigation below names its code (file and
function or module) and the tests that cover it; where no test exists, the text says so.
Every test named here exists in the tree at `e915b6d` and passed in
`cargo test --workspace --locked --no-fail-fast` on Windows (x86_64-pc-windows-msvc,
2026-10-28, the Meta key variables unset): 32 test executables and 4 doc-test groups,
**599 passed (596 test functions and 3 doc tests), 0 failed, 2 ignored** (`live_hyphae_probe`,
`live_meta_propose`, the gated live probes); every test cited here was then run again by
exact name, binary by binary (`cargo test -p <crate> --locked` with `--lib`, `--bin` or
`--test <binary>`, then `-- --exact <names>`; 30 binaries), and passed. Platform-gated
tests are marked where they are cited: unix-only (`#[cfg(unix)]`, not part of this run)
are `socket_mode_requires_explicit_origins_and_hosts` and
`a_log_replaced_by_a_symlink_is_refused`; Windows-only are
`an_append_the_disk_refuses_is_neither_stored_nor_published` (`#[cfg(windows)]`) and
`without_unix_sockets_the_public_bind_is_required` (`#[cfg(not(unix))]`). On this machine
`a_symlinked_artifact_pointing_outside_the_directory_is_refused` returned early (the
account has no symlink privilege; the test then counts as passed) and
`a_symlink_loop_under_the_directory_is_refused_not_followed` used a directory junction.
Linux parity (`scripts/linux-gates.sh`, Ubuntu 24.04) passed on the tree of `25c7867`:
7 of 7 gates, 603 passed / 0 failed / 2 ignored, unix-only tests included. Tests added
by `25c7867` (named below where they apply) passed in both runs. The web unit
and end-to-end suites (`apps/web`, Docker-only toolchain), `cargo deny`, `pnpm audit` and
the two sealed-snapshot tooling self-tests were not run for this document; where their
results are cited, they are the results the lanes recorded in their commit messages. An
independent review of the live-hardening code (`561c25e`, `e915b6d`) then found one high
issue and six lows: `25c7867` fixes the high (the ledger tool and the metered operator
console refused the deployed `public.env`, T30) and five lows, documents the sixth
(section 5, item 10) and leaves its medium, the web client's generated types, to the web
lane (section 5, item 14). No deployed system exists, so
everything that depends on the edge, the tunnel or the server is marked pending.

## 1. System and trust boundaries

| Participant | Trust | Authority and what it can do |
|---|---|---|
| Runtime (`consensus-runtime`, `consensus-domain`) | Trusted | State transitions, the evaluation contract, persistence, policy derivation, receipts. Models never write a recommendation or an authorization |
| Public server (`consensus-api serve-public`) | Trusted code, untrusted input | Serves anonymous sessions over the unix socket; owner-scoped reads; guided runs (scenario id) and, with live mode enabled, live runs (preset id) over the sealed snapshot (`CONSENSUS_SNAPSHOT_DIR` is then required, and on the socket both snapshot pins); a snapshot that does not boot leaves the portal up with live mode unavailable. Holds no Hyphae credential; runs `hyphae proof verify` as a child process when live mode is on; consults the Meta key only through the live gate |
| Operator server (`consensus-api serve-operator`) | Trusted code, authenticated operator | Loopback only, bearer token on every `/api` route. The unit template has no `[Install]` section (started with `systemctl start` over SSH, cannot be enabled) and runs as the same user `consensus` as the public server, with its own `LoadCredential` copies of `meta.key` and `operator.token` (no Turnstile secret since `e915b6d`). Free task text, caller-chosen project, `model=meta`, `memory=hyphae`. Its `model=meta` runs are metered on the operator lane of the live ledger when `CONSENSUS_LIVE_LEDGER_DIR` is set (the template inherits it from `public.env`), unmetered otherwise (its startup line says which). Its decisions carry `operator` authority; its views keep raw failure reasons and verifier output |
| Cloudflare edge and WAF | Trusted for `CF-Connecting-IP` and `CF-IPCountry` **only** when the request arrived through the unix socket (or a development bind with `CONSENSUS_TRUST_PROXY_HEADERS=true`) | Geo rules and one rate-limit rule are the primary controls; the origin re-checks the same country list. Edge features that rewrite HTML must be off (G6). Nothing at the edge is configured yet |
| cloudflared and the unix socket | The socket (mode 0660) is the trust boundary for the proxy headers | Templates: `/run/consensus` is `2750 consensus:consensus-sock` (`deploy/tmpfiles.d/consensus.conf`), so the socket's group is `consensus-sock`, cloudflared's only extra group. The socket's owner `consensus` can connect too, and that user runs both servers and every verifier child: any of them, and root, can present forged `CF-*` headers (T41) |
| Turnstile / Siteverify | Trusted to say whether a challenge was solved; proves no identity | Fail closed: outage, wrong `action`, wrong `hostname`, stale `challenge_ts`, non-200 or unparseable body all refuse; at most 8 Siteverify calls in flight, past that `503 turnstile_unavailable` before the token is sent |
| Visitors and their sessions | Untrusted | May create a session (POST only), start guided runs, start live runs behind the gate, read, stream, download and decide on **their own** runs; a public decision carries `public_demo` authority and no operational effect. The optional decision note is the only visitor free text the server stores |
| Judges with invite codes | Untrusted; a code only selects a lane | A code whose digest is in the codes file enters the judge lane (its own budget slice, daily cap, rolling-hour window and slot, and at most `CONSENSUS_LIVE_JUDGE_CODE_DAILY` runs per code per UTC day); an unknown code falls to the public lane without an error of its own, but the lane stays observable (T29) |
| Models (Proposer / Critic / Evaluator / reviser, Meta Model API) | Untrusted | Produce drafts only; drafts have no outcome or authorization fields; every decision-relevant id is validated; the Evaluator's judgments are the policy's inputs (T1–T4) |
| Meta Model API and the owner's key | The key is the asset; the API is a paid, rate-limited third party with no hard spending cap | The key is read from a credential file (or a key variable), zeroized on drop, never printed; every HTTP attempt of a public run, and of a metered operator run, is drawn on the ledger before it is sent |
| Hyphae memory backend (operator lab only) | Integrity-verifiable, content untrusted | `hyphae mcp --profile memory` without `--allow-write`; the reader key reaches the child as a file path; records keep the backend's own project and scope |
| Hyphae verifier binary (`hyphae proof verify`), a subprocess | Trusted to verify; untrusted output | Run with a deadline and killed past it, its stdout/stderr capped, the Meta key variables and the five credential-path variables removed from its environment (the environment is not cleared, and the child runs as the same user, so it can read the credential files); `Valid` requires its report to say `verified`, `kind: memory`, `scope: semantic_reexecution`, `anchor_digest` equal to the anchor it was given. In the public live mode a run's `Valid` stands only for the digests the boot check accepted, and the binary's SHA-256 can be pinned (`CONSENSUS_HYPHAE_BIN_SHA256`, required with live mode on the socket) and is checked at boot only |
| Sealed snapshot artifacts (`manifest.json`, `SHA256SUMS`, captures) | Integrity-checked; proofs and witnesses are **private** | Checked at boot: `SHA256SUMS` over every file, the manifest's digest of every capture file (response, proof, witness, capture-time report), the manifest and verifier-binary pins when set (both required with live mode on the socket), the served project, one anchor, both served captures re-verified against their capture-time digests. After boot the manifest and the two served captures' files are re-hashed by every admission and, at most every 2 s, for `/ready` and `/capabilities`; a change makes live mode unavailable. Per run, a `Valid` stands only for the boot-accepted proof and result digests. Record texts are served from memory as boot read them. Witnesses are full copies of a store's data directory; no route serves them (T11) and `.gitignore` excludes them |
| Research archive (`research/`) | Read-only, pinned by `MANIFEST.json` and `SHA256SUMS` | Served verbatim, including the container paths the 2026-10-26/27 and 2026-10-30 batteries recorded (T11); showcase views are display copies; never opened writable |
| Ledger files (`<ledger dir>/YYYY-MM-DD.jsonl`, `lifetime.json`, `ledger.lock`, `kill-switch`) | Trusted; written by the server processes and by the operator (the `ledger` subcommand, the kill-switch file) only | `serve-public`, `serve-operator` (only with `CONSENSUS_LIVE_LEDGER_DIR` set) and `consensus-api ledger` serialize through the lock. The kill-switch file trips the ledger, which then refuses every lane (public, judge, operator) until `consensus-api ledger reset`. Operator runs on a server without the variable are outside all of it |
| Web client (`apps/web`, static export) | Untrusted at runtime (it is a browser); its build is a supply-chain surface | Renders what the API says (typed client, SSE stream and run workspace since `4c42ae2`, its types generated from contract `cc890dc030ba`, one contract behind the server, section 5); LIVE only from per-frame `delivery` of a live model run; builds only inside Docker containers because the owner's machine holds the key |
| Retrieved memories, repository contents, model outputs | Untrusted data | Never executed; shown to models as data inside `<UNTRUSTED_*>` blocks (with the id exceptions of T2), which is defense in depth, not a boundary |
| Human operator | Trusted authorizer | The only decision with operational authority; holder of the operator token, the server key file, the kill switch, the ledger subcommand and the capture session |

**Data flow.** A visitor's request reaches the Cloudflare edge (WAF geo rules, the
rate-limit rule, TLS), is forwarded by cloudflared to the unix socket, and is handled by
`serve-public`. In order, the request passes: the request timeout (`408` past 30 s); the
16 KiB body limit (`413`); the security headers layer; the `Host` allowlist (`421`); the
CSRF rules for state-changing `/api` requests (`Origin` required and allowlisted,
`Sec-Fetch-Site` absent or `same-origin`, `Content-Type: application/json` where a body is
read; `403`/`415`); the portal-wide geographic block on every public route (`451`); the
session extractor (`401 session_required` without a live cookie); the client-key extractor
(`CF-Connecting-IP` required behind the socket, `400 client_unidentified` without it).
Handlers run store reads and every ledger or Siteverify call on the blocking pool
(`tokio::task::spawn_blocking`). A guided start claims its `Idempotency-Key`, passes the
creation admission (shutdown, rate, the visitor-run ring), takes a deliberation permit,
creates the run's files, appends `TaskCreated` (validated, fsync'd,
then published) and spawns the deliberation on the blocking pool. A live start
(`api/v1.rs::create_live_run`) looks its key up, passes the creation admission (shutdown,
rate, the visitor-run ring), then the gate's first phase (`live/gate.rs::Inner::verify_blocking`:
preset, geography, attestation, the evidence status, Turnstile, the lane), then takes the
single live slot, then the gate's second phase (`Inner::admit_blocking`: the daily quota,
the ledger reservation, the metered model client, the deliberation built over a full
re-check of the snapshot's files), then starts. The orchestrator calls the model over
blocking reqwest and the evidence client (a fake, the sealed snapshot, or on the operator
surface the Hyphae MCP child), and verifies proofs through the verifier subprocess.
Streams subscribe to the run's broadcast channel first, then replay the durable log, then
follow. The operator server runs the same handlers in its own process over its own
`RunManager`, behind the bearer token, on loopback, with `Host` and `Origin` checks; it
trusts no proxy header and keys limits on the TCP peer. The unit templates point both
processes at the same run-store directory, whose index lock and per-run locks serialize
them across processes (`run_store.rs`).

What is trusted where: `CF-Connecting-IP` and `CF-IPCountry` are read only when
`ServeConfig.client_key_policy == TrustedProxyHeader` (`api/mod.rs::trusted_country`,
`Client` extractor), which is the socket's default and cannot be switched off with the
socket (`public_config.rs`: `CONSENSUS_TRUST_PROXY_HEADERS=false` is an error there). The
`Idempotency-Key` header is trusted only as a per-owner replay key; the Turnstile token
only as something to hand to Siteverify under a fresh UUID. Nothing in a request body
names a project, a model, a task text or an identity on the public surface.

## 2. Assets and security goals

1. **The owner's Meta key and spend.** The key's value stays in the process that reads
   it (file first, zeroized, redacted; the key variables and the credential-path
   variables are removed from children, although a child running as the same user can
   still read the key file, T34); nothing a visitor does can spend it before the ledger
   has reserved the run's worst case; the daily, lifetime, judge, hourly, per-client and
   concurrency caps and the kill switch hold under crashes and restarts for every process
   that opens the ledger: `serve-public` always in live mode, `serve-operator` only when
   `CONSENSUS_LIVE_LEDGER_DIR` is set (T32).
2. **Integrity of the chains.** Every run's log is a genesis-anchored SHA-256 chain,
   fsync'd per append, with one writer, a tamper-evident run index and a read-only
   verifier anyone can run on a downloaded log and receipt. Integrity is not truth: a
   valid chain or proof says what was recorded, not that it is correct.
3. **Visitors' isolation and privacy.** A visitor reaches only their own runs (404 for
   everything else); the session id is held in memory only (`session.rs`; the run store
   keeps owner = SHA-256 of the id; no test scans the store for the id itself);
   decisions are attributed to `visitor:<16 hex>`, a domain-separated, truncated SHA-256
   of the id: an attribution, not a signature, and nothing verifies it); no free text of
   a visitor reaches a model; the only visitor free text stored is the optional decision
   note (at most 2,000 characters; control characters other than newline, tab and
   carriage return refused, and since `9bfe2e2` the bidirectional controls U+202A–U+202E
   and U+2066–U+2069 and the zero-width U+200B, U+2060 and U+FEFF refused; joiners,
   directional marks and other format characters accepted), kept in the run's chain,
   detail and receipt, and in a guided run's public `events.ndjson`, until the run is
   purged; visitor runs expire.
4. **Honesty of labels.** Model, evidence, source and delivery provenance are derived
   from the chained records, never from the request; LIVE is a per-frame property of a
   live model run; simulated proofs say SIMULATED in the record; a `supported` is shown
   with its policy trace, its open items and what older protocols dropped.
5. **The demo store's witnesses and the lab stores.** Witnesses (full data directories,
   WAL and key verifiers included) and proofs stay on the server and out of git (the
   template installs the snapshot tree `0700` with files `0640`, read-only to both units;
   no automated test checks the permissions); no container, proof or witness path reaches
   a view, receipt or error the server builds. The verbatim research archive at
   `/research` still carries the container paths the 2026-10-26/27 and 2026-10-30
   batteries recorded
   (accepted: they are not secrets, and the files are pinned byte for byte).
6. **Availability within the budget.** Bounded tables, bounded queues, bounded streams,
   bounded bodies, per-run deadlines, cancellation, graceful shutdown, a visitor-run ring,
   a request timeout; one damaged run never takes another offline (T39).

## 3. Threats and mitigations

Each threat names its mitigation, where it lives, the tests that cover it (exact names;
all exist and passed, see the header), and the residual risk, stated plainly.

### 3.1 Model layer

**T1. Model self-approval or outcome forgery.** Drafts (`DraftProposal`, `DraftChallenge`,
`DraftEvaluation`, `DraftRevision` in `crates/consensus-api/src/meta.rs`) have no outcome
or authorization fields; the recommendation is derived by `policy::explain`
(`crates/consensus-runtime/src/policy.rs`) and the state machine accepts
`HumanDecisionRecorded` only from `Actor::Human`
(`crates/consensus-runtime/src/state_machine.rs`, `RunAggregate::apply`). On the public
surface `authorized_by` and `authority` come from the authenticated identity, never the
body (`api/mod.rs::Identity`, `api/types.rs` with `deny_unknown_fields`).
Tests: `agent_cannot_self_approve`, `approval_before_awaiting_is_rejected`,
`replay_recomputes_recommendation_and_rejects_forged_kind`,
`requests_refuse_unknown_fields_and_free_strings`,
`a_decision_shows_the_authority_it_was_recorded_under`. Residual: none known for the
record; a human can still approve a bad recommendation (that is the design).

**T2. Prompt injection through retrieved memory, the task text, or another model's
output.** Every foreign text is inside an `<UNTRUSTED_*>` block (`meta.rs`:
`task_json`/`UNTRUSTED_TASK`, `proposal_block`, `evidence_block` as a JSON array of
`{"id","kind","integrity","content"}`, `objections_block`, `UNTRUSTED_EVALUATION`);
untrusted text is one neutralized line, bounded, with invisible and bidi characters
removed, angle-bracket look-alikes and full-width forms folded, and any `<` that would
open or close a delimiter written `&lt;` (`meta.rs::untrusted_line`, `json_string`,
`opens_untrusted_delimiter`). Rules live only in the system message. Two kinds of id sit
outside the blocks: the claim ids listed in the Evaluator's instruction and the record
ids of carried contradictions, each written as one neutralized line of at most 128
characters (`meta.rs::evaluate_prompt`, `ID_MAX_CHARS`); inside the evidence array a
record id is JSON-escaped but not length-capped (`evidence_block`). Tests:
`no_delimiter_variant_survives_in_any_untrusted_field`,
`the_task_is_an_untrusted_bounded_block_and_cuts_are_marked`,
`evidence_is_a_json_array_and_no_record_can_forge_another`,
`forwarded_model_text_stays_inside_untrusted_blocks_on_one_line`,
`every_role_keeps_its_system_rules_and_protocol_instructions`,
`untrusted_text_cannot_close_its_delimiter_block`, and at the policy level
`policy_never_authorizes_from_memory_content`,
`memory_prompt_injection_is_insufficient_evidence_however_valid_its_proof`. No test bounds
an evidence id's length. **Residual risk:** delimiters are defense in depth, not a
boundary. A hostile record stored through legitimate channels carries a valid proof, and
an Evaluator that obeyed it could corroborate on valid evidence and reach `supported`;
the human decision is the last line. Live evidence (I01/I02, H01/H02 in
`docs/evaluation.md`) shows the model treating such a note as contradicting evidence,
which is evidence, not a guarantee. A hostile record that contradicts legitimate ones
forces `contested` (fail-safe denial of support).

**T3. Fail-open evaluation (an omitted, misspelled, duplicated or unknown id silently
dropping an objection or a contradiction; audit MA-1, TC-2).** The Evaluator's strict
schema is built per call with one required key per claim id and per open item id
(`meta.rs::evaluation_schema`, `key_property`); the adapter maps an unknown, repeated or
missing decision-relevant id to `MalformedOutput` naming the field
(`meta.rs::assessments_from_draft`, `verdicts_from_draft`, `cleared_from_draft`); the
runtime validates every evaluation of every `ModelClient`, fakes included, before it is
recorded and again on replay (`crates/consensus-runtime/src/contract.rs::validate_evaluation`,
called from `orchestrator.rs::deliberate` and `state_machine.rs::RunAggregate::apply` for
schema 1.3 records). Tests: `the_evaluate_request_is_keyed_by_this_calls_claim_and_item_ids`,
`a_missing_key_is_malformed_output_naming_the_claim_or_item`,
`an_unknown_key_is_malformed_output_naming_it`,
`every_ma1_variant_fails_evaluate_and_never_reaches_supported`,
`evaluate_names_the_field_and_value_of_every_rejected_id`,
`a_scripted_evaluation_missing_one_verdict_fails_the_run`,
`every_claim_is_assessed_once_citing_only_retrieved_records`,
`claims_are_assessed_exactly_once_and_cite_only_retrieved_records`,
`every_expected_item_gets_exactly_one_verdict`, `unresolved_items_mirror_the_unresolved_verdicts`,
`a_forged_1_3_log_fails_replay`, `evaluations_are_checked_against_the_events_from_schema_1_3_only`,
`renamed_or_missing_draft_fields_are_malformed_never_empty`,
`structured_output_is_strict_and_every_list_is_required`. Live: K03 and K04 (`docs/evaluation.md`
§6.2) exercised the keyed schema; the empty `item_verdicts` object is still unexercised
live. Residual: pre-1.3 records are not held to the contract (by design: they replay as
recorded, `pre_1_3_records_are_never_held_to_the_contract`).

**T4. Earlier-round objections and contradictions forgotten (audit RT-1), and revision
scope drift (K03/K04).** Open items and uncleared contradictions are carried into every
later round's context (`state_machine.rs::RunAggregate::evaluation_context`, `open(k) =
open(k-1) − resolved(k-1) + items(k)`); silence never clears an item (`contract.rs`:
`carried_contradiction_judged`, `one_verdict_per_item`); progress is set-based
(`orchestrator.rs`). Receipts v2 list `earlier_round_objections_never_rejudged` for every
protocol (`crates/consensus-runtime/src/receipts.rs::build_receipt_v2`). Tests:
`rt1_round_two_silence_cannot_reach_supported`,
`rt1_round_two_silence_cannot_reach_supported_through_the_adapter`,
`each_round_context_carries_every_record_open_item_and_contradiction`,
`the_aggregate_carries_open_items_and_contradictions_across_rounds`,
`prompts_carry_open_objections_and_contradictions_across_rounds`,
`a_carried_contradiction_is_never_cleared_by_silence`, `progress_is_set_based_not_count_based`,
`recorded_supported_runs_depend_on_objections_never_judged_again`,
`only_the_final_listing_or_a_later_resolution_accounts_for_an_objection`. **Residual
risk, stated plainly:** the runtime does not judge whether a revision still serves the
task. In K03 and K04 (`prompt-freeze-2`, `docs/evaluation.md` §6.2) the reviser replaced
the asked-for change with the stored facts, every carried objection received a `resolved`
verdict, and the policy returned `supported` for a proposal that no longer does what the
task asked. That `supported` is honest only when shown with the superseded proposal, the
revision's `supersedes` and `addressed_items`, the verdicts and the unresolved
uncertainties. The record carries them (the receipt's deliberation lists each revision
with `supersedes` and `addressed_items`: `reference_receipt_shows_the_deliberation_and_the_dropped_objections`;
the recommendation keeps the final proposal's uncertainties and assumptions:
`the_recommendation_carries_the_final_proposals_uncertainties`), and the public detail
exposes them (`public_view::detail`: `current_proposal`, `challenge_items`,
`recommendation.trace`). The web run workspace (`4c42ae2`,
`apps/web/src/components/run/DecisionTab.tsx`) shows the scope change next to the
outcome (the original and revised proposals side by side, the rationale, the
assumptions, the uncertainties and the open objections); it does not render the policy
trace, and its tests were not run for this document. The counted protocol-1.3 battery ran
on 2026-10-30 (`docs/evaluation.md` §6.3; section 5): its one `supported` run, M03, is again
for a revision that departs from the task, and with 2 of its 4 runs lost to provider
errors no rate of `supported` is claimed.

**T5. Endless debate and token exhaustion.** `MAX_REVISION_ROUNDS = 2`
(`state_machine.rs`), a set-based no-progress exit, at most 12 claims per proposal
(`meta.rs`), at most `MAX_EVIDENCE_PER_RUN = 20` distinct records with every drop
recorded (`orchestrator.rs`), a per-run token cap (`RunLimits`, default 150,000; the
public live cap is 70,000) with missing usage treated as a violation, and a wall-clock
deadline per run (`CONSENSUS_RUN_DEADLINE_SECS`, default 900 s) checked at every
checkpoint between calls (`RunControl`, `control.rs`). Tests:
`two_revision_rounds_are_reachable_and_bounded`, `revision_beyond_round_limit_is_rejected`,
`no_progress_exits_instead_of_looping`, `proposal_claim_count_is_bounded`,
`evidence_cap_is_strict_recorded_and_retrieval_is_runtime_attributed`,
`evidence_cap_reached_in_round_one_keeps_round_two_at_the_carried_records`,
`token_budget_exceeded_fails_run_safely_with_recorded_reason`, `missing_token_usage_fails_closed`,
`expired_deadline_fails_the_run_with_stage_deadline`. Residual: the token cap is checked
after each call, so a run can overshoot by one call's tokens (the live ledger's
reservation includes one `max_completion_tokens` for that reason). The deadline is
checked between calls, and one call's budget is `CONSENSUS_MODEL_TIMEOUT_SECS` (1–3600 s,
default 120 s), so a run can overrun its deadline by one call in flight; no test runs a
call past the deadline. The 60 s shutdown grace is shorter than one default call (T43).

**T6. Silent degradation (timeouts, malformed output, truncated or refused output,
missing fields).** Every model failure fails the run with a stable code next to its reason
(`orchestrator.rs::codes`, `meta.rs::codes`); `finish_reason: length` is
`model_output_truncated`, `message.refusal` is `model_refusal`; label-only Critic items and
empty claims are malformed; a call refused after the provider answered keeps its
invocation on the record (TC-7). Tests: `model_timeout_fails_run_safely_with_reason`,
`malformed_model_output_fails_at_that_stage`, `label_only_challenge_items_are_malformed`,
`empty_claim_statement_is_malformed`, `truncated_output_has_its_own_code_and_keeps_the_billed_call`,
`a_refusal_has_its_own_code_keeps_the_call_and_never_its_text`,
`every_refused_answer_keeps_its_invocation`, `an_answered_call_whose_body_timed_out_is_recorded_and_charged`,
`every_failure_records_a_stable_code_next_to_its_reason`,
`an_answered_call_whose_output_was_refused_stays_on_the_record`. Residual: none known
beyond T2–T5.

### 3.2 Evidence layer

**T7. Integrity/truth confusion.** Orthogonal axes; `supported` requires every claim
corroborated by a `Valid` record it cites itself, no contradicting record, no invalid
proof anywhere (`policy.rs::explain`, one implementation with a frozen differential
copy). Every receipt, the public ones included, carries
`RunReceipt::DEFAULT_LIMITATIONS` as its `proof_limitations`
(`crates/consensus-domain/src/receipt.rs`, set by `receipts.rs::build_receipt_v2`; the
public `sanitize_receipt` keeps it). Since `821c217` it says that a valid proof means the
verifier re-executed the request the proof sealed against its witness and accepted it at
the anchor it was given; that it covers that request's whole result (for the sealed
snapshot, the complete record list of the snapshot's project at that anchor), never a
claim, even when the request's query is a claim's text (this clause reworded on
2026-10-29, ADR-0008 decision 4; it used to say "not the claim that led to the
retrieval"); that a record's text is not byte-compared with the proof; that a
simulated proof checks nothing; and that proofs do not establish truth, relevance, completeness or a model's
reasoning. The verifier's own detail says the same on the operator surface; public views
drop the detail of every real verifier (`public_view::verification_v1`,
`sanitize_receipt`). Tests: `valid_proof_does_not_imply_corroboration`,
`no_checked_evidence_is_never_supported`,
`corroborated_claim_without_own_evidence_is_not_supported`,
`contradicting_evidence_prevents_supported_even_if_not_relevant`,
`explain_agrees_with_the_frozen_derivation_on_random_cases`, `policy_properties_hold_on_random_cases`;
the limitations text: `receipt_preserves_exact_model_identity_and_limitations` (checks
the truth clause) and `committed_contract_matches_the_code`, which pins the whole text
through the committed public receipt fixtures (`contracts/api-v1/fixtures/receipt-v2-*.json`);
no test asserts the byte-comparison clause by itself. Residual: the records of the
sealed snapshot are operator-seeded synthetic facts; a proof says they are the
snapshot's records, not that they are true or that a model used them.

**T8. A `Valid` label not bound to the record it labels (audit HY-1).** `Valid` needs
exit 0 and a report with `status: verified`, `semantic_reexecution_performed: true`,
`scope: semantic_reexecution`, `kind: memory` and an `anchor_digest` equal to the anchor
given; a verified proof of another kind or anchor is `Invalid`, an incomplete report a
`VerificationError` (`crates/consensus-api/src/hyphae.rs::HyphaeProofVerifier::check_artifacts`).
Tests: `valid_requires_a_reexecuted_memory_proof_bound_to_the_anchor_given`,
`exit_zero_without_a_verified_reexecution_report_is_not_valid`,
`verifier_failure_categories_captured_live_map_to_integrity`,
`proof_verifier_maps_exit_codes_to_integrity_only`. **Residual risk:** the record text
handed to models is not byte-compared with the proof (every receipt's
`proof_limitations` and the operator's verifier detail say so, T7); a buggy or
compromised backend could pair altered text with a valid proof of the same recall. For
the snapshot, the response file that holds the record texts is bound to the manifest by
its `response_sha256` (since `16cafd3`) and the manifest to a published digest when it is
pinned (T10), never to the proof itself.

**T9. Corrupted, mismatched or failed proofs.** Any invalid proof in the retrieved set
blocks, without a revision attempt; a corruption-class proof-generation failure is
`Invalid` (never downgraded) with an honest note that Hyphae's `corruption` code also
covers capacity and configuration failures (`orchestrator.rs::upstream_proof_check`,
`GENERIC_CORRUPTION_NOTE`); transport failures (exit 125–127, spawn errors, timeouts) are
`VerificationError`, never tampering (`hyphae.rs`, `HyphaeProofVerifier::verify`). Tests:
`invalid_proof_anywhere_in_retrieved_evidence_blocks`,
`invalid_proof_blocks_without_revision_attempt`, `abstention_with_invalid_proof_is_blocked_and_replayable`,
`upstream_corruption_signal_blocks_and_is_never_downgraded`,
`non_corruption_proof_failure_is_unavailable_not_supported`,
`the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it`,
`transport_exit_codes_are_verification_errors_not_invalid` (exit 125),
`proof_verifier_maps_exit_codes_to_integrity_only` (a spawn error, and exit 1 as
`Invalid`), `missing_proof_artifacts_are_unavailable_never_valid`,
`a_failed_proof_travels_typed_next_to_its_text`. Exits 126 and 127 have no test.
Residual: a healthy store past its proof-size ceiling reads as `Invalid` (fail-closed;
the detail says why; a distinct `generation_failed` status is a post-hackathon decision).

**T10. Snapshot swap, tamper or replay (ADR-0008).** *Load*
(`crates/consensus-api/src/snapshot.rs::Snapshot::load_pinned`): the directory is walked
without following symbolic links (any link is `symlink_refused`) and no deeper than the
flat layout (`MAX_DEPTH = 2`, `layout_too_deep`; both since `29b95aa`); `SHA256SUMS` must
list every file and match; `manifest.json` must hash to `CONSENSUS_SNAPSHOT_MANIFEST_SHA256`
when that pin is set (`manifest_pin_mismatch`, checked before parsing); every field and
the directory name (`snapshot_id`) are checked; every capture file (response, proof,
witness, capture-time report) must match the manifest's digest before it is parsed, and
only the hashed bytes are parsed (`response_sha256` and `verify_sha256` since `16cafd3`,
`digest_mismatch`); one anchor everywhere (manifest, response, verify record); exactly
two required captures with distinct proof bytes and distinct `proof_blake3`
(`proof_shared`); exactly `record_count` records, each attributed to the served project or
to `_global`; plain relative paths only, files over 64 MiB refused (`file_too_large`, no
test). *Boot* (`live/snapshot_factory.rs::SnapshotFactory::open`, only with live mode
enabled): the project must be the presets' `demo-repo` (`project_mismatch`); with
`CONSENSUS_HYPHAE_BIN_SHA256` set, `CONSENSUS_HYPHAE_BIN` must be an absolute path, the
file must hash to the pin and the pin must equal the manifest's `hyphae.binary_sha256`
(`snapshot.rs::check_verifier_binary`: `verifier_binary_mismatch`,
`capture_binary_mismatch`), all before any verifier run;
`CONSENSUS_HYPHAE_EXPECTED_ANCHOR` must equal the manifest anchor; then the verifier runs
on both served captures, which must be `Valid` as `memory` proofs at
`semantic_reexecution` against that anchor with the `proof_digest` and `result_digest`
of the capture-time report (`SnapshotMemoryClient::from_snapshot`, `verify_capture`).
With live mode on the unix socket both pins are required, or `serve-public` does not
start (`server.rs::require_snapshot_pins`, since `e915b6d`; the real binary is tested by
`with_live_mode_enabled_on_the_socket_serve_public_requires_both_pins`, unix-only, since
`25c7867`); a development TCP bind may
run unpinned, and the startup line says which pins are set. *After boot*:
`SnapshotMemoryClient::check_artifacts` re-hashes `manifest.json` and the four files of
each served capture against the digests boot accepted; every admission runs it in full
(`SnapshotFactory::build`, before any model call), and `/ready`, `/capabilities` and the
gate's pre-Turnstile check read it cached for at most 2 s (`READY_CACHE_TTL`). Every run
gets a fresh verifier wrapped in `snapshot.rs::BootBoundVerifier`: a `Valid` stands only
when the report's `proof_digest` and `result_digest` are the ones boot accepted for the
record's `proof_ref`, else `Invalid` with the fixed detail `NOT_THE_BOOT_ARTIFACTS` (since
`4da2b1b`; before it, the isolation capture's proof and witness copied over the list's
verified `Valid`). *Failure modes*: a configuration error (a missing or malformed anchor
or pin, a relative pinned binary path) stops startup naming its variable; a snapshot
that does not load, match its pins or verify leaves the portal up with live mode
unavailable (`UnavailableEvidence`: admissions `503 live_unavailable` before Turnstile,
`/ready` `live.evidence_ok: false`, `/capabilities` `live_enabled: false`); the stable
code is logged once per change between ok and not ok, never a path or a file's
contents. Tests (`snapshot_client`): `a_well_formed_snapshot_loads_and_opens_after_verifying_both_captures`,
`tampering_with_any_file_fails_open_with_its_code`,
`a_record_text_edited_with_the_listing_regenerated_does_not_load`,
`the_pinned_manifest_digest_refuses_every_other_manifest`,
`the_digest_pins_are_validated_and_a_pinned_binary_must_be_an_absolute_path`,
`a_pinned_verifier_binary_must_be_the_build_the_snapshot_was_captured_with`,
`the_pinned_anchor_must_be_the_manifest_anchor`, `the_snapshot_config_requires_the_pinned_anchor`,
`a_set_snapshot_dir_with_a_missing_or_malformed_anchor_is_a_boot_error`,
`the_required_captures_must_be_present_and_unique`,
`boot_verification_requires_a_valid_memory_proof_at_the_anchor`,
`boot_verification_requires_the_manifest_anchor_from_the_verifier_itself`,
`one_proof_cannot_stand_for_both_served_captures`,
`a_symlink_loop_under_the_directory_is_refused_not_followed` (a junction on Windows
without symlink privilege), `a_symlinked_artifact_pointing_outside_the_directory_is_refused`
(returns early without symlink privilege, as it did here),
`a_directory_nested_deeper_than_the_flat_layout_is_refused`,
`status_rehashes_the_served_artifacts_on_every_call`,
`the_served_responses_and_reports_are_rechecked_after_boot`; (unit)
`identifiers_digests_and_paths_are_checked_strictly`, `checksum_lines_follow_the_sha256sum_format`,
`an_unhealthy_detail_becomes_a_stable_code_and_never_a_path`,
`a_snapshot_that_did_not_boot_is_unavailable_with_its_code_logged_once`,
`live_mode_on_the_socket_requires_both_snapshot_pins`; (`live_snapshot`)
`a_tampered_snapshot_does_not_boot_and_live_mode_stays_refused_without_a_factory`,
`files_changed_after_boot_make_live_mode_unavailable_at_once_without_a_path`,
`a_record_text_or_a_report_edited_after_boot_makes_live_mode_unavailable_at_once`,
`the_isolation_artifacts_copied_over_the_list_capture_are_never_verified_valid`,
`the_status_is_cached_and_every_admission_rechecks_the_files_in_full`,
`a_snapshot_that_fails_boot_verification_leaves_the_portal_up_with_live_mode_unavailable`,
`a_snapshot_of_another_project_does_not_boot_for_live_mode`,
`a_proof_tampered_with_consistently_is_refused_by_the_verifier_on_every_platform`,
`a_snapshot_that_breaks_its_binding_leaves_the_portal_up_with_live_mode_unavailable`,
`the_startup_line_says_whether_the_manifest_and_the_verifier_binary_are_pinned`,
`the_fixture_snapshot_is_a_complete_directory`; (`live_gate`)
`startup_refuses_live_mode_until_the_snapshot_client_is_wired` (no factory, no live
mode), `the_snapshot_factory_is_absent_without_its_directory_and_fails_closed_without_the_anchor`;
and the real `serve-public` binary on a development bind:
`with_live_mode_disabled_serve_public_never_reads_the_snapshot_and_starts`,
`with_live_mode_enabled_serve_public_starts_on_a_snapshot_that_does_not_boot_and_refuses_bad_settings`,
`with_live_mode_enabled_serve_public_boots_pinned_and_refuses_an_unusable_pin`. All of
them run over a fixture snapshot and a fake `hyphae` script, not a real capture. The
capture side binds every file to its capture through digests recorded the moment each
file was written (`scripts/hyphae-container/capture-snapshot.sh`, `captures.index`);
`scripts/snapshot_manifest.py build` copies those digests into the manifest
(`response_sha256`, `verify_sha256`; `882599d`), `check` requires and compares them and
refuses symbolic links and nesting like the runtime (`99c1b09`), and
`check --reverify` requires the verifier to reproduce the captured reports. Since
`29b95aa` the capture script aborts the session on a seed error (a JSON-RPC error, an
`isError` or tool-error answer), refuses a raw control character inside a JSON string and
a NUL byte in the JSON it parses, and traps INT, TERM and HUP to stop the daemon in
bounded time (SIGINT, then SIGTERM after 10 s, then SIGKILL after 10 s) before
re-raising the signal.
The two self-tests (`snapshot_manifest.py selftest`, `capture-snapshot.sh --selftest`) run
in CI and in `scripts/linux-gates.sh`; they were not run for this document. **Residual
risk:** the verifier binary is hashed at boot only, so one replaced while the server
runs, or a wrapper that prints the boot-accepted reports, passes every later run
(`BootBoundVerifier` compares report digests, which such a wrapper reproduces) until the
next start; unpinned (a development bind), a consistent rewrite of `manifest.json`,
`SHA256SUMS` and the capture files loads, and only a comparison with the committed and
published digests detects it; the pinned anchor detects another store's artifacts, not a
lying server; `proof_blake3` cannot be recomputed here (no BLAKE3 in the workspace), and
the record texts are bound to the manifest, not to the proofs (T8). The capture session
took place on 2026-10-29 (`b0375df`); the anchor, the manifest digest and the binary digest
are published in the README ("Published snapshot digests"), and the binary digest is not yet
matched against the release asset (section 5).

**T11. Witness exposure.** Witnesses and proofs are never served: `public_view::evidence_v1`
drops `proof_file`, `witness_file`, `backend_ref` and the `proof_error` text on the public
surface (a boolean `proof_error_reported` stays); `hyphae.rs::artifact_ref` and
`program_name` keep directory paths out of the proof references and error texts they
persist (file names only), while `EvidenceRetrieved` records persist `proof_file` and
`witness_file` with their full paths (for the operator, and dropped by the public views);
`.gitignore` excludes `*.proof`, `*.witness`, `*.hyproof`, `*.hywit`; the deployment
template installs the snapshot tree `0700` with files `0640`, read-only to both units
(runbook; `deploy/scripts/sandbox-probe.sh` is the manual probe). Tests:
`public_views_strip_internals_and_operator_views_keep_them`,
`artifact_refs_never_carry_a_path`, `public_receipts_keep_only_the_public_invocation_fields`,
`a_live_run_over_the_sealed_snapshot_is_proven_verified_and_published_without_paths` (no
proof, witness or container path, no artifact contents and no key in any public response
of a live run over the snapshot). No test checks file permissions on a deployed server
(G9, pending). Residual: the operator surface shows paths by design; the verbatim public
`events.ndjson` would carry them, which is one reason it is withheld for live-declared
runs (T24); and the verbatim research archive at `/research` carries 652 container paths
(`/hyphae-home/.local/state/hyphae/proofs/recall-*.proof` and `.witness`) in 35 data files
(540 in 27 files of the 2026-10-26/27 batteries, 112 in the 8 files of 2026-10-30; residual
5), served by `api/v1.rs::research_file` (accepted: the paths
are not secrets, the files are pinned byte for byte by `research/MANIFEST.json`, and
editing them would break that pin).

**T12. Verifier subprocess hangs, floods or inherits secrets (audit HY-2, MA-4).**
`SystemCommandRunner::run` kills the child past the deadline (default 60 s,
`CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS`, 1–3600) and caps each output stream;
`hyphae.rs::hyphae_command`, which both the verifier and `hyphae mcp` use, removes the
three Meta key variables (`META_KEY_VARIABLES`) and, since `6fb4ac3`, the five
credential-path variables (`CREDENTIAL_PATH_VARIABLES`: `CREDENTIALS_DIRECTORY`,
`CONSENSUS_META_API_KEY_FILE`, `CONSENSUS_TURNSTILE_SECRET_FILE`,
`CONSENSUS_OPERATOR_TOKEN_FILE`, `CONSENSUS_LIVE_JUDGE_CODES_FILE`). Results are cached by
the SHA-256 of the proof and witness bytes and the anchor, never by path, and never for a
`VerificationError` (HY-13); in the public live mode every run gets a fresh verifier, so
that cache never outlives a run. Tests:
`a_verifier_past_its_deadline_is_a_verification_error_with_its_code`,
`the_system_runner_kills_a_command_past_its_deadline`, `command_output_is_capped_and_marked`,
`hyphae_children_never_inherit_the_meta_key` (sets all eight variables and runs a child
through `SystemCommandRunner`; the `hyphae mcp` spawn path shares the helper but is not
tested separately), `results_are_reused_by_artifact_bytes_never_by_path`,
`one_proof_covering_several_records_is_verified_once`,
`every_run_verifies_the_proofs_again_with_a_fresh_verifier`. **Residual risk:** the
environment is not cleared (`env_clear()` is not called), so the rest of the server's
environment reaches the child, and the child runs as the server's user, so it can open
the credential files at their predictable path (`/run/credentials/<unit>/meta.key`):
withholding the variables hides the paths, it does not make the files unreadable. Only
the direct child is killed, so a wrapper's own children (the Windows `cmd.exe` →
`docker exec` path) can survive; the unit templates point `CONSENSUS_HYPHAE_BIN` at the
native binary, and `KillMode=mixed` kills the rest of the process group only when the
service stops. Hyphae extracts witnesses to a temporary directory that its destructor
removes; a killed verifier leaves it behind. No sweep exists in this tree; the unit's
`PrivateTmp=yes` discards the private `/tmp` only when the service stops, so a leftover
copy stays until the next stop or restart (the checklist's G9 wording "temp-directory
sweep tested" is not met; section 5).

**T13. Untrusted or unnormalized proof anchor (audit HY-10, ADR-0003 decision 6).**
The pinned anchor is trimmed, lowercased and required to be 64 hex digits at load
(`config.rs::normalize_anchor`, `HyphaeConfig`, `SnapshotConfig`); a record anchored
elsewhere is `Invalid` without running the verifier, and the pinned value is what the
verifier receives; the snapshot mode requires the pin and requires it to equal the
manifest anchor. `SnapshotConfig::from_env`, which folded errors into "not configured",
is gone (`29b95aa`): with `CONSENSUS_SNAPSHOT_DIR` set, a missing or malformed anchor is
an error naming the variable. With live mode disabled, `serve-public` does not read the
snapshot variables at all (one notice when `CONSENSUS_SNAPSHOT_DIR` is set). Tests:
`the_pinned_anchor_is_normalized_and_validated_at_load`,
`the_pinned_anchor_is_normalized_and_handed_to_the_verifier`,
`pinned_anchor_mismatch_is_invalid_without_running_verifier`,
`the_snapshot_config_requires_the_pinned_anchor`,
`a_set_snapshot_dir_with_a_missing_or_malformed_anchor_is_a_boot_error`,
`every_run_verifies_the_proofs_again_with_a_fresh_verifier` (each run's verifier gets the
pinned anchor), `with_live_mode_disabled_serve_public_never_reads_the_snapshot_and_starts`.
**Residual risk:** the anchor is read from the same store at capture and pinned by the
operator; pinning detects change, not a lying server. Without a pin (operator lab only)
`Valid` means self-consistency against the backend-reported anchor, and the detail says
so.

**T14. Cross-project memory leakage (audit SEC-3, TC-4).** The public surface has no
project field at all (presets are `demo-repo`; `requests_refuse_unknown_fields_and_free_strings`);
the sealed snapshot serves one project, answers the isolation project with a proven empty
recall and any other project with a recorded `EvidenceUnavailable`
(`snapshot.rs::recall_with_proof`), and a snapshot of another project does not boot for
live mode (`project_mismatch`); the live adapter keeps the backend's `project` and
`scope` on every record and returns records of another project apart, which the
orchestrator drops with a recorded reason (`hyphae.rs::map_record`, `Recall::foreign`,
`orchestrator.rs::codes::FOREIGN_RECORD`). Tests: `a_recall_of_the_served_project_is_the_whole_proven_list_whatever_the_query`,
`a_recall_of_the_isolation_project_is_empty_with_its_own_proof`,
`a_recall_of_an_unknown_project_is_a_backend_error_with_a_reason`,
`a_snapshot_of_another_project_does_not_boot_for_live_mode`,
`records_keep_their_backend_attribution_and_foreign_ones_are_dropped`,
`the_orchestrator_records_every_proof_and_every_dropped_record`,
`foreign_records_are_dropped_and_proofs_recorded_even_when_empty`,
`fake_memory_honors_project_isolation_like_hyphae`. Residual: the operator surface still
takes a caller-chosen project by design (authenticated operator, lab stores); the live
isolation evidence is two empty recalls (L10, R08) plus the snapshot's captured empty
recall `recall-other-repo-isolation` (captured 2026-10-29, `b0375df`; verified by
`hyphae proof verify`).

### 3.3 Public surface

**T15. Cross-visitor isolation, session flushing and stolen cookies.** Sessions are
256-bit random ids (`crates/consensus-api/src/session.rs`, `getrandom`), minted by
`POST /api/v1/session` only, in a bounded LRU table (`CONSENSUS_SESSION_MAX`, 50,000)
with a fixed TTL from creation (`CONSENSUS_SESSION_TTL_SECS`, 86,400 s); a run's owner is
`Owner::session(id)` = SHA-256 of the id, and reads, streams, receipts, verbatim logs,
decisions and cancels go through owner-checked paths (`runs.rs::RunManager::snapshot`,
`owned_meta`, `decide`, `cancel`; `run_store.rs::read_run`, `open_run_as`, `meta_for`)
that answer the same `404` for a run that is not owned, not indexed, purged, expired or
malformed. `GET /api/v1/runs` lists the owner's runs only. Tests:
`another_session_sees_nothing_of_a_run`,
`a_non_owner_is_told_nothing_and_never_touches_the_runs_lock_or_log`,
`a_stored_run_is_read_by_its_owner_alone_and_reading_changes_nothing`,
`owner_checked_paths_refuse_a_run_once_its_owners_access_has_ended`,
`list_for_returns_only_the_owners_live_runs_oldest_first`, `meta_for_checks_owner_and_access_without_touching_the_log`,
`sessions_are_256_bit_hex_and_owner_hashes_differ_from_ids`, `lookup_accepts_only_live_well_formed_ids`,
`the_table_is_bounded_with_lru_eviction`, `expired_sessions_are_refused`,
`concurrent_guided_runs_stay_isolated_and_ordered`. **Residual risk:** sessions are
minted without Turnstile (the `session` widget action is configurable, but no route
verifies a session token), at 10 per minute per client key, so a client with many
prefixes can fill the 50,000-entry table and evict every visitor's session (they are
logged out; their runs stay stored but unreachable until purged). A stolen cookie is a
bearer credential for the rest of its 24 h with no binding to the client;
`DELETE /api/v1/session` is the only revocation. A visitor who clears cookies loses their
runs (by design), and the table is in memory, so a restart logs every visitor out.

**T16. CSRF, DNS rebinding, Host and Origin confusion (audit SEC-11, SEC-14).**
`api/mod.rs::host_check` answers `421 host_not_allowed` for any `Host` (or HTTP/2
authority) outside `CONSENSUS_ALLOWED_HOSTS` (required with the socket; derived from the
loopback bind in development); `csrf_check` requires an allowlisted `Origin` on every
state-changing `/api` request of the public surface (`403 csrf_rejected`), rejects
`Sec-Fetch-Site` other than `same-origin`, and requires `Content-Type: application/json`
where a body is read (`415`); the operator surface rejects a foreign `Origin` and tolerates
a missing one (scripts). The cookie is `__Host-hc_session; HttpOnly; Secure;
SameSite=Strict; Path=/` (named `hc_session` without the prefix only when
`CONSENSUS_COOKIE_SECURE=false`, never `__Host-` without `Secure`). Tests:
`csrf_content_type_unknown_fields_and_size_are_enforced` (403, 415, 413, 421, 422),
`host_and_origin_are_checked`, `sessions_are_minted_by_post_only_and_carry_the_right_attributes`,
`a_host_prefixed_cookie_is_never_emitted_without_secure`,
`api_paths_and_bodyless_posts_are_recognised`, `non_loopback_public_bind_is_refused`,
`socket_mode_requires_explicit_origins_and_hosts` (unix-only). Residual: `GET` routes are
not CSRF-relevant but are readable cross-site only through the cookie, which
`SameSite=Strict` withholds.

**T17. Session fixation and cookie planting.** No `GET` and no static response sets a
cookie; `POST /api/v1/session` with a live cookie returns the existing session without
minting; the `__Host-` prefix forbids a `Domain` attribute, so a sibling host cannot plant
one; ids are looked up only if well-formed 64-hex. Tests:
`sessions_are_minted_by_post_only_and_carry_the_right_attributes`,
`lookup_accepts_only_live_well_formed_ids`. Residual: none known beyond T15.

**T18. SSE floods and replay traps (audit SEC-7, RT-12).** A stream needs the owner
(checked without reading the log), passes the per-client stream rate (30/min), takes a
permit under three caps (8 per run, 8 per client key, 512 per process;
`limits.rs::StreamCaps`), subscribes to the run's bounded broadcast channel before the one
full read, replays from `Last-Event-ID` (or `?after`), follows, dedupes by sequence, resyncs
from the durable log on a lag, gap or close, sends a heartbeat comment every 15 s and a
`stream_end` once the run rests or ends (`crates/consensus-api/src/stream.rs::run_stream`,
`api/v1.rs::run_events`). Tests: `sse_resumes_from_every_sequence_without_duplicates_or_gaps`,
`a_lagged_subscriber_reconciles_from_the_log`, `rates_streams_and_client_keys_are_enforced_per_slash_64`,
`stream_caps_count_and_release`, `a_stream_dedupes_fills_gaps_from_the_log_and_ends_once_at_the_terminal_head`,
`subscribing_first_then_snapshotting_misses_nothing_while_appends_race`,
`a_disconnected_client_never_stops_a_run`. Residual: a stream's body is not covered by
the request timeout once its headers are sent (by design); Cloudflare's 125 s read
timeout is covered by the heartbeat, which has not been measured through a real tunnel
(G11; `deploy/scripts/sse-heartbeat-test.sh` is the procedure, not yet run).

**T19. Oversized or malformed input (audit SEC-8, UI-5).** `DefaultBodyLimit` of 16 KiB
on every route (`413 payload_too_large`); every request type uses `deny_unknown_fields`
and enums, with no silent defaults (`api/types.rs`); public requests carry a scenario id
or a preset id, `attest_adult` and an optional judge code (1–`JUDGE_CODE_MAX_CHARS`
characters once trimmed, no control characters), never a title, description, project or
identity; decision notes are at most 2,000 characters through `api/types.rs::check_text`
(control characters other than newline, tab and carriage return refused; since `9bfe2e2`
also U+202A–U+202E, U+2066–U+2069, U+200B, U+2060 and U+FEFF; joiners and directional
marks accepted); operator requests bound their fields with the same check; the
`Idempotency-Key` is 1–128 characters of `[A-Za-z0-9_.:-]`; list pages are 1–50.
Deserialization errors return `422 invalid_request` with the first 200 characters of
serde's message (`api/mod.rs::ApiJson`, `bounded_detail`); that message names the field
and may quote the offending value from the body (`unknown variant`, `unknown field`,
`invalid type: string "..."`), and query-string rejections of `GET /api/v1/runs` and
`/events` are echoed whole through `body_text()` (`api/v1.rs::list_runs`, `run_events`).
Tests: `csrf_content_type_unknown_fields_and_size_are_enforced`,
`requests_refuse_unknown_fields_and_free_strings`, `stored_text_refuses_hidden_format_characters`,
`operator_request_bounds_its_fields`, `modes_are_explicit_and_credentials_are_required`,
`the_module_exposes_no_way_to_carry_visitor_text`. Residual: up to 200 characters of the
caller's own body can come back to the caller (JSON, `nosniff`; never to anyone else);
format characters other than the refused ones (an invisible operator, a soft hyphen, the
tag block) are stored in a note.

**T20. Duplicate runs and duplicate decisions on retry (audit UI-1, TC-8, UI-7, RT-8,
SEC-12).** A creation may carry an `Idempotency-Key`: the same key with the same body
(SHA-256 of the parsed body) returns the run already made, a different body is `409
idempotency_key_reused`, and keys are scoped per owner in a bounded LRU
(`api/mod.rs::CreationKeys`). Since `f24d05f` a guided or operator creation claims its key
before admission and the start (`CreationKeys::claim`, `api/v1.rs::claim_creation`): a
duplicate with the same body waits for the claimant and answers with its run, or claims
the key itself when that creation failed; a claim dropped unfinished frees the key. The
live route keeps a lookup without a claim (`replayed_creation`): concurrent duplicates all
reach the gate, where one solved Turnstile challenge admits one run (T28). A decision
requires an `Idempotency-Key` and an `expected` block (recommendation kind and last
sequence); a mismatch is `409 decision_conflict`, a run still deliberating `409
run_not_awaiting_decision`, a reused key with another choice `409 idempotency_key_reused`
(`runs.rs::RunManager::decide`; the event log refuses the same id with different content).
Tests: `a_retried_creation_with_the_same_key_returns_the_same_run`,
`concurrent_duplicates_of_one_guided_submit_start_one_run`,
`a_refused_guided_creation_frees_its_key`,
`concurrent_duplicates_of_one_operator_submit_start_one_run`,
`concurrent_duplicates_of_one_submit_start_one_run` (live),
`creation_keys_are_scoped_per_owner_and_bounded`, `a_claimed_key_is_held_until_its_creation_ends`,
`a_guided_run_is_accepted_streamed_decided_and_receipted` (409 cases),
`two_concurrent_decisions_record_exactly_one`,
`a_decision_during_deliberation_is_refused_and_live_frames_say_live`,
`two_handles_racing_two_decisions_record_exactly_one`, `a_decision_race_on_a_stored_run_records_one_decision_and_one_terminal_head`,
`reused_event_id_with_different_content_is_a_conflict`, `approval_retry_with_same_idempotency_key_is_noop`,
`reusing_an_idempotency_key_across_runs_records_both_decisions`. Residual: the creation
key table is in memory and bounded, so after a restart or eviction a retried submit makes
a second run (bounded by the rates and, for live runs, by Turnstile and the ledger).

**T21. Geo evasion, unknown countries and cached responses (ADR-0007 duty 7).**
`api/mod.rs::portal_geo` refuses every public route (static pages and `/research`
included) with `451 geo_blocked_portal` for the 23 portal codes and `451 geo_unknown` when
`CF-IPCountry` is absent behind the socket; the live gate additionally refuses South Korea
and every unknown value (`XX`, `T1`, malformed, missing) with `451 geo_blocked_live`
(`live/geo.rs::GeoPolicy::decide`, `live/gate.rs`), before the attestation and Turnstile.
Lists come from `CONSENSUS_GEO_*` with the owner's defaults. Tests:
`the_portal_block_covers_every_route_and_fails_closed_without_a_country`,
`korea_blocks_live_mode_only_and_an_unknown_country_blocks_it_too`,
`ukraine_is_blocked_on_the_whole_portal`, `south_korea_is_blocked_for_live_mode_only`,
`hong_kong_and_macao_are_blocked_like_the_other_restricted_territories`,
`unknown_country_is_refused_for_live_and_allowed_on_the_portal`,
`allowed_countries_pass_both_surfaces_and_lowercase_input_is_normalized`, `custom_lists_drive_the_policy`,
`default_lists_match_the_owner_decision`, `list_parsing_refuses_junk_and_empty`,
`geo_codes_are_the_geo_modules_fixed_public_codes`. **Residual risk:** country comes
from Cloudflare's geolocation of the client address; a VPN or proxy defeats it (an
accepted limit of any geo-block). A development bind that trusts no proxy header blocks
nothing on the portal and refuses every live run (`geo_blocked_live`, asserted by the
Korea test). Responses the edge serves from its cache never reach the origin's check:
`/research` files carry `public, max-age=86400`, `/_next/static/` assets
`public, max-age=31536000, immutable` and other static files `public, max-age=3600`
(`api/v1.rs::research_file`, `static_files.rs`), so for those only the edge rule applies.
The Cloudflare rule is the primary control and has not been configured yet (G10).

**T22. Provenance forgery (a scripted run shown as live; audit UI-4, SEC-16, LC-05).**
Provenance is computed from the chained records: model from the recorded invocations
(`fake` → `scripted`, `meta` with request ids → `live_meta`, else `mixed`/`undetermined`),
evidence from retrieval and verification records, delivery per SSE frame; the declared
`Task.mode` is only cross-checked and a disagreement sets `provenance_inconsistent`
(`public_view.rs::provenance`); simulated verifications carry `simulated: true` and keep
their SIMULATED detail; a resting run is never `active` whatever the server claims
(`can_be_active`). Tests: `provenance_is_derived_from_records_and_cross_checked`,
`a_resting_run_is_never_active_whatever_the_server_claims`,
`a_decision_during_deliberation_is_refused_and_live_frames_say_live` (every frame of a
live run says `delivery: live`, replayed ones `replay`),
`the_meta_key_never_appears_in_any_output` (asserts `undetermined` for a declared Meta run
with no call), `a_protocol_label_alone_never_hides_dropped_objections`. The web run
workspace (`4c42ae2`) renders the axes (`apps/web/src/components/ProvenanceBadges.tsx`,
`apps/web/src/lib/provenance.ts`): the LIVE word only through `isLive`, and simulated
proofs marked as such instead of a green "valid". Its Vitest and Playwright suites were run in Docker by the
lane (recorded at `cc7b9ab` against contract `cc890dc030ba`: Vitest 333 passed and 15
skipped, Playwright 115 passed, axe 204 scans without a violation), not for this
document. Residual: at `e915b6d` the web client's generated types still name contract
`cc890dc030ba` while the server publishes `465f4117` (`561c25e` added the error code
`live_client_quota` and `index_error_code` in `/ready`), so its drift check fails until
the types are regenerated (section 5); nothing has been checked through the public
hostname (G7 pending).

**T23. Error and detail leakage (audit SEC-9, MA-4, DOC-6).** One error envelope
(`api/error.rs::ApiError`: stable `code`, fixed `message`, optional `retry_after_secs`),
internal errors logged server-side and answered as `internal`; public views replace
recorded failure reasons with fixed statements per code (`public_view::failure_message`,
`unavailable_message`), drop verifier output, `proof_error` text, backend references,
paths and the prompt and schema digests; live-mode refusals answer fixed sentences, never
the ledger's or a factory's text (`live/gate.rs::refusal_message`); the snapshot's
failure reaches the public only as `live.evidence_ok: false` in `/ready`, its code going
to the log; `/api/v1/health` is static facts only; `/api/v1/ready` names a run-index
failure by a stable code (`index_tampered`, `index_locked`, `index_io`,
`index_unreadable`), the store's text (which can name files by absolute path) staying on
the operator surface and in the log, once per change (`api/v1.rs::run_store_status`,
`store_health`; `5f9c5bf`); the Meta adapter never puts a response body or a URL into an
error, an event or a receipt (`meta.rs::http_failure`, `without_url()`), and Hyphae RPC
errors keep only a code and a bounded message. Tests:
`codes_serialize_snake_case_and_carry_their_status`, `failure_messages_never_echo_the_reason`,
`public_views_strip_internals_and_operator_views_keep_them`,
`public_receipts_keep_only_the_public_invocation_fields`,
`every_refusal_and_trip_reason_maps_to_a_code_status_and_fixed_message`,
`upstream_bodies_and_the_key_never_reach_errors_or_the_log`, `the_meta_key_never_appears_in_any_output`,
`rpc_errors_keep_only_their_code_and_a_bounded_message`, `operator_views_keep_the_raw_failure_reason`,
`ready_names_an_index_failure_by_its_code_and_publishes_no_store_text`,
`the_public_ready_names_an_index_error_by_code_never_by_its_text`,
`the_operator_ready_keeps_the_index_error_text_next_to_its_code`,
`health_reports_a_stable_code_with_every_index_error`,
`an_unhealthy_detail_becomes_a_stable_code_and_never_a_path`. **Residual risk:** the
hashed chain still holds English free text written by the adapters (a failure reason, up
to 300 characters of verifier stderr in a verification detail, `GENERIC_CORRUPTION_NOTE`);
the public views strip it, but the verbatim `events.ndjson` of a live-declared run would
not, hence T24. The 422 messages of T19 quote serde's text.

**T24. The verbatim chain of a live run.** `public_view::ndjson_publishable` publishes
`GET /api/v1/runs/{id}/events.ndjson` on the public surface only for a run declared
scripted over simulated or no evidence whose records agree; a live-declared run's owner gets
`404 ndjson_unavailable` and keeps the detail and the receipt (`v1.rs::run_events_ndjson`).
The operator surface serves every chain. Tests: `public_ndjson_is_published_only_for_scripted_chains`,
`ndjson_is_published_only_for_chains_that_cannot_carry_upstream_text`,
`the_operator_gets_the_whole_chain_of_a_live_run`,
`a_live_run_over_the_sealed_snapshot_is_proven_verified_and_published_without_paths`
(`404 ndjson_unavailable` for a live run over the snapshot). Residual: a visitor cannot
recompute a live run's chain from the verbatim bytes; they can download the receipt and
run `consensus-api verify` on a log the operator exports. Lifting this needs the adapters
to record only codes in the chain (section 5).

**T25. Path traversal and hostile ids.** Run ids are `run-` plus a lowercase UUID v4 and
are parsed before any path is built (`run_store.rs::parse_run_id`; unknown ids are refused
from the index alone, and lookups create nothing); research files are served only by a
path that exists in `research/MANIFEST.json`, never through a path join
(`showcase.rs::research_file`); static pages reject `..`, empty segments, backslashes,
NUL and drive letters (`static_files.rs::StaticSite::serve`); snapshot paths must be plain
relative paths, and the snapshot walk refuses symbolic links and nesting (T10); a run log
replaced by a symlink is refused. Tests:
`parse_run_id_accepts_exactly_the_uuid_id_gen_format`, `unknown_and_malformed_ids_never_create_a_file`,
`research_files_are_served_by_manifest_path_only`, `traversal_is_refused_and_missing_pages_get_the_404_page`,
`identifiers_digests_and_paths_are_checked_strictly`,
`a_symlink_loop_under_the_directory_is_refused_not_followed`,
`a_directory_nested_deeper_than_the_flat_layout_is_refused`,
`a_log_replaced_by_a_symlink_is_refused` (unix-only: the G8 Linux run recorded at
`2f0efe0` included it; it never runs on Windows), `research_files_are_served_verbatim_with_their_digest`.
Residual: none known.

**T26. Web client injection and script injection by the edge (ADR-0009, ABUSE-5).**
The static export is served with a per-page CSP from `csp-manifest.json`
(`static_files.rs`: `default-src 'none'; script-src 'self' <hashes>
https://challenges.cloudflare.com; frame-src https://challenges.cloudflare.com; frame-ancestors
'none'; base-uri 'none'; object-src 'none'`, no `unsafe-inline` or `unsafe-eval` for
scripts) and the security headers (`nosniff`, `Referrer-Policy: no-referrer`,
`X-Frame-Options: DENY`, `Cross-Origin-Resource-Policy: same-origin`,
`Cross-Origin-Opener-Policy: same-origin`, `Cache-Control: no-store` on `/api`); the
console page gets its own hash CSP. The build fails on inline event handlers,
`javascript:` URLs and cross-origin subresources (`apps/web/scripts/csp-manifest.mjs`); an
ESLint rule and a unit test keep the theme script the only `dangerouslySetInnerHTML`
(`apps/web/tests/theme-script.test.ts`, `csp-manifest.test.ts`, `serve-static.test.ts`,
not run here); the W1 client runs zod without its `new Function` compiler
(`apps/web/src/lib/api/schemas.ts`, `z.config({ jitless: true })`) and uses its own tabs
component (`20ba9c0`: under the hash-only CSP, zod's compiler was reported as a violation
on every page and the design system's tab list injected a `<style>` element at run time).
Tests (Rust): `security_headers_and_per_page_csp_are_present`,
`the_console_is_served_with_a_hash_csp_and_no_store`, `csp_hash_matches_the_specification_example`.
**Residual risk:** Cloudflare features that inject scripts (Rocket Loader, Email Address
Obfuscation, Zaraz, Web Analytics) would break the hash CSP and must be off; HSTS is the
edge's; neither is verifiable from the repository (G6, pending; `deploy/scripts/verify-edge.sh`
compares edge and origin bodies from the owner machine, not yet run). `style-src-attr
'unsafe-inline'` is allowed for the framework's inline style attributes.

### 3.4 Cost and abuse

**T27. Budget exhaustion on the owner's key (audit SEC-2, MA-2, MA-5; ABUSE-1).** No
visitor text reaches a model; a live run is one of five compiled-in presets
(`live/presets.rs`, byte-equal to the recorded batteries). Admission runs in a fixed
order, stopping at the first refusal and spending nothing before the ledger has reserved
the run's worst case (`live/gate.rs`). Phase one (`Inner::verify_blocking`), before the
live slot, holds and charges nothing: preset, geography, 18+ attestation, the evidence
status (`503 live_unavailable` while the snapshot is not ok, before any token is spent),
Turnstile (T28), the lane. The route then takes the single live slot (`429
too_many_runs`). Phase two (`Inner::admit_blocking`), with the slot held: the lane's daily
quota, then the reservation (`MeteredReservation::reserve`), the metered model client,
the deliberation (the factory re-checks the snapshot's files in full and refuses, with
the reservation released, before any model call), the grant. The daily quota
(`limits.rs::DailyCounts`, since `b120317`) allows at most `CONSENSUS_LIVE_CLIENT_DAILY`
(default 3) public-lane admissions per client key (IPv4 /32, IPv6 /64) per UTC day of the
ledger's clock (`429 live_client_quota`, `Retry-After` to 00:00 UTC), in a table of at
most 10,000 keys; it is counted just before the ledger is asked and given back by every
refusal and failed start; the judge lane is exempt (T29 has its per-code limit). The
ledger (`live/budget.rs::Ledger`) is append-only JSONL per UTC day plus an atomically
rewritten lifetime summary, fsync'd, serialized through an exclusive file lock shared by
every process that opens it; it reserves `(run token cap + max_completion_tokens +
input cap) × output price` per run, which is $2.36 at the defaults
((70,000 + 16,000 + 32,000) × $20/M: the input cap's default is 32,000 since `0318566`,
because 8,000 refused every snapshot-backed run at its first evaluation), draws before
**every** HTTP attempt (retries included) and settles each draw once
(`meta.rs::MetaModelClient::with_meter`, `draw_attempt`, `settle_attempt`): actual usage
on success, zero on 400/401/402/403/404/413/429 and on an attempt that was never sent, the
full estimate on timeouts, post-send transport errors, other statuses and usage-less
successes, never refunded. Caps: $10 per UTC day with an even refill and a 25% burst at
00:00 (the $2.36 worst case fits the $2.50 burst), $60 lifetime, a $15 judge slice with
its own daily cap (default the slice), 6 reservations per rolling hour per lane (public
and judge), 1 concurrent public reservation per process; the operator lane (T32) shares
the daily cap and the lifetime public share ($60 − $15) with the public lane, without
pacing, hourly cap or slot. The configuration refuses caps that could never admit a run
and, when `CONSENSUS_LIVE_EXPOSURE_DAYS` is set, a daily cap times the days above the
lifetime cap. The reservation is released exactly once when the run ends, is dropped, or
fails to start, with bounded retries; a failed release is logged with its id, and
`consensus-api ledger release <id> --by <operator>` (`ledger_cli.rs`, since `4b8b5ae`)
reconciles it from a shell. Tests (ledger):
`fifty_concurrent_reservations_never_exceed_the_cap`, `crash_between_reserve_and_settle_counts_worst_case_after_reopen`
(the handle is dropped in-process without settling or releasing, then the directory is
reopened; no test kills a process),
`unknown_outcomes_charge_the_full_estimate_and_never_refund`, `retries_take_one_draw_per_attempt`,
`the_reservation_is_a_hard_ceiling`, `lifetime_cap_reached_trips_across_reopen`,
`hourly_admission_cap_and_even_daily_refill_with_burst`, `public_lane_never_reserves_into_the_judge_slice`,
`judge_lane_admits_while_public_is_exhausted_and_never_beyond_its_slice`,
`the_judge_lane_has_its_own_daily_cap`, `operator_runs_debit_the_same_ledger_without_pacing`,
`input_and_output_caps_refuse_before_anything_is_written`,
`two_handles_on_one_directory_serialize_through_the_lock`, `exact_decimal_money_over_ten_thousand_settlements`,
`a_torn_tail_is_cut_and_a_corrupt_line_fails_closed`, `the_lifetime_summary_is_a_floor_when_day_files_go_missing`,
`a_stale_summary_does_not_override_newer_day_files`, `a_failed_release_keeps_the_handle_and_the_slot_until_it_succeeds`,
`release_by_id_reconciles_a_crashed_reservation`, `an_attempt_that_was_never_sent_settles_at_zero_and_is_recorded_as_such`;
(ledger subcommand): `status_lists_the_reservation_a_crash_left_open`,
`release_ends_a_reservation_left_by_a_crash_once`, `releasing_an_unknown_reservation_fails`,
`a_reservation_id_is_normalized_to_the_ledger_spelling`;
(gate and adapter): `admission_refuses_in_order_and_charges_nothing`,
`the_gate_verifies_before_the_slot_charges_after_it_and_refunds_a_failed_start`,
`requests_waiting_on_siteverify_never_hold_the_live_slot`,
`the_live_slot_refuses_only_after_turnstile_passed_and_the_token_stays_spent`,
`a_client_starts_its_daily_share_of_public_runs_and_no_more`,
`a_failed_start_gives_its_client_count_back`,
`daily_counts_hold_a_limit_per_key_and_day_and_give_back_what_was_refused`,
`a_live_run_is_metered_streamed_and_published_without_the_key`,
`a_two_round_run_over_the_snapshot_fits_the_default_input_cap`,
`a_start_that_fails_after_admission_releases_the_reservation`,
`a_panicking_deliberation_still_releases_its_reservation`,
`a_draw_refused_mid_run_fails_the_run_at_its_stage_and_charges_once`,
`a_connection_dropped_after_the_request_draws_twice_and_settles_both`,
`dropping_a_metered_deliberation_releases_its_reservation_once`,
`a_dropped_deliberation_retries_its_release_until_the_ledger_is_free`,
`a_deliberation_dropped_on_an_async_worker_still_releases`,
`a_release_that_cannot_reach_the_ledger_is_logged_and_frees_the_slot`; (configuration):
`defaults_are_the_owner_decisions`, `defaults_load_with_live_disabled`,
`cross_checks_hold_the_caps_together`, `the_judge_daily_cap_defaults_to_the_slice_and_stays_within_it`,
`the_daily_quotas_default_to_three_per_client_and_five_per_judge_code`,
`enabling_live_mode_requires_the_ledger_dir_secret_and_hostname`,
`every_preset_equals_the_battery_script_byte_for_byte`, `the_module_exposes_no_way_to_carry_visitor_text`.
**Residual risk, stated plainly:** Meta offers no hard spending cap, so this ledger is
the only cap; one UTC day's exposure is `daily + judge daily` ($25 at the defaults), and
operator runs spend from the public day (T32). The ledger bounds the cost, not the
fairness: the per-client quota is process-local (a restart forgets every count) and keyed
by IPv4 /32 or IPv6 /64, so three runs a day are shared by everyone behind one IPv4 NAT,
and a client with many prefixes, solving one Turnstile per run, can still take the
public lane's hourly allowance (6) and its day. The lifetime cap is per ledger directory:
the runbook keeps one per phase (staging, then public from zero), so the key's total
exposure is the sum over the directories it used (runbook 4.5 and open question 6). A
release that keeps failing leaves the reservation at its worst case until an operator
reconciles it by hand. The public per-run token cap is enforced by the orchestrator after
each call and by the reservation's ceiling per attempt.

**T28. Turnstile replay and Siteverify floods (the high finding fixed in `aed879d`).**
Siteverify is called with a fresh UUID v4 as `idempotency_key` on every verification,
never the request's `Idempotency-Key`: Cloudflare's idempotency is global and immediate,
while the live route's replay is owner-scoped and recorded only after the start, so
forwarding the header let one solved challenge start a run per session or several at
once. A token is therefore single-use at Siteverify; a retry whose first attempt
succeeded is answered from the stored creation before the gate is asked; `remoteip` is
not sent (the client key is a prefix). The verifier also requires `success`, the expected
`action` (`live-run`, which must differ from the session action; the web widget is
rendered with it since `8106a54`), the expected `hostname`, and a `challenge_ts` within
300 s (+60 s skew); tokens over 2,048 bytes never reach Cloudflare; test secrets are
refused in public mode (`live/turnstile.rs`, `live/config.rs::load_turnstile`). Since
`b120317` Turnstile runs in phase one, before the live slot, so requests waiting on
Siteverify (up to `SITEVERIFY_TIMEOUT`, 10 s) hold no slot; since `e915b6d` at most
`SITEVERIFY_MAX_IN_FLIGHT = 8` calls are in flight, and past that a request is refused at
once with `503 turnstile_unavailable` and `Retry-After: 2`, before its token is sent. A
token Siteverify accepted stays spent when the slot or phase two then refuses. Tests:
`admission_refuses_in_order_and_charges_nothing` (no `remoteip`, a fresh UUID per call,
never the request's key), `a_token_replayed_from_another_session_starts_no_second_run`,
`concurrent_duplicates_of_one_submit_start_one_run`,
`requests_waiting_on_siteverify_never_hold_the_live_slot`,
`siteverify_calls_in_flight_are_bounded_and_a_refused_token_is_not_spent`,
`the_live_slot_refuses_only_after_turnstile_passed_and_the_token_stays_spent`,
`the_same_token_and_key_pass_twice_so_the_gate_never_reuses_a_key`,
`a_retry_with_the_same_idempotency_key_passes_and_a_new_key_does_not`,
`a_matching_fresh_token_passes_and_the_call_carries_ip_and_key` (the verifier forwards
an IP when given one; the gate gives none), `wrong_action_is_refused`,
`wrong_hostname_is_refused`, `hostname_comparison_ignores_case`, `stale_or_far_future_challenge_ts_is_refused`,
`success_false_and_outages_fail_closed`, `empty_and_oversized_tokens_never_reach_cloudflare`,
`for_action_shares_the_transport_and_checks_its_own_action`,
`http_transport_posts_the_documented_form_and_parses_the_answer`,
`http_transport_treats_non_200_and_unparseable_bodies_as_refusals`,
`test_secrets_are_recognized_and_real_ones_are_not`, `cloudflare_test_secrets_are_refused_in_public_mode_only`,
`secret_is_redacted_and_test_secrets_are_recognized`. **Residual risk:** bogus tokens
sent fast enough can keep the eight permits busy (each up to 10 s), and every genuine
visitor is then refused `503 turnstile_unavailable` with their token unspent: a denial of
live mode at no cost to the key, bounded only by the per-client run rate (6 per minute
per client key) and the edge rule. A Turnstile solve spent on a start that the slot or
phase two refuses, or that fails after admission, cannot be refunded (the visitor solves
again); a Siteverify outage disables live mode (`503 turnstile_unavailable`), which is the
intended direction.

**T29. Judge-code probing and leaked codes.** Codes exist only as unsalted SHA-256
digests in the codes file; a presented code is trimmed, hashed and compared against every
digest in constant time (`live/judge.rs::JudgeCodes::lookup`); an unknown code takes the
public lane without an error of its own; malformed lines are reported by number, never
content. Since `b120317` a code can start at most `CONSENSUS_LIVE_JUDGE_CODE_DAILY`
(default 5) judge-lane runs per UTC day, refused past that with the judge lane's own daily
refusal (`429 live_budget_judge_daily`), whichever client presents it; since `8f3f4d6`
`deploy/scripts/judge-codes.sh` makes codes of 140 random bits (28 symbols of a 32-symbol
alphabet), prints them once and writes only their digests (`root:consensus 0640`, no
overwrite without `--force`); since `25c7867` the server matches a code as typed, then
in capitals, so a printed code typed in lowercase is the same code (one id, one daily
count). Tests: `verifies_hashed_codes_and_refuses_everything_else`,
`a_printed_code_typed_in_lowercase_is_the_same_code`,
`a_known_code_has_one_id_however_it_is_presented`, `malformed_lines_are_reported_by_number_only`,
`judge_codes_load_as_digests_and_malformed_lines_are_never_echoed`,
`a_known_judge_code_takes_the_judge_lane_and_an_unknown_one_the_public_lane`,
`a_judge_code_starts_its_daily_limit_and_is_then_refused_like_the_judge_day`,
`judge_lane_admits_while_public_is_exhausted_and_never_beyond_its_slice`,
`the_judge_lane_has_its_own_daily_cap`; `judge-codes.sh` has no automated test (checked by
hand, per its commit). **Residual risk:** the lane is observable. Refusals differ by lane
(`live_budget_judge_slice` and `live_budget_judge_daily` against the public lane's
`live_budget_daily`, `live_hourly_admissions`, `live_concurrency`, `live_client_quota`),
the judge lane has its own rolling-hour window and slot, and `/capabilities` publishes a
`daily_remaining` that counts only public and operator commitments next to a
`lifetime_remaining` that counts all lanes. So a prober who solves one Turnstile per
guess can tell a valid code; with the script's 140-bit codes guessing is out of reach,
but nothing checks the length or randomness of a code written into the file some other
way, and an unsalted digest of a short code is guessable offline if the file leaks. A
code is a bearer secret shared through the submission's testing instructions: a leaked
code can start five runs a day until the judge slice is spent; revocation is editing the
file and restarting. A judge whose code is mistyped silently spends the public lane.

**T30. Hourly, daily, lifetime caps and the kill switch.** See T27 for the caps. The
ledger trips, persistently, when a settlement exceeds the attempt's output bound or
estimate, on 401/402/403 from the provider, when the lifetime cap is reached, or when the
kill-switch file (`<ledger dir>/kill-switch` or `CONSENSUS_LIVE_KILL_SWITCH_FILE`) exists;
tripped, it refuses every reservation and every draw on every lane, so in-flight runs stop
at their next call; `/ready` and `/capabilities` report the file as a trip the moment it
exists (`live/gate.rs::Inner::trip_state`). Removing the file clears nothing once an
admission has recorded the trip: only `consensus-api ledger reset --by <operator>` does
(`ledger_cli.rs`, since `4b8b5ae`; recorded with the operator's name, refused while the
kill-switch file is present, nothing recorded without a trip), run from a shell on the
server (`deploy/scripts/ledger.sh` runs it under the public unit's own sandbox, without
credentials; `deploy/scripts/kill-switch.sh` sets, shows and removes the file). The
subcommand reads the server's live configuration with the live switch, the judge-codes
file and every Turnstile setting hidden (`live/config.rs::LedgerEnv`; until `25c7867` the
Turnstile hostname passed through, and with the deployed `public.env`, which sets it, the
load refused itself as an inconsistent Turnstile configuration), and refuses a
missing ledger directory instead of creating one. Tests:
`kill_switch_file_refuses_everything_until_an_operator_reset`, `completion_over_max_trips_and_the_trip_survives_reopen`,
`a_charge_above_the_attempt_draw_trips`, `provider_key_refusals_trip_and_other_no_retry_statuses_charge_zero`,
`lifetime_cap_reached_trips_across_reopen`, `the_summary_floor_covers_the_judge_lane_and_a_reached_cap_trips_at_open`,
`ledger_refusals_answer_with_their_codes_and_the_kill_switch_trips`,
`a_draw_refused_mid_run_fails_the_run_at_its_stage_and_charges_once`,
`snapshot_cached_never_waits_for_the_lock_and_never_writes`,
`the_kill_switch_is_reported_before_any_admission_trips_the_ledger`,
`an_unreadable_ledger_publishes_no_budget_and_admits_nobody`,
`a_metered_operator_run_debits_the_live_ledger_and_is_refused_while_it_is_tripped`
(the switch stops console runs; removing the file does not reopen; a reset does);
(subcommand) `reset_clears_a_trip_and_the_server_sees_it_at_its_next_read`,
`reset_refuses_while_the_kill_switch_file_is_present`, `reset_without_a_trip_records_nothing`,
`arguments_are_checked_before_any_ledger_is_opened`,
`an_absent_ledger_directory_is_an_error_and_is_never_created`,
`no_credential_is_read_and_the_server_caps_apply`, `the_by_value_is_bounded_and_plain`.
`kill-switch.sh` and `ledger.sh` have no automated test. Residual: the kill switch and the
reset need shell access to the server; a trip from one 401/402/403 stops live mode until
an operator resets it.

**T31. Duplicate billing on retries (audit MA-8, TC-7).** Only 429 and 503 are retried,
after `Retry-After` when it fits within 30 s and leaves the retry at least a second, else
full-jitter backoff from 500 ms, within one time budget per call (default 120 s; 1–3600)
that also covers the body read; a connection that never opened is retried; a connection
that broke after the request was sent is retried **exactly once** (default on,
`CONSENSUS_META_RETRY_TRANSPORT_ONCE`) and the lost attempt is charged at its full
estimate on a metered run; timeouts, 504 and non-retryable 4xx are never retried
(`meta.rs::MetaModelClient::send`). Tests: `only_429_and_503_are_retried`,
`retry_after_in_seconds_is_honored_on_429`,
`retry_after_as_an_http_date_is_honored_on_503`, `a_retry_after_beyond_the_budget_fails_at_once`,
`a_retry_after_that_leaves_no_time_for_the_retry_fails_at_once`,
`a_refused_connection_is_retried_and_a_dropped_one_is_not_with_the_transport_retry_off`,
`a_connection_dropped_after_the_request_is_retried_once_by_default`,
`one_budget_bounds_the_whole_call_including_the_body_read`, `persistent_503_retries_once_then_unavailable_and_500_is_never_retried`,
`retries_take_one_draw_per_attempt`, `a_connection_dropped_after_the_request_draws_twice_and_settles_both`,
`invocation_records_schema_name_attempts_and_finish_reason`. Residual: the one post-send
retry can bill one duplicate generation that no invocation records; the ledger counts it
on a metered run, the operator's receipt shows `attempts: 2`, and the public receipt
drops `attempts` (`public_view::sanitize_receipt`); an unmetered operator run accounts for
nothing of it.

**T32. Operator-console runs on the owner's key (audit ABUSE-1, R3 of the v2 review).**
Since `314eee3`, when `CONSENSUS_LIVE_LEDGER_DIR` is set, `serve-operator` opens that
ledger at startup with `serve-public`'s caps, read credential-free
(`server.rs::operator_ledger`, `LiveConfig::load_credential_free`; an absent directory or
an unusable budget variable stops startup), and every `model=meta` console run takes the
operator process's live slot, reserves its worst case on `Lane::Operator` through the
public gate's own `MeteredReservation`, draws and settles every HTTP attempt, and releases
the reservation when it ends (`api/v1.rs::operator_meta_start`); a refusal answers with
the ledger's code and fixed sentence, and a run whose `CONSENSUS_META_MAX_COMPLETION_TOKENS`
exceeds the ledger's per-call output bound is refused before anything is reserved. The
unit template loads `public.env`, so a templated server meters. Without the variable the
runs are unmetered, and the startup line says so (`server.rs::operator_metering_line`).
Tests: `a_metered_operator_run_debits_the_live_ledger_and_is_refused_while_it_is_tripped`,
`a_metered_operator_run_must_fit_the_ledgers_output_bound`,
`without_a_ledger_directory_operator_runs_are_unmetered_and_the_startup_line_says_so`,
`serve_operator_opens_an_existing_live_ledger_with_the_server_caps_and_no_credential`,
`operator_runs_debit_the_same_ledger_without_pacing`. Mitigation of who can reach it:
loopback only, behind a bearer token, started on demand. **Residual risk:** the operator
lane spends from the public lane's day and lifetime share without pacing, an hourly cap
or a per-client quota, so console runs can use up the public day; a metered operator run
spends at most its reservation (the live worst case, $2.36 at the defaults) although the
console's own run token cap is 150,000 by default, so a long console run fails once a
draw no longer fits; anyone holding the operator token and a process on the host can
start such runs (the token has no expiry, T35); on a server started without the variable
(a development machine running the recorded batteries) console runs spend outside every
cap and the kill switch.

**T33. Request floods and resource exhaustion.** Per-client fixed-window rates keyed by
IPv4 /32 or IPv6 /64 (sessions 10/min, runs 6/min, streams 30/min) in bounded LRU tables
(50,000 entries); deliberation pools of 4 guided and 1 live with a 2 s admission wait and
a queue of 32 (`429 too_many_runs`); a visitor-run ring of 2,000 that purges the oldest
resting runs to admit a new one and answers `503 storage_full` only when nothing can be
freed; visitor runs readable for 24 h and deleted after 7 days by a purge loop every 10
minutes; a 30 s request timeout (`408`); store health re-read at most once per 2 s for
`/ready`, the snapshot's re-hash at most once per 2 s for `/ready`, `/capabilities` and
the gate's pre-Turnstile check, the ledger view at most once per 5 s
(`limits.rs`, `runs.rs::RunManager::reserve`, `make_room`, `api/v1.rs::store_health`,
`live/snapshot_factory.rs::status`, `live/gate.rs::SNAPSHOT_MAX_AGE`,
`server.rs::run_server`); at most 8 Siteverify calls in flight (T28); the unit templates
add `MemoryMax=1G`, `TasksMax=1024` and `LimitNOFILE=65536` for the public server. Tests:
`rates_streams_and_client_keys_are_enforced_per_slash_64`,
`rate_table_refuses_past_the_window_and_stays_bounded`, `ipv6_addresses_in_one_slash_64_share_a_key`,
`the_guided_ring_frees_the_oldest_resting_run_and_refuses_only_when_nothing_can_be_freed`,
`the_ring_frees_the_oldest_session_runs_ended_access_first_and_skips_held_ones`,
`purge_deletes_only_expired_session_runs_whole_and_records_each_purge`,
`the_operator_surface_has_its_own_generous_limits`,
`the_status_is_cached_and_every_admission_rechecks_the_files_in_full`. No test exercises
the `408` itself or the purge loop's timer. **Residual risk:** an attacker with many /64
prefixes evades the per-client tables. Cloudflare's rate-limit rule is a coarse
complement: on the Free plan one rule counts requests per IP, matches only on the path
and verified-bot fields, with a 10 s counting period and a 10 s mitigation, and each data
center keeps its own counters (https://developers.cloudflare.com/waf/rate-limiting-rules/
and its request-rate page, read 2026-10-28). Under heavy load the ring can evict a
visitor's run before its 24 h window ends (open owner decision); the global stream cap
(512) is shared by everyone; the session table can be flushed (T15); there is no
header-read timeout at the origin (section 5).

### 3.5 Credentials

**T34. The Meta key in files, environment, argv, logs or `/proc`.** The key is read from
`CONSENSUS_META_API_KEY_FILE` first, else the first of `CONSENSUS_META_API_KEY`,
`MODEL_API_KEY`, `META_API_KEY` (`config.rs::MetaConfig::from_lookup`); the unit templates
use `LoadCredential=meta.key:/etc/consensus/credentials/meta.key`, so the file is
`/run/credentials/consensus-public.service/meta.key` (the operator unit loads its own
copy), and they never set a key variable. The key lives in a `Zeroizing<String>`
(`ApiKey`), prints as `ApiKey(<redacted>)`, has no `Display`, is checked for the
documented shape without being shown, is sent as a sensitive header value only
(`meta.rs::MetaModelClient::new`, `set_sensitive(true)`, redirects disabled, https
required except to loopback), and is not written to argv, events or receipts. Hyphae
children lose the key variables and, since `6fb4ac3`, the credential-path variables
(T12). The public server forces `public_mode`, which refuses the raw-draft debug
directory (`server.rs::public_meta_config`, `config.rs::MetaConfig::validate`). Tests:
`the_key_file_variable_comes_first_and_its_errors_name_it`, `a_credential_file_key_is_read_trimmed_and_checked`,
`meta_key_precedence_is_consensus_then_model_then_meta`, `the_key_never_prints_and_is_trimmed`,
`unusable_meta_settings_are_errors_naming_the_variable`, `the_debug_dir_is_refused_in_public_mode`,
`the_debug_dir_keeps_sanitized_files_without_the_key_and_never_in_public_mode`,
`hyphae_children_never_inherit_the_meta_key`, `upstream_bodies_and_the_key_never_reach_errors_or_the_log`,
and the canary `the_meta_key_never_appears_in_any_output` (a fake key in the environment,
a refusing endpoint; the key is searched for in every response, SSE frame, stored file and
the health body, whose keys are asserted). **Residual risk:** a child process running as
the same user can read the key file at its predictable path; a key passed through a key
variable instead of the file is visible in `/proc/<pid>/environ` to anyone who can read
that process; the development machine's user environment holds the key, which is why the
Node toolchain runs only in Docker (ADR-0009). `/proc/<pid>/environ`, the client bundle
and the deployed environment are not covered by any test (G4);
`deploy/scripts/canary-scan.sh` is the manual procedure and has not been run.

**T35. Other secrets: the Turnstile secret, judge codes, the operator token.** The
Turnstile secret is read from a file into `TurnstileSecret` (redacted `Debug`, no
serialization) and sent only in the Siteverify form body; only the public unit loads it
(the operator unit no longer does since `e915b6d`, and the ledger reads hide its
variable). Judge codes exist only as digests (T29). The operator token comes from
`CONSENSUS_OPERATOR_TOKEN_FILE` (at least 32 characters; the operator unit template loads
it with `LoadCredential=operator.token`) or is generated (32 random bytes) and printed
once, and is compared in constant time (`api/mod.rs::OperatorAuth`, `constant_time_eq`,
which since `2bdcdcf` folds the whole length difference into the result instead of a
`u8` cast that let lengths 256 apart compare equal when the longer value ended in NULs).
Tests: `secret_is_redacted_and_test_secrets_are_recognized`,
`judge_codes_load_as_digests_and_malformed_lines_are_never_echoed`,
`constant_time_eq_compares_whole_values` (including the 256- and 512-NUL cases),
`every_api_route_needs_the_bearer_token`. **Residual risk:** a generated operator token is
printed to stdout (under systemd, to the journal; the template's token file avoids it);
the token has no expiry, no rotation beyond a restart with a new file, and no throttling
of wrong attempts beyond the operator surface's rates.

**T36. Hyphae credentials on the public server.** None exist there: `serve-public` has no
route that reads `HYPHAE_NATIVE_API_KEY_FILE` or connects to a daemon (only the operator
route `operator_meta_start` reads `HyphaeConfig`); it runs the verifier subprocess, which
needs no key; the templates leave the Hyphae variables unset (`public.env.example` says
they must stay unset) and keep the snapshot read-only; nothing in the code refuses those
variables if an operator sets them, and the verifier child would inherit them (T12). The
sealed instance's credentials are destroyed at the end of a successful capture session
(`capture-snapshot.sh` step 7, which lists names and sizes only and refuses to continue if
the directory survives); a failed or interrupted session destroys nothing and logs that
they must be destroyed by hand, and SIGKILL bypasses the traps. The operator lab passes
the reader key as a file path to `hyphae mcp` and never `--allow-write`. Tests:
`hyphae_needs_a_key_file_and_a_daemon_and_bounds_the_verifier` (asserts no
`--allow-write`), `connect_rejects_surface_without_memory_recall`. No test covers the
deployed server (G9; `deploy/scripts/sandbox-probe.sh` runs one verifier under the unit's
directives by hand). Residual: witnesses of the sealed instance carry the verifiers of its
destroyed keys; they stay private.

### 3.6 Persistence

**T37. Torn tails, crashes, two writers (audit RT-6, TC-3, RT-9, RT-13).** Every append
is fsync'd before it is acknowledged (and new directory entries on Unix); a torn final
record is cut on a writable open and is an error on `open_read_only`; a failed append rolls
back and a failed rollback poisons the handle; a stale handle that finds the file changed
refuses with `ConcurrentWriter`; `open_locked` holds an exclusive lock file for the
handle's life; each run has one owner through `<run_id>.lock`, readers hold it shared;
`SharedRunLog` validates each record against the stored history in the same critical
section that appends it, publishes only after the fsync, and catches a panicking publisher;
the run manager tries a terminal `RunFailed` on an error or a caught panic and recovery
closes interrupted runs at startup (`event_log.rs`, `run_store.rs`, `shared_log.rs`,
`runs.rs::RunManager::try_fail`). Tests: `truncated_tail_is_recovered_and_append_continues`,
`failed_mid_record_append_rolls_back_and_log_stays_valid`, `failed_rollback_poisons_the_handle`,
`a_stale_handle_never_writes_over_another_writers_records`, `locked_open_shuts_out_a_second_writer_until_the_first_drops`,
`read_only_open_refuses_a_torn_tail_and_leaves_it_in_place`, `read_only_open_never_creates_a_log`,
`a_run_has_one_owner_at_a_time_across_handles_and_stores`, `publishes_each_record_once_after_it_is_durable_and_outside_the_lock`,
`refused_appends_are_neither_stored_nor_published`,
`an_append_the_disk_refuses_is_neither_stored_nor_published` (Windows-only: it fakes a full
disk through Windows' mandatory locks),
`a_panicking_publisher_never_unwinds_into_the_writer`, `a_second_writer_ending_the_run_mid_deliberation_never_breaks_replay`,
`a_panicking_model_fails_only_its_run`, `recovery_closes_interrupted_runs_records_terminal_heads_and_reports_the_rest`,
`a_run_whose_deliberation_died_is_closed_when_the_store_starts_again`,
`startup_recovery_closes_crashed_runs_and_reports_broken_ones`, `interrupt_reopen_and_continue_to_decision`,
`write_atomic_never_leaves_a_partial_file_at_its_path`, `a_sidecar_write_that_fails_midway_leaves_no_sidecar_and_no_run`.
Residual: the full-disk append path has no test on Linux, the deployment target, and no
test drives a persistence failure through `RunManager`; there is no disk-space floor; on
Windows the directory entry is not fsync'd (std cannot open a directory); locks are std
file locks, so a process that holds a lock and is frozen keeps its run busy
(`409 run_busy`).

**T38. Tampering with logs, sidecars or the index (audit RT-4, RT-10, DOC-9, LC-12).**
Each record's `hash` is SHA-256 over `prev_hash` and the record's canonical JSON with
`hash` blanked, anchored at `genesis` per run; replay re-derives every recommendation with
the rules of its recorded schema, refuses a chained record labelled 1.0 and any schema
downgrade within a run, and holds 1.3 records to the contract; the sidecar is checked
against its file name and against the digest the `created` index record committed to; the
run index is itself a chain, verified whenever the store opens (`IndexTampered` keeps the
whole store closed, `IndexMissing` too), and a run's log is compared with its latest
indexed head before anything is written to it; receipts v2 recompute the chain and the
policy and refuse a forged kind, rationale or assumption list; `consensus-api verify`
does the same read-only for a downloaded log and receipt. Tests:
`tampered_record_breaks_hash_chain`, `corrupt_middle_line_is_hard_error`, `sequence_gap_is_detected`,
`a_relabelled_record_is_refused_even_when_re_hashed`, `a_runs_schema_version_never_goes_down`,
`one_record_of_a_1_3_run_relabeled_down_fails_replay`, `a_forged_1_3_verification_fails_replay`,
`relabelling_a_record_cannot_escape_re_derivation`, `a_forged_rationale_or_assumption_list_gets_no_receipt`,
`any_edit_to_the_run_index_is_detected_when_the_store_opens`, `a_run_rewritten_and_rehashed_is_refused_and_left_as_found`,
`a_sidecar_that_disagrees_with_its_run_is_provenance_inconsistent`, `a_sidecar_edited_in_canonical_form_is_provenance_inconsistent`,
`a_lost_run_index_keeps_the_store_closed_unless_an_empty_one_is_accepted`, `run_files_the_index_never_recorded_are_never_served`,
`health_reports_the_index_head_a_cut_tail_and_an_index_edited_while_open`,
`a_good_log_and_its_receipt_verify`, `one_changed_byte_in_the_log_fails`, `the_research_archive_verifies_run_by_run`,
`chain_links_verify_from_the_raw_lines`, `every_log_line_round_trips_byte_for_byte`,
`read_only_open_never_changes_an_archived_log`. **Residual risk, stated plainly:** the
chains and the index are unsigned. They are tamper-evident against a published head (a
receipt's `head_hash`, the index head from `/ready`, the committed `research/MANIFEST.json`),
not tamper-proof against someone who rewrites the logs and the index together; records
appended after a run's latest checkpoint and removed again are not detectable; relabelling
a whole run to 1.1/1.2 and re-hashing it passes as an honest older run. Signed receipts are
post-hackathon.

**T39. Deleted, truncated or unindexed runs; damaged runs taking others offline (audit
RT-3).** One file per run: a damaged file fails only that run (`a_damaged_stored_run_is_reported_never_served_as_if_whole`),
a truncated or deleted run is reported by recovery and left alone
(`a_truncated_or_deleted_run_is_reported_and_left_alone`, `a_run_awaiting_its_decision_that_lost_records_is_reported_and_left_as_found`,
`a_run_cut_inside_its_terminal_record_or_extended_after_it_is_left_as_found`), run files the
index never recorded are never served, and purges are recorded and finished after a crash
(`a_purge_interrupted_after_its_index_record_is_finished_later`,
`discard_empty_run_removes_only_an_empty_unheld_run_and_records_the_purge`). Residual: see
T38.

### 3.7 Supply chain and build

**T40. Dependencies, toolchains, CI and what the repository publishes.** Rust toolchain
pinned by `rust-toolchain.toml` (1.99.1); every gate runs `--locked`; `deny.toml` allows
exactly the permissive license expressions in `Cargo.lock`, denies yanked crates and
wildcard dependencies and allows only crates.io; `THIRD_PARTY_NOTICES.md` is generated
from `cargo metadata`. The CI workflow (`.github/workflows/ci.yml`) pins every action by
commit SHA and the Playwright image by digest, and runs fmt, clippy `-D warnings`,
cargo-deny, the tests, the conformance example, the demo crate and the two snapshot-tooling
self-tests, plus the web checks. `scripts/linux-gates.sh` (rewritten in `e9eefb4`) runs
fmt, clippy, the workspace tests, the demo crate's tests, the conformance example and both
self-tests in an `ubuntu:24.04` container pinned by digest, with the repository mounted
read-only and no host environment; every gate runs even after one failed, and the script
exits 1 when any failed; it does not run cargo-deny (CI and the host do); the toolchain
comes from `https://sh.rustup.rs` at run time when its volume is empty (rustup checks the
checksums of what it downloads) and the apt packages are not pinned; `--release` builds
`dist/consensus-api` only from a clean checkout after every gate passed, with its
SHA-256 checked after the copy. The web toolchain runs only in Docker:
`apps/web/scripts/in-docker.sh` uses a digest-pinned Node image, mounts `apps/web`
writable except `scripts/` and `pnpm-workspace.yaml` (read-only) and `contracts/`
read-only, passes no environment variable, and leaves Docker's defaults for the network,
the user (root) and the capabilities; `playwright-docker.sh` runs with `--network none`,
`--cap-drop ALL`, `no-new-privileges`, user `pwuser` and a read-only `/app` except
`test-results/`. pnpm is pinned by `packageManager` hash, with exact versions, a frozen
lockfile, `minimumReleaseAge: 4320` minutes and every dependency build script reviewed
and denied (`allowBuilds`); the project's rule (ADR-0009) is not to run `astryx init`,
which writes agent-instruction files, and no such file is in the tree. Tests: `committed_contract_matches_the_code`, `health_reports_the_digest_of_the_committed_schemas`
(the API contract files and fixtures drift-checked); the gates themselves are otherwise
the check. **Residual risk:** dependency code that runs during `pnpm install`, the build,
lint and Vitest has the network and writes `apps/web/out/`, including
`out/csp-manifest.json`, which `serve-public` serves and trusts for its CSP hashes;
`cargo deny check` and `pnpm audit --prod` were last reported clean on 2026-10-27 and were
not re-run for this document; the workflow has not run on GitHub yet (the repository is
private and only the owner pushes); there is no secret scanner (gitleaks) or push
protection; `docs/reviews/` names operational details (where credentials are stored,
container paths, digests; no secret value) that become public with the repository; the
Linux fresh-clone run is still owed (G8).

### 3.8 Deployment (templates in the repository; nothing deployed)

`docs/deployment/runbook.md` and `deploy/` hold the unit files, the tmpfiles entry, the
cloudflared ingress example and drop-in, the environment templates and the operator
scripts (`install.sh`, `verify-edge.sh`, `sse-heartbeat-test.sh`, `canary-scan.sh`,
`kill-switch.sh`, `ledger.sh`, `judge-codes.sh`, `export-ledger.sh`, `sandbox-probe.sh`).
None has been run against a server; they are verifiable only on a deployment that does
not exist yet.

**T41. Listener exposure and header forgery (ABUSE-2).** `serve-public` binds a unix
socket (`CONSENSUS_PUBLIC_SOCKET`, default `/run/consensus/public.sock`, mode 0660 set
after the bind; `server.rs::serve_unix`) or a **loopback** TCP address for development
(`CONSENSUS_PUBLIC_BIND`; anything else is refused); with the socket
`CONSENSUS_ALLOWED_ORIGINS` and `CONSENSUS_ALLOWED_HOSTS` are required,
`CONSENSUS_TRUST_PROXY_HEADERS=false` is refused, and with live mode both snapshot pins
are required (T10); `serve-operator` binds loopback only (`CONSENSUS_OPERATOR_BIND`,
default `127.0.0.1:8787`). The templates: both units run as `User=consensus`,
`Group=consensus`, `UMask=0027`, `ProtectSystem=strict` with `ReadWritePaths` limited to
the run store, the ledger tree and (public only) the socket directory, `PrivateTmp=yes`,
`NoNewPrivileges=yes`, an empty capability set and a system-call filter, secrets through
`LoadCredential` (the public unit `meta.key` and `turnstile.secret`, the operator unit
`meta.key` and `operator.token`), `KillMode=mixed`, `TimeoutStopSec=90`; `/run/consensus`
is created `2750 consensus:consensus-sock` by `deploy/tmpfiles.d/consensus.conf`, so the
socket inherits the group `consensus-sock`, cloudflared's only extra group (its drop-in
runs it as user `cloudflared`); `deploy/cloudflared/config.yml.example` sends `<hostname>`
and `staging.<hostname>` to `unix:/run/consensus/public.sock` and ends with
`http_status:404`, with no rule for the operator console; the operator unit has no
`[Install]` section. Tests: `non_loopback_public_bind_is_refused`,
`socket_mode_requires_explicit_origins_and_hosts` (unix-only),
`without_unix_sockets_the_public_bind_is_required` (Windows-only),
`tcp_dev_mode_derives_hosts_and_origins_and_trusts_no_proxy`,
`trusted_proxy_policy_requires_the_header`, `operator_defaults_to_loopback_8787`,
`invalid_values_name_the_variable`, `live_mode_on_the_socket_requires_both_snapshot_pins`.
No test binds the unix socket or checks its mode, and no unit file is tested.
**Residual risk:** anything that can connect to the socket can present forged `CF-*`
headers: the members of `consensus-sock` (cloudflared), the socket's owner `consensus`,
which runs both servers and every verifier child, and root; staging shares the production
process. `CONSENSUS_PUBLIC_BIND` overrides the socket even on unix, and with
`CONSENSUS_TRUST_PROXY_HEADERS=true` (allowed on TCP) any local process can forge
`CF-Connecting-IP` and `CF-IPCountry`; nothing refuses live mode on a TCP bind (only the
pins are relaxed there), so that configuration is development-only by discipline, not by
code. The ingress file is reviewed by hand (G1).

**T42. Edge features that rewrite responses.** See T26: Rocket Loader, Email Address
Obfuscation, Zaraz and Web Analytics must be off for the hostname, `cf-cache-status` must
never be HIT on `/api` (the origin sends `no-store`), HSTS is set at the edge, the WAF geo
rules (the 23 codes of `geo::DEFAULT_PORTAL_BLOCKED`) and the one rate-limit rule are
configured by the owner. The runbook holds those steps and `deploy/scripts/verify-edge.sh`
checks bodies, cache status and headers from outside; nothing is configured or verified
yet, and nothing of it is testable from the repository (G6, G10, pending).

**T43. Graceful shutdown and restarts.** SIGTERM/Ctrl-C cancels every active run at its
next checkpoint, waits up to 60 s, and refuses new runs meanwhile
(`server.rs::graceful`, `runs.rs::RunManager::shutdown`); startup recovery closes runs a
dead process left mid-deliberation as `interrupted` before anything is served. The public
unit template sends SIGTERM with `KillMode=mixed` and a 90 s stop timeout (longer than the
60 s grace) and restarts on failure. Tests:
`shutdown_cancels_active_runs_and_refuses_new_ones`, `shutdown_is_recorded_with_stage_shutdown`,
`cancel_at_every_stage_records_a_cancelled_failure`, `a_run_whose_deliberation_died_is_closed_when_the_store_starts_again`.
The signal handler itself is untested. Residual: a redeploy still loses the tokens of the
runs it cancels, and a run inside a model call when the grace ends is closed by the next
start's recovery; the staging rehearsal (runbook 8.8, which trips and then resets the
staging ledger) is pending (G11).

## 4. Status of the 2026-10-27 audit findings at `e915b6d`

Statuses: **closed** (with where and how), **partly** (what remains), **accepted** (with
the rationale), **open**, **pending** (needs the deployment, the capture session or the
owner), **superseded**. Commits are from `git log 72e422a..e915b6d`; "lane" names the
merged lane (S-b1 `9dca9f1`, S-b2 `f52b68f`, R-b `6c98fb4`/`e22399a`, D2 `4d4f01a`/`5455e69`,
live gate `d1e58f1`, snapshot factory `2f0efe0`, snapshot binding `2a9d4ad`, live
hardening `561c25e`, web W1 `4c42ae2`, runbook `2fecb32`). The red-team ids (ABUSE-*,
LC-*) were defined in the plan, which is kept outside the repository; the rows below use
the meaning the code comments give each id and say so when an id is not described
anywhere in the tree.

| Id | Finding (short) | Status at `e915b6d` | Where |
|---|---|---|---|
| SEC-1 | No auth or ownership on reads; loopback is no boundary behind a tunnel | closed in code; pending deployment probe (G1, G2) | sessions + owner-scoped 404 (`session.rs`, `runs.rs`, `run_store.rs`), listener split (`server.rs`); `another_session_sees_nothing_of_a_run`; lane S-b1 |
| SEC-2 | Anonymous callers start unlimited live runs on the server key | closed in code (two-phase gate, ledger, per-client daily quota; startable over a verified snapshot since `2f0efe0`); deployment pending (snapshot captured 2026-10-29, `b0375df`; it boots only with `0515f29`, merged as `7e26866`) | `live/gate.rs`, `live/budget.rs`, `limits.rs::DailyCounts`, `runs.rs::reserve`; `admission_refuses_in_order_and_charges_nothing`, `a_client_starts_its_daily_share_of_public_runs_and_no_more`; `d1e58f1`, `2f0efe0`, `561c25e` |
| SEC-3 | Caller-chosen `project_id` reads any project | closed for the public surface; accepted on the operator surface | no project field in public requests (`api/types.rs`), presets fixed to `demo-repo`, the snapshot serves one project and must be `demo-repo` to boot; `requests_refuse_unknown_fields_and_free_strings`, `a_snapshot_of_another_project_does_not_boot_for_live_mode` |
| SEC-4 | One std mutex held for whole runs inside async handlers | closed | per-run `SharedRunLog`, blocking pool, `RunManager` (`runs.rs`); `a_decision_during_deliberation_is_refused_and_live_frames_say_live`; `200b0df`, `92bb836` |
| SEC-5 | Synchronous POST outlasts the edge timeout | closed in code; heartbeat through a tunnel pending (G11; `deploy/scripts/sse-heartbeat-test.sh` not yet run) | `202` + SSE with 15 s heartbeats (`v1.rs`, `stream.rs`); `a_guided_run_is_accepted_streamed_decided_and_receipted` |
| SEC-6 | Every read re-parses a growing multi-tenant log | partly: per-run files, index, ring, retention, pagination; no disk-space floor | `run_store.rs`, `v1.rs::list_runs`; `the_guided_ring_frees_the_oldest_resting_run_and_refuses_only_when_nothing_can_be_freed` |
| SEC-7 | SSE replay unauthenticated and uncapped | closed | owner check, caps, per-run ids, `stream_end` (`stream.rs`, `limits.rs`); `sse_resumes_from_every_sequence_without_duplicates_or_gaps`, `rates_streams_and_client_keys_are_enforced_per_slash_64` |
| SEC-8 | No field limits; 2 MB strings into prompts | closed: 16 KiB body, `deny_unknown_fields`, enums, bounded fields; no visitor text reaches a prompt (the decision note is stored only, with bidi and zero-width characters refused since `9bfe2e2`) | `api/mod.rs`, `api/types.rs::check_text`; `csrf_content_type_unknown_fields_and_size_are_enforced`, `stored_text_refuses_hidden_format_characters` |
| SEC-9 | Internal errors, upstream bodies, verifier output and paths in responses and events | closed for the responses and views the server builds (`/ready` names a run-index failure by code since `5f9c5bf`); partly for the chain (free text inside hashed records, so live ndjson is withheld, T24); accepted for the verbatim `/research` files (652 container paths in 35 data files, 112 of them in the 8 files of 2026-10-30, T11) | `api/error.rs`, `public_view.rs`, `api/v1.rs::run_store_status`, `meta.rs`, `hyphae.rs::artifact_ref`; `failure_messages_never_echo_the_reason`, `upstream_bodies_and_the_key_never_reach_errors_or_the_log`, `ready_names_an_index_failure_by_its_code_and_publishes_no_store_text` |
| SEC-10 | Operator token printed, compared in non-constant time; self-asserted identities | partly: constant-time compare over the whole length (`2bdcdcf`), identity from the channel, the templates load the token from a credential; a generated token is still printed (accepted: use the token file); no expiry, rotation or throttling (open) | `api/mod.rs::OperatorAuth`, `constant_time_eq`, `server.rs::operator_token`; `constant_time_eq_compares_whole_values`, `every_api_route_needs_the_bearer_token` |
| SEC-11 | No Host or Origin check (DNS rebinding) | closed | `host_check`, `csrf_check` on both surfaces; `host_and_origin_are_checked`, `csrf_content_type_unknown_fields_and_size_are_enforced` |
| SEC-12 | A decision not bound to the recommendation seen | closed on the server and in the web Decision tab, which sends the state it shows; the legacy console defeats it (UI-3) | `DecisionRequest.expected` → `409 decision_conflict`; `409` during deliberation (`runs.rs::decide`); `a_guided_run_is_accepted_streamed_decided_and_receipted` |
| SEC-13 | No security headers, no CSP | closed at the origin; HSTS and edge pending (G6) | `security_headers`, per-page CSP (`static_files.rs`), console CSP; `security_headers_and_per_page_csp_are_present`, `the_console_is_served_with_a_hash_csp_and_no_store` |
| SEC-14 | CSRF resistance implicit and untested | closed | as SEC-11; `__Host-` `SameSite=Strict` cookie; `a_host_prefixed_cookie_is_never_emitted_without_secure` |
| SEC-15 | No graceful shutdown, request timeouts or connection limits | partly: graceful shutdown and a 30 s request timeout (the `408` untested); no header-read timeout at the origin (`axum::serve` sets no hyper timer); connection limits delegated to cloudflared | `server.rs::graceful`, `TimeoutLayer`, `public_config.rs`; `shutdown_cancels_active_runs_and_refuses_new_ones` |
| SEC-16 | Simulated proofs shown as a green `valid` | closed in the API (`simulated` flag, detail kept for fakes) and in the web workspace (`4c42ae2`, tests not run here); pending through the hostname (G7) | `public_view::verification_v1`, `apps/web/src/lib/provenance.ts`; `provenance_is_derived_from_records_and_cross_checked` |
| ABUSE-1 | Spend on the owner's key must be reserved before anything runs | closed for the public and judge lanes, and for operator console runs when `CONSENSUS_LIVE_LEDGER_DIR` is set (`314eee3`); a console without it is unmetered (by configuration, its startup line says so) | `live/budget.rs::Ledger`, `live/gate.rs::MeteredReservation`, `api/v1.rs::operator_meta_start`; `fifty_concurrent_reservations_never_exceed_the_cap`, `crash_between_reserve_and_settle_counts_worst_case_after_reopen`, `a_metered_operator_run_debits_the_live_ledger_and_is_refused_while_it_is_tripped` |
| ABUSE-2 | Forged `CF-*` headers | closed in code by the unix socket and the trust policy; the templates' socket group `consensus-sock`; residual: same-user processes and root can connect, and a TCP bind can trust the headers (T41); deployment pending | `public_config.rs`, `limits.rs::client_key`, `deploy/tmpfiles.d/consensus.conf`; `trusted_proxy_policy_requires_the_header` |
| ABUSE-3 | Web toolchain rewriting what the host runs | partly: `playwright-docker.sh` has no network, no capabilities, `no-new-privileges` and a read-only mount; `in-docker.sh` keeps only `scripts/` and `pnpm-workspace.yaml` read-only and runs with the network, as root, with default capabilities; no automated test | `apps/web/scripts/in-docker.sh`, `playwright-docker.sh`; `17fbe20` |
| ABUSE-4 | Per-client keying must be /64 with bounded tables | closed: rates, streams and the live per-client daily quota keyed by IPv4 /32 or IPv6 /64 in bounded LRU tables (`561c25e`); the quota is process-local | `limits.rs` (`RateTable`, `StreamCaps`, `DailyCounts`); `ipv6_addresses_in_one_slash_64_share_a_key`, `rates_streams_and_client_keys_are_enforced_per_slash_64`, `daily_counts_hold_a_limit_per_key_and_day_and_give_back_what_was_refused` |
| ABUSE-5 | Inline scripts need per-page CSP hashes | closed at the origin; browser verification pending (G6) | `apps/web/scripts/csp-manifest.mjs`, `static_files.rs`; `security_headers_and_per_page_csp_are_present` |
| ABUSE-6 | Cookies set by GET or static responses | closed | `session.rs`, `v1.rs::create_session`; `sessions_are_minted_by_post_only_and_carry_the_right_attributes` |
| ABUSE-7 | Ids before paths; owner-checked reads | closed | `run_store.rs::parse_run_id`, `read_run`; `unknown_and_malformed_ids_never_create_a_file`, `a_non_owner_is_told_nothing_and_never_touches_the_runs_lock_or_log` |
| ABUSE-8 | Visitor free text reaching a model | closed | `live/presets.rs`, `api/types.rs`; `the_module_exposes_no_way_to_carry_visitor_text` |
| ABUSE-9 | Admission order, Turnstile fail-closed, geo on live | closed (two phases since `561c25e`: Turnstile before the live slot, the quota and the reservation after it; at most 8 Siteverify calls in flight since `e915b6d`) | `live/gate.rs`, `live/turnstile.rs`, `api/mod.rs::portal_geo`; `admission_refuses_in_order_and_charges_nothing`, `requests_waiting_on_siteverify_never_hold_the_live_slot`, `siteverify_calls_in_flight_are_bounded_and_a_refused_token_is_not_spent`, `the_portal_block_covers_every_route_and_fails_closed_without_a_country` |
| ABUSE-10 | not described in the repository | — | no code or document reference |
| ABUSE-11 | Geographic policy (owner decision lists) | closed in code; Cloudflare rule pending (G10) | `live/geo.rs`; `ukraine_is_blocked_on_the_whole_portal`, `south_korea_is_blocked_for_live_mode_only` |
| ABUSE-12 | not described in the repository | — | no code or document reference |
| MA-1 | Evaluator ids dropped silently | closed | keyed per-call schema + contract (`meta.rs`, `contract.rs`); `every_ma1_variant_fails_evaluate_and_never_reaches_supported`, `a_missing_key_is_malformed_output_naming_the_claim_or_item`; `1ee386a`, `3d1aeec` |
| MA-2 | Live mode spends the environment key for anyone | closed by design (no BYOK, owner key, gate, key file; startable since `2f0efe0`); partly for the file: a Hyphae child running as the same user can read it (`6fb4ac3` withholds the path only) | `server.rs::public_meta_config`, `config.rs::META_KEY_FILE_VARIABLE`, `hyphae.rs::CREDENTIAL_PATH_VARIABLES`; `the_meta_key_never_appears_in_any_output`, `hyphae_children_never_inherit_the_meta_key`; `83e0829`, `d1e58f1` |
| MA-3 | Run wall time incompatible with an edge proxy | partly: `202` + SSE, idempotent creation (guided and operator keys claimed before the start, `f24d05f`); the call budget is 1–3600 s and the deadline is checked between calls, so a run can overrun it by one call in flight, and the 60 s shutdown grace is shorter than one default call | `api/v1.rs`, `orchestrator.rs`, `config.rs`; `one_budget_bounds_the_whole_call_including_the_body_read`, `a_retried_creation_with_the_same_key_returns_the_same_run`, `expired_deadline_fails_the_run_with_stage_deadline` |
| MA-4 | Upstream error text and inherited environments leak | partly: bodies never persisted; the key variables and the five credential-path variables removed from children (`6fb4ac3`); the environment is not cleared and the files stay readable by the same user | `meta.rs::http_failure`, `hyphae.rs::hyphae_command`; `upstream_bodies_and_the_key_never_reach_errors_or_the_log`, `hyphae_children_never_inherit_the_meta_key`; lane R-b |
| MA-5 | Per-call spend unbounded and partly invisible | closed | `max_completion_tokens`, `finish_reason`, ledger draws, failed calls recorded; `truncated_output_has_its_own_code_and_keeps_the_billed_call`, `input_and_output_caps_refuse_before_anything_is_written`; `30b598f` |
| MA-6 | Visitor and model text outside untrusted framing | closed | `meta.rs` blocks; `the_task_is_an_untrusted_bounded_block_and_cuts_are_marked`, `forwarded_model_text_stays_inside_untrusted_blocks_on_one_line` |
| MA-7 | Untrusted content forges prompt structure | closed (defense in depth; claim and contradiction ids outside the blocks as bounded neutralized lines, T2) | `meta.rs::untrusted_line`, `json_string`; `no_delimiter_variant_survives_in_any_untrusted_field`, `evidence_is_a_json_array_and_no_record_can_forge_another` |
| MA-8 | Retry policy bills twice, ignores `Retry-After` | closed (one bounded post-send retry, charged at worst case on a metered run) | `meta.rs::send`; `only_429_and_503_are_retried`, `a_connection_dropped_after_the_request_is_retried_once_by_default` |
| MA-9 | Debug dir file names from the response id | closed | `meta.rs::debug_file_name`, refused in public mode; `the_debug_dir_keeps_sanitized_files_without_the_key_and_never_in_public_mode` |
| MA-10 | `strict_schema` walk key-agnostic; no golden files | partly: per-call keyed schema asserted strict in tests; `strict_schema` still treats any object with `properties` as a schema; no golden files | `meta.rs::strict_schema`; `structured_output_is_strict_and_every_list_is_required`, `the_evaluate_request_is_keyed_by_this_calls_claim_and_item_ids` |
| HY-1 | `Valid` not bound to the record | closed for kind, scope and anchor; open for the byte-binding of record text to the proof, which every receipt now states (`proof_limitations`, `821c217`, public receipts included) next to the operator's verifier detail | `hyphae.rs::check_artifacts`, `consensus-domain/src/receipt.rs::DEFAULT_LIMITATIONS`; `valid_requires_a_reexecuted_memory_proof_bound_to_the_anchor_given`, `committed_contract_matches_the_code` |
| HY-2 | Verifier without a timeout | closed for the deadline and output caps; open for the temp-directory sweep and process-tree kill (the unit's `PrivateTmp` and `KillMode=mixed` act only at stop; untested) | `hyphae.rs::SystemCommandRunner`; `a_verifier_past_its_deadline_is_a_verification_error_with_its_code`, `the_system_runner_kills_a_command_past_its_deadline` |
| HY-3 | Live Hyphae behind the public portal | closed by design: the sealed snapshot is the public live mode's evidence factory (`2f0efe0`), bound to every run (`BootBoundVerifier`) and pinned (`2a9d4ad`); live Hyphae stays operator-only; capture session held 2026-10-29 (`b0375df`); the real snapshot boots only with `0515f29` (merged as `7e26866`) | `live/snapshot_factory.rs`, `snapshot.rs`, `v1.rs::operator_meta_start`; `a_live_run_over_the_sealed_snapshot_is_proven_verified_and_published_without_paths`, `an_orchestrated_run_over_the_snapshot_is_proven_verified_and_recommended`; ADR-0008 |
| HY-4 | Generic `corruption` becomes `Invalid` | accepted (fail-closed) with an honest detail | `orchestrator.rs::GENERIC_CORRUPTION_NOTE`, typed `ProofFailure`; `the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it` |
| HY-5 | 4 MiB ceiling; witnesses accumulate | accepted (operator hygiene; the snapshot avoids the ceiling); no code | `docs/architecture/sealed-snapshot.md`, container README |
| HY-6 | Receipts cannot locate later-round proofs | closed | `RecallProven`, `EvidenceVerification.round`/`proof_ref` (schema 1.3); `recall_proven_is_accepted_where_evidence_events_are`, `a_verification_is_exactly_what_the_rounds_evaluation_implies` |
| HY-7 | Unstable fallback id; unknown shapes read as empty | closed | `hyphae.rs::map_record`, `record_list`; `a_record_without_an_id_is_named_the_same_way_every_time`, `an_answer_without_a_record_list_is_an_error_never_an_empty_recall` |
| HY-8 | Keyword fallback marks stderr `Invalid` | accepted (not adopted, LC-10) | `hyphae.rs::check_artifacts`; `the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it` |
| HY-9 | stdio transport weaknesses | partly: one deadline per request with a cancellation notice, bounded lines, stderr tail kept, child killed on drop; process-tree kill on Windows still absent | `hyphae.rs::StdioTransport`; `mcp_lines_are_bounded`, `the_stdio_transport_reports_a_dead_child_and_keeps_its_stderr_tail` |
| HY-10 | Pinned anchor neither validated nor normalized | closed (normalized and validated; required for the snapshot, where a malformed one is a boot error since `29b95aa`); the operator lab may still run unpinned (self-consistency only, its detail says so) | `config.rs::normalize_anchor`, `SnapshotConfig`; `the_pinned_anchor_is_normalized_and_validated_at_load`, `the_pinned_anchor_is_normalized_and_handed_to_the_verifier`, `a_set_snapshot_dir_with_a_missing_or_malformed_anchor_is_a_boot_error` |
| HY-11 | `agent ui` path for capture | exercised in the capture session of 2026-10-29, used there only and never exposed: it works through the session's daemon, refuses a string request id (the script now sends an integer, `b0375df`), and wraps its result in an envelope with `result`, which the runtime at `6e202eb` did not read (`response_invalid` at boot; fixed in `0515f29`, merged as `7e26866`) | `capture-snapshot.sh`, `docs/architecture/sealed-snapshot.md` §9 and "Observed in the capture session" |
| HY-12 | Empty-recall proofs discarded | closed | `Recall::proof`, `RecallProven{empty: true}`; `an_empty_proven_recall_still_returns_its_proof`, `foreign_records_are_dropped_and_proofs_recorded_even_when_empty` |
| HY-13 | Verification cache keyed by path | closed; a fresh verifier per public live run | `hyphae.rs::HyphaeProofVerifier::verify`, `live/snapshot_factory.rs::run_verifier`; `results_are_reused_by_artifact_bytes_never_by_path`, `every_run_verifies_the_proofs_again_with_a_fresh_verifier` |
| RT-1 | Earlier-round objections forgotten | closed in code; in the counted live battery (2026-10-30, `docs/evaluation.md` §6.3) every receipt's `earlier_round_objections_never_rejudged` is empty (a check that matters only for M01 and M03, the runs with a recorded round-2 evaluation; M02 and M04 recorded one evaluation each), and the contract refused one evaluation that left a carried contradiction unjudged (M04) | `state_machine.rs`, `contract.rs`, `meta.rs` prompts; `rt1_round_two_silence_cannot_reach_supported`; `1ee386a` |
| RT-2 | Global mutex, no cancellation, no subprocess timeout | closed | `runs.rs`, `control.rs`, `hyphae.rs`; `cancel_at_every_stage_records_a_cancelled_failure`, `a_disconnected_client_never_stops_a_run` |
| RT-3 | Event store does not scale; one bad record takes all offline | closed | `run_store.rs`; `a_damaged_stored_run_is_reported_never_served_as_if_whole` |
| RT-4 | Receipts commit to one global chain | closed (per-run chain, receipt v2 chain scope, verify CLI); signed receipts post-hackathon | `receipts.rs`, `verify_cli.rs`; `interleaved_runs_need_the_whole_log_and_head_is_the_runs_own`, `a_good_log_and_its_receipt_verify`; `40b6d06` |
| RT-5 | Recommendation and receipt leave out what is open | closed | receipt v2 `deliberation`, `unresolved_uncertainties`, round-tagged verifications; `reference_receipt_shows_the_deliberation_and_the_dropped_objections`, `the_recommendation_carries_the_final_proposals_uncertainties` |
| RT-6 | No single writer across processes | closed | `EventLog::open_locked`, run locks, the run index lock; `locked_open_shuts_out_a_second_writer_until_the_first_drops`, `a_run_has_one_owner_at_a_time_across_handles_and_stores` |
| RT-7 | No-progress exit compares counts | closed | set-based progress; `progress_is_set_based_not_count_based`; `1ee386a` |
| RT-8 | Idempotent append ignores content | closed | `IdempotencyConflict`, `409`; `reused_event_id_with_different_content_is_a_conflict` |
| RT-9 | Failed rollback does not stop writes | closed | poisoned handle; `failed_rollback_poisons_the_handle` |
| RT-10 | Chained 1.0 records skip recomputation | closed | refused at open and on replay; `a_relabelled_record_is_refused_even_when_re_hashed`, `relabelling_a_record_cannot_escape_re_derivation` |
| RT-11 | Console read path diverges; replay errors dropped | closed | validated reads (`read_run`, writer snapshot), `FailureV1`; `a_damaged_stored_run_is_reported_never_served_as_if_whole` |
| RT-12 | SSE global ids, no `Last-Event-ID`, 200 for unknown runs | closed | `stream.rs`, `v1.rs::run_events`; `sse_resumes_from_every_sequence_without_duplicates_or_gaps` |
| RT-13 | Persistence error aborts without a terminal event | partly: a terminal event is attempted and the stream ends when the writer goes away; the full-disk append path is tested on Windows only, and no test drives a persistence failure through `RunManager` | `runs.rs::try_fail`, `stream.rs` (`Closed` → end), `shared_log.rs`; `a_panicking_model_fails_only_its_run`, `an_append_the_disk_refuses_is_neither_stored_nor_published` (Windows-only) |
| RT-14 | Re-recalled evidence not logged again | closed | `RecallProven` per recall; `the_orchestrator_records_every_proof_and_every_dropped_record` |
| LC-01 | Schema 1.3 must round-trip archived records byte for byte | closed | `every_log_line_round_trips_byte_for_byte`, `every_new_1_3_field_round_trips_byte_for_byte`, `absent_1_3_fields_are_never_written` |
| LC-02 | not described in the repository | — | — |
| LC-03 | Scripted fakes get no exemption from the contract | closed | `a_scripted_evaluation_missing_one_verdict_fails_the_run` |
| LC-04 | Verbatim research files next to labelled display copies | closed | `/research/{*path}` verbatim with digest, showcase views as display copies; `research_files_are_served_verbatim_with_their_digest` |
| LC-05 | Provenance from the records, not the request | closed | `public_view::provenance`; `provenance_is_derived_from_records_and_cross_checked` |
| LC-06 | not described in the repository | — | — |
| LC-07 | Admission before durability; a terminal event on error | closed | `runs.rs` (`reserve`, `abandon_start`, `try_fail`), `shared_log.rs`; `the_gate_verifies_before_the_slot_charges_after_it_and_refunds_a_failed_start`, `refused_appends_are_neither_stored_nor_published` |
| LC-08 | not described in the repository | — | — |
| LC-09 | Evidence cap under carry-forward | closed | `evidence_cap_reached_in_round_one_keeps_round_two_at_the_carried_records` |
| LC-10 | Narrowing the corruption classifier would be a new fail-open | accepted (classifier widened only) | `hyphae.rs`, `orchestrator.rs::upstream_proof_check`; `the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it` |
| LC-11 | Authority and identity never from the body; pre-1.3 reads `unspecified` | closed | `api/mod.rs::Identity`, `receipts.rs::UNSPECIFIED_AUTHORITY`; `requests_refuse_unknown_fields_and_free_strings`, `a_decision_shows_the_authority_it_was_recorded_under` |
| LC-12 | Per-run chains need a cross-run anchor | closed (hash-chained run index) | `run_store.rs`; `any_edit_to_the_run_index_is_detected_when_the_store_opens` |
| LC-13, LC-14 | not described in the repository | — | — |
| LC-15 | Ids validated before any path | closed | `run_store.rs::parse_run_id`, `showcase.rs`; `parse_run_id_accepts_exactly_the_uuid_id_gen_format`, `research_files_are_served_by_manifest_path_only` |
| UI-1 | Synchronous creation without idempotency | closed: `202` + `Location`, `Idempotency-Key`; guided and operator creations claim their key before the start (`f24d05f`); the live route keeps a lookup, one solved challenge admitting one run | `api/mod.rs::CreationKeys`, `api/v1.rs::claim_creation`; `a_retried_creation_with_the_same_key_returns_the_same_run`, `concurrent_duplicates_of_one_guided_submit_start_one_run`, `concurrent_duplicates_of_one_submit_start_one_run` |
| UI-2 | One mutex; every read re-parses | closed | as SEC-4, SEC-6 |
| UI-3 | Approve/Reject go to a stale global run | closed on the server (`expected` block, `409`) and in the web Decision tab (`4c42ae2`), which sends the state it shows; open in the legacy console: `decide()` re-fetches the run at click time and posts that state as `expected`, so the stale-view check cannot fire, and `selectRun` sets `selectedRun` before its fetch with no error handling, so a failed load leaves run A on screen while Approve posts to run B with A's note | `runs.rs::decide`, `apps/web/src/components/run/DecisionTab.tsx`, `console.html::decide`, `selectRun` |
| UI-4 | Provenance not first-class | closed in the API and in the web workspace (`4c42ae2`, tests not run here); pending through the hostname (G7) | `public_view.rs`, `apps/web/src/lib/provenance.ts`; `provenance_is_derived_from_records_and_cross_checked` |
| UI-5 | `fake` defaults; unknown fields ignored | closed | enums, `deny_unknown_fields`, no defaults; `modes_are_explicit_and_credentials_are_required` |
| UI-6 | Approval view leaves out what is approved | closed in the API (`current_proposal`, `challenge_items`, `rounds`, decision note) and in the web workspace, whose Stage tab shows the claims and whose Decision tab shows the recommendation, the scope change, the assumptions, the uncertainties and the open objections (the policy trace is not rendered); open in the legacy operator console, which shows no claim text, challenge items or uncertainties | `public_view::detail`, `apps/web/src/components/run/StageTab.tsx`, `DecisionTab.tsx`, `console.html` |
| UI-7 | Conflicting second decision returns 200 | closed | `409 idempotency_key_reused`; `a_guided_run_is_accepted_streamed_decided_and_receipted` |
| UI-8 | Accessibility gaps | partly: the web shell and run views run axe in Playwright (`apps/web/e2e/`, the lane recorded 204 scans without a violation at `cc7b9ab`; not run here); the legacy console is unchanged and operator-only | `apps/web`, `console.html` |
| UI-9 | `irrelevant` coloured positive | open in the legacy console: `badge()` still gives the positive class to any value matching `relevant` (among `valid`, `supported`, `approved`, `corroborated`) and excludes only `invalid` and `uncorroborated`, so `irrelevant` reads positive (the text label is always shown); the web workspace uses typed labels | `console.html::badge` |
| UI-10 | Inconsistent errors; replay failures hidden | closed | `api/error.rs`, `FailureV1`; `codes_serialize_snake_case_and_carry_their_status` |
| UI-11 | SSE traps; console never used SSE | closed (the console and the web client stream; the contract is typed) | `console.html::streamRun`, `apps/web/src/lib/api/sse.ts`, `contracts/api-v1/ConsensusFrameV1.schema.json`; `committed_contract_matches_the_code` |
| UI-12 | No security headers; internal paths in responses | closed for headers and for the responses the server builds; accepted for the verbatim `/research` files, which keep the batteries' container paths (T11) | as SEC-9, SEC-13; `research_files_are_served_verbatim_with_their_digest` |
| TC-1 | Run lifecycle has zero tests | closed | `api_public.rs` (25 tests), `run_store.rs` tests; `concurrent_guided_runs_stay_isolated_and_ordered`, `two_concurrent_decisions_record_exactly_one` |
| TC-2 | "Every claim assessed" only in the adapter | closed | `contract.rs::validate_evaluation`; `every_claim_is_assessed_once_citing_only_retrieved_records` |
| TC-3 | Nothing enforces a single writer | closed | as RT-6 |
| TC-4 | Adapter relabels records with the requested project | closed | `hyphae.rs::map_record`; `records_keep_their_backend_attribution_and_foreign_ones_are_dropped` |
| TC-5 | HTTP and SSE handlers untested | closed | `api_public.rs`, `api_operator.rs`, `live_gate.rs`, `live_snapshot.rs`, `api_canary.rs`, `api_showcase.rs`, `api_verify.rs`, `api_contract.rs`, `ledger_cli.rs` |
| TC-6 | Descriptive claims in `docs/evaluation.md` off | closed (Corrections section) | `docs/evaluation.md`; `e4ef201` |
| TC-7 | Failed-after-response calls never recorded or charged | closed | `ModelError::after_call`, `take_failed_invocation`; `every_refused_answer_keeps_its_invocation`, `an_answered_call_whose_body_timed_out_is_recorded_and_charged` |
| TC-8 | Idempotency gaps | closed | as UI-1, UI-7; `a_claimed_key_is_held_until_its_creation_ends` |
| TC-9 | Case-sensitive neutralization; task outside any block | closed | as MA-6, MA-7 |
| TC-10 | A test asserts nothing when a key is set | open: `modes_are_explicit_and_credentials_are_required` still skips its no-credentials branch when `MetaConfig::from_env()` finds a key (it ran here, with the key variables unset) | `crates/consensus-api/tests/api_operator.rs` |
| TC-11 | Conformance check prints a constant metric | open: `evaluate.rs` still prints "Unauthorized executions: 0" unconditionally; the guided scenarios are also a `#[test]` now | `crates/consensus-tests/examples/evaluate.rs`; `every_guided_scenario_reaches_its_documented_outcome` |
| TC-12 | Batteries not tied to code or configuration; replay can modify | partly: read-only `verify`, `prompt_sha256`/`schema_sha256` per invocation, `Task.mode`; the battery driver now drives the v1 operator API (`be10c71`); no run-start record of commit and configuration | `verify_cli.rs`, `meta.rs::Role::prompt_sha256`, `scripts/live_battery.py`; `prompt_hashes_identify_the_template_never_the_input`, `read_only_open_never_changes_an_archived_log` |
| DOC-1 | No license | closed | `35297e7`, `e4ef201` (`LICENSE`, `NOTICE`, SPDX headers, README) |
| DOC-2 | Docs said to delete `demo-run/` | closed: the docs and the archive (`ff0d2fb`, `e4ef201`) and the code (`0e8479e`: the scripted demo logs to `scripted-demo/events.jsonl`, refuses to run inside `demo-run/` or `research/`, and an unknown argument prints the usage instead of running it) | `crates/consensus-api/src/main.rs`; `the_scripted_demo_never_writes_below_the_research_directories` |
| DOC-3 | Overclaim about corroborated claims | closed | `e4ef201` (README, pitch, evaluation; errata in `docs/reviews/README.md`) |
| DOC-4 | Field-drift fail-open misattributed | closed | `e4ef201` |
| DOC-5 | `.gitignore` gaps; no secret scanning | closed for the ignore rules and the `.env.example` entries; open for gitleaks and push protection | `.gitignore`, `.env.example`; `35297e7`, `4cb7529` |
| DOC-6 | Raw error bodies in the log | closed | as MA-4; ADR-0002 note |
| DOC-7 | Uncertainties not carried to the human | closed | `Recommendation.unresolved_uncertainties`; `the_recommendation_carries_the_final_proposals_uncertainties`; ADR-0002 note |
| DOC-8 | Seeded fact 5 misdescribes the test | closed (correction notes; fixture comments) | `4cb7529`, `e4ef201` |
| DOC-9 | `head_hash` documented as the chain head | closed (the docs in `e4ef201`; the code comments and the schema description in `40b6d06`) | `receipt.rs`, `event_log.rs::head_hash`, `schemas/run-receipt.schema.json` |
| DOC-10 | Stale statements across ADRs, docs, env template | closed for the docs and scripts and the missing `.env.example` entries (`e4ef201`, `4cb7529`); and, after `e915b6d`, for `.env.example` (`0c473a6`: the dead `CONSENSUS_BIND` and `CONSENSUS_EVENT_LOG` replaced by the operator server's variables) and `ports.rs` (its module doc no longer promises an async M2 adapter) | `crates/consensus-runtime/src/ports.rs`, `.env.example` |
| DOC-11 | Historical briefs carry superseded instructions | closed | `docs/reviews/README.md` (banners, errata) |
| DOC-12 | Personal email in history; "mario" in demo defaults | accepted (history kept; neutral identities have no work item) | `main.rs`, `scenarios.rs` |
| DOC-13 | Promised submission artifacts missing | open (the video and the submission checklist; the deployment runbook now exists but was not part of this finding) | — |
| DOC-14 | Crates lacked `publish = false` | closed | `35297e7` |

The audit also names FEAS-1 to FEAS-10, JUDGE-1, JUDGE-2 and DELIV-1 (delivery and
judging-fit issues of the plan). They are not security findings, are not referenced
outside `docs/reviews/` at `e915b6d`, and are outside this document.

## 5. Known gaps and open decisions at `e915b6d`

Nothing in this list is claimed fixed. Each item names where it stands in the code.

1. **The sealed snapshot is captured but not yet usable by this code** (update of
   2026-10-29). The capture session ran on 2026-10-29 (`b0375df`,
   `snapshots/snap-2026-10-29-demo-repo/`); the anchor, the manifest digest and the binary
   digest are published in the README; `docs/architecture/sealed-snapshot.md`, "Observed in
   the capture session", records items 1 to 8 of §9 as observed (whether `proof_blake3`
   is the BLAKE3 of the proof bytes still cannot be recomputed here, and its item 2 is
   corrected in `0515f29`: the result object is under `result`). The code at `e915b6d` (and at `6e202eb`)
   refuses the real snapshot at boot (`response_invalid`: `hyphae agent ui` 4.0.0 holds the
   result under `result`); the fix is `0515f29`, merged as `7e26866`. With it, the real snapshot
   boots on the development machine and its mutated copies are refused (ADR-0008,
   "Capture"); the committed tests still run over a fixture and a fake `hyphae` script (the
   real-snapshot test of that check is not committed). Still open: the binary digest
   matched against the release asset (runbook 3.2) and the snapshot on the server. Without
   `CONSENSUS_SNAPSHOT_DIR`, `serve-public` refuses to start with live mode on
   (`server.rs::build_live_gate`), and on the socket it also needs both pins.
2. **The counted protocol-1.3 live battery ran on 2026-10-30 and settles no rate**
   (`docs/evaluation.md` §6.3; the last 4 of the 8 authorized runs, K01–K04 having been
   smoke runs). It did not run on 2026-10-29, when the Meta key was not set on the new
   development machine (no Meta call then). Of the 4 runs, 2 were lost to provider errors
   (a call past the battery's 120 s budget, where the smoke runs had 240 s; an HTTP 500),
   1 ended `supported` for a revision that departs from the
   task, and 1 was failed closed by the evaluation contract. No claim about live behaviour
   under protocol 1.3 beyond `docs/evaluation.md` §6 is made.
3. **The verifier binary is checked at boot only** (`snapshot.rs::check_verifier_binary`):
   a binary replaced while the server runs, or a wrapper that prints the boot-accepted
   reports, is not detected until the next start.
4. **Snapshot pins are optional on the development TCP bind.** Since `e915b6d` both are
   required with live mode on the unix socket (`server.rs::require_snapshot_pins`);
   unpinned, a consistent rewrite of the manifest, `SHA256SUMS` and the capture files
   loads. `CONSENSUS_PUBLIC_BIND` overrides the socket even on unix, and nothing refuses
   live mode on a TCP bind (T41).
5. **The verbatim `/research` files keep the batteries' container paths** (652 paths in
   35 data files: 540 in 27 files of 2026-10-26/27 and 112 in the 8 files of 2026-10-30,
   counted as occurrences of `/hyphae-home/`; the earlier figure of 541 in 28 files also
   counted the one mention in `research/README.md`, which is not counted here). Accepted: they are not secrets, and the files are pinned
   byte for byte (T11). The checklist's G9 wording ("no witness or container path in any
   public response") is therefore not met as written.
6. **A run's deadline can be overrun by one call in flight** (checked between calls; one
   call may take up to `CONSENSUS_MODEL_TIMEOUT_SECS`, 1–3600 s, default 120 s), and the
   60 s shutdown grace is shorter than one default call (T5, T43).
7. **No header-read timeout at the origin.** `axum::serve` sets no hyper timer; the 30 s
   request timeout covers handling, not the header read. Behind the tunnel cloudflared is
   the socket's only client and holds the connection limits.
8. **Sessions are minted without Turnstile** (10 per minute per client key into a
   50,000-entry table; T15).
9. **Deserialization messages may quote up to 200 characters of the request body back to
   the caller** (`422 invalid_request`; T19).
10. **Live quotas are process-local and keyed by address prefix.** A restart resets the
    per-client and per-code daily counts; three public runs a day are shared behind one
    IPv4 NAT, and many prefixes evade them; the hourly cap (6) and the single public slot
    are shared by everyone; the eight Siteverify permits can be held by a bogus-token
    flood (T27, T28). A request abandoned after its daily count was taken and before its
    run started (a client disconnect, the request timeout) keeps the count, so that client
    has one run fewer that day; the money is not affected (the dropped reservation is
    released).
11. **Operator runs count against the public day.** The operator lane shares the daily
    cap and the lifetime public share with the public lane, without pacing (T32); a
    `serve-operator` without `CONSENSUS_LIVE_LEDGER_DIR` meters nothing.
12. **The attestation wording and the legal texts are pending the owner** (terms, privacy
    notice, limitations and about pages in four languages; the web form words the
    attestation as the server records it, with the legal text pending sign-off); the
    public contact email is a placeholder (`apps/web/src/lib/site.ts`, `CONTACT_EMAIL`).
13. **Public `events.ndjson` is withheld for live-declared runs** (`public_view::ndjson_publishable`)
    because the chain can carry adapter and verifier free text (up to 300 characters of
    verifier stderr in a verification detail, failure reasons). Lifting it needs the
    adapters to persist codes only.
14. **UI gaps.** The web client is one contract behind the server: its generated types
    (`apps/web/src/lib/api/types.ts`) name contract `cc890dc030ba`, while
    `contracts/api-v1/contract-sha256.txt` is `465f4117…` since `561c25e` (the error code
    `live_client_quota` and `/ready`'s `index_error_code`, neither known to the web
    client). `pnpm api:check` regenerates the types from the committed contract and
    compares them byte for byte, so it, `pnpm check` and the CI web job fail until the
    types are regenerated (read from `apps/web/scripts/gen-api-types.mjs`; not run here).
    The web workspace does not render the policy trace; G6 and G7 have not been checked
    through the hostname. The legacy operator console keeps UI-3 (decisions against the
    state at click time, a stale selection), UI-6 (no claim text) and UI-9 (`irrelevant`
    coloured positive).
15. **Verifier hygiene:** no temp-directory sweep for a killed `hyphae proof verify` (G9's
    "temp-directory sweep tested" is not met); only the direct child is killed; children
    lose the key and credential-path variables but not the rest of the environment, and
    can read the credential files as the same user (T12).
16. **HY-1 residual:** the record text is not byte-compared with the proof; every receipt
    says so, and the snapshot binds texts to its manifest, not to the proofs (T8, T10).
17. **Unsigned chains and index** (T38): tamper-evident against a published head, not
    tamper-proof. Signed receipts are post-hackathon.
18. **Test and tooling hygiene left open:** TC-10 (the operator credential test skips its
    no-key branch when a key is set), TC-11 (`evaluate.rs` prints a constant
    "Unauthorized executions: 0"), MA-10 (`strict_schema` walks every object with
    `properties`; no golden schema files); no test for `408 request_timeout`, the
    unix-socket bind or its mode, the signal handler, the purge timer, the 64 MiB snapshot
    file limit, verifier exits 126 and 127, or a persistence failure through `RunManager`
    (the full-disk append test is Windows-only); `kill-switch.sh`, `ledger.sh`,
    `judge-codes.sh` and the other deploy scripts have no automated test; `cargo deny
    check` and `pnpm audit` not re-run at `e915b6d`; the web suites and the snapshot
    tooling self-tests not run for this document; Linux parity passed on the tree of `25c7867` (G8);
    the fresh-clone run is still owed; the CI workflow has not run on GitHub;
    no gitleaks or push protection.
19. **Code-lane leftovers from the audit:** the demo identity "mario" stays (DOC-12,
    accepted); the stale `ports.rs` promise of an async M2 adapter and the dead
    `.env.example` entries were corrected after `e915b6d` (DOC-10).
20. **The deployment exists as templates only** (`docs/deployment/runbook.md`,
    `deploy/systemd`, `deploy/tmpfiles.d`, `deploy/cloudflared/config.yml.example`,
    `deploy/etc/consensus`, `deploy/scripts`): nothing is deployed, the edge is not
    configured, and the probes (`verify-edge.sh`, `sse-heartbeat-test.sh`,
    `canary-scan.sh`, `sandbox-probe.sh`) have not been run. G1–G7 and G9–G11 are pending.
21. **Cached responses bypass the origin's geographic check** (`/research` and static
    assets carry public cache lifetimes; T21); only the edge rule covers them.
22. **The live-hardening code was not independently reviewed** at the time of writing
    (`561c25e`, `e915b6d`); this document describes it as committed.
23. **Open owner decisions** (`docs/journal.md`, 2026-10-27 night, and the runbook's open
    questions): the judge lane's daily cap (default: the whole $15 slice, so a day's
    exposure is $25); the ring's eviction of in-window visitor runs under load; the public
    contact email; the legal texts in four languages; the Day-2 items (hostname, named
    Tunnel, Turnstile widget and secret, the dedicated server key); whether an optional
    single-model baseline battery is run; egress for updates and the resolved-address
    allowlist; `CONSENSUS_LIVE_EXPOSURE_DAYS` against the judging window; Bot Fight Mode;
    staging after go-live; `sudo -n` or group membership for the origin probes.
24. **Residuals accepted for the hackathon:** a generated operator token is printed once
    to stdout (use `CONSENSUS_OPERATOR_TOKEN_FILE`; the template does); a key passed by
    variable is visible in the process environment (use the key file); a mistyped judge
    code silently spends the public lane; a Turnstile solve spent on a refused or failed
    start is not refunded; geo-blocking is defeated by a VPN; `style-src-attr
    'unsafe-inline'` is allowed; the `/research` container paths (item 5).

## 6. Out of scope

Signed logs and receipts; a cross-process or hardware root of trust for the Hyphae anchor;
the restricted executor (post-M3, separately reviewed before any real mutation is
permitted: in every mode today a decision is recorded and nothing executes); the legacy
operator console's accessibility beyond what it has (it is an operator-only tool on
loopback); hosted BYOK (ruled out by Meta's terms; "bring your own deployment" instead);
a Meta moderation or guard model for visitor input (no visitor text reaches a model: the
public surface takes ids, an attestation flag, a judge code that is only hashed, and a
decision note that is accepted only once the deliberation has ended (`409
run_not_awaiting_decision` before) and is then stored and shown, so none is needed
today); the security of the owner's development machine beyond keeping the Node toolchain
in containers; and the Cloudflare dashboard settings (WAF rules, edge features, Access,
Turnstile widget), which no file in this repository configures and the release checklist
gates. The server side is in scope as far as the `deploy/` templates describe it (T41).
