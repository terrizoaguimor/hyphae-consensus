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

> **Status (2026-10-27, owner-decision round; schema 1.2; corrected the same day
> after the independent audit).**
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
- Operator console (Axum + SSE replay), synthetic demo repository (`demo-repo/`),
  adversarial integration tests, a scripted protocol conformance check, and a live
  Meta + Hyphae battery (`docs/evaluation.md`).

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
once a run has started. With `memory=hyphae`, a console run whose Hyphae MCP process
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
| `crates/consensus-api` | Meta + Hyphae adapters, Axum operator console (with an SSE replay endpoint), scripted CLI demo |
| `crates/consensus-tests` | Deterministic integration and adversarial scenarios |

Architecture decisions: `docs/adr/0001-m1-architecture.md`. Threat model:
`docs/threat-model.md`. Implementation journal: `docs/journal.md`. Reviews, agent
briefs and the 2026-10-27 independent audit: `docs/reviews/README.md`.

## Setup and verification (M1)

Prerequisites: Rust 1.99.1 via [rustup](https://rust-lang.org/tools/install) with the
MSVC build tools on Windows (see the
[rustup book](https://rust-lang.github.io/rustup/installation/windows-msvc.html)).
The toolchain is pinned in `rust-toolchain.toml`.

```powershell
cargo test --workspace                                        # 107 tests (+2 ignored live), deterministic
cargo run -p consensus-domain --example write_schemas         # regenerate schemas/ (1.2)
cargo run -p consensus-tests --example evaluate               # scripted protocol conformance check
$env:CONSENSUS_EVENT_LOG = "$env:TEMP\hc-console.jsonl"       # a fresh log, never one in demo-run/ or research/
cargo run -p consensus-api -- serve                           # operator console → http://127.0.0.1:8787/
```

The console prints a per-process **operator token** at startup; the decision panel
needs it to record an approval or rejection, and approving a `blocked` or `abstained`
run additionally requires a note. Live modes (console `model=meta` / `memory=hyphae`,
and the gated tests) read credentials from the environment — see `.env.example` and
`scripts/hyphae-container/README.md`. Without them the API returns an explicit
configuration error instead of silently falling back; `model=fake` with a non-fake
memory is rejected rather than ignored. However, `model` and `memory` default to
`fake` when omitted, and unknown request fields are ignored, so a request with a
misspelled field (for example `memroy`) runs the live Meta model against simulated
proofs (audit finding UI-5).

The scripted CLI demo is the same binary without `serve`: with no arguments it runs
the reference scenario to `AwaitingHumanApproval`, and `--approve` or `--reject`
also records a decision (no execution occurs). It always uses `demo-run/events.jsonl`
under the current working directory, and replays the first run it finds there
instead of calling models again. Run it from an empty scratch directory: build once
with `cargo build -p consensus-api`, then, from that directory, start the
repository's `target\debug\consensus-api.exe` by its full path. For a fresh run,
use a new empty directory.

**Never delete `demo-run/`.** The git-ignored `demo-run/` in the repository root
holds the original live research data behind `docs/evaluation.md`: the battery
logs, the run records with their receipts, the raw model drafts and the Hyphae
recovery files. `research/` holds byte-identical copies of its event logs and run
records (`research/README.md`); the raw drafts and the recovery files exist only in
`demo-run/`. Never point the console or the CLI demo at a log in either place:
`EventLog::open` opens a log read-write and can repair a torn tail, and the console
appends new runs, decisions and startup-recovery events. The console's default
log, `demo-run/events.jsonl`, already holds the 2026-10-26 evening runs of
`docs/evaluation.md` §4, so set `CONSENSUS_EVENT_LOG` to a fresh file before
`serve`, as above; the file and its folder are created on first use.

No credentials are needed for M1. From M2 onward, copy `.env.example` to your
environment configuration; secrets must come from environment variables, never files
committed to this repository.

## Safety invariants (enforced in code, covered by tests)

- No self-approval: only a human actor can record an authorization decision, and the
  console requires the operator token printed at startup (a typed name is a label, not
  authentication).
- No execution before approval: the executor is deferred through M3; approval currently
  only records the decision.
- Bounded debate and cost: at most 2 revision rounds plus a no-progress exit that
  stops revising when the number of unresolved Critic items does not drop from one
  round to the next (corrected 2026-10-27: earlier text said the exit was measured
  per item; it compares counts, and item ids are new every round, audit finding
  RT-7). A revision may also follow an abstention that still lists unresolved
  items. At most 9 model calls, 20 distinct evidence records, and
  `CONSENSUS_MAX_RUN_TOKENS` (default 150,000) input + output tokens per run, enforced
  by the runtime after each call, so a run can overshoot by one call; at most 12
  claims per proposal from the Meta adapter. A run that crosses the token cap fails
  with stage `token_budget`.
- Tamper-evident history: events are SHA-256 hash-chained and verified on open; replay
  recomputes every recommendation recorded under schema 1.1 or later and rejects a
  forged one. Each receipt carries `head_hash`, the hash of the run's last event at
  receipt time. The chain is log-wide: every record links to the previous record in
  the file, whichever run it belongs to, so verifying one run's receipt needs the
  whole log file, and edits after the run's last event are outside that receipt
  (corrected 2026-10-27, audit findings DOC-9 and RT-4). The chain is unsigned, so it
  is evident, not proof, against a full rewrite.
- Replay never repeats external effects: replay is read-only state reconstruction.
- Retrieved memory and other models' outputs are untrusted data, never instructions,
  and no model output can authorize. In prompts, retrieved evidence is delimited,
  escaped, and truncated to 2,000 characters, and the proposal is delimited and
  escaped; Critic item descriptions and Evaluator notes are escaped but sit outside
  the delimited blocks, and the escaping matches exact-case delimiters only
  (corrected 2026-10-27, audit findings MA-6 and MA-7). That is a mitigation, not a
  guarantee. The scripted injection test is a *policy* test; prompt-level behavior
  was observed live (`docs/evaluation.md`).
- Agents receive read-only memory access by contract; writes are a separate
  runtime-service trait (`MemoryAdmin`) that agent-facing code cannot reach.
