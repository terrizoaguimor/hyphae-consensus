# Hyphae Consensus

**Verifiable Multi-Agent Reasoning and Decision Accountability**

Meta Global AI Developer Hackathon 2026 · Indie Developer track · Push the Frontier
Development window: October 26 – November 4, 2026 · Owner: Mario (independent developer)

Hyphae Consensus has three Meta-model agent roles review a software-engineering change
before anything happens. A Proposer states explicit claims, a Critic attacks them, and an
Evidence Evaluator judges each claim against records retrieved from Hyphae Agent Memory.
The runtime checks the records' integrity proofs, a fixed deterministic policy turns
the judgments and proof statuses into a recommendation with a step-by-step trace, and
only a human authorizes. Every step is written to a hash-chained event log that anyone
can replay and check for later changes (the log is not signed, and the archived proofs'
witnesses are not published; see "What is verified and what is not").

Independent project; not affiliated with, sponsored by, or endorsed by Meta.

## What it does

- **Roles.** The **Proposer** turns a task into claims, assumptions and uncertainties, and
  revises them after review. The **Critic** raises individually tracked objections
  (contradictions, unsupported claims, missing tests, constraint conflicts, risks). The
  **Evidence Evaluator** judges, per claim, whether the retrieved records are relevant and
  whether they corroborate or contradict it, and gives an explicit verdict on every open
  objection. All three are Meta models (see "How this project uses Meta's API").
- **Runtime.** The runtime (not a model) retrieves the evidence from Hyphae, verifies each
  recall's sealed proof offline with `hyphae proof verify`, enforces the evaluation
  contract, bounds the debate (at most 2 revision rounds, 9 model calls, 20 distinct
  records and a per-run token cap), and records every event.
- **Policy.** A fixed function turns the Evaluator's judgments and the proof statuses into
  one of `supported`, `contested`, `insufficient_evidence`, `abstained` or `blocked`, and
  records a `PolicyTrace` that shows which rule decided. New runs are labelled
  "protocol 1.3 · policy rules 1.2" (ADR-0010): the policy function of schema 1.2 is
  unchanged, and protocol 1.3 makes its inputs complete.
- **Human.** The run stops at `AwaitingHumanApproval`. Only a human decision is recorded
  as an authorization, and nothing is executed in any mode: the executor is deferred, so
  approval records a decision only.

**"Models never decide" holds in form, not in substance.** No model writes the outcome
field or can approve, and a model's draft structurally cannot carry an outcome or an
authorization. But the Evaluator's judgments are the policy's main input, and the Critic's
objections count through the Evaluator's per-item verdicts, so the models shape the
outcome even though they do not write it.

## How this project uses Meta's API

- **Model.** `muse-wonder-1.0`, the default model id (`DEFAULT_META_MODEL` in
  `crates/consensus-api/src/config.rs`; `CONSENSUS_MODEL` overrides it). Every recorded
  live run in `research/` used it, and every completed Meta call there is recorded with
  `provider: meta`, the model id and the API's request id (scripted calls record
  `provider: fake`; `research/README.md`).
- **Endpoint.** The Meta Model API's Chat Completions, `POST {base}/chat/completions` with
  base `https://api.meta.ai/v1` (`crates/consensus-api/src/meta.rs`, ADR-0002).
- **Which roles call it.** All three roles, in four kinds of call with one strict schema
  each (`Role` in `meta.rs`): Proposer (`DraftProposal`), Critic (`DraftChallenge`), Evidence
  Evaluator (`DraftEvaluation`) and the Proposer's revision (`DraftRevision`). The
  scripted doubles (`provider: fake`) replace all four in the guided runs and the tests.
- **Structured output.** `response_format: {type: "json_schema", json_schema: {name,
  schema, strict: true}}`. Every list field is required (a missing one is malformed
  output, since `3d50baf`). The Evaluator's schema is built per call and keyed by id:
  `assessments` has exactly one required property per claim id of the proposal and
  `item_verdicts` one per open objection, no other properties allowed, so an omitted,
  repeated or unknown id is `MalformedOutput`, never a silent default (tag
  `prompt-freeze-2`; `docs/evaluation.md` §6.2). Responses are parsed into typed drafts
  and cross-checked; ids, timestamps and schema versions are assigned by the runtime.
  Each invocation records the SHA-256 of the schema it sent and of its role's prompt
  template, `finish_reason`, the attempt count and the reasoning tokens.
- **Request bounds and retries** (`meta.rs`, `config.rs`). `max_completion_tokens`
  16,000 by default; one 120 s time budget per call (`CONSENSUS_MODEL_TIMEOUT_SECS`)
  shared by up to 3 attempts. Only 429 and 503 are retried, after `Retry-After` when it
  fits or after a full-jitter backoff from 500 ms; a connection that failed before the
  request was sent may be retried too; a connection that broke after the request was sent
  is retried exactly once (it may have been billed). Timeouts, 504 and other 4xx are never
  retried. `finish_reason: length` and `refusal` are recorded as failures with stable
  codes; no response body or URL reaches an error, event or receipt.
