# ADR-0008: Public evidence for live mode — a sealed Hyphae snapshot

Date: 2026-10-27 · Status: accepted; `SnapshotMemoryClient` implemented and wired into the
public live mode as its evidence factory (`SnapshotFactory`, `CONSENSUS_SNAPSHOT_DIR`;
integrated 2026-10-28 with the fixes of its review, see "Wiring" below); the record texts, the
manifest and the verifier binary bound before the capture (2026-10-28, review R6, see
"Binding" below); the capture session (plan item B11) held on 2026-10-29, snapshot
`snap-2026-10-29-demo-repo` (`b0375df`, see "Capture" below) · Scope: the public
portal's live mode

## Context

Live runs need evidence with real integrity proofs, but the Hyphae container that served
the recorded batteries cannot sit behind a public server:

- the adapter reaches it through `docker exec`, so a web tier would need Docker-daemon
  access (root-equivalent) and would sit next to the writer and operator keys;
- proven recall fails once a store passes about 3.9 MB (`hyphae mcp` caps every response
  at 4 MiB and the proof carries the whole directory witness, ADR-0003);
- every proven recall writes a ~3.3 MB witness to disk with no retention (about 400 MB
  after the 2026-10-27 batteries);
- a witness is a full copy of the store's data directory, including the WAL with
  forgotten records and the access-control catalog with API-key verifiers. Hyphae calls
  witnesses private operator artifacts.

## Decisions

1. **Three evidence modes, fixed server-side per route, never a silent fallback:**
   `simulated` (guided mode, `fake-verifier`, labelled SIMULATED), `sealed_snapshot`
   (public live mode), `live_hyphae` (operator lab only, never public). A snapshot that
   fails its boot check disables the mode and says why, and so does one whose served files
   change after boot: the rest of the portal (guided mode, the showcase, the static site)
   keeps serving, every live admission is answered `503 live_unavailable` before Turnstile,
   `/ready` reports `ready: false` with `live.evidence_ok: false`, `/capabilities` reports
   `live_enabled: false` without a live section, and the server log carries the stable
   reason code (`[snapshot] live evidence unavailable: <code>`, once at startup and once on
   every change between ok and not ok, never a path). `/health` stays static facts. A
   configuration error (an unset `CONSENSUS_SNAPSHOT_DIR` or a missing or malformed pinned
   anchor with live mode enabled) still stops startup, naming the variable.

2. **Capture, once, in an authorized container session**, scripted as
   `capture-snapshot.sh` with every mutating command documented: a new HOME with
   `hyphae agent setup --no-service`; only the 7 synthetic `demo-repo` facts seeded,
   nothing ever forgotten in that store; proven `list` per project (the released
   `hyphae agent ui` operator interface, whose native envelope allows 16 MiB) plus proven
   recalls for each preset query and one proven empty recall for `other-repo` as isolation
   evidence; `hyphae proof verify` on every capture requiring `kind = memory`,
   `scope = semantic_reexecution` and one shared anchor; `manifest.json` and `SHA256SUMS`;
   then the store's credentials are destroyed and the store is never served again. The
   anchor, the sha256 of the snapshot's `manifest.json` and the Hyphae binary's sha256
   (matched against the GitHub release asset) are published out of band in the README and
   the release (submitted) tag, and the production server pins all three ("Binding" below).
   No Hyphae code is modified.

3. **Witnesses are never published.** Not of the demo, lab or archived stores (their
   contents are described above), and not of the public snapshot either (owner decision:
   no downloadable witnesses). Visitors can verify the event chains and receipts; the
   recorded integrity statuses are run-time results re-verified by the server.

4. **Runtime.** `SnapshotMemoryClient` serves one fixed project; any other project
   returns empty with a recorded reason. Recall is query-independent: every recall
   returns the full proven `list` set, which matches the observed live behavior (13 of
   16 `demo-repo` runs retrieved all 7 records). There is no hidden ranking. Each
   evidence reference records the proof's **own** request (`list(project = demo-repo)`)
   next to the live claim query that triggered retrieval, and a test asserts that a
   proof is attached to a record only if that record is in the proof's captured result.
   The UI and receipt say "proof covers: complete record list of `demo-repo` at anchor
   <prefix>", never "recall of <claim>".

