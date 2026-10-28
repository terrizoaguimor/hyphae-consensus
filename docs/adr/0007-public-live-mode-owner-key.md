# ADR-0007: Public live mode on the owner's key; no hosted BYOK

Date: 2026-10-27 · Status: accepted; admission library (lane S-b2) and public wiring
(plan B9, `crates/consensus-api/src/live/`) implemented; the sealed-snapshot evidence
client (ADR-0008, lane D2) pending, so `serve-public` refuses to start with live mode
enabled until it is plugged in · Scope: the public portal

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
  Turnstile token in `cf-turnstile-response`; the gate
  (`crates/consensus-api/src/live/gate.rs`) checks, in order and stopping at the first
  refusal: preset (`404 preset_unknown`), geography on the live surface (`451`), the
  attestation (`422 attestation_required`), Turnstile fail-closed (`403
  turnstile_required|turnstile_failed`, `503 turnstile_unavailable`), the lane (a hashed
  judge code or the public lane, silently), then the ledger reservation (`429` with
  `Retry-After` when the ledger gives a hint, `503` otherwise; stable codes
  `live_budget_*`, `live_concurrency`, `live_hourly_admissions`, `live_tripped`,
  `live_unavailable`; fixed sentences, never the ledger's text). Siteverify gets a fresh
  UUID as `idempotency_key` on every call, never the request's `Idempotency-Key`:
  Cloudflare's idempotency is global and immediate while the route's replay is
  owner-scoped and recorded only after the start, so forwarding the header let one
  solved challenge start a run per session (or several at once). A retry whose first
  attempt succeeded is answered from the route's stored creation before the gate; one
  whose first attempt died after Siteverify needs a new challenge.
- Every Meta HTTP attempt of a public run is drawn on the run's reservation and settled
  once (`MetaModelClient::with_meter`); a refused draw fails the run at its stage with
  `budget_refused`. The reservation is released, with bounded retries in every case,
  when the run ends, when its deliberation is dropped (a caught panic, a start whose
  task creation failed), or by the route's refund for a start refused earlier; a
  release that still fails is logged with its id for `release_by_id`.
  `safety_identifier` is the session's pseudonym (`visitor:<16 hex>`), not an HMAC;
  `max_completion_tokens` is the live bound.
- The portal-wide block (duty 7) is a middleware on every public route reading
  `CF-IPCountry` from a listener behind cloudflared: a listed country is `451
  geo_blocked_portal`, a missing header `451 geo_unknown`; the live surface also refuses
  South Korea and any unknown country (`451 geo_blocked_live`), which includes a
  development bind that trusts no proxy header. `/capabilities` publishes the presets,
  the public budget projection and the requirements (never the judge lane; no budget
  at all when the ledger cannot be read); `/ready` is false while the ledger is
  tripped, unreadable, or the evidence source is not ok. Both report the kill-switch
  file as a trip the moment it exists, before the next admission records it.
- The server key may come from a file (`CONSENSUS_META_API_KEY_FILE`, systemd
  `LoadCredential`), ahead of every key variable.
- Follow-ups: the operator console's own live runs (`model=meta` on `serve-operator`)
  do not go through the ledger yet, so their spend is not counted against the caps; the
  per-client live limit (one per /64) is not enforced beyond the global slots; the
  sealed-snapshot factory (ADR-0008) is the one missing part of the public live path.

## Consequences

- The recorded live batteries and the video are the only places where live Meta +
  Hyphae behavior is shown without a cap; the portal's live mode is bounded and may
  answer "daily cap reached" with a link to a recorded run.
- The owner must claim the hackathon credits on the key's account, set a spend alert,
  and keep the portal up through about 2026-11-10 (judging, finals, bonus awards).
- Public decisions on live runs carry `authority: public_demo` and no operational
  authority; nothing executes in any mode.
