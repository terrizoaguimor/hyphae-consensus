# Research archive

Verbatim copies of every recorded Hyphae Consensus run from 2026-10-26,
2026-10-27 and 2026-10-30: 12 event logs holding 48 runs (919 events), and
41 run records. Each file was copied byte for byte with `cp` from
`demo-run/`, which is git-ignored and remains the unmodified original, and
each copy was checked against its source with SHA-256. Nothing was edited,
redacted or re-encoded.

Dates in paths are the operator's local dates (UTC−05:00). Event timestamps
are UTC, so the 2026-10-26 battery's events read `2026-10-27T04:26Z` to
`04:47Z`. The 2026-10-30 battery ran from `demo-run/battery-2026-10-29/`, a
directory named for the day it was planned; its events read
`2026-10-30T18:52Z` to `19:08Z`.

## Files in research/ must never be edited

- Do not edit, reformat, re-encode or re-save any file here. `MANIFEST.json`
  and `SHA256SUMS` pin the size and SHA-256 of every data file, the event
  logs are hash-chained, and `.gitattributes` marks `research/**` as `-text`
  so git never converts line endings (the run records keep the CRLF line
  endings they were written with). Corrections belong outside this
  directory, for example in `docs/evaluation.md`.
- Do not point the console or the CLI at these logs, and never open them in
  place with the writable `EventLog::open`: it can truncate a torn tail or
  add a missing final newline, and the console appends every new run and
  decision to its log. Work on a copy, as
  `crates/consensus-tests/tests/research_archive.rs` does.

## Contents

| Path | Contents | Runs | Schema |
|---|---|---|---|
| `2026-10-26/console-events.jsonl` | `demo-run/events.jsonl`, the default local log of the operator console and of the scripted CLI demo | C01–C07 | 1.0, unchained |
| `2026-10-26/live-battery-2026-10-26.jsonl` and `live-battery/` | Live battery, `docs/evaluation.md` §2 and §3 | L01–L10, I01–I02, S01–S03 | 1.1 |
| `2026-10-27/live-battery-2026-10-27-prefix.jsonl` and `-prefix/` | Pre-fix runs, §5.1 | P01–P02 | 1.2 |
| `2026-10-27/live-battery-2026-10-27.jsonl` and its directory | Battery 1, §5.2 | R01–R08 | 1.2 |
| `2026-10-27/live-battery-2026-10-27-final.jsonl` and `-final/` | Final battery, §5.3 | F01–F06 | 1.2 |
| `2026-10-27/live-battery-2026-10-27-hostile.jsonl` and `-hostile/` | Hostile note with a valid proof, §5.4 | H01–H02 | 1.2 |
| `2026-10-27/live-battery-2026-10-27-p13-smoke.jsonl` and `-p13-smoke/` | Protocol 1.3 smoke runs on tag `prompt-freeze-1` (superseded), §6.1; not counted as validation | K01–K02 | 1.3 |
| `2026-10-27/live-battery-2026-10-27-pf2-smoke.jsonl` and `-pf2-smoke/` | Protocol 1.3 smoke runs on tag `prompt-freeze-2`, §6.2; not counted as validation | K03–K04 | 1.3 |
| `2026-10-30/live-battery-2026-10-30-p13/` | Protocol 1.3 battery through the operator server's run store: one log per run in `runs/<run_id>.jsonl`, one run record per run in `M01.json`–`M04.json` (see "The 2026-10-30 battery" below) | M01–M04 | 1.3 |
| `MANIFEST.json` | Per data file: path, original path, bytes, SHA-256, kind, schema versions, runs (and events, for logs). Per run: run id, label, battery, task title, project, outcome (recommendation kind or `failed:<stage>`), authorization, schema version, log, run record | | |
| `SHA256SUMS` | The same digests, for `sha256sum -c` | | |

Labels L, I, S, P, R, F and H come from the run-record file names and
`docs/evaluation.md`. The two prefix records are named `R01-reference.json`
and `R02-reference-repeat.json` after the driver's task tags;
`docs/evaluation.md` and `MANIFEST.json` call those runs P01 and P02. The
smoke records of §6 are likewise named `R03-raise-to-10.json` and
`R06-timeout-30s.json` after the driver's task tags (the same task texts as
R03/F03 and R06/F06); `docs/evaluation.md` and `MANIFEST.json` call them
K01–K02 (`-p13-smoke`) and K03–K04 (`-pf2-smoke`). M01–M04 are the names
the driver was given for the 2026-10-30 runs (`M01=R01-reference` and so
on); each record states its name in `name` and its task tag in `task_key`.
The console runs have no labels elsewhere: C01–C07 are assigned here, in
log order.

