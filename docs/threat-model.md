# Threat model — Hyphae Consensus (v2)

Date: 2026-10-28 · Version 2 · Describes the code at `d1e58f1` (branch
`feat/public-portal`) · Supersedes v1 (2026-10-26/27, which described `72e422a`)

Companions: ADR-0005 (per-run logs, asynchronous runs, live SSE), ADR-0006 (public
API v1, sessions, listener split), ADR-0007 (public live mode on the owner's key),
ADR-0008 (sealed Hyphae snapshot), ADR-0009 (frontend), ADR-0010 (protocol 1.3),
ADR-0002 and ADR-0003 with their 2026-10-27 notes; the independent audit of `72e422a`
(`docs/reviews/2026-10-27-independent-audit.md`); the security release checklist
(`docs/security/release-checklist.md`, gates G1–G12); the sealed-snapshot architecture
(`docs/architecture/sealed-snapshot.md`).

**What changed since v1.** The operator console of v1 (one loopback router, one
multi-run log behind one mutex, a synchronous `POST /api/runs`, an operator token in the
request body) no longer exists. The binary now has two listeners: `serve-public`, a unix
socket for cloudflared (or a loopback TCP bind for development) serving anonymous cookie
sessions, guided and live runs, the static web export and the research archive; and
`serve-operator`, a loopback TCP listener behind a bearer token, started on demand, which
is the only place the Meta key is consulted today and the only place a live Hyphae store
can be reached. Every run has its own hash-chained log, index entry and lock; runs start
with `202` and stream over SSE; a cancel, a deadline and a graceful shutdown exist. The
evaluation protocol is 1.3 (fail-closed contract, carry-forward), the Meta and Hyphae
adapters are hardened, a USD ledger, Turnstile, presets and a geographic policy gate the
public live mode, and a sealed-snapshot client exists for its evidence. **The public
portal is not exposed.** Nothing is tunneled until every gate G1–G12 reads PASS, and at
`d1e58f1` the public live path is not even startable: `serve-public` refuses to start with
`CONSENSUS_LIVE_ENABLED=true` because no production evidence factory is wired (section 6).

**How this document was verified.** Every mitigation below names its code (file and
function or module) and the tests that cover it. Every test named here exists in the
tree and passed in `cargo test --workspace --locked` at `d1e58f1` on Windows
(x86_64-pc-windows-msvc, 2026-10-28): 35 test binaries, **534 passed, 0 failed,
2 ignored** (`live_hyphae_probe`, `live_meta_propose`, the gated live probes). Where no
test exists, the text says so. Unix-only tests (`#[cfg(unix)]`) are marked; they were not
part of this Windows run. The web unit and end-to-end suites (`apps/web`, Docker-only
toolchain) were not run for this document. No deployed system exists yet, so everything
that depends on the edge, the tunnel or the server is marked as pending.

## 1. System and trust boundaries

| Participant | Trust | Authority and what it can do |
|---|---|---|
| Runtime (`consensus-runtime`, `consensus-domain`) | Trusted | State transitions, the evaluation contract, persistence, policy derivation, receipts. Models never write a recommendation or an authorization |
| Public server (`consensus-api serve-public`) | Trusted code, untrusted input | Serves anonymous sessions over the unix socket; owner-scoped reads; guided runs (scenario id) and, when wired, live runs (preset id); never holds a Hyphae credential; consults the Meta key only through the live gate |
| Operator server (`consensus-api serve-operator`) | Trusted code, authenticated operator | Loopback only, bearer token on every `/api` route, started over SSH. Free task text, caller-chosen project, `model=meta`, `memory=hyphae`. Its decisions carry `operator` authority; its views keep raw failure reasons and verifier output |
| Cloudflare edge and WAF | Trusted for `CF-Connecting-IP` and `CF-IPCountry` **only** when the request arrived through the unix socket (or a development bind with `CONSENSUS_TRUST_PROXY_HEADERS=true`) | Geo rules and one rate-limit rule are the primary controls; the origin re-checks the same country list. Edge features that rewrite HTML must be off (G6) |
| cloudflared and the unix socket | The socket (mode 0660) is the trust boundary for the proxy headers | No public TCP port exists, so a host process outside the socket's group cannot present forged `CF-*` headers |
| Turnstile / Siteverify | Trusted to say whether a challenge was solved; proves no identity | Fail closed: outage, wrong `action`, wrong `hostname`, stale `challenge_ts`, non-200 or unparseable body all refuse |
| Visitors and their sessions | Untrusted | May create a session (POST only), start guided runs, start live runs behind Turnstile and the ledger, read, stream, download and decide on **their own** runs; a public decision carries `public_demo` authority and no operational effect |
| Judges with invite codes | Untrusted; a code only selects a lane | A code hashed into the digest file enters the judge lane (its own budget slice, daily cap and slot); an unknown code falls to the public lane silently |
| Models (Proposer / Critic / Evaluator / reviser, Meta Model API) | Untrusted | Produce drafts only; drafts have no outcome or authorization fields; every decision-relevant id is validated; the Evaluator's judgments are the policy's inputs (threat T1–T4) |
| Meta Model API and the owner's key | The key is the asset; the API is a paid, rate-limited third party with no hard spending cap | The key is read from a credential file (or a key variable), zeroized on drop, never printed; every HTTP attempt of a public run is drawn on the ledger before it is sent |
| Hyphae memory backend (operator lab only) | Integrity-verifiable, content untrusted | `hyphae mcp --profile memory` without `--allow-write`; the key reaches the child as a file path; records keep the backend's own project and scope |
| Hyphae verifier binary (`hyphae proof verify`), a subprocess | Trusted to verify; untrusted output | Run with a deadline and killed past it, its stdout/stderr capped, the Meta key variables removed from its environment; `Valid` requires its report to say `verified`, `kind: memory`, `scope: semantic_reexecution`, `anchor_digest` equal to the anchor it was given |
| Sealed snapshot artifacts (`manifest.json`, `SHA256SUMS`, captures) | Integrity-checked at boot and per run; proofs and witnesses are **private** | The public live mode's only evidence; witnesses are full copies of a store's data directory and are never served or committed |
| Research archive (`research/`) | Read-only, pinned by `MANIFEST.json` and `SHA256SUMS` | Served verbatim; showcase views are display copies; never opened writable |
| Ledger files (`<ledger dir>/YYYY-MM-DD.jsonl`, `lifetime.json`, `ledger.lock`, `kill-switch`) | Trusted, operator-written only | Every process that holds the key serializes through the lock; the kill-switch file trips everything |
| Web client (`apps/web`, static export) | Untrusted at runtime (it is a browser); its build is a supply-chain surface | Renders what the API says; LIVE only from per-frame `delivery` of a live model run; builds only inside Docker containers because the owner's machine holds the key |
| Retrieved memories, repository contents, model outputs | Untrusted data | Never executed, never instructions; shown to models inside `<UNTRUSTED_*>` blocks |
| Human operator | Trusted authorizer | The only decision with operational authority; holder of the operator token, the server key file, the kill switch and the capture session |

**Data flow.** A visitor's request reaches the Cloudflare edge (WAF geo rules, the
rate-limit rule, TLS), is forwarded by cloudflared to the unix socket, and is handled by
`serve-public`. In order, the request passes: the request timeout (`408` past 30 s); the
16 KiB body limit (`413`); the security headers layer; the `Host` allowlist (`421`); the
CSRF rules for state-changing `/api` requests (`Origin` required and allowlisted,
`Sec-Fetch-Site` absent or `same-origin`, `Content-Type: application/json` where a body is
read; `403`/`415`); the portal-wide geographic block on every public route (`451`); the
session extractor (`401 session_required` without a live cookie); the client-key extractor
(`CF-Connecting-IP` required behind the socket, `400 client_unidentified` without it).
Handlers run every store read and every ledger or Siteverify call on the blocking pool
(`tokio::task::spawn_blocking`); no handler holds a lock across an await. A run start takes
a deliberation permit, creates the run's files, appends `TaskCreated` (validated, fsync'd,
then published) and spawns the deliberation on the blocking pool; the orchestrator calls
the model over blocking reqwest and the evidence client (a fake, the sealed snapshot, or on
the operator surface the Hyphae MCP child), and verifies proofs through the verifier
subprocess. Streams subscribe to the run's broadcast channel first, then replay the durable
log, then follow. The operator server runs the same handlers over the same `RunManager`
and store, behind the bearer token, on loopback, with `Host` and `Origin` checks; it trusts
no proxy header and keys limits on the TCP peer.

What is trusted where: `CF-Connecting-IP` and `CF-IPCountry` are read only when
`ServeConfig.client_key_policy == TrustedProxyHeader` (`api/mod.rs::trusted_country`,
`Client` extractor), which is the socket's default and is refused off with the socket
(`public_config.rs`: `CONSENSUS_TRUST_PROXY_HEADERS=false` is an error there). The
`Idempotency-Key` header is trusted only as a per-owner replay key; the Turnstile token
only as something to hand to Siteverify under a fresh UUID. Nothing in a request body
names a project, a model, a task text or an identity on the public surface.

## 2. Assets and security goals

1. **The owner's Meta key and spend.** The key never leaves the process that reads it
   (file first, zeroized, redacted, removed from children); nothing a visitor does can
   spend it before the ledger has reserved the run's worst case; the daily, lifetime,
   judge, hourly and concurrency caps and the kill switch hold under crashes and
   restarts.
2. **Integrity of the chains.** Every run's log is a genesis-anchored SHA-256 chain,
   fsync'd per append, with one writer, a tamper-evident run index and a read-only
   verifier anyone can run on a downloaded log and receipt.
3. **Visitors' isolation and privacy.** A visitor reaches only their own runs (404 for
   everything else); the session id never reaches disk (owner = SHA-256 of the id,
   decisions signed with a second truncated digest); no free text of a visitor reaches a
   model or the record; visitor runs expire.
4. **Honesty of labels.** Model, evidence, source and delivery provenance are derived
   from the chained records, never from the request; LIVE is a per-frame property of a
   live model run; simulated proofs say SIMULATED in the record; a `supported` is shown
   with its policy trace, its open items and what older protocols dropped.
5. **The demo store's witnesses and the lab stores.** Witnesses (full data directories,
   WAL and key verifiers included) and proofs stay on the server (`0600`) and out of git;
   no container path, proof path or witness path reaches a public response.