5. **Verifier hardening (all modes).** `Valid` requires the verifier's report to say
   `verified`, semantic re-execution performed, `kind = memory`, the memory scope, and an
   anchor digest equal to the pinned anchor (HY-1); the pinned anchor is mandatory for
   every non-simulated mode and normalized (HY-10); the verifier runs under a deadline
   and is killed on expiry (HY-2); results are cached by content digest (HY-13); a
   byte-binding check that each record's id and text appear in the verified proof is
   prototyped on the captured artifact, and if the layout cannot be confirmed the label
   says that integrity was checked for the proof Hyphae named and the text is not
   byte-compared. Artifacts are verified at boot and re-verified per run, so each receipt
   carries its own verification time.

6. **Honest label.** "Evidence: sealed Hyphae snapshot captured <date>; integrity
   re-verified now with `hyphae proof verify` against pinned anchor <prefix>; this proves
   the records are the snapshot's records, not that they are true or that a model used
   them."

7. **Proof-generation failures stay fail-closed.** In the operator lab, Hyphae's generic
   `corruption` code from proof generation still maps to `Invalid` → `blocked` (policy
   unchanged); the recorded detail states honestly that the same code covers capacity and
   configuration failures (HY-4). A distinct `generation_failed` status is a
   post-hackathon owner decision.

## Consequences

- Public live runs attest the preset's claims against a frozen, operator-seeded,
  7-record memory. That is the same evidence base as the recorded batteries, and it is
  said so on every run.
- The container keeps its proof artifacts under retention rules; pruning only after
  archiving and only by the owner (HY-5).
- The capture session is the only authorized container mutation of the plan; the demo
  and lab stores are not touched by it.

## Implementation (2026-10-27, lane A; the capture followed on 2026-10-29)

- `scripts/hyphae-container/capture-snapshot.sh` is the session script of decision 2
  (POSIX sh, runs inside the container, `--dry-run`, every mutation printed and logged,
  credentials destroyed at the end, the sha256 of every capture file recorded the moment it
  is written). `scripts/snapshot_manifest.py` (`build`, `check`, `selftest`; standard
  library) writes and validates `manifest.json` and `SHA256SUMS`, binds every file to its
  capture through those recorded digests (every proof of a session shares the anchor, so
  the verifier alone cannot tell a swapped pair) and, with `--reverify`, runs
  `hyphae proof verify` again and requires the captured report to be reproduced.
- The artifact layout, the manifest schema (`hyphae-consensus-snapshot-manifest-v1`), the
  host-side procedure, the verification semantics, the runtime contract of decision 4
  and the list of points that stay unverified until the session are in
  `docs/architecture/sealed-snapshot.md`. Snapshot directories live under `snapshots/`;
  `.proof` and `.witness` files are git-ignored there, the rest may be committed.
- Two captures are required by the runtime: `list-demo-repo` (`list`, `demo-repo`,
  `limit 64`, proven, exactly 7 records) and `recall-other-repo-isolation` (`recall`,
  `other-repo`, proven, empty). The per-preset proven recalls named in decision 2 are
  optional additional captures: the runtime is query-independent (decision 4) and uses
  only the two above.
- One ordering detail: the daemon for the new HOME is started before seeding, because the
  only available write path (`hyphae_store`, the MCP writer path of the seed scripts) is a
  daemon client; whether `hyphae agent ui` itself needs that daemon is unverified until the
  session (`--ui-direct` covers the other case).

## Wiring (2026-10-28, lane D2 and its review)

`crates/consensus-api/src/live/snapshot_factory.rs` is the public live gate's evidence
factory; `server.rs` `serve_public_with` decides when it is built.

