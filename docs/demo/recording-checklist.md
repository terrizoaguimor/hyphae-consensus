# Recording checklist

Prerequisites, owner actions and the checks before the video is published.
Written 2026-10-29 against `bc3b262`; nothing below has been done yet. No step
needs a credential stored in the repository: the Meta key, the Cloudflare
Access service token, the Turnstile secret and any judge invite code live only
where `docs/deployment/runbook.md` §4.2–4.3 and §7.9 put them, and never appear
on screen.

Steps marked **(owner)** are the owner's to do or to approve; nobody else runs
them.

## 1. Before any recording (both cuts)

- [ ] `demo-run/` is backed up off-machine and nothing writes to it. The video
      needs nothing from it: recorded runs come from `research/`.
- [ ] `research/` checks out clean: `sha256sum -c SHA256SUMS` in `research/`
      passes, and `git status research/` is clean. The showcase refuses files
      whose SHA-256 differs from `research/MANIFEST.json`.
- [ ] The web export in `apps/web/out` is built from the commit being recorded
      (`sh apps/web/scripts/in-docker.sh pnpm check`, which includes the export;
      `apps/web/README.md`). Write the commit sha down for the end card and the
      submission.
- [ ] The figures in `script.md` still match `docs/evaluation.md` and
      `research/README.md` at that commit (re-read §5.2, §5.3, §6 and the
      corrections). If a section changed, update `script.md` first.
- [ ] Screen: a clean browser profile (no extensions, bookmarks bar hidden, no
      autofill), English locale, light theme, 100 % zoom, notifications off.
      Nothing else on screen: no terminal history, no file explorer, no other
      tabs.

## 2. Planned cut: staging and the live preset runs (owner)

Blocked until all of these are done, in this order:

- [ ] **(owner)** The sealed snapshot is captured and its digests are recorded
      (`docs/architecture/sealed-snapshot.md`).
- [ ] **(owner)** The counted protocol 1.3 battery has run on `prompt-freeze-2`
      and is archived in `research/` and written up in `docs/evaluation.md` §6,
      exactly as observed. The presets repeat task texts of the recorded
      batteries (`crates/consensus-api/src/live/presets.rs`); the task set of
      the 1.3 battery is not fixed yet.
- [ ] **(owner)** Staging is up behind Cloudflare Access with live mode on, the
      snapshot loaded and the staging ledger in place
      (`docs/deployment/runbook.md` §4.5, §7.9, §8; staging live runs spend real
      money on the dedicated key and are recorded in the staging ledger).
- [ ] The staging build carries the Turnstile site key (without it New
      deliberation says no live run can be started from the build).
- [ ] **(owner)** Before the session, decide and write in `docs/journal.md`
      which run goes in the video: the first run of the preset "Raise payment
      retry attempts to 10" started in the recording session, whatever its
      outcome, including a failure. Its run id goes in the end card and the
      submission notes.
- [ ] **(owner)** Record the live preset runs on staging. For each run:
      - start it from New deliberation in the recording browser, through the
        real gate (attestation, Turnstile), so its frames arrive with
        `delivery=live`;
      - record the whole run at normal speed, from Start to the Decision tab at
        rest; keep the raw file;
      - note the run id, the start and end time (UTC) and the preset;
      - do not reload the page during the run (a reload turns the stored frames
        into REPLAY);
      - do not restart a run because of its outcome. A failed run is kept and
        may be shown as a failure (`script.md`, beat 3).
- [ ] **(owner)** After the session: read `lifetime_spent_usd` of the staging
      ledger and carry it into the owner's accounting (runbook §4.5, rule 1).

## 3. Fallback cut: the public server on the recording machine

No staging, no Meta key, no Hyphae container. Live mode stays off.

- [ ] Start `consensus-api serve-public` from a built binary with
      `CONSENSUS_PUBLIC_BIND=127.0.0.1:8788`, `CONSENSUS_WEB_ROOT=apps/web/out`,
      `CONSENSUS_RESEARCH_DIR=research`, a new empty `CONSENSUS_RUN_STORE_DIR`
      outside `demo-run/` and `research/`, and `CONSENSUS_COOKIE_SECURE=false`
      (`apps/web/README.md`, manual smoke; `crates/consensus-api/src/public_config.rs`).
      Do not set any Meta or Hyphae variable.
- [ ] Open `http://127.0.0.1:8788/en/` and check that the Overview shows the
      impact metrics, not the "Not connected" placeholder.
