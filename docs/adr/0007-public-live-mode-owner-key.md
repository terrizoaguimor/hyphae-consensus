# ADR-0007: Public live mode on the owner's key; no hosted BYOK

Date: 2026-10-27 · Status: accepted; admission library (lane S-b2) and public wiring
(plan B9, `crates/consensus-api/src/live/`) implemented; the sealed-snapshot evidence
client (ADR-0008) plugged in as the evidence factory (`SnapshotFactory`, configured by
`CONSENSUS_SNAPSHOT_DIR`); `serve-public` refuses to start with live mode enabled and no
snapshot configured, and keeps the portal up with live mode unavailable when a configured
snapshot does not boot; since 2026-10-28 Turnstile is verified before the live slot is
taken, the public lane has a per-client daily quota and each judge code a daily limit, and
the operator console's `model=meta` runs are metered on the same ledger (the
"Implementation" section) · Scope: the public portal

## Context

The portal was meant to let visitors "bring their own Meta API key". The 2026-10-27
research into Meta's Model API terms (last updated 2026-10-09) found that a hosted key
field conflicts with them: §3.1 makes keys confidential and forbids sharing or
transferring them to any third party without Meta's prior written permission; §10.1(i)
forbids accessing the Services on behalf of a third party or offering access through a
gateway, proxy or similar offering; Meta's Help Center states that sharing keys outside
the team violates the Terms. Meta documents no OAuth or delegated flow, Muse Code
subscription credentials are harness-only, and no consumer Muse or Muse Charm product
carries an API entitlement. Meta also offers **no hard spending cap**, only email alerts,
and the account holder pays any overage.

## Decisions

1. **No hosted BYOK for the hackathon.** The portal never accepts a visitor's Meta key.
   Visitors who want to use their own key get "bring your own deployment": the Apache-2.0
   repository with a documented self-host where the key lives only in their own
   environment. Hosted BYOK is reconsidered only with Meta's written permission.

2. **Live mode runs on the owner's dedicated key, as an Integrated Product.** A separate
   key created for the public server (never the development key), supplied on the server
   as a credential file, revocable on its own and revoked after judging. Key precedence
   in configuration: `CONSENSUS_META_API_KEY`, then `MODEL_API_KEY` (Meta's documented
   name), then `META_API_KEY` (also read by the Muse Code CLI, whose harness credentials
   are not valid API keys).

3. **Presets only.** The public server accepts no visitor free text anywhere. Live runs
   take a preset id; presets are the task texts the recorded live batteries ran, verbatim,
   against project `demo-repo`. Guided runs take a scenario id. Nothing a visitor types
   can reach a model, so the portal is an application, not model access.

4. **Admission.** Turnstile verified server-side (success, action and hostname checked,
   fail closed on outage); an 18+ attestation; per-client windows keyed by IPv4 /32 or
   IPv6 /64 in bounded tables; live concurrency 1 per client and 1–2 globally; a judge
   lane behind invite codes (stored as digests) with its own slot and budget slice.

5. **A USD ledger that reserves the worst case.** Because Meta has no hard cap, the
   server keeps an append-only, fsynced daily ledger behind an exclusive file lock shared
   by every process that holds the key. Admission reserves a run's worst case (run token
   cap + `max_completion_tokens` + per-call input cap, all priced at the output rate,
   $20/M for `muse-wonder-1.0`); each HTTP attempt draws on the reservation; settlement
   charges actual usage on success, zero on non-retryable 4xx, and the attempt's full
   worst case, never refunded, on timeouts, 5xx, post-send transport errors or a response
   without usage. Startup counts unsettled reservations at worst case. The ledger trips
   itself on an overrun, on 401/402/403 from Meta, or when the lifetime cap is reached;
   the kill switch is the same state. **Caps (owner decision): $10 per UTC day, $60
   lifetime, of which about $15 is reserved for the judge lane.** Daily cap × exposed
   days must not exceed the lifetime cap, which must not exceed the remaining credit.
   Measured cost on the final code: $0.81 mean, $0.98 max per run.

6. **Request parameters.** `max_completion_tokens` (default 16,000; observed maximum
   output 10,692; reasoning tokens count as output), `finish_reason` and `refusal`
   parsed, full-jitter backoff honoring `Retry-After`, no retry on 504 or non-retryable
   4xx and never after a request was sent, `safety_identifier` = HMAC of the visitor
   session, Standard models only (never `-contributor` for visitor text), a public per-run
   token cap of about 70,000.

