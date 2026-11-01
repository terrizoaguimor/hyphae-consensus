# Submission draft (Cerebral Valley form)

Text to paste into the submission form of the Meta AI Developer Hackathon, Indie track.
Written 2026-10-31 against branch `feat/public-portal` at `a657531`. The form's exact
fields are only visible with the owner signed in, so the sections below follow the
judging criteria of the Official Rules (Impact 30 %, Use of Meta AI Tooling 25 %,
Technical Depth & Execution 20 %, Demo 25 %); cut or merge them to fit the fields.
Every claim names the file where a judge can check it. Replace `[VIDEO_URL]`,
`[DEMO_URL]` (or drop it, see "Links") and `[REPO_URL]` before submitting; keep the wording of the figures as is
(they are copied from `docs/evaluation.md`, never rounded up).

## Name and tagline

**Hyphae Consensus** — AI agents argue over a code change against verified project
memory; a fixed policy, not a vote, recommends; only a human decides.

## Links

- Repository: [REPO_URL]
- Demo video: [VIDEO_URL]
- Live project: [DEMO_URL] (guided runs are SIMULATED and labelled so; research runs are
  RECORDED; the public live mode is off). Conditional: the release checklist exposes
  nothing until every gate G1–G12 reads PASS, so this link exists only if that happens or
  the owner records the decision to expose a replay-only portal earlier
  (`docs/roadmap.md`, "Remaining for tomorrow"); otherwise drop this line.

## Short description

Coding agents increasingly propose changes on their own. Hyphae Consensus puts three
Meta-model roles between the proposal and anything happening: a Proposer states explicit
claims, a Critic attacks them as individually tracked objections, and an Evidence
Evaluator judges every claim against project records retrieved from Hyphae Agent Memory,
whose integrity proofs the runtime verifies offline. A fixed, deterministic policy, not a
vote and not a model, turns those judgments into one of `supported`, `contested`,
`insufficient_evidence`, `abstained` or `blocked`, with a trace of the rule that decided.
The run then stops for a human; nothing is executed. Every step is written to a
hash-chained event log that anyone can replay. Mission Control, the web portal, shows each
run with its provenance (SIMULATED, RECORDED or LIVE), the evidence, the proofs, the
policy trace, the Meta API calls and the audit trail, in English, Spanish, Portuguese and
French.

## Impact

**Who it is for.** Teams that let coding agents propose changes to real code and need to
know, before approving, whether the change contradicts what the project already decided,
and why the system recommends what it recommends.

**What it prevents, in one recorded run.** In run F03 (`docs/evaluation.md` §5.3; run
record `research/2026-10-27/live-battery-2026-10-27-final/F03-raise-to-10.json`) the task
was "Raise payment retry attempts to 10". The Proposer claimed that 10 attempts
"complies with any documented maximum retry limit". The project's stored constraint caps
retries at 3 attempts with jitter, and the Evaluator recorded 2 of the 4 claims as
contradicted by records whose proofs verified. The revision proposed the recorded fix
instead (3 attempts, jitter of at least 50 ms), and the run still ended `contested`
because the Critic kept 6 objections open (3 missing tests, 2 unsupported claims, 1
security risk). The decision went to a human with all of that on screen.

**Across the 2026-10-27 final battery** (F01–F06, schema 1.2, same section): every first
proposal had 1 to 3 claims contradicted by stored records; each revision dropped or fixed
them; no run ended `supported`, and in every final round every claim was corroborated by
verified records it cites, so what remained was the Critic's open objections, mostly
missing tests.

**Under protocol 1.3** (M01–M04, 2026-10-30, `docs/evaluation.md` §6.3, the latest
counted battery): 2 of the 4 runs were lost to provider errors (M01: a call past the
battery's 120 s budget; M02: Meta HTTP 500), M04 failed closed (the evaluation contract
refused an evaluation that left a carried contradiction unjudged), and M03, the only
`supported` run, is `supported` for a revision that does not claim the exponential
backoff the task asked for (as were the uncounted smoke runs K03 and K04, §6.2). With 2 of 4 runs
lost, no rate of any outcome is claimed.

**Why a human still decides.** The system never authorizes: `supported` attests that the
claims are corroborated by verified records, not that the task was done, and even a
`supported` recommendation (M03) waits for a human. A contradiction outranks an
abstention, an invalid proof blocks, and an objection stays open until the Evaluator
judges it (`README.md`, "What is verified and what is not"; ADR-0010). A planted "SYSTEM OVERRIDE"
record with a valid proof was flagged by the Critic, recorded as contradicting evidence by
the Evaluator and never obeyed (H01–H02, §5.4; two runs, one model, one note: not a
robustness result).

**Limits, stated once.** The project records are synthetic and operator-seeded
(`demo-repo/payment-client/CONSTRAINTS.md`). No single-agent baseline was run, so no
comparative rate is claimed. A valid proof attests integrity, not truth.

## Use of Meta AI tooling

- **Model and endpoint.** `muse-wonder-1.0` (`DEFAULT_META_MODEL` in
  `crates/consensus-api/src/config.rs`) through the Meta Model API's Chat Completions,
  `POST https://api.meta.ai/v1/chat/completions` (`crates/consensus-api/src/meta.rs`,
  ADR-0002).
- **Three roles, four strict schemas.** Proposer (`DraftProposal`), Critic
  (`DraftChallenge`), Evidence Evaluator (`DraftEvaluation`) and the Proposer's revision
  (`DraftRevision`), each with `response_format: {type: "json_schema", json_schema:
  {name, schema, strict: true}}` (`meta.rs`).