6. **Availability within the budget.** Bounded tables, bounded queues, bounded streams,
   bounded bodies, per-run deadlines, cancellation, graceful shutdown, a visitor-run ring,
   a request timeout; one damaged run never takes another offline.

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
output.** Everything a role did not write itself is inside an `<UNTRUSTED_*>` block
(`meta.rs`: `task_json`/`UNTRUSTED_TASK`, `proposal_block`, `evidence_block` as a JSON
array of `{"id","kind","integrity","content"}`, `objections_block`, `UNTRUSTED_EVALUATION`);
untrusted text is one neutralized line, bounded, with invisible and bidi characters
removed, angle-bracket look-alikes and full-width forms folded, and any `<` that would
open or close a delimiter written `&lt;` (`meta.rs::untrusted_line`, `json_string`,
`opens_untrusted_delimiter`). Rules live only in the system message. Tests:
`no_delimiter_variant_survives_in_any_untrusted_field`,
`the_task_is_an_untrusted_bounded_block_and_cuts_are_marked`,
`evidence_is_a_json_array_and_no_record_can_forge_another`,
`forwarded_model_text_stays_inside_untrusted_blocks_on_one_line`,
`every_role_keeps_its_system_rules_and_protocol_instructions`,
`untrusted_text_cannot_close_its_delimiter_block`, and at the policy level
`policy_never_authorizes_from_memory_content`,
`memory_prompt_injection_is_insufficient_evidence_however_valid_its_proof`. **Residual
risk:** delimiters are defense in depth, not a boundary. A hostile record stored through
legitimate channels carries a valid proof, and an Evaluator that obeyed it could
corroborate on valid evidence and reach `supported`; the human decision is the last line.
Live evidence (I01/I02, H01/H02 in `docs/evaluation.md`) shows the model treating such a
note as contradicting evidence, which is evidence, not a guarantee. A hostile record that
contradicts legitimate ones forces `contested` (fail-safe denial of support).

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
open(k-1) − resolved(k-1) + items(k)`); silence never clears an item
(`contract.rs`: `carried_contradiction_judged`, `one_verdict_per_item`); progress is
set-based (`orchestrator.rs`). Receipts v2 list
`earlier_round_objections_never_rejudged` for every protocol
(`crates/consensus-runtime/src/receipts.rs::build_receipt_v2`). Tests:
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
uncertainties, which the record carries (`the_recommendation_carries_the_final_proposals_uncertainties`)
and the public detail exposes (`public_view::detail`: `current_proposal`,
`challenge_items`, `recommendation.trace`). No UI renders it yet (section 6), and the
counted protocol-1.3 battery is pending, so no rate of `supported` is claimed.

**T5. Endless debate and token exhaustion.** `MAX_REVISION_ROUNDS = 2`
(`state_machine.rs`), a set-based no-progress exit, at most 12 claims per proposal
(`meta.rs`), at most `MAX_EVIDENCE_PER_RUN = 20` distinct records with every drop
recorded (`orchestrator.rs`), a per-run token cap (`RunLimits`, default 150,000; the
public live cap is 70,000) with missing usage treated as a violation, and a wall-clock
deadline per run honoured at every checkpoint (`RunControl`, `control.rs`). Tests:
`two_revision_rounds_are_reachable_and_bounded`, `revision_beyond_round_limit_is_rejected`,
`no_progress_exits_instead_of_looping`, `proposal_claim_count_is_bounded`,
`evidence_cap_is_strict_recorded_and_retrieval_is_runtime_attributed`,
`evidence_cap_reached_in_round_one_keeps_round_two_at_the_carried_records`,
`token_budget_exceeded_fails_run_safely_with_recorded_reason`, `missing_token_usage_fails_closed`,
`expired_deadline_fails_the_run_with_stage_deadline`. Residual: the token cap is checked
after each call, so a run can overshoot by one call's tokens (the live ledger's reservation
includes one `max_completion_tokens` for that reason).

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
beyond T2–T4.

### 3.2 Evidence layer

**T7. Integrity/truth confusion.** Orthogonal axes; `supported` requires every claim
corroborated by a `Valid` record it cites itself, no contradicting record, no invalid
proof anywhere (`policy.rs::explain`, one implementation with a frozen differential
copy). The receipt's `proof_limitations` and every verifier detail say what a proof does
not prove. Tests: `valid_proof_does_not_imply_corroboration`, `no_checked_evidence_is_never_supported`,
`corroborated_claim_without_own_evidence_is_not_supported`,
`contradicting_evidence_prevents_supported_even_if_not_relevant`,
`explain_agrees_with_the_frozen_derivation_on_random_cases`, `policy_properties_hold_on_random_cases`.
Residual: the records of the sealed snapshot are operator-seeded synthetic facts; a proof
says they are the snapshot's records, not that they are true or that a model used them.

**T8. A `Valid` label not bound to the record it labels (audit HY-1).** `Valid` needs
exit 0 and a report with `status: verified`, `semantic_reexecution_performed: true`,
`scope: semantic_reexecution`, `kind: memory` and an `anchor_digest` equal to the anchor
given; a verified proof of another kind or anchor is `Invalid`, an incomplete report a
`VerificationError` (`crates/consensus-api/src/hyphae.rs::HyphaeProofVerifier::check_artifacts`).
Tests: `valid_requires_a_reexecuted_memory_proof_bound_to_the_anchor_given`,
`exit_zero_without_a_verified_reexecution_report_is_not_valid`,
`verifier_failure_categories_captured_live_map_to_integrity`,
`proof_verifier_maps_exit_codes_to_integrity_only`. **Residual risk:** the record text
handed to models is not byte-compared with the proof (the detail text says so); a buggy
or compromised backend could pair altered text with a valid proof of the same recall. For
the snapshot the responses are bound to their proofs only by the manifest and `SHA256SUMS`
written at capture (section 3.2, T10).

**T9. Corrupted, mismatched or failed proofs.** Any invalid proof in the retrieved set
blocks, without a revision attempt; a corruption-class proof-generation failure is
`Invalid` (never downgraded) with an honest note that Hyphae's `corruption` code also
covers capacity and configuration failures (`orchestrator.rs::upstream_proof_check`,
`GENERIC_CORRUPTION_NOTE`); transport failures (exit 125–127, spawn errors, timeouts) are
`VerificationError`, never tampering. Tests: `invalid_proof_anywhere_in_retrieved_evidence_blocks`,
`invalid_proof_blocks_without_revision_attempt`, `abstention_with_invalid_proof_is_blocked_and_replayable`,
`upstream_corruption_signal_blocks_and_is_never_downgraded`,
`non_corruption_proof_failure_is_unavailable_not_supported`,
`the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it`,
`transport_exit_codes_are_verification_errors_not_invalid`,
`missing_proof_artifacts_are_unavailable_never_valid`, `a_failed_proof_travels_typed_next_to_its_text`.
Residual: a healthy store past its proof-size ceiling reads as `Invalid` (fail-closed;
the detail says why; a distinct `generation_failed` status is a post-hackathon decision).

**T10. Snapshot swap, tamper or replay (ADR-0008).** `Snapshot::load`
(`crates/consensus-api/src/snapshot.rs`) checks the manifest schema and every field,
the directory name against `snapshot_id`, `SHA256SUMS` over every file, every proof and
witness against the manifest digests, one anchor everywhere (manifest, response, verify
record), exactly two required captures with distinct proof bytes and distinct
`proof_blake3` (`proof_shared`), exactly `record_count` records all attributed to the
served project, and refuses `..`, absolute paths and oversized files.
`SnapshotMemoryClient::open` then requires `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` to equal the
manifest anchor and runs the verifier on both served captures, requiring `Valid` as a
`memory` proof at `semantic_reexecution` against that anchor and the same `proof_digest`
and `result_digest` the capture-time `.verify.json` recorded (`verify_capture`). Tests:
`a_well_formed_snapshot_loads_and_opens_after_verifying_both_captures`,
`tampering_with_any_file_fails_open_with_its_code`, `the_pinned_anchor_must_be_the_manifest_anchor`,
`the_snapshot_config_requires_the_pinned_anchor`, `the_required_captures_must_be_present_and_unique`,
`boot_verification_requires_a_valid_memory_proof_at_the_anchor`,
`boot_verification_requires_the_manifest_anchor_from_the_verifier_itself`,
`one_proof_cannot_stand_for_both_served_captures`, `identifiers_digests_and_paths_are_checked_strictly`,
`checksum_lines_follow_the_sha256sum_format`. The capture side binds every file to its
capture through digests recorded the moment each file was written
(`scripts/hyphae-container/capture-snapshot.sh`, `captures.index`) and
`scripts/snapshot_manifest.py check --reverify` requires the verifier to reproduce the
captured report (its `selftest` subcommand is the only automated test of the Python tool;
not run for this document). **Residual risk:** a rewrite of the whole directory that
keeps every file consistent with every other is caught only by the shape checks, by the
anchor and binary digest published out of band, and by the committed `manifest.json` and
`SHA256SUMS` in git; `proof_blake3` cannot be recomputed here (no BLAKE3 in the workspace).
The capture session has not happened yet (section 6).

**T11. Witness exposure.** Witnesses and proofs are never served: `public_view::evidence_v1`
drops `proof_file`, `witness_file`, `backend_ref` and the `proof_error` text on the public
surface (a boolean `proof_error_reported` stays); `hyphae.rs::artifact_ref` and
`program_name` keep file names out of persisted text; `.gitignore` excludes `*.proof`,
`*.witness`, `*.hyproof`, `*.hywit`; the snapshot directory is copied with `0700`/`0600`
(documented procedure). Tests: `public_views_strip_internals_and_operator_views_keep_them`,
`artifact_refs_never_carry_a_path`, `public_receipts_keep_only_the_public_invocation_fields`.
No test checks file permissions on a deployed server (G9, pending). Residual: the
operator surface shows paths by design; the verbatim public `events.ndjson` would carry
them, which is one reason it is withheld for live-declared runs (T24).

**T12. Verifier subprocess hangs, floods or inherits secrets (audit HY-2, MA-4).**
`SystemCommandRunner::run` kills the child past the deadline (default 60 s,
`CONSENSUS_HYPHAE_VERIFY_TIMEOUT_SECS`, 1–3600), caps each output stream, and removes the
Meta key variables from the child's environment (`hyphae.rs::hyphae_command`, `env_remove`
of `META_KEY_VARIABLES`). Results are cached by the SHA-256 of the proof and witness bytes
and the anchor, never by path, and never for a `VerificationError` (HY-13). Tests:
`a_verifier_past_its_deadline_is_a_verification_error_with_its_code`,
`the_system_runner_kills_a_command_past_its_deadline`, `command_output_is_capped_and_marked`,
`hyphae_children_never_inherit_the_meta_key`, `results_are_reused_by_artifact_bytes_never_by_path`,
`one_proof_covering_several_records_is_verified_once`. **Residual risk:** only the direct
child is killed, so a wrapper's own children (the Windows `cmd.exe` → `docker exec` path)
can survive; deployments must run the native binary directly. Hyphae extracts witnesses to
a temporary directory that its destructor removes; a killed verifier leaves it behind, and
no sweep exists in this tree (the checklist's G9 wording "temp-directory sweep tested"
is not met; section 6). The environment is scrubbed of the Meta key variables only, not
cleared (`env_clear()`), so other host variables reach the child.

**T13. Untrusted or unnormalized proof anchor (audit HY-10, ADR-0003 decision 6).**
The pinned anchor is trimmed, lowercased and required to be 64 hex digits at load
(`config.rs::normalize_anchor`, `HyphaeConfig`, `SnapshotConfig`); a record anchored
elsewhere is `Invalid` without running the verifier, and the pinned value is what the
verifier receives; the snapshot mode requires the pin and requires it to equal the
manifest anchor. Tests: `the_pinned_anchor_is_normalized_and_validated_at_load`,
`the_pinned_anchor_is_normalized_and_handed_to_the_verifier`,
`pinned_anchor_mismatch_is_invalid_without_running_verifier`,
`the_snapshot_config_requires_the_pinned_anchor`. **Residual risk:** the anchor is read
from the same store at capture and pinned by the operator; pinning detects change, not a
lying server. Without a pin (operator lab only) `Valid` means self-consistency against the
backend-reported anchor, and the detail says so.

**T14. Cross-project memory leakage (audit SEC-3, TC-4).** The public surface has no
project field at all (presets are `demo-repo`; `requests_refuse_unknown_fields_and_free_strings`);
the sealed snapshot serves one project, answers the isolation project with a proven empty
recall and any other project with a recorded `EvidenceUnavailable`
(`snapshot.rs::recall_with_proof`); the live adapter keeps the backend's `project` and
`scope` on every record and returns records of another project apart, which the
orchestrator drops with a recorded reason (`hyphae.rs::map_record`, `Recall::foreign`,
`orchestrator.rs::codes::FOREIGN_RECORD`). Tests: `a_recall_of_the_served_project_is_the_whole_proven_list_whatever_the_query`,
`a_recall_of_the_isolation_project_is_empty_with_its_own_proof`,
`a_recall_of_an_unknown_project_is_a_backend_error_with_a_reason`,
`records_keep_their_backend_attribution_and_foreign_ones_are_dropped`,
`the_orchestrator_records_every_proof_and_every_dropped_record`,
`foreign_records_are_dropped_and_proofs_recorded_even_when_empty`,
`fake_memory_honors_project_isolation_like_hyphae`. Residual: the operator surface still
takes a caller-chosen project by design (authenticated operator, lab stores); the live
isolation evidence is two empty recalls (L10, R08) plus the snapshot's captured empty
recall once the capture happens.

### 3.3 Public surface

**T15. Cross-visitor isolation.** Sessions are 256-bit random ids
(`crates/consensus-api/src/session.rs`, `getrandom`), minted by `POST /api/v1/session`
only, in a bounded LRU table with a TTL; a run's owner is `Owner::session(id)` = SHA-256 of
the id, and reads, streams, receipts, verbatim logs, decisions and cancels go through
owner-checked paths (`runs.rs::RunManager::snapshot`, `owned_meta`, `decide`, `cancel`;
`run_store.rs::read_run`, `open_run_as`, `meta_for`) that answer the same `404` for a run
that is not owned, not indexed, purged, expired or malformed. `GET /api/v1/runs` lists the
owner's runs only. Tests: `another_session_sees_nothing_of_a_run`,
`a_non_owner_is_told_nothing_and_never_touches_the_runs_lock_or_log`,
`a_stored_run_is_read_by_its_owner_alone_and_reading_changes_nothing`,
`owner_checked_paths_refuse_a_run_once_its_owners_access_has_ended`,
`list_for_returns_only_the_owners_live_runs_oldest_first`, `meta_for_checks_owner_and_access_without_touching_the_log`,
`sessions_are_256_bit_hex_and_owner_hashes_differ_from_ids`, `lookup_accepts_only_live_well_formed_ids`,
`the_table_is_bounded_with_lru_eviction`, `expired_sessions_are_refused`,
`concurrent_guided_runs_stay_isolated_and_ordered`. Residual: a visitor who clears cookies
loses their runs (by design); the session table is in memory, so a restart logs every
visitor out while their runs stay readable to nobody until purged.

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
`lookup_accepts_only_live_well_formed_ids`. Residual: none known.

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
the request timeout once its headers are sent (by design); Cloudflare's 125 s read timeout
is covered by the heartbeat, which has not been measured through a real tunnel (G11).

**T19. Oversized or malformed input (audit SEC-8, UI-5).** `DefaultBodyLimit` of 16 KiB
on every route (`413 payload_too_large`); every request type uses `deny_unknown_fields`
and enums, with no silent defaults (`api/types.rs`); public requests carry a scenario id
or a preset id, `attest_adult` and an optional judge code (1–`JUDGE_CODE_MAX_CHARS`
characters, no control characters), never a title, description, project or identity;
decision notes are at most 2,000 characters; operator requests bound their fields; the
`Idempotency-Key` is 1–128 characters of `[A-Za-z0-9_.:-]`; list pages are 1–50.
Deserialization errors return the first 200 characters of serde's message, which names the
field, never the body (`api/mod.rs::ApiJson`, `bounded_detail`). Tests:
`csrf_content_type_unknown_fields_and_size_are_enforced`, `requests_refuse_unknown_fields_and_free_strings`,
`operator_request_bounds_its_fields`, `modes_are_explicit_and_credentials_are_required`,
`the_module_exposes_no_way_to_carry_visitor_text`. Residual: none known.

**T20. Duplicate runs and duplicate decisions on retry (audit UI-1, TC-8, UI-7, RT-8,
SEC-12).** A creation may carry an `Idempotency-Key`: the same key with the same body
(SHA-256 of the parsed body) returns the run already made, a different body is `409
idempotency_key_reused`, and keys are scoped per owner in a bounded LRU
(`api/mod.rs::CreationKeys`, `v1.rs::replayed_creation`). A decision requires an
`Idempotency-Key` and an `expected` block (recommendation kind and last sequence); a
mismatch is `409 decision_conflict`, a run still deliberating `409
run_not_awaiting_decision`, a reused key with another choice `409 idempotency_key_reused`
(`runs.rs::RunManager::decide`; the event log refuses the same id with different content).
Tests: `a_retried_creation_with_the_same_key_returns_the_same_run`,
`creation_keys_are_scoped_per_owner_and_bounded`, `a_guided_run_is_accepted_streamed_decided_and_receipted`
(409 cases), `two_concurrent_decisions_record_exactly_one`,
`a_decision_during_deliberation_is_refused_and_live_frames_say_live`,
`two_handles_racing_two_decisions_record_exactly_one`, `a_decision_race_on_a_stored_run_records_one_decision_and_one_terminal_head`,
`reused_event_id_with_different_content_is_a_conflict`, `approval_retry_with_same_idempotency_key_is_noop`,
`reusing_an_idempotency_key_across_runs_records_both_decisions`. Residual: the creation
key table is in memory and bounded, so after a restart or eviction a retried submit makes
a second run (bounded by the rates and, for live runs, the ledger).

**T21. Geo evasion and unknown countries (ADR-0007 duty 7).** `api/mod.rs::portal_geo`
refuses every public route (static pages and `/research` included) with `451
geo_blocked_portal` for the 23 portal codes and `451 geo_unknown` when `CF-IPCountry` is
absent behind the socket; the live gate additionally refuses South Korea and every unknown
value (`XX`, `T1`, malformed, missing) with `451 geo_blocked_live` (`live/geo.rs::GeoPolicy::decide`,
`live/gate.rs`). Lists come from `CONSENSUS_GEO_*` with the owner's defaults. Tests:
`the_portal_block_covers_every_route_and_fails_closed_without_a_country`,
`korea_blocks_live_mode_only_and_an_unknown_country_blocks_it_too`,
`ukraine_is_blocked_on_the_whole_portal`, `south_korea_is_blocked_for_live_mode_only`,
`hong_kong_and_macao_are_blocked_like_the_other_restricted_territories`,
`unknown_country_is_refused_for_live_and_allowed_on_the_portal`,
`allowed_countries_pass_both_surfaces_and_lowercase_input_is_normalized`, `custom_lists_drive_the_policy`,
`default_lists_match_the_owner_decision`, `list_parsing_refuses_junk_and_empty`,
`geo_codes_are_the_geo_modules_fixed_public_codes`. **Residual risk:** country comes
from Cloudflare's geolocation of the client address; a VPN or proxy defeats it (an
accepted limit of any geo-block), and a development bind that trusts no proxy header
blocks nothing. The Cloudflare rule is the primary control and has not been configured
yet (G10).

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
with no call), `a_protocol_label_alone_never_hides_dropped_objections`. Residual: the web
client that renders these axes does not exist yet beyond a legend
(`apps/web/src/components/ProvenanceLabels.tsx`), so G7 is pending.

**T23. Error and detail leakage (audit SEC-9, MA-4, DOC-6).** One error envelope
(`api/error.rs::ApiError`: stable `code`, fixed `message`, optional `retry_after_secs`),
internal errors logged server-side and answered as `internal`; public views replace
recorded failure reasons with fixed statements per code (`public_view::failure_message`,
`unavailable_message`), drop verifier output, `proof_error` text, backend references,
paths and the prompt and schema digests; live-mode refusals answer the ledger's fixed
sentences, never its text (`live/gate.rs::refusal_message`); `/api/v1/health` is static
facts only; the Meta adapter never puts a response body or a URL into an error, an event
or a receipt (`meta.rs::http_failure`, `without_url()`), and Hyphae RPC errors keep only a
code and a bounded message. Tests: `codes_serialize_snake_case_and_carry_their_status`,
`failure_messages_never_echo_the_reason`, `public_views_strip_internals_and_operator_views_keep_them`,
`public_receipts_keep_only_the_public_invocation_fields`,
`every_refusal_and_trip_reason_maps_to_a_code_status_and_fixed_message`,
`upstream_bodies_and_the_key_never_reach_errors_or_the_log`, `the_meta_key_never_appears_in_any_output`,
`rpc_errors_keep_only_their_code_and_a_bounded_message`, `operator_views_keep_the_raw_failure_reason`.
**Residual risk:** the hashed chain still holds English free text written by the adapters
(a failure reason, up to 300 characters of verifier stderr in a verification detail,
`GENERIC_CORRUPTION_NOTE`); the public views strip it, but the verbatim `events.ndjson` of
a live-declared run would not, hence T24.

**T24. The verbatim chain of a live run.** `public_view::ndjson_publishable` publishes
`GET /api/v1/runs/{id}/events.ndjson` on the public surface only for a run declared
scripted over simulated or no evidence whose records agree; a live-declared run's owner gets
`404 ndjson_unavailable` and keeps the detail and the receipt (`v1.rs::run_events_ndjson`).
The operator surface serves every chain. Tests: `public_ndjson_is_published_only_for_scripted_chains`,
`ndjson_is_published_only_for_chains_that_cannot_carry_upstream_text`,
`the_operator_gets_the_whole_chain_of_a_live_run`. Residual: a visitor cannot recompute a
live run's chain from the verbatim bytes; they can download the receipt and run
`consensus-api verify` on a log the operator exports. Lifting this needs the adapters to
record only codes in the chain (section 6).

**T25. Path traversal and hostile ids.** Run ids are `run-` plus a lowercase UUID v4 and
are parsed before any path is built (`run_store.rs::parse_run_id`; unknown ids are refused
from the index alone, and lookups create nothing); research files are served only by a
path that exists in `research/MANIFEST.json`, never through a path join
(`showcase.rs::research_file`); static pages reject `..`, empty segments, backslashes,
NUL and drive letters (`static_files.rs::StaticSite::serve`); snapshot paths must be plain
relative paths and a log replaced by a symlink is refused. Tests:
`parse_run_id_accepts_exactly_the_uuid_id_gen_format`, `unknown_and_malformed_ids_never_create_a_file`,
`research_files_are_served_by_manifest_path_only`, `traversal_is_refused_and_missing_pages_get_the_404_page`,
`identifiers_digests_and_paths_are_checked_strictly`, `a_log_replaced_by_a_symlink_is_refused`,
`research_files_are_served_verbatim_with_their_digest`. Residual: none known.

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
not run here). Tests (Rust): `security_headers_and_per_page_csp_are_present`,
`the_console_is_served_with_a_hash_csp_and_no_store`, `csp_hash_matches_the_specification_example`.
**Residual risk:** Cloudflare features that inject scripts (Rocket Loader, Email Address
Obfuscation, Zaraz, Web Analytics) would break the hash CSP and must be off; HSTS is the
edge's; neither is verifiable from the repository (G6, pending). `style-src-attr
'unsafe-inline'` is allowed for the framework's inline style attributes.

### 3.4 Cost and abuse

**T27. Budget exhaustion on the owner's key (audit SEC-2, MA-2, MA-5; ABUSE-1).** No
visitor text reaches a model; a live run is one of five compiled-in presets
(`live/presets.rs`, byte-equal to the recorded batteries). Admission runs in a fixed
order, stopping at the first refusal and spending nothing before the ledger has reserved
the run's worst case (`live/gate.rs::Inner::admit_blocking`): preset, geography, 18+
attestation, Turnstile, lane, reservation, metered model client, deliberation, grant. The
ledger (`live/budget.rs::Ledger`) is append-only JSONL per UTC day plus an atomically
rewritten lifetime summary, fsync'd, serialized through an exclusive file lock shared by
every process that holds the key; it reserves `(run token cap + max_completion_tokens +
input cap) × output price` ($1.88 at the defaults) per run, draws before **every** HTTP
attempt (retries included) and settles each draw once (`meta.rs::MetaModelClient::with_meter`,
`draw_attempt`, `settle_attempt`): actual usage on success, zero on
400/401/402/403/404/413/429 and on an attempt that was never sent, the full estimate on
timeouts, post-send transport errors, other statuses and usage-less successes, never
refunded. Caps: $10 per UTC day with an even refill and a 25% burst at 00:00, $60 lifetime,
a $15 judge slice with its own daily cap (default the slice), 6 reservations per rolling
hour per lane, 1 concurrent public reservation per process; the configuration refuses caps
that could never admit a run and, when `CONSENSUS_LIVE_EXPOSURE_DAYS` is set, a daily cap
times the days above the lifetime cap. The reservation is released exactly once when the
run ends, is dropped, or fails to start, with bounded retries, and a failed release is
logged with its id for `Ledger::release_by_id`. Tests (ledger):
`fifty_concurrent_reservations_never_exceed_the_cap`, `crash_between_reserve_and_settle_counts_worst_case_after_reopen`,
`unknown_outcomes_charge_the_full_estimate_and_never_refund`, `retries_take_one_draw_per_attempt`,
`the_reservation_is_a_hard_ceiling`, `lifetime_cap_reached_trips_across_reopen`,
`hourly_admission_cap_and_even_daily_refill_with_burst`, `public_lane_never_reserves_into_the_judge_slice`,
`judge_lane_admits_while_public_is_exhausted_and_never_beyond_its_slice`,
`the_judge_lane_has_its_own_daily_cap`, `input_and_output_caps_refuse_before_anything_is_written`,
`two_handles_on_one_directory_serialize_through_the_lock`, `exact_decimal_money_over_ten_thousand_settlements`,
`a_torn_tail_is_cut_and_a_corrupt_line_fails_closed`, `the_lifetime_summary_is_a_floor_when_day_files_go_missing`,
`a_stale_summary_does_not_override_newer_day_files`, `a_failed_release_keeps_the_handle_and_the_slot_until_it_succeeds`,
`release_by_id_reconciles_a_crashed_reservation`, `an_attempt_that_was_never_sent_settles_at_zero_and_is_recorded_as_such`;
(gate and adapter): `admission_refuses_in_order_and_charges_nothing`,
`the_live_gate_is_asked_after_the_slot_and_refunded_when_the_start_fails`,
`a_live_run_is_metered_streamed_and_published_without_the_key`,
`a_start_that_fails_after_admission_releases_the_reservation`,
`a_panicking_deliberation_still_releases_its_reservation`,
`a_draw_refused_mid_run_fails_the_run_at_its_stage_and_charges_once`,
`a_connection_dropped_after_the_request_draws_twice_and_settles_both`,
`dropping_a_metered_deliberation_releases_its_reservation_once`,
`a_dropped_deliberation_retries_its_release_until_the_ledger_is_free`,
`a_deliberation_dropped_on_an_async_worker_still_releases`,
`a_release_that_cannot_reach_the_ledger_is_logged_and_frees_the_slot`; (configuration):
`defaults_are_the_owner_decisions`, `cross_checks_hold_the_caps_together`,
`enabling_live_mode_requires_the_ledger_dir_secret_and_hostname`,
`every_preset_equals_the_battery_script_byte_for_byte`, `the_module_exposes_no_way_to_carry_visitor_text`.
**Residual risk, stated plainly:** Meta offers no hard spending cap, so this ledger is the
only cap; one UTC day's exposure is `daily + judge daily` ($25 at the defaults); a process
that crashes between reserve and release keeps the worst case counted until an operator
reconciles it; the public per-run token cap is enforced by the orchestrator after each call
and by the reservation's ceiling per attempt; the operator console's own live runs do not
go through the ledger (section 6).

**T28. Turnstile replay (the high finding fixed in `aed879d`).** Siteverify is called with
a fresh UUID v4 as `idempotency_key` on every verification, never the request's
`Idempotency-Key`: Cloudflare's idempotency is global and immediate, while the route's
replay is owner-scoped and recorded only after the start, so forwarding the header let one
solved challenge start a run per session or several at once. A token is therefore
single-use at Siteverify; a retry whose first attempt succeeded is answered from the stored
creation before the gate is asked; `remoteip` is not sent (the client key is a prefix). The
verifier also requires `success`, the expected `action` (`live-run`, which must differ
from the session action), the expected `hostname`, and a `challenge_ts` within 300 s
(+60 s skew); tokens over 2,048 bytes never reach Cloudflare; test secrets are refused in
public mode (`live/turnstile.rs`, `live/config.rs::load_turnstile`). Tests:
`a_token_replayed_from_another_session_starts_no_second_run`,
`the_same_token_and_key_pass_twice_so_the_gate_never_reuses_a_key`,
`a_retry_with_the_same_idempotency_key_passes_and_a_new_key_does_not`,
`a_matching_fresh_token_passes_and_the_call_carries_ip_and_key`, `wrong_action_is_refused`,
`wrong_hostname_is_refused`, `hostname_comparison_ignores_case`, `stale_or_far_future_challenge_ts_is_refused`,
`success_false_and_outages_fail_closed`, `empty_and_oversized_tokens_never_reach_cloudflare`,
`http_transport_posts_the_documented_form_and_parses_the_answer`,
`http_transport_treats_non_200_and_unparseable_bodies_as_refusals`,
`test_secrets_are_recognized_and_real_ones_are_not`, `cloudflare_test_secrets_are_refused_in_public_mode_only`,
`secret_is_redacted_and_test_secrets_are_recognized`. Residual: a Turnstile solve spent on
a start that fails after Siteverify cannot be refunded (the visitor solves again); a
Siteverify outage disables live mode (`503 turnstile_unavailable`), which is the intended
direction.

**T29. Judge-code probing.** Codes exist only as SHA-256 digests in the codes file; a
presented code is hashed and compared against every digest in constant time; an unknown
code falls to the public lane silently and nothing in any response says whether codes
exist (`live/judge.rs::JudgeCodes::verify`, `live/gate.rs` step (e)); malformed lines are
reported by number, never content. Tests: `verifies_hashed_codes_and_refuses_everything_else`,
`malformed_lines_are_reported_by_number_only`, `judge_codes_load_as_digests_and_malformed_lines_are_never_echoed`,
`a_known_judge_code_takes_the_judge_lane_and_an_unknown_one_the_public_lane`. **Residual
risk:** a probe is a full public live run (Turnstile solved, public budget reserved): the
public caps bound the cost, and the prober learns nothing from the response; a judge whose
code is mistyped silently spends the public lane instead of theirs.

**T30. Hourly, daily, lifetime caps and the kill switch.** See T27 for the caps. The
ledger trips, persistently, when a settlement exceeds the attempt's output bound or
estimate, on 401/402/403 from the provider, when the lifetime cap is reached, or when the
kill-switch file (`<ledger dir>/kill-switch` or `CONSENSUS_LIVE_KILL_SWITCH_FILE`) exists;
tripped, it refuses every reservation and every draw, so in-flight runs stop at their next
call; only an operator `Ledger::reset` clears it, and `/ready` and `/capabilities` report
the file as a trip the moment it exists (`live/gate.rs::Inner::trip_state`). Tests:
`kill_switch_file_refuses_everything_until_an_operator_reset`, `completion_over_max_trips_and_the_trip_survives_reopen`,
`a_charge_above_the_attempt_draw_trips`, `provider_key_refusals_trip_and_other_no_retry_statuses_charge_zero`,
`lifetime_cap_reached_trips_across_reopen`, `the_summary_floor_covers_the_judge_lane_and_a_reached_cap_trips_at_open`,
`ledger_refusals_answer_with_their_codes_and_the_kill_switch_trips`,
`a_draw_refused_mid_run_fails_the_run_at_its_stage_and_charges_once`,
`snapshot_cached_never_waits_for_the_lock_and_never_writes`. Residual: the kill switch is
a file on the server; using it needs shell access.

**T31. Duplicate billing on retries (audit MA-8, TC-7).** Only 429 and 503 are retried,
after `Retry-After` when it fits within 30 s and leaves the retry at least a second, else
full-jitter backoff from 500 ms, within one time budget per call (default 120 s; 1–3600)
that also covers the body read; a connection that never opened is retried; a connection
that broke after the request was sent is retried **exactly once** (default on,
`CONSENSUS_META_RETRY_TRANSPORT_ONCE`) and the lost attempt is charged at its full
estimate; timeouts, 504 and non-retryable 4xx are never retried (`meta.rs::MetaModelClient::send`).
Tests: `only_429_and_503_are_retried`, `retry_after_in_seconds_is_honored_on_429`,
`retry_after_as_an_http_date_is_honored_on_503`, `a_retry_after_beyond_the_budget_fails_at_once`,
`a_retry_after_that_leaves_no_time_for_the_retry_fails_at_once`,
`a_refused_connection_is_retried_and_a_dropped_one_is_not_with_the_transport_retry_off`,
`a_connection_dropped_after_the_request_is_retried_once_by_default`,
`one_budget_bounds_the_whole_call_including_the_body_read`, `persistent_503_retries_once_then_unavailable_and_500_is_never_retried`,
`retries_take_one_draw_per_attempt`, `a_connection_dropped_after_the_request_draws_twice_and_settles_both`,
`invocation_records_schema_name_attempts_and_finish_reason`. Residual: the one post-send
retry can bill one duplicate generation that no invocation records (the ledger counts it;
the receipt shows `attempts: 2`).

**T32. Operator-console runs outside the ledger.** `POST /api/v1/operator/runs` with
`model=meta` builds a `MetaModelClient` without a meter (`v1.rs::operator_meta_start`),
so an operator's live runs are not counted against the daily or lifetime caps, although
the ledger library supports an `operator` lane
(`operator_runs_debit_the_same_ledger_without_pacing`). Mitigation today: the operator
surface is loopback-only behind a bearer token and started on demand. **Residual risk:**
the owner's own runs can exceed the public caps; this is an accepted follow-up (ADR-0007
implementation note; section 6) and the checklist's G3 criterion "an operator-console
live run debits the same ledger" is not met end to end.

**T33. Request floods and resource exhaustion.** Per-client fixed-window rates keyed by
IPv4 /32 or IPv6 /64 (sessions 10/min, runs 6/min, streams 30/min) in bounded LRU tables
(50,000 entries); deliberation pools of 4 guided and 1 live with a 2 s admission wait and
a queue of 32 (`429 too_many_runs`); a visitor-run ring of 2,000 that purges the oldest
resting runs to admit a new one and answers `503 storage_full` only when nothing can be
freed; visitor runs readable for 24 h and deleted after 7 days by a purge loop every 10
minutes; a 30 s request timeout (`408`); store reads at most once per 2 s for `/ready`
(`limits.rs`, `runs.rs::RunManager::reserve`, `make_room`, `api/v1.rs::store_health`,
`server.rs::run_server`). Tests: `rates_streams_and_client_keys_are_enforced_per_slash_64`,
`rate_table_refuses_past_the_window_and_stays_bounded`, `ipv6_addresses_in_one_slash_64_share_a_key`,
`the_guided_ring_frees_the_oldest_resting_run_and_refuses_only_when_nothing_can_be_freed`,
`the_ring_frees_the_oldest_session_runs_ended_access_first_and_skips_held_ones`,
`purge_deletes_only_expired_session_runs_whole_and_records_each_purge`, `the_operator_surface_has_its_own_generous_limits`.
No test exercises the `408` itself or the purge loop's timer. **Residual risk:** an
attacker with many /64 prefixes evades the per-client tables (Cloudflare's rate-limit rule
is the complement); under heavy load the ring can evict a visitor's run before its 24 h
window ends (open owner decision); the global stream cap (512) is shared by everyone.

### 3.5 Credentials

**T34. The Meta key in files, environment, argv, logs or `/proc`.** The key is read from
`CONSENSUS_META_API_KEY_FILE` first (systemd `LoadCredential` on the server), else the
first of `CONSENSUS_META_API_KEY`, `MODEL_API_KEY`, `META_API_KEY`
(`config.rs::MetaConfig::from_lookup`); it lives in a `Zeroizing<String>` (`ApiKey`),
prints as `ApiKey(<redacted>)`, has no `Display`, is checked for the documented shape
without being shown, is sent as a sensitive header value only (`meta.rs::MetaModelClient::new`,
`set_sensitive(true)`, redirects disabled, https required except to loopback), and never
reaches argv, events, receipts or child processes (`hyphae.rs::hyphae_command`). The
public server forces `public_mode`, which refuses the raw-draft debug directory
(`server.rs::public_meta_config`, `config.rs::MetaConfig::validate`). Tests:
`the_key_file_variable_comes_first_and_its_errors_name_it`, `a_credential_file_key_is_read_trimmed_and_checked`,
`meta_key_precedence_is_consensus_then_model_then_meta`, `the_key_never_prints_and_is_trimmed`,
`unusable_meta_settings_are_errors_naming_the_variable`, `the_debug_dir_is_refused_in_public_mode`,
`the_debug_dir_keeps_sanitized_files_without_the_key_and_never_in_public_mode`,
`hyphae_children_never_inherit_the_meta_key`, `upstream_bodies_and_the_key_never_reach_errors_or_the_log`,
and the canary `the_meta_key_never_appears_in_any_output` (a fake key in the environment,
a refusing endpoint; the key is searched for in every response, SSE frame, stored file and
the health body, whose keys are asserted). **Residual risk:** a key passed through a key
variable instead of the file is visible in `/proc/<pid>/environ` to anyone who can read
that process; the development machine's user environment holds the key, which is why the
Node toolchain runs only in Docker (ADR-0009); `/proc/<pid>/environ`, the client bundle and
the deployed environment are not covered by any test (G4).

**T35. Other secrets: the Turnstile secret, judge codes, the operator token.** The
Turnstile secret is read from a file into `TurnstileSecret` (redacted `Debug`, no
serialization) and sent only in the Siteverify form body; judge codes exist only as
digests; the operator token comes from `CONSENSUS_OPERATOR_TOKEN_FILE` (at least 32
characters) or is generated (32 random bytes) and printed once, and is compared in
constant time (`api/mod.rs::OperatorAuth`, `constant_time_eq`). Tests:
`secret_is_redacted_and_test_secrets_are_recognized`, `judge_codes_load_as_digests_and_malformed_lines_are_never_echoed`,
`constant_time_eq_compares_whole_values`, `every_api_route_needs_the_bearer_token`. **Residual
risk:** a generated operator token is printed to stdout (anyone who reads the server's
output holds it; use the token file); the token has no expiry or rotation beyond a restart.

**T36. Hyphae credentials on the public server.** None exist there: the public server
holds no `HYPHAE_NATIVE_API_KEY_FILE`, runs no daemon and has no writer path; the sealed
instance's credentials are destroyed at the end of the capture session
(`capture-snapshot.sh` step 7, which lists names and sizes only and refuses to continue
if the directory survives). The operator lab passes the reader key as a file path to
`hyphae mcp` and never `--allow-write`. Tests: `hyphae_needs_a_key_file_and_a_daemon_and_bounds_the_verifier`
(asserts no `--allow-write`), `connect_rejects_surface_without_memory_recall`. No test
covers the deployed server (G9). Residual: witnesses of the sealed instance carry the
verifiers of its destroyed keys; they stay private.

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
`refused_appends_are_neither_stored_nor_published`, `an_append_the_disk_refuses_is_neither_stored_nor_published`,
`a_panicking_publisher_never_unwinds_into_the_writer`, `a_second_writer_ending_the_run_mid_deliberation_never_breaks_replay`,
`a_panicking_model_fails_only_its_run`, `recovery_closes_interrupted_runs_records_terminal_heads_and_reports_the_rest`,
`a_run_whose_deliberation_died_is_closed_when_the_store_starts_again`,
`startup_recovery_closes_crashed_runs_and_reports_broken_ones`, `interrupt_reopen_and_continue_to_decision`,
`write_atomic_never_leaves_a_partial_file_at_its_path`, `a_sidecar_write_that_fails_midway_leaves_no_sidecar_and_no_run`.
Residual: on Windows the directory entry is not fsync'd (std cannot open a directory);
locks are std file locks, so a process that holds a lock and is frozen keeps its run
busy (`409 run_busy`).

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

**T40. Dependencies, toolchains and CI.** Rust toolchain pinned by `rust-toolchain.toml`
(1.99.1); every gate runs `--locked`; `deny.toml` allows exactly the permissive license
expressions in `Cargo.lock`, denies yanked crates and wildcard dependencies and allows only
crates.io; `THIRD_PARTY_NOTICES.md` is generated from `cargo metadata`; the CI workflow
(`.github/workflows/ci.yml`) pins every action by commit SHA and the Playwright image by
digest, runs fmt, clippy `-D warnings`, cargo-deny, tests, the conformance example and the
demo crate; `scripts/linux-gates.sh` runs the same gates in an Ubuntu 24.04 container with
the repository mounted read-only and no host environment. The web toolchain runs only in
Docker (`apps/web/scripts/in-docker.sh`: digest-pinned Node image, only `apps/web`
mounted, `scripts/` and `pnpm-workspace.yaml` read-only, no environment;
`playwright-docker.sh`: `--network none`, `--cap-drop ALL`, `no-new-privileges`,
read-only `/app`), with pnpm pinned by `packageManager` hash, exact versions, a frozen
lockfile, `minimumReleaseAge: 4320` minutes and every dependency build script reviewed
and denied (`allowBuilds`); `astryx init` is never run because it writes agent-instruction
files. Tests: `committed_contract_matches_the_code`, `health_reports_the_digest_of_the_committed_schemas`
(the API contract files drift-checked); the gates themselves are the test. **Residual
risk:** `cargo deny check` and `pnpm audit --prod` were last reported clean on 2026-10-27
and were not re-run for this document; the workflow has not run on GitHub yet (the
repository is private and only the owner pushes); there is no secret scanner (gitleaks)
or push protection; the Linux fresh-clone run is still owed (G8).

### 3.8 Deployment (short; the runbook owns the detail, and no runbook exists yet)

**T41. Listener exposure and header forgery (ABUSE-2).** `serve-public` binds a unix
socket (`CONSENSUS_PUBLIC_SOCKET`, default `/run/consensus/public.sock`, mode 0660;
`server.rs::serve_unix`) or a **loopback** TCP address for development (`CONSENSUS_PUBLIC_BIND`;
anything else is refused); with the socket `CONSENSUS_ALLOWED_ORIGINS` and
`CONSENSUS_ALLOWED_HOSTS` are required and `CONSENSUS_TRUST_PROXY_HEADERS=false` is refused;
`serve-operator` binds loopback only (`CONSENSUS_OPERATOR_BIND`, default `127.0.0.1:8787`).
Tests: `non_loopback_public_bind_is_refused`, `socket_mode_requires_explicit_origins_and_hosts`
(unix-only), `tcp_dev_mode_derives_hosts_and_origins_and_trusts_no_proxy`,
`trusted_proxy_policy_requires_the_header`, `operator_defaults_to_loopback_8787`,
`invalid_values_name_the_variable`. No test binds the unix socket or checks its mode.
**Residual risk:** any process in the socket's group can present forged `CF-*` headers;
the tunnel ingress (public hostname → socket, a final 404 catch-all) is reviewed by hand
(G1); systemd hardening (`LoadCredential`, `DynamicUser`, `ProtectSystem`, a private
`/tmp` for the verifier) is documented in prose only, with no unit file in the repository.

**T42. Edge features that rewrite responses.** See T26: Rocket Loader, Email Address
Obfuscation, Zaraz and Web Analytics must be off for the hostname, `cf-cache-status` must
never be HIT on `/api` (the origin sends `no-store`), HSTS is set at the edge, the WAF geo
rules and the one rate-limit rule are configured by the owner. None of this is testable
from the repository (G6, G10, pending).

**T43. Graceful shutdown and restarts.** SIGTERM/Ctrl-C cancels every active run at its
next checkpoint, waits up to 60 s, and refuses new runs meanwhile
(`server.rs::graceful`, `runs.rs::RunManager::shutdown`); startup recovery closes runs a
dead process left mid-deliberation as `interrupted` before anything is served. Tests:
`shutdown_cancels_active_runs_and_refuses_new_ones`, `shutdown_is_recorded_with_stage_shutdown`,
`cancel_at_every_stage_records_a_cancelled_failure`, `a_run_whose_deliberation_died_is_closed_when_the_store_starts_again`.
The signal handler itself is untested. Residual: a redeploy still loses the tokens of the
runs it cancels; a staging rehearsal is pending (G11).

## 4. Status of the 2026-10-27 audit findings at `d1e58f1`

Statuses: **closed** (with where and how), **partly** (what remains), **accepted** (with
the rationale), **open**, **pending** (needs the deployment, the capture session or the
owner), **superseded**. Commits are from `git log 72e422a..d1e58f1`; "lane" names the
merged lane (S-b1 `9dca9f1`, S-b2 `f52b68f`, R-b `6c98fb4`/`e22399a`, D2 `4d4f01a`/`5455e69`,
live gate `d1e58f1`). The red-team ids (ABUSE-*, LC-*) were defined in the plan, which is
kept outside the repository; the rows below use the meaning the code comments give each
id and say so when an id is not described anywhere in the tree.

| Id | Finding (short) | Status at `d1e58f1` | Where |
|---|---|---|---|
| SEC-1 | No auth or ownership on reads; loopback is no boundary behind a tunnel | closed in code; pending deployment probe (G1, G2) | sessions + owner-scoped 404 (`session.rs`, `runs.rs`, `run_store.rs`), listener split (`server.rs`); `another_session_sees_nothing_of_a_run`; lane S-b1 |
| SEC-2 | Anonymous callers start unlimited live runs on the server key | closed in code (gate not yet startable, section 6) | `live/gate.rs`, `live/budget.rs`, `runs.rs::reserve`; `admission_refuses_in_order_and_charges_nothing`; `d1e58f1` |
| SEC-3 | Caller-chosen `project_id` reads any project | closed for the public surface; accepted on the operator surface | no project field in public requests (`api/types.rs`), presets fixed to `demo-repo`, snapshot serves one project; `requests_refuse_unknown_fields_and_free_strings` |
| SEC-4 | One std mutex held for whole runs inside async handlers | closed | per-run `SharedRunLog`, blocking pool, `RunManager` (`runs.rs`); `a_decision_during_deliberation_is_refused_and_live_frames_say_live`; `200b0df`, `92bb836` |
| SEC-5 | Synchronous POST outlasts the edge timeout | closed in code; heartbeat through a tunnel pending (G11) | `202` + SSE with 15 s heartbeats (`v1.rs`, `stream.rs`); `a_guided_run_is_accepted_streamed_decided_and_receipted` |
| SEC-6 | Every read re-parses a growing multi-tenant log | closed | per-run files, index, ring, retention, pagination (`run_store.rs`, `v1.rs::list_runs`); `the_guided_ring_frees_the_oldest_resting_run_and_refuses_only_when_nothing_can_be_freed` |
| SEC-7 | SSE replay unauthenticated and uncapped | closed | owner check, caps, per-run ids, `stream_end` (`stream.rs`, `limits.rs`); `sse_resumes_from_every_sequence_without_duplicates_or_gaps`, `rates_streams_and_client_keys_are_enforced_per_slash_64` |
| SEC-8 | No field limits; 2 MB strings into prompts | closed | 16 KiB body, `deny_unknown_fields`, enums, bounded fields, no public free text (`api/mod.rs`, `api/types.rs`); `csrf_content_type_unknown_fields_and_size_are_enforced` |
| SEC-9 | Internal errors, upstream bodies, verifier output and paths in responses and events | closed for responses and public views; partly for the chain (free text remains inside hashed records, so live ndjson is withheld, T24) | `api/error.rs`, `public_view.rs`, `meta.rs`, `hyphae.rs::artifact_ref`; `failure_messages_never_echo_the_reason`, `upstream_bodies_and_the_key_never_reach_errors_or_the_log` |
| SEC-10 | Operator token printed, compared in non-constant time; self-asserted identities | closed except the printed generated token (accepted: use the token file) | `api/mod.rs::OperatorAuth`, `server.rs::operator_token`, identity from the channel; `constant_time_eq_compares_whole_values`, `every_api_route_needs_the_bearer_token` |
| SEC-11 | No Host or Origin check (DNS rebinding) | closed | `host_check`, `csrf_check` on both surfaces; `host_and_origin_are_checked`, `csrf_content_type_unknown_fields_and_size_are_enforced` |
| SEC-12 | A decision not bound to the recommendation seen | closed | `DecisionRequest.expected` → `409 decision_conflict`; `409` during deliberation (`runs.rs::decide`); `a_guided_run_is_accepted_streamed_decided_and_receipted` |
| SEC-13 | No security headers, no CSP | closed at the origin; HSTS and edge pending (G6) | `security_headers`, per-page CSP (`static_files.rs`), console CSP; `security_headers_and_per_page_csp_are_present`, `the_console_is_served_with_a_hash_csp_and_no_store` |
| SEC-14 | CSRF resistance implicit and untested | closed | as SEC-11; `__Host-` `SameSite=Strict` cookie; `a_host_prefixed_cookie_is_never_emitted_without_secure` |
| SEC-15 | No graceful shutdown, request timeouts or connection limits | closed (connection limits delegated to cloudflared); `408` untested | `server.rs::graceful`, `TimeoutLayer` 30 s; `shutdown_cancels_active_runs_and_refuses_new_ones` |
| SEC-16 | Simulated proofs shown as a green `valid` | closed in the API (`simulated` flag, detail kept for fakes); pending in the web UI (G7) | `public_view::verification_v1`; `provenance_is_derived_from_records_and_cross_checked` |
| ABUSE-1 | Spend on the owner's key must be reserved before anything runs | closed for the public lane; operator surface unmetered (open) | `live/budget.rs::Ledger`; `fifty_concurrent_reservations_never_exceed_the_cap`, `crash_between_reserve_and_settle_counts_worst_case_after_reopen` |
| ABUSE-2 | Forged `CF-*` headers | closed by the unix socket and the trust policy; deployment pending | `public_config.rs`, `limits.rs::client_key`; `trusted_proxy_policy_requires_the_header` |
| ABUSE-3 | Web toolchain rewriting what the host runs | closed in the scripts (read-only mounts, no network, dropped capabilities); no automated test | `apps/web/scripts/in-docker.sh`, `playwright-docker.sh`; `17fbe20` |
| ABUSE-4 | Per-client keying must be /64 with bounded tables | closed for rates and streams; live per-client concurrency open | `limits.rs`; `ipv6_addresses_in_one_slash_64_share_a_key`, `rates_streams_and_client_keys_are_enforced_per_slash_64` |
| ABUSE-5 | Inline scripts need per-page CSP hashes | closed at the origin; browser verification pending (G6) | `apps/web/scripts/csp-manifest.mjs`, `static_files.rs`; `security_headers_and_per_page_csp_are_present` |
| ABUSE-6 | Cookies set by GET or static responses | closed | `session.rs`, `v1.rs::create_session`; `sessions_are_minted_by_post_only_and_carry_the_right_attributes` |
| ABUSE-7 | Ids before paths; owner-checked reads | closed | `run_store.rs::parse_run_id`, `read_run`; `unknown_and_malformed_ids_never_create_a_file`, `a_non_owner_is_told_nothing_and_never_touches_the_runs_lock_or_log` |
| ABUSE-8 | Visitor free text reaching a model | closed | `live/presets.rs`, `api/types.rs`; `the_module_exposes_no_way_to_carry_visitor_text` |
| ABUSE-9 | Admission order, Turnstile fail-closed, geo on live | closed | `live/gate.rs`, `live/turnstile.rs`, `api/mod.rs::portal_geo`; `admission_refuses_in_order_and_charges_nothing`, `the_portal_block_covers_every_route_and_fails_closed_without_a_country` |
| ABUSE-10 | not described in the repository | — | no code or document reference |
| ABUSE-11 | Geographic policy (owner decision lists) | closed in code; Cloudflare rule pending (G10) | `live/geo.rs`; `ukraine_is_blocked_on_the_whole_portal`, `south_korea_is_blocked_for_live_mode_only` |
| ABUSE-12 | not described in the repository | — | no code or document reference |
| MA-1 | Evaluator ids dropped silently | closed | keyed per-call schema + contract (`meta.rs`, `contract.rs`); `every_ma1_variant_fails_evaluate_and_never_reaches_supported`, `a_missing_key_is_malformed_output_naming_the_claim_or_item`; `1ee386a`, `3d1aeec` |
| MA-2 | Live mode spends the environment key for anyone | closed by design (no BYOK, owner key, gate, key file); gate not yet startable | `server.rs::public_meta_config`, `config.rs::META_KEY_FILE_VARIABLE`; `the_meta_key_never_appears_in_any_output`; `83e0829`, `d1e58f1` |
| MA-3 | Run wall time incompatible with an edge proxy | closed | `202` + SSE, call budget 1–3600 s, idempotent creation; `one_budget_bounds_the_whole_call_including_the_body_read`, `a_retried_creation_with_the_same_key_returns_the_same_run` |
| MA-4 | Upstream error text and inherited environments leak | closed (key variables removed from children; bodies never persisted) | `meta.rs::http_failure`, `hyphae.rs::hyphae_command`; `upstream_bodies_and_the_key_never_reach_errors_or_the_log`, `hyphae_children_never_inherit_the_meta_key`; lane R-b |
| MA-5 | Per-call spend unbounded and partly invisible | closed | `max_completion_tokens`, `finish_reason`, ledger draws, failed calls recorded; `truncated_output_has_its_own_code_and_keeps_the_billed_call`, `input_and_output_caps_refuse_before_anything_is_written`; `30b598f` |
| MA-6 | Visitor and model text outside untrusted framing | closed | `meta.rs` blocks; `the_task_is_an_untrusted_bounded_block_and_cuts_are_marked`, `forwarded_model_text_stays_inside_untrusted_blocks_on_one_line` |
| MA-7 | Untrusted content forges prompt structure | closed (defense in depth) | `meta.rs::untrusted_line`, `json_string`; `no_delimiter_variant_survives_in_any_untrusted_field`, `evidence_is_a_json_array_and_no_record_can_forge_another` |
| MA-8 | Retry policy bills twice, ignores `Retry-After` | closed (one bounded post-send retry, charged at worst case) | `meta.rs::send`; `only_429_and_503_are_retried`, `a_connection_dropped_after_the_request_is_retried_once_by_default` |
| MA-9 | Debug dir file names from the response id | closed | `meta.rs::debug_file_name`, refused in public mode; `the_debug_dir_keeps_sanitized_files_without_the_key_and_never_in_public_mode` |
| MA-10 | `strict_schema` walk key-agnostic; no golden files | partly: per-call keyed schema asserted strict in tests; `strict_schema` still treats any object with `properties` as a schema; no golden files | `meta.rs::strict_schema`; `structured_output_is_strict_and_every_list_is_required`, `the_evaluate_request_is_keyed_by_this_calls_claim_and_item_ids` |
| HY-1 | `Valid` not bound to the record | closed for kind, scope and anchor; open for the byte-binding of record text (documented) | `hyphae.rs::check_artifacts`; `valid_requires_a_reexecuted_memory_proof_bound_to_the_anchor_given` |
| HY-2 | Verifier without a timeout | closed for the deadline and output caps; open for the temp-directory sweep and process-tree kill | `hyphae.rs::SystemCommandRunner`; `a_verifier_past_its_deadline_is_a_verification_error_with_its_code`, `the_system_runner_kills_a_command_past_its_deadline` |
| HY-3 | Live Hyphae behind the public portal | closed by design (sealed snapshot; live Hyphae operator-only); capture session pending | `snapshot.rs`, `v1.rs::operator_meta_start`; `an_orchestrated_run_over_the_snapshot_is_proven_verified_and_recommended`; ADR-0008 |
| HY-4 | Generic `corruption` becomes `Invalid` | accepted (fail-closed) with an honest detail | `orchestrator.rs::GENERIC_CORRUPTION_NOTE`, typed `ProofFailure`; `the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it` |
| HY-5 | 4 MiB ceiling; witnesses accumulate | accepted (operator hygiene; snapshot avoids the ceiling); no code | `docs/architecture/sealed-snapshot.md`, container README |
| HY-6 | Receipts cannot locate later-round proofs | closed | `RecallProven`, `EvidenceVerification.round`/`proof_ref` (schema 1.3); `recall_proven_is_accepted_where_evidence_events_are`, `a_verification_is_exactly_what_the_rounds_evaluation_implies` |
| HY-7 | Unstable fallback id; unknown shapes read as empty | closed | `hyphae.rs::map_record`, `record_list`; `a_record_without_an_id_is_named_the_same_way_every_time`, `an_answer_without_a_record_list_is_an_error_never_an_empty_recall` |
| HY-8 | Keyword fallback marks stderr `Invalid` | accepted (not adopted, LC-10) | `hyphae.rs::check_artifacts`; `the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it` |
| HY-9 | stdio transport weaknesses | partly: one deadline per request with a cancellation notice, bounded lines, stderr tail kept, child killed on drop; process-tree kill on Windows still absent | `hyphae.rs::StdioTransport`; `mcp_lines_are_bounded`, `the_stdio_transport_reports_a_dead_child_and_keeps_its_stderr_tail` |
| HY-10 | Pinned anchor neither validated nor normalized | closed | `config.rs::normalize_anchor`; `the_pinned_anchor_is_normalized_and_validated_at_load`, `the_pinned_anchor_is_normalized_and_handed_to_the_verifier` |
| HY-11 | `agent ui` path for capture | pending the capture session (unverified until then) | `capture-snapshot.sh`, `docs/architecture/sealed-snapshot.md` §9 |
| HY-12 | Empty-recall proofs discarded | closed | `Recall::proof`, `RecallProven{empty: true}`; `an_empty_proven_recall_still_returns_its_proof`, `foreign_records_are_dropped_and_proofs_recorded_even_when_empty` |
| HY-13 | Verification cache keyed by path | closed | `hyphae.rs::HyphaeProofVerifier::verify`; `results_are_reused_by_artifact_bytes_never_by_path` |
| RT-1 | Earlier-round objections forgotten | closed in code; counted live battery pending | `state_machine.rs`, `contract.rs`, `meta.rs` prompts; `rt1_round_two_silence_cannot_reach_supported`; `1ee386a` |
| RT-2 | Global mutex, no cancellation, no subprocess timeout | closed | `runs.rs`, `control.rs`, `hyphae.rs`; `cancel_at_every_stage_records_a_cancelled_failure`, `a_disconnected_client_never_stops_a_run` |
| RT-3 | Event store does not scale; one bad record takes all offline | closed | `run_store.rs`; `a_damaged_stored_run_is_reported_never_served_as_if_whole` |
| RT-4 | Receipts commit to one global chain | closed (per-run chain, receipt v2 chain scope, verify CLI); signed receipts post-hackathon | `receipts.rs`, `verify_cli.rs`; `interleaved_runs_need_the_whole_log_and_head_is_the_runs_own`, `a_good_log_and_its_receipt_verify`; `40b6d06` |
| RT-5 | Recommendation and receipt leave out what is open | closed | receipt v2 `deliberation`, `unresolved_uncertainties`, round-tagged verifications; `reference_receipt_shows_the_deliberation_and_the_dropped_objections`, `the_recommendation_carries_the_final_proposals_uncertainties` |
| RT-6 | No single writer across processes | closed | `EventLog::open_locked`, run locks; `locked_open_shuts_out_a_second_writer_until_the_first_drops`, `a_run_has_one_owner_at_a_time_across_handles_and_stores` |
| RT-7 | No-progress exit compares counts | closed | set-based progress; `progress_is_set_based_not_count_based`; `1ee386a` |
| RT-8 | Idempotent append ignores content | closed | `IdempotencyConflict`, `409`; `reused_event_id_with_different_content_is_a_conflict` |
| RT-9 | Failed rollback does not stop writes | closed | poisoned handle; `failed_rollback_poisons_the_handle` |
| RT-10 | Chained 1.0 records skip recomputation | closed | refused at open and on replay; `a_relabelled_record_is_refused_even_when_re_hashed`, `relabelling_a_record_cannot_escape_re_derivation` |
| RT-11 | Console read path diverges; replay errors dropped | closed | validated reads (`read_run`, writer snapshot), `FailureV1`; `a_damaged_stored_run_is_reported_never_served_as_if_whole` |
| RT-12 | SSE global ids, no `Last-Event-ID`, 200 for unknown runs | closed | `stream.rs`, `v1.rs::run_events`; `sse_resumes_from_every_sequence_without_duplicates_or_gaps` |
| RT-13 | Persistence error aborts without a terminal event | closed (terminal event attempted; the stream ends when the writer goes away) | `runs.rs::try_fail`, `stream.rs` (`Closed` → end); `a_panicking_model_fails_only_its_run`, `an_append_the_disk_refuses_is_neither_stored_nor_published` |
| RT-14 | Re-recalled evidence not logged again | closed | `RecallProven` per recall; `the_orchestrator_records_every_proof_and_every_dropped_record` |
| LC-01 | Schema 1.3 must round-trip archived records byte for byte | closed | `every_log_line_round_trips_byte_for_byte`, `every_new_1_3_field_round_trips_byte_for_byte`, `absent_1_3_fields_are_never_written` |
| LC-02 | not described in the repository | — | — |
| LC-03 | Scripted fakes get no exemption from the contract | closed | `a_scripted_evaluation_missing_one_verdict_fails_the_run` |
| LC-04 | Verbatim research files next to labelled display copies | closed | `/research/{*path}` verbatim with digest, showcase views as display copies; `research_files_are_served_verbatim_with_their_digest` |
| LC-05 | Provenance from the records, not the request | closed | `public_view::provenance`; `provenance_is_derived_from_records_and_cross_checked` |
| LC-06 | not described in the repository | — | — |
| LC-07 | Admission before durability; a terminal event on error | closed | `runs.rs` (`reserve`, `abandon_start`, `try_fail`), `shared_log.rs`; `the_live_gate_is_asked_after_the_slot_and_refunded_when_the_start_fails`, `refused_appends_are_neither_stored_nor_published` |
| LC-08 | not described in the repository | — | — |
| LC-09 | Evidence cap under carry-forward | closed | `evidence_cap_reached_in_round_one_keeps_round_two_at_the_carried_records` |
| LC-10 | Narrowing the corruption classifier would be a new fail-open | accepted (classifier widened only) | `hyphae.rs`, `orchestrator.rs::upstream_proof_check`; `the_typed_corruption_category_widens_the_keyword_rule_and_never_narrows_it` |
| LC-11 | Authority and identity never from the body; pre-1.3 reads `unspecified` | closed | `api/mod.rs::Identity`, `receipts.rs::UNSPECIFIED_AUTHORITY`; `requests_refuse_unknown_fields_and_free_strings`, `a_decision_shows_the_authority_it_was_recorded_under` |
| LC-12 | Per-run chains need a cross-run anchor | closed (hash-chained run index) | `run_store.rs`; `any_edit_to_the_run_index_is_detected_when_the_store_opens` |
| LC-13, LC-14 | not described in the repository | — | — |
| LC-15 | Ids validated before any path | closed | `run_store.rs::parse_run_id`, `showcase.rs`; `parse_run_id_accepts_exactly_the_uuid_id_gen_format`, `research_files_are_served_by_manifest_path_only` |
| UI-1 | Synchronous creation without idempotency | closed | `202` + `Location`, `Idempotency-Key`; `a_retried_creation_with_the_same_key_returns_the_same_run` |
| UI-2 | One mutex; every read re-parses | closed | as SEC-4, SEC-6 |
| UI-3 | Approve/Reject go to a stale global run | closed on the server (`expected` block, `409`); the legacy console still keeps a `selectedRun` global but now sends the run's current `expected` state | `runs.rs::decide`, `console.html::decide` |
| UI-4 | Provenance not first-class | closed in the API; pending in the web UI (G7) | `public_view.rs`; `provenance_is_derived_from_records_and_cross_checked` |
| UI-5 | `fake` defaults; unknown fields ignored | closed | enums, `deny_unknown_fields`, no defaults; `modes_are_explicit_and_credentials_are_required` |
| UI-6 | Approval view leaves out what is approved | closed in the API (`current_proposal`, `challenge_items`, `rounds`, decision note); pending in the web UI (Decision center) | `public_view::detail` |
| UI-7 | Conflicting second decision returns 200 | closed | `409 idempotency_key_reused`; `a_guided_run_is_accepted_streamed_decided_and_receipted` |
| UI-8 | Accessibility gaps | partly: the new web shell runs axe in Playwright (`apps/web/e2e/a11y.spec.ts`, not run here); the legacy console is unchanged and operator-only | `apps/web`, `console.html` |
| UI-9 | `irrelevant` coloured positive | open in the legacy console: `badge()` still gives the positive class to any value matching `relevant` (among `valid`, `supported`, `approved`, `corroborated`) and excludes only `invalid` and `uncorroborated`, so `irrelevant` reads positive (the text label is always shown); typed tables for the new UI pending | `console.html::badge` |
| UI-10 | Inconsistent errors; replay failures hidden | closed | `api/error.rs`, `FailureV1`; `codes_serialize_snake_case_and_carry_their_status` |
| UI-11 | SSE traps; console never used SSE | closed (the console streams; the contract is typed) | `console.html::streamRun`, `contracts/api-v1/ConsensusFrameV1.schema.json`; `committed_contract_matches_the_code` |
| UI-12 | No security headers; internal paths in responses | closed | as SEC-9, SEC-13; `research_files_are_served_verbatim_with_their_digest` |
| TC-1 | Run lifecycle has zero tests | closed | `api_public.rs` (22 tests), `run_store.rs` tests; `concurrent_guided_runs_stay_isolated_and_ordered`, `two_concurrent_decisions_record_exactly_one` |
| TC-2 | "Every claim assessed" only in the adapter | closed | `contract.rs::validate_evaluation`; `every_claim_is_assessed_once_citing_only_retrieved_records` |
| TC-3 | Nothing enforces a single writer | closed | as RT-6 |
| TC-4 | Adapter relabels records with the requested project | closed | `hyphae.rs::map_record`; `records_keep_their_backend_attribution_and_foreign_ones_are_dropped` |
| TC-5 | HTTP and SSE handlers untested | closed | `api_public.rs`, `api_operator.rs`, `live_gate.rs`, `api_canary.rs`, `api_showcase.rs`, `api_verify.rs`, `api_contract.rs` |
| TC-6 | Descriptive claims in `docs/evaluation.md` off | closed (Corrections section) | `docs/evaluation.md`; `e4ef201` |
| TC-7 | Failed-after-response calls never recorded or charged | closed | `ModelError::after_call`, `take_failed_invocation`; `every_refused_answer_keeps_its_invocation`, `an_answered_call_whose_body_timed_out_is_recorded_and_charged` |
| TC-8 | Idempotency gaps | closed | as UI-1, UI-7 |
| TC-9 | Case-sensitive neutralization; task outside any block | closed | as MA-6, MA-7 |
| TC-10 | A test asserts nothing when a key is set | open: `modes_are_explicit_and_credentials_are_required` still skips its no-credentials branch when `MetaConfig::from_env()` finds a key | `crates/consensus-api/tests/api_operator.rs` |
| TC-11 | Conformance check prints a constant metric | open: `evaluate.rs` still prints "Unauthorized executions: 0" unconditionally; the guided scenarios are also a `#[test]` now | `crates/consensus-tests/examples/evaluate.rs`; `every_guided_scenario_reaches_its_documented_outcome` |
| TC-12 | Batteries not tied to code or configuration; replay can modify | partly: read-only `verify`, `prompt_sha256`/`schema_sha256` per invocation, `Task.mode`; no run-start record of commit and configuration | `verify_cli.rs`, `meta.rs::Role::prompt_sha256`; `prompt_hashes_identify_the_template_never_the_input`, `read_only_open_never_changes_an_archived_log` |
| DOC-1 | No license | closed | `35297e7`, `e4ef201` (`LICENSE`, `NOTICE`, SPDX headers, README) |
| DOC-2 | Docs said to delete `demo-run/` | closed for the docs and the archive (`ff0d2fb`, `e4ef201`); open for the code lane: `main.rs` still defaults the CLI demo to `demo-run/events.jsonl` and says to delete that directory | `crates/consensus-api/src/main.rs` |
| DOC-3 | Overclaim about corroborated claims | closed | `e4ef201` (README, pitch, evaluation; errata in `docs/reviews/README.md`) |
| DOC-4 | Field-drift fail-open misattributed | closed | `e4ef201` |
| DOC-5 | `.gitignore` gaps; no secret scanning | closed for the ignore rules and the `.env.example` entries; open for gitleaks and push protection | `.gitignore`, `.env.example`; `35297e7`, `4cb7529` |
| DOC-6 | Raw error bodies in the log | closed | as MA-4; ADR-0002 note |
| DOC-7 | Uncertainties not carried to the human | closed | `Recommendation.unresolved_uncertainties`; `the_recommendation_carries_the_final_proposals_uncertainties`; ADR-0002 note |
| DOC-8 | Seeded fact 5 misdescribes the test | closed (correction notes; fixture comments) | `4cb7529`, `e4ef201` |
| DOC-9 | `head_hash` documented as the chain head | closed (docs and code comments) | `receipt.rs`, `event_log.rs::head_hash`; `e4ef201` |
| DOC-10 | Stale statements across ADRs, docs, env template | closed for the docs, `.env.example`, scripts (`e4ef201`, `4cb7529`); open for `ports.rs` lines 4–5, which still promise an async M2 adapter | `crates/consensus-runtime/src/ports.rs` |
| DOC-11 | Historical briefs carry superseded instructions | closed | `docs/reviews/README.md` (banners, errata) |
| DOC-12 | Personal email in history; "mario" in demo defaults | accepted (history kept; neutral identities have no work item) | `main.rs`, `scenarios.rs` |
| DOC-13 | Promised submission artifacts missing | open (video, submission checklist, runbook) | — |
| DOC-14 | Crates lacked `publish = false` | closed | `35297e7` |