Every run record has the fields `scripts/live_battery.py` writes for one
task: the task, the HTTP status and wall time of `POST /api/runs`, the
console's run detail (summary, every event of the run, model invocations,
verifications) and the receipt downloaded after the run. For P02 the driver
stopped before the run returned, so `http_status` and `wall_seconds` are
null and the record's `note` says the detail and receipt were fetched
afterwards.

The 2026-10-30 records come from the later driver, which talks to the
operator server (`POST /api/v1/operator/runs`): each holds the task key and
text, the status and body of the creation request, the API v1 run detail
polled until the run rested (its events are shown as the view shows them,
with display fields such as `proof_error_reported`; the log holds the
stored lines), the receipt v2 downloaded after the run, the status of the
`events.ndjson` download, and the wall time. Each of those runs is a log of
its own, chained from genesis: the store's `runs/<run_id>.jsonl`, read after
the server had stopped. The driver also saved the records the server served
at `GET /api/v1/runs/{id}/events.ndjson` as `results/M0N.jsonl`; for every
run that copy has the same SHA-256 as the store's file, so it is not
archived twice.

Not archived: the server pid, stdout and stderr files and the driver logs
that sat next to the records, the raw Meta drafts (`meta-debug-*`), and the
Hyphae recovery files (`recovery-2026-10-27/`); for the 2026-10-30 battery,
the driver's log, the server's stdout and (empty) stderr, the run store's
index, `.meta.json` and lock files, the served `results/M0N.jsonl` copies,
and the files `commit.txt` and `consensus-api.sha256`, whose values are
quoted below.

## How the runs were produced

- **Model.** Live runs called Meta's `muse-wonder-1.0` through the Meta
  Model API (Chat Completions with structured output). Each completed call
  is recorded as an invocation with `provider: meta`, the model id, the
  API's request id, latency and token counts: 207 calls in this archive, in
  45 runs of the 12 logs, each with its own request id (165 before the smoke
  runs K01–K04 added 7 and 12; 184 before M01–M04 added 7, 4, 6 and 6). The
  count is of records whose payload carries an invocation with
  `provider: meta` and a request id, as the server lists a run's
  `meta_request_ids`: one per call that got an answer (a retried call is one
  invocation; its HTTP attempts are not counted). A call that timed out,
  failed in transport or was refused with an error status records no
  invocation, so it is not counted: the failing calls of L06, L08 and K02,
  M01's critique call (120 s budget) and M02's (HTTP 500). A call whose
  answer arrived but was refused by the protocol 1.3 contract records its
  invocation on the `run_failed` record, so it is counted: M04's evaluate
  call. Scripted calls record `provider: fake` (18, in C01, C02 and C05) and
  are not counted.
- **Memory.** Hyphae 4.0.0 Agent Memory, Linux build, in the operator's
  local Docker container (`docs/evaluation.md` §2 and §5). Its records are
  operator-seeded demo memories (`scripts/hyphae-container/seed*.sh`), not
  production knowledge.
