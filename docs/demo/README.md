# Demo package

The demo video and everything needed to record it honestly. Rewritten on
2026-10-31 against the frozen UI at `a657531` (monochrome, brushed glass,
Quorum logo), for a **local recording** on the development machine. **Nothing
has been recorded yet.** No staging deployment and no live Meta call are
needed: the cut shows a guided run (scripted roles, SIMULATED evidence) and two
recorded runs of the research showcase (F03 and M04, RECORDED), each labelled
as the UI labels it.

| File | What it holds |
|---|---|
| [`impact-story.md`](impact-story.md) | The impact story: run F03, where the Critic and the Evaluator caught the claim a single Proposer call would have shipped; three sentences, a 60-word version for the submission form, and where each fact is in `research/`. |
| [`script.md`](script.md) | The script, beat by beat (3:00 target; the rules set no length), with the narration, the numbers on each screen and their sources. |
| [`shot-list.md`](shot-list.md) | The Mission Control screens per beat: route, tab, state, what must be visible and what must not appear. |
| [`recording-checklist.md`](recording-checklist.md) | The local server, the browser window (1920 × 1080, full screen, 125 % zoom, dark theme), the order of clicks, the stop rules, editing, the honesty checks and the upload. |
| [`captions-en.srt`](captions-en.srt), [`captions-es.srt`](captions-es.srt) | Subtitles timed to the script's beats (English word for word; Spanish translation). Re-time them to the final cut. |
| [`console-storyboard-2026-10-26.md`](console-storyboard-2026-10-26.md) | History: the storyboard written for the loopback operator console. Not to be recorded. |

## Rules this package keeps

- A scripted or simulated run is never called live; a recorded run is never
  presented as running. LIVE does not appear in this cut.
- Every figure said or shown comes from a named run in `research/` or a section
  of `docs/evaluation.md`, as recorded, or is rendered by the portal from the
  run on screen. No figure is rounded or upgraded, and no rate of any outcome
  is claimed.
- Integrity is not truth: a `valid` proof says a record matches its anchored
  snapshot, not that its content is true (`research/README.md`, "What cannot
  be verified from these files").
- Never delete or write to `demo-run/`; it holds the original research data.
