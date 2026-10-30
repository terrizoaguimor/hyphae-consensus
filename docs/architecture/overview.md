# Architecture overview — Hyphae Consensus

Date: 2026-10-29 · Describes the code at `bc3b262` (branch `feat/public-portal`, merge of
web lane W2, committed 2026-10-28); the snapshot capture of 2026-10-29 (`b0375df`) and its
boot check (`0515f29`) are noted where they change the picture · Nothing is deployed: the public portal is not exposed
until every release gate G1–G12 (`docs/security/release-checklist.md`) reads PASS.

This document consolidates ADR-0001 to ADR-0010 with the code as it stands. Where an ADR
and the code differ, this document follows the code and says so (section 11). It is a
map: the decisions and their reasons stay in the ADRs, the threats and their tests in
`docs/threat-model.md` (v2), the snapshot procedure in `docs/architecture/sealed-snapshot.md`
and the deployment steps in `docs/deployment/runbook.md`.

## 1. What the system does

Three Meta-model roles (Proposer, Critic, Evidence Evaluator; production model
`muse-wonder-1.0` through the Meta Model API) deliberate over a software-engineering
task. The runtime, not a model, retrieves evidence from Hyphae Agent Memory and checks
its integrity proofs with `hyphae proof verify`. A deterministic policy turns the
Evaluator's per-claim judgments and the proof statuses into a recommendation
(`supported`, `contested`, `insufficient_evidence`, `abstained`, `blocked`) with a
`PolicyTrace`. Only a human records an authorization, and recording it executes nothing:
the executor is deferred and `/api/v1/health` reports `executor: "disabled"`.

Models never write the outcome field, but the Evaluator's judgments are the policy's main
input, so "models never decide" holds in form, not in substance (ADR-0001, 2026-10-26
amendment). A valid proof shows that a record is the record the backend sealed, not that
it is true, relevant or complete (`RunReceipt::DEFAULT_LIMITATIONS`).

Runs come in three honest modes, fixed server-side per route (ADR-0006, ADR-0008):

| Mode | Model | Evidence | Where |
|---|---|---|---|
| Guided | scripted doubles (`provider: "fake"`) | seeded records, proofs labelled SIMULATED | public `POST /api/v1/runs` (scenario id only) |
| Live | Meta Model API on the owner's key | the sealed Hyphae snapshot | public `POST /api/v1/live-runs` (preset id only), off by default; not deployed. The snapshot `snap-2026-10-29-demo-repo` was captured on 2026-10-29 (`b0375df`), but the code at `bc3b262` (and at `6e202eb`) refuses it at boot (`response_invalid`, section 2.4); with the fix `0515f29` (merged as `7e26866`) one live run went through the public router in-process over the real snapshot, against a local fake Meta server. No live run has used the real Meta API in this mode |
| Recorded showcase | as recorded | as recorded | `GET /api/v1/showcase*`, `/research/*`: the archived 2026-10-26/27 and 2026-10-30 runs, read-only |

The operator console (`serve-operator`) can also run `model=meta` with `memory=hyphae`
against a running Hyphae deployment; that mode is never public.

## 2. Components

### 2.1 Workspace

| Path | Responsibility | ADR |
|---|---|---|
| `crates/consensus-domain` | Typed contracts only, no I/O: `run.rs` (`Task`, `TaskMode`, `RunState`, `Recommendation`, `HumanAuthorization`, `Authority`), `proposal.rs` (`Proposal`, `Claim`, `Challenge`, `ChallengeItem`, `Evaluation`, `ItemVerdict`, `Revision`), `evidence.rs` (`EvidenceReference`, `EvidenceVerification`, the three status axes), `events.rs` (`ConsensusEvent`, `EventPayload`, `ModelInvocation`), `receipt.rs` (`RunReceipt`, `RunReceiptV2`, `PolicyTrace`). `SCHEMA_VERSION` is `1.3`; versions `1.0`–`1.3` stay readable | 0001, 0010 |
| `crates/consensus-runtime` | The only component that changes run state: `state_machine.rs` (`RunAggregate`, `MAX_REVISION_ROUNDS`), `orchestrator.rs` (`create_task`, `deliberate`, `record_human_decision`), `contract.rs` (the protocol 1.3 evaluation contract), `policy.rs` (`explain`, `derive_recommendation_with`, `PolicyVersion::for_schema`), `event_log.rs`, `run_store.rs`, `shared_log.rs`, `sink.rs`, `control.rs` (`RunControl`), `receipts.rs` (`build_receipt_v2`, `protocol_label`), `ports.rs`, `fakes.rs`, `scenarios.rs` | 0001, 0005, 0010 |
| `crates/consensus-api` | Adapters, both HTTP surfaces and the CLI: `meta.rs`, `hyphae.rs`, `snapshot.rs`, `api/` (`mod.rs` routers and middleware, `v1.rs` handlers, `types.rs`, `error.rs`, `contract.rs`), `runs.rs` (`RunManager`), `stream.rs`, `session.rs`, `limits.rs`, `public_view.rs`, `public_config.rs`, `config.rs`, `showcase.rs`, `static_files.rs`, `guided.rs`, `live/` (`gate.rs`, `budget.rs`, `turnstile.rs`, `presets.rs`, `geo.rs`, `judge.rs`, `money.rs`, `config.rs`, `snapshot_factory.rs`), `server.rs`, `verify_cli.rs`, `ledger_cli.rs`, `console.html` | 0002–0008 |
| `crates/consensus-tests` | Integration and adversarial scenarios, the scripted conformance check (`--example evaluate`), replay of the `research/` archive | 0001, 0010 |
| `apps/web` | Mission Control: Next.js static export served by `serve-public` | 0009 |
| `contracts/api-v1` | Generated JSON Schemas and fixtures of the public API (section 7) | 0006 |
| `research/` | Byte-identical archive of the recorded runs with `MANIFEST.json` and `SHA256SUMS` (12 event logs, 48 runs, 919 events, 41 run records; `research/README.md`) | 0006 |
| `deploy/`, `docs/deployment/runbook.md` | systemd units, cloudflared ingress, tmpfiles, environment templates and operator scripts; nothing deployed | 0006–0008 |
| `scripts/` | `linux-gates.sh` (Linux parity), `live_battery.py`, `snapshot_manifest.py`, `make_research_manifest.py`, `third_party_notices.py`, `hyphae-container/` (container runbook, `capture-snapshot.sh`) | 0003, 0008 |

