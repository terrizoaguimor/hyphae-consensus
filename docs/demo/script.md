# Video script (3:00 of narration, 3:06 with the end card)

Rewritten 2026-10-31 against `a657531`, the frozen UI (monochrome, brushed
glass, Quorum logo), for a **local recording**: Mission Control served by
`consensus-api serve-public` on the recording machine, a guided run started in
the recording session, and two recorded runs of the research showcase. No live
Meta call is made or shown. Screens per beat are in
[`shot-list.md`](shot-list.md); the setup, window, theme and the order of
clicks are in [`recording-checklist.md`](recording-checklist.md); the captions
are [`captions-en.srt`](captions-en.srt) and [`captions-es.srt`](captions-es.srt).
The story of beat 1 is [`impact-story.md`](impact-story.md).

**Length.** The hackathon rules set no video length (owner research of
2026-10-31: "minute" appears on none of the event's pages; the Demo criterion
only scores "the quality of the demo video"). 3:00 of narration is our own
target, chosen to cover the four judging criteria without padding; the end
card follows it on its own, silent, for 6 s (3:00–3:06), so the export runs
about 3:06. If the submission form, once
the owner is logged in, names a shorter limit, that limit wins: cut beat 4's
guided half first, then beat 7's last sentence.

## Rules for every beat

- **Labels.** Every sequence is labelled on screen as the UI labels it, and the
  burned-in caption says the same in plain words:
  - the guided run: header badges SCRIPTED · SIMULATED · THIS SERVER and a
    delivery badge (REPLAY in the reference shots), never LIVE;
    caption "Guided example · scripted roles · SIMULATED evidence · no model
    call";
  - F03: header badges META MODEL · HYPHAE RECALL · RECORDED · MODE UNDECLARED;
    caption "Recorded run F03 · 27 Oct 2026 · from the research archive · not
    live";
  - M04: header badges META MODEL · HYPHAE RECALL · RECORDED; caption
    "Recorded run M04 · 30 Oct 2026 · from the research archive · not live".
  No sequence is called live, in narration, caption or title. No run on
  screen is labelled LIVE in this cut (nothing here is a live run). The one
  place the portal draws a LIVE META badge outside a run, the Overview's
  "Provenance labels" legend, is kept out of frame (`shot-list.md`, 2a).
  The label lines of the SRT files are short forms of these captions (they
  drop "from the research archive" and, after the first cue of a run, the
  date); the burned-in caption is always the full label above.
- **Figures.** Each beat lists the numbers on its screen and what the
  narration says, with the source. The value written here is what the archive
  holds; the portal renders it from the run. If the screen shows a different
  value, stop and report it: do not record over it, and do not "correct" the
  narration to a number the archive does not hold.
- **Pace.** About 145 words a minute on average (435 words in 3:00). Beats
  run between about 130 and 156 words a minute (table at the end); the
  fastest are beats 3 and 5, at about 155, a brisk but normal narration pace.
  Do a timed read before the session: a beat that overruns its slot is cut in
  words, not sped up. The captions use the same words.
- **Integrity is not truth.** A `valid` proof says the record is the one that
  was stored, not that it is true; beat 5 says so in words.

## The runs on screen

| Label | Run id | What it is | Source |
|---|---|---|---|
| Guided | the id the server gives the run started in the session | scenario "Reference: retry budget" (`retry`): scripted doubles, SIMULATED evidence, ends `supported` for revised proposal `prop-2`; 14 records awaiting the decision, 15 after it | `contracts/api-v1/fixtures/run-detail-retry.json`, `scenarios.json` |
| F03 | `run-d6f28bca-0a10-4fcf-9257-4cfb7374280c` | final battery of 27 Oct 2026, task "Raise payment retry attempts to 10", `contested` | `research/2026-10-27/live-battery-2026-10-27-final.jsonl`, records 64 to 91; `docs/evaluation.md` §5.3 |
| M04 | `run-c912672a-f72c-4f7e-bf5c-1d2bcf91f451` | protocol 1.3 battery of 30 Oct 2026, task "Remove the retry cap from the payment client", failed at `evaluate`, `evaluation_contract` | `research/2026-10-30/live-battery-2026-10-30-p13/runs/run-c912672a-….jsonl`; §6.3 |

---

## Beat 1 · 0:00–0:25 · The problem, and what a single agent would have shipped