- **A schema generated per call.** The Evaluator's schema is built for each call with
  exactly one required property per claim id of the proposal and one per open objection,
  and no other property allowed, so an omitted, repeated or unknown id is malformed output,
  never a silent default (tag `prompt-freeze-2`; `docs/evaluation.md` §6.2). This closed
  two fail-open paths an independent audit found (MA-1, RT-1; ADR-0010).
- **Measured and recorded.** Every recorded Meta call keeps the API's request id
  (`docs/evaluation.md`, "Request ids"); since protocol 1.3 each invocation also records
  the SHA-256 of the schema and of the prompt template it sent, `finish_reason`, the attempt
  count and the reasoning tokens. The protocol 1.3 battery made 25 Meta calls (23
  answered, 0 retries; the 2 unanswered ones, a call past its 120 s budget and an HTTP 500,
  cost M01 and M02 their runs), 37,363 input / 95,850 output tokens (§6.3). Measured cost on the
  final code of 2026-10-27: $0.81 mean, $0.98 maximum per run (ADR-0007).
- **Bounded.** `max_completion_tokens` 16,000; one time budget per call shared by up to 3
  attempts; only 429, 503 and transport failures retried (a request that may have been
  billed, at most once); a per-run token cap; at most 9 model calls per run
  (`README.md`, "How this project uses Meta's API").
- **Built with Meta's coding tools too.** Development used Meta's Muse Code (with Muse
  Wonder Full) alongside Anthropic's Claude; inside the product, every inference is
  `muse-wonder-1.0` on the Meta Model API (`README.md`, "AI assistance").

## Technical depth and execution

- **Protocol 1.3** (ADR-0010): a fail-closed evaluation contract enforced by the runtime
  for every model client (one assessment per claim, cited ids drawn from the retrieved
  set, an explicit verdict for every open objection) and cross-round carry-forward of
  objections and contradictions. In the live run M04 the contract refused an evaluation
  that left a carried contradiction unjudged, as specified (§6.3).
- **Deterministic policy with a trace**: `PolicyTrace` shows which rule decided
  (`crates/consensus-domain`).
- **Tamper-evident history**: a SHA-256 hash-chained, crash-safe, append-only event log
  per run; replay recomputes every recommendation and rejects a forged one;
  `consensus-api verify <log>` checks a log read-only. The chain is not signed.
- **Verified evidence**: Hyphae recall proofs re-executed offline with
  `hyphae proof verify` against a pinned anchor; a sealed evidence snapshot for the public
  mode, with its digests published in the README (ADR-0008).
- **A public surface built to be exposed**: typed API v1 with a drift-tested contract
  (`contracts/api-v1/`), a two-phase live-mode gate, a USD ledger with daily and lifetime
  caps, a threat model v2 mapping each threat to code and tests (`docs/threat-model.md`),
  and twelve release gates (`docs/security/release-checklist.md`).
- **Tests at `a657531`** (2026-10-31, Windows): `cargo test --workspace --locked` 603
  passed / 0 failed / 2 ignored (the 2 live tests); the scripted protocol conformance
  check 10/10 (`cargo run -p consensus-tests --example evaluate`); Vitest 904 passed / 16
  skipped in 42 files; 501 Playwright end-to-end tests in Chromium (desktop and mobile),
  with a smoke subset in Firefox and WebKit (500 passed in
  the full run; the one failure, a browser-context teardown past the 120 s timeout under
  load, passed on a re-run), with 340 axe accessibility scans and 0 violations.
- **Verbatim research archive**: 12 event logs with 48 runs (919 events) and 41 run
  records, byte-identical to the originals and pinned by `research/MANIFEST.json` and
  `research/SHA256SUMS` (`research/README.md`).

## Demo

- **Video**: [VIDEO_URL]. Recorded on the frozen UI; scripted runs are labelled
  SIMULATED, archived runs RECORDED, and nothing recorded is presented as running live.
- **Live project**: [DEMO_URL] (conditional, see "Links").
- **Run it locally** without any credential and without a Meta call (README, "For
  judges"): two `in-docker.sh` commands build the web export, and
  `cargo run -p consensus-api --locked -- serve-public` serves it with the guided runs and
  the research showcase.
- **What to click**: New deliberation → a guided scenario → the run's Decision tab (the
  outcome, the policy trace, the human decision); the Research showcase → F03 → Evidence,
  Proof inspector, Audit trail (the hash chain checked in the browser) and Meta API (the
  recorded calls with their request ids).

## Built with

- **Meta products**: Muse Wonder (`muse-wonder-1.0`) via the Meta Model API, Chat
  Completions with strict structured output; Muse Code during development.
- **Pre-existing, not claimed as new work**: Hyphae Core and Hyphae Agent Memory 4.0.0,
  the owner's own earlier open-source project (Apache-2.0), frozen at `418a3da` before the
  hackathon and used only through its public interfaces (`README.md`, "Competition
  boundary").
- **Backend**: Rust 1.99.1 (Tokio, Axum, reqwest, serde, schemars, sha2).
- **Frontend**: Next.js 16 static export, React 19, next-intl (4 locales), zod, Astryx
  design system.
- **Tests and tooling**: cargo test, Vitest, Playwright with axe-core, Docker for the
  Node toolchain and Hyphae, GitHub Actions (`.github/workflows/ci.yml`).
- **License**: Apache-2.0 (`LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES.md`).
