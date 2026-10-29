# Shot list (Mission Control)

Screens per beat of [`script.md`](script.md). Routes, tab names and labels were
checked against `apps/web/src` and `apps/web/messages/en.json` at `bc3b262`.
Record in English (`/en/`), light theme unless stated, browser zoom at 100 %,
window at least 1280 px wide so the run header's provenance badges do not wrap
off screen.

## Routes and tabs

| Screen | Route | Notes |
|---|---|---|
| Overview | `/en/` | "How a deliberation works", provenance labels, "Impact metrics" (computed in the browser from `GET /api/v1/showcase`) |
| New deliberation | `/en/new/` | "Guided examples" and "Live presets" |
| My runs | `/en/runs/` | this browser session's runs |
| Research showcase | `/en/showcase/` | the archived runs, each opening in the run workspace in RECORDED mode |
| Run workspace, own run | `/en/run/?id=<run_id>` | tabs: Live or Playback, Explorer, Evidence, Proof inspector, Audit trail, Meta API, Decision; opens on Live/Playback |
| Run workspace, recorded run | `/en/run/?id=<run_id>&source=showcase` | same tabs without Live/Playback; opens on Decision; read-only |
| Docs article | `/en/docs/verified-evidence/` | "Verified evidence" |

The stage tab reads **Live** only while the run deliberates and its newest frame
is LIVE (`isLive` in `apps/web/src/lib/provenance.ts`); otherwise it reads
**Playback**. A recorded run has no stage tab.

Recorded runs used here (ids from `research/MANIFEST.json`):

| Label | Run id | Log |
|---|---|---|
| F03 | `run-d6f28bca-0a10-4fcf-9257-4cfb7374280c` | `research/2026-10-27/live-battery-2026-10-27-final.jsonl` |
| R06 | `run-d4d214a4-0532-4b44-9a98-715bd29d81da` | `research/2026-10-27/live-battery-2026-10-27.jsonl` |

## Labels that must stay in frame

| Run kind | Header provenance badges (expected) | Also visible |
|---|---|---|
| Guided run | SCRIPTED · SIMULATED · THIS SERVER · delivery STREAMING or REPLAY, never LIVE | Playback tab; "simulated · paced for readability"; outcome "supported — on SIMULATED evidence"; proofs as SIMULATED, never a green "valid" |
| Recorded run (F03, R06) | META MODEL (warning colour) · HYPHAE RECALL · RECORDED · flag MODE UNDECLARED | the "Recorded run, read-only" section with the banner "Run <label> of battery <battery>, shown from the research archive: nothing streams, nothing runs, and no decision can be recorded here." |
| Live run on staging | LIVE META · SEALED SNAPSHOT · THIS SERVER · LIVE for frames that arrive while the page is open; after a reload the stored frames read REPLAY and the model META MODEL, with "Meta model · recorded <date>" | the Live tab while it deliberates (Playback once it rests); the burned-in time-lapse caption |

The MODE UNDECLARED flag on recorded runs is expected: those runs were recorded
before schema 1.3 and declare no mode (`Provenance.flagNotes.mode_undeclared`).
Do not crop it or cover it with a caption.

## Beat 1 · 0:00–0:25 · Impact first

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 1a | `/en/showcase/` | archive loaded, manifest check done | the count line ("… recorded runs (… records), as the server lists them from the archived chains"; `research/README.md`: 44 runs, 790 events), the `live-battery-2026-10-27-final` battery group with F01–F06, all contested | — |
| 1b | F03 recorded · Evidence | loaded | claim 2 of the first proposal ("…set to exactly 10 attempts."), marked contradicted; the constraint record "Constraint: outbound retries must be capped at 3 attempts with jitter; …"; the RECORDED badge in the header (scroll or split-screen so it is seen at least once); the burned-in caption "Demo-repo fixture · 7 seeded records" | the constraint-test fact (record 5) as the focus of the shot (it misdescribes the fixture test, Corrections item 3) |

## Beat 2 · 0:25–0:55 · Architecture

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 2a | `/en/` | API connected | "How a deliberation works": Three roles deliberate · The runtime checks evidence · A fixed policy recommends · A person authorizes | — (this section is static and renders whether or not the API answers) |
| 2b | `/en/` | scrolled | the "Provenance labels" legend (SIMULATED, RECORDED, LIVE META, …); "Impact metrics" with its figures if it passes through the frame | the "Not connected" placeholder in place of the Impact metrics (means the API is not reachable: fix before recording) |
| 2c | `/en/docs/verified-evidence/` | — | the article title, one second | — |

## Beat 3 · 0:55–1:45 · A live preset run

