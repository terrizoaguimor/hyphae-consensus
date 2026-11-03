<!-- SPDX-License-Identifier: Apache-2.0 -->

# Heuristic evaluation, 2026-10

A summary of the heuristic evaluation that started the redesign: what was examined, the defects it found, what was done about each and what is not shown to be done. The full evaluation is about 58 KB of working notes with screenshot references and is not published; this page keeps only claims those notes and the repository history support. "HE #n" is a problem's rank in the evaluation's top 25.

Contents: [Method](#method) · [Defects fixed in code](#defects-found-and-fixed-in-code) · [Defects answered by the redesign](#defects-answered-by-the-redesign) · [What the repository does not show](#what-remains-or-is-not-shown) · [Conflicts](#evaluator-suggestions-that-were-not-followed)

## Method

- **Input:** 230 screenshots of the web app as built at commit `e4e4c73` (the head of the portal branch on 2026-10-30, before any redesign), covering 95 screens and states in dark and light, a Spanish subset and a 390 px mobile set. They were taken over the end-to-end suite's mocked API (contract fixtures and the real archived bytes of `research/`), so what they show is the app's own fixtures, not the live site.
- **Technique:** the screens were split into six groups, evaluated separately and then merged and de-duplicated (overview and new deliberation; runs and playback; evidence, proofs, audit, model calls; decision, failures and errors; research showcase and recorded runs; docs, settings, legal, 404, mobile).
- **Lens:** Nielsen's ten heuristics, plus checks the owner asked for: a first sentence free of jargon, one obvious primary action, status legible at a glance, honesty labels *understood* and not only present, noise, hierarchy, alignment, rhythm, density and shapes (pills, ovals, circles). Three personas: a hackathon judge with three minutes, a product manager who is not an engineer, and a security-minded engineer.
- **Severity:** *Critical* breaks a binding product rule (honesty labels, a simulated proof never looks verified, integrity is not truth, the no-authority notice, WCAG AA) or hides the core action. *Major* misleads or stops a persona on a main task. *Minor* is friction.
- **Measured and estimated:** contrast and page widths marked "measured" came from the PNG pixels or the rendered page; other values were visual estimates. Capture artifacts (a navigation background that stops at 900 px, a "Connection lost" line from the mock's stream, identical fixture timestamps) were not counted as defects. Fixture contradictions *were* counted where the UI showed them unreconciled, since a real inconsistent run would look the same.

Result: 25 ranked problems (13 critical, 12 major) and a per-screen list.

## Defects found and fixed in code

Fixes landed on 2026-10-31 in "round 4", as parallel lanes merged onto `e4e4c73`. The merge commit names the problems it answers.

| HE | Severity | Defect found | Fix | Commit |
|---|---|---|---|---|
| 1 | Critical | **Simulated work painted green.** "supported — on SIMULATED evidence" was a solid green pill and "corroborated" claims were green on runs where nothing was checked. Green reads "verified" before the word SIMULATED is read | Supported is an informational tint with a `circle-dot` icon, never green, never a check mark; one colour per provenance label | merge `e6cbbce` (lane commit `62a9031`) |
| 2 | Critical | **Simulated proofs counted as "valid" or "checked":** "proofs: 2 valid" in a rationale, "2 proofs checked" beside "no cryptography checked", "1 valid, 1 invalid" on a simulated blocked run, a receipt's "integrity valid: 4" with SIMULATED only on the next line | Counters read as simulated proofs; the receipt prints SIMULATED on the same line; a caption under server text that says "valid" for a simulated proof | merge `e6cbbce` |
| 16 | Major | **One colour meant many things, and one label had several colours:** yellow was objection, insufficient evidence, not connected and "changed"; META MODEL changed colour between screens; LIVE shared the green of "supported" | One colour per provenance label, then removed entirely by the monochrome system | merge `e6cbbce`; superseded by round 5 |
| 3 | Critical | **The outcome tag truncated exactly the honesty qualifier:** "supported — on SIMULAT…", "blocked — on SIMULATE…"; in Spanish "respaldada — con eviden…" lost SIMULADO | Outcome and evidence are two tags that wrap and never truncate (`OutcomeTags`), in all four locales | merge `c626a55` (lane commit `1721e4c`) |
| 24 | Major | The runs list could not be scanned; "on NO EVIDENCE evidence" and, in Spanish, "con evidencia SIMULADO" | The template became `Evidence: {label}`; "awaiting your decision" a status tag | merge `c626a55` |
| 4 | Critical | **The human decision was hidden:** the 7th tab, Approve and Reject about 75×30 px and 1,800 to 3,000 px down the page | "Go to your decision" in the run header, a prominent decision card | merge `73839cc` |
| 12 | Critical | **On blocked and abstained runs the form invited the override:** Approve stayed the primary button and "Required to approve a blocked run" was 12 px helper text | Equal-weight buttons, the override requirement before the click, override wording | merge `73839cc` |
| 5 | Critical | **Integrity failures did not look like failures and did not travel:** a failed chain check stayed inside the Audit panel while the header kept a green "supported" | A persistent integrity callout travels to the header and the decision card | merge `b40481c` |
| 7 | Critical | **Contradictory provenance shown unreconciled:** a LIVE tag over "scripted double, no model call"; META MODEL over four "SIMULATED proof" rows | A page-computed "This page found labels that disagree with the run's records" callout (it appears on the mock's sealed-snapshot fixture, not on the screenshots in this folder) | merge `b40481c` |
| 6 | Critical | **Statuses contradicted each other on one screen:** during paced replay the header said "approved / supported" while the stage said "No recommendation issued yet"; after a rejection a grey "rejected" sat next to a green "supported" | Status line and header follow one clock; guided cards say SCRIPTED instead of "SIMULATED model" | merge `3b44b49` (`2b07292`, `d77caf1`) |
| 8 | Critical | **Pages misreported their own state:** copy said "Once connected, the portal will…" under "Connected to the API" | Copy variants for connected and not-connected states | merge `111e25c` (`22252c3`) |
| 18 | Major | **Error states had the wrong copy and no way forward:** one headline for a 500, a 503 and a 429 | Error headlines by class | merge `111e25c` |
| 9 | Critical | **Horizontal overflow on a phone:** the decision page and the recorded decision page rendered **437 px wide on a 390 px viewport (481 px in Spanish)**, measured. The revision comparison's four-column claims table set the minimum width of its block; claim ids broke mid-word | Below 672 px the claims table is one block per claim; `min-width: 0` on the panel's children; ids never wrap. A review of that fix then found the Explorer tab measured **600 to 621 px** on a 390 px phone (guided and recorded runs); its table now scrolls inside a focusable region | merge `00d8fd5` (`b8cc86a`, `22ea650`) |
| 10 | Critical | **Non-text contrast failed WCAG 1.4.11 (measured):** unselected radio rings 1.43:1 light and 2.09:1 dark, Settings card borders 1.48 and 2.22, the off switch track 1.33 | A stronger border token on controls (round 4 used a slate value; the token `--hc-color-border-strong` now resolves to `#6E6E6E` light and `#9E9E9E` dark, `mono/6` and `mono/5`) | merge `00d8fd5` |

Overflow came back in a different form once the layout changed, and was fixed again and tested: on 2026-11-02 the run stage's three role cards went three-up at a 1024 px viewport, leaving about 210 to 267 px per card, and the page grew to 1137 px at 1024 and 417 px at 390 and 360 px (`f6b91ed`); a drawer control ran past its frame at 320 px in three locales and some button labels were cut (`37f8cfc`). `apps/web/e2e/responsive.spec.ts` now measures every screen at 1920, 1440, 1280, 1024, 768, 390 and 360 px in English and Spanish and fails on any document wider than the viewport or text cut by an overflow-hidden box. The commit that closed it also covers 320 px (`cdfe1e5`).

## Defects answered by the redesign

Three families were answered by the redesign's system rather than by a numbered fix:

- **Pills, ovals, circles and soft nested frames (HE #17):** the square-first radii, square step markers, a segmented theme control and a segmented tour bar (merge `4436188`, "round 5 shapes"), with a radius scan in the test suite.
- **Colour doing the work of status (HE #1, #16 in their final form):** the monochrome foundation, hue-free status tags with a hatch for simulated, and the LIVE square (merges `888c618`, `3c2d516`).
- **Non-text contrast across the system:** 740 of 740 required pairs pass in the design's checker.

## What remains, or is not shown

The repository history cites HE numbers only for the rows above. For the rest, this page makes no claim that they were fixed, because no commit or note states it:

- **HE #11** (the revision story is lost in playback), **#13** (the injection demo shows no injection), **#14** (overloaded run header, generic page title), **#15** (honesty labels present but not understood), **#19** (four treatments for not-connected states), **#20** (no single primary action), **#21** (the audit trail buries its demo moment), **#22** (repetition and density), **#23** (missing data looks like data), **#25** (long legal pages without structure, an unguarded reset).
- Some of these changed with the redesign, and in the screenshots in this folder the run header does open with an answer sentence and a "Go to your decision" action, and the provenance strip sits under the title. Others (for example #25's unguarded reset) were not re-examined for this page. Read this list as "not verified here", not as "open".
- **Honesty labels in the other languages.** The evaluation found the labels translated although the inventory called them literal; the owner decided to translate them ([README](README.md#honesty-labels-in-four-languages)).
- **Not covered by the evaluation:** it looked at screenshots over a mock. It did not test a screen reader, a real device, Firefox or Safari rendering of dashed and double lines (an open item in the design notes), or the live site. No user was tested.

## Evaluator suggestions that were not followed

Where an evaluator proposed something that contradicted a binding rule, the rule stood and the finding was fixed another way.

| Suggestion | Kept instead | Why |
|---|---|---|
| Make Reject the primary button, and Approve a secondary "Override and approve…", on blocked runs | Approve and Reject at equal weight | Buttons must not take the policy's side. Fixed with a "required for this run" note before the click and an override label |
| A decision bar on the Playback tab | Decision buttons only in the decision card under the no-authority notice | The notice must sit directly above the buttons. Fixed with "Go to your decision" |
| Drop THIS SERVER from list rows to cut noise | Provenance labels are never hidden | Noise is reduced by saying a caveat once, never by dropping a tag |
| Outline "valid at run time" proofs on recorded runs | Verified at run time stays the verified solid | The caveat about operator-held artifacts is said once at the top of the tab |
| LIVE in red | A neutral LIVE role with a square marker | Became moot with the monochrome system; LIVE shares no look with "supported" |
| Draft notices and placeholders in amber | Draft and "Pending" are informational | Avoids alarm fatigue |
