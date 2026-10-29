# Video script (3 minutes)

Written 2026-10-29 against `bc3b262`. Nothing has been recorded. Screens per beat
are in [`shot-list.md`](shot-list.md); prerequisites and the checks before
publishing are in [`recording-checklist.md`](recording-checklist.md).

## How to read this script

- **Figures.** Every number a beat says or shows is listed under "Figures" with
  its source: a run label of `research/` (labels as in `research/MANIFEST.json`)
  or a section of `docs/evaluation.md`. They are stated as recorded. Where the
  portal renders a figure from the run on screen (the trace, the counts, the
  Meta API rows), the narration reads what the screen shows; the value written
  here is what the recorded data holds and is a check, not a line to say over a
  different screen.
- **Markers.** **[PENDING: …]** marks a beat that depends on work not yet done.
  Each such beat has a **Fallback** that needs none of it.
- **No lucky outcomes.** The live run of beats 3–5 may end in any outcome, or
  fail. Beat 4 has one narration per outcome; the cut is assembled after the
  run, from what its Decision tab shows. Nothing is re-run to get a better
  outcome (as in `docs/evaluation.md` §2: "the battery was not re-run to hide
  the timeouts").
- **Narration pace.** About 150 words a minute; each beat's narration fits its
  slot at that pace.

## Dependencies

| Marker | Work | Status on 2026-10-29 |
|---|---|---|
| STAGING | Access-protected staging deployment with live mode on (`docs/deployment/runbook.md` §8) | pending |
| SNAPSHOT | Sealed Hyphae snapshot capture; live mode cannot start without it (`docs/deployment/runbook.md` §4.1, `CONSENSUS_SNAPSHOT_DIR`) | captured 2026-10-29 (`b0375df`); boots with `0515f29` (merged as `7e26866`); not on the server |
| BATTERY-1.3 | Counted protocol 1.3 battery on `prompt-freeze-2` (`docs/evaluation.md` §6); the public presets repeat battery tasks, and no 1.3 figure may be shown before it is in §6 | pending; not run on 2026-10-29 (no Meta key on the development machine) |
| STAGING-REC | The live preset runs recorded on staging (`recording-checklist.md` §2) | pending; needs STAGING, SNAPSHOT, BATTERY-1.3 |
| CHAIN-BROWSER | The in-browser chain check (merged with web lane W2 at `bc3b262`) passes its review | review running |

**Fallback cut, as a whole:** guided mode (SIMULATED) +
the research showcase (RECORDED) + a battery replay labelled "replay". In this
script the "battery replay" is the recorded run F03 opened from the showcase
with a burned-in caption "REPLAY · recorded run F03, 27 Oct 2026 · not live".
The portal itself labels it RECORDED; it has no stream and no LIVE word
(`apps/web/src/components/run/useRecordedRun.ts`).

---

## Beat 1 · 0:00–0:25 · Impact first

**Screen.** Research showcase → F03, Evidence tab: the first proposal's claim
"set to exactly 10 attempts" next to the stored constraint.

**Narration (about 65 words).**
"On 27 October 2026 we ran Meta's muse-wonder-1.0 six times against a demo
project's seeded memory, whose proofs verified. In every run, the first
proposal contained claims that the stored records contradict, and none was
recommended. Here the task asked to raise payment retries to 10. The demo
project's stored constraint says retries are capped at 3 attempts with jitter."

Burned-in caption while the Evidence tab is on screen: "Demo-repo fixture ·
7 seeded records".

**Figures.**
- Final battery F01–F06: 6 runs, every first proposal had 1–3 claims
  contradicted by stored records, none supported, all 6 `contested`
  (`docs/evaluation.md` §5.3).
- F03 first proposal, claim 2: "The payment client's retry attempts
  configuration will be set to exactly 10 attempts." (F03, proposal `prop-1`
  in `research/2026-10-27/live-battery-2026-10-27-final.jsonl`).
- Stored constraint, as retrieved in F03: "Constraint: outbound retries must be
  capped at 3 attempts with jitter; unbounded retries caused the 2026-08
  outage." (same log, F03's retrieved evidence).
- The memory: a fresh demo store of 7 operator-seeded demo-repo records
  (`docs/evaluation.md` §5).