- [ ] New deliberation lists the five guided examples and says live mode is not
      enabled; that sentence must not be in the video.
- [ ] Crop or hide the address bar: it shows a loopback address, which is
      correct but distracting. If it stays, the caption must not call the
      server "staging" or "production".

## 4. Editing

- [ ] **Time-lapse.** Every sped-up stretch carries a burned-in caption for its
      whole duration: "Recorded on staging, <date> · time-lapse ×N". N is the
      real factor (recorded duration ÷ on-screen duration, rounded down). No
      cut inside a run hides a stage; a cut is marked by the caption changing.
- [ ] **Replay.** The recorded battery run of the fallback carries "REPLAY ·
      recorded run F03, 27 Oct 2026 · not live" for as long as it is on screen.
- [ ] **Guided.** The guided run carries "Guided example · scripted roles ·
      SIMULATED evidence · no model call" whenever its header badges are out of
      frame.
- [ ] **Length.** At most 3:00 including the end card, or the limit of the
      submission form if it is shorter. Check the form before the final export.
- [ ] **Captions.** An English caption file (SRT or VTT) with the narration
      word for word and the burned-in labels repeated as captions. Check it
      against the final cut, line by line.
- [ ] **Audio.** Narration only, or music the owner holds a licence for.
- [ ] **End card.** The non-affiliation line ("Independent project; not
      affiliated with, sponsored by, or endorsed by Meta."), the footage line
      of `script.md` beat 6, the commit sha, and the repository link once the
      repository is public.

## 5. Honesty checks before publishing (owner signs off)

Watch the final export from start to end with `script.md` open. Every box
must be ticked; one unticked box means re-edit, not publish.

- [ ] No scripted, simulated, recorded or replayed footage is called live, in
      the narration, the captions or the title. The word "live" is used only
      over footage recorded on staging with LIVE badges on screen.
- [ ] LIVE appears on screen only on frames of the staging live run that
      arrived while the page was open. No LIVE on a reloaded run, a guided run
      or a recorded run.
- [ ] SIMULATED is visible (badge or caption) whenever the guided run is on
      screen; no simulated proof is shown as a green "valid".
- [ ] RECORDED and the MODE UNDECLARED flag are visible at least once per
      recorded run, and the "Recorded run, read-only" banner is not cropped
      away.
- [ ] Every figure said or shown is in `script.md` with its source, or is
      rendered by the portal from the run on screen. No figure is rounded,
      combined or upgraded, and none comes from the protocol 1.3 battery
      unless it is in `docs/evaluation.md` §6.
- [ ] R06 is never shown without its two caveats: 7 earlier-round objections
      never re-judged (on screen), and a cited record that misdescribes the
      fixture test (the portal does not show it: the burned-in caption of
      `shot-list.md` 4b carries it). The same caption, or a frame without
      `claim-15`, applies to F03's Decision tab.
- [ ] The Meta API tab is not narrated as showing attempts (the public surface
      does not publish them), and the Audit trail's chain check is not
      narrated as proving more than "no change since the manifest pinned the
      digest".
- [ ] A `supported` recommendation for a revised proposal is never shown
      without its scope line.
- [ ] "Integrity is not truth" is said, and no line says a valid proof shows a
      record is true or that a model reasoned correctly.
- [ ] No cost is claimed over the Meta API tab (it shows none). Any dollar
      figure comes from the portal's budget panel or the staging ledger.
- [ ] Nothing secret or personal is on screen or in the audio: no key, token,
      invite code, Access login, cookie, environment variable, IP address,
      e-mail address, machine path or user name, and no other window. Check the
      frames around every cut.
- [ ] The decision recorded on camera carries the public notice ("Decisions on
      this public portal carry no operational authority: …"), and the
      narration says approving executes nothing.
- [ ] The legal pages (Terms, Privacy, Limitations) are not shown while their
      texts are drafts (`apps/web/src/lib/site.ts`, `LEGAL_TEXT_STATUS`).

## 6. Upload (owner)

- [ ] **(owner)** Upload to the host the submission form names, as
      **unlisted**, with the caption file attached.
- [ ] **(owner)** Title and description: the project name, "3-minute demo", the
      footage line of the end card, and no claim beyond the script.
- [ ] **(owner)** Open the link in a private window, logged out: it plays,
      captions included, and the length is as exported.
- [ ] **(owner)** Put the link in the submission form. Record the date, the
      link and the commit sha in `docs/journal.md`.