The audit also names FEAS-1 to FEAS-10, JUDGE-1, JUDGE-2 and DELIV-1 (delivery and
judging-fit issues of the plan). They are not security findings, are not described in the
repository, and are outside this document.

## 5. Known gaps and open decisions at `d1e58f1`

Nothing in this list is claimed fixed. Each item names where it stands in the code.

1. **The public live path is not wired end to end.** `serve_public()` calls
   `serve_public_with(None)` (`server.rs`), and the only `impl DeliberationFactory` in the
   tree is the test double `ScriptedEvidenceFactory` (`tests/common/live.rs`). With
   `CONSENSUS_LIVE_ENABLED=true`, `build_live_gate` fails at startup ("requires the sealed
   snapshot client (`CONSENSUS_SNAPSHOT_DIR`); not wired in this build"). The
   `SnapshotMemoryClient` (`snapshot.rs`) and the `LiveGate` (`live/gate.rs`) exist and are
   tested separately; the factory that joins them is missing. The portal can be exposed in
   guided + showcase mode without it (ADR-0006 consequence).
2. **The sealed-snapshot capture session has not happened.** No `snapshots/` directory
   exists; the anchor and binary digest are unpublished; nine points stay unverified until
   the session (`docs/architecture/sealed-snapshot.md` §9), among them whether `hyphae
   agent ui` needs the daemon and whether `proof_blake3` is the BLAKE3 of the proof bytes.
3. **Operator live runs are unmetered** (`v1.rs::operator_meta_start` builds no meter);
   their spend is outside the caps (ADR-0007 follow-up).
4. **The per-client (/64) live concurrency limit is not enforced** beyond the global
   `max_concurrent` slots and the run manager's live semaphore (`live/mod.rs`).
5. **Public `events.ndjson` is withheld for live-declared runs** (`public_view::ndjson_publishable`)
   because the chain can carry adapter and verifier free text (up to 300 characters of
   verifier stderr in a verification detail, failure reasons). Lifting it needs the
   adapters to persist codes only.
6. **The counted protocol-1.3 live battery is pending** (4 of the 8 authorized runs left;
   K01–K04 were smoke runs). No claim about live behaviour under protocol 1.3 beyond
   `docs/evaluation.md` §6 is made. The K03/K04 scope drift (T4) must be shown in the
   Decision center and the receipt next to the outcome.
7. **The web run views are placeholders.** `apps/web/src/app/[locale]/runs/page.tsx` and
   `new/page.tsx` render `NotConnected`; no API client, SSE client, provenance rendering of
   real runs, Decision center or Evidence inspector exists; `ProvenanceLabels.tsx` is a
   legend. G6 (Playwright through the hostname) and G7 (honesty assertions) are pending.
8. **The fresh-clone Linux run is owed** (checklist G8): G8 passed on 2026-10-28 at
   `bfb0125` (Ubuntu 24.04, 510 tests) and this document's Windows run at `d1e58f1` passed
   534 tests, but no Linux run of `d1e58f1` from a fresh clone has been recorded.
9. **No deployment runbook, no systemd unit, no tunnel ingress file** exists in the
   repository; the unix-socket listener, the socket mode, the `LoadCredential` key file,
   the WAF rules and the edge feature switches are described in prose (ADR-0006,
   ADR-0007, the checklist) and cannot be verified from the tree.
10. **Verifier hygiene:** no temp-directory sweep for a killed `hyphae proof verify`
    (G9's "temp-directory sweep tested" is not met); only the direct child is killed;
    children get the Meta key variables removed, not a cleared environment.
11. **HY-1 residual:** the record text is not byte-compared with the proof; the snapshot
    binds responses to proofs only through the capture-time digests.
12. **Unsigned chains and index** (T38): tamper-evident against a published head, not
    tamper-proof. Signed receipts are post-hackathon.
13. **Test and tooling hygiene left open:** TC-10 (the operator credential test skips its
    no-key branch when a key is set), TC-11 (`evaluate.rs` prints a constant
    "Unauthorized executions: 0"), MA-10 (`strict_schema` walks every object with
    `properties`; no golden schema files), no test for `408 request_timeout`, no test of the
    unix-socket bind, the signal handler or the purge timer; `cargo deny check` and `pnpm
    audit` not re-run at `d1e58f1`; the web unit and e2e suites not run for this document;
    no gitleaks or push protection.
14. **Code-lane leftovers from the audit:** `main.rs` still defaults the CLI demo to
    `demo-run/events.jsonl` and says to delete that directory (DOC-2); `ports.rs` still
    promises an async M2 adapter (DOC-10 b); the demo identity "mario" stays (DOC-12,
    accepted).
15. **Open owner decisions** (`docs/journal.md`, 2026-10-27 night): the judge lane's daily
    cap (default: the whole $15 slice, so a day's exposure is $25); the ring's eviction of
    in-window visitor runs under load; the public contact email; the legal texts in four
    languages; the Day-2 items (hostname, named Tunnel, Turnstile widget and secret, the
    dedicated server key); whether an optional single-model baseline battery is run.
16. **Residuals accepted for the hackathon:** a generated operator token is printed once
    to stdout (use `CONSENSUS_OPERATOR_TOKEN_FILE`); a key passed by variable is visible in
    the process environment (use the key file); a mistyped judge code silently spends the
    public lane; a Turnstile solve spent on a failed start is not refunded; geo-blocking
    is defeated by a VPN; `style-src-attr 'unsafe-inline'` is allowed.

## 6. Out of scope

Signed logs and receipts; a cross-process or hardware root of trust for the Hyphae anchor;
the restricted executor (post-M3, separately reviewed before any real mutation is
permitted: in every mode today a decision is recorded and nothing executes); the legacy
operator console's accessibility beyond what it has (it is an operator-only tool on
loopback); hosted BYOK (ruled out by Meta's terms; "bring your own deployment" instead);
a Meta moderation or guard model for visitor tasks (the public surface accepts no visitor
text, so none is needed today); the security of the owner's development machine beyond
keeping the Node toolchain in containers; and everything on the Cloudflare and server
side that no file in this repository configures, which the release checklist gates.