- F03 round 1: 0/4 corroborated, 5 open items, 2 claims contradicted
  (`docs/evaluation.md` §5.3); the two contradicted claims (claims 2 and 3) are
  each contradicted by the constraint, the 2026-09 decision and the
  constraint-test fact (F03's first evaluation in the log).

**Say only if the test fact is on screen.** Frame the shot on the constraint
and the decision. If the constraint-test fact (seeded record 5) is in frame
anyway, add: "One of these records, the test fact, misdescribes the fixture's
test; the constraint and the decision contradict the claim on their own."
(`docs/evaluation.md`, Corrections item 3.)

**Depends on.** Nothing pending: the showcase serves `research/` today.

---

## Beat 2 · 0:25–0:55 · Architecture

**Screen.** Overview, "How a deliberation works" (four steps), then the
provenance labels legend; one second on Docs → "Verified evidence".

**Narration (about 75 words).**
"Three roles, all Meta's model: a Proposer turns the task into explicit
claims, a Critic challenges them, an Evidence Evaluator judges each claim
against the evidence. The runtime, not a model, retrieves the records from
Hyphae Agent Memory and verifies their integrity proofs. A fixed policy turns
judgments and proof statuses into a recommendation. Only a person approves,
and approving executes nothing. A valid proof means the record is the one
that was stored. It does not mean the record is true."

**Figures.** None. The wording follows the Overview's flow texts
(`apps/web/messages/en.json`, `Overview.flow`) and `research/README.md`
("Truth": a valid proof "establishes that a record matches its anchored
snapshot, not that its content is true").

**Depends on.** Nothing pending.

---

## Beat 3 · 0:55–1:45 · A live preset run

**[PENDING: STAGING-REC, which needs STAGING, SNAPSHOT and BATTERY-1.3]**

**Screen (planned cut).** New deliberation → Live presets → "Raise payment
retry attempts to 10" → attestation and Turnstile → Start live run → run
workspace, Live tab, shown as a time-lapse with a burned-in caption
"Recorded on staging, <date> · time-lapse ×N". Then the Evidence tab on a
contradicted claim, then the Proof inspector on the record that contradicts it.

**Narration (planned cut; about 110 words; adapt the bracketed parts to the
run).**
"This is a live run on our staging server, sped up [N] times. The Proposer
answers the task as asked. The Critic [raises a constraint conflict: the cap is
3]. [The Evaluator records the claim as contradicted by a stored record.] The
evidence comes from a sealed Hyphae snapshot, and the runtime re-verifies its
proofs for every run: [valid]. The Proposer revises; the Critic and the
Evaluator run again on the revision. Every model call goes to Meta's API on the
owner's key; visitors choose a preset and can never send free text to the
model."

- Say a Critic objection kind, a contradiction or a proof status only if it
  is on screen in this run. If the run has no `constraint_conflict`, name the
  kinds the Critic did raise. If no claim is contradicted, show the Evidence tab
  as it is and drop that sentence.
- If the run fails (for example a transport failure, as K02 in
  `docs/evaluation.md` §6.1), show the failure sentence in the header, say
  "the run failed safely: no recommendation, nothing approved, the reason
  recorded", and use the fallback for the rest of beats 3–5.
- N is the recorded duration divided by the on-screen duration, rounded down
  to a whole number, and the caption shows it for the whole sped-up stretch.
  Runs of this task took 205.5 s (K03, protocol 1.3 smoke) and 234.4 s (F03)
  of model latency (`docs/evaluation.md` §6.2, §5.3).

**Figures.** Only what the run on screen shows. No figure from
BATTERY-1.3 unless it is already in `docs/evaluation.md` §6.

**Fallback (about 50 s).**
1. 0:55–1:10. New deliberation → Guided examples → "Reference: retry
   budget" → Run. The run workspace's Playback tab, paced ("simulated · paced
   for readability"). Narration: "First, a guided example: scripted roles and
   SIMULATED evidence, no model called. It shows the steps: proposal, critique,
   evaluation, revision."
2. 1:10–1:45. Showcase → F03 (RECORDED), caption "REPLAY · recorded run F03,
   27 Oct 2026 · not live". Explorer tab scrolled from the first proposal to
   the first evaluation, then the Evidence tab on claim 2, then the Proof
   inspector on the constraint record. Narration: "Now a real run, recorded on
   27 October and replayed from the archive. The Critic raised a constraint
   conflict. The Evaluator recorded two claims as contradicted by stored
   records. Each record's proof was verified at run time: valid. The proof
   files stay with the operator, so you can re-check the chain, not the
   proofs. The Proposer then revised."

**Fallback figures.**
- Guided "Reference: retry budget" reaches `supported` on SIMULATED evidence
  (`crates/consensus-api/src/guided.rs`, `info`); a scripted outcome, never to
  be described as a model result.
- F03 round 1: the Critic raised 5 items, among them a `constraint_conflict`
  (F03's first challenge in the log); 2 claims contradicted
  (`docs/evaluation.md` §5.3); 6 valid in the final round, all 91 verification
  records of the final battery `valid` (§5.3).
- The proof artifacts are held by the operator; the public can re-verify the
  event chain, not the proofs (`apps/web/messages/en.json`,
  `proofs.recordedAccepted`; `research/README.md`, "What cannot be verified").

---

## Beat 4 · 1:45–2:20 · Decision center

**[PENDING for the first half: STAGING-REC. The R06 half has no dependency.]**

**Screen (planned cut).** The live run's Decision tab: the outcome badge with
its evidence label, the scope line when the recommendation is for a revision,
"Why the policy recommends this" with the counts rendered from the trace, the
open objections. Then Showcase → R06 (RECORDED), Decision tab.

**Narration, first half (about 40 words): read the line that matches the
screen.**
- `contested`: "Contested. [k of n] claims corroborated, [m] objections still
  open, [most of them missing tests]. The policy does not recommend a change
  while objections are open."
- `supported` for a revised proposal: "Supported, but read the line under it:
  for the revised proposal, which [no longer does what the task asked]. The
  runtime does not judge that; the person does." (The K03 precedent:
  `docs/evaluation.md` §6.2.)
- `insufficient_evidence` or `abstained`: "[Insufficient evidence / The
  Evaluator abstained]: the evidence does not back every claim, so nothing is
  recommended."
- Failed: covered in beat 3; use the fallback.

**Narration, second half (about 55 words).**
"In the first live battery, one run reached supported: R06, on 27 October. Its
trace shows seven earlier-round objections never judged again, and our
evaluation's corrections record that one record it cites misdescribes a test.
A valid proof cannot catch that: integrity is not truth. Protocol 1.3 carries
every objection forward; its counted battery is still pending."

Burned-in caption for as long as R06's Decision tab is on screen: "Cited
record 5 misdescribes the fixture test: docs/evaluation.md, Corrections 3".
The portal does not show this caveat, so the caption carries it.

**Figures.**
- Live run: only what its Decision tab renders.
- R06: `supported` after 1 revision; Critic 7 → 0; Evaluator 0/3, 7 open, 2
  contradicted → 3/3, 0 open; 6 valid (`docs/evaluation.md` §5.2).
- R06: 7 earlier-round objections never re-judged (`research/README.md`,
  "Earlier-round objections never re-judged"; `docs/evaluation.md`,
  Corrections item 4). The portal renders this line for runs before protocol
  1.3 ("Earlier-round objections never re-judged: 7.").
- R06's claim 3 (`claim-16`) cites seeded record 5, which misdescribes the
  fixture test; R06's `supported` does not depend on it (`docs/evaluation.md`,
  Corrections item 3; `research/README.md`). Say "one record it cites
  misdescribes a test", never "R06 was wrong".