The binary `consensus-api` has these entry points (`crates/consensus-api/src/main.rs`):
`serve-public`, `serve-operator` (alias `serve`), `verify <log.jsonl> [--run <id>]
[--receipt <receipt.json>]`, `ledger status | reset --by <operator> | release
<reservation-id> --by <operator>`, and, with no argument or with `--approve` /
`--reject` alone, the scripted CLI demo, which writes `scripted-demo/events.jsonl` under
the current directory and refuses to run inside `demo-run/` or `research/`. `-h`,
`--help` and `help` print the usage; any other argument prints it and exits 2.

### 2.2 Runtime ports

`ports.rs` defines the boundaries the orchestrator calls through:

- `ModelClient`: `propose`, `critique`, `evaluate`, `revise`, each returning a document
  and its `ModelInvocation`; `take_failed_invocation` keeps a call the provider answered
  but whose output was refused.
- `MemoryClient`: read-only `recall`, `recall_with_proof`, `status`. `MemoryAdmin`
  (writes) is a separate trait that agent-facing code cannot reach.
- `ProofVerifier`: `verify(&EvidenceReference) -> ProofCheck`.
- `ExecutorPort`: `execute_approved`, never invoked.

The orchestrator is synchronous; the servers run each deliberation on the blocking pool
(ADR-0002 decision 7, ADR-0005 decision 5).

### 2.3 The Meta adapter (`meta.rs`, ADR-0002)

`MetaModelClient` calls Chat Completions (`POST https://api.meta.ai/v1/chat/completions`)
with `response_format.json_schema` and `strict: true`. The model fills draft types
(`DraftProposal`, `DraftChallenge`, `DraftEvaluation`, `DraftRevision`) that carry no
outcome or authorization field; ids, timestamps and schema versions are assigned by the
adapter. Since tag `prompt-freeze-2` the Evaluator's schema is built per call
(`evaluation_schema(claim_ids, item_ids)`) with one required key per claim id and per
open item id, so an omitted assessment or verdict cannot parse; an unknown, repeated or
missing decision-relevant id is `MalformedOutput`. Untrusted inputs (task, evidence as a
JSON array, objections, the evaluation under review) sit in separate delimited blocks
with case-, whitespace- and fullwidth-insensitive neutralization; every invocation
records `prompt_sha256` and `schema_sha256`. Retries: 429 and 503, with full-jitter
backoff honouring `Retry-After`; a connection that failed before the request was sent;
and one retry of a request that broke after it was sent without an HTTP status
(`CONSENSUS_META_RETRY_TRANSPORT_ONCE`, on by default); nothing else. Output is
bounded by `max_completion_tokens` (16,000 by default). The key is read from
`CONSENSUS_META_API_KEY_FILE` first, then `CONSENSUS_META_API_KEY`, `MODEL_API_KEY`,
`META_API_KEY`; it is zeroized on drop and never logged. In the public live mode and in a
metered operator run every HTTP attempt is drawn on the ledger before it is sent
(`MetaModelClient::with_meter`).

### 2.4 The Hyphae adapter and the snapshot verifier (`hyphae.rs`, `snapshot.rs`; ADR-0003, ADR-0008)

- **Operator lab only:** `HyphaeMemoryClient` speaks MCP revision `2025-06-18` as
  newline-delimited JSON-RPC over stdio to `hyphae mcp --profile memory`, never with
  `--allow-write`, so only `hyphae_memory_recall` and `hyphae_memory_status` exist
  (`StdioTransport`; `InMemoryTransport` for tests). A proven recall returns one sealed
  proof for its whole result; a failed proof generation is recorded on the record
  (`proof_error`), never dropped.
- **Every non-simulated mode:** `HyphaeProofVerifier` runs `hyphae proof verify --proof
  --witness --anchor` through a `CommandRunner`, under a deadline, with stdout and stderr
  capped and the Meta key variables and five credential-path variables removed from the
  child's environment. The environment is not otherwise cleared and the child runs as
  the same user, so it can still read the credential files, the Meta key file included
  (`docs/threat-model.md` T12, MA-2, MA-4; release gate G4 not met as written for
  subprocesses). `Valid` requires the report to say `verified`, semantic
  re-execution performed, `kind = memory`, the memory scope, and an anchor equal to the
  pinned anchor.
