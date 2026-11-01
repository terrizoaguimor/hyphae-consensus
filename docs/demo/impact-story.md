# Impact story: run F03

Written 2026-10-31 against `a657531`. One real archived run, told as what a
single agent would have approved and what the three roles caught instead.
The same text opens the video (`script.md`, beat 1) and can head the README and
the submission form. Every fact below is checkable in `research/`; the table at
the end says where.

**The run.** F03, `run-d6f28bca-0a10-4fcf-9257-4cfb7374280c`, final battery of
27 October 2026 (`research/2026-10-27/live-battery-2026-10-27-final.jsonl`,
records 64 to 91; `docs/evaluation.md` §5.3). Task: "Raise payment retry
attempts to 10". Project `demo-repo`, a demo fixture whose Hyphae memory holds 7
records the operator seeded; the "2026-08 outage" and the "2026-09 payments
review" are part of that fixture, not events at a real company.

Why F03 and not another candidate: it is the clearest case where the first
proposal states, in its own words, the thing the stored records forbid, and
both the Critic and the Evaluator caught it on the record. M04 (2026-10-30,
the fail-closed refusal) shows the evaluation contract at work but ends with no
recommendation at all; the video uses it in the Meta API beat instead. H01
(the hostile note) is a security story, not an everyday one.

## Three sentences

Teams that let coding agents propose changes need each proposal checked against
what the project has already decided, before a person signs off. In recorded
run F03, Meta's `muse-wonder-1.0` as Proposer answered "raise payment retries to
10" with a claim that 10 attempts "complies with any documented maximum retry
limit"; the Critic objected that it contradicts the documented 3-attempt cap
and the 2026-09 payments review decision, and the Evidence Evaluator recorded
that claim and the 10-attempt claim as contradicted by stored records whose
integrity proofs verified, which is what a single Proposer call would have
shipped unchecked. The revision went back to the documented 3 attempts with
50 ms jitter, yet the run ends `contested`, not `supported`, because 6
objections stay open (among them that no record shows a retried payment cannot
be charged twice) and no stored record settles them, so the decision goes to a
person with every open question listed.

## 60 words (submission form)

Coding agents propose changes that must respect what a project
decided. Hyphae Consensus has three Meta muse-wonder-1.0 roles review a change
against proof-checked memory. In recorded run F03, the Critic and Evaluator
caught "raise retries to 10" breaking the demo project's 3-attempt cap. The
revision kept the cap; six open objections, including duplicate-charge risk,
kept it contested: a human decides.

(60 words, counted with `wc -w` on the paragraph; "muse-wonder-1.0" and
"3-attempt" count as one word each.)

## Where each fact is

Records are numbered by `sequence` in
`research/2026-10-27/live-battery-2026-10-27-final.jsonl`.

| Fact | Where |
|---|---|
| Task title and text ("Timeouts persist under load. Increase the payment client's retry attempts to 10 …") | record 64, `task_created` |
| Proposer's first proposal `prop-1`; `claim-2` "… will be set to exactly 10 attempts."; `claim-3` "Setting payment retry attempts to 10 complies with any documented maximum retry limit for the payment client."; `claim-4` "… will not result in duplicate payment charges." | record 65, `proposal_submitted`, request id `chatcmpl-01a17c50-1917-73e5-a8ad-705baefd6cc9` |
| The Proposer's first call sees no memory | `docs/evaluation.md` §5.4 ("it never sees memory"); §5.3: "a single Proposer call would have shipped at least one claim the stored records contradict" |
| The constraint: "Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage." | record 66, `evidence_retrieved`, id `186670067796945807561772405393844769735` |
| The decision: "Decision (2026-09 payments review): … max_attempts = 3 with jitter_ms of at least 50; raising the attempt cap above 3 is not allowed." | record 71, id `328114099256634798832222840773545327795` |
| Critic, round 1: 5 items; `item-6` contradictory evidence ("Setting payment retry attempts to 10 contradicts the documented maximum of 3 attempts and the 2026-09 payments review decision …"), `item-7` constraint conflict (jitter), `item-8` regression risk, `item-9` and `item-10` unsupported claims (duplicate charges, timeout budget) | record 73, `challenge_submitted` |
| Evaluator, round 1: `claim-2` and `claim-3` not corroborated, each contradicted by the constraint, record `237412834382378687445660534362297845298` and the decision; 0 of 4 corroborated; 5 items unresolved | record 74, `evaluation_submitted`; `docs/evaluation.md` §5.3 ("0/4, 5 open, 2 contradicted") |
| Those proofs verified: `integrity_status: valid`, verifier `hyphae proof verify`, factual `contradicted` for the 3 records | records 75 to 81, `evidence_verified` |
| Revision `prop-12`: "set the payment-client RetryPolicy to max_attempts = 3 with jitter_ms = 50", drops the duplicate-charge and timeout-budget claims; uncertainty "No evidence was provided about idempotency or duplicate-charge protection …" | record 82, `revision_submitted` |
| Critic, round 2: 6 items (`item-18` to `item-23`): 3 missing test, 2 unsupported claim, 1 security risk ("… increases retries for payment SendError::Timeout failures without any retrieved evidence of idempotency or duplicate-charge protection …") | record 83 |
| Evaluator, round 2: 5 of 5 claims corroborated, 6 items unresolved | record 84; §5.3 ("5/5, 6 open") |
| Recommendation `contested`: "Contested: 5/5 claims corroborated; proofs: 6 valid, 0 invalid, 0 unavailable/error; 6 unresolved challenges." Authorization `pending` | record 91; `research/MANIFEST.json` (run F03) |
| 6 Meta calls, 6,874 input / 31,910 output tokens, 234.4 s model latency | §5.3; the six invocations on records 65, 73, 74, 82, 83, 84 |

## What the story must not say

- Not that F03 was "right" or that the change is safe: `contested` means the
  policy recommends nothing while objections are open. Integrity is not truth:
  a `valid` proof says the record is the one stored, not that it is true.
- Not that the records are production knowledge: they are 7 operator-seeded
  demo records (`research/README.md`, "Memory").
- One of the three records that contradict `claim-2` and `claim-3` (record
  `237412…`, the constraint-test fact) misdescribes the fixture's test
  (`docs/evaluation.md`, Corrections item 3); the constraint and the decision
  contradict both claims on their own, so the story names only those two. The
  revision's `claim-15` restates that record and was corroborated by it: keep
  `claim-15` out of any close-up, or caption it (`shot-list.md`).
- F03 ran under protocol 1.2: its 5 round-1 objections were not judged again
  in round 2 (`research/README.md`, "Earlier-round objections never
  re-judged"; the portal shows "Earlier-round objections never re-judged: 5.").
  Its `contested` comes from the 6 round-2 objections, which the final
  evaluation kept open, so the caveat does not change the outcome; it is shown,
  not hidden.
- Not a rate: F01 to F06 all ended `contested` (§5.3), and that is 6 runs of
  one model on one demo store, not a measured accuracy.