- **Startup.** The live configuration is read first. With live mode disabled the snapshot
  is never loaded or verified, and a set `CONSENSUS_SNAPSHOT_DIR` earns one notice. With
  live mode enabled, `SnapshotFactory::live_evidence_from_env` runs before the server
  listens: an unset directory or an unusable snapshot setting stops startup naming the
  variable; a snapshot that does not load, verify, bind its pinned anchor, match its digest
  pins or serve the presets' project (`snapshot_dir_missing`, `checksums_mismatch`,
  `digest_mismatch`, `manifest_pin_mismatch`, `pinned_anchor_mismatch`,
  `verifier_binary_mismatch`, `capture_binary_mismatch`, `boot_verification_failed`,
  `project_mismatch`, ...) becomes `UnavailableEvidence`, which reports its code and builds
  nothing (decision 1); a snapshot that boots prints one line (id, project, record count,
  anchor prefix, whether the manifest and the verifier binary are pinned with a digest prefix,
  verification time, no path).
- **Admissions and runs.** The gate refuses before Turnstile while the factory's status is
  not ok. Every admission then re-hashes the served files in full
  (`SnapshotMemoryClient::check_artifacts`) and refuses a changed snapshot before any model
  call, releasing the reservation. Each run gets a copy of the client and a fresh, pinned
  `hyphae proof verify` verifier wrapped in `BootBoundVerifier`: since every proof of a
  session shares the anchor, a `Valid` stands only when the report's `proof_digest` and
  `result_digest` are the ones boot accepted for the record's `proof_ref`; anything else is
  `Invalid` with a fixed detail, so the policy blocks the run.
- **Status.** The factory's status (`/ready`'s `evidence_ok`, `/capabilities`'s
  `live_enabled`) is the same re-hash, cached for two seconds (`READY_CACHE_TTL`) so an
  anonymous caller cannot make the server hash the artifacts at will; an admission's own
  re-check refreshes it. Public responses carry the boolean only; the code is in the log.
- What stays open: the honest label of decision 6 and the "proof covers" sentence of
  decision 4 in the receipt and the web app (not in this lane), and the behaviour of the
  real `hyphae proof verify` on the 2026-10-29 snapshot (every verifier result in the
  tests comes from a fake binary). The latter was observed on 2026-10-29 ("Capture" below).

## Binding (2026-10-28, review R6)

The review found that the record texts were bound to the proofs only by `SHA256SUMS`, a file
in the same directory (the manifest held no digest of the responses), and that nothing at
runtime pinned `manifest.json` or the verifier binary: a record text edited with `SHA256SUMS`
regenerated passed load and boot, and a replaced `CONSENSUS_HYPHAE_BIN`, or a wrapper that
prints the capture-time `.verify.json`, passed boot and every run. The proofs cannot help:
they do not bind the text (HY-1). Before the capture publishes its digests:

- **The manifest binds every capture file.** Each capture entry carries `response_sha256` and
  `verify_sha256` next to `proof_sha256` and `witness_sha256` (required; written by
  `snapshot_manifest.py build` from the digests the session recorded in `captures.index` the
  moment each file was written, checked by `check`). The runtime checks all four against the
  very bytes it then parses, before parsing a response or a report (`digest_mismatch`), and
  re-checks the two served captures' four files, with `manifest.json`, at every status read and
  admission.
- **The manifest pin.** `CONSENSUS_SNAPSHOT_MANIFEST_SHA256` (optional, 64 hex; production sets
  it to the published digest): the loader refuses a `manifest.json` with another SHA-256
  before parsing it (`manifest_pin_mismatch`). With it, every byte served is bound to the
  published value, whatever was regenerated in the directory.
- **The verifier binary pin.** `CONSENSUS_HYPHAE_BIN_SHA256` (optional, 64 hex; production sets
  it to the published binary digest): `CONSENSUS_HYPHAE_BIN` must then be an absolute path; at
  boot, before any verifier run, that file must hash to the pin (`verifier_binary_mismatch`:
  a replaced binary or wrapper never runs) and the pin must be the manifest's
  `hyphae.binary_sha256` (`capture_binary_mismatch`: the snapshot was captured with another
  Hyphae build).