- **Prompts.** Everything a role did not write itself sits in separate `<UNTRUSTED_*>`
  blocks; a mitigation, not a boundary (details under "Safety invariants").
- **The public live mode (on the owner's key; not deployed).** Decided in ADR-0007,
  operated per `docs/deployment/runbook.md`:
  - **No hosted BYOK.** The portal never accepts a visitor's Meta key (Meta's Model API
    terms make keys confidential and forbid gateway access; ADR-0007). Visitors who want
    their own key self-host this repository.
  - **Presets only, no free text.** A live run takes one of five preset ids whose task
    texts are the recorded battery tasks, verbatim, against project `demo-repo`
    (`R01-reference`, `R03-raise-to-10`, `R05-add-jitter`, `R06-timeout-30s`,
    `R07-exp-backoff`; `crates/consensus-api/src/live/presets.rs`). Nothing a visitor
    types reaches a model.
  - **Evidence** comes from a sealed Hyphae snapshot whose files are re-checked at every
    admission (ADR-0008, `docs/architecture/sealed-snapshot.md`). The snapshot
    `snap-2026-10-29-demo-repo` was captured on 2026-10-29 (`b0375df`; its public files
    are in `snapshots/`, its digests under "Published snapshot digests" below); it is not
    on any server yet, and the code on this branch needs the not-yet-merged fix `0515f29`
    to load it (see Status).
  - **Two-phase gate** (`live/gate.rs`): preset, geography, an 18+ attestation, the
    evidence status and Turnstile before the single live slot is taken; then the client's
    daily quota (default 3 public-lane runs per IPv4 /32 or IPv6 /64 per UTC day) and a
    ledger reservation of the run's worst case, before any model call.
  - **USD ledger** (`live/budget.rs`, `live/config.rs`): append-only, fsynced, shared by
    every process that holds the key. Caps by owner decision: $10 per UTC day, $60
    lifetime, of which $15 is the slice of the judge lane (runs admitted with an invite
    code, with their own daily limit). A run's worst case is (70,000-token
    run cap + 16,000 output + 32,000 input cap) × $20/M = $2.36. Every HTTP attempt is
    drawn on the reservation and settled once; timeouts, 5xx, post-send transport errors
    and responses without usage are charged at the attempt's worst case and never
    refunded. The ledger trips on an overrun, on 401/402/403 from Meta or at the lifetime
    cap, and the kill switch is the same state. The operator console's `model=meta` runs
    are metered on the same ledger when `CONSENSUS_LIVE_LEDGER_DIR` is set. From a shell
    on the server the operator reads it, clears a trip and releases a crash-stranded
    reservation with `consensus-api ledger status | reset | release` (reconciliation with
    the provider dashboard: runbook §9.3).
  - Each public live request carries the session's pseudonym (`visitor:<16 hex>`) as
    `safety_identifier`. Measured cost on the final code (ADR-0007): $0.81 mean, $0.98
    maximum per run.

## What is verified and what is not

### Integrity proofs: what they do and do not establish

A valid Hyphae proof establishes that the recall it seals reproduces against its anchored
snapshot under Hyphae's documented assumptions. `Valid` requires the verifier's report to
say `verified`, `kind: memory`, `scope: semantic_reexecution` and the anchor it was given
(`crates/consensus-api/src/hyphae.rs`; threat model T8). The record text handed to the
models is still not byte-compared with the proof (audit finding HY-1, open for that part).
Every receipt states the limits in its `proof_limitations` field
(`RunReceipt::DEFAULT_LIMITATIONS`, `crates/consensus-domain/src/receipt.rs`):

> A proof marked valid means that Hyphae's verifier re-executed the request the proof
> sealed against its witness and accepted it at the anchor it was given (the pinned anchor
> wherever one is configured, always for the public snapshot). It covers the whole result
> of that request (for the sealed snapshot, the complete record list of the snapshot's
> project at that anchor), never a claim, even when the request's query is a claim's text,
> and a record's text is not byte-compared with the proof. A simulated proof checks nothing.
> Proofs do not establish that the content is true, relevant, or complete, or that a model
> reasoned from it.