7. **Duties of an Integrated Product.** The whole portal is blocked for Meta's Restricted
   Territories at country level, **including all of Ukraine** because Cloudflare's Free
   plan cannot isolate Crimea, Donetsk and Luhansk; South Korea is blocked for live mode
   only (Muse Wonder is unavailable there); Hong Kong and Macau are listed explicitly. A
   terms page binds visitors to Meta's Acceptable Use Policy; a privacy notice names the
   data controller (Mario Gutiérrez) and a contact, states retention (visitor runs
   readable for 24 h, deleted whole after 7 days except a one-line index entry and the
   run's ledger entry), the pseudonymous identifier sent to Meta, and Cloudflare's and
   the host's processing; an AI disclosure and a limitations panel are shown. No Meta
   logo, no "Built with / Powered by Meta" badge, factual references only, and a visible
   "independent project, not affiliated with or endorsed by Meta" line.

## Implementation (2026-10-27)

- `POST /api/v1/live-runs` takes `{preset_id, attest_adult, judge_code?}` plus the
  Turnstile token in `cf-turnstile-response`. After the route's own steps (body bounds,
  the `Idempotency-Key` lookup, the session, the run rate and the visitor-run ring) the
  gate (`crates/consensus-api/src/live/gate.rs`) admits in two phases with the route's
  one live deliberation slot taken between them, stopping at the first refusal and
  spending nothing before the ledger has reserved the run's worst case:
  1. *Phase one* (`LiveGate::verify`, before the slot; it holds and charges nothing):
     preset (`404 preset_unknown`), geography on the live surface (`451`), the
     attestation (`422 attestation_required`), the evidence source (`503
     live_unavailable` while the sealed snapshot is not ok, before Turnstile, so the
     refused visitor spends no token), Turnstile fail-closed (`403
     turnstile_required|turnstile_failed`, `503 turnstile_unavailable`), then the lane
     (a hashed judge code or the public lane, silently).
  2. *The slot* (`RunManager::reserve`): `429 too_many_runs` or `503 shutting_down`.
     Requests still waiting on Siteverify (up to its 10 s timeout) hold no slot, so bogus
     tokens sent from many prefixes no longer keep it from a visitor who solved a
     challenge. They wait concurrently on the blocking pool instead, bounded by the
     per-client run rate and the edge rate-limit rule.
  3. *Phase two* (`VerifiedLiveRequest::admit`, with the slot held): the lane's daily
     quota (below), the ledger reservation (`429` with `Retry-After` when the ledger
     gives a hint, `503` otherwise; stable codes `live_budget_*`, `live_concurrency`,
     `live_hourly_admissions`, `live_tripped`, `live_unavailable`; fixed sentences,
     never the ledger's text), the metered model client and the deliberation.

  A token Siteverify accepted in phase one is consumed even when the slot or phase two
  then refuses: Siteverify accepts a token once, and the web client resets its widget
  after every submit, so the refused visitor solves a new (usually invisible) challenge.
  Siteverify gets a fresh UUID as `idempotency_key` on every call, never the request's
  `Idempotency-Key`: Cloudflare's idempotency is global and immediate while the route's
  replay is owner-scoped and recorded only after the start, so forwarding the header let
  one solved challenge start a run per session (or several at once). A retry whose first
  attempt succeeded is answered from the route's stored creation before the gate; one
  whose first attempt died after Siteverify needs a new challenge.
- Per-client daily quota (fairness): at most `CONSENSUS_LIVE_CLIENT_DAILY` (default 3)
  public-lane admissions per client key (IPv4 /32, IPv6 /64, the rate tables' key) per
  UTC day of the ledger's clock, in a bounded LRU table (`limits::DailyCounts`,
  `gate::CLIENT_QUOTA_TABLE_MAX` = 10,000 keys; a key enters it only through a counted
  admission, which the ledger's hourly cap bounds to 144 a day with the defaults; past
  the bound the least recently counted key is forgotten first). The count goes with the
  reservation: it is taken just before the ledger is asked (so concurrent requests of one
  client cannot overshoot the quota) and given back when the ledger refuses, when the run
  cannot be built and when its start fails, so only a granted, started run keeps it and a
  refusal never consumes it. Over the quota: `429 live_client_quota`,
  `Retry-After` until 00:00 UTC, a fixed sentence without figures. The judge lane is
  exempt. The table is process-local: a restart of `serve-public` forgets every count.
- Judge codes: `deploy/scripts/judge-codes.sh` generates them on the server (140 random
  bits each from `/dev/urandom`, 28 symbols of `0-9 A-Z` without `I L O U` in seven
  hyphenated groups of four, entered exactly as printed), prints them once for the
  submission's testing instructions, and writes only their SHA-256 digests,
  `root:consensus 0640`, refusing to overwrite an existing digests file without
  `--force`. Each code admits at most `CONSENSUS_LIVE_JUDGE_CODE_DAILY` (default 5)
  judge-lane runs per UTC day (process-local, one table entry per code at most); a code
  over its limit is refused with the judge lane's own daily refusal (`429
  live_budget_judge_daily`), so its holder learns nothing new from it. Revocation is
  deleting the code's digest line and restarting `serve-public` (the file is read at
  startup only), or the kill switch for every lane at once. What remains: the lane is
  observable (its refusal codes and sentences differ from the public lane's, it has its
  own hourly window and slot, and `/capabilities`' `daily_remaining` leaves judge
  reservations out), so a prober who solves one Turnstile challenge per guess can tell
  whether a code is valid, which is why codes must be long and random; the digests are
  unsalted SHA-256; and a leaked code drains the judge slice and the judge daily cap at
  up to its per-code limit a day until it is revoked.
- Operator metering: `serve-operator` holds the Meta key too. With
  `CONSENSUS_LIVE_LEDGER_DIR` set (on the server the operator unit loads `public.env`
  before `operator.env`) it opens that ledger with `serve-public`'s caps
  (`server::operator_ledger`: the configuration read credential-free, with the live
  switch, the Turnstile secret file and the judge-codes file hidden from
  `LiveConfig::load`, as the `ledger` subcommand reads it; an absent directory stops
  startup instead of starting a fresh ledger), and every console `model=meta` run takes
  the live slot, reserves its worst case on the operator lane (the daily and lifetime
  caps, no pacing; it spends from the day the public lane shares), draws and settles
  every HTTP attempt and releases the reservation when it ends, through the public gate's
  own `MeteredReservation` and `MeteredDeliberation`. A ledger refusal answers with its
  stable code and fixed sentence; a run whose `CONSENSUS_META_MAX_COMPLETION_TOKENS`
  exceeds the ledger's `CONSENSUS_LIVE_MAX_COMPLETION_TOKENS` is refused (`400
  model_not_configured`) before anything is reserved. The kill switch and every trip
  therefore stop operator runs too. Without the variable (a development machine running
  the recorded batteries) operator runs are unmetered, and `serve-operator` prints one
  line saying so (`Operator model=meta runs: UNMETERED ...`; on the server: `... metered
  on the live ledger at <dir> ...`).
- Every Meta HTTP attempt of a public run is drawn on the run's reservation and settled
  once (`MetaModelClient::with_meter`); a refused draw fails the run at its stage with
  `budget_refused`. The reservation is released, with bounded retries in every case,
  when the run ends, when its deliberation is dropped (a caught panic, a start whose
  task creation failed), or by the route's refund for a start refused earlier; a
  release that still fails is logged with its id for `release_by_id`. The operator
  calls that, and clears a trip (`Ledger::reset`), with the `consensus-api ledger`
  subcommand (`ledger_cli.rs`: `status`, `reset --by`, `release <id> --by`) from a shell
  on the server; neither listener exposes either.
  `safety_identifier` is the session's pseudonym (`visitor:<16 hex>`), not an HMAC;
  `max_completion_tokens` is the live bound.
- The portal-wide block (duty 7) is a middleware on every public route reading
  `CF-IPCountry` from a listener behind cloudflared: a listed country is `451
  geo_blocked_portal`, a missing header `451 geo_unknown`; the live surface also refuses
  South Korea and any unknown country (`451 geo_blocked_live`), which includes a
  development bind that trusts no proxy header. `/capabilities` publishes the presets,
  the public budget projection and the requirements (never the judge lane; no budget
  at all when the ledger cannot be read; `live_enabled: false` and no live section at all
  while the evidence source is not ok); `/ready` is false while the ledger is
  tripped, unreadable, or the evidence source is not ok. Both report the kill-switch
  file as a trip the moment it exists, before the next admission records it.
- The server key may come from a file (`CONSENSUS_META_API_KEY_FILE`, systemd
  `LoadCredential`), ahead of every key variable.
- The evidence factory is the sealed snapshot (ADR-0008, lane D2, closed; its "Wiring"
  section): with live mode enabled, `SnapshotFactory::live_evidence_from_env` boots the
  client from `CONSENSUS_SNAPSHOT_DIR` before the server listens. An unset directory or an
  unusable snapshot setting stops startup naming the variable; a snapshot that does not
  boot leaves the portal up with live mode unavailable and its stable code in the log
  (ADR-0008 decision 1). Every admission re-checks the served files in full and refuses a
  changed snapshot before any model call; every admitted run gets a copy of the client
  with a fresh, pinned `hyphae proof verify` verifier whose `Valid` stands only for the
  digests boot accepted; `/ready`'s `evidence_ok` and `/capabilities`'s `live_enabled` read
  the factory's status (cached two seconds). With live mode disabled the snapshot is not
  read at all. Every public live run is declared `{meta, sealed_snapshot, public,
  <preset>}`.
- The per-call input cap (`CONSENSUS_LIVE_INPUT_CAP_TOKENS`) defaults to 32,000 tokens
  since 2026-10-28: the ledger bounds a call's input at `request bytes + 1024` (a token
  count never exceeds the UTF-8 byte count; over-bounding costs reservation headroom,
  never money), and the Evaluator's request over the seven snapshot records exceeds 8,000
  under it, so the former default refused every snapshot-backed run at its first
  evaluation after two paid calls. A run's worst case is then (70,000 + 16,000 + 32,000) ×
  $20/M = $2.36, inside the $2.50 the day offers at 00:00 UTC (25% burst).
- Follow-ups: the public snapshot itself is to be captured on 2026-10-29.

## Consequences

- The recorded live batteries and the video are the only places where live Meta +
  Hyphae behavior is shown without a cap; the portal's live mode is bounded and may
  answer "daily cap reached" with a link to a recorded run.
- The owner must claim the hackathon credits on the key's account, set a spend alert,
  and keep the portal up through about 2026-11-10 (judging, finals, bonus awards).
- Public decisions on live runs carry `authority: public_demo` and no operational
  authority; nothing executes in any mode.
