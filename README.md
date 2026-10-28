# Hyphae Consensus

**Verifiable Multi-Agent Reasoning and Decision Accountability**

Meta Global AI Developer Hackathon 2026 · Indie Developer track · Push the Frontier
Development window: October 26 – November 4, 2026 · Owner: Mario (independent developer)

Independent project; not affiliated with, sponsored by, or endorsed by Meta.

Hyphae Consensus coordinates three Meta-AI agent roles — **Proposer**, **Critic**, and
**Evidence Evaluator** — that propose solutions to software-engineering problems,
challenge each other's assumptions, and revise conclusions. The **runtime** (not a
model) retrieves evidence from a verifiable memory backend and checks its integrity
proofs; the **Evaluator** judges relevance and factual corroboration per claim; a fixed,
deterministic runtime policy turns those judgments and the proof statuses into an
auditable recommendation; and only a **human** authorizes. No model writes the outcome
or can approve, but the Evaluator's judgments are the policy's main input, so "models
never decide" holds in form, not in substance. Nothing is executed before a human
approves (the executor is deferred: approval records a decision only).

> **Status (2026-10-28 evening, branch `feat/public-portal`; nothing exposed yet).**
> The public live mode is wired end to end in code and stays off until its gates pass.
> It runs only over a sealed Hyphae snapshot whose manifest, record texts and verifier
> binary are pinned and re-checked at every admission (a snapshot that fails disables
> live mode, not the portal), behind a two-phase gate: geography, the adult attestation,
> the evidence status and Turnstile before the single live slot is taken, then a daily
> quota per network and a USD ledger reservation of the run's worst case, with every
> Meta HTTP attempt metered. On the server the operator console is metered on the same
> ledger, and `consensus-api ledger` lets the operator read, reset and reconcile it from
> a shell. Receipts state what a valid proof does not establish (a record's text is not
> byte-compared with the proof). The threat model v2 (`docs/threat-model.md`) maps each
> threat to its code and tests and lists what is still open; of the release gates G1–G12,
> the Linux correctness gate (G8) passes, supply chain (G12) is partial and the rest are
> pending. The web portal has its typed API client, the live stream, New deliberation, My
> runs and the run workspace's Live/Playback, Explorer and Decision views; the Evidence,
> Proof, Audit and Meta API views are being built. Still ahead: the snapshot capture and
> the counted protocol 1.3 battery (2026-10-29), the staging deployment and the video.
>
> **Status (2026-10-27 night, branch `feat/public-portal`; nothing exposed yet).**
> Since the audit below: protocol 1.3 (ADR-0010) closes MA-1 and RT-1. The runtime
> validates every evaluation (one assessment per claim, one explicit verdict per open
> objection, objections and contradictions carried across rounds) and the Meta
> adapter's per-call keyed schema makes an omitted id impossible. Receipts v2 carry a
> deterministic `PolicyTrace`. Runs now have one hash-chained log each plus a run index,
> start asynchronously and stream live over SSE (ADR-0005). A public API v1 with visitor
> sessions, owner-scoped `404`s and a listener split exists (ADR-0006), as does the
> live-mode admission library (USD ledger, Turnstile, presets, geo; ADR-0007), wired into
> the public route with the sealed-snapshot client (ADR-0008) as its evidence factory,
> the public snapshot itself still to be captured; the Meta and Hyphae adapters are hardened
> (ADR-0002 note). Two smoke runs on tag `prompt-freeze-2` (K03, K04;
> `docs/evaluation.md` §6.2) confirmed the keyed schema live; both ended `supported`
> **for a revised proposal that departs from the task text**, and the counted protocol
> 1.3 battery is still pending, so no rate of any outcome under 1.3 is claimed. The web
> portal (`apps/web`) has its shell, legal and docs pages; the run views are being
> built. Nothing is deployed: the release gates G1–G12
> (`docs/security/release-checklist.md`) are the condition for exposure.
>
> **Earlier status (2026-10-27 morning, owner-decision round; schema 1.2; corrected
> the same day after the independent audit).**
> Typed contracts, deterministic state machine, hash-chained crash-safe event log,
> Meta Model API adapter (strict structured output), Hyphae MCP read-only adapter +
> offline proof verifier, Axum operator console with an SSE replay endpoint,
> synthetic demo repository, a 10-case scripted protocol conformance check, and
> live batteries. **Live status:**
> Hyphae proofs now verify end to end. `hyphae proof verify` re-executes each recall
> against its sealed witness, checked against an operator-pinned anchor. The earlier
> "corruption" answers were never data corruption: the daemon's socket sat inside
> its data directory, and later the store outgrew the 4 MiB MCP response that
> carries the witness (ADR-0003). Across 18 live runs on 2026-10-27 (real Meta API,
> `muse-wonder-1.0`, and real Hyphae memory), one counted run reached `supported`
> with every cited record verified (R06). P02 also reached `supported` but does
> not count (`docs/evaluation.md` §5.1). Two defects surfaced live and were
> fixed: the first Evaluator wording, new in this round, which let objections
> that still applied count as resolved, as in P02; and field-name drift under
> non-strict structured output, a fail-open that had existed since the first
> commit and was exposed live in this round. On the final code all 8
> runs ended `contested`. Only the 6 runs of the final battery (F01–F06) had every
> final-round claim corroborated by verified memory, with the Critic's open
> objections about missing tests and unproven risks as the remaining blocker (the
> unchanged policy grants `supported` only when none is open). In the two
> hostile-note runs, H02's final round corroborated 1 of 4 claims, and H01's
> final round had claims with contradicting evidence. A hostile note with a valid
> proof was treated as contradicting data, never as instructions
> (`docs/evaluation.md` §5). Hyphae runs
> its pinned Linux build in a local Docker container, because the Windows build's
> Agent Memory surface does not start on this host (`docs/journal.md`). Demo modes
> are labeled in the launch form and in every verification record and receipt
> (`fake-verifier`, `SIMULATED proof`), but the console's result views show a
> simulated proof with the same `valid` badge as a real one (audit finding UI-4).
>
> **Independent audit (2026-10-27).** A read-only, adversarially verified audit
> of commit `72e422a` (`docs/reviews/2026-10-27-independent-audit.md`)
> reproduced every gate and battery number above and found two fail-open paths
> toward `supported`. MA-1: Evaluator ids that are not byte-exact are silently
> dropped, which can empty the list of open objections or hide a contradiction.
> RT-1: objections and contradictions from earlier rounds are never shown or
> judged again, because the policy reads only the final round. Fixes are in
> progress on branch `feat/public-portal`. Until they land, read R06's
> `supported` with that caveat: its 7 round-1 objections were never re-judged.
> The console must stay on loopback; it is not safe to expose (audit SEC-1 to
> SEC-11).