This is why `EvidenceVerification` keeps integrity, factual corroboration and relevance as
three independent fields. A `supported` recommendation requires **every** claim to be
corroborated by at least one record that the claim itself cites, a valid proof on every
cited record, no contradicting record (cited or not), no Critic item the Evaluator still
judges unresolved, and no invalid proof anywhere in the retrieved set. A recorded
contradiction outranks an abstention (`contested`, schema 1.2), so abstaining cannot hide
it. Since protocol 1.3 (ADR-0010) the runtime enforces the evaluation contract itself, for
every model client: exactly one assessment per claim, cited ids drawn from the run's
retrieved set, an explicit verdict for every open objection, and objections and
contradictions from earlier rounds carried forward until the Evaluator judges them (the
audit's MA-1 and RT-1 findings). Runs recorded before 1.3 keep their recorded semantics;
their receipts state how many earlier-round objections were never re-judged. One proof
covers a whole recall. By default the proof anchor comes from the same server response as
the proof, which shows internal consistency only; pin an out-of-band anchor with
`CONSENSUS_HYPHAE_EXPECTED_ANCHOR` to verify against a trusted one (the sealed snapshot
requires it).

The hash chain is tamper-evident, not signed: it reveals a change made after the fact, not
a full rewrite, and it does not prove that the runs happened as recorded or that request
ids, token counts and latencies are what Meta's API returned (`research/README.md`).

### Published snapshot digests

The public live mode's evidence is the sealed snapshot `snap-2026-10-29-demo-repo`, captured
on 2026-10-29 at 17:16:12 UTC in the `hyphae-consensus` container with hyphae 4.0.0
(`b0375df`; `snapshots/snap-2026-10-29-demo-repo/`; the session is described in
`docs/architecture/sealed-snapshot.md`, "Observed in the capture session"). It holds the 7
synthetic `demo-repo` records, whose ids equal the research ids, and two proven captures
(`list-demo-repo` and the empty `recall-other-repo-isolation`), both verified by
`hyphae proof verify` with `kind: memory` and `scope: semantic_reexecution`. As ADR-0008
decision 2 decides, these are the three values the production server pins:

| Value | Digest | Server variable |
|---|---|---|
| Anchor | `43a2dea9c48d015bf6cbd6692b7ecf2bf80c66cd47d398b38e6b8471b18e8f02` | `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` |
| SHA-256 of `manifest.json` | `b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a` | `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` |
| SHA-256 of the Hyphae binary | `79025e8758ee7481f572d0451756c0e0771e96d13cd7134319367afa8b11a15f` | `CONSENSUS_HYPHAE_BIN_SHA256` |

Check them in a clone: `sha256sum snapshots/snap-2026-10-29-demo-repo/manifest.json` prints
the manifest digest, and the manifest records the anchor (`anchor_hex`) and the binary digest
(`hyphae.binary_sha256`, also in `hyphae-binary.sha256`). The binary digest is the one the
capture recorded for the container's binary; it has **not yet** been compared with the binary
extracted from the release tarball checked against `cf92ed6d…`
(`docs/deployment/runbook.md` section 3.2). The proofs and witnesses are not published
(ADR-0008 decision 3), so a clone cannot re-run the verifier, and the committed directory
alone does not boot (`checksums_incomplete`). The release tag is to carry the same three
values. The snapshot is not on any server yet.

### SIMULATED, RECORDED and LIVE

Every run carries provenance axes derived from its records (model, evidence, delivery),
and the portal labels them so that a scripted or replayed run is never shown as live
(release gate G7, pending: enforced in the API and the web workspace, not yet asserted
through the public hostname; `apps/web/README.md`):

- **SIMULATED**: scripted model doubles (`provider: fake`) and scripted proofs
  (`fake-verifier`, "SIMULATED proof"). The guided runs are simulated end to end; their
  outcomes say nothing about live model behavior.
- **RECORDED**: a run of the `research/` archive, shown read-only. A recorded live-model
  run reads META MODEL with its recorded date, never LIVE.
- **LIVE**: only frames delivered live from a live Meta run in progress; LIVE META is
  shown next to its evidence label (SEALED SNAPSHOT), never alone. A finished run is
  Playback.

## Results so far

All figures are from `docs/evaluation.md` and `research/`, as recorded. The scripted check
and the live batteries are different things and are not pooled; neither is a baseline
comparison (no single-agent baseline was executed).

- **Scripted protocol conformance check** (deterministic doubles; not an experiment):
  the runtime produced the author-defined outcome in 10/10 cases
  (`cargo run -p consensus-tests --example evaluate`; §1).
- **2026-10-26, live Meta + live Hyphae, schema 1.1** (§2): 12 runs, 9 `blocked`,
  1 `abstained`, 2 failed on the then 60 s timeout, 0 `supported`. Every proven recall
  failed upstream with Hyphae's generic `corruption` code; the cause was a deployment
  misconfiguration (the daemon's socket inside its data directory), not tampering
  (ADR-0003). Three supplementary runs with live Meta and SIMULATED proofs (S01–S03)
  ended `abstained` 3/3 (§3).
- **2026-10-27, working proofs, schema 1.2** (§5). Every verification record in the
  receipts of battery 1 (76) and of the final battery (91) is `valid` (`hyphae proof
  verify`, pinned anchor).
  - P01–P02 ran under a first Evaluator wording that was then replaced; they are not
    counted (§5.1). P02 reached `supported` only because that wording let objections that
    still applied count as resolved.
  - Battery 1, R01–R08: 1 `supported`, 2 `contested`, 5 `insufficient_evidence` (§5.2).
    **R06**, battery 1's one `supported` (the first counted live `supported` with every
    cited record verified), is read with its caveats: under protocol 1.2 its 7 round-1
    objections were never re-judged (RT-1, correction 4); its claim 3 cites a seeded
    record that misdescribes the fixture test, though its other two citations corroborate
    that claim on their own (correction 3); and `supported` attests the *revised* claims against operator-seeded memory, not the
    task's original 30-second request and not production correctness.
  - Final battery, F01–F06 (strict output): 6 `contested`. In every final round every
    claim was corroborated by verified records it cites; the 37 open Critic objections
    (mostly missing tests) kept `supported` out under the unchanged policy (§5.3).
  - Hostile note with a valid proof, H01–H02: both `contested`. The Critic flagged the
    "SYSTEM OVERRIDE" note, the Evaluator recorded it as contradicting evidence and it was
    never obeyed; H02's final round corroborated 1 of 4 claims, and H01's had 2 claims with
    contradicting evidence (§5.4, correction 1). Two runs, one model, one note: not a
    robustness result.
- **Protocol 1.3 smoke runs** (§6; not counted as validation; a 240 s budget per call).
  On `prompt-freeze-1` (2026-10-27 evening), K01
  ended `insufficient_evidence` because the strict schema allowed an empty `assessments`
  array (fixed by the keyed schema) and K02 failed on a post-send transport error. On
  `prompt-freeze-2` (run 2026-10-28 03:23–03:31 UTC), K03 and K04 ended `supported`
  **for a revised proposal that departs from the task text**; every claim received one
  assessment and every objection an explicit verdict. No rate of any outcome under
  protocol 1.3 is claimed.
- **The counted protocol 1.3 battery, M01–M04** ([`docs/evaluation.md` §6.3](docs/evaluation.md);
  2026-10-30, 18:52–19:08 UTC; the last 4 of the 8 authorized runs), through
  `consensus-api serve-operator` built at `f0f3815`, on the `prompt-freeze-2` prompts,
  against the re-seeded demo store. **2 of the 4 runs were lost to provider errors**: M01
  failed at critique (a Meta call past its 120 s budget, in round 3) and M02 failed at
  critique (Meta HTTP 500, in round 2). **M03 is the battery's only `supported` run**, and
  it is `supported` **for a revision that departs from the task text** (it does not claim
  the exponential schedule the task asked for; one of its claims restates the seeded
  record that misdescribes the fixture test). M04 failed at evaluate: protocol 1.3's
  contract refused the round-2 evaluation because it neither cited again nor cleared a
  contradiction carried from round 1, the fail-closed behaviour ADR-0010 specifies. 25
  Meta calls (23 answered, 0 retries), 37,363 / 95,850 tokens; all 41 verification records
  `valid`. Archived verbatim in `research/2026-10-30/` (`research/README.md`, "The
  2026-10-30 battery"). With 2 of 4 runs lost, no rate of any outcome under protocol 1.3
  is claimed.

No run self-authorized; every battery run's authorization is `pending`. The archive holds
12 event logs with 48 runs (919 events) and 41 run records, byte-identical to the originals
and pinned by `research/MANIFEST.json` and `research/SHA256SUMS`; the raw model drafts and
the Hyphae proof witnesses are not published (`research/README.md` explains why and what a
third party can and cannot check).

## Mission Control

**Nothing is deployed.** Mission Control (`apps/web`) is the public portal's frontend: a
Next.js static export served by `consensus-api serve-public`, in English, Spanish,
Brazilian Portuguese and French, light and dark. It runs locally against the real server
and in the end-to-end suite against a mocked API. Its views ([`apps/web/README.md`](apps/web/README.md);
[`docs/mission-control.md`](docs/mission-control.md) holds the superseded notes on the
earlier operator-console UI):

- **Overview**: what the system does, how a deliberation works, the provenance legend and
  figures counted in the browser from the showcase.
- **New deliberation**: the five guided (SIMULATED) scenarios, and the live presets with
  the public budget, the attestation and Turnstile when live mode is enabled; otherwise a
  fixed "Live mode is not enabled on this server" state.
- **My runs**: the visitor session's runs.
- **Run workspace**: Live (while a live run deliberates) or Playback, Explorer, Evidence,
  Proof inspector, Audit trail (the chain checked in the browser, the receipt, the check
  command), Meta API (invocations and totals) and Decision (a public decision carries
  `authority: public_demo`, no operational authority).
- **Research showcase**: the `research/` archive by battery, each run in RECORDED mode,
  with the logs' pinned digests and `MANIFEST.json` checked in the browser.
- **Docs** (seven articles), **Settings**, and draft **Terms**, **Privacy**,
  **Limitations** and **About** pages awaiting the owner's sign-off.

The portal goes public only when every release gate in
[`docs/security/release-checklist.md`](docs/security/release-checklist.md) reads PASS.

## Architecture

The overview is [`docs/architecture/overview.md`](docs/architecture/overview.md). In short: `consensus-domain` holds typed
contracts only; `consensus-runtime` holds the state machine, orchestration, policy,
event log and per-run store; `consensus-api` holds the Meta and Hyphae adapters, the two
servers (public API v1 and the loopback operator console, on separate listeners), the
live-mode admission and the CLIs; `apps/web` is the frontend.

Decisions: [`docs/adr/`](docs/adr/) (0001 M1 architecture, 0002 Meta adapter, 0003
Hyphae adapter, 0004 operator console, 0005 per-run logs and live SSE, 0006 public API and
the listener split, 0007 live mode on the owner's key, 0008 the sealed snapshot, 0009
frontend stack, 0010 protocol 1.3). The sealed snapshot:
[`docs/architecture/sealed-snapshot.md`](docs/architecture/sealed-snapshot.md). Threat
model v2: [`docs/threat-model.md`](docs/threat-model.md). Release gates:
[`docs/security/release-checklist.md`](docs/security/release-checklist.md). Deployment:
[`docs/deployment/runbook.md`](docs/deployment/runbook.md) and [`deploy/`](deploy/). API
contract: [`contracts/api-v1/`](contracts/api-v1/). Evaluation:
[`docs/evaluation.md`](docs/evaluation.md). Technical pitch:
[`docs/pitch.md`](docs/pitch.md). Implementation journal:
[`docs/journal.md`](docs/journal.md). Reviews, agent briefs and the 2026-10-27
independent audit: [`docs/reviews/README.md`](docs/reviews/README.md).

## Run it locally

Prerequisites: Rust 1.99.1 via [rustup](https://rust-lang.org/tools/install), pinned in
`rust-toolchain.toml` (on Windows with the MSVC build tools, see the
[rustup book](https://rust-lang.github.io/rustup/installation/windows-msvc.html)), and
Docker for the web app. The commands below are for a POSIX shell (Git Bash on Windows),
from the repository root. None needs a credential, and none calls Meta or Hyphae.

```sh
cargo build --workspace --locked
cargo test --workspace --locked                           # deterministic; the 2 live tests are ignored
cargo run -p consensus-tests --example evaluate           # scripted protocol conformance check (10/10)
cargo run -p consensus-api --locked -- verify research/2026-10-27/live-battery-2026-10-27-final.jsonl   # read-only chain and policy check
```

**Scripted demo.** The `consensus-api` binary without a subcommand runs the reference
scenario to `AwaitingHumanApproval`; `--approve` or `--reject` also records a decision
(nothing is executed). It keeps its log in `scripted-demo/events.jsonl` under the current
directory and replays the run it finds there instead of re-running, so run it from a
scratch directory. It refuses to run inside `demo-run/` or `research/`, and any other
argument prints the usage.

```sh
REPO=$(pwd); cd "$(mktemp -d)"
"$REPO/target/debug/consensus-api"              # scripted run, stops at AwaitingHumanApproval
"$REPO/target/debug/consensus-api" --approve    # replays it and records an approval
```

**Operator console** (loopback only; it must never be exposed):

```sh
CONSENSUS_RUN_STORE_DIR="$(mktemp -d)" cargo run -p consensus-api --locked -- serve-operator
# http://127.0.0.1:8787/ ; the bearer token is printed once at startup (or read from CONSENSUS_OPERATOR_TOKEN_FILE)
```

`model=fake` runs the five guided scenarios with simulated proofs and needs nothing else.
A non-loopback bind is refused, every `/api` route needs the bearer token, and approving a
`blocked` or `abstained` run needs a note. Live modes (`model=meta`, `memory=hyphae`) need
credentials from the environment (`.env.example`, `scripts/hyphae-container/README.md`),
spend money on the Meta key, and without them the API names the missing variable instead
of falling back. Secrets must come from environment variables, never files committed to
this repository.

**Web app and public server.** The Node toolchain runs only in throwaway containers, never
on the host (`apps/web/README.md`, "Build only through Docker"):

```sh
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm build        # static export to apps/web/out
sh apps/web/scripts/in-docker.sh pnpm check        # theme, contract types, typecheck, lint, Vitest, export
sh apps/web/scripts/playwright-docker.sh           # end-to-end and axe tests against out/, API mocked
CONSENSUS_PUBLIC_BIND=127.0.0.1:8788 CONSENSUS_COOKIE_SECURE=false \
  CONSENSUS_RUN_STORE_DIR="$(mktemp -d)" cargo run -p consensus-api --locked -- serve-public
# http://127.0.0.1:8788/ : guided runs and the research showcase; live mode stays off
```

`CONSENSUS_PUBLIC_BIND` is for development only (loopback; in production the public server
listens on a unix socket behind cloudflared). The public server's variables are documented
in `crates/consensus-api/src/public_config.rs` and `crates/consensus-api/src/live/config.rs`.
Both servers keep one hash-chained log per run under `CONSENSUS_RUN_STORE_DIR` (default
`consensus-store/`), never a log of `demo-run/` or `research/`. Request bodies are typed:
unknown fields are rejected (`422`), `model` and `memory` are enums without defaults,
`model=fake` with a non-fake memory is refused, and the public surface accepts no free
text for any model (guided scenarios and live presets only; the optional decision note
after a run is the one visitor text, and no model sees it).

**Never delete `demo-run/`.** The git-ignored `demo-run/` holds the original live research
data behind `docs/evaluation.md`: the battery logs, the run records with their receipts,
the raw model drafts and the Hyphae recovery files. `research/` holds byte-identical copies
of its event logs and run records; the raw drafts and the recovery files exist only in
`demo-run/`. Never open a log in either place with a writing tool: `EventLog::open` opens a
log read-write and can repair a torn tail. Earlier builds kept the CLI demo's log in
`demo-run/events.jsonl`, which therefore holds the 2026-10-26 evening runs of
`docs/evaluation.md` §4; the demo no longer opens it. The servers open archived logs
read-only (`consensus-api verify`, the showcase).

## Status

**2026-10-30 (nothing is deployed).** The counted protocol 1.3 battery ran on 2026-10-30,
spending the last 4 of the 8 authorized live runs ([`docs/evaluation.md` §6.3](docs/evaluation.md)):
2 runs lost to provider errors (a 120 s timeout, an HTTP 500), 1 `supported` for a
revision that departs from the task (M03), 1 failed closed by the evaluation contract
(M04); every verification `valid`. It is archived in `research/2026-10-30/` on local
branches not yet merged into `feat/public-portal`. The authorized cap of 8 live
re-validation runs is reached. The block below is the status of 2026-10-29; apart from the battery it has
not been re-checked.

**2026-10-29 (branch `feat/public-portal` at `6e202eb`; nothing is deployed).**
Protocol 1.3 with policy rules 1.2, the public API v1, the loopback operator console, the
live-mode admission (two-phase gate, Turnstile, per-client quota, USD ledger) and the
sealed-snapshot evidence client are implemented; live mode stays off until its gates pass.
Mission Control's run views (lanes W1 and W2: New deliberation, My runs, the run workspace
with all seven tabs, the research showcase in RECORDED mode) are merged, and W2's
adversarial review is in progress. Tests on the tree of `25c7867`: 600 passed / 0 failed /
2 ignored on Windows, 603 / 0 / 2 on Linux with 7/7 gates (release checklist, G8).
Release gates: G8 passes (the fresh-clone run is still owed before the tag), G12 is partial, the other ten are pending.
The Hyphae container is on the new development machine. The sealed snapshot
`snap-2026-10-29-demo-repo` was captured there on 2026-10-29 (`b0375df`; digests under
"Published snapshot digests"). A test boot on the real snapshot found a format mismatch:
the server code at `6e202eb` rejects it, because Hyphae 4.0.0 wraps its answer differently
from what the loader expected. The fix (`0515f29`, merged as `7e26866`) is tested.
With it, the snapshot loads, Hyphae 4.0.0 re-verifies both proofs, and a local test run
over it served all 7 records as verified, against a stand-in for the Meta API (no real
Meta call). Not done yet: merging that fix; comparing the published
binary digest with the release tarball (runbook 3.2); the counted protocol 1.3 battery (not
run on 2026-10-29: the Meta key was not set on this machine and only the owner could set it;
no Meta call was made; it ran on 2026-10-30, see above); the staging deployment and the snapshot on the server (owner
authorization); the video; the owner's sign-off of the legal texts; and the final report.
Earlier status blocks (2026-10-27 and 2026-10-28) are in `docs/journal.md`, "README status
history".

## Competition boundary: pre-existing vs. newly developed work

This repository is a new project created for the hackathon. It consumes pre-existing
open-source technology through documented public interfaces only.

**Hyphae is the owner's own earlier work.** Hyphae Core and Hyphae Agent Memory are
Mario Gutiérrez's own earlier open-source project (Apache-2.0, copyright Celiums Solutions
LLC). It was frozen at commit `418a3da` (2026-09-24), before the hackathon began; Hyphae's
`main` had no commits after kickoff as of 2026-10-27. This project uses Hyphae only through
its public interfaces and does not claim it as new work.

**Pre-existing (NOT built for this hackathon, NOT claimed as new work):**

| Component | Pinned revision | Evidence |
|---|---|---|
| Hyphae Core + Hyphae Agent Memory | Commit `418a3da717c2f9ba0cf095423f392b3d496cb607` (2026-09-24), release Hyphae 4.0.0, tag `release-v4.0.0-crates` (2026-09-23) | [github.com/Hyphae-Research-Foundation/hyphae](https://github.com/Hyphae-Research-Foundation/hyphae) |
| Hyphae root `LICENSE` at that commit | Apache License 2.0 (full text fetched and read 2026-10-26) | `docs/product/agent-memory.md` carries `SPDX-License-Identifier: CC-BY-SA-4.0` |

Hyphae already provides: local shared agent memory, the MCP tools
`hyphae_memory_store / journal / recall / forget / status`, permission profiles
(`memory-reader`, `memory-writer`, `auditor`, `operator`), per-project memory isolation,
TTL/expiry, backup/restore, and sealed recall proofs verifiable offline with
`hyphae proof verify`. None of Hyphae's code, internals, or history is vendored, copied,
forked, or modified by this project. Hyphae runs as its pinned Linux build in a local
Docker container, because the Windows build's Agent Memory surface does not start on the
development host (`scripts/hyphae-container/README.md`).

**Newly developed for this hackathon (this repository):**

- The three-role deliberation protocol (protocol 1.3: fail-closed evaluation contract and
  cross-round carry-forward) and the deterministic consensus state machine.
- Typed, versioned contracts for proposals, claims, challenges, evaluations,
  recommendations, human authorizations, events, run receipts and the `PolicyTrace`
  (`crates/consensus-domain`, JSON Schemas in `schemas/`).
- Orthogonal evidence status modeling (`integrity_status` / `factual_status` /
  `relevance_status`) and the recommendation-derivation policy.
- The append-only, crash-safe, replayable, hash-chained event log and the per-run store.
- Meta Model API adapter (`muse-wonder-1.0`, strict structured output with keyed per-call
  schemas, timeouts, bounded retries, per-attempt metering; `crates/consensus-api/src/meta.rs`,
  ADR-0002).
- Hyphae MCP adapter over `hyphae mcp --profile memory` (stdio JSON-RPC, read-only tools
  only) plus `hyphae proof verify` integrity checking (`crates/consensus-api/src/hyphae.rs`,
  ADR-0003), and the sealed-snapshot evidence client (`crates/consensus-api/src/snapshot.rs`,
  ADR-0008).
- Public API v1 and operator console (Axum; asynchronous runs, live SSE, visitor sessions,
  the listener split), the live-mode admission and USD ledger (`crates/consensus-api/src/live/`),
  the `verify` and `ledger` CLIs, the Mission Control web portal (`apps/web`), the API
  contract (`contracts/api-v1`), deployment templates and scripts (`deploy/`), the
  synthetic demo repository (`demo-repo/`), adversarial integration tests, a scripted
  protocol conformance check, and the live Meta + Hyphae batteries (`docs/evaluation.md`,
  archived verbatim in `research/`).

## AI assistance

Development used AI coding assistants: Meta's Muse Code (with Muse Wonder Full) and
Anthropic's Claude. Commits where Claude contributed carry a `Co-Authored-By: Claude …`
trailer, and the briefs and reports of the headless Claude runs are kept in
`docs/reviews/` (index: `docs/reviews/README.md`). All inference inside the product uses
Meta's Model API only (`muse-wonder-1.0`); the coding assistants are not part of the
product and never appear in receipts. The owner reviews and authorizes every change.

## What works when Hyphae is offline

The event log, state machine, orchestration, receipts, and human-authorization recording
are fully operational without Hyphae. When the memory backend is unreachable, evidence
retrieval fails per claim and an `evidence_unavailable` event records the reason. No
evidence is retrieved, so no verification record exists at all, and with no checked
evidence the policy can never return `supported`: the run ends `insufficient_evidence`,
`contested` (unresolved Critic items), or `abstained`, depending on the Evaluator.
Uncertainty is surfaced, never converted into confidence. This per-claim path applies once
a run has started. With `memory=hyphae`, an operator run whose Hyphae MCP process cannot be
started or connected answers HTTP 502 and records no run at all (corrected 2026-10-27,
audit finding DOC-10). The public live mode reads only the sealed snapshot, never a Hyphae
daemon: a snapshot that does not boot leaves the portal up with live mode unavailable, and
a changed snapshot is refused at admission before any model call (ADR-0007, ADR-0008).

## Safety invariants (enforced in code, covered by tests)

- No self-approval: only a human actor can record an authorization decision. The operator
  server requires its bearer token on every `/api` route; on the public server a visitor can
  decide only on their own runs, and that decision is recorded with
  `authority: public_demo` (no operational authority). A typed name is a label, not
  authentication.
- No execution before approval: the executor is deferred; approval only records the
  decision.
- Bounded debate and cost: at most 2 revision rounds plus a no-progress exit that stops
  revising when no previously open objection was resolved in the last round (set-based
  since protocol 1.3, with run-unique item ids; schema 1.2 runs used a count-based rule,
  audit finding RT-7). A revision may also follow an abstention that still lists
  unresolved items. At most 9 model calls, 20 distinct evidence records, and
  `CONSENSUS_MAX_RUN_TOKENS` (default 150,000) input + output tokens per run (public live
  runs use `CONSENSUS_LIVE_RUN_TOKEN_CAP`, default 70,000), enforced by the runtime after each call, so a run can overshoot
  by one call; at most 12 claims per proposal from the Meta adapter. A run that crosses
  the token cap fails with stage `token_budget`.
- Tamper-evident history: events are SHA-256 hash-chained and verified on open; replay
  recomputes every recommendation recorded under schema 1.1 or later and rejects a forged
  one. Each receipt carries `head_hash`, the hash of the run's last event at receipt time.
  Runs started by the servers have one log each plus a hash-chained run index (ADR-0005),
  so one run's receipt verifies on its own with `consensus-api verify`. The archived
  multi-run logs in `research/` keep their log-wide chain: there, verifying one run's
  receipt needs the whole file, and edits after the run's last event are outside that
  receipt (audit findings DOC-9 and RT-4). The chain is unsigned, so it is evident, not
  proof, against a full rewrite.
- Replay never repeats external effects: replay is read-only state reconstruction.
- Retrieved memory and other models' outputs are untrusted data, never instructions, and
  no model output can authorize. In prompts, the task, the retrieved evidence (as a JSON
  array), the objections and the evaluation under review sit in separate untrusted blocks;
  delimiter neutralization is case-, whitespace- and fullwidth-insensitive, evidence text
  is truncated, and every invocation records the digests of the prompt and the schema it
  sent (ADR-0002 note; audit findings MA-6 and MA-7). That is a mitigation, not a
  guarantee. The scripted injection test is a *policy* test; prompt-level behavior was
  observed live (`docs/evaluation.md`).
- Agents receive read-only memory access by contract; writes are a separate
  runtime-service trait (`MemoryAdmin`) that agent-facing code cannot reach.

## Workspace layout

| Path | Responsibility |
|---|---|
| `crates/consensus-domain` | Typed contracts only; no I/O |
| `crates/consensus-runtime` | State machine, event log, per-run store, orchestration, policy, ports, fakes |
| `crates/consensus-api` | Meta + Hyphae adapters, sealed-snapshot client, public API v1 and operator console (Axum), live-mode admission (`live/`), `verify` and `ledger` CLIs, scripted CLI demo |
| `crates/consensus-tests` | Deterministic integration and adversarial scenarios; replay of the `research/` archive; the scripted conformance check |
| `apps/web` | Mission Control: Next.js static export served by `serve-public` (EN/ES/PT-BR/FR, light/dark) |
| `contracts/api-v1` | Generated JSON Schemas and fixtures of the public API; a drift test keeps the server equal to them |
| `schemas/` | JSON Schemas of the domain contracts (`cargo run -p consensus-domain --example write_schemas`) |
| `research/` | Verbatim archive of the recorded runs, with `MANIFEST.json` and `SHA256SUMS` |
| `deploy/` | systemd units, cloudflared template, environment templates and operator scripts for the planned server (`deploy/README.md`) |
| `scripts/` | Hyphae container scripts, battery driver and table, snapshot and research manifest tools, Linux gates, third-party notices |
| `demo-repo/` | Synthetic demo repository (the payment-client fixture) |
| `docs/` | ADRs, architecture, threat model, release checklist, runbook, evaluation, journal, reviews |

## License and notices

This repository is licensed under the Apache License 2.0 (`LICENSE`), copyright 2026
Mario Gutiérrez; see `NOTICE`. The license covers this repository only: Hyphae is not
relicensed. It keeps its own license and notices, it is consumed only via its public
interfaces, and no Hyphae code is redistributed here. "Hyphae" is a trademark of Celiums
Solutions LLC, used in the name "Hyphae Consensus" with its permission (`NOTICE`).
Third-party dependencies keep their own licenses (`THIRD_PARTY_NOTICES.md`, generated by
`scripts/third_party_notices.py`).
