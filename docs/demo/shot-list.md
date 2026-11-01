# Shot list (Mission Control, local recording)

Screens per beat of [`script.md`](script.md), for the frozen UI at `a657531`.
Routes, tab names and labels were checked against `apps/web/src` and
`apps/web/messages/en.json`, and against reference screenshots taken on
2026-10-31 in the pinned Playwright image (`apps/web/scripts/playwright-docker.sh`)
over the suite's API mock (`apps/web/e2e/api-mock.ts`), dark and light theme,
1920 × 1080 viewport. Those shots are scratch (`apps/web/test-results/`,
git-ignored) and show the mock's fixture content for recorded runs: they fix
the layout and the labels, not F03's text, which the real server renders from
the archive. The archive's own values are in `script.md`.

Recording setup (theme, window, zoom, server, order of clicks):
[`recording-checklist.md`](recording-checklist.md). Base URL
`http://127.0.0.1:8788`; language English (`/en/`).

## Routes and tabs

| Screen | Route | Notes |
|---|---|---|
| Overview | `/en/` | title "Verifiable multi-agent reasoning and decision accountability"; "How a deliberation works" (four numbered cards); "Provenance labels"; "Impact metrics" counted in the browser from the showcase listing |
| New deliberation | `/en/new/` | "Guided examples" (five cards, each SCRIPTED · SIMULATED evidence, with its expected outcome and a Run button); live mode is off on the local server, and the line that says so stays out of frame |
| My runs | `/en/runs/` | this browser session's runs: the guided run of beat 4 is found here again for beat 7 |
| Research showcase | `/en/showcase/` | "48 recorded runs (919 records), as the server lists them from the archived chains", grouped by battery; manifest check "Matches" |
| Guided run | `/en/run/?id=<run id>` | tabs Playback, Decision, Explorer, Evidence, Proof inspector, Audit trail, Meta API; opens on Playback |
| F03 | `/en/run/?id=run-d6f28bca-0a10-4fcf-9257-4cfb7374280c&source=showcase` | RECORDED mode: tabs Decision, Explorer, Evidence, Proof inspector, Audit trail, Meta API (no Playback); opens on Decision; read-only |
| M04 | `/en/run/?id=run-c912672a-f72c-4f7e-bf5c-1d2bcf91f451&source=showcase` | RECORDED mode, as F03 |

## Labels that must stay in frame

| Run | Header provenance badges | Also visible |
|---|---|---|
| Guided | SCRIPTED · SIMULATED · THIS SERVER · a delivery badge (REPLAY in the reference shots), never LIVE; "scripted double, no model call" | Playback: "simulated · paced for readability"; outcome "supported" with "Evidence: SIMULATED"; Decision: "Every proof status of this run is SIMULATED: the scripted verifier recorded it, and nothing was checked." |
| F03 | META MODEL · HYPHAE RECALL · RECORDED · MODE UNDECLARED; "Meta model · recorded <date>"; "The run was recorded before schema 1.3 and declares no mode." | the section "Recorded run, read-only" with "Run F03 of battery live-battery-2026-10-27-final, shown from the research archive: nothing streams, nothing runs, and no decision can be recorded here." and the archive's note |
| M04 | META MODEL · HYPHAE RECALL · RECORDED (schema 1.3 declares its mode, so no MODE UNDECLARED flag is expected; if the header shows one, record it as shown) | "The run failed" box; the "Recorded run, read-only" section ("Run M04 of battery live-battery-2026-10-30-p13 …") |

The header of a recorded run sits above the tabs; at 125 % zoom a tab's content
is below the fold. Scroll so the badges are seen once per run, then scroll to
the tab content; the burned-in caption carries the label while the badges are
out of frame.

## Beat 1 · 0:00–0:25 · The problem

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 1a | `/en/showcase/` | listing loaded | the count line (48 runs, 919 records) and the `live-battery-2026-10-27-final` group with F03 "Raise payment retry attempts to 10", contested; click F03 | — |
| 1b | F03 · Explorer | loaded | the header badges (one second), then `proposal_submitted` of `prop-1` with `claim-2` "… set to exactly 10 attempts." and `claim-3` "… complies with any documented maximum retry limit …" | — |
| 1c | F03 · Evidence | — | the constraint "Constraint: outbound retries must be capped at 3 attempts with jitter; …" and round 1's reading: contradicted, integrity valid | the constraint-test fact (record `237412…`) as the focus of a close-up |

Caption for 1b–1c: "Recorded run F03 · 27 Oct 2026 · from the research
archive · not live". Second caption line for 1c: "Demo project · 7 seeded
memory records".

## Beat 2 · 0:25–0:40 · How it works

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 2a | `/en/` | API connected ("Connected to the API: schema 1.3, policy 1.2, contract …") | the four cards: Three roles deliberate · The runtime checks evidence · A fixed policy recommends · A person authorizes | the tour invitation (dismiss it before recording: "No, thanks"); "Not connected" anywhere |

