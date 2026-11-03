# Evaluation

This directory is the entry point to the evaluation of Hyphae Consensus: where the harness is,
where the recorded runs are, which numbers they gave and how to re-check the archive yourself.
The evaluation itself is written up in [docs/evaluation.md](../docs/evaluation.md) (every run,
battery and correction) and [docs/results.md](../docs/results.md) (the same in prose, with the
caveats); this page does not add a result.

```sh
docker build -t hyphae-consensus .      # once
sh eval/verify-archive.sh               # re-verify every archived log, offline
```

Two kinds of evidence are reported there and are never pooled:

1. A **scripted protocol conformance check**: deterministic doubles, ten pinned cases, each
   with an outcome its author wrote down in advance
   ([docs/evaluation.md](../docs/evaluation.md) section 1). It shows the runtime enforces its own
   rules; it is not an experiment.
2. **Live runs**: real Meta Model API calls with real Hyphae memory, reported as observed, with
   request ids. The archive holds 53 consensus runs in 17 event logs (1,103 records) and 8
   single-agent baseline runs in 8 logs (168 records).

## Map

| What | Where |
|---|---|
| The evaluation, section by section | [docs/evaluation.md](../docs/evaluation.md): 1 scripted check; 2 and 3 the live battery of 2026-10-26; 5 the batteries with working proofs (5.3 the final battery F01-F06, 5.4 hostile note H01-H02); 6 protocol 1.3 (6.3 M01-M04, 6.6 V01-V03, 6.7 V04-V05); 7 the single-agent baseline (7.7 B01-B04, 7.8 B05-B08); the corrections of the independent audit; the request ids |
| The results in prose, with caveats | [docs/results.md](../docs/results.md) |
| What proofs do and do not establish | [docs/verification.md](../docs/verification.md) |
| The scripted conformance example | `crates/consensus-tests/examples/evaluate.rs`; run `cargo run -q -p consensus-tests --example evaluate`, or `sh tests/run-all.sh --quick` (see [tests/README.md](../tests/README.md)) |
| The archive | [research/](../research/README.md): verbatim copies of every recorded run, `MANIFEST.json` (size, SHA-256, kind, run labels, counts of each file) and `SHA256SUMS` |
| The archived batteries | `research/2026-10-26` (C01-C07 console runs; L01-L10, I01-I02, S01-S03), `research/2026-10-27` (P01-P02, R01-R08, F01-F06, H01-H02, K01-K04), `research/2026-10-30` (M01-M04), `research/2026-11-01` (V01-V05; baselines B01-B04 and B05-B08, the latter under `baseline-2026-11-01-r10`) |
| The battery driver (live runs through the operator server) | `scripts/live_battery.py` (stdlib Python; the task texts are in its `TASKS`) |
| The single-agent baseline driver | `scripts/baseline_battery.py` (one `consensus-api baseline run` per task, paired with an archived consensus run; its `selftest` runs offline) |
| The table of a battery | `scripts/battery_table.py` (Markdown summary computed from saved run records) |
| The archive's manifest builder | `scripts/make_research_manifest.py` (writes only `MANIFEST.json` and `SHA256SUMS`) |
| Re-verification of the archive | `eval/verify-archive.sh` (this directory) |

The drivers talk to Meta and need an operator key; they are how the runs were made, not part of
checking them, and nothing in the tests or in `verify-archive.sh` calls them.

## What each run records

Per run, from the receipt and the event log (`consensus-api verify` prints the first three):

- the outcome (`supported`, `contested`, `insufficient_evidence`, `abstained`, `blocked`,
  `failed`) and the policy rule that decided it;
- per round, how many claims the Evidence Evaluator corroborated out of those proposed, and
  how many of the Critic's objections stayed open;
- the integrity of every verification record (`valid` and the other statuses) and the proof's
  anchor;
- the number of model calls and retries;
- per call: latency in milliseconds, input, output and reasoning tokens, the output bound, the
  finish reason, attempts, the schema and prompt digests and the request id Meta returned.

