# ADR-0002: Meta Model API adapter

Date: 2026-10-26 · Status: accepted · Scope: milestone M2

## Decisions

1. **Endpoint**: Chat Completions, `POST https://api.meta.ai/v1/chat/completions`,
   Bearer auth. Structured output uses the documented Chat Completions field
   `response_format: {type: "json_schema", json_schema: {name, schema, strict}}`.
   The Responses API equivalent (`text.format`) is not interchangeable and is not
   used. Source: https://dev.meta.ai/docs/structured-output (read 2026-10-26).
   The request sends `strict: false`, so the API is **not** relied on to
   enforce the schema: conformance is enforced on our side by deserializing
   every response into the typed draft (serde), and any mismatch is
   `MalformedOutput`.

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

4. **Failure semantics.** Per-call timeout (default 60 s, env-configurable);
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

## Verification

Tests run against a local fake HTTP server speaking the documented
envelope: happy path + exact request-field assertion, malformed content,
persistent-500 retry counting, id filtering, delimiter-breakout escaping +
truncation (asserted on the captured prompt), revise-prompt contents +
item-id filtering, and the claim-count bound. The live test
`live_meta_propose` (`#[ignore]`d; requires `CONSENSUS_LIVE_TESTS=1` plus a
key) **has been run and passes** against the real API: first on
2026-10-26 (evening), and again in the audit-fix round
(`muse-wonder-1.0`, request `chatcmpl-01a17b9a-a9a0-75f7-8828-0158895f7e61`,
13 s). Full live runs are recorded in `docs/evaluation.md`.
