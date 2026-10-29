# Demo package

The 3-minute video and everything needed to record it honestly. Written on
2026-10-29 against branch `feat/public-portal` at `bc3b262`. **Nothing has been
recorded yet**, and nothing is deployed.

| File | What it holds |
|---|---|
| [`script.md`](script.md) | The video script, beat by beat, with timings, narration, the figures each beat may state and the run or section each figure comes from. Every beat that depends on unfinished work is marked, with its fallback. |
| [`shot-list.md`](shot-list.md) | The Mission Control screens per beat: route, tab, state, and what must be visible (provenance labels, flags, the trace) or must not appear. |
| [`recording-checklist.md`](recording-checklist.md) | Prerequisites and owner actions: staging, the live preset runs on staging, the honesty checks before publishing, captions, length, the unlisted upload. |
| [`console-storyboard-2026-10-26.md`](console-storyboard-2026-10-26.md) | History: the storyboard written for the loopback operator console, moved here from `docs/demo-script.md` and kept as written. Not to be recorded. |

## Two cuts, one script

- **Planned cut.** A live preset run of the public portal, recorded on the
  Access-protected staging deployment (real `delivery=live` frames, shown as a
  labelled time-lapse), plus recorded runs of the research showcase.
- **Fallback cut.** Guided mode (scripted roles, SIMULATED evidence) + the
  research showcase (RECORDED) + a recorded battery run captioned as a replay.
  It needs no staging, no Meta key and no Hyphae container: the public server
  with the static export and `research/` is enough
  (`recording-checklist.md`, "Fallback cut").

Every beat has a fallback, and no beat depends on a lucky outcome: the
narration of a live run is chosen after the run, from what its Decision tab
shows (`script.md`, beat 4).

## Open work this package depends on (as of 2026-10-29)

| Work | Needed by | Status |
|---|---|---|
| Sealed Hyphae snapshot capture (`docs/architecture/sealed-snapshot.md`) | live mode on staging (beats 3–5, planned cut) | captured 2026-10-29 (`b0375df`, digests in the README); the runtime boots it with `0515f29` (merged as `7e26866`); not on the server |
| Counted protocol 1.3 battery on `prompt-freeze-2` (`docs/evaluation.md` §6) | the live presets shown on staging; any 1.3 figure in the video | pending; only the smoke runs K01–K04 exist, not counted as validation; not run on 2026-10-29 (no Meta key on the development machine; `docs/evaluation.md` §6.3) |
| Staging deployment (`docs/deployment/runbook.md`) | the planned cut of beats 3–5 | pending |
| Review of the in-browser chain check (web lane W2, merged at `bc3b262`) | beat 5 in the browser; otherwise the Rust CLI | adversarial review running |
| Legal sign-off of the portal texts | publishing a video that shows the Terms, Privacy or Limitations pages | pending (`docs/security/release-checklist.md`, G10) |

## Rules this package keeps

- A scripted or simulated run is never called live; a recorded run is never
  presented as running. LIVE appears only where the portal itself shows it.
- Every figure said or shown comes from a named run in `research/` or a section
  of `docs/evaluation.md`, as recorded, or is rendered by the portal from the
  run on screen. No figure is rounded or upgraded.
- Integrity is not truth: a `valid` proof says a record matches its anchored
  snapshot, not that its content is true (`research/README.md`, "What cannot
  be verified from these files").
- Never delete or write to `demo-run/`; it holds the original research data.
