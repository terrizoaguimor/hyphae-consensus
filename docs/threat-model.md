# Threat model — Hyphae Consensus (M1)

Date: 2026-10-26 · Companion to `docs/adr/0001-m1-architecture.md`

## Trust boundaries

| Participant | Trust level | Authority |
|---|---|---|
| Runtime (this code) | Trusted | State transitions, persistence, tool authorization, final status |
| Models (Proposer/Critic/Evaluator) | Untrusted | Propose payloads only; cannot set outcomes or authorize |
| Memory backend (Hyphae) | Integrity-verifiable, content untrusted | Stores/recalls records; proofs attest integrity, not truth |
| Retrieved memories / repo contents | Untrusted data | Never executed, never instructions |
| Human operator | Trusted authorizer | Sole approver/rejecter of runs |

## Threats and mitigations

1. **Model self-approval / outcome forgery.** Model payloads have no outcome fields;
   the state machine accepts `HumanDecisionRecorded` only from `Actor::Human`.
   Tested: `agent_cannot_self_approve`.

2. **Prompt injection through retrieved memory.** Evidence content is data handed to
   roles inside a bounded context; authorization cannot be produced by any model call
   path. Tested: `injected_instruction_in_memory_is_data_not_authority` (M2 adds
   prompt-delimiting and live-model injection cases).

3. **Integrity/truth confusion.** Orthogonal status axes; `supported` requires both
   corroborated claims and valid proofs on all cited evidence. Tested:
   `valid_proof_does_not_imply_corroboration`,
   `no_checked_evidence_is_never_supported`.

4. **Corrupted or mismatched evidence.** Invalid proof → `blocked`, no revision
   attempt (prose cannot repair corruption). Tested:
   `invalid_proof_blocks_without_revision_attempt`. M3 demonstrates only failure
   modes the real Hyphae proof contract detects; no faked cryptography.

5. **Endless debate / resource exhaustion.** Max 2 revision rounds + no-progress
   exit; model/memory calls are bounded per run. Tested:
   `no_progress_exits_instead_of_looping`.

6. **Crash inconsistency.** fsync per append; truncated-tail recovery; mid-log
   corruption and sequence gaps are hard errors rather than silent guesses. Tested:
   event-log unit tests.

7. **Duplicate decisions on retry.** Idempotency keys on events and on human
   authorizations. Tested: `approval_retry_with_same_idempotency_key_is_noop`.

8. **Unauthorized external mutation.** Executor deferred through M3; approval only
   appends a decision event. Tested by event-census assertions in
   `challenge_then_revision_reaches_supported_and_approval_only_records`.

9. **Cross-project memory leakage.** Hyphae's documented rule (named project +
   `_global` only) is mirrored by the fake now and must be re-verified against the
   real backend in M3. Tested: `fake_memory_honors_project_isolation_like_hyphae`.

10. **Secret leakage.** Secrets come from environment variables only; events and
    receipts store claims/justifications and model metadata, never credentials or
    raw reasoning traces. Hyphae keys are passed by file path
    (`HYPHAE_NATIVE_API_KEY_FILE`), never as values (M3).

11. **Silent degradation.** Model timeout/malformed output fails the run with a
    recorded reason; memory outages cap the recommendation. Tested: timeout,
    malformed-output, and offline scenarios.

## Out of scope for M1

Live-model prompt construction (M2), real Hyphae transport/auth (M3), console session
security (console milestone), and the restricted executor (post-M3, separately
reviewed before any real mutation is permitted).
