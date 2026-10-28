# ADR-0007: Public live mode on the owner's key; no hosted BYOK

Date: 2026-10-27 · Status: accepted; admission library in progress (lane S-b2,
`crates/consensus-api/src/live/`) · Scope: the public portal

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

## Consequences

- The recorded live batteries and the video are the only places where live Meta +
  Hyphae behavior is shown without a cap; the portal's live mode is bounded and may
  answer "daily cap reached" with a link to a recorded run.
- The owner must claim the hackathon credits on the key's account, set a spend alert,
  and keep the portal up through about 2026-11-10 (judging, finals, bonus awards).
- Public decisions on live runs carry `authority: public_demo` and no operational
  authority; nothing executes in any mode.
