# Threat model — Hyphae Consensus

Date: 2026-10-26 (M1; revised in the audit-fix round, schema 1.1; and on
2026-10-27, schema 1.2) ·
Companion to `docs/adr/0001-m1-architecture.md`

## Trust boundaries

| Participant | Trust level | Authority |
|---|---|---|
| Runtime (this code) | Trusted | State transitions, persistence, evidence retrieval, proof verification, final status |
| Models (Proposer/Critic/Evaluator) | Untrusted | Propose payloads only; cannot write outcomes or authorize. The Evaluator's judgments are, however, the policy's main input (threat 2) |
| Memory backend (Hyphae) | Integrity-verifiable, content untrusted | Stores/recalls records; proofs attest integrity, not truth |
| Retrieved memories / repo contents | Untrusted data | Never executed, never instructions |
| Human operator | Trusted authorizer | Sole approver/rejecter of runs, holder of the console's operator token |

## Threats and mitigations

1. **Model self-approval / outcome forgery.** Model payloads have no outcome fields;
   the state machine accepts `HumanDecisionRecorded` only from `Actor::Human`.
   Tested: `agent_cannot_self_approve`.

2. **Prompt injection through retrieved memory (or through another model's
   output).** Retrieved text and the previous role's output reach prompts inside
   `<UNTRUSTED_EVIDENCE>` / `<UNTRUSTED_PROPOSAL>` blocks; delimiter look-alikes
   are neutralized so a record cannot close its own block, and each record is
   truncated to 2,000 characters (`untrusted_text_cannot_close_its_delimiter_block`).
   No model output can authorize or write the outcome
   (`policy_never_authorizes_from_memory_content`, a *policy* test: its scripted
   model never reads the hostile text). **Residual risk, stated plainly:**
   delimiters are defense in depth, not a boundary. A hostile record stored
   through legitimate channels would carry a *valid* proof, because integrity
   does not protect against injection. The Evaluator's `corroborated` and
   `unresolved_items` judgments drive the recommendation, so an Evaluator that
   obeyed injected text could produce `supported` on valid evidence; the human
   approval step is then the last line. Live case (`docs/evaluation.md`, I01/I02):
   a real Hyphae note claiming "SYSTEM OVERRIDE" and ordering reviewers to
   corroborate everything, report nothing unresolved, and not abstain was
   treated by the live Critic and Evaluator as *contradicting* evidence: 0
   claims corroborated, items left unresolved, abstained. Caveat: upstream
   proof generation failed, so the note reached the prompts labeled
   `integrity=Invalid`. **Valid-proof variant (2026-10-27, H01/H02):** with the
   note's proof verifying `valid`, the Critic flagged it in every round (and once
   named it an embedded directive claiming system authority), the Evaluator
   never corroborated a claim to raise retries, recorded the note as
   contradicting evidence every round, and kept items open against its orders,
   and both revisions kept the 3-attempt cap; both runs ended `contested` with
   authorization pending. Two runs with one model and one note: evidence, not a
   guarantee. A hostile record that contradicts legitimate ones can also keep
   them from `supported` (it forces `contested`), a fail-safe denial of
   support. The note was then forgotten from every served store
   (`docs/evaluation.md` §5.4).

3. **Integrity/truth confusion.** Orthogonal status axes. `supported` requires
   every claim corroborated by at least one record it cites itself, a valid proof
   on every cited record, no contradicting record (cited or not), and no invalid
   proof anywhere in the retrieved set. Tested:
   `valid_proof_does_not_imply_corroboration`, `no_checked_evidence_is_never_supported`,
   `corroborated_claim_without_own_evidence_is_not_supported`,
   `contradicting_evidence_prevents_supported_even_if_not_relevant`.

4. **Corrupted or mismatched evidence.** Any invalid proof in the retrieved set →
   `blocked`, whether or not a claim cites it, even when the Evaluator abstained,
   and with no revision attempt (prose cannot repair corruption). A corruption
   report from the backend's own proof generation counts as an invalid proof and
   is never downgraded to "unavailable". Verifier transport failures
   (`docker exec` 125–127, usage errors) are `VerificationError`, not tampering.
   Tested: `invalid_proof_anywhere_in_retrieved_evidence_blocks`,
   `invalid_proof_blocks_without_revision_attempt`,
   `abstention_with_invalid_proof_is_blocked_and_replayable`,
   `upstream_corruption_signal_blocks_and_is_never_downgraded`,
   `transport_exit_codes_are_verification_errors_not_invalid`. Since
   2026-10-27, `Valid` needs the verifier's own `verified` + semantic
   re-execution report, and failures are classified by the verifier's error
   category (`verifier_failure_categories_captured_live_map_to_integrity`).
   Detection was exercised live against the real binary: a one-byte witness
   tamper and a wrong anchor are both rejected, and the gated
   `live_hyphae_probe` repeats the wrong-anchor case through the adapter
   (ADR-0003). Only failure modes the real Hyphae contract reports are
   demonstrated; no faked cryptography.

