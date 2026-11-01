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
(`docs/threat-model.md` section 4 tracks the status of its findings) qualifies several decisions
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

## Note — 2026-10-27 evening: what the hardened adapter changed

Commits `6c98fb4`, `e22399a` (lane R-b) and `3d1aeec`, `0745f3b` (tag
`prompt-freeze-2`) replace the behaviour the audit note above describes. The
decisions are kept as history; the current behaviour is:

- **Decisions 3 and 8 (MA-1) are closed.** The runtime validates every
  evaluation against the protocol 1.3 contract for every `ModelClient`, before
  it is recorded and again on replay (ADR-0010). The adapter maps an unknown,
  repeated or missing decision-relevant id to `MalformedOutput` naming the field
  and the id; nothing is dropped or completed. Since `prompt-freeze-2` the
  Evaluator schema is built per call with one required key per claim id and per
  open item id, so an omitted assessment or verdict is impossible by
  construction (smoke run K01 had shown that a strict schema with arrays still
  allowed an empty `assessments` list; K03 and K04 confirm the keyed schema live,
  `docs/evaluation.md` §6).
- **Decision 11 (uncertainties)**: `Recommendation.unresolved_uncertainties`
  (schema 1.3) carries the final proposal's uncertainties to the human and into
  receipts v2; K03 and K04 record 4 and 3 of them.
- **Decision 5 (key handling)**: the key is read with the precedence
  `CONSENSUS_META_API_KEY`, `MODEL_API_KEY`, `META_API_KEY`, zeroized on drop,
  and never logged; raw model output is kept only when `CONSENSUS_META_DEBUG_DIR`
  is set, which `CONSENSUS_PUBLIC_MODE` refuses. Fixed public error codes and
  messages (C5) belong to the public API lane (ADR-0006).
- **Decision 6 (prompt injection)**: delimiter neutralization is case- and
  whitespace-insensitive and covers fullwidth look-alikes; the task, the evidence
  (as a JSON array), the objections and the evaluation under review sit in
  separate untrusted blocks; every invocation records `prompt_sha256` and
  `schema_sha256` of what was sent.
- **Decision 4 (retries)**: only 429 and 503 are retried, with full-jitter
  backoff that honours `Retry-After` within a fixed bound and the call's budget;
  timeouts, 504 and 400/401/402/403/404/413 are never retried; a request that
  fails after it was sent without an HTTP status is retried exactly once when
  `CONSENSUS_META_RETRY_TRANSPORT_ONCE` is on (the default), recorded as
  `attempts: 2` on the answering call (smoke run K02 motivated this). The
  request also bounds the output (`max_completion_tokens`, 16,000 by default)
  and records `finish_reason`, `refusal` and the reasoning tokens.

## Note — 2026-11-01: 5xx retries, the run deadline and shutdown

After the first public live runs (M01 `meta_timeout` and M02 `meta_unavailable`
HTTP 500, both at critique; `docs/evaluation.md` §6.3):

- **Decision 4 (retries), amended**: HTTP 500 and 502 are retried **once per
  call**, within the attempt budget, the call's time budget and the 1 s headroom
  rule, after `Retry-After` when it fits (else the call fails at once with that
  status) or after the full-jitter backoff. The failed attempt may already have
  been billed: it stays settled at its full estimate on the live ledger, and the
  retry can bill **at most one duplicate generation per call**, which no
  invocation records (the answering call shows `attempts: 2`). 504 is still never
  retried (the gateway gave up on a generation that may have completed upstream),
  nor are timeouts or 400/401/402/403/404/413. 429 and 503 keep their retries up
  to the attempt budget; 503 is still charged at the estimate.
- **Run deadline**: the run manager binds the client to the run's control
  (`ModelClient::bind_run_control`). A call's budget is
  `min(started + timeout, run deadline)`; with less than `MIN_CALL_WINDOW` (30 s)
  left the call is refused before any ledger draw or request, code
  `run_deadline_reached`, which a call cut short by the run deadline also gets.
- **Shutdown**: a call still in progress once the shutdown grace has passed
  (public server: one call budget plus 10 s, at least 60 s) is abandoned when the
  run's `RunControl::abort_in_flight` is set: the attempt is settled at its
  estimate and the run fails with code `shutdown`, so its reservation is released
  before the server returns.
- **Connections**: pooled connections idle for more than 5 s are never reused.
- **`CONSENSUS_MODEL_TIMEOUT_SECS`** is strict (1 to 3600 whole seconds); an
  invalid value stops the server instead of falling back to 120 s.