## Competition boundary: pre-existing vs. newly developed work

This repository is a new project created for the hackathon. It consumes
pre-existing open-source technology through documented public interfaces only.

**Hyphae is the owner's own earlier work.** Hyphae Core and Hyphae Agent Memory are
Mario Gutiérrez's own earlier open-source project (Apache-2.0, copyright Celiums
Solutions LLC). It was frozen at commit `418a3da` (2026-09-24), before the hackathon
began; Hyphae's `main` had no commits after kickoff as of 2026-10-27. This project
uses Hyphae only through its public interfaces and does not claim it as new work.

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
forked, or modified by this project.

**Newly developed for this hackathon (this repository):**

- The three-role deliberation protocol and deterministic consensus state machine.
- Typed, versioned contracts for proposals, claims, challenges, evaluations,
  recommendations, human authorizations, events, and run receipts (`crates/consensus-domain`,
  JSON Schemas in `schemas/`).
- Orthogonal evidence status modeling (`integrity_status` / `factual_status` /
  `relevance_status`) and the recommendation-derivation policy.
- The append-only, crash-safe, replayable application event log.
- Meta Model API adapter (`muse-wonder-1.0`, Chat Completions structured output per
  Meta's docs) with timeouts, bounded retries, and strict server-side validation
  (`crates/consensus-api/src/meta.rs`, ADR-0002).
- Hyphae MCP adapter over `hyphae mcp --profile memory` (stdio JSON-RPC, read-only
  tools only) plus `hyphae proof verify` integrity checking
  (`crates/consensus-api/src/hyphae.rs`, ADR-0003).
- Public API v1 and operator console (Axum; asynchronous runs, live SSE, visitor
  sessions, per-run logs), the live-mode admission library, the Mission Control web
  portal (`apps/web`), synthetic demo repository (`demo-repo/`), adversarial
  integration tests, a scripted protocol conformance check, and live Meta + Hyphae
  batteries (`docs/evaluation.md`, archived verbatim in `research/`).

**License.** This repository is licensed under the Apache License 2.0 (`LICENSE`),
copyright 2026 Mario Gutiérrez; see `NOTICE`. The license covers this repository
only: Hyphae is not relicensed. It keeps its own license and notices, it is consumed
only via its public interfaces, and no Hyphae code is redistributed here.
Third-party Rust crates keep their own licenses.

## AI assistance

Development used AI coding assistants: Meta's Muse Code (with Muse Wonder Full)
and Anthropic's Claude. Commits where Claude contributed carry a
`Co-Authored-By: Claude …` trailer, and the briefs and reports of the headless
Claude runs are kept in `docs/reviews/` (index: `docs/reviews/README.md`). All
inference inside the product uses Meta's Model API only (`muse-wonder-1.0`); the
coding assistants are not part of the product and never appear in receipts. The
owner reviews and authorizes every change.

## What works when Hyphae is offline

The event log, state machine, orchestration, receipts, and human-authorization recording
are fully operational without Hyphae. When the memory backend is unreachable, evidence
retrieval fails per claim and an `evidence_unavailable` event records the reason. No
evidence is retrieved, so no verification record exists at all, and with no checked
evidence the policy can never return `supported`: the run ends `insufficient_evidence`,
`contested` (unresolved Critic items), or `abstained`, depending on the Evaluator.
Uncertainty is surfaced, never converted into confidence. This per-claim path applies
once a run has started. With `memory=hyphae`, an operator run whose Hyphae MCP process
cannot be started or connected answers HTTP 502 and records no run at all
(corrected 2026-10-27, audit finding DOC-10).

## Integrity proofs: what they do and do not establish

A valid Hyphae proof establishes that the recall it seals reproduces against its
anchored snapshot under Hyphae's documented assumptions. Hyphae's proof seals each
record's text, but this adapter does not yet check that a proof is a memory proof,
or byte-compare the record text it gives the models against the proof, so `Valid`
attests the proof Hyphae returned with the record, not that the displayed text
matches it (corrected 2026-10-27, audit finding HY-1). A valid proof does **not**
establish that the content is true, relevant, or complete, or that a model reasoned
from it. This is why `EvidenceVerification` keeps integrity, factual corroboration,
and relevance as three independent fields. A `supported` recommendation requires **every** claim to be
corroborated by at least one record that the claim itself cites, a valid proof on
every cited record, no contradicting record (cited or not), no Critic item the
Evaluator still judges unresolved, and no invalid proof anywhere in the retrieved
set. A recorded contradiction outranks an abstention (`contested`, schema 1.2), so
abstaining cannot hide it. Since protocol 1.3 (ADR-0010) the runtime enforces the
evaluation contract itself, for every model client: exactly one assessment per claim,
cited ids drawn from the run's retrieved set, an explicit verdict for every open
objection, and objections and contradictions from earlier rounds carried forward until
the Evaluator judges them (the audit's MA-1 and RT-1 findings). The Meta adapter's
per-call schema keys assessments and verdicts by id, so an omitted, repeated or unknown
id is a malformed output, never a silent default. Runs recorded before 1.3 keep their
recorded semantics; their receipts state how many earlier-round objections were never
re-judged. `Valid` means `hyphae proof verify` re-executed the
recall against its sealed witness and reproduced the result; one proof covers a
whole recall. By default the proof anchor comes from the same server response as
the proof, which shows internal consistency only; pin an out-of-band anchor with
`CONSENSUS_HYPHAE_EXPECTED_ANCHOR` to verify against a trusted one.

## Workspace layout

| Crate | Responsibility |
|---|---|
| `crates/consensus-domain` | Typed contracts only; no I/O |
| `crates/consensus-runtime` | State machine, event log, orchestration, policy, ports, fakes |
| `crates/consensus-api` | Meta + Hyphae adapters, public API v1 and operator console (Axum; run manager, live SSE, sessions, listener split), live-mode admission (`live/`), `verify` CLI, scripted CLI demo |
| `crates/consensus-tests` | Deterministic integration and adversarial scenarios; replay of the `research/` archive |
| `apps/web` | Mission Control: Next.js static export served by `serve-public` (EN/ES/PT-BR/FR, light/dark) |
| `contracts/api-v1` | Generated JSON Schemas and fixtures of the public API; a drift test keeps the server equal to them |

Architecture decisions: `docs/adr/0001-m1-architecture.md`. Threat model:
`docs/threat-model.md`. Implementation journal: `docs/journal.md`. Reviews, agent
briefs and the 2026-10-27 independent audit: `docs/reviews/README.md`.

## Setup and verification (M1)

Prerequisites: Rust 1.99.1 via [rustup](https://rust-lang.org/tools/install) with the
MSVC build tools on Windows (see the
[rustup book](https://rust-lang.github.io/rustup/installation/windows-msvc.html)).
The toolchain is pinned in `rust-toolchain.toml`.

```powershell
cargo test --workspace --locked                               # 494 tests (+2 ignored live), deterministic
cargo run -p consensus-domain --example write_schemas         # regenerate schemas/ (1.3)
cargo run -p consensus-tests --example evaluate               # scripted protocol conformance check (10/10)
cargo run -p consensus-api -- serve-operator                  # operator console → http://127.0.0.1:8787/ (alias: serve)
$env:CONSENSUS_PUBLIC_BIND = "127.0.0.1:8788"                 # development only: in production the public server listens on a unix socket
cargo run -p consensus-api -- serve-public                    # public API v1 + the static export in apps/web/out
cargo run -p consensus-api -- verify research\2026-10-27\live-battery-2026-10-27-final.jsonl   # read-only chain and policy check
```

Both servers keep one hash-chained log per run under `CONSENSUS_RUN_STORE_DIR`
(default `consensus-store/`), never a log of `demo-run/` or `research/`. The operator
server requires its **bearer token** on every route (from `CONSENSUS_OPERATOR_TOKEN_FILE`,
or generated and printed once at startup); approving a `blocked` or `abstained` run
additionally requires a note. Live modes (operator `model=meta` / `memory=hyphae`, and
the gated tests) read credentials from the environment — see `.env.example` and
`scripts/hyphae-container/README.md`. Without them the API returns an explicit
configuration error naming the variable, never a silent fallback. Request bodies are
typed: unknown fields are rejected (`422`), `model` and `memory` are enums without
defaults, `model=fake` with a non-fake memory is refused, and the public surface accepts
no free text at all (guided scenarios and curated live presets only). The public
server's own variables are documented in `crates/consensus-api/src/public_config.rs`.

The scripted CLI demo is the same binary without a subcommand: with no arguments it runs
the reference scenario to `AwaitingHumanApproval`, and `--approve` or `--reject`
also records a decision (no execution occurs). It keeps its log in
`scripted-demo/events.jsonl` under the current working directory (git-ignored) and
replays the run it finds there instead of re-running; for a fresh run, delete
`scripted-demo/` and nothing else. It refuses to run inside `demo-run/` or
`research/`, and any other argument (`--help` included) prints the usage instead of
starting it.

**Never delete `demo-run/`.** The git-ignored `demo-run/` in the repository root
holds the original live research data behind `docs/evaluation.md`: the battery
logs, the run records with their receipts, the raw model drafts and the Hyphae
recovery files. `research/` holds byte-identical copies of its event logs and run
records (`research/README.md`); the raw drafts and the recovery files exist only in
`demo-run/`. Never open a log in either place with a writing tool: `EventLog::open`
opens a log read-write and can repair a torn tail. Earlier builds kept the CLI demo's
log in `demo-run/events.jsonl`, which therefore holds the 2026-10-26 evening runs of
`docs/evaluation.md` §4; the demo no longer opens it. The servers use the per-run store
and open archived logs read-only (`consensus-api verify`, the showcase).

No credentials are needed for M1. From M2 onward, copy `.env.example` to your
environment configuration; secrets must come from environment variables, never files
committed to this repository.

## Safety invariants (enforced in code, covered by tests)

- No self-approval: only a human actor can record an authorization decision. The
  operator server requires its bearer token on every route; on the public server a
  visitor can decide only on their own simulated runs, and that decision is recorded
  with `authority: public_demo` (no operational authority). A typed name is a label,
  not authentication.
- No execution before approval: the executor is deferred through M3; approval currently
  only records the decision.
- Bounded debate and cost: at most 2 revision rounds plus a no-progress exit that
  stops revising when no previously open objection was resolved in the last round
  (set-based since protocol 1.3, with run-unique item ids; schema 1.2 runs used a
  count-based rule, audit finding RT-7). A revision may also follow an abstention
  that still lists unresolved items. At most 9 model calls, 20 distinct evidence records, and
  `CONSENSUS_MAX_RUN_TOKENS` (default 150,000) input + output tokens per run, enforced
  by the runtime after each call, so a run can overshoot by one call; at most 12
  claims per proposal from the Meta adapter. A run that crosses the token cap fails
  with stage `token_budget`.
- Tamper-evident history: events are SHA-256 hash-chained and verified on open; replay
  recomputes every recommendation recorded under schema 1.1 or later and rejects a
  forged one. Each receipt carries `head_hash`, the hash of the run's last event at
  receipt time. Runs started by the servers have one log each plus a hash-chained run
  index (ADR-0005), so one run's receipt verifies on its own with `consensus-api
  verify`. The archived multi-run logs in `research/` keep their log-wide chain: there,
  verifying one run's receipt needs the whole file, and edits after the run's last
  event are outside that receipt (audit findings DOC-9 and RT-4). The chain is
  unsigned, so it is evident, not proof, against a full rewrite.
- Replay never repeats external effects: replay is read-only state reconstruction.
- Retrieved memory and other models' outputs are untrusted data, never instructions,
  and no model output can authorize. In prompts, the task, the retrieved evidence (as a
  JSON array), the objections and the evaluation under review sit in separate
  untrusted blocks; delimiter neutralization is case-, whitespace- and
  fullwidth-insensitive, evidence text is truncated, and every invocation records the
  digests of the prompt and the schema it sent (ADR-0002 note; audit findings MA-6 and
  MA-7). That is a mitigation, not a guarantee. The scripted injection test is a *policy* test; prompt-level behavior
  was observed live (`docs/evaluation.md`).
- Agents receive read-only memory access by contract; writes are a separate
  runtime-service trait (`MemoryAdmin`) that agent-facing code cannot reach.