## Beat 3 · 0:40–1:15 · The Meta API

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 3a | F03 · Meta API | — | "Meta API panel", "6 model invocations"; six rows: Proposer, Critic, Evidence Evaluator, Proposer, Critic, Evidence Evaluator, each "muse-wonder-1.0 via meta" with its recorded request id and latency; the totals box: input 6,874 and output 31,910 "(reported by 6 of 6 calls)"; the three notes under it ("Request ids are the ones the runtime recorded …", "No row carries the output bound, the attempt count or the schema and prompt digests …", "No cost is shown …") | a caption or narration about attempts or cost |
| 3b | `/en/showcase/` → M04 (group `live-battery-2026-10-30-p13`) | loaded | header: Outcome "None: the run failed", "29 records"; the box "The run failed · A model document broke the deliberation protocol and was refused. · Stage evaluate, code evaluation_contract."; the badges | the word "blocked" in a caption (M04 failed; it was not blocked by the policy) |
| 3c (optional, 3 s) | M04 · Meta API | — | 6 invocations, the last one on the failure record; totals input 9,849, output 22,544, reasoning 19,030 | — |

Caption 3a: the F03 label. Caption 3b–3c: "Recorded run M04 · 30 Oct 2026 ·
from the research archive · not live".

## Beat 4 · 1:15–1:45 · The deliberation

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 4a | `/en/new/` · Guided examples | — | the intro ("The roles are scripted doubles and the evidence is simulated: no model is called and no proof is real. …") and the card "Reference: retry budget" with SCRIPTED · SIMULATED evidence; click its Run | the live-mode-off line |
| 4b | guided run · Playback | pacing on | Playback tab; header SCRIPTED · SIMULATED; "simulated · paced for readability"; the Proposer, Critic and Evidence Evaluator columns filling in (proposal, "constraint conflict on claim-2", "not corroborated", "contradicted by ev-constraint", then the revision) | LIVE anywhere |
| 4c | F03 · Decision | — | "contested" with "Evidence: HYPHAE RECALL"; "This recommendation is for revised proposal prop-12, which replaced the original proposal prop-1." with the task; "Why the policy recommends this"; the rationale "Contested: 5/5 claims corroborated; proofs: 6 valid, 0 invalid, 0 unavailable/error; 6 unresolved challenges."; "Open objections" (3 missing test, 2 unsupported claim, 1 security risk); "Earlier-round objections never re-judged: 5." | `claim-15` in a close-up, unless the caption 4b′ is on screen |

Caption 4a–4b: "Guided example · scripted roles · SIMULATED evidence · no model
call". Caption 4c: the F03 label, and, for as long as the claims of `prop-12`
are in frame, "4b′: one cited record misdescribes the fixture test
(docs/evaluation.md, Corrections 3)".

If the never-re-judged count differs from 5, stop and report it.

## Beat 5 · 1:45–2:10 · Evidence and proofs

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 5a | F03 · Evidence | — | the retrieved records with integrity valid and the Evaluator's per-round reading (contradicted in round 1, corroborated in round 2) | — |
| 5b | F03 · Proof inspector | — | a valid proof in plain words: what was checked and what it does not establish; the sentence "Verified at run time on … by hyphae proof verify; the artifacts are held privately by the operator (witnesses contain the full store), so the public can re-verify the event chain, not these proofs." in full | that sentence cropped |

## Beat 6 · 2:10–2:30 · Chain check

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 6a | F03 · Audit trail | before the check | "The chain": first record 64, last record 91; "Check the chain in this browser" and its button | — |
| 6b | same | after "Check the chain" | "Chain checks out"; "All 189 records hash and link as recorded, and this run's segment matches what the views hold."; the five "matches" lines (file SHA-256 `3d647ed9…` (292,322 bytes) against the manifest's pin; 189 records checked; sequences 64 to 91 (28 records); the head `629a2fb5…`; record count against the receipt's (28)); the limits paragraph | the limits paragraph scrolled away before it is read |

## Beat 7 · 2:30–2:50 · A person decides

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 7a | `/en/runs/` → the guided run · Decision | awaiting the decision | "Your decision" with the notice "Decisions on this public portal carry no operational authority: approving or rejecting records your choice on this run and executes nothing."; Approve and Reject; below, "supported" with "Evidence: SIMULATED" and the scope line "This recommendation is for revised proposal prop-2, which replaced the original proposal prop-1." | a typed note |
| 7b | same | Approve clicked | "Decision recorded: approved by visitor:… on …"; "This decision carries no operational authority: it records a visitor's choice on a simulated run."; header "Your decision: Approved", 15 records | — |

## Beat 8 · 2:50–3:00 · Close

| # | Screen | Must be visible |
|---|---|---|
| 8a | `/en/` top | the Quorum logo and "Hyphae Consensus"; "Verifiable multi-agent reasoning and decision accountability" |
| 8b | end card (editor) | the four lines of `script.md`, beat 8 |

## Never on screen

The terminal, the address bar (record in full screen, see the checklist), the
operator console, any environment variable, key or token, the Docker or file
explorer windows, browser notifications, the legal pages (Terms, Privacy,
Limitations: their texts are drafts, `LEGAL_TEXT_STATUS` in
`apps/web/src/lib/site.ts`), and the New deliberation line saying live mode is
not enabled.
