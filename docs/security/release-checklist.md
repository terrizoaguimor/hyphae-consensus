# Security release checklist (public exposure gates)

Status as of 2026-10-27. Nothing is exposed until **every** gate below reads PASS, with
the evidence linked. The old operator router (ADR-0004) is never exposed under any gate.
Companion documents: `docs/threat-model.md`, the independent audit
(`docs/reviews/2026-10-27-independent-audit.md`), ADR-0007 (live mode), ADR-0008
(evidence), ADR-0009 (frontend).

| # | Gate | Pass criterion | How it is checked | Status |
|---|---|---|---|---|
| G1 | Old router never exposed | The tunnel ingress names only the public unix socket plus a final 404 catch-all; the operator server listens on loopback only and is started on demand over SSH | Ingress file review; an automated probe through the public hostname of `/api/runs`, the old console `/`, and random paths returns nothing from the v0 router | pending |
| G2 | Isolation | Non-owners get 404 on run reads, streams, receipts, verbatim logs, decisions and cancels; no endpoint lists another session's runs | Cross-session HTTP test suite; a two-browser probe through the public hostname | pending |
| G3 | Abuse and cost | Body and field limits, /64 keying, bounded tables, concurrency semaphores, the USD ledger (reservation, settlement, trip, lifetime cap, judge lane), kill switch | Unit and HTTP tests (50 concurrent reservations never exceed the cap; kill -9 between reserve and settle is counted at worst case; no refunds on unknown outcomes; an operator-console live run debits the same ledger; the lifetime cap persists across restarts); Cloudflare rate-limit and geo rules observed (`cf-ray`, 429 at the edge) | pending |
| G4 | Credentials | A canary key never appears in any response, event, receipt, verbatim log, server log, client bundle, subprocess environment or `/proc/<pid>/environ`; the public server uses a dedicated Meta key from a credential file; `.env*` and key files are git-ignored | Canary test suite; `git check-ignore`; deployment review | pending |
| G5 | Errors | Public responses and public events carry stable codes and fixed messages only: no paths, upstream bodies, panics or internal reasons | Fuzzed upstream errors against the adapter; HTTP tests on public views | pending |
| G6 | Web | Per-page CSP hashes from the manifest; zero `securitypolicyviolation` events across pages × 4 locales × 2 themes including the Turnstile flow; `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`/`frame-ancestors`; edge HTML byte-equal to origin (Rocket Loader, Email Obfuscation, Zaraz and Web Analytics off); `cf-cache-status` never HIT on `/api` | Playwright through the public hostname; byte comparison of edge and origin responses | pending |
| G7 | Honesty | Provenance axes shown on every run; LIVE only for live frames of a live model run; simulated proofs never green "valid"; public decisions labelled as carrying no authority; `PolicyTrace` figures rendered from the run, never hard-coded | End-to-end assertions in the Playwright suite | pending |
| G8 | Correctness | MA-1 and RT-1 regression suites green; the differential policy test green; every `research/` log round-trips byte for byte and replays identically from a fresh clone; `consensus-api verify` accepts the archive and rejects a one-byte change | `cargo test --workspace --locked` on a fresh clone, on Linux (`scripts/linux-gates.sh`) | PASS on 2026-10-28 at `609225f` on Ubuntu 24.04 (fmt, clippy, 539 tests passed / 2 ignored, evaluate 10/10, the sealed-snapshot tooling self-tests; `consensus-api verify` exists and is tested by `api_verify`); the fresh-clone run is still owed before the tag |
| G9 | Hyphae | No Hyphae daemon, credentials or writer path on the public server; the snapshot boot check is green (kind, scope, anchor, binding); no witness or container path in any public response; verifier deadline and temp-directory sweep tested | Deployment review, including `python -I scripts/snapshot_manifest.py check <snapshot dir> --reverify <hyphae>` on the deployed directory (the captured verifier reports reproduced by the deployed binary) and `--public-only` on the committed subset (`docs/architecture/sealed-snapshot.md`); HTTP tests on public views; unit tests | pending |
| G10 | Terms | Whole-portal geo-block of the Restricted Territories incl. Hong Kong, Macau and all of Ukraine; South Korea blocked for live mode; 18+ attestation; terms, privacy notice and limitations pages approved by the owner; non-affiliation line; presets only (a test proves the public server accepts no free text) | Cloudflare rule review plus the server-side `CF-IPCountry` check; owner sign-off recorded in `docs/reviews/` | pending |
| G11 | Ops | Graceful shutdown, restart recovery, bounded guided-run retention, health and readiness endpoints, ledger trip and kill switch exercised on staging; the operator runbook exists | Staging rehearsal with written results | pending |
| G12 | Supply chain | `pnpm audit --prod` clean, `cargo deny` (or an equivalent advisory check) clean, lockfiles frozen, actions pinned by SHA, no new agent-instruction files, Docker images pinned by digest | CI run plus local gates | partly: web scaffold `pnpm audit` clean and `cargo deny check` (deny.toml: permissive license allowlist, advisories, bans, crates.io only) passing on 2026-10-27; THIRD_PARTY_NOTICES.md generated from `cargo metadata`; Docker images and actions pinned |

## Owner-only steps that the gates depend on

1. Dedicated public-server Meta key created on dev.meta.ai and supplied as a credential
   file on the server; the development key never leaves the development machine.
2. Hostname chosen in the Cloudflare zone; named Tunnel and Turnstile widget created;
   WAF rules: geo-block (whole portal: AF BY KH CN HK MO CU ER ET HT IR IQ LY MM NI KP PK
   RU SO SY UA VE EH; live routes additionally KR), one rate-limit rule on the live-run
   route; Rocket Loader, Email Address Obfuscation and automatic analytics off for the
   hostname.
3. Legal texts (terms, privacy notice, limitations, about) reviewed and approved in all
   four languages; the public contact email set.
4. Daily and lifetime USD caps set in the server configuration ($10 / $60 decided), the
   judge-lane invite code generated and shared only in the submission's testing notes.
5. After judging (about 2026-11-10): revoke the public key, export the ledger, keep the
   research archive.
