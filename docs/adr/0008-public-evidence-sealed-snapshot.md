# ADR-0008: Public evidence for live mode — a sealed Hyphae snapshot

Date: 2026-10-27 · Status: accepted; capture and `SnapshotMemoryClient` pending (plan
item B11/D2) · Scope: the public portal's live mode

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
   fails its boot check disables the mode, and `/health` says why.

2. **Capture, once, in an authorized container session**, scripted as
   `capture-snapshot.sh` with every mutating command documented: a new HOME with
   `hyphae agent setup --no-service`; only the 7 synthetic `demo-repo` facts seeded,
   nothing ever forgotten in that store; proven `list` per project (the released
   `hyphae agent ui` operator interface, whose native envelope allows 16 MiB) plus proven
   recalls for each preset query and one proven empty recall for `other-repo` as isolation
   evidence; `hyphae proof verify` on every capture requiring `kind = memory`,
   `scope = semantic_reexecution` and one shared anchor; `manifest.json` and `SHA256SUMS`;
   then the store's credentials are destroyed and the store is never served again. The
   anchor and the Hyphae binary's sha256 (matched against the GitHub release asset) are
   published out of band in the README and the submitted tag. No Hyphae code is modified.

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

## Implementation (2026-10-27, lane A; capture pending 2026-10-29)

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
