# ADR-0002: Meta Model API adapter

Date: 2026-10-26 · Status: accepted · Scope: milestone M2

## Decisions

1. **Endpoint**: Chat Completions, `POST https://api.meta.ai/v1/chat/completions`,
   Bearer auth. Structured output uses the documented Chat Completions field
   `response_format: {type: "json_schema", json_schema: {name, schema, strict}}`.
   The Responses API equivalent (`text.format`) is not interchangeable and is not
   used. Source: https://dev.meta.ai/docs/structured-output (read 2026-10-26).
   **Amended 2026-10-27:** the request sends `strict: true` with the schema
   rewritten to the strict form (every property required, no additional
   properties). Until then it sent `strict: false` with optional list fields,
   and raw drafts captured live showed the Evaluator renaming or dropping
   them in 4 of 5 calls (`refs`, `relevant_evidence_ids`,
   `contradicting_evidence_ids`, no `unresolved_items` at all, plus invented
   keys such as `recommendation`). Serde defaulted those to empty lists, so a
   dropped field read as "nothing unresolved" or "no contradictions":
   fail-open. Replaying the same recorded inputs with the strict request, 5 of
   5 drafts used exactly the schema keys. The API is still not trusted to
   enforce anything: every list field is required in our typed draft, a
   missing or renamed field is `MalformedOutput`, and unknown extra keys are
   ignored (they cannot reach a decision). `CONSENSUS_META_DEBUG_DIR`
   (opt-in) keeps each raw draft for audit, and the `replay_evaluate` example
   re-asks a recorded Evaluator question.

2. **Drafts, not domain documents.** The model fills small draft schemas
   (`DraftProposal`, `DraftChallenge`, `DraftEvaluation`, `DraftRevision`)
   generated with schemars from Rust types in `consensus-api/src/meta.rs`.
   Correlation ids, timestamps, and schema versions are assigned by the adapter.
   Drafts contain no outcome/authorization fields by construction.

3. **Server-side cross-checks.** Claim/evidence ids returned by the model are
   accepted only if they exist in the run context; unknown ids are dropped.
   Claims the evaluator omits are completed as *uncorroborated* (conservative:
   can only lower support, never raise it). Unparseable payloads are
   `MalformedOutput` and fail the run with a recorded reason.

4. **Failure semantics.** Per-call timeout (default 120 s since 2026-10-27,
   previously 60 s; `CONSENSUS_MODEL_TIMEOUT_SECS` overrides it);
   429/5xx retried with bounded backoff (default 2 retries) then `Unavailable`;
   transport timeouts map to `ModelError::Timeout`. Latency and token usage are
   recorded in `ModelInvocation`; `model` preserves the exact identity echoed by
   the API (fallback: configured `CONSENSUS_MODEL`, default `muse-wonder-1.0`).

5. **Credentials.** Read once from `META_API_KEY` (fallback `MODEL_API_KEY`; both
   names appear in Meta ecosystem documentation). Held in memory only; `Debug`
   redacts it; it never reaches argv, logs, events, or receipts.

6. **Prompt-injection posture (defense in depth, not a boundary).** Retrieved
   evidence is embedded inside `<UNTRUSTED_EVIDENCE>` delimiters and the
   previous role's output inside `<UNTRUSTED_PROPOSAL>`, with instructions to
   treat both as data. Delimiter look-alikes inside untrusted text are
   neutralized (`</UNTRUSTED…` → `< /UNTRUSTED…`) so a record cannot close
   its own block; each evidence record is truncated to 2,000 characters;
   Critic item descriptions and Evaluator notes are escaped the same way.
   Role prompts forbid asserting outcomes or approvals. None of this makes a
   model injection-proof: the hard boundary is that no model output can
   authorize, and the residual risk is a manipulated Evaluator (see the
   threat model and the live injection case in `docs/evaluation.md`).

7. **Sync boundary.** The adapter uses `reqwest::blocking`; the orchestrator
   stays synchronous and the HTTP console runs it on a blocking thread.

8. **Per-item challenge tracking (schema 1.1).** The adapter assigns an id to
   every Critic item. The Evaluator reports `unresolved_items` and the
   revision reports `addressed_items` by item id; ids not in the current
   challenge are dropped. The revision prompt carries the Critic's items,
   the Evaluator's per-claim notes (with relevant/contradicting ids), and
   the delimited evidence, so the Proposer sees what it is asked to fix.

9. **Bounded drafts.** A proposal or revision with no claims, or more than
   12, is `MalformedOutput`: claims drive one memory recall each per round.