- Protocol 1.3 carries open items and contradicted records forward and requires
  an explicit verdict per item (`docs/evaluation.md`, Corrections item 4;
  ADR-0010). Do not claim what 1.3 does to outcomes beyond the smoke runs,
  which are not counted as validation (§6.1, §6.2).

**Fallback, first half.** Showcase → F03, Decision tab. Narration:
"Contested. After the revision the Evaluator corroborated all five claims
against records whose proofs verified; corroborated is not true. Six
objections stayed open: three missing tests, two unsupported claims, one
security risk. The policy does not recommend a change while objections are
open."

**Fallback figures (F03).**
- Final round: 5/5 corroborated, 6 open, 6 valid (`docs/evaluation.md` §5.3);
  recorded rationale "Contested: 5/5 claims corroborated; proofs: 6 valid, 0
  invalid, 0 unavailable/error; 6 unresolved challenges." (F03's
  recommendation in the log).
- The 6 open items: 3 `missing_test`, 2 `unsupported_claim`, 1 `security_risk`
  (F03's second challenge and second evaluation in the log; counted for this
  script from the archived events).
- One of the five corroborated claims, `claim-15`, restates seeded record 5
  (the constraint-test fact), which misdescribes the fixture test; the
  Evaluator corroborated it with that record (F03's revision and second
  evaluation in the log; `docs/evaluation.md`, Corrections item 3). Keep
  `claim-15` off screen in this shot, or add the caption of the R06 half for
  it.
