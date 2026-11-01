# Recording checklist (local recording)

Rewritten 2026-10-31 against `a657531` for the recording of 2026-11-01: the UI
runs on the development machine, served by `consensus-api serve-public` on
loopback. The cut shows a guided run started in the session (scripted,
SIMULATED) and two recorded runs of the research showcase (F03, M04). **No live
Meta call is needed or made**, no Meta or Hyphae variable is set, no staging,
no Turnstile, no Hyphae container. Nothing has been recorded yet.

Steps marked **(owner)** are the owner's to do or to approve.

## 1. Before the session

- [ ] `demo-run/` is backed up off-machine and nothing writes to it. The video
      needs nothing from it.
- [ ] `research/` checks out clean: in `research/`, `sha256sum -c SHA256SUMS`
      passes, and `git status research/` is clean (the showcase refuses a file
      whose SHA-256 differs from `research/MANIFEST.json`).
- [ ] Build the commit being recorded and write its sha down (end card,
      submission):
      ```sh
      export HC_WEB_NODE_MODULES_VOLUME=hcw-recording
      sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
      sh apps/web/scripts/in-docker.sh pnpm build     # static export in apps/web/out
      env -u META_API_KEY cargo build -p consensus-api --locked
      git rev-parse HEAD
      ```
- [ ] The figures of `script.md` still match `docs/evaluation.md` §5.3 and §6.3
      and `research/README.md` at that commit. If a section changed, update
      `script.md` and the captions first.

## 2. Start the local server

From the repository root, in a terminal that stays off screen. The run store is
a new empty directory outside `demo-run/` and `research/`; no Meta or Hyphae
variable is set, and `CONSENSUS_LIVE_ENABLED` is left unset (live mode off by
default, `crates/consensus-api/src/live/config.rs`).

```sh
STORE=$(mktemp -d)
env -u META_API_KEY \
  CONSENSUS_PUBLIC_BIND=127.0.0.1:8788 \
  CONSENSUS_WEB_ROOT=apps/web/out \
  CONSENSUS_RESEARCH_DIR=research \
  CONSENSUS_RUN_STORE_DIR="$STORE" \
  CONSENSUS_COOKIE_SECURE=false \
  target/debug/consensus-api serve-public
```

(`target/debug/consensus-api.exe` on Windows; the variables are documented in
`crates/consensus-api/src/public_config.rs`.)

- [ ] `http://127.0.0.1:8788/en/` shows "Connected to the API: schema 1.3,
      policy 1.2, contract …" under the buttons, and "Impact metrics" with
      figures, not "Not connected".
- [ ] `http://127.0.0.1:8788/en/showcase/` reads "48 recorded runs (919
      records), as the server lists them from the archived chains." and the
      manifest check says "Matches".
- [ ] `http://127.0.0.1:8788/en/new/` lists "The server lists 5 guided
      scenarios." (the line that live mode is not enabled stays off camera).

## 3. Browser and window

- [ ] Chrome (or Edge) with a **fresh profile**: no extensions, no bookmarks bar,
      no autofill, notifications off. For example
      `chrome --user-data-dir="$(mktemp -d)" --window-size=1920,1080 --force-device-scale-factor=1 http://127.0.0.1:8788/en/`.
- [ ] **Window 1920 × 1080**, then full screen (F11) so the address bar and tabs
      are not recorded. Record the screen at 1920 × 1080, 30 or 60 fps.
- [ ] **Browser zoom 125 %** (Ctrl + plus, twice from 100 %). At 100 % the
      text is small in a 1080p video; at 125 % the layout keeps its sidebar
      and the run header's badges stay on one line.
- [ ] **Theme: dark.** Click the moon button at the right of the header (the
      three buttons are system, light, dark; the active one shows a check). Dark
      shows the brushed-glass surfaces best. The choice is kept by the browser.
- [ ] **Language: English** (the "English" link in the header is underlined).
- [ ] On the Overview, click **"No, thanks"** on "New here? Take the one-minute
      tour" so the invitation does not appear in the video.
- [ ] Reduced motion is **off** in the operating system, so the guided run's
      Playback is paced ("simulated · paced for readability").

## 4. Order of clicks