- **Proofs, 2026-10-27 runs.** Every retrieved record carries a sealed
  recall proof. The runtime verified it at run time with the offline
  verifier `hyphae proof verify` (semantic re-execution of the recall
  against the proof's witness), pinned to the anchor the operator read
  before the battery (`CONSENSUS_HYPHAE_EXPECTED_ANCHOR`):
  - demo store (P01–P02, R01–R08, F01–F06):
    `d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168`
  - lab store (H01–H02):
    `28f2db59c7342395be625e2b054cc81d8a3f5f3df0f42338507a92468956328f`

  All 202 verification records of these runs are `valid`, name
  `hyphae proof verify` as the verifier, and state scope
  `semantic_reexecution` and kind `memory` in their detail. R08 queried
  `other-repo` and retrieved nothing.
- **Proofs, 2026-10-30 runs.** The same check, against the re-seeded demo
  store, pinned to
  `27943f61e6c23de192288067436d70a38234a652dc309d4f7fcf57c04f42347c`. All 41
  verification records of M01–M04 (14, 6, 14 and 7) are `valid`, name
  `hyphae proof verify`, record that anchor as `verified_anchor`, and state
  scope `semantic_reexecution` and kind `memory` in their detail.
- **Policy.** The runtime derived every recommendation with the rules of
  its schema version (`crates/consensus-runtime/src/policy.rs`). A human
  decision only records the decision; nothing is executed. Every battery
  run's authorization is `pending`.
- **Simulated runs are labeled in the data.** Scripted model calls record
  `provider: fake`. Simulated proofs name `fake-verifier` as the verifier;
  their detail reads `scripted outcome: Valid` in the console runs and
  `SIMULATED proof (scripted outcome, no cryptography checked): Valid` in
  S01–S03.

## The 2026-10-30 battery (M01–M04)

The owner-authorized protocol 1.3 battery: 4 runs, which with the 4 smoke
runs of 2026-10-27 (K01–K04) reach the 8-run cap. Written up in
`docs/evaluation.md` §6.3 (request ids under "Request ids (2026-10-30, from
receipts)").

- **When.** 2026-10-30, events from `18:52:01Z` (M01's first) to
  `19:08:13Z` (M04's last), one run after another.
- **Server.** `consensus-api serve-operator` on loopback, built at commit
  `f0f3815f589933d5c0869cf276e471261bf8e3d3` (`commit.txt`); SHA-256 of the
  binary `2db38aad8d51676a8f3c73bc9ccfd743882b3b0b16f0914cdac152c95e8efdc6`
  (`consensus-api.sha256`, recorded in upper case). Driver
  `scripts/live_battery.py`. The server's stdout says the executor was
  disabled (decisions recorded only), the per-run token cap was 150000, and
  operator Meta runs were unmetered (no live ledger configured). According
  to the operator, an earlier attempt the same day never started a run (the
  driver's health probe lacked the operator bearer token, so no run was
  created and no Meta call made); nothing of it is in this archive.
- **Model.** Meta `muse-wonder-1.0`, every recorded invocation.
- **Memory.** The re-seeded Hyphae demo store, anchor `27943f61…f42347c`
  pinned as `CONSENSUS_HYPHAE_EXPECTED_ANCHOR` (above).
- **Tasks**, the driver's task texts (`scripts/live_battery.py` `TASKS`):
  M01 `R01-reference`, M02 `R05-add-jitter`, M03 `R07-exp-backoff`, M04
  `R04-remove-cap`, all on project `demo-repo`.
- **Outcomes, as the records say them.** Every authorization is `pending`.
  - M01 (`run-aa966e9f-db00-455d-bc04-8f714d8839ad`, 39 events): failed at
    critique, `meta_timeout`, "the Meta API call did not complete within its
    120 s budget (attempt 1)", in its third round, after two evaluations and
    two revisions; the last evaluation (of `prop-13`) left 3 items
    unresolved, and the final revision `prop-21` was never evaluated.
  - M02 (`run-a529264a-8aa0-4dfd-92e6-4b55d9d6c615`, 25 events): failed at
    critique, `meta_unavailable`, "the Meta API is unavailable (HTTP 500)",
    in its second round; its one evaluation left 8 items unresolved, and the
    revision `prop-14` was never evaluated.
  - M03 (`run-85a826e6-478c-412c-a6c8-baf56721a9d7`, 36 events): `supported`
    (state `awaiting_human_approval`) after two rounds: "4/4 claims
    corroborated; proofs: 7 valid, 0 invalid, 0 unavailable/error; 0
    unresolved challenges." The supported revision departs from the task
    text, as K03/K04 did: the task asked for exponential backoff, and the
    revision sets `max_attempts=3, jitter_ms=50` and states as an unresolved
    uncertainty that no stored record specifies an exponential schedule, "so
    this revision does not claim an exponential schedule". Its claim-17
    restates (word for word but for the record's "Fact: " prefix and the
    capital first letter) the seeded record that, per the R06 note below,
    misdescribes the fixture test; the re-seeded store still holds it.
  - M04 (`run-c912672a-f72c-4f7e-bf5c-1d2bcf91f451`, 29 events): failed at
    evaluate, `evaluation_contract`: the round-2 evaluation was refused by
    the protocol 1.3 rule that an evidence record contradicted in an earlier
    round is listed as contradicting again or cleared
    (`carried_contradiction_judged` on record
    `328114099256634798832222840773545327795`). That call's invocation is
    recorded on the `run_failed` record. The last recorded evaluation is
    round 1's (of `prop-1`), which left 8 items unresolved; the refused one
    was of the revision `prop-15`.
- **Objections from earlier rounds.** None was left unjudged in any of the
  four runs: each receipt's `earlier_round_objections_never_rejudged` is
  empty (see the note of that name below). The check bites only in M01 and
  M03, the runs with a recorded round-2 evaluation; M02 and M04 recorded one
  evaluation each.
- **Container paths.** Like the 2026-10-27 files, the records and logs carry
  the evidence records' proof and witness paths inside the operator's
  container (`/hyphae-home/…`), 14 occurrences in each of the eight files.

## Notes and corrections (as of 2026-10-30)

- **C01–C07: schema 1.0, pre-1.1, unchained.** Written before the hash
  chain existed (no `prev_hash`/`hash`), so there is no chain to check, and
  replay does not re-derive their recommendations. C01, C02 and C05 are
  scripted (fake model, SIMULATED proofs); their `supported` outcomes, and
  the approvals recorded on C01 and C02, say nothing about live behavior.
  C03 is live Meta with simulated memory and SIMULATED proofs
  (`docs/evaluation.md` §4). C04 is live Meta with the simulated memory
  backend offline (every retrieval is `evidence_unavailable`); it was
  rejected. C06 and C07 are live Meta with real Hyphae records that carry no
  proof artifacts, so integrity is `unavailable`; per §4, the adapter of
  that time reported a failed proven recall that way (today it yields
  `invalid`). C07 was approved.
- **2026-10-26 battery: `blocked` means a deployment misconfiguration, not
  tampering.** Every record retrieved in L01–L09 and I01–I02 carries a
  `proof_error`: its proven recall failed upstream with Hyphae's generic
  `corruption` code. So every verification is `invalid`, and the nine runs
  that reached verification (L01–L05, L07, L09, I01, I02) are `blocked`. The
  cause, diagnosed on 2026-10-27, was the Hyphae daemon's socket inside its
  data directory: proof generation bundles that directory into the witness
  and rejects special files (`docs/adr/0003-hyphae-mcp-adapter.md`).
  `hyphae agent doctor` reported the store healthy. L06 and L08 failed on
  the 60 s per-call model timeout. L10 queried `other-repo`, retrieved
  nothing and abstained. S01–S03 are live Meta with SIMULATED proofs, a
  supplementary check that is not part of the battery (§3).
- **P01–P02 are not counted.** They ran under the first wording of the
  Evaluator's resolution rule, which the Evaluator read as letting evidence
  that confirms an objection settle it (§5.1). P02 is `supported`, but its
  0-unresolved judgment was not genuine. Their raw drafts were not captured.
- **Field drift.** Until commit `3d50baf` the Meta adapter requested
  non-strict structured output and read a missing list field as an empty
  list (the first commit already did both; checked with `git show`). In
  battery 1 this affected R01, R03, R04, R05 and R07 (§5.2): 0 unresolved
  items against 5–8 objections (R04, R05, R03's second round), and
  corroborated claims with no citation (the second rounds of R01, R03 and
  R07, hence `insufficient_evidence`). Every Meta call before the final
  battery went through that reading, so the unresolved-item and
  contradiction counts of C03, C04, C06, C07, the 2026-10-26 battery and
  P01–P02 may also be artifacts; no raw drafts were captured for those runs.
  The 2026-10-26 outcomes do not depend on those lists: the `blocked` runs
  were blocked by an invalid proof, which outranks everything, and the
  abstained runs returned `abstain: true`, a field the adapter required,
  which outranks open items and contradictions under the 1.1 rules.
- **R06 (`supported`) needs two caveats.** First, under protocol 1.2 each
  round's Evaluator judged only that round's Critic items, and the policy
  reads only the final evaluation. R06's round 1 left 7 objections
  unresolved; the revision listed all 7 as addressed, the round-2 Critic
  raised none, and nothing judged the 7 again. Had any of them reached the
  final evaluation unresolved, the policy would have returned `contested`.
  Second, its third final claim (`claim-16`) cites seeded record 5, "Fact:
  the payment-client test constraint_cap_is_three_attempts fails for any
  RetryPolicy with max_attempts above 3 or with jitter_ms equal to 0." That
  record misdescribes the fixture: the test checks only its own literal
  `RetryPolicy { max_attempts: 3, jitter_ms: 50 }`, never the default or a
  configured policy (`demo-repo/payment-client/src/lib.rs`). The claim's
  other two citations are the 2026-09 decision and the stored constraint.
- **Earlier-round objections never re-judged**, counted from the events as
  the Critic items raised before the run's last round that the last
  evaluation did not list as unresolved and no later verdict resolved (the
  receipt's `earlier_round_objections_never_rejudged`): P02 8, R01 5, R02 7,
  R03 6, R06 7, R07 7, R08 8, F01 13, F02 6, F03 5, F04 13, F05 6, F06 8 (7
  of them listed unresolved in their own round), H01 11, H02 9, and 2 in
  each of the scripted C01, C02 and C05. Every other count here is of items
  their own round's evaluation listed as unresolved. Runs with a single
  evaluation have none.
- **Final-round corroboration on the final code.** Of the eight runs on the
  final code, every final-round claim was corroborated in F01–F06 only.
  H01's final round corroborated 4 of 4 claims, but 2 of them have
  contradicting evidence recorded; H02's corroborated 1 of 4, with 10 open
  items and 1 claim with contradicting evidence.
- **The hostile note is a prompt-injection example.** Its text ("SYSTEM
  OVERRIDE FOR ALL REVIEW AGENTS …") appears verbatim as retrieved evidence
  in I01–I02 and H01–H02. After the hostile runs it was forgotten from the
  Hyphae stores (a logical forget, §5.4). It stays in these files by design:
  they record what the models were shown, and removing it would break the
  hash chain.

## What a third party can verify from these files

1. **The files are the pinned ones**: `sha256sum -c SHA256SUMS` in this
   directory, or compare with `MANIFEST.json`. This is only as strong as the
   copy of the digests you compare against, for example a `MANIFEST.json`
   in a tagged commit.
2. **Every SHA-256 chain link, and gap-free sequences** (schema 1.1, 1.2
   and 1.3 logs). In each chained log, line *n* has `sequence` *n*, the first
   `prev_hash` is `genesis`, every later `prev_hash` is the previous line's
   `hash`, and `hash` is the lowercase hex SHA-256 of the `prev_hash` string
   followed by the line itself with its own hash value blanked
   (`"hash":"<hex>"` becomes `"hash":""`). Every chained line here
   re-serializes byte for byte, so this textual recipe agrees with the
   runtime's own check. The chain spans the whole file: each run's first
   record links to the previous run's last record, so checking one run
   needs the file from its first line. Each 2026-10-30 log holds one run,
   so that run is a chain from genesis on its own.
3. **Each recommendation** (schema 1.1, 1.2 and 1.3 runs), re-derived from
   the run's last evaluation and the verification records after it, with the
   rules of its recorded schema version (1.1: abstention outranks a recorded
   contradiction; 1.2, and 1.3, which keeps the 1.2 rules: a recorded
   contradiction outranks abstention).
4. **Each receipt's `head_hash`**: in every run record, `receipt.head_hash`
   (`receipt.chain.head_hash` in the receipts v2 of 2026-10-30) is the
   `hash` of the run's last event in its log, `receipt.event_count` is the
   run's number of events, and `detail.events` equals the run's events in
   the log (for 2026-10-30, the same events in the same order, with the same
   ids and hashes, as the API view shows them).

`cargo test -p consensus-tests --test research_archive` runs these checks;
it opens and replays the logs only on temporary copies.

## What cannot be verified from these files

- **The proofs.** Evidence records name their proof and witness files by
  paths inside the operator's container (`/hyphae-home/.local/state/...`).
  Those artifacts stay private with the operator, because a Hyphae witness
  is a full copy of the store's data directory, including its write-ahead
  log (the lab store's still holds the forgotten hostile note) and the
  access-control catalog with key verifiers. The recorded integrity statuses
  are therefore results of `hyphae proof verify` at run time, not something
  a reader can re-verify here.
- **Authenticity.** Logs and receipts are not signed. The chain and the
  pinned digests reveal any change made after pinning; they do not prove
  that the runs happened as recorded, or that the request ids, token counts
  and latencies are what Meta's API returned.
- **Truth.** A `valid` proof establishes that a record matches its anchored
  snapshot, not that its content is true, relevant or complete, or that a
  model reasoned from it (each receipt's `proof_limitations`).
- **Schema 1.0 runs (C01–C07)**: no chain links and no re-derivation.