**Screen.** Research showcase → F03 → Explorer, the first proposal `prop-1`
with its claims; then Evidence, `claim-2`/`claim-3` marked contradicted next to
the constraint record. Caption: the F03 label.

**Narration (54 words).**
"Teams now let coding agents propose changes. This run was recorded on 27
October. Asked to raise payment retries to 10, Meta's model, as Proposer,
claimed that 10 attempts comply with any documented limit. The demo project's
memory says the opposite: retries are capped at 3. A single agent would have
shipped that claim."

**Figures.**
- Task "Raise payment retry attempts to 10" (record 64).
- `claim-3`: "Setting payment retry attempts to 10 complies with any documented
  maximum retry limit for the payment client." `claim-2`: "… set to exactly 10
  attempts." (record 65).
- The constraint: "Constraint: outbound retries must be capped at 3 attempts
  with jitter; unbounded retries caused the 2026-08 outage." (record 66). Demo
  fixture memory: 7 records (`docs/evaluation.md` §6, "Same demo store as §5
  (7 records)", and §6.3, "each run retrieved all 7 records of the store"),
  operator-seeded demo memories (`research/README.md`, "Memory").
- "A single agent would have shipped that claim": the Proposer's first call
  sees no memory (`docs/evaluation.md` §2, I01, and §5.4, H01: "it never sees
  memory"; in F03's log the proposal, record 65, precedes the retrievals,
  records 66 to 72), and "a single Proposer call would have shipped at least
  one claim the stored records contradict" (§5.3).

---

## Beat 2 · 0:25–0:40 · How it works

**Screen.** Overview, "How a deliberation works", the four numbered cards.

**Narration (38 words).**
"Hyphae Consensus gives that change three roles, all Meta's model: a Proposer,
a Critic and an Evidence Evaluator. The runtime, not a model, fetches the
records and checks their proofs. A fixed policy recommends. Only a person
decides."

**Figures.** None. Wording from the Overview cards (`Overview.flow.*` in
`apps/web/messages/en.json`).

---

## Beat 3 · 0:40–1:15 · How the Meta API is used

**Screen.** F03 → Meta API tab: six rows (role, `muse-wonder-1.0 via meta`,
recorded request id, latency, input and output tokens) and "Totals over the
recorded model calls" (0:40–0:55). Then Showcase → M04: the header's
"The run failed" box, "A model document broke the deliberation protocol and
was refused. Stage evaluate, code evaluation_contract." (0:55–1:15). Caption
changes from the F03 label to the M04 label at the cut, at 0:55 (SRT cue 16):
the protocol 1.3 sentence is said over M04, which ran under protocol 1.3.

**Narration (90 words).**
"Here is how the Meta API is used. This run made six calls, each recorded with
Meta's request id, its latency and its token counts: 6,874 in, 31,910 out.
Every call sends a strict JSON schema for its role. Under protocol 1.3 the
Evaluator's schema is built per call, one required key per claim and per open
objection, so nothing can be skipped in silence. And it fails closed. In run
M04, on 30 October, the Evaluator left a contradiction unjudged: the runtime
refused the answer and issued no recommendation."

**Figures.**
- F03: 6 model invocations, 3 roles (Proposer twice, Critic twice, Evidence
  Evaluator twice); request ids `chatcmpl-01a17c50-1917-…` to
  `chatcmpl-01a17c53-61f8-…` (`showcase-entry-F03.json`; §"Request ids
  (2026-10-27)"). Totals on screen: latency 234,395 ms, input 6,874, output
  31,910 (each "reported by 6 of 6 calls"); reasoning tokens and attempts
  "0 (reported by 0 of 6 calls)", because schema 1.2 recorded neither (§5.3;
  computed from the six invocations of records 65, 73, 74, 82, 83, 84). Read
  6,874 and 31,910 only; do not read the zeros as values.
- Strict schemas: four call kinds (proposal, challenge, evaluation, revision),
  each with its own schema sent with `strict: true`, every list field required
  since `3d50baf` (F03 ran on that commit); the Evaluator's schema keyed by id
  per call since tag `prompt-freeze-2` (README, "How this project uses Meta's
  API"; §6.2). F03 predates the keyed schema, so the narration says "under
  protocol 1.3" for it. M04's two Evaluator calls sent different schemas
  (`schema_sha256` `782d909b…` and `3534b6ea…`, records 15 and 29 of its log),
  as designed; the public surface does not publish digests, so they are not on
  screen and not said.
- M04: failed at `evaluate`, `evaluation_contract`; verbatim reason in the log
  "evaluate output refused by the protocol 1.3 contract:
  carried_contradiction_judged: "328114099256634798832222840773545327795",
  contradicted in an earlier round, is neither listed as contradicting again
  nor cleared" (record 29); no recommendation, authorization `pending` (§6.3).
  On screen: Outcome "None: the run failed", 29 records. Its Meta API tab, if
  shown: 6 invocations, the last on the failure record; totals 9,849 input /
  22,544 output / 19,030 reasoning tokens, 167,627 ms.
- Do not say "attempts" or "retries" over either Meta API tab: the public
  surface does not publish attempt counts (`Run.meta.operatorOnly`). No cost is
  shown or said.

---

## Beat 4 · 1:15–1:45 · The deliberation

**Screen.** 1:15–1:30: New deliberation → Guided examples → "Reference: retry
budget" → Run; the run workspace's Playback tab with "simulated · paced for
readability", the three role columns filling in. Caption: the guided label.
1:30–1:45: F03 → Decision tab: "contested", Evidence: HYPHAE RECALL, the scope
line, "Why the policy recommends this", "Open objections". Caption: the F03
label.

**Narration (68 words).**
"To watch the steps, a guided example: scripted roles, simulated evidence, no
model called. The Proposer claims, the Critic objects, the Evaluator judges
each claim, the Proposer revises, and the round runs again. In F03 the revision
went back to 3 attempts with 50 millisecond jitter, and all 5 claims were
corroborated. Still contested: 6 objections stay open, three missing tests, two
unsupported claims and one duplicate-charge risk."

**Figures.**
- Guided: whatever its header and Playback show; it ends `supported` on
  SIMULATED evidence, which is a scripted outcome and is never described as a
  model result (do not say its outcome).
- F03 revision `prop-12`: "max_attempts = 3 with jitter_ms = 50" (record 82).
- F03 final round: 5/5 corroborated, 6 open (§5.3); rationale on screen
  "Contested: 5/5 claims corroborated; proofs: 6 valid, 0 invalid, 0
  unavailable/error; 6 unresolved challenges." (record 91).
- The 6 open items: 3 `missing_test`, 2 `unsupported_claim`, 1 `security_risk`
  (record 83; `item-19` is the duplicate-charge risk).
- The Decision tab also shows "Earlier-round objections never re-judged: 5."
  (F03 ran under protocol 1.2; `research/README.md`). Keep it in frame; it is
  neither narrated nor captioned, for lack of time: the portal's own line says
  it.
- Keep `claim-15` out of a close-up (it restates the seeded record that
  misdescribes the fixture test; `docs/evaluation.md`, Corrections item 3).
  The caption "One cited record misdescribes the fixture test
  (docs/evaluation.md, Corrections 3)" is on screen from 1:30 to 1:45, the
  whole F03 half of this beat, including "all 5 claims were corroborated"
  (SRT cues 25 to 28).

---

## Beat 5 · 1:45–2:10 · The evidence and the proofs

**Screen.** F03 → Evidence tab (records with integrity and the Evaluator's
reading per round), then Proof inspector: the plain reading of a valid proof
and the sentence "… the artifacts are held privately by the operator
(witnesses contain the full store), so the public can re-verify the event
chain, not these proofs." in full.

**Narration (65 words).**
"Each claim is tied to the records that back it or contradict it. Every record
came with a Hyphae recall proof, verified at run time against a pinned anchor:
all six valid in the final round. Valid means this is the record that was
stored. It does not mean the record is true. The proof files stay with the
operator; anyone can re-check the chain."

**Figures.**
- F03: 7 records retrieved; final round 6 valid (§5.3 "6 valid"; records 85 to
  90); every verification of F03 `valid`, verifier `hyphae proof verify`,
  anchor `d91ac10d…` (`research/README.md`, "Proofs, 2026-10-27 runs").
- "Valid … not true": `research/README.md` ("Truth"); the Overview's provenance
  note ("A valid proof shows that Hyphae can reproduce the sealed recall a
  record came from, not that the record is true …").

---

## Beat 6 · 2:10–2:30 · The chain check, in this browser

**Screen.** F03 → Audit trail → "Check the chain in this browser" → Check the
chain → "Chain checks out", the five "matches" lines and the limits paragraph
under them.

**Narration (50 words).**
"Every record is hash-chained. This browser just downloaded the archived log
and recomputed every hash: all 189 records check out, and the file matches the
digest pinned in the archive's manifest. The log is unsigned: this shows the
bytes match that pinned digest, not that anything in it is true."

The claim stays within the UI's own: the log and the manifest come from the
same server, and the limits paragraph says a log rewritten and re-hashed as a
whole would check out too unless a digest obtained earlier says otherwise. The
narration therefore claims a match with the digest, not "nothing changed"; the
same digest is in `research/MANIFEST.json` and `research/SHA256SUMS` of the
committed repository, where anyone can compare it.

**Figures (on screen; checked against the real archive file in the Playwright
image on 2026-10-31).**
- "All 189 records hash and link as recorded, and this run's segment matches
  what the views hold." (`research/MANIFEST.json`: the file holds 189 events.)
- "File SHA-256 3d647ed975654be2d9fa2feccfdc6059f28e3f16ad72969740e27809f6f68c76
  (292,322 bytes), against the digest the archive's manifest pins."
- "189 records of the file checked …"; "This run's records in the file:
  sequences 64 to 91 (28 records) …"; the head
  `629a2fb5923350e5…`; "Record count against the receipt's (28)."
- The limits paragraph ("… The chain is unsigned: a log rewritten and re-hashed
  as a whole would check out too, …") stays in frame.

---

## Beat 7 · 2:30–2:47 · A person decides

**Screen.** The guided run of beat 4 (from My runs) → Decision tab: the outcome
with "Evidence: SIMULATED", the scope line, "Your decision" with the notice;
click Approve; "Decision recorded: approved by visitor:… on …" and "This
decision carries no operational authority: it records a visitor's choice on a
simulated run." Caption: the guided label.

**Narration (40 words).**
"The run stops at a decision point. Only a person approves or rejects, and on
this public portal the decision carries no authority: approving records a
choice and executes nothing. This is the guided run; a recorded run is
read-only."

**Figures.** On screen only: the guided run's records go from 14 to 15 with the
decision. The notice text is `Run.decision.publicDemo` in
`apps/web/messages/en.json`: "Decisions on this public portal carry no
operational authority: approving or rejecting records your choice on this run
and executes nothing." The pseudonym `visitor:<16 hex>` is not personal data;
type no note.

---

## Beat 8 · 2:47–3:00 · Close (end card 3:00–3:06)

**Screen.** Overview header ("Verifiable multi-agent reasoning and decision
accountability") under the narration, 2:47–3:00; then the end card alone,
silent, 3:00–3:06, long enough to read its four lines.

**Narration (30 words).**
"Hyphae Consensus: agent recommendations you can inspect, check and hold to
account before anyone acts. Its three roles run on Meta's muse-wonder-1.0; we
built it with Muse Code and Claude."

**Figures.** `muse-wonder-1.0` in every recorded live run; development used
Meta's Muse Code and Anthropic's Claude (README, "AI assistance": "Meta's Muse
Code (with Muse Wonder Full) and Anthropic's Claude"); the narration and the
end card both name both.

**End card text.**
- "Independent project; not affiliated with, sponsored by, or endorsed by Meta."
- "Footage: Mission Control on a local server. Guided example: scripted roles,
  SIMULATED evidence. Recorded runs F03 (27 Oct 2026) and M04 (30 Oct 2026)
  from the research archive. No live run is shown."
- "Built with Meta's Muse Code and Anthropic's Claude as coding assistants;
  inside the product, only Meta's Model API."
- The commit sha the export was built from, and the repository link once it is
  public.

## Word count

| Beat | Slot | Words | Words a minute |
|---|---|---|---|
| 1 | 0:00–0:25 | 54 | 130 |
| 2 | 0:25–0:40 | 38 | 152 |
| 3 | 0:40–1:15 | 90 | 154 |
| 4 | 1:15–1:45 | 68 | 136 |
| 5 | 1:45–2:10 | 65 | 156 |
| 6 | 2:10–2:30 | 50 | 150 |
| 7 | 2:30–2:47 | 40 | 141 |
| 8 | 2:47–3:00 | 30 | 138 |
| Total | 3:00 | 435 | 145 |
| End card | 3:00–3:06 | none | — |