Record each numbered take as its own clip; the edit puts them in script order.
Keep each take at normal speed. Wait two seconds of stillness at the start and
end of each take.

**Take A · guided run (beats 4 and 7).** Starts the run the decision needs.

1. `http://127.0.0.1:8788/en/new/`. Hold on "Guided examples" and the intro.
2. On "Reference: retry budget", click **Run**. The run workspace opens on
   **Playback**. Do not click "Skip pacing"; let it play to the end (header:
   outcome "supported", "Evidence: SIMULATED", "This run is waiting for your
   decision.").
3. Write down the run id shown under "Run id" (for the end-card notes).
4. Do **not** decide yet. Leave the run.

**Take B · Overview (beats 2 and 8).**

5. `http://127.0.0.1:8788/en/`. Hold on the title, then scroll slowly to "How a
   deliberation works" and hold on the four cards for 15 s. Stop scrolling
   with the cards at the bottom of the frame: the "Provenance labels" legend
   right below them draws a LIVE META badge and must stay out of frame (if it
   enters, retake; it is a legend, not a run, but this cut shows no LIVE).

**Take C · F03 (beats 1, 3, 4, 5, 6).** `http://127.0.0.1:8788/en/showcase/`.

6. Hold on the count line, scroll to the group `live-battery-2026-10-27-final`,
   click **F03 · Raise payment retry attempts to 10**. (Direct URL if the scroll
   is long: `http://127.0.0.1:8788/en/run/?id=run-d6f28bca-0a10-4fcf-9257-4cfb7374280c&source=showcase`.)
7. Hold on the header: META MODEL · HYPHAE RECALL · RECORDED · MODE UNDECLARED
   and "Recorded run, read-only".
8. **Explorer** tab: scroll to the first `proposal_submitted` (`prop-1`) so
   `claim-2` and `claim-3` are readable; hold 5 s.
9. **Evidence** tab: the constraint record and its round-1 reading
   (contradicted, integrity valid); hold 5 s; then scroll through round 2.
10. **Meta API** tab: the six rows, then the totals box; hold 10 s.
11. **Decision** tab: the outcome, the scope line, "Why the policy recommends
    this", the rationale, "Open objections" and "Earlier-round objections never
    re-judged: 5."; scroll slowly, 15 s.
12. **Proof inspector** tab: a valid proof in plain words and the "held
    privately by the operator" sentence; hold 8 s.
13. **Audit trail** tab: "The chain" (first record 64, last record 91), then
    "Check the chain in this browser" → click **Check the chain**. Hold on
    "Chain checks out", the five "matches" lines and the limits paragraph,
    10 s.

**Take D · M04 (beat 3).**

14. `http://127.0.0.1:8788/en/showcase/`, group `live-battery-2026-10-30-p13`,
    click **M04 · Remove the retry cap from the payment client** (direct URL:
    `http://127.0.0.1:8788/en/run/?id=run-c912672a-f72c-4f7e-bf5c-1d2bcf91f451&source=showcase`).
    Hold on the header: "None: the run failed", "29 records", the box "The run
    failed · A model document broke the deliberation protocol and was refused.
    · Stage evaluate, code evaluation_contract."; 8 s.
15. Optional: **Meta API** tab, totals; 3 s.

**Take E · the decision (beat 7).** Last, because it changes the guided run.

16. `http://127.0.0.1:8788/en/runs/` → the guided run of take A → **Decision**
    tab. Hold on "Your decision" and the notice "Decisions on this public portal
    carry no operational authority: …", 4 s.
17. Type no note. Click **Approve**. Hold on "Decision recorded: approved by
    visitor:… on …" and "This decision carries no operational authority: it
    records a visitor's choice on a simulated run.", 6 s.

If take E goes wrong (wrong button, a typed note), start a new guided run (take
A again) instead of showing the first one.

## 5. Stop rules during the session

Stop and report, do not record over it, if any of these appears:

- LIVE anywhere (badge, tab or caption): nothing in this cut is a live run.
- A figure that differs from `script.md` (for F03: 6 calls, 6,874 / 31,910
  tokens, 5/5 corroborated, 6 open, "never re-judged: 5", 189 records,
  sequences 64 to 91; for M04: 29 records, `evaluation_contract`).
- A green or "valid"-looking proof on the guided run (its proofs must read
  SIMULATED).
- "Not connected", an error box, or the manifest check reading "Differs".

## 6. Editing

- [ ] **Captions burned in**, one per sequence, for as long as the sequence is
      on screen:
      - guided run: "Guided example · scripted roles · SIMULATED evidence · no
        model call";
      - F03: "Recorded run F03 · 27 Oct 2026 · from the research archive · not
        live";
      - M04: "Recorded run M04 · 30 Oct 2026 · from the research archive · not
        live";
      - beat 1c, second line: "Demo project · 7 seeded memory records";
      - beat 4c, second line, for the whole F03 half of beat 4 (1:30–1:45):
        "One cited record misdescribes the fixture test (docs/evaluation.md,
        Corrections 3)".
      The M04 caption starts at the cut to M04, 0:55.
