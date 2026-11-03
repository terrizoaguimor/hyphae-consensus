# What is verified and what is not

The [README](../README.md) has a summary of this page.

## Integrity proofs: what they do and do not establish

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

## Published snapshot digests

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
capture recorded for the container's binary; on 2026-11-01, during the deploy, it was compared
with the binary extracted from the release tarball (checked against `cf92ed6d…`) and matched
(`docs/deployment/runbook.md` section 3.2; the owner's deploy log, outside the repository). The proofs and witnesses are not published
(ADR-0008 decision 3), so a clone cannot re-run the verifier, and the committed directory
alone does not boot (`checksums_incomplete`). The release tag is to carry the same three
values. The sealed snapshot has been on the production server since 2026-11-01.

## SIMULATED, RECORDED and LIVE

Every run carries provenance axes derived from its records (model, evidence, delivery),
and the portal labels them so that a scripted or replayed run is never shown as live
(release gate G7, no recorded PASS: enforced in the API and the web workspace, and the
Playwright suite asserts it over a mocked API, but no run through the public hostname is
recorded; `apps/web/README.md`):

- **SIMULATED**: scripted model doubles (`provider: fake`) and scripted proofs
  (`fake-verifier`, "SIMULATED proof"). The guided runs are simulated end to end; their
  outcomes say nothing about live model behavior.
- **RECORDED**: a run of the `research/` archive, shown read-only. A recorded live-model
  run reads META MODEL with its recorded date, never LIVE.
- **LIVE**: only frames delivered live from a live Meta run in progress; LIVE META is
  shown next to its evidence label (SEALED SNAPSHOT), never alone. A finished run is
  Playback.

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
  unresolved items. At most 9 role calls plus at most 2 bounded re-asks per run (the
  Meta adapter asks again, once per role call, only an answer with no usable claim or one
  it could not read, never a judgment; [evaluation.md §6.4](evaluation.md)), 20 distinct
  evidence records, and
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