- Each of these refusals makes live mode unavailable with its code (decision 1), the portal
  up; an unusable pin value is a configuration error naming the variable, so live-enabled
  startup stops; the startup line says whether each pin is set (digest prefix only).

Residual risk: without the pins nothing at runtime binds the directory or the
binary (a consistent rewrite of `manifest.json` and `SHA256SUMS` loads, and the startup line
says `not pinned`); a pin is only as good as the value pinned, so the published digests in the
README and the release tag are the trust root, and comparing them with the server's
configuration is a manual step; the binary is hashed at boot only, so one replaced while the
server runs (or between the hash and a run) is not detected until the next start; the dev
machine's container wrapper cannot be pinned; the verifier build the pins name, the capture
session and its script are trusted, and the texts are still not byte-compared with what the
proofs seal (HY-1). `docs/architecture/sealed-snapshot.md` section 6 lists what each check
catches and what it does not.

## Capture (2026-10-29)

- **The session.** Held in the `hyphae-consensus` container on the new development machine,
  2026-10-29 17:16–17:17 UTC (`session.json`: `captured_at` 17:16:12Z, `finished_at`
  17:17:00Z; two scratch sessions ran before it from 17:11), committed in `b0375df` as `snapshots/snap-2026-10-29-demo-repo/`
  (the public subset; `.proof` and `.witness` stay git-ignored, decision 3). Hyphae 4.0.0;
  7 records whose ids equal the research ids; both required captures verified (`kind memory`,
  `scope semantic_reexecution`, one anchor); `snapshot_manifest.py check --reverify` through
  `scripts\hyphae-docker.cmd` reproduced both verifier reports; `check --public-only` passed.
  The first scratch session showed that `hyphae agent ui` refuses a string request id; the
  script now sends an integer. Details: `docs/architecture/sealed-snapshot.md`, "Observed in
  the capture session".
- **The three published values (decision 2).** Anchor
  `43a2dea9c48d015bf6cbd6692b7ecf2bf80c66cd47d398b38e6b8471b18e8f02`; SHA-256 of
  `manifest.json` `b731346c8464796ec17641d5099819cd1ff99525f1ee51409ac8ad790bc1014a`; SHA-256
  of the Hyphae binary `79025e8758ee7481f572d0451756c0e0771e96d13cd7134319367afa8b11a15f`. They
  are in the README ("Published snapshot digests"). The binary digest is the container
  binary's as the capture recorded it; the match against the GitHub release asset that
  decision 2 requires (runbook 3.2, tarball `cf92ed6d…`) is not done yet. The release tag
  does not exist yet.
- **The boot check on the real snapshot.** The runtime at `6e202eb` refused it:
  `response_invalid: captures/list-demo-repo.response.json: no memories array`, because
  `agent ui` 4.0.0 puts the result object under `result` and the loader read only the top
  level. Fixed in `0515f29` (branch `test/snapshot-boot`, not yet merged into
  `feat/public-portal`). With the fix, the strict boot (`SnapshotFactory`) accepted the real
  snapshot with the anchor and manifest pins (verifier-binary pin unset on the development
  machine), both proofs were re-verified by hyphae 4.0.0 in the container, mutated copies
  were refused with `checksums_mismatch`, `digest_mismatch`, `manifest_pin_mismatch` and
  `pinned_anchor_mismatch`, the committed checkout without its git-ignored proofs and
  witnesses was refused with `checksums_incomplete`, and one in-process live run through
  the public router, against a local fake Meta server, retrieved the 7 records with every
  `integrity_status` valid and each per-run report's digests equal to the captured
  `list-demo-repo.verify.json`. No real Meta call was made.
- **Still open.** Merging `0515f29`; the binary-digest comparison above; the snapshot on the
  server (owner authorization; runbook 3.3 lists the values the server pins). A git checkout
  of the snapshot directory does not boot (`checksums_incomplete`), so the server copy comes
  from the container or the owner's main checkout, under the directory name
  `snap-2026-10-29-demo-repo` (`snapshot_id_mismatch` otherwise).