- [ ] **Speed.** Everything at normal speed. If a stretch of the guided
      Playback is shortened, it carries "sped up ×N" (N = recorded ÷ shown,
      rounded down) for the whole stretch, and no cut inside it hides a stage.
- [ ] **Length.** Target 3:00 of narration, then the end card alone and
      silent for 6 s: about 3:06 in all (`script.md`, beat 8). The rules set no length;
      if the submission form, seen logged in, names a shorter limit, follow it
      (`script.md`, "Length").
- [ ] **Subtitles.** `captions-en.srt` (and `captions-es.srt`) are timed to the
      script's beats; after the edit, shift each cue to the final cut and check
      it line by line against the narration actually recorded.
- [ ] **Audio.** Narration only, or music the owner holds a licence for.
- [ ] **End card.** The four lines of `script.md` beat 8, the commit sha, the
      repository link once public.

## 7. Honesty checks before publishing (owner signs off)

Watch the final export from start to end with `script.md` open. One unticked
box means re-edit, not publish.

- [ ] Nothing is called live: not in narration, captions, title or
      description. No recorded or scripted run is presented as running now.
- [ ] SIMULATED is visible (badge or caption) whenever the guided run is on
      screen, and its outcome is never narrated as a model result.
- [ ] RECORDED is visible at least once per recorded run, and the "Recorded run,
      read-only" banner is not cropped away. MODE UNDECLARED on F03 is not
      covered by a caption.
- [ ] Every figure said or shown is in `script.md` with its source, or is
      rendered by the portal from the run on screen. Nothing is rounded or
      upgraded; no rate of any outcome is claimed.
- [ ] "Integrity is not truth" is said (beat 5); the chain check is not
      narrated as proving more than that the file matches the digest the
      committed manifest pins (beat 6); not as "nothing changed".
- [ ] `supported` (guided run) is never shown without its scope line and
      "Evidence: SIMULATED".
- [ ] The Meta API tabs are not narrated as showing attempts or cost.
- [ ] M04 is described as failed by the evaluation contract, not as blocked by
      the policy and not as a provider error.
- [ ] The decision on camera carries the notice "Decisions on this public
      portal carry no operational authority: …", and the narration says
      approving executes nothing.
- [ ] Nothing secret or personal is on screen or in the audio: no key, token,
      cookie, environment variable, machine path, user name, terminal or other
      window. Check the frames around every cut.
- [ ] The legal pages are not shown (their texts are drafts,
      `LEGAL_TEXT_STATUS` in `apps/web/src/lib/site.ts`).

## 8. Upload (owner)

- [ ] **(owner)** Upload to the host the form accepts (the platform embeds
      YouTube and Vimeo links), as **unlisted**, with `captions-en.srt`
      (and `captions-es.srt`) attached.
- [ ] **(owner)** Title and description: the project name, "demo, local
      recording", the footage line of the end card, nothing beyond the script.
- [ ] **(owner)** Open the link in a private window, logged out: it plays,
      captions included, length as exported. Keep a local MP4 copy.
- [ ] **(owner)** Put the link in the submission form; record the date, the link
      and the commit sha in `docs/journal.md`.
- [ ] After the session: stop the server (Ctrl + C) and delete the temporary
      run store directory.
