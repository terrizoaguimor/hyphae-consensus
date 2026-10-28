# ADR-0006: Public API v1, visitor sessions and the listener split

Date: 2026-10-27 · Status: accepted; implemented in lane S-b1 (`crates/consensus-api/src/api/`,
`session.rs`, `public_view.rs`, `public_config.rs`, `limits.rs`, `showcase.rs`,
`static_files.rs`, `verify_cli.rs`, `contracts/api-v1/`), being integrated · Scope: the
public portal's HTTP surface

Numbering is not chronological: see ADR-0005.

## Context

The audit found the prototype's router unsafe to expose (SEC-1..11): every run was
listable and readable by anyone, anonymous callers could start live runs billed to the
server key, only `/decision` was authenticated, upstream bodies and container paths were
persisted into public events, and the loopback console could be reached through DNS
rebinding. The owner decided (2026-10-27) to publish a portal with three honest modes
(guided, live on the owner's key with presets, recorded showcase), so the server needs a
public surface that is isolated per visitor, abuse-bounded, honest about provenance, and
physically separate from the operator's tools. ADR-0004 (the operator console) stays as
history; its router is never exposed.

## Decisions

1. **Two binaries' worth of surface in one binary, never on one listener.**
   `consensus-api serve-public` serves the static web export and `/api/v1` on a Unix
   socket (`/run/consensus/public.sock`, mode 0660) that only cloudflared reaches; there
   is no public TCP port, so nobody on the host can forge `CF-*` headers (ABUSE-2). For
   development the public server may listen on a loopback TCP address
   (`CONSENSUS_PUBLIC_BIND`; a non-loopback address is refused; required on Windows). With
   the socket, `CONSENSUS_ALLOWED_ORIGINS` and `CONSENSUS_ALLOWED_HOSTS` are mandatory and
   `CF-Connecting-IP` is required on every request. `consensus-api serve-operator` (the
   old `serve` name is an alias) listens on loopback only (`127.0.0.1:8787`), requires a
   bearer token on every route (from a file of at least 32 characters, or generated and
   printed once; compared in constant time), checks `Host` and `Origin`, and is started
   by the owner over SSH when needed. There is no `/api/admin` on the public listener.

2. **Anonymous sessions, owner-scoped everything.** `POST /api/v1/session` mints a 256-bit
   random id in an `HttpOnly; Secure; SameSite=Strict; Path=/` cookie named
   `__Host-hc_session`, held in a bounded server-side table with a TTL and LRU eviction;
   no GET and no static response ever sets a cookie (ABUSE-6). The raw id never reaches
   disk: a run's owner is `sha256(id)`, and a visitor's decision is signed
   `visitor:<pseudonym>`, a second truncated digest, so neither a receipt nor a log
   reveals the owner hash. Reads, streams, receipts, verbatim logs, decisions and cancels
   succeed only for the run's owner; everyone else gets `404`, and no route lists another
   session's runs (G2). State-changing requests must carry an allowed `Origin`, a JSON
   body, and are rejected on unknown fields (`deny_unknown_fields`, enums instead of
   strings, no silent defaults).

3. **Routes.** Public: `POST/GET/DELETE /api/v1/session`; `GET /api/v1/scenarios`,
   `/capabilities`, `/health`, `/ready`; `POST /api/v1/runs` (guided, scenario id only);
   `POST /api/v1/live-runs` (preset id only; `503 live_disabled` until the admission
   layer of ADR-0007 is wired); `GET /api/v1/runs`, `/runs/{id}`, `/runs/{id}/events`
   (SSE, ADR-0005), `/runs/{id}/events.ndjson` (the verbatim stored records),
   `/runs/{id}/receipt`; `POST /runs/{id}/decision`, `/runs/{id}/cancel`;
   `GET /api/v1/showcase`, `/showcase/{run_id}`, `/showcase/{run_id}/receipt` and
   `/research/{*path}` (the archived files, byte for byte). Operator, in addition: the
   console page and `POST /api/v1/operator/runs` (model `fake|meta`, memory
   `fake|offline|hyphae`, free task text allowed; the only place the Meta credentials are
   consulted today). Visitor free text reaches no model anywhere on the public surface.

4. **Provenance from the records, not the request (B4, LC-05).** Every run view carries
   four axes: model (`scripted | live_meta`, from the recorded invocations), evidence
   (`simulated | hyphae_snapshot | hyphae_live | none`, from the retrieval and
   verification records), source (`this_server | research_archive`) and, per SSE frame,
   delivery (`live | replay`). The declared `Task.mode` is only cross-checked; a
   disagreement sets `provenance_inconsistent`, which a client must show and must never
   render as LIVE. Public decisions carry `authority: public_demo` and a fixed notice that
   they have no operational authority; records from before schema 1.3 read as
   `unspecified (pre-1.3)`, never as operator authority (LC-11).

5. **Public views strip, public errors are fixed.** The public surface removes proof and
   witness paths, backend references, verifier output and raw failure reasons; each
   stable failure code maps to one fixed sentence (`public_view::failure_message`), and
   `ApiError` carries a stable code and a fixed message, never an upstream body, a path
   or a panic (G5). The operator surface keeps the detail. The verbatim log route serves
   the stored bytes, so what a visitor downloads is what was hashed; the rendered views
   are display copies.

6. **Bounds everywhere.** Body and field limits; per-client rate tables keyed by IPv4 /32
   or IPv6 /64 (sessions 10/min, run creations 6/min, streams 30/min; bounded tables);
   separate concurrency pools for guided (4) and live (1) deliberations; stream caps per
   run, per client and global; a bounded ring of visitor runs; a wall-clock run deadline.
   The numbers live in `public_config.rs` with their environment variables and defaults.

7. **The contract is a file, not a type.** `contracts/api-v1/` holds hand-written JSON
   fixtures (run summary and detail, SSE frames, session, scenarios, showcase entry) and
   the JSON Schemas exported from the Rust types; a drift test keeps the handlers and the
   fixtures equal, the web client generates its types from the same files, and the
   contract's digest is reported by `/api/v1/health` (`contract_sha256`), while
   `/api/v1/capabilities` reports the bounds, the protocol label and whether live mode is
   wired.

8. **The showcase is the archive.** `GET /api/v1/showcase` lists the runs of
   `research/MANIFEST.json`; detail views are rendered from the archived logs opened
   read-only on a copy, labelled `research_archive`, and `/research/{*path}` serves the
   pinned files so a browser (or `consensus-api verify`) can recompute the chain.

## Implementation note (2026-10-28): creation keys

A run creation may carry an `Idempotency-Key` (1 to 128 characters of
`[A-Za-z0-9_.:-]`), scoped to the caller's owner and remembered with the SHA-256 of the
parsed body in a bounded LRU (`api::CreationKeys`, `CONSENSUS_RATE_TABLE_MAX` entries).
Guided runs (`POST /api/v1/runs`) and operator runs (`POST /api/v1/operator/runs`)
claim the key before anything is admitted or started (`CreationKeys::claim`, one step
under the table's lock): a duplicate with the same body that arrives while the first
creation is still under way waits until it ends (bounded by the request timeout), then
answers `202` with the same run, or claims the key itself when that creation failed (a
claim dropped unfinished frees the key); the same key with another body is `409
idempotency_key_reused`; a key whose run was purged is free again. So concurrent
duplicates of one submit start one run. The live route (`POST /api/v1/live-runs`) keeps
its lookup without a claim (`CreationKeys::recorded`) and records the key after the
start: concurrent duplicates all reach the gate, where Siteverify accepts their shared
token once (ADR-0007), so one solved challenge still admits one run.

## Consequences

- G1 holds by construction: the v0 router no longer exists in the binary; the operator
  console is the same v1 surface behind a bearer token on loopback.
- Live mode on the public surface is a separate decision (ADR-0007) plugged in through
  `AppState::live_gate`; until it is wired the route refuses, so the portal can be
  exposed in guided + showcase mode first.
- A visitor who clears cookies loses access to their runs (by design; nothing links a
  run to a person). The 24 h access window and 7 d deletion (ADR-0005) bound that loss.
- Every public statement about a run is derived from hashed records; the web client is
  told, on every frame, whether it is watching live or replayed data.