10. **Degenerate output is malformed (2026-10-27).** A claim with an empty
    statement, or a Critic item whose description is empty, shorter than
    three words, or made only of category words (live run L02 returned bare
    labels such as `constraint_conflict` for all 9 items), makes the whole
    document `MalformedOutput`; nothing is silently trimmed. The Critic prompt
    states the rule.

11. **Prompt guidance (2026-10-27, owner decision).** The Proposer is asked
    for claims that the project's stored records could confirm or contradict
    (a concrete value, rule, or fact), with expectations and guesses moved to
    assumptions or uncertainties, which the recommendation carries to the
    human as unresolved. The Evaluator corroborates a claim only when a
    retrieved record with integrity `Valid` directly states it, cites that
    record, treats non-`Valid` records as unable to corroborate anything,
    reports an unsettled claim per claim instead of abstaining, and abstains
    only when the evidence cannot be assessed at all. A Critic objection stays
    in `unresolved_items` while it still applies to the proposal as written,
    including when the evidence confirms it (a first wording, "resolved when
    the evidence settles it", let live run R01 report a confirmed objection as
    resolved; it was replaced before the battery). A revision after an
    abstention shows the reviser the Evaluator's reason. None of this changes
    the policy: `supported` still needs every claim corroborated by its own
    `Valid` evidence, no contradiction, and no unresolved item.

## Verification

Tests run against a local fake HTTP server speaking the documented
envelope: happy path + exact request-field assertion, malformed content,
persistent-500 retry counting, id filtering, delimiter-breakout escaping +
truncation (asserted on the captured prompt), revise-prompt contents +
item-id filtering, the claim-count bound, label-only and empty-item
rejection, and the presence of the guidance above in each role's prompt. The live test
`live_meta_propose` (`#[ignore]`d; requires `CONSENSUS_LIVE_TESTS=1` plus a
key) **has been run and passes** against the real API: first on
2026-10-26 (evening), and again in the audit-fix round
(`muse-wonder-1.0`, request `chatcmpl-01a17b9a-a9a0-75f7-8828-0158895f7e61`,
13 s), and on 2026-10-27 with the new prompts
(`chatcmpl-01a17c1b-9061-7231-a59c-07ebb1cecf91`, 20 s). Full live runs are
recorded in `docs/evaluation.md`.

## Note — 2026-10-27 independent audit

The independent audit of `72e422a`
(`docs/reviews/2026-10-27-independent-audit.md`) qualifies several decisions
above. The decisions are kept as written; this note records what the audit
found.

- **Decisions 3 and 8: dropping unknown ids is the MA-1 fail-open, not a
  conservative check.** Unresolved-item, relevant and contradicting ids that
  are not byte-exact known ids are dropped silently, and only the first
  assessment per claim is kept. An emptied `unresolved_items` list, or a dropped
  contradiction, can then let the policy return `supported` although the
  Evaluator said an objection still applies. Nothing records what was dropped,
  and the adapter test asserts the drop as intended. It is latent: in 30 captured
  live drafts no id failed exact match. Planned fix (portal plan, work item A1):
  an unknown or duplicate id is `MalformedOutput`, and every evaluation is
  validated in the runtime for every `ModelClient`.
- **Decision 11: uncertainties are not yet carried to the human.** The
  recommendation's `unresolved_assumptions` copies the proposal's assumptions
  only; its uncertainties appear neither in the recommendation nor in the
  receipt (audit finding DOC-7). Planned: receipts v2 and schema 1.3 (A3, A6).
- **Decision 11: "live run R01" means the pre-fix run P01**, stored as
  `demo-run/live-battery-2026-10-27-prefix/R01-reference.json`. The R01 of
  `docs/evaluation.md` §5.2 is a later run (DOC-10).
- **Decision 5: "read once".** `MetaConfig::from_env` runs on every run creation
  and on every `/api/health` request (DOC-10). The key itself never reaches
  argv, events or receipts, but up to 300 characters of an upstream error body
  are stored in `RunFailed.reason`, and child processes inherit the whole
  environment (MA-4, DOC-6). Planned: C5, C6.
- **Decision 6: neutralization matches exact-case delimiters only.** Mixed-case,
  whitespace and fullwidth closers pass, the record `kind` and ids are inserted
  without escaping, and Critic descriptions, Evaluator notes and the abstention
  reason sit outside the delimited blocks (MA-6, MA-7). Planned: C6.
- **Decision 4: retries.** 429 and every 5xx, 504 included, are retried after
  250 and 500 ms without honoring `Retry-After`, against Meta's error-handling
  guidance (MA-8, not adversarially verified). Planned: C6.