- **Public live mode:** `SnapshotMemoryClient` serves one fixed project (`demo-repo`) from
  a sealed snapshot directory (`manifest.json`, `SHA256SUMS`, captures). Recall is
  query-independent: every recall returns the full proven `list` set (7 records by
  contract), and each evidence reference records the proof's own request next to the
  claim query, so the label reads "proof covers: complete record list of `demo-repo` at
  anchor <prefix>". A recall of the isolation project returns no records with the
  empty capture's proof (`RecallProven` with `empty: true`); a recall of any other
  project is refused and recorded as `EvidenceUnavailable` with its reason. Each run's verifier is
  wrapped in `BootBoundVerifier`: a `Valid` stands only when the report's proof and
  result digests are the ones the boot check accepted. Witnesses are never served or
  committed. The capture (`scripts/hyphae-container/capture-snapshot.sh`,
  `scripts/snapshot_manifest.py`) ran on 2026-10-29: `snapshots/snap-2026-10-29-demo-repo/`
  (`b0375df`; hyphae 4.0.0, 7 records, anchor `43a2dea9…8f02`, manifest SHA-256
  `b731346c…014a`; proofs and witnesses git-ignored). The loader at `bc3b262` reads
  `memories`, `proof` and `snapshot` only at the top level of a captured response, while
  the real `hyphae agent ui` 4.0.0 response holds them under `result`
  (`{"id":1,"ok":true,"result":{…},"schema":…}`), so it refuses the real snapshot with
  `response_invalid: captures/list-demo-repo.response.json: no memories array`.
  `snapshot_manifest.py` accepts both shapes, which is why the capture's `build` and
  `check --reverify` passed. `0515f29` (merged as `7e26866`) adds `snapshot::result_object` (top level or `result`; an error
  envelope, `ok` not true or a non-object `result` is refused); with it the real snapshot
  boots and both proofs are re-verified at boot.

### 2.5 The live gate and the ledger (`live/`, ADR-0007)

`LiveGate` (`live/gate.rs`) admits a public live run in two phases around the single live
slot, stopping at the first refusal and spending nothing before the ledger has reserved
the run's worst case:

1. `LiveGate::verify` (no slot, no charge): preset (`404 preset_unknown`), geography on
   the live surface (`451`), the 18+ and territory attestation (`422
   attestation_required`), the evidence factory's status (`503 live_unavailable`, before
   Turnstile), Turnstile through Siteverify, fail-closed, with a fresh UUID as its
   idempotency key and at most 8 calls in flight (`403 turnstile_required|turnstile_failed`,
   `503 turnstile_unavailable`), then the lane (a judge code's digest, or the public lane).
2. The slot (`RunManager::reserve`): `429 too_many_runs` or `503 shutting_down`.
3. `VerifiedLiveRequest::admit` (slot held): the lane's daily quota
   (`CONSENSUS_LIVE_CLIENT_DAILY`, default 3 per IPv4 /32 or IPv6 /64 per UTC day; per
   judge code `CONSENSUS_LIVE_JUDGE_CODE_DAILY`, default 5), `Ledger::reserve`, the
   metered model client, and the deliberation built by `SnapshotFactory` over a full
   re-hash of the served snapshot files.

`Ledger` (`live/budget.rs`) is an append-only, fsynced USD ledger: one JSONL file per UTC
day (`YYYY-MM-DD.jsonl`), `lifetime.json`, `ledger.lock`, and an optional `kill-switch`
file whose presence trips it. Records are `reservation`, `draw`, `settlement`,
`release`, `trip` and `reset`; amounts are six-place decimal strings in the records,
computed in integer micro-dollars. A run's worst case is
`(run token cap + max_completion_tokens + per-call input cap)` at the output rate; with
the defaults `(70,000 + 16,000 + 32,000) × $20/M = $2.36`. Caps by default: $10.00 per
UTC day, $60.00 lifetime, of which $15.00 is the judge slice (`live/config.rs`).
Settlement charges actual usage on success, zero on the listed non-retryable 4xx and on a
request never sent, and the attempt's full estimate, never refunded, on unknown
outcomes. A trip lasts until `consensus-api ledger reset --by <operator>`; neither
listener can clear one. `serve-operator` meters its `model=meta` runs on the operator
lane of the same ledger when `CONSENSUS_LIVE_LEDGER_DIR` is set, and says at startup
whether its runs are metered.

### 2.6 The run store and event logs (`run_store.rs`, `event_log.rs`, `shared_log.rs`; ADR-0005)

Layout under `CONSENSUS_RUN_STORE_DIR` (default `consensus-store`):

```text
runs/<run_id>.jsonl      the run's EventLog: sequence from 1, chain from genesis
runs/<run_id>.meta.json  sidecar: owner (sha256 of the session id, operator or showcase) and retention
runs/<run_id>.lock       held exclusively by the run's one writer
runs/index.jsonl         hash-chained run index: created, checkpoints, terminal, purged
runs/index.lock          serializes index updates across processes
```

Run ids are `run-` plus a lowercase UUID v4 and are validated before any path is built.
`SharedRunLog` validates every append against the run's aggregate, appends, fsyncs, and
only then publishes to the per-run broadcast channel (`stream.rs`), so nothing a
subscriber sees is absent from the log. Restart recovery closes deliberations cut short
as `RunFailed{code: "interrupted"}`. Archived multi-run logs (`demo-run/`, `research/`)
are opened only through `EventLog::open_read_only` or `verify_bytes`.

### 2.7 apps/web (ADR-0009, `apps/web/README.md`)

Next.js 16.4 App Router as a static export (`output: "export"`), React 19.3, Astryx 0.7.4
prebuilt CSS with the `hyphae-mission` theme, `next-intl` in `en`, `es`, `pt-BR` and
`fr`, light and dark. The export is served by `serve-public` from `CONSENSUS_WEB_ROOT`
(default `apps/web/out`), so every API call is same-origin and relative. Its types
(`src/lib/api/types.ts`) are generated from `contracts/api-v1` and validated at runtime
with zod mirrors; `src/lib/api/sse.ts` follows a run's stream; `src/lib/chain/verify.ts`
recomputes each chained record's SHA-256 in the browser with WebCrypto from the raw line
with its hash blanked and follows every link; `event_log.rs` hashes its re-serialization
instead, so the two agree on the canonical lines the runtime writes and can disagree
either way on any other line. It reads only the chain fields, neither replays the history
nor re-derives the recommendation, and leaves that to `consensus-api verify`.
Pages: Overview, New deliberation, My runs, the run workspace (`run/?id=`, tabs Live or
Playback, Explorer, Evidence, Proof inspector, Audit trail, Meta API, Decision; RECORDED
mode with `&source=showcase`), Research showcase, Docs, Settings, Terms, Privacy,
Limitations, About. LIVE is shown only through `isLive` (`src/lib/provenance.ts`): a
frame with `delivery: live`, model axis `live_meta` and no flag. The Node toolchain runs
only in Docker (`apps/web/scripts/in-docker.sh`, `playwright-docker.sh`). A post-build
step (`apps/web/scripts/csp-manifest.mjs`) hashes every inline script and style into
`csp-manifest.json`, which the server turns into a per-page Content Security Policy
(`static_files.rs`).

## 3. Listeners and routes

Both surfaces are built in `crates/consensus-api/src/api/mod.rs` and run the same
handlers (`api/v1.rs`) over their own `RunManager`.

| Route | Public (`serve-public`) | Operator (`serve-operator`) |
|---|---|---|
| `POST`, `GET`, `DELETE /api/v1/session` | yes | no |
| `POST /api/v1/runs` (guided, `{scenario}`) | yes | no |
| `POST /api/v1/live-runs` (`{preset_id, attest_adult, judge_code?}` + `cf-turnstile-response` header) | yes | no |
| `POST /api/v1/operator/runs` (`{model, memory, scenario?, title?, description?, project_id?}`) | no | yes |
| `GET /` (the console page, `console.html`) | no (static export) | yes |
| `GET /api/v1/scenarios`, `/capabilities`, `/health`, `/ready` | yes | yes |
| `GET /api/v1/runs`, `/runs/{id}`, `/runs/{id}/events` (SSE), `/runs/{id}/events.ndjson`, `/runs/{id}/receipt` | yes, owner only | yes |
| `POST /api/v1/runs/{id}/decision`, `/runs/{id}/cancel` | yes, owner only | yes |
| `GET /api/v1/showcase`, `/showcase/{run_id}`, `/showcase/{run_id}/receipt` | yes | yes |
| `GET /research/{*path}` | yes | yes |
| anything else | JSON 404 under `/api`, else the static export | under `/api`: `401` without the token, then 404; elsewhere 404 |

**`serve-public`** listens on a unix socket (`CONSENSUS_PUBLIC_SOCKET`, default
`/run/consensus/public.sock`, mode 0660) that only cloudflared reaches; there is no
public TCP port. For development it may listen on a loopback TCP address
(`CONSENSUS_PUBLIC_BIND`, required on Windows; a non-loopback address is refused). With
the socket, `CONSENSUS_ALLOWED_ORIGINS` and `CONSENSUS_ALLOWED_HOSTS` are mandatory,
`CF-Connecting-IP` is required on every request, and live mode needs both snapshot pins
(`require_snapshot_pins`). Layers, outermost first: request timeout (30 s, `408`), body
limit (16 KiB, `413`), security headers, `Host` allowlist, CSRF rules for
state-changing `/api` requests (allowlisted `Origin`, `Sec-Fetch-Site` absent or
`same-origin`, JSON body), then the portal-wide geographic block (`portal_geo`, `451`)
on every route including the static export. Sessions are a 256-bit random id in an
`HttpOnly; Secure; SameSite=Strict; Path=/` cookie named `__Host-hc_session`
(`hc_session` only with `CONSENSUS_COOKIE_SECURE=false`), minted only by `POST
/api/v1/session`. Every run read, stream, receipt, verbatim log, decision and cancel is
owner-checked; a non-owner gets `404`.

**`serve-operator`** listens on loopback only (`CONSENSUS_OPERATOR_BIND`, default
`127.0.0.1:8787`). Every `/api` request needs `Authorization: Bearer <token>` (from
`CONSENSUS_OPERATOR_TOKEN_FILE`, at least 32 characters, or generated and printed once;
compared in constant time); the fallbacks sit inside the bearer layer, so an unknown
`/api` path or a wrong method on an `/api` route answers `401` without the token. The console page at `/`
and the `/research/*` files are served without the token (loopback only; the console
holds no data), and other unknown paths answer 404 (`api/mod.rs::operator_bearer`). `Host` must be the bound address, `127.0.0.1:<port>` or
`localhost:<port>`. It accepts free task text and is where `memory=hyphae` exists; its
views keep raw failure reasons and verifier output; its decisions carry `operator`
authority.

Request bodies reject unknown fields (`deny_unknown_fields`), and `model` and `memory`
are enums without defaults. The public surface accepts no free text that reaches a model;
the only visitor free text it stores is the optional decision note (at most 2,000
characters, control characters, bidirectional controls and zero-width characters
refused). Run creations accept an `Idempotency-Key` (1 to 128 characters of
`[A-Za-z0-9_.:-]`); guided and operator creations claim it before the start, the live
route looks it up and records it after the start (ADR-0006, implementation note).

## 4. Data flow of a run, from creation to receipt

The diagram shows a public live run as the code implements it. No public live run has
happened yet: nothing is deployed, and the only run over the real sealed snapshot was the
in-process boot check of 2026-10-29 (`0515f29`), whose Meta API was a local fake server. A guided run follows the same path without the
gate, Siteverify, the ledger and the Meta API: the scripted doubles and the simulated
verifier answer in milliseconds, and the stream then delivers the stored records as
`replay` frames, which the web client paces and labels "simulated · paced for
readability".

```mermaid
sequenceDiagram
    autonumber
    participant B as Browser (apps/web)
    participant CF as Cloudflare edge + cloudflared
    participant P as serve-public (router, LiveGate)
    participant TS as Turnstile Siteverify
    participant L as USD ledger
    participant M as RunManager + RunStore + SharedRunLog
    participant O as Orchestrator (blocking pool)
    participant META as Meta Model API
    participant S as SnapshotMemoryClient + hyphae proof verify

    B->>CF: POST /api/v1/session
    CF->>P: unix socket, CF-Connecting-IP, CF-IPCountry
    P-->>B: 201, __Host-hc_session cookie
    B->>CF: POST /api/v1/live-runs {preset_id, attest_adult} + cf-turnstile-response
    CF->>P: forwarded
    Note over P: timeout, body limit, Host, CSRF, portal geo, session, client key,<br/>Idempotency-Key lookup, rate, visitor-run ring
    P->>P: LiveGate::verify: preset, live geo, attestation, evidence status
    P->>TS: Siteverify (fresh UUID idempotency key)
    TS-->>P: success, action, hostname
    P->>M: reserve the live slot
    P->>L: daily quota, then Ledger::reserve(worst case)
    P->>S: SnapshotFactory: re-hash the served files, client copy, BootBoundVerifier
    P->>M: create run files, append TaskCreated (validated, fsync, then published)
    M-->>B: 202 RunCreatedV1
    M->>O: spawn deliberate() under catch_unwind
    B->>CF: GET /api/v1/runs/{id}/events (SSE)
    CF->>P: forwarded
    P-->>B: subscribe, replay from Last-Event-ID, then follow
    loop up to 1 + 2 revision rounds
        O->>L: draw (per HTTP attempt)
        O->>META: propose or revise
        META-->>O: draft + usage
        O->>L: settle
        O->>M: ProposalSubmitted / RevisionSubmitted
        O->>S: recall_with_proof (full sealed list of demo-repo)
        O->>M: EvidenceRetrieved, RecallProven
        O->>S: hyphae proof verify per distinct proof
        O->>META: critique (metered as above)
        O->>M: ChallengeSubmitted (after contract::validate_challenge)
        O->>META: evaluate (per-call keyed schema, metered)
        O->>M: EvaluationSubmitted (after contract::validate_evaluation)
        O->>M: EvidenceVerified per record of the round
        O->>O: policy derives the round's kind (policy::explain)
        M-->>B: consensus frames, delivery: live
        Note over O: revise only if not blocked, items open, progress, rounds left;<br/>else the round's kind is final
    end
    O->>M: RecommendationIssued, state AwaitingHumanApproval
    M->>L: release the reservation
    M-->>B: stream_end
    B->>CF: POST /api/v1/runs/{id}/decision {approved, note?, expected} + Idempotency-Key
    CF->>P: forwarded
    P->>M: owner check, HumanDecisionRecorded (authority public_demo)
    M-->>B: DecisionResponseV1 (no execution)
    B->>CF: GET /api/v1/runs/{id}/receipt
    CF->>P: forwarded
    P->>M: read the run's events
    P-->>B: RunReceiptV2 (build_receipt_v2: replay, re-derivation, chain check)
```

Points the diagram compresses, from the code:

- Before the critique, each record of the round context is integrity-checked
  (`verifier.verify`, once per distinct proof); the `EvidenceVerified` records, which join
  that integrity status with the Evaluator's factual and relevance judgments, are
  appended after `EvaluationSubmitted` (`orchestrator.rs`).
- Protocol 1.3 carries open items and uncleared contradictions across rounds:
  `open(k) = open(k-1) − resolved(k-1) + items(challenge k)`. A revision happens only
  while items remain open, the round's policy result is not `blocked`, the round resolved
  at least one previously open item (the first evaluation counts as progress), and the
  bound of 2 revision rounds allows (ADR-0010 decision 4). The policy runs after each
  round's `EvidenceVerified` records; when no revision follows, that round's kind is the
  recommendation (`orchestrator.rs`).
- A violating proposal, challenge or evaluation is refused before it is recorded:
  `RunFailed` at that stage with code `proposal_contract`, `challenge_contract` or
  `evaluation_contract`, the refused call kept in `RunFailed.invocation`.
- `RunControl` is checked before every model call, recall, proof verification and the
  recommendation; cancel, the wall-clock deadline (`CONSENSUS_RUN_DEADLINE_SECS`, 900)
  and shutdown end the run there with `RunFailed` (stage `cancelled`, `deadline` or
  `shutdown`). A browser disconnect does not stop a run.
- The runtime enforces `CONSENSUS_MAX_RUN_TOKENS` (default 150,000) after every call;
  the public live mode uses `CONSENSUS_LIVE_RUN_TOKEN_CAP` (default 70,000). A run that
  crosses its cap fails with stage `token_budget`.
- While a run deliberates, a decision is refused (`409`). The decision carries
  `expected: {recommendation, last_sequence}`; a run that no longer matches is `409
  decision_conflict`. Approving a `blocked` or `abstained` run needs a note.
- On the public surface a live-declared run's `events.ndjson` answers `404
  ndjson_unavailable` (`public_view::ndjson_publishable`): its chain may hold upstream
  text in failure reasons. Its detail and receipt stay available.

## 5. Trust boundaries

`docs/threat-model.md` section 1 is the full table; this is the shape.

```text
 Browser (untrusted)                    Cloudflare edge: TLS, WAF geo rules, rate-limit rule,
   apps/web static export        --->   Turnstile widget; sets CF-Connecting-IP, CF-IPCountry
                                                  |
                                             cloudflared (named Tunnel; group consensus-sock only)
                                                  | unix socket /run/consensus/public.sock (0660)
                                                  v
                                   serve-public (trusted code, untrusted input)
                                    | sessions, owner-scoped reads, guided runs,
                                    | live gate -> ledger files (trusted)
                                    | Meta key read through the live gate only
            +-----------------------+------------------------+
            v                                                v
   Meta Model API (paid third party,              hyphae proof verify (subprocess, trusted
   no hard cap; models untrusted)                 to verify, output untrusted) over the
                                                  sealed snapshot (integrity-checked;
                                                  witnesses private)

 serve-operator: loopback 127.0.0.1:8787, bearer token, started on demand over SSH;
   the only place for free task text and memory=hyphae (Hyphae container, operator lab only)
```

- **Public listener vs operator listener.** Two routers in one binary, never on one
  listener (ADR-0006 decision 1). The public one has no operator route, no console and no
  `/api/admin`; the operator one is loopback-only behind a bearer token, and its unit
  template has no `[Install]` section.
- **Browser.** Untrusted. Every label it shows is derived from server fields that are in
  turn derived from chained records: model (`scripted | live_meta | mixed |
  undetermined`), evidence (`simulated | hyphae_snapshot | hyphae_live | none | mixed`),
  source (`this_server | research_archive`), per-frame delivery (`replay | live`), and
  flags (`provenance_inconsistent`, `unchained`, `mode_undeclared`) (`api/types.rs`,
  `public_view.rs`). The declared `Task.mode` is only cross-checked.
- **Cloudflare.** Trusted for `CF-Connecting-IP` and `CF-IPCountry` only on the socket
  (or a development bind with `CONSENSUS_TRUST_PROXY_HEADERS=true`). The origin repeats
  the country block (`live/geo.rs`): the whole portal is blocked for the configured list
  (23 codes by default, Ukraine included), live mode also for `KR`. A request without
  `CF-IPCountry` is refused on the whole portal (`451 geo_unknown`); `XX`, `T1` or a
  malformed code is refused only for live mode. Edge features that rewrite HTML must stay off (gate G6). Nothing at the edge
  is configured yet.
- **Hyphae container.** Reached only by the operator lab through `docker exec`
  (`scripts/hyphae-docker.cmd`); it never sits behind the public server (ADR-0008
  context). The public server holds no Hyphae credential and runs only the verifier
  binary over the sealed snapshot. That verifier child runs as the same user as the
  server and can read its credential files, the Meta key file included (T12, MA-2).
- **Meta API.** Untrusted model output, paid calls with no hard spending cap; the ledger
  reserves before any public call and meters every attempt. The key is withheld from
  Hyphae children's environments but not from their file reads (section 2.4).

## 6. Event log and receipt formats

### 6.1 The hash chain

One JSONL record per event (`ConsensusEvent`): `schema_version`, `sequence` (gap-free
from 1 in its file), `event_id` (idempotency key), `run_id`, `actor` (`agent` with a
role, `runtime`, or `human` with an id), `occurred_at`, `prev_hash`, `hash`, `payload`.
`hash` is SHA-256 over `prev_hash` followed by the record's canonical JSON with `hash`
blanked, lowercase hex (`event_log.rs::compute_hash`); the first record's `prev_hash` is
`genesis`, and the first chained record after a pre-1.1 prefix links to
`pre-1.1:<sequence>`. Payload types (`EventPayload`, tagged by `type`): `task_created`,
`proposal_submitted`, `evidence_retrieved`, `evidence_unavailable`, `recall_proven`,
`evidence_verified`, `challenge_submitted`, `evaluation_submitted`, `revision_submitted`,
`recommendation_issued`, `human_decision_recorded`, `run_failed`.

Each append is fsync'd; a torn final record is cut on a writable open and is an error on
a read-only one; a failed append is rolled back; a record labelled with an older schema
than an earlier record of the same run fails (`SchemaDowngrade`). Replay re-derives every
recommendation recorded under schema 1.1 or later with the rules of its own version and
rejects a forged one; schema 1.3 records also re-run the evaluation contract. The chain
is unsigned: it is tamper-evident against edits, not against a consistent rewrite of the
whole file. The run index (section 2.6) records each run's head, so a run rewritten after
its head was published is detectable against that head.

### 6.2 PolicyTrace

`PolicyTrace` (`receipt.rs`) is computed by `policy::explain`, the single implementation
of the policy: `version` (`1.1` or `1.2`), `kind`, `rule` (the rule that decided:
`invalid_proof`, `recorded_contradiction`, `abstention`, `unresolved_items`,
`insufficient_evidence`, `all_checks_passed`), `reasons` (every failed check in the
version's priority order, each tagged by `reason`: `invalid_proofs`,
`contradicted_evidence`, `evaluator_abstained`, `open_items`, `no_assessments`,
`uncorroborated_claims`, `claims_without_own_evidence`, `cited_evidence_not_valid`,
`no_checked_evidence`; empty exactly when the run is `supported`) and `counts` (claims,
corroborated, proofs valid / invalid / unavailable or error, unresolved items). Schema 1.3
runs use the 1.2 rules and are labelled "protocol 1.3 · policy rules 1.2"
(`receipts::protocol_label`); 1.2 and 1.3 outcomes are never pooled.

### 6.3 RunReceiptV2

`GET /api/v1/runs/{id}/receipt` builds `RunReceiptV2` (`format:
"hyphae-consensus-run-receipt-v2"`) from the run's events at read time
(`build_receipt_v2`); receipts are never stored in the log. Fields: `schema_version`,
`run_id`, `run_schema_version`, `protocol`, `task`, `final_state`, `failure` (stage,
reason, code), `recommendation`, `policy_trace` (re-derived; always present for a
recommendation recorded under 1.1 or later), `deliberation` (rounds, final and evaluated
proposal, plan steps, claims with their final assessment, unmatched assessments, open
items, unevaluated items, assumptions, uncertainties, missing tests, revisions,
`earlier_round_objections_never_rejudged`), `authorization` (state, authority,
decided_at, note), `model_invocations`, `verifications` (each nested with its round),
`event_count`, `chain` (`scope`: `run`, `log`, `partly_chained` or `unchained`;
`head_hash`; first and last sequence; a note), `generated_at`, and `proof_limitations`.
The public surface sanitizes the receipt (`public_view::sanitize_receipt`).

A receipt and a log are checked offline with `consensus-api verify <log.jsonl> [--run
<id>] [--receipt <receipt.json>]`: read-only, chain and schema checks, replay, policy
re-derivation, and a comparison of the receipt's `run_id`, `event_count`, `head_hash` and
recommendation kind; exit code 0 only when everything verifies.

## 7. The public API contract

`contracts/api-v1/` holds one JSON Schema per request, response and SSE frame type (28
schema files: `ApiError`, `CreateGuidedRunRequest`, `CreateLiveRunRequest`,
`CreateOperatorRunRequest`, `DecisionRequest`, `ListRunsQuery`, `EventsQuery`,
`RunCreatedV1`, `RunSummaryV1`, `RunDetailV1`, `RunListV1`, `DecisionResponseV1`,
`CancelResponseV1`, `EventV1`, `ProvenanceV1`, `SseFrameV1`, `ConsensusFrameV1`,
`StreamEndV1`, `ResyncV1`, `SessionV1`, `ScenariosV1`, `CapabilitiesV1`, `HealthV1`,
`ReadyV1`, `ShowcaseV1`, `ShowcaseEntryV1`, `RunReceiptV2`, `PolicyTrace`) and
`fixtures/` produced by the real view builders over the five guided scenarios, plus a
live reference run (R01), a showcase entry (F03), the public capabilities and error
samples, with a fixed clock and sequential ids. The SSE event names are `consensus`, `resync` and
`stream_end`, with a 15 s heartbeat comment and a `retry` hint of 3,000 ms.

The contract's digest is SHA-256 over the sorted schemas, `<name>\n<pretty JSON>\n` per
schema (`api/contract.rs::contract_sha256_of`). At `bc3b262`:

```text
465f4117d8a12365ed06bc594f5972f19160d3016b085bcb5836e75786ad760e
```

`GET /api/v1/health` reports it as `contract_sha256`, next to `schema_version` (`1.3`),
`policy_version` (`1.2`) and `executor` (`disabled`). `cargo test -p consensus-api --test
api_contract` fails on drift; `cargo run -p consensus-api --example export_contract`
regenerates the directory; the web client's `pnpm api:check` fails when its generated
types name another digest.

## 8. Deployment topology (as documented; nothing deployed)

From `docs/deployment/runbook.md` and `deploy/`:

- A Cloudflare Free zone; a **named** Tunnel; `cloudflared` on the server whose ingress
  (`deploy/cloudflared/config.yml.example`) sends `<hostname>` and `staging.<hostname>`
  to `unix:/run/consensus/public.sock` and ends with `http_status:404`. Staging sits
  behind Cloudflare Access.
- An Ubuntu 24.04 server with 2 GiB RAM plus swap. Inbound only SSH from an allowlist;
  outbound only to `api.meta.ai`, `challenges.cloudflare.com` and Cloudflare's tunnel
  edge, so the server cannot fetch its own hostname and the edge probes run from the
  owner machine.
- `consensus-public.service` runs `consensus-api serve-public` as user `consensus` with
  `/etc/consensus/public.env` and `LoadCredential` for `meta.key` and `turnstile.secret`.
  `consensus-operator.service` (no `[Install]`, started on demand) runs
  `serve-operator` with `public.env` and `operator.env` and `LoadCredential` for
  `meta.key` and `operator.token`.
- `/run/consensus` is created by tmpfiles.d as `2750 consensus:consensus-sock`, so the
  socket's group is `consensus-sock`, cloudflared's only extra group: it can reach the
  socket and nothing else of the service.
- Writable: `/var/lib/consensus/runs` and `/var/lib/consensus/ledger/{staging,public}`
  (one ledger directory per phase), plus `/run/consensus` for the public unit's socket. Read-only to the services: the web export, the
  research archive and `/var/lib/consensus/snapshots/` (`0700`, files `0640`). Hyphae
  4.0.0 lives under its own user in `/opt/hyphae/` and is used only as the offline
  verifier. No Docker on the server.
- The binary is built reproducibly with `scripts/linux-gates.sh --release` in a pinned
  `ubuntu:24.04` container. Operator scripts: `install.sh`, `verify-edge.sh`,
  `sse-heartbeat-test.sh`, `canary-scan.sh`, `sandbox-probe.sh`, `kill-switch.sh`,
  `ledger.sh`, `judge-codes.sh`, `export-ledger.sh` (`deploy/README.md`).

## 9. Decision → ADR

| Decision | ADR |
|---|---|
| Four crates; contracts without I/O; runtime as the only state writer | 0001 |
| Models cannot write outcomes; the policy is one pure function; only a human authorizes | 0001 |
| Three orthogonal evidence axes (integrity, factual, relevance) | 0001 |
| Deterministic state machine, at most 2 revision rounds | 0001, 0010 |
| Append-only JSONL log, SHA-256 hash chain, version-faithful replay | 0001 (1.1 and 1.2 amendments) |
| Policy order 1.2: a recorded contradiction outranks abstention | 0001 (1.2 amendment) |
| Per-run token cap, `token_budget` failure | 0001 (1.2 amendment) |
| Chat Completions with strict structured output; drafts, not domain documents | 0002 |
| Keyed per-call Evaluator schema; retries on 429/503 and pre-send connection failures; one post-send transport retry | 0002 (2026-10-27 evening note) |
| Key precedence and handling; prompt-injection posture as defense in depth | 0002, 0007 |
| Hyphae memory profile without `--allow-write`; proof mapping; anchor pinning | 0003 |
| One proof per recall; store size under the 4 MiB MCP response cap | 0003 |
| Explicit, labelled operator modes; record-only, token-gated approval | 0004 (history), 0006 |
| One hash-chained log per run, a hash-chained run index, read-only legacy logs | 0005 |
| Validated, durable, then published; asynchronous `202` starts; `RunControl` checkpoints | 0005 |
| Live SSE: subscribe, replay, follow; `delivery` on every frame; `resync`, `stream_end` | 0005 |
| Per-run retention (24 h access, 7 d deletion, visitor-run ring) | 0005, 0006 |
| Public and operator surfaces never on one listener; unix socket behind cloudflared | 0006 |
| Anonymous cookie sessions; owner-scoped `404`s; no visitor free text to a model | 0006, 0007 |
| Provenance axes from records, not requests; `public_demo` authority | 0006, 0010 |
| Fixed public errors and stripped public views | 0006 |
| The contract is a file; its digest in `/health` | 0006 |
| The showcase is the archive, served byte for byte | 0006 |
| No hosted BYOK; live mode on the owner's dedicated key; presets only | 0007 |
| Two-phase live admission; Turnstile before the slot; per-client and per-code daily quotas | 0007 (implementation) |
| USD ledger reserving the worst case; caps $10/day, $60 lifetime, judge slice | 0007 |
| Operator `model=meta` runs metered on the same ledger | 0007 (implementation) |
| Geographic duties of an Integrated Product | 0007 |
| Sealed Hyphae snapshot as public evidence; witnesses never published | 0008 |
| Snapshot binding: manifest digests, manifest pin, verifier-binary pin; `BootBoundVerifier` | 0008 (Wiring, Binding) |
| A snapshot that fails disables live mode, not the portal | 0008 decision 1 |
| Next.js static export, Astryx prebuilt, four locales, own theme | 0009 |
| Honesty is data: LIVE only for live frames of a live model run | 0009, 0006 |
| Node toolchain only in Docker; per-page CSP hashes | 0009 |
| Schema 1.3 additive; policy rules unchanged ("protocol 1.3 · policy rules 1.2") | 0010 |
| Fail-closed evaluation contract before recording and on replay | 0010 |
| Cross-round carry-forward; set-based progress | 0010 |
| No schema downgrade inside a run | 0010 |
| Receipts v2 list earlier-round objections never re-judged | 0010 |

## 10. Verification at this commit

- Rust gates (`docs/threat-model.md` header, `docs/security/release-checklist.md` G8):
  Windows at `25c7867`, 600 passed / 0 failed / 2 ignored; Linux parity
  (`scripts/linux-gates.sh`, Ubuntu 24.04) on the tree of `25c7867`, 7 of 7 gates,
  603 passed / 0 failed / 2 ignored. The two ignored tests are the gated live probes
  `live_hyphae_probe` and `live_meta_propose`. The fresh-clone run of G8 is still owed.
- Release gates: G8 PASS; G12 partly; G1–G7 and G9–G11 pending
  (`docs/security/release-checklist.md`).
- Since `25c7867` the crates changed only in a doc comment (`ports.rs`, `88e85d5`), and
  lane W2's merge (`bc3b262`) changed `apps/web`; the gates were not re-run at `bc3b262`.

## 11. Where the documents and the code differ (at `bc3b262`)

This overview follows the code in each case:

1. README, "Safety invariants": "a visitor can decide only on their own simulated runs".
   The code lets a visitor decide on any run they own, live runs included, always with
   `authority: public_demo` (`api/v1.rs::decide`, `runs.rs::RunManager::decide`); ADR-0007
   "Consequences" says the same as the code.
2. ADR-0005 decision 7: "a bounded ring of visitor runs (`503 storage_full` beyond it)".
   The code purges the oldest resting visitor runs to make room and answers `503
   storage_full` only when nothing can be freed (`public_config.rs`,
   `CONSENSUS_GUIDED_RING`; `api/v1.rs::admit_creation`).
3. ADR-0006 decision 3: `POST /api/v1/live-runs` answers `503 live_disabled` "until the
   admission layer of ADR-0007 is wired". It is wired; `503 live_disabled` now means
   `CONSENSUS_LIVE_ENABLED` is off.
4. ADR-0006 decision 4 lists the evidence axis as `simulated | hyphae_snapshot |
   hyphae_live | none` and the model axis as `scripted | live_meta`. The code adds
   `mixed` to both and `undetermined` to the model axis (`api/types.rs`).
5. ADR-0009 decision 5 names `@xyflow/react` 12 with dagre for a claim/evidence graph.
   Neither is a dependency in `apps/web/package.json`, and no source file imports them.
6. Doc comments in `consensus-domain`: `lib.rs` says of `RecallProven` "nothing emits it
   yet", but `orchestrator.rs` emits it after every proven recall; `run.rs` says
   `HumanAuthorization.authority` "stays `None` until the operator and public routes set
   it", and both routes set it.
7. README: the setup block says "494 tests (+2 ignored live)" and the 2026-10-28 status
   says the Evidence, Proof, Audit and Meta API views "are being built"; the gates at
   `25c7867` count 600 (Windows) and 603 (Linux), and those views landed with `bc3b262`.
8. ADR-0007 decision 3: "The public server accepts no visitor free text anywhere." The
   code accepts one, the optional decision note (at most 2,000 characters), which it
   stores and shows and never sends to a model (`api/types.rs`; release checklist G10).
9. `docs/architecture/sealed-snapshot.md`, "Observed in the capture session" item 2, says
   `agent ui` prints the result object at the root next to `ok`, `schema` and `id`. The
   committed capture holds it under `result`
   (`snapshots/snap-2026-10-29-demo-repo/captures/list-demo-repo.response.json` begins
   `{"id":1,"ok":true,"result":{`), and the code at `bc3b262` reads the root only
   (section 2.4). Both the code and that sentence are corrected in `0515f29`, merged as `7e26866`.