- 5 earlier-round objections never re-judged (`research/README.md`); the
  portal shows that line for F03 too. If there is time, read it.

---

## Beat 5 · 2:20–2:45 · Audit trail and the Meta API

**[PENDING: CHAIN-BROWSER for the in-browser check; STAGING-REC for the live
run's Meta API rows]**

**Screen.** R06 (RECORDED), Audit trail → "Check the chain" → "Chain checks
out". Then the live run's Meta API tab (fallback: R06's Meta API tab). Planned
cut only: the live run's Decision tab, Approve, the recorded decision with the
notice that public decisions carry no operational authority.

**Narration (about 60 words).**
"Every record is hash-chained. This browser downloaded the archived log and
recomputed every hash: the chain checks out and matches the digest pinned in
the archive's manifest, so nothing changed since pinning. The log is unsigned;
this says nothing about whether the records are true. The Meta API panel lists
every call: request id, latency, tokens. [And a person decides: approving
executes nothing.]"

Do not say "attempts": the public surface does not publish the attempt count
(`crates/consensus-api/src/public_view.rs`, `operator_only`;
`Run.meta.operatorOnly` in `apps/web/messages/en.json`), and R06 (schema 1.2)
recorded none; attempts are recorded from protocol 1.3 on (`docs/evaluation.md`
§6).

**Figures.**
- R06's log is `research/2026-10-27/live-battery-2026-10-27.jsonl` (181 records,
  R01–R08; R06 is 28 of them), so the check covers the whole file
  (`research/README.md`: "checking one run needs the file from its first
  line"). Read the count the screen shows.
- What the check shows: the chain is unsigned, so a log rewritten and
  re-hashed as a whole would also check out, unless an earlier digest says
  otherwise; the comparison with the digest `research/MANIFEST.json` pins is
  what shows no change since pinning (`Run.chain.limits` and
  `Run.chain.sha256` in `apps/web/messages/en.json`; `research/README.md`,
  "What cannot be verified from these files", Authenticity).
- R06's Meta API tab: 6 calls, 6,382 input / 25,685 output tokens
  (`docs/evaluation.md` §5.2; request ids under "Request ids (2026-10-27)").
- The Meta API tab shows no cost: the contract publishes no prices
  (`apps/web/src/components/run/MetaTab.tsx`). Do not say a cost over it. The
  live budget in US dollars is visible on New deliberation (beat 3).

**Why the chain check uses a recorded run.** The public surface does not
publish the verbatim log of a live run ("a live model's records may quote
upstream text"; `Run.audit.logUnavailable`), so the in-browser check runs on
recorded and guided runs only.

**Fallback (if CHAIN-BROWSER is not cleared).** A terminal, cropped to the
command and its output:
`consensus-api verify research/2026-10-27/live-battery-2026-10-27.jsonl --run run-d4d214a4-0532-4b44-9a98-715bd29d81da`
(read-only; README, "Setup and verification"). Same narration, "this command"
for "this browser". Approve on the guided run of beat 3 instead of a live run.

---

## Beat 6 · 2:45–3:00 · Close

**Screen.** Overview, the project name and tagline; end card with the
non-affiliation line and the repository link.

**Narration (about 25 words).**
"Hyphae Consensus makes agent recommendations inspectable, evidence-aware, and
accountable before a human authorizes anything."

**End card text.** "Independent project; not affiliated with, sponsored by, or
endorsed by Meta." (README). Add "Footage: [live run recorded on staging,
<date>, time-lapse] / [guided run: SIMULATED] / [recorded runs F03, R06: 27 Oct
2026]" for whatever the cut contains.

**Depends on.** Nothing pending.