In the web app, open the research showcase (`/showcase`), choose a recorded run and its
**Meta API** tab: one row per recorded invocation with provider and model, request id, latency,
tokens, output bound, finish reason and attempts, and totals over real calls only. The other
tabs (Decision, Explorer, Evidence, Proof inspector, Audit trail) show the rest, and the Audit
trail can check the run's log in the browser. The published contract carries no prices, so no
cost is shown there; the operator's spend figures are in docs/evaluation.md where they were
computed. Schema 1.0 runs (C01-C07) were written before the hash chain existed and carry less.

## Headline numbers, as docs/evaluation.md and docs/results.md give them

- **Scripted check**: 10 of 10 cases produced the author-defined outcome; unauthorized
  executions 0 (docs/evaluation.md section 1).
- **Final battery F01-F06** (section 5.3, commit `3d50baf`, strict output): 6 runs, all
  `contested`, 45 Meta calls, 60,493 input and 230,597 output tokens; all 91 verification
  records `valid`. F03, for example: 6 calls, 6,874 input and 31,910 output tokens. In every
  final round every claim was corroborated by records whose proofs verified; the runs are
  contested only by the Critic's open objections (the section explains why, and that the policy
  was not changed).
- **Validation V04-V05** (section 6.7, results.md): 2 runs, 15 calls, none failed, none retried,
  all 35 verification records `valid`. V04 `supported` (for a revision that claims no
  exponential schedule), V05 `contested`. Two runs: no rate and no comparison of models is
  claimed.
- **Single-agent baseline B05-B08** (section 7.8): 4 calls, all answered at the first attempt,
  each run recorded an assessment (4,734 input and 30,632 output tokens). On the comparison,
  section 7.8 concludes that under the conditions of section 7.4 that can be applied, no task
  counts as "the baseline approved what the consensus caught", and that the condition needing
  the two independent readers stays open because their reading was not made. So the baseline
  battery does not show a single agent approving what the consensus caught; it is one run per
  task, and section 7.5 lists what it cannot show.
  The first baseline attempt, B01-B04, failed at its one call in every run (section 7.7).

Other batteries (the 2026-10-26 live battery, R01-R08, H01-H02, M01-M04, V01-V03) have their own
sections; two of the four M runs were lost to provider errors, and no rate of any outcome is
claimed for protocol 1.3. Read the caveats there before quoting a figure from here.

## Re-verifying the archive

`sh eval/verify-archive.sh` runs, in a container made from the image of the `Dockerfile`
(`docker build -t hyphae-consensus .`; no network, read-only filesystem, non-root user), for
every file `research/MANIFEST.json` lists:

- an **event log** (17): `consensus-api verify`, which checks schema versions, gap-free
  sequences, the hash chain, replays every run through the state machine and re-derives its
  recommendation; the run and record counts are compared with the manifest;
- a **single-agent baseline log** (8): `consensus-api baseline verify`;
- every file, including the **run records** (46 + 8 JSON files, which are not event logs and
  get only this check): size and SHA-256 against the manifest.

It prints one line per log (labels of its runs, path, runs, records, chain scope, digest) and
ends with `N logs verified, M failed` (M counts every manifest entry that failed any check,
run records included); the exit status is 0 only if nothing failed. Options: `--research DIR`
(another directory, mounted read-only, for example this checkout's `research` or a copy you
altered), `--image NAME`, and `CONSENSUS_API=/path/to/consensus-api` to use a locally built
binary with no Docker.

Result on this tree: 25 logs verified, 0 failed. The 17 consensus logs hold 53 runs and 1,103
records, the same figures the portal's research showcase reports; the 8 baseline logs hold 8
runs and 168 records, counted apart. The 54 run records match their pinned size and digest.
Two things this does not do, which [research/README.md](../research/README.md) explains: the
schema 1.0 console log (C01-C07) predates the hash chain, so it has no chain to verify and is not
re-derived on replay (the script prints `chain=Unchained`), and the Hyphae proofs themselves cannot be
re-verified from these files, since their witnesses are private to the operator. Logs and
receipts are not signed: the chain and the pinned digests reveal a change made after pinning,
not that the runs happened as recorded.
