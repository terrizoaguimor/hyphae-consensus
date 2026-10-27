# ADR-0002: Meta Model API adapter

Date: 2026-10-26 · Status: accepted · Scope: milestone M2

## Decisions

1. **Endpoint**: Chat Completions, `POST https://api.meta.ai/v1/chat/completions`,
   Bearer auth. Structured output uses the documented Chat Completions field
   `response_format: {type: "json_schema", json_schema: {name, schema, strict}}`.
   The Responses API equivalent (`text.format`) is not interchangeable and is not
   used. Source: https://dev.meta.ai/docs/structured-output (read 2026-10-26).

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

6. **Prompt-injection posture.** Retrieved evidence is embedded inside explicit
   `UNTRUSTED_EVIDENCE` delimiters with instructions to treat it as data. Role
   prompts forbid asserting outcomes or approvals.

7. **Sync boundary.** The adapter uses `reqwest::blocking`; the orchestrator
   stays synchronous and the HTTP console runs it on a blocking thread.

## Verification

Unit/integration tests run against a local fake HTTP server speaking the
documented envelope (happy path + exact request-field assertion, malformed
content, persistent-500 retry counting, id filtering). The live test
`live_meta_propose` is `#[ignore]`d and requires `CONSENSUS_LIVE_TESTS=1` plus a
real key; it has not been run in this environment (no credential available).