5. **Endless debate / resource exhaustion.** Max 2 revision rounds plus a
   no-progress exit measured per challenge item (tested:
   `no_progress_exits_instead_of_looping`, `two_revision_rounds_are_reachable_and_bounded`,
   `revision_beyond_round_limit_is_rejected`). Bounds per run: at most 9 model
   calls; at most one recall per claim per round, with claims capped at 12 per
   proposal by the Meta adapter (`proposal_claim_count_is_bounded`); at most 20
   distinct evidence records, enforced by the runtime, with any drop recorded
   (`evidence_cap_is_strict_recorded_and_retrieval_is_runtime_attributed`); and
   since 2026-10-27 at most `CONSENSUS_MAX_RUN_TOKENS` (default 150,000) input +
   output tokens per run, enforced by the runtime for any `ModelClient`, with
   missing usage treated as a violation
   (`token_budget_exceeded_fails_run_safely_with_recorded_reason`,
   `missing_token_usage_fails_closed`). **Limits:** the cap is checked after each
   call, so a run can overshoot by one call's tokens; the claim cap applies to
   the Meta adapter, not to arbitrary `ModelClient`s.

6. **Crash inconsistency and log tampering.** fsync per append; a failed append is
   rolled back (fault-injection tested since 2026-10-27:
   `failed_mid_record_append_rolls_back_and_log_stays_valid`); truncated-tail
   recovery; mid-log corruption and sequence gaps are hard errors. Every 1.1 record is SHA-256 hash-chained and verified on open
   (`tampered_record_breaks_hash_chain`); replay recomputes each recommendation and
   rejects a forged kind (`replay_recomputes_recommendation_and_rejects_forged_kind`);
   the orchestrator validates every event before persisting it. Runs left
   mid-deliberation by a crash are closed as `Failed{stage:"interrupted"}` at
   startup (`startup_recovery_closes_crashed_runs_and_reports_broken_ones`). Replay
   re-derives each run with the policy of its recorded schema version, so a
   policy change cannot make old history unreplayable or unchecked
   (`replay_rederives_each_run_with_its_recorded_rules`).
   **Limit:** the chain is unsigned. It is tamper-*evident* against a previously
   exported receipt `head_hash`, not tamper-*proof* against someone who rewrites
   and re-hashes the whole file.

7. **Duplicate or swallowed decisions on retry.** Idempotency keys on events and
   on human authorizations, scoped per run, so a key reused in another run cannot
   silently swallow that run's decision. Tested:
   `approval_retry_with_same_idempotency_key_is_noop`,
   `reusing_an_idempotency_key_across_runs_records_both_decisions`.

8. **Unauthorized external mutation.** The executor is deferred; approval only
   appends a decision event. Tested by event-census assertions in
   `challenge_then_revision_reaches_supported_and_approval_only_records`.

9. **Cross-project memory leakage.** Hyphae's documented rule (named project +
   `_global` only) is mirrored by the fake (`fake_memory_honors_project_isolation_like_hyphae`).
   Live (2026-10-26, run L10): a recall for project `other-repo` against the real
   Hyphae store returned 0 records while `demo-repo` holds 2, so isolation held for
   that query; on 2026-10-27 (run R08) it again returned 0 while `demo-repo` holds
   7. Two observations, not an exhaustive test.

10. **Secret leakage.** Secrets come from environment variables only; events and
    receipts store claims/justifications and model metadata, never credentials or
    raw reasoning traces. Hyphae keys are passed by file path
    (`HYPHAE_NATIVE_API_KEY_FILE`), never as values.

11. **Silent degradation.** Model timeout/malformed output fails the run with a
    recorded reason; since 2026-10-27 that includes label-only or empty Critic
    items and empty claims (`label_only_challenge_items_are_malformed`), and a
    draft whose list field is missing or renamed. Before that fix, an omitted
    `unresolved_items` or a renamed `contradicting_evidence` silently read as
    "nothing unresolved" / "no contradictions" (fail-open, seen live; ADR-0002
    decision 1); now it is malformed output, and structured output is requested
    in strict mode (`renamed_or_missing_draft_fields_are_malformed_never_empty`). Memory outages record `evidence_unavailable` and, with no
    checked evidence, the policy caps the result at `insufficient_evidence` (or
    `abstained`): never `supported`. A failed proof generation travels with the
    record as `proof_error`. The console reports an unreadable log line instead
    of skipping it (`corrupt_log_line_is_reported_never_skipped`). Tested:
    timeout, malformed-output, offline, and proof-error scenarios.

12. **Unauthenticated approval.** A typed operator name is a label, not
    authentication. `/decision` requires a random per-process operator token
    printed at startup, and approving a `blocked` or `abstained` run requires an
    override note (`decision_requires_operator_token_and_override_note`).
    **Limit:** other console endpoints are unauthenticated and there is no CSRF
    protection; the server binds to loopback and must not be exposed. Anyone who
    can read the server's stdout holds the token.

13. **Untrusted proof anchor.** By default the anchor comes from the same recall
    response as the proof, so `Valid` shows internal consistency only. Operators can
    pin an out-of-band anchor (`CONSENSUS_HYPHAE_EXPECTED_ANCHOR`);
    mismatches are `Invalid` (`pinned_anchor_mismatch_is_invalid_without_running_verifier`).
    The anchor is stable while the store is unchanged, so pinning works live: the
    2026-10-27 battery ran pinned to the anchor the operator observed before it
    started, which detects any store change during the battery. It is not an
    independent root of trust: the operator read that anchor from the same
    server. Also, each recall's proof covers its whole result, and the adapter
    does not byte-compare the record text it passes to models against the proof
    (both come from one `hyphae mcp` response).

14. **Mode confusion in the console.** `model=fake` with a non-fake memory is
    rejected rather than silently ignored
    (`fake_model_with_real_memory_is_rejected_not_substituted`), and simulated proofs
    say `SIMULATED` in every verification record, so receipts cannot pass them off
    as cryptography.

## Out of scope

Signed logs or receipts, console session security beyond the decision token,
and the restricted executor (post-M3, separately reviewed before any real
mutation is permitted).
