# Roadmap

Date: 2026-10-29 · State of branch `feat/public-portal` at `bc3b262`, with the snapshot
capture (`b0375df`) added the same day; its boot-check fix (`0515f29`) is merged (`7e26866`) · Submission
deadline: 2026-11-05T02:00Z (development window October 26 – November 4, 2026).

Nothing is deployed. The public portal stays unexposed until every release gate G1–G12
(`docs/security/release-checklist.md`) reads PASS. Dates below are the commit dates of
the git history (the operator's local time, UTC−05:00) or the dated entries of
`docs/journal.md`; the architecture is described in `docs/architecture/overview.md`.

## Done

### 2026-10-26 — M1 to M3, first live runs

| What | Commits | Source |
|---|---|---|
| M1: four-crate workspace, typed contracts and JSON Schemas, deterministic state machine, crash-safe JSONL event log, recommendation policy, ports and fakes, scripted demo | `adbbb66` | journal 2026-10-26 |
| M2: Meta Model API adapter (structured output, timeouts, bounded retries); live Meta test passed (`muse-wonder-1.0`) | `adbbb66` | journal, ADR-0002 |
| M3: Hyphae MCP read-only adapter and `hyphae proof verify` integration | `adbbb66` | journal, ADR-0003 |
| Operator console (Axum, SSE replay) and the synthetic `demo-repo/` fixture | `adbbb66` | ADR-0004 |
| First Mission Control dashboard, replacing the operator console's page; since superseded | `83e71fb`, merged `c3ba8a1` | `docs/mission-control.md` (superseded) |
| Hyphae's pinned Linux build in a local Docker container, because the Windows build's Agent Memory surface does not start on the development machine | `371113f` | journal 2026-10-26 evening |
| Independent audit and fix round: schema 1.1 (hash chain, policy order, per-item challenge ids) | `4faa6af` (dated 2026-10-27) | journal 2026-10-26 night, `docs/reviews/2026-10-26-claude-fix-report.md` |
| Live battery of 12 Meta + Hyphae runs: 9 `blocked`, 1 `abstained`, 2 failed safely on 60 s timeouts, 0 `supported`. The blocks came from proofs reported invalid by an infrastructure fault, not corrupt evidence (the daemon's socket inside its data directory; later the store outgrew the 4 MiB MCP response), fixed on 2026-10-27 | `89b03ca` (dated 2026-10-27) | `docs/evaluation.md` §2, §5 |

### 2026-10-27 — live proofs, schema 1.2, the audit, protocol 1.3, the public API

| What | Commits | Source |
|---|---|---|
| Hyphae proofs verify live (socket moved out of the data directory; store kept under the 4 MiB response cap); policy 1.2; per-run token cap | `addd4a8`, `a721955` | ADR-0001 (1.2 amendment), ADR-0003 |
| Strict structured output; missing draft fields fail closed; a confirmed objection stays unresolved | `3d50baf`, `a1db715` | ADR-0002 |
| 18 live runs with every proof `valid`: one counted `supported` (R06, whose 7 round-1 objections were never re-judged and whose claim 3 cites a seeded record that misdescribes the fixture test; `docs/evaluation.md` Corrections 3 and 4), the final battery F01–F06 all `contested` | `e513176` (code `a1db715`, `3d50baf`) | `docs/evaluation.md` §5 |
| Independent read-only audit of `72e422a` (91 findings, two fail-open paths MA-1 and RT-1) | `e4ef201` | `docs/reviews/2026-10-27-independent-audit.md` |
| Apache-2.0 license, SPDX headers, `NOTICE` | `35297e7`, `e4ef201` | README, "License" |
| Byte-identical research archive with `MANIFEST.json` and replay tests | `ff0d2fb` | `research/README.md` |
| `EventSink`, `create_task` / `deliberate` split, `RunControl` checkpoints | `e1304f2` | ADR-0005 |
| `PolicyTrace`, receipts v2, read-only and locked event logs | `40b6d06`, `092f492` | ADR-0010 decision 6 |
| Protocol 1.3: fail-closed evaluation contract, cross-round carry-forward (closes MA-1 and RT-1) | `1ee386a`, `92ccfeb`, `08f6d33`, `c8749f5` | ADR-0010 |
| Per-run `RunStore`, hash-chained run index, `SharedRunLog` | `200b0df`, `a6911e4`, `745609b` | ADR-0005 |
| Hardened Meta and Hyphae adapters | `6c98fb4`, `e22399a` | ADR-0002 (evening note) |
| CI workflow and Linux parity script; cargo-deny policy and third-party notices | `e39c49c`, `a50281f` | release checklist G8, G12 |
| ADR-0007 to ADR-0010 and the security release checklist; ADR-0005 and ADR-0006 | `58c9bb4`, `e7f6775` | `docs/adr/` |
| Mission Control scaffold (Next.js static export, Astryx, four locales); shell, settings, legal and trust pages, docs, onboarding, e2e harness | `2249ae7`, `17fbe20` | ADR-0009, `apps/web/README.md` |
| Keyed per-call Evaluator schema, one post-send transport retry (tag `prompt-freeze-2`) | `3d1aeec`, `0745f3b` | ADR-0002 (evening note) |
| Protocol 1.3 smoke runs, not counted as validation: K01–K02 on `prompt-freeze-1` (superseded; they exposed the empty-`assessments` hole and the cost of never retrying a sent request), K03–K04 on `prompt-freeze-2` (the keyed schema confirmed live; both ended `supported` for a revised proposal that departs from the task text) | `308e4c4`, `5055195` | `docs/evaluation.md` §6 |
| Public API v1, run manager, live SSE, sessions, listener split, contracts, `verify` CLI (lane S-b1) | merged `9dca9f1` | ADR-0006 |
| Live-mode admission library: USD ledger, Turnstile, presets, geo, judge codes (lane S-b2) | merged `f52b68f` | ADR-0007 |

### 2026-10-28 — the sealed snapshot client, live wiring and hardening, the runbook, the web views

| What | Commits | Source |
|---|---|---|
| Sealed-snapshot capture tooling (`capture-snapshot.sh`, `snapshot_manifest.py`) and its architecture document | merged `4d4f01a`; hardening `29b95aa`, `99c1b09` | `docs/architecture/sealed-snapshot.md` |
| `SnapshotMemoryClient` with boot verification | merged `5455e69` | ADR-0008 |
| Public live-mode wiring: `LiveGate`, portal-wide geo block, metered Meta adapter, key file, `/capabilities` and `/ready` live sections (tested over the fixture snapshot; no public live run yet) | merged `d1e58f1` | ADR-0007 (implementation) |
| Deployment runbook, systemd units, cloudflared ingress, tmpfiles and operator scripts | merged `2fecb32` | `docs/deployment/runbook.md`, `deploy/README.md` |
| Hyphae children no longer see the credential-path variables | `6fb4ac3` | threat model v2 |
| The operator's `consensus-api ledger` subcommand (`status`, `reset --by`, `release <id> --by`) | `4b8b5ae` | ADR-0007 (implementation) |
| The sealed snapshot as the live mode's evidence factory, bound to every run (`BootBoundVerifier`) | merged `2f0efe0` | ADR-0008 (Wiring) |
| Snapshot binding: manifest digests of every capture file, manifest pin, verifier-binary pin | merged `2a9d4ad` | ADR-0008 (Binding) |
| Live hardening: Turnstile before the live slot, per-client daily quota, judge-code daily limit, metered operator runs, stable `/ready` codes, both snapshot pins required on the socket | merged `561c25e`; `e915b6d`, `25c7867` | ADR-0007 (implementation) |
| The scripted demo never opens `demo-run/` or `research/`; `live_battery.py` drives the v1 operator API | `0e8479e`, `be10c71` | README |
| Web lane W1: typed API client, SSE stream, New deliberation, My runs, run workspace (Live/Playback, Explorer, Decision) | merged `4c42ae2`; contract types `7b7fbfb` | `apps/web/README.md` |
| Threat model v2 and the release-checklist statuses | merged `cd27b28`; `88e85d5` | `docs/threat-model.md` |
| Web lane W2: Evidence, Proof inspector, Audit trail with the chain checked in the browser, Meta API panel, policy-trace explanations, revision view, showcase in RECORDED mode, Overview figures from the archive | merged `bc3b262` | `apps/web/README.md` |
| Gates: Windows 600 passed / 0 failed / 2 ignored at `25c7867`; Linux parity 7/7 gates, 603 passed / 0 failed / 2 ignored on the tree of `25c7867`; release gate G8 PASS | `10ed950` and later | release checklist G8 |

### 2026-10-29 — the sealed snapshot captured; boot check fixed (`7e26866`)

| What | Commits | Source |
|---|---|---|
| Sealed snapshot `snap-2026-10-29-demo-repo` captured in the `hyphae-consensus` container (now on the new development machine): hyphae 4.0.0, 7 records whose ids equal the research ids, both proofs verified (`kind memory`, `scope semantic_reexecution`), `snapshot_manifest.py check --reverify` reproduced both verifier reports, `check --public-only` passed; anchor `43a2dea9…8f02`, manifest SHA-256 `b731346c…014a`. The capture script now sends an integer request id (`agent ui` refuses a string id) | `b0375df` | `docs/architecture/sealed-snapshot.md`, "Observed in the capture session"; `snapshots/snap-2026-10-29-demo-repo/` |
| Boot check on the real snapshot: the runtime at `6e202eb` refused it (`response_invalid: … no memories array`, because `agent ui` 4.0.0 wraps the result object in an envelope with `result`); fixed, then the snapshot boots with the pinned anchor and the manifest pin (the verifier-binary pin `CONSENSUS_HYPHAE_BIN_SHA256` was not set), both proofs re-verified by hyphae 4.0.0 in the container, and an in-process live run through the public router was served from it (7 records, every `integrity_status` valid, local fake Meta server only). Mutated copies are refused (`checksums_mismatch`, `digest_mismatch`, `manifest_pin_mismatch`, `pinned_anchor_mismatch`), and the committed checkout, which lacks the git-ignored proofs and witnesses, is refused with `checksums_incomplete` | `0515f29` (merged as `7e26866`) | `git show 0515f29` (test `the_agent_ui_envelope_of_the_real_capture_loads_and_opens`, correction of `sealed-snapshot.md` section 9 item 2) |

## In progress (2026-10-29)

- An independent review of the W2 web views (`bc3b262`).
- Documentation: this roadmap, the architecture overview and the other documents updated
  on 2026-10-29.

## Pending before submission

### Engineering

- **Sealed snapshot: what remains after the capture.** The capture session ran on
  2026-10-29 (`b0375df`, see "Done") and the three digests are published in the README
  ("Published snapshot digests"). Still owed: merging the boot fix `0515f29` into `feat/public-portal` (without
  it the runtime refuses the real snapshot); comparing `hyphae-binary.sha256` with the
  binary extracted from the release tarball checked against `cf92ed6d…` (runbook 3.2);
  the same three digests in the release tag; and the snapshot on the server (runbook 3.3,
  owner authorization). A fresh git checkout of the snapshot directory cannot boot
  (`checksums_incomplete`): the `.proof` and `.witness` files are git-ignored, so the
  server copy must come from the container or the main checkout, under a directory named
  `snap-2026-10-29-demo-repo`.
- **Counted protocol 1.3 battery.** Four of the eight authorized live re-validation runs
  remain; until it runs, no rate of any outcome under protocol 1.3 is claimed
  (`docs/evaluation.md` §6). Results go into `docs/evaluation.md` exactly as observed and
  into `research/` with its manifest. Not run on 2026-10-29: the Hyphae container is now on
  the development machine, but `META_API_KEY` is not set there, and only the owner can set
  it. The battery script stopped before starting the server; no Meta call was made.
- **Release gates.** G1–G7 and G9–G11 pending, G12 partly
  (`docs/security/release-checklist.md`): among them the fresh-clone G8 run before the
  tag, the deployed-ingress review and probe (G1), the two-browser isolation probe (G2),
  the edge rules observed (G3), the deployment canary scan (G4), Playwright and the edge
  comparison through the hostname (G6, G7), the snapshot checks on the deployed directory
  (G9), the owner's legal sign-off (G10), the staging rehearsal (G11), and a CI run on
  GitHub (G12). Open engineering items the checklist marks "not met as written": a
  Hyphae child runs as the same user and can read the key file at its credentials path
  (G4); no fuzzing harness exists (G5); no temp-directory sweep for the verifier exists,
  and the verbatim `/research` files carry 541 container paths (G9).
- **Staging deployment** on the server behind Cloudflare Access, per
  `docs/deployment/runbook.md`, then go-live once every gate G1–G12 reads PASS; live mode
  may stay off (`CONSENSUS_LIVE_ENABLED=false`) at launch if the owner chooses.
- **Deliverables still to write:** the Mission Control design specification, the demo
  package and the rewritten demo script (`docs/demo-script.md` still describes the old
  console), the README section on how the project uses Meta's API, third-party notices
  for the npm packages and fonts, and the final engineering report (implemented, tested,
  passed, failed, incomplete, simulated, live).
- **Video** (3 minutes): every figure rendered from the run shown; recorded or sped-up
  footage labelled on screen; scripted runs never presented as live.
- **Optional:** a single-model baseline battery (one Meta call per final-battery task on
  the same evidence), only with the owner's spend approval. Without it, no comparative
  claim beyond what the recorded runs show is made.

### Owner actions

Only the owner can do these (runbook section 1, release checklist "Owner-only steps"):

- Choose the hostname in the Cloudflare zone; create the named Tunnel and the Turnstile
  widget; set the WAF geo and rate-limit rules and Cloudflare Access on staging; keep
  Rocket Loader, Email Obfuscation and automatic analytics off for the hostname.
- Prepare the Ubuntu 24.04 server (2 GiB plus swap, SSH allowlist).
- Create a dedicated Meta key for the public server (never the development key), claim
  the hackathon credits on its account and set a spend alert; set the daily and lifetime
  USD caps ($10 and $60 decided).
- Set the Meta key on the development machine for the counted battery (the battery is
  authorized; the snapshot capture session took place on 2026-10-29), and authorize
  placing the snapshot on the server.
- Approve the legal texts (terms, privacy notice, limitations, about) in all four
  languages and set the public contact email; sign off the ES, PT-BR and FR texts.
- Generate the judge invite codes on the server and place them only in the submission's
  testing instructions.
- Approve the video and upload it.
- Merge, push `main` and the release tag, make the repository public, and submit.
- After submission: keep the portal monitored through about 2026-11-10 (daily ledger and
  health checks, the kill switch if needed), then revoke the public key, export the
  ledger and keep the research archive.

## After the hackathon

- Hosted BYOK, only with Meta's written permission (ADR-0007 decision 1); until then,
  "bring your own deployment".
- Other model providers, after the winners are announced.
- Signed receipts (an outside anchor for the hash chains, ADR-0010 consequences).
- The executor, after a separate review (approval records a decision only).
- Asynchronous ports in the runtime.
- An upstream Hyphae fix for the proof size (the witness travels inside one MCP response
  capped at 4 MiB, ADR-0003).
- A distinct `generation_failed` integrity status for proof-generation failures, an
  owner decision (ADR-0008 decision 7).