Planned cut **[PENDING: STAGING-REC]**:

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 3a | `https://<staging-host>/en/new/` · Live presets | live mode on, budget readable | "Public live budget" (Available now, Left today (UTC), Left in total); the preset "Raise payment retry attempts to 10"; the attestation checkbox; the Turnstile widget | the Access login page, a judge invite code, any terminal, the address bar if it shows anything but the hostname |
| 3b | `/en/run/?id=<run_id>` · Live | deliberating | Live tab label; header badges LIVE META · SEALED SNAPSHOT · THIS SERVER · LIVE; the role columns filling in; the burned-in caption "Recorded on staging, <date> · time-lapse ×N" for the whole sped-up stretch | the Live tab label after the run rests (it must read Playback), or any LIVE label after a reload of the page (stored frames must read REPLAY); either one is a G7 defect: stop and report it |
| 3c | same · Evidence | after the first evaluation | the contradicted claim (if any) with its record and the record's integrity status | — |
| 3d | same · Proof inspector | — | the plain view of that record's proof: what was checked and what it does not establish | — |

Fallback cut:

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 3a′ | `/en/new/` · Guided examples | guided enabled | the intro ("The roles are scripted doubles and the evidence is simulated: no model is called and no proof is real. …"), the SIMULATED model and SIMULATED evidence badges, "Reference: retry budget" with its expected outcome | — |
| 3b′ | `/en/run/?id=<guided run_id>` · Playback | pacing on | Playback tab label; SCRIPTED · SIMULATED badges; "simulated · paced for readability" | the word LIVE anywhere |
| 3c′ | F03 recorded · Explorer | loaded | first proposal → challenge (with `constraint_conflict`) → evaluation; burned-in caption "REPLAY · recorded run F03, 27 Oct 2026 · not live" | — |
| 3d′ | F03 recorded · Evidence, then Proof inspector | — | claim 2 contradicted; the constraint record's proof: valid, verified at run time; the sentence "… the artifacts are held privately by the operator …, so the public can re-verify the event chain, not these proofs." in full | that sentence cropped or cut short |

## Beat 4 · 1:45–2:20 · Decision center

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 4a | live run · Decision **[PENDING: STAGING-REC]** | at rest, awaiting a decision | the outcome badge with its evidence label ("<kind> — on SEALED SNAPSHOT evidence"); for a revision, the scope line "This recommendation is for revised proposal …, which replaced the original proposal …" with the task and both proposals; "Why the policy recommends this" with its counts; "Open objections" | the word `supported` without its scope line, if the recommendation is for a revision (cut wider, never tighter) |
| 4a′ | F03 recorded · Decision (fallback) | — | "contested — on HYPHAE RECALL evidence"; the counts (5 claims, 5 corroborated; 6 objections unresolved); the open objections with their kinds; "This run was recorded before protocol 1.3 …" and "Earlier-round objections never re-judged: 5." | `claim-15` (it restates seeded record 5, which misdescribes the fixture test), unless the record-5 caption of 4b is on screen with it |
| 4b | R06 recorded · Decision | — | "supported — on HYPHAE RECALL evidence"; "This run was recorded before protocol 1.3: each round's evaluation judged only that round's objections, and the policy read the final round only."; "Earlier-round objections never re-judged: 7."; the archive's note on the log (MANIFEST.json); the burned-in caption "Cited record 5 misdescribes the fixture test: docs/evaluation.md, Corrections 3" for as long as the tab is on screen (the portal does not show this caveat) | the outcome badge alone in a close-up: the never-re-judged line must be in the same frame |

If the never-re-judged count on screen differs from `research/README.md` (F03 5,
R06 7), stop and report it; do not record over it.

## Beat 5 · 2:20–2:45 · Audit trail and the Meta API

| # | Route · tab | State | Must be visible | Must not appear |
|---|---|---|---|---|
| 5a | R06 recorded · Audit trail **[PENDING: CHAIN-BROWSER]** | after "Check the chain" | "Chain checks out" and its sentence ("All … records hash and link as recorded, and this run's segment matches what the views hold."); the scope note that the run is one segment of a multi-run log; the file SHA-256 line ("… against the digest the archive's manifest pins."); the limits note ("… The chain is unsigned: a log rewritten and re-hashed as a whole would check out too, …") | the limits note cropped away |
| 5a′ | terminal (fallback) | — | the `consensus-api verify … --run run-d4d214a4-…` command and its output, nothing else | the prompt's user name or machine path, other windows, environment variables |
| 5b | live run · Meta API (fallback: R06 · Meta API) | — | one row per call: role, model via provider, recorded request id, latency, input/output/reasoning tokens; the totals; "No row carries the output bound, the attempt count or the schema and prompt digests: the public surface does not publish them."; "Request ids are the ones the runtime recorded for Meta's API; the logs are unsigned, so they show what was recorded, not that Meta returned it." | any narration or caption naming attempts |
| 5c | live run · Decision (fallback: the guided run) | Approve clicked | "Decisions on this public portal carry no operational authority: approving or rejecting records your choice on this run and executes nothing."; "Decision recorded: approved by … on …" | a note with personal data |

## Beat 6 · 2:45–3:00 · Close

| # | Screen | Must be visible |
|---|---|---|
| 6a | `/en/` header | "Verifiable multi-agent reasoning and decision accountability" |
| 6b | end card (editor) | "Independent project; not affiliated with, sponsored by, or endorsed by Meta."; the footage line of `script.md` beat 6; the repository link once the owner has made it public |
