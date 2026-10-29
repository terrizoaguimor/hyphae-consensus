# apps/web: Mission Control

The public portal's frontend: a Next.js App Router app exported as static
files (`output: "export"`), in English, Spanish (`es`), Brazilian Portuguese
(`pt-BR`) and French (`fr`), styled with Astryx's prebuilt CSS and a custom
`hyphae-mission` theme. Plan items E1 (scaffold, theme, i18n plumbing, CSP
manifest, Node container) and E2 (shell, navigation, onboarding, settings,
legal and trust pages, in-app docs), with a Playwright and axe harness, plus
the first API lane (W1): the generated contract types, the same-origin API
client, the SSE run stream, runtime connectivity, and the New deliberation,
My runs and run workspace views; and the second (W2): the run workspace's
Evidence, Proof inspector, Audit trail and Meta API tabs, the plain and
technical explanations of the policy trace, the revision view, the chain
checked in the browser, the research showcase with the run workspace in
RECORDED mode, and the Overview's figures. The export is served by the Rust server
itself (`consensus-api serve-public`), so every API call is relative
(`/api/v1/…`); whether the API is there is found out at runtime
(`GET /api/v1/health`, once per page load), and a view that needs it shows the
not-connected placeholder when it does not answer: no sample data, no
spinner.

## Pages

18 pages per locale, all pre-rendered (`/<locale>/…/`, trailing slash), plus
the language-neutral root page `/` and the 404 page:

| Page | Path | Content |
|---|---|---|
| Overview | `/` | What the system does, the main call to action (until the health probe answers, and when the API is unreachable: "How guided examples will work", linking to the Modes article; once the API answered it opens New deliberation and reads "Run a guided example" only when `capabilities.guided_enabled` says so, with the server's schema, policy and contract versions from the health answer, and a note when the server's contract is not the one this build was generated from), the tour invitation, how a deliberation works, the provenance legend; the impact figures, counted in the browser from `GET /showcase` (runs and records, runs by outcome in columns of their evidence axis, by battery with schema and policy rules, by model axis and authorization, the recorded Meta request ids, the runs before the hash chain), the not-connected placeholder without the API |
| New deliberation | `new/` | Guided examples from `GET /scenarios` (the count the server lists, title, description, expected outcome as a neutral chip, SIMULATED model and evidence badges, a Run button: `ensureSession`, `POST /runs` with a fresh `Idempotency-Key`, then the run page) and the live presets from `capabilities.live` (`LiveCapabilitiesV1`: each preset with the badges of its own recorded model and evidence axes, the public budget in US dollars, the 18+ and territory attestation linking to the Terms, an optional invite code under a disclosure, the Turnstile widget, `POST /live-runs`; see "Live form" below), or the fixed "Live mode is not enabled on this server" state linking to the showcase; every API error shows its code's sentence and the Retry-After wait |
| My runs | `runs/` | The session's runs from `GET /runs` (title, state, outcome with its evidence axis, the failure sentence of a failed run, provenance badges, never a LIVE label: a live-model run reads META MODEL with its recorded date, created and updated, "access ends" and "deleted after" from the capabilities limits), the empty state, no list and no count without a session |
| Run workspace | `run/?id=<run_id>` | Client-rendered from the query string: header (title, state, outcome with evidence axis and, for a revision, which proposal it is for, records, revision round, run id, the failure sentence of a failed run, provenance axes with the latest frame's delivery, flags), the error of a failed detail request with a retry, tabs Live (only while the run deliberates and its latest frame is LIVE; Playback otherwise), Explorer, Evidence, Proof inspector, Audit trail, Meta API and Decision; `run/?id=<run_id>&source=showcase` opens a recorded run of the archive in RECORDED mode; see "Run workspace" below |
| Research showcase | `showcase/` | The archive from `GET /showcase`, by battery: each run RECORDED with its outcome next to its evidence axis, its schema (1.0: pre-1.1, unchained), authorization and records, opening in the run workspace in RECORDED mode; each battery's log with the SHA-256 the manifest pins, for download; `MANIFEST.json` hashed in the browser, its notes (the archive's own, English) shown only when it matches the published digest; the not-connected placeholder without the API |
| Docs | `docs/` | Index of 7 articles, tour launcher, explanation level |
| Docs articles | `docs/<slug>/` | `what-is`, `three-roles`, `not-voting`, `verified-evidence`, `abstain-contested`, `human-authorization`, `modes`: summary, sections, technical details, previous/next |
| Settings | `settings/` | Theme, language, motion, explanation level, remove saved settings |
| Terms of use, Privacy notice, Limitations | `terms/`, `privacy/`, `limitations/` | Drafts for the planned public deployment, marked "Draft" while `LEGAL_TEXT_STATUS` is `"draft"`, each dated from its own entry in `LEGAL_TEXT_CHANGED` (the day its own text last changed, pinned to a digest of that text by `tests/legal-dates.test.ts`) |
| About | `about/` | Independence from Meta, Hyphae as the owner's earlier project (copyright Celiums Solutions LLC, Apache-2.0, docs CC-BY-SA-4.0), license, AI assistance, source repository (text until `SOURCE_REPOSITORY_URL` is set); marked "Draft" like the other legal pages |
| Not found | `/404.html` | `src/app/global-not-found.tsx` (Next's experimental `globalNotFound`): `lang="en"`, the four language links, the non-affiliation line, the theme script and the app styles; served for every unknown path |

Every internal link is built from the site map in `src/lib/site.ts`;
`tests/site-map.test.ts` requires a route file for each entry, and the e2e
crawler requires every exported link to answer 200 without a redirect. The
run workspace is not a navigation entry: it is reached from New deliberation
(after a run is created) and from My runs (`runPath()` in `site.ts`).

## API client (lane W1)

- **Contract types**: `src/lib/api/types.ts` is GENERATED by
  `scripts/gen-api-types.mjs` from the JSON Schemas in `contracts/api-v1`
  (schemars draft-07; a closed subset: `definitions`/`$ref`, `properties` +
  `required`, nullable `type` lists, `enum`, `oneOf`/`anyOf` unions,
  single-element `allOf`, `items`). The header names the contract's SHA-256
  (`contract-sha256.txt`, computed as the server does: `<name>\n<file>` per
  schema in name order), which `GET /api/v1/health` reports as
  `contract_sha256`. `pnpm api:types` regenerates, `pnpm api:check` (part of
  `pnpm check`) and `tests/api-types.test.ts` fail on drift. No generator
  dependency was added: the policy's release cooldown and the small subset
  made a 300-line script the smaller risk.
- **Where the contract is**: inside the containers, `in-docker.sh` and
  `playwright-docker.sh` mount the repository's `contracts/` read-only at
  `/contracts`; on the host it is `../../contracts/api-v1`. The generator,
  the tests and the e2e specs resolve it in that order (`API_CONTRACT_DIR`
  overrides).
- **Validation**: `src/lib/api/schemas.ts` holds zod mirrors of the
  contract for every response the UI reads, loose on unknown fields
  (additive server changes pass through) and strict on the enumerations the
  honesty rules depend on; each is checked against the generated type with
  `satisfies`, so a mirror that drifts fails `tsc`.
  `tests/api-schemas.test.ts` parses every fixture of the contract.
- **Client**: `src/lib/api/client.ts`, `createApiClient()` / `api`: JSON
  only, `Content-Type: application/json` on bodies, `credentials:
  "same-origin"`, `Idempotency-Key` on run creation and decisions, the
  Turnstile token in `cf-turnstile-response`. Every failure is an `ApiError`
  (`code`, `status`, `retryAfterSecs` from the body or the `Retry-After`
  header, `path`); the client's own codes are `unreachable`,
  `unexpected_status` and `bad_response`. `ensureSession()` POSTs only when
  GET says there is no session.
- **Stream**: `src/lib/api/sse.ts`, `RunStream` over `EventSource`:
  `consensus`, `resync` and `stream_end` events, duplicates dropped, gaps
  and resyncs ask the owner to re-read the detail, `stream_end` closes the
  source, browser reconnects are reported as `reconnecting`. Every frame's
  `delivery` is passed through as the server sent it.
- **Connectivity**: `useApiHealth()` probes `GET /api/v1/health` once per
  page load (module-level cache); `<ApiGate>` renders its children once the
  probe answered and the `NotConnected` placeholder (with the failed
  request's code and a retry button) when it did not. `API_CONNECTED` is
  gone from `src/lib/site.ts`.
- **Errors in the UI**: `<ApiErrorNotice>` shows the translated sentence of
  the code (`Api.errors.<code>` in the catalogs, one per `ErrorCode` of the
  contract plus the client's own; an unknown code gets the generic
  sentence), the Retry-After wait, the code and HTTP status, and a "Try
  again" button when the caller passes `onRetry`. The registry is
  `src/lib/api/errors.ts`: `satisfies Record<ErrorCode, true>` makes a
  contract code without an entry fail `tsc`, and `tests/api-errors.test.ts`
  reads the generated `ErrorCode` union and requires a sentence for each
  member in every catalog. The server's English `message` is never shown.
- **Failed runs**: a failed run (header, Explorer row, My runs card) reads
  the translated sentence of its failure CODE (`src/lib/run/failure.ts`,
  `Run.failure.codes.<code>`: every code `public_view::failure_message`
  knows, the live ledger's `budget_refused` and `budget_unsettled`, the Meta
  adapter's provider codes and the runtime's model-output codes), never the
  server's English message; another code gets the generic sentence, which
  names the code. `evidence_unavailable` records likewise
  (`Run.unavailable`).

## Run workspace

`src/components/run/`: `useRunData` loads the detail (`GET /runs/{id}`) and
opens the stream (`GET /runs/{id}/events`, every stored frame then the live
tail, until `stream_end`; a gap, a resync or the end re-reads the detail).
A failed detail request is kept as an error the workspace shows with a
retry (never a loading line), the responses of older requests are dropped,
and a detail never replaces one that is further along; the header shows the
newest known summary (`src/lib/run/view.ts` `newestSummary`). Frames go
through a pacer (`src/lib/run/pacing.ts`) before they are shown: a guided
run finishes in milliseconds and streams as `delivery: replay`, so the
replayed frames of a scripted run (`isPaceable` in `src/lib/provenance.ts`)
are released one every 650 ms, labelled "simulated · paced for
readability" with a waiting count and a "Skip pacing" control while frames
are queued, announced once when pacing starts and once when it ends; a live
or mixed model run is never paced or labelled simulated, live frames are
never delayed, and reduced motion (the OS setting or the in-app preference)
turns the pacing off. Server timestamps are shown untouched.

- **Live / Playback** (`StageTab`): stream status (a failed stream says the
  stage shows the stored records and offers a reconnect after the newest
  record held), pacing control, the three-agent stage (Proposer, Critic,
  Evidence Evaluator) where each card shows the latest document of its role
  with the delivery badge of the record that carried it, the runtime lane
  (records retrieved, proofs checked, recalls proven), and the
  recommendation with its evidence axis. The records are the frames shown so
  far plus what the detail holds and the stream did not deliver (a gap at
  once; the rest once the stream is settled and nothing is paced), read as
  replayed (`stageRecords`), so the counts match the run. The tab is named
  Live only while the run deliberates and its latest frame is LIVE, Playback
  otherwise.
- **Explorer** (`ExplorerTab`): the event timeline (sequence, type, actor,
  time, a one-line summary, the public event as expandable JSON), filterable
  by type; the rows are the union of the detail's events and the frames'.
  Every status is translated: a scripted proof reads "SIMULATED proof
  (scripted outcome …)", factual and relevance statuses and a decision's
  authority have their own words, a failure or an unavailable retrieval
  reads the sentence of its code.
- **Decision** (`DecisionTab`): the recommendation kind with its evidence
  axis and, right under it, the scope line when the recommendation is for a
  revision (which proposal it is for, which one it replaced, the task, both
  summaries side by side: K03/K04), rationale, protocol, unresolved
  assumptions and uncertainties, the open objections, the public decision
  form (approve or reject with an optional note; an override of a blocked or
  abstained run needs the note) with the fixed public_demo notice, enabled
  only while the run awaits a decision and belongs to this session; the
  recorded decision with the server's notice; the receipt link, and the
  verbatim log link only when the public surface publishes it
  (`ndjsonPublishable`, the server's rule: a live-declared run answers `404
  ndjson_unavailable`, said in a sentence instead); cancel for a run still
  deliberating.
- **Evidence** (`EvidenceTab`, lane W2): every record the run retrieved
  (`src/lib/run/evidence.ts`, from `mergeEvents`): kind, project, retrieval
  mode, snapshot id and capture time when present, what its proof covers
  (from the proof's own request, never the claim's query: "Proof covers:
  complete record list of <project> at anchor <prefix>." for the sealed
  snapshot, the sealed request for a live recall, a sentence for a pre-1.3
  record that names none, "no proof" when its generation failed upstream),
  its text as untrusted English content (`UntrustedText`: plain text,
  `lang="en"`, a note that it is data, and in another locale that it is the
  English original), and its verifications by round (integrity through
  `proofToken`, factual and relevance translated, never a positive tone for
  "irrelevant"); failed retrievals with their code's sentence.
- **Proof inspector** (`ProofsTab`): per verification, the technical fields
  (verifier, checked_at, verified anchor, proof ref, the report's status,
  kind, scope and digests, round, log record, whether the verifier is the
  scripted double) and a plain reading worded from the receipt's
  `proof_limitations` (`src/lib/run/receipt.ts` pins the text the server
  writes; a receipt with any other text is shown as written, untranslated):
  a simulated proof checked nothing cryptographic, a proof whose generation
  failed upstream was never checked, a recorded run's proofs were verified at
  run time by the named verifier and their artifacts stay with the operator.
- **Audit trail** (`AuditTab`): the chain (scope in words, first and last
  sequence, head; each record's link as the views carry it), the receipt
  (authorization, deliberation summary with the earlier-round objections
  never re-judged and, apart, those their own round left unlisted, model
  calls, verifications by round and status, protocol, proof limitations,
  the receipt as JSON), the verbatim log where it is
  published (a guided run's `events.ndjson`; a recorded run's research file
  with its pinned SHA-256) with the receipt download, the exact check
  `consensus-api verify <log.jsonl> --run <id> --receipt <receipt.json>`, and
  the chain checked in the browser (`ChainCheck`, `src/lib/chain/verify.ts`:
  every record's SHA-256 recomputed with WebCrypto exactly as `event_log.rs`
  does, the raw line with its top-level hash blanked textually, never
  re-serialized; every link followed; the run's segment compared with the
  pinned digest, the receipt's head and count and the detail's chain; pass,
  or the first failing record, or, for a run whose records carry no hash
  (schema 1.0), a neutral "no chain to check" with the chain "not checked"
  and the head "not compared", introduced without any claim of hashing; a
  run that began before schema 1.1 (hashless first records, chained after)
  reads as a neutral "checked in part" naming its unverified records: the
  green "Chain checks out" only for a run whose every record was verified
  link by link, and its record count only over the records that carry a
  hash). Every redacted view (the links, the receipt
  JSON, the Explorer's public event) says it is a display copy, not the
  hashed bytes.
- **Meta API** (`MetaTab`): one row per recorded model invocation
  (`src/lib/run/invocations.ts`, from the events that carry them, so each
  row has its role): provider and model, recorded request id, latency,
  input, output and reasoning tokens, output bound, finish reason, attempts,
  schema name and hash prefix, prompt hash prefix; a scripted double reads
  "scripted double, no model call" with nothing that passes for a call;
  totals from the rows, over real calls only. The contract publishes no
  prices: no cost is shown.
- **Explanations** (`Explanation`, in the Decision tab): the policy trace in
  plain words (`src/lib/run/explain.ts`: the deciding rule with its kind, the
  trace's counts, one sentence per failed check) and the technical view
  (rule and rule order, every failed check with its ids, the counts by field,
  the rationale code, the server's English rationale), expanded at the
  "technical" explanation level; what the records add is said apart from it
  (SIMULATED proof statuses; before protocol 1.3, the final round only and
  "earlier-round objections never re-judged: N", then, when some of them
  were not listed unresolved even by their own round's evaluation, how many:
  the receipt's `unresolved_in_its_round` false, also on the Audit trail).
  When every record the invalid_proofs check names reports that no proof
  could be generated upstream, the lead and that sentence say nothing was
  checked, as the Proof inspector does, never that a check came back
  invalid, and the proofs count reads the same trace counts as integrity
  statuses (`plain.counts.proofs_ungenerated`), never as proofs found
  invalid or none failed.
- **Revision view** (`RevisionComparison`, next to the scope line): the
  claims of the original and the revised proposal
  (`src/lib/run/revision.ts` `claimComparison`), and each revision's
  addressed items, said as its own claim, not a verdict. The records relate
  a revision to the proposal it supersedes only as a whole; a claim carries
  no link to an earlier claim but its id, and the Meta model adapter numbers
  every proposal's claims anew, so no live-model revision shares a claim id
  (none in research/ does; tests/run-revision.test.ts checks them all). When the two share ids (scripted runs),
  claim by claim by id: unchanged, changed, added, dropped, with a sentence
  when only some ids are shared (ids alone cannot tell a dropped claim from
  one restated under another id). When they share none, the two lists side
  by side with a sentence saying nothing pairs them, and no claim is marked
  added, changed or dropped; the Meta model adapter is named as the cause
  only when every revision of the run records it as provider
  (`revisedByMetaModel`).
- **RECORDED mode** (`?source=showcase`, `useRecordedRun`): a run of the
  research archive read once from `GET /showcase/{id}` with its showcase
  entry and the manifest's note, and its receipt from `/showcase/{id}/receipt`;
  no stream and no stage, Decision first and read-only, never presented as
  running; the header says it is recorded and carries what the archive data
  supports: schema 1.0 reads "pre-1.1, unchained", and a run blocked because
  every invalid proof's record reports an upstream proof error reads as a
  failure to produce proofs, not an alteration, next to the manifest's own
  note on its log (for the 2026-10-26 battery: a deployment misconfiguration).
  Its receipt is the server's rebuild from the archived log on each request
  (`generated_at` is the request's time, `proof_limitations` today's text):
  the Proof inspector and the Audit trail say so next to the limitations and
  the receipt download (`RunTimeReceiptNote`), link the run record that holds
  the receipt downloaded at run time (`record_file`, at `/research/<path>`),
  and on request quote that receipt's own limitations and time
  (`runTimeReceipt` in `src/lib/run/receipt.ts`), saying whether they differ.
  The Decision tab's receipt link says the same in one sentence
  (`Run.decision.receiptRebuilt`) and points to the Audit trail.

Honesty rules the views follow (release gate G7; `src/lib/provenance.ts`,
each with a unit test): LIVE only when a frame's delivery is `live`, the
model axis is `live_meta` and no flag is raised (`provenance_inconsistent`,
`unchained` and `mode_undeclared` are shown and never render as LIVE; a
live-delivered record of a scripted run reads STREAMING); every label with a
LIVE word (the LIVE delivery badge, the LIVE META model badge, "Meta model ·
live", the Live tab) goes through `isLive`, and no evidence or source label
carries the LIVE word (the evidence of a live Hyphae recall reads HYPHAE
RECALL), so a replayed frame, a list entry, a preset, a recorded run or a
flagged run reads META MODEL (a warning with a blocking
flag) with "Meta model · recorded <date>" next to REPLAY, in every locale
(`tests/provenance.test.ts` checks each locale's LIVE word); a scripted
provider reads "scripted double, no model call"; simulated evidence is named
next to every outcome ("supported — on SIMULATED evidence"); a simulated
proof is the neutral SIMULATED token with its scripted outcome as text,
never a green "valid"; every count comes from the run data; nothing
pretends to load.

## Conventions for the views that follow (lane W2)

The Evidence tab, Proof inspector, Audit trail, Meta API panel, showcase
detail and the plain/technical explanations of `PolicyTrace` build on what
exists; none of it is to be duplicated.

- **One run, one data source.** `useRunData(runId)` (mounted once by
  `RunWorkspace` under `key={runId}`) is the only reader of
  `GET /runs/{id}` and the stream; a new tab takes `data: RunData` (or
  `detail`/`frames`) as props and adds itself to `TabKey` and the `items` of
  `<Tabs>` in `RunWorkspace.tsx` (one panel, `run-tabpanel`). Records come
  from `src/lib/run/view.ts`: `mergeEvents` (every record of the detail and
  the frames: the Audit trail's list), `stageRecords` (what pacing has shown),
  `newestSummary`, `supersededProposal`. The Explorer already lists the
  events with their public JSON: the Audit trail is about the chain
  (`detail.chain`, `prev_hash`/`hash`, the receipt), not a second timeline.
- **Requests** go through `api` (`src/lib/api/client.ts`); a new endpoint
  gets a method there and a zod mirror in `schemas.ts` (`z.looseObject`,
  strict enumerations, `satisfies z.ZodType<T.X>` against the generated
  `types.ts`, which is never edited by hand: `pnpm api:types`). The showcase
  client methods exist (`showcase`, `showcaseDetail`, `showcaseReceipt`);
  archived runs are not streamed and are never presented as running:
  `source: research_archive` reads RECORDED, and without a delivery the
  model badge is META MODEL by construction.
- **Labels** come only from `src/lib/provenance.ts` (`isLive`,
  `modelLabel(provenance, delivery?)`, `deliveryLabel`, `evidenceLabel`,
  `sourceLabel`, `flagLabel`, `modelNoteKey`, `proofToken`,
  `ndjsonPublishable`, `isPaceable`) and `<ProvenanceBadges>` (pass
  `delivery` only for a frame being watched). A simulated proof is never a
  green or cyan "valid" (`proofToken`; `ProofStatus` in `StageTab.tsx`
  renders it); Hyphae cyan is for real proof and evidence only. Request ids
  (`meta_request_ids`, `ModelInvocationV1.request_id`) are evidence of a
  recorded Meta call, never of liveness.
- **Errors and failures**: `<ApiErrorNotice error onRetry>` (every
  `ErrorCode` has a sentence; `error.message` is never shown); a failure
  code through `FailureSentence` / `useFailureText` / `useUnavailableText`
  (`src/components/run/FailureSentence.tsx`). A settled failure is never a
  loading line.
- **Explanations**: `RecommendationV1.trace` (`PolicyTrace`: rule, reasons,
  counts, version) and `rationale_code` are the data; render them through
  catalog templates keyed by rule and reason, plain or technical after the
  `explanation` preference (`usePreferences()`), and keep the server's
  English `rationale` for the technical level only (the Decision tab still
  shows it as recorded).
- **i18n**: every string in `messages/{en,es,pt-BR,fr}.json`
  (`react/jsx-no-literals`); a namespace a client component reads must be
  listed in `clientNamespaces` (`src/i18n/messages.ts`); computed keys go
  through `dynamicTranslator` with a unit test that every computed key exists
  in the four catalogs (as `tests/provenance.test.ts`, `tests/api-errors.test.ts`,
  `tests/run-failure.test.ts` do); every plural carries `=0`, `one` and
  `other` (`tests/plurals.test.ts`; Spanish keeps its `many` branches); no
  enumeration value is interpolated raw (translate it, as
  `Run.explorer.factual|relevance|authority` and `Provenance.proof.*` do);
  rich text only with `RICH_TAGS` and `richTags(locale)`.
- **Tests**: logic lives in `src/lib/**` and is unit-tested in Vitest's node
  environment (there is no DOM test setup); the views are covered by
  `e2e/api.spec.ts` over `mockApi()` (`detail`, `frames`, `extraRuns`,
  `detailFailures`, `streamFailures`, `liveEnabled`/`live*`, `capabilities`,
  `health`), with variants derived from the fixtures (`awaitingDecisionDetail`,
  `liveModelDetail`, `framesWith`, `inProgressLiveRun`, `failedDetail`,
  `liveR01Run`); the showcase routes are mocked too (`showcase`,
  `recordedRuns`, `research`, `showcaseFailures`; `recordedRun()` derives a
  recorded run from a guided fixture with its real segment's chain, and
  `e2e/research.ts` builds entries from the archive's manifest and logs), and
  so is a guided run's verbatim log (`ndjson`, `chainedRun()`). The shared
  assertions are in `e2e/helpers.ts`. Use `expectNoLive(page, locale)` for
  every view that must not read LIVE, `takeProblem` for each API error a
  test provokes, `reducedMotion: "reduce"` to skip pacing, and add the new
  tabs to the axe scans (`writeScans`, `blocking`).
- **CSP**: no Astryx component built on `Layer` (TabList, popover, tooltip,
  dialog, `disabledMessage` tooltips, a TextInput `status` message): they
  inject a `<style>` element the hash-only policy blocks; no `new Function`
  (zod stays jitless); no inline event handler attributes.

## Live form

`src/components/new/NewDeliberation.tsx` over `src/lib/live/form.ts` (the
rules, unit-tested) and `src/lib/live/turnstile.ts` (the widget):

- `capabilities.live` is mirrored exactly (`LiveCapabilitiesV1`, `PresetV1`,
  `PublicBudgetV1` in `src/lib/api/schemas.ts`). Each preset shows the
  badges of its own model and evidence axes (no delivery, so never the
  LIVE-worded model label); the budget is shown with translated labels in
  US dollars, cut to whole cents on the ledger's decimal string, never
  rounded up.
- The form is closed, with one sentence and the showcase link, when the
  ledger is tripped (with the trip code's reason), when no budget is
  published (the ledger could not be read: nothing can be admitted), when no
  preset is listed, or when the server requires Turnstile and this build has
  no site key (every request would be refused). A closed form lists the
  presets to read and offers no control.
- An open form asks for a preset, the attestation (when
  `requires_adult_attestation`: 18 or older and not using live mode from a
  restricted territory, as the server records it, linking to the Terms;
  legal wording pending the owner's sign-off), an optional invite code (sent
  trimmed, only when 1 to 256 characters remain) and, when
  `requires_turnstile`, a Turnstile token: Start stays disabled until each
  is there.
- Turnstile (`src/components/new/Turnstile.tsx`) loads
  `challenges.cloudflare.com` only when `NEXT_PUBLIC_TURNSTILE_SITE_KEY` was
  set at build time, and then at runtime (never from the exported HTML,
  which the CSP manifest would reject). The widget is rendered with the
  action the server verifies (`live-run`, `CONSENSUS_TURNSTILE_ACTION_LIVE`).
  A token is single-use (the gate spends it at Siteverify before it reserves
  the budget, whatever it answers): every submit hands it to that request
  only and resets the widget. The server's CSP has to allow that origin in
  `script-src` and `frame-src` when the key is set.

Manual smoke against the real server (host side, read-only use of the
prebuilt `target/debug/consensus-api.exe serve-public` with
`CONSENSUS_PUBLIC_BIND=127.0.0.1:8788`, `CONSENSUS_WEB_ROOT=apps/web/out`,
`CONSENSUS_RESEARCH_DIR=research`, a temporary `CONSENSUS_RUN_STORE_DIR` and
`CONSENSUS_COOKIE_SECURE=false`; the e2e container cannot reach it): curl the
routes and save the bodies into the git-ignored `test-results/smoke/`
(`health.json`, `capabilities.json`, `scenarios.json`, `session.json`,
`run-list.json`, `run-detail.json`, `receipt.json`, `showcase.json`,
`showcase-detail.json`, `showcase-receipt.json`, `showcase-detail-live.json`,
`showcase-receipt-live.json`, `events.sse.txt`). `tests/api-smoke.test.ts`
then validates every capture against the zod mirrors (and the SSE capture's
sequence and ids); without a capture it skips. Done on 2026-10-28 for a
guided `retry` run and the archived runs: every capture parsed.

Docs and legal statements come from the root README, `docs/threat-model.md`,
the multi-agent (AI) audit of 2026-10-27 and the approved plan, with the owner's decisions of
2026-10-27 for the legal texts: the whole portal is geo-blocked at country
level for Meta's Restricted Territories (Hong Kong and Macau named as part of
China; all of Ukraine, because the listed regions cannot be isolated at
country level) and South Korea for live mode only; the data controller is
Mario Gutiérrez with a public contact address (`CONTACT_EMAIL` in
`src/lib/site.ts`, a marked placeholder until the owner provides it); hosting
is a Linux server administered by the owner at "the owner's hosting provider"
behind Cloudflare (`HOSTING_COUNTRY`, a marked placeholder until the owner
confirms it); retention is run files deleted whole after 7 days except a
one-line run-index entry and the budget-ledger cost entry, hashed-address
tables in memory and bounded, server/cloudflared/host logs under the host's
rotation, and an unpublished operator detail log keyed by request id.
**The legal texts (Terms, Privacy, Limitations, About) are drafts for the
owner's review** in all four languages; the ES/PT/FR texts are drafts too and
need the Day-5 language sign-off.

Honesty rules the texts follow: a valid proof shows that the Hyphae recall it
seals reproduces against its pinned anchor, not yet that the displayed text is
the sealed text (audit finding HY-1), and never that a record is true,
relevant or complete or that a model used it; unbuilt features are described
in the future tense; LIVE META is never shown alone but next to its evidence
label (SEALED SNAPSHOT), and the archived hybrid runs of a live model with
simulated proofs read META MODEL next to SIMULATED, never LIVE META (the
Limitations page and the Modes article say so since 2026-10-28, pending the
owner's sign-off); runs recorded under protocol 1.2 read only the
final round and the 2026-10-27 multi-agent (AI) audit found two fail-open
paths (MA-1, RT-1), while protocol 1.3 rejects inexact ids and carries every
open objection and contradiction forward and has not yet been re-validated
live.

## Build only through Docker

Never run `node`, `npm`, `pnpm`, `npx` or any script of this app on the host.

Why: the owner's Windows user environment holds the Meta API key, and every
process started on the host inherits it. Dependency code does not only run at
install time (where build scripts are blocked here); it also runs during
`next build`, linting and tests (Next config and plugins, the CSS pipeline,
ESLint plugins, Vitest, Playwright), with the caller's full environment and
file access. One compromised package would then reach the key, `~/.ssh`,
`~/.cloudflared` and the git credentials. So the whole Node toolchain runs in
throwaway containers that see `apps/web` and nothing else (red-team finding
ABUSE-3).

From the repository root, in Git Bash (or any POSIX shell on Linux or macOS):

```sh
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm check          # theme, types, lint, Vitest, export, CSP manifest
sh apps/web/scripts/playwright-docker.sh             # e2e + axe against out/
```

`scripts/in-docker.sh` runs `docker run --rm` on `node:24-bookworm-slim`
pinned by digest (Node 24.22.1), mounts `apps/web` at `/app`, the
repository's `contracts/` (the API contract the type generator and the tests
read) read-only at `/contracts` and `research/` (the archive the chain
verifier's tests read, public data) read-only at `/research`, passes no `-e`
variables, no home directory
and no TTY, enables corepack, switches off Next.js telemetry inside the
container, and runs the given command. Two parts of the `/app` mount are
read-only, so code running in the container can never rewrite what the host
runs or trusts: `scripts/` (the two `*.sh` files run on the host, where the
Meta key is set) and `pnpm-workspace.yaml` (the supply-chain policy). The
rest stays writable because the toolchain writes there: `node_modules/`
(pnpm install, with its store in `node_modules/.pnpm-store`), `.next/` and
`next-env.d.ts` (`next typegen`, `next build`), `tsconfig.tsbuildinfo`
(`tsc`), `out/` (the export and `csp-manifest.json`), and
`src/lib/api/types.ts` (`pnpm api:types`). Nothing else in `apps/web` is
written by a command of this project.

Where writes through the bind mount are slow (Docker Desktop on Windows: a
`pnpm install` there took hours on 2026-10-29), set
`HC_WEB_NODE_MODULES_VOLUME=<name>` on the host for both scripts. `node_modules/`
(and pnpm's store with it) then lives in that named Docker volume, mounted
read-only for Playwright, and the same install takes about 30 seconds. Use
one volume per checkout and remove it with `docker volume rm <name>` when
the checkout goes away:

```sh
export HC_WEB_NODE_MODULES_VOLUME=hcw-main
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm check
sh apps/web/scripts/playwright-docker.sh
```

`scripts/playwright-docker.sh` runs `playwright test` in
`mcr.microsoft.com/playwright:v1.64.1-noble`, pinned by digest
(`sha256:47b510dd…b8ea`, Node 24.22.1, the browsers of Playwright 1.64.1, the
same release as the pinned `@playwright/test`). Same isolation as
`in-docker.sh` (the contract and the research archive are mounted
read-only at `/contracts` and `/research` too: the API specs serve the
contract's fixtures as the mocked `/api/v1` and the archive's real bytes as
the mocked `/research`), plus `--network
none`: the tests serve `out/` on the container's own loopback, so nothing a
page loads can come from anywhere else. The real server cannot be reached
from that container, so the API is always mocked with `page.route()`
(`e2e/api-mock.ts`), including the SSE stream (the frames fixture as one
`text/event-stream` body). The container is further confined: `/app` is mounted read-only with
`test-results/` as the only writable directory (Playwright's transform cache
goes to the container's `/tmp`), it runs as the image's unprivileged `pwuser`
(so `test-results/` is not root-owned on Linux and macOS), every capability
is dropped and no process can gain privileges (`--cap-drop ALL
--security-opt no-new-privileges`; Playwright launches Chromium with its
sandbox off by default, so none is needed). Extra arguments go to
`playwright test` (for example `--project=chromium e2e/a11y.spec.ts`). It
needs a built export.

Notes:

- `node_modules` is created by Linux inside the container. It holds Linux
  binaries and Linux symlinks, so Windows tools cannot use it (by design).
  pnpm 12 keeps its package store in `node_modules/.pnpm-store` when only the
  project is mounted, so later installs reuse downloaded packages.
- The bind mount from Windows is slow: `pnpm build` takes about 50 s, a full
  `pnpm check` several minutes.
- Never run `astryx init`: it writes the Astryx component index into
  `AGENTS.md`/`CLAUDE.md`, which would change the instructions of coding
  agents working on this repository. Run the Astryx CLI only through the
  `theme:*` scripts or `pnpm exec astryx …` inside the container, and check
  `git status` afterwards for new `AGENTS.md`, `CLAUDE.md`, `.cursor*` or
  `.github/copilot-instructions.md` files.

## Scripts

| Script | What it does |
|---|---|
| `pnpm install --frozen-lockfile` | Install exactly what `pnpm-lock.yaml` records |
| `pnpm typecheck` | `next typegen`, then `tsc --noEmit` (strict) |
| `pnpm lint` | ESLint (`eslint-config-next` core-web-vitals + TypeScript), zero warnings allowed |
| `pnpm test` | Vitest: locale parity, ICU and rich-text tags of every catalog, =0 branches of every plural, site map, preferences, theme contrast, theme script, CSP manifest, static server, i18n helpers, the generated contract types (drift and hash), the zod mirrors over every contract fixture (per-fixture expectations, the live section), a sentence for every `ErrorCode` read from the generated union, the API client (judge code trimming), the SSE stream, the pacer, the honesty predicates (no LIVE word unless `isLive`, in every locale), the live form's rules, the Turnstile widget, the run view helpers and the failure sentences; lane W2: the evidence records and proof readings, the receipt's pinned limitations and the check command, the invocation rows and totals, the explanations rendered in four locales (exactly the trace's counts and kind), the revision comparison, the showcase figures and manifest notes, no LIVE word on an evidence or source label, and the chain verifier over the real archive at `/research` (every pinned digest, all eight logs, F03's head, a flipped byte failing at its sequence) |
| `pnpm build` | Static export to `out/`, then `node scripts/csp-manifest.mjs` |
| `pnpm theme:build` | Compile `src/theme/hyphaeMissionTheme.ts` with `astryx theme build` |
| `pnpm theme:check` | Fail if the committed theme outputs drift from the source |
| `pnpm api:types` | Regenerate `src/lib/api/types.ts` from `contracts/api-v1` (`scripts/gen-api-types.mjs`) |
| `pnpm api:check` | Fail if the committed contract types drift from the schemas |
| `pnpm check` | `theme:check`, `api:check`, `typecheck`, `lint`, `test` and `build`, in that order |
| `pnpm serve` | `scripts/serve-static.mjs`: serve `out/` on 127.0.0.1:4173 with per-page CSP (inside a container) |
| `pnpm e2e` | Fails fast (exit 64) with a message pointing at `scripts/playwright-docker.sh`: the Node container has no browsers, and the suite runs only there |

Lint rules beyond the Next.js defaults: `react/no-danger` (the theme script
file is the single exception) and `react/jsx-no-literals` (UI text comes from
the message catalogs).

## Supply chain

- **pnpm 12.15.0**, pinned with its sha512 through `packageManager`, so
  corepack verifies the package it downloads (the pin matches the registry's
  integrity value). pnpm 12 is a native binary; according to its wrapper's
  source, the wrapper downloads that binary from the npm registry and checks
  npm's registry signature.
- **Exact versions only**, and the lockfile is committed. Installs use
  `--frozen-lockfile`.
- **Release cooldown**: `minimumReleaseAge: 4320` (3 days) in
  `pnpm-workspace.yaml`. Because the value is explicit, pnpm enforces it
  strictly: adding `motion@14.7.1` (published the same day) fails with
  `ERR_PNPM_NO_MATURE_MATCHING_VERSION`. pnpm 12 also fails on any setting it
  does not recognize in that file (`minimumReleaseAg` gives
  `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS`), so a misspelled policy cannot
  be dropped silently.
- **Dependency build scripts**: pnpm 12 runs none unless a package is set to
  `true` under `allowBuilds`, and fails the install while any is undecided.
  The five packages that have scripts are denied, each with the reviewed
  reason in `pnpm-workspace.yaml`; the build works without them. The test
  packages added on 2026-10-27 (`@playwright/test`, `playwright`,
  `playwright-core`, `@axe-core/playwright`, `axe-core`) have no install
  scripts.
- **Browsers** come from the digest-pinned Playwright image, never from
  `playwright install`.
- **`pnpm audit --prod`** and **`pnpm audit`**: "No known vulnerabilities
  found" (2026-10-27, with the test packages).

## Versions

Pinned 2026-10-27, each the newest release at least 3 days old unless an
exception is noted.

| Package | Version | Published | Note |
|---|---|---|---|
| `next`, `eslint-config-next` | 16.4.3 | 2026-10-23 | |
| `react`, `react-dom` | 19.3.1 | 2026-10-14 | |
| `@astryxdesign/core`, `theme-neutral`, `cli` | 0.7.4 | 2026-10-23 | Beta; minor releases have carried breaking changes |
| `@stylexjs/stylex` | 0.19.1 | 2026-09-15 | Peer dependency that Astryx imports at runtime, listed explicitly (Astryx issue #4276) |
| `next-intl` | 4.14.9 | 2026-10-02 | |
| `motion` | 14.6.0 | 2026-10-22 | Exception: 14.7.0 and 14.7.1 were published 2026-10-27, inside the cooldown |
| `typescript` | 6.0.3 | 2026-04-16 | Exception: 7.0.2 is newer, but typescript-eslint 8.72.0 (used by `eslint-config-next`) requires `<6.1.0` |
| `eslint` | 9.39.5 | 2026-07-10 | Exception: 10.13.0 is newer, but `eslint-plugin-react` 7.37.5, `eslint-plugin-import` 2.32.0 and `eslint-plugin-jsx-a11y` 6.10.2 (dependencies of `eslint-config-next` 16.4.3) declare peers only up to ESLint 9, and `create-next-app` 16.4.3 pins `^9`. npm marks 9.x "no longer supported" |
| `vitest` | 5.0.5 | 2026-10-15 | |
| `vite` | 8.3.5 | 2026-10-21 | Required peer of Vitest 5 |
| `intl-messageformat` | 11.2.15 | 2026-09-12 | 11.x on purpose: next-intl and Astryx use 11.x; 12.1.4 is newer |
| `zod` | 4.6.7 | 2026-10-12 | Runtime validation of API responses (`src/lib/api/schemas.ts`); no install scripts |
| `parse5` | 8.0.1 | 2026-04-19 | HTML parser for the CSP manifest |
| `@playwright/test` | 1.64.1 | 2026-10-14 | Newest stable (1.65 is in alpha); brings `playwright` and `playwright-core` 1.64.1 |
| `@axe-core/playwright` | 4.14.0 | 2026-10-15 | Brings `axe-core` 4.14.0 (2026-10-05), already in the lockfile through `eslint-plugin-jsx-a11y` |
| `@types/node` | 24.19.2 | 2026-10-09 | Matches the Node 24 runtime; 26.x is newer |
| `@types/react`, `@types/react-dom` | 19.3.0 | 2026-09-09 | |
| `pnpm` | 12.15.0 | 2026-10-21 | |
| Image `node:24-bookworm-slim` | `sha256:55fb5f5a…d205` | pulled 2026-10-27 | Node 24.22.1 |
| Image `mcr.microsoft.com/playwright:v1.64.1-noble` | `sha256:47b510dd…b8ea` | built 2026-10-23, pulled 2026-10-27 | Ubuntu 24.04, Node 24.22.1, Chromium/Firefox/WebKit of Playwright 1.64.1 |

## Astryx

Prebuilt path only: no swizzled components, no StyleX compiler, no Tailwind.

- **Stylesheets** (`src/app/styles.ts`, imported by both root layouts), in
  layer order: `@astryxdesign/core/reset.css` (`reset`) →
  `@astryxdesign/core/astryx.css` (`astryx-base`) →
  `src/theme/hyphae-mission.css` (`astryx-theme`) → `src/app/globals.css`
  (`app`). App rules stay inside `@layer app`, because unlayered rules would
  beat every Astryx layer.
- **Theme**: `src/theme/hyphaeMissionTheme.ts` extends the neutral theme with
  `defineTheme`. `pnpm theme:build` writes the committed
  `hyphae-mission.{css,js,d.ts,css.d.ts,variants.d.ts}`; rebuild after any
  Astryx upgrade. Tokens: accent `#0064E0` (light) and `#4BA9FE` (dark, text
  and icons); text on accent white in light mode and `#111112` in dark mode
  (white on `#4BA9FE` measures 2.50:1 and fails WCAG AA); surfaces white and
  ice blue in light mode (body `#F3F7FC`, muted `#EDF3FB`) and deep midnight
  in dark mode (body `#0A0F1E`, surface `#121A2D`, card `#0F1628`, popover
  `#141D33`), with a soft blue ambient glow behind the content sheet; Hyphae
  cyan `#45E4D0` as local tokens reserved for proof and evidence (nothing in
  E2 uses it); radii 8 px (element) and 16 px (container).
  `tests/contrast.test.ts` reads the compiled theme and requires 4.5:1 for
  every text token on every surface token, in both modes. Fonts are set
  through the typography roles, not plain tokens: neutral's
  touch-and-narrow adaptation re-derives the font tokens from those roles.
- **Fonts**: Inter (variable, with optical size) and JetBrains Mono through
  `next/font/google`, downloaded at build time and served from
  `/_next/static/media`. theme-neutral's `fonts.css` is never imported (it
  loads Figtree from Google at runtime). `.hc-tabular-nums` and `.hc-code`
  are the utilities for metrics and for hashes and ids.
- **Components used**: `Button`, `RadioList`, `Switch`, `Badge`, `Heading`,
  `Text`, `CheckboxInput`, `TextInput` and `TextArea`. Long-form content
  (docs, legal pages) is plain HTML styled by the theme's reset layer, so it
  needs no hydration. Dialogs (tour, mobile drawer) are our own
  `src/components/Modal.tsx` on the native `<dialog>`, with a strict focus
  trap; the run workspace's tabs are our own `src/components/run/Tabs.tsx`
  (WAI-ARIA tabs pattern, classes only).
- **Not usable under the CSP**: Astryx components built on its `Layer`
  (`TabList`, whose overflow menu goes through `useLayer`, and the popover,
  tooltip and dialog family) append a `<style>` element at runtime
  (`Layer/layerSlideRules.js`). The portal's policy is hash-only
  (`style-src 'self'` plus the hashes of the exported HTML), so the browser
  blocks that element and the e2e fixture fails the page on the violation.
  Disclosures use the native `<details>`; anything layered needs our own
  component or a CSS-only solution.
- **Client boundaries**: `Theme`, `LinkProvider`, Astryx i18n,
  `NextIntlClientProvider`, `MotionConfig` and the tour live in
  `src/components/Providers.tsx`; the preference state is
  `src/components/preferences.tsx`.
- **Astryx strings**: Astryx resolves a locale only from specific to general,
  so its `es-ES`, `pt-BR` and `fr-FR` catalogs (default exports of
  `@astryxdesign/core/locales/<tag>.generated.js`) are registered under `es`,
  `pt-BR` and `fr`. Astryx's own Spanish strings are therefore Spain Spanish,
  while the app's catalog is neutral Latin American Spanish.
- **License**: `@astryxdesign/core@0.7.4` ships **no LICENSE file** (its
  `files` list omits it); its `package.json` says MIT.
  `@astryxdesign/theme-neutral` and `@astryxdesign/cli` 0.7.4 ship the
  standard MIT text, "Copyright (c) 2026 Meta Platforms, Inc.". Notices for
  core have to carry that text.

## Preferences (localStorage only, never cookies)

| Key | Values | Default when absent |
|---|---|---|
| `hc-theme` | `light`, `dark` | Match the system (read by the head script) |
| `hc-motion` | `reduce` | Match the system's `prefers-reduced-motion` |
| `hc-explanation` | `technical` | `simple` |
| `hc-locale` | `en`, `es`, `pt-BR`, `fr` | The root page negotiates `navigator.languages` |
| `hc-tour` | `skipped`, `completed` | The Overview invites to the tour |
| `hc-notice` | `locale` | One-shot: Settings confirms a language change on the next page |

Settings writes them (a default removes the key), confirms each change in a
polite live region, and can remove them all. Other tabs follow through the
`storage` event.

## Color mode and motion

`src/components/themeScriptSource.ts` holds a one-line inline script, the
app's only `dangerouslySetInnerHTML` (`ThemeScript.tsx`). In `<head>`, before
the body is parsed, it sets `data-theme` on `<html>` from localStorage
`hc-theme`, or from `prefers-color-scheme` when nothing valid is stored.
"Match system" in Settings removes the key; the page then follows the OS,
live.

The static HTML renders Astryx's root `Theme` in "system" mode, and hydration
must match it. The preference state reads the head script's choice during the
first client render and applies it in a layout effect, before the first
paint after hydration. Until then, `globals.css` makes the root Theme wrapper
inherit the color scheme of `<html>`, including Lightning CSS's
`--lightningcss-light` / `--lightningcss-dark` switches (the build compiles
every `light-dark()` into them). The header switch shows "off" in the static
HTML and takes its real state on hydration.

Reduced motion comes from the OS (`prefers-reduced-motion`) or from Settings
(`data-motion="reduce"` on `<html>`), and is tiered: the drawer's slide and
the dialog's small shift go, short opacity fades stay, everything else is cut
to 0.01 ms (so `transitionend` still fires), and Motion gets
`reducedMotion="always"`. Frosted glass is limited to the top bar, the
sidebar and the two dialogs, with a solid background under
`prefers-reduced-transparency: reduce`, without `backdrop-filter` support,
and in forced-colors mode.

## i18n

next-intl 4 with static rendering and no middleware or proxy:
`generateStaticParams` and `dynamicParams = false` in the `[locale]` layout
and the docs route, `setRequestLocale` in every localized layout and page,
and a fixed `UTC` time zone.

Each locale has three catalog files, merged by `src/i18n/messages.ts`:
`messages/<locale>.json` (UI), `messages/docs/<locale>.json` (Docs) and
`messages/legal/<locale>.json` (Legal and About). Keys are typed from the
English files. Only the UI namespaces the client components read (`Nav`,
`Shell`, `Theme`, `LocaleSwitcher`, `Settings`, `Tour`) are serialized into
every page; docs and legal texts reach the browser only as the HTML of the
page that shows them. Rich text uses a fixed set of tags (`src/i18n/richTags.ts`:
`strong`, `em`, `code`, `en` for an English term kept in a translation, which
renders `lang="en"`, and links to fixed destinations), so a translation
cannot add a URL or markup. The tests require, for every file: identical key
sets in all four locales, valid ICU, the same rich-text tags as English
(`<en>` excepted, since English has nothing to mark), no URLs or HTML in the
text, no shared top-level namespace, and the required facts in the legal
drafts. The legal pages' date is an ICU argument (`{date, date, long}`),
formatted per locale from the page's own `LEGAL_TEXT_CHANGED` entry; the contact address and the
hosting country are ICU arguments too (`{contact}`, `{hostingCountry}`),
filled from the constants in `src/lib/site.ts`. Counts use an explicit `=0`
branch (CLDR maps 0 to "other" in Spanish but to "one" in Portuguese and
French). French text uses no-break spaces before `: ; ! ?` and inside « ».

The root page `/` (its own root layout) lists the four languages, sends
JavaScript visitors to the language they chose before (`hc-locale`) or else
to their browser's preference, and sends visitors without JavaScript to
`/en/` through a `<noscript>` refresh.

## Accessibility

- Skip link (first focusable element) to `<main id="main" tabindex="-1">`;
  landmarks: banner, "Primary" and "Language" navigation, main, contentinfo
  with its own navigation. Section headings are plain headings, not region
  landmarks.
- Focus management on route change (`src/components/RouteFocus.tsx`): after
  every client-side navigation, focus moves to the new page's `<h1>`; Next's
  route announcer announces the title.
- Current page marked with `aria-current` and by weight and an indicator
  bar, not color alone. Touch targets at least 40 px high in the navigation.
- Lists styled without markers carry `role="list"` (Safari drops the list
  semantics otherwise). The Settings status box is sticky, and while it has
  text the document gets `scroll-padding-block-end`, so focus scrolling keeps
  the focused control above it (WCAG 2.2, 2.4.11). The docs cards hand their
  focus ring to the card only inside `@supports selector(:has(*))`; the Menu
  icon's bars use `CanvasText` in forced-colors mode. English terms kept in
  translations (outcome names such as "(blocked)") carry `lang="en"`.
- The root page has one `<h1>` (the wordmark) and the non-affiliation line;
  the 404 page has `lang`, one `<h1>`, the four language links and the
  non-affiliation line.
- Dialogs: native modal `<dialog>` (the page behind is inert), named by their
  title, Escape and backdrop close them, Tab and Shift+Tab wrap inside, and
  focus returns to the opener (or to the `<h1>` when the opener is gone). The
  tour keeps focus on its primary button and announces each step in a polite
  live region.
- axe (2026-10-28, full suite, Chromium 1.64.1 build): 300 scans (17 pages ×
  4 locales × 2 themes, the tour and the drawer in each locale and theme, the
  404 page in both themes, and 4 phone layouts; connected to the mocked API,
  /new, /runs and the run workspace's seven tabs × 4 locales × 2 themes, and
  the open live form, a failed run and a failed detail in both themes; the
  Overview's figures, /showcase and a recorded run's six tabs × 4 locales ×
  2 themes): **0 violations of any impact**, 13,250 rule passes, 141,919
  element checks passed. `color-contrast` stays "incomplete" on every scan:
  axe cannot compute contrast over the body gradient and the translucent
  glass (the wordmark, the Menu button, the footer line); the solid
  surfaces are covered by `tests/contrast.test.ts`.
- axe (2026-10-29, full suite, Chromium 1.64.1 build): 336 scans, the 300 above plus
  the failure and edge states of the showcase and a recorded run (a failing
  and a partial chain check, a schema-1.0 run and its chain check, a forged
  manifest on the list and on a run, a failed listing, a failed recorded
  detail, a run the listing does not name: 10 × 2 themes, fr) and the
  Overview's figures, /showcase and a recorded run's six tabs on a phone
  (390x844: 8 × 2 themes, fr): **0 violations of any impact**, 14,914 rule
  passes, 165,678 element checks passed.

## CSP manifest

`scripts/csp-manifest.mjs` runs after the export. It parses every
`out/**/*.html` with parse5, hashes each inline `<script>` and `<style>` body
(sha256, base64), and writes `out/csp-manifest.json`: page path → hashes. It
fails on inline event handler attributes, `javascript:` URLs, and
subresources loaded from another origin (including absolute URLs in the
exported CSS). serve-public is meant to send each page
`script-src 'self' <that page's hashes>`; when Turnstile is added, its origin
has to be allowed in the script and in `script-src` / `frame-src`.

On 2026-10-27 the export had 72 HTML files. Each locale page has 4 inline
scripts (the theme script with the same hash everywhere, two Next.js
flight-data scripts and Turbopack's chunk-loader bootstrap), no inline
style, and 2 `style` attributes (the header and drawer theme switches; 9 on
Settings, from the radio lists), so the policy needs
`style-src-attr 'unsafe-inline'`. The 404 page (`/404.html`, also exported as
`/404/` and `/_not-found/`) is rendered from `src/app/global-not-found.tsx`
and has the same 4 inline scripts, no inline style and no style attribute;
the e2e server sends it with its own hashes for every unknown path.

The e2e server (`scripts/serve-static.mjs`) sends every page the policy built
from the manifest (`default-src 'none'`, `script-src 'self'` plus the page's
hashes, `style-src-attr 'unsafe-inline'`, `frame-ancestors 'none'`, and so
on), and the suite fails on any CSP violation, console error, failed
same-origin request or request to another origin.

## End-to-end tests

`e2e/` (Playwright 1.64.1, run through `scripts/playwright-docker.sh`):

| Spec | Covers |
|---|---|
| `navigation.spec.ts` | Every primary nav item in every locale (URL, `<h1>`, focus on the `<h1>`, `aria-current`, `lang`, title); a crawl of all 69 pages that requests every internal link without following redirects and allows only the four known external links; unknown paths answer 404 with the global not-found page (lang, language links, non-affiliation line, CSP); skip link; landmarks; docs pager and section marking; the mobile drawer (modal, focus trap, Escape, focus return, navigation, language and theme) |
| `preferences.spec.ts` | Theme switch and persistence; OS following; Settings (each choice, its announcement, persistence, Docs details, reset); "Match system"; a language chosen in Settings confirmed in the new language; the locale switcher and the root page's redirect; no cookies at all |
| `onboarding.spec.ts` | Offered but never opened by itself; Skip, Escape and backdrop close it and are remembered; all six steps with Back and Finish; strict focus trap and inert page; reopening from Docs; translated in every locale |
| `motion.spec.ts` | OS reduced motion and the in-app setting remove movement and keep a short fade; "Match system" follows the OS live; the drawer; reduced transparency makes the glass solid (Chromium, through the DevTools protocol) |
| `a11y.spec.ts` | axe on every page in both themes and all locales, the overlays, the 404 page in both themes, and phone layouts |
| `api.spec.ts` | Over the mocked `/api/v1` (`e2e/api-mock.ts`, the contract fixtures; the live section in the contract's shape, validated with the client's zod mirror): New deliberation -> run -> frames -> decision -> receipt link (with the `Idempotency-Key` and the `expected` block checked); a replayed finished run never reads LIVE and is named Playback; no LIVE word on a replayed, flagged or listed live-model run in the four locales, and LIVE for a live-model run in progress; a `provenance_inconsistent` run never reads LIVE even on live-delivered frames; pacing only for scripted runs, and off with the in-app Reduced setting; the header's labels; the live section's disabled state, the closed form (no site key, tripped or unreadable ledger) and the open form's request (trimmed invite code, a live refusal's sentence); an error's sentence and Retry-After; a failed detail request with its retry; a failed stream with the stored records and a reconnect; a failed run explained from its code; the scope line of a revision; no verbatim-log link for a live-declared run; My runs with and without a session; the Explorer (filter, translated statuses, SIMULATED proofs, =0 count); the run page without an id; the runtime NotConnected state against the static server; the Overview's button (and a server on another contract); the four locales; axe on `/new`, `/runs` and the three tabs of `/run` in every locale and theme, and on the open live form, a failed run and a failed detail; lane W2: the Evidence tab (simulated and sealed-snapshot coverage, the English original note), the Proof inspector (simulated and real proofs, a failed receipt, a receipt with other limitations), the Audit trail (chain, receipt, log, check command; a live-declared run), a guided run's log checked in the browser (pass, then a changed record failing at its sequence), the Meta API panel (scripted doubles, recorded Meta calls and totals), the explanations (plain, technical, before protocol 1.3, three locales), the revision view, and axe on all seven tabs |
| `showcase.spec.ts` | Over the mocked `/showcase` and `/research` (entries from the archive's manifest and logs, the server's F03 entry): the list (RECORDED, by battery, outcomes next to their evidence, the manifest checked, the archive's note), a recorded run in RECORDED mode (no stream, read-only Decision, archive receipt and log, recorded proofs, the check command), its archived log checked in the browser (pass against the pinned digest and the receipt's head; a flipped byte failing at its record), its receipt said to be the server's rebuild with the run record's own limitations quoted on request (and an unreadable record said), a run blocked by proof-generation failures, a schema-1.0 run (no run record), a run the listing does not name, a failed listing, a forged manifest, no LIVE in four locales, the Overview's figures recounted from the same entries, and axe on the figures, the list and every recorded tab in four locales and both themes; a recorded live-model run with no flag at all (no delivery badge, never LIVE, four locales); the Overview's figures read as visible text against numbers counted from two different archives; hostile markup, markdown links and an unclosed bidi override in record text, model output, the manifest's note, the receipt's and the run record's strings, titles and labels rendered inert in every recorded view (with a control showing the check sees each); axe on a failing and a partial chain check, a schema-1.0 run, a forged manifest (list and run), a failed listing, a failed recorded detail and a run the listing does not name (fr, both themes), and on a phone (390x844); the recorded tabs (arrows, Home, End) and the chain check driven by the keyboard |

The page fixture (`e2e/fixtures.ts`) fails a test on any console error, page
error, CSP violation, request to another origin or same-origin response of
400 or more, with one exception added with the API lane: the static server's
own HTML 404 for an `/api/v1/…` path, which is the "API unreachable"
condition the views must handle, not a broken link. Mocked API errors (JSON
bodies) still count, and `api.spec.ts` takes the ones it expects out of the
list, asserting they happened.

Projects: `chromium` (everything except `@mobile`), `chromium-mobile`
(Pixel 7, `@mobile`), and `firefox` and `webkit` (the `@smoke` tests). Results
go to `test-results/` (git-ignored); `test-results/axe-summary.json` adds up
the axe scans.

## Known gaps

- The 404 page is English only (with the four language links); a 404 cannot
  know the visitor's locale before any script runs.
- `CONTACT_EMAIL` and `HOSTING_COUNTRY` in `src/lib/site.ts` are marked
  placeholders until the owner provides them; the texts render them as such.
- Outside this directory, and not changed from here: the territory list of
  the Terms (owner decisions of 2026-10-27) has to go into release gate G10
  of the plan. (The root `README.md`, rewritten on 2026-10-29, describes
  protocol 1.3 and its stop rule and no longer says "fixes in progress".)
- No `hreflang` alternates yet: they need the public hostname (Day 2).
- The SIL OFL 1.1 texts of the self-hosted fonts ship with the export as
  `/licenses/Inter-OFL.txt` and `/licenses/JetBrainsMono-OFL.txt`
  (`public/licenses/`), but no page links to them yet: a link under the
  About page's License section needs a new key in the four legal catalogs
  and the owner's sign-off (2026-10-29).
- The texts written for the unconnected build (the tour steps, the
  provenance legend's intro, the Settings description of the technical
  level, the Overview's build note) still speak in the future tense; the
  build note now shows only when the API is unreachable, the rest awaits a
  rewrite in the four languages once the connected portal is reviewed.
- The live form with a Turnstile site key (Start waiting for the token, the
  token spent and the widget reset on every submit, the `live-run` action)
  is covered by unit tests (`tests/live-form.test.ts`,
  `tests/turnstile.test.ts`) only: the e2e export is built without a key,
  so the suite sees the closed form, and the open form only through a
  mocked server that requires no token.
- The live attestation wording (age and territory) and the Limitations
  text are legal drafts awaiting the owner's sign-off; the Limitations page
  now says an archived live-model run is labelled next to META MODEL, as the
  views read it (2026-10-28, pending that sign-off).
- Lane W2: the public surface publishes no output bound, attempt count or
  schema and prompt digests of a model call (operator only), so the Meta API
  panel says they are not published; the contract publishes no prices, so no
  cost is shown; showcase entries carry no dates, so the Overview's figures
  name runs, batteries, schema versions and policy rules but no dates; and
  the in-browser chain check reads a line with invalid UTF-8 as a failure
  where the runtime's lossy decoding would fail it at the hash instead.
- Script weight: `/en/` loads 13 async scripts, 797,215 bytes raw (230,921
  gzip at level 9), plus a 112,594-byte `noModule` polyfill that modern
  browsers skip. The whole export is 460 files, 7,132,989 bytes (each page
  also exports its RSC payloads). Not optimized yet.
- Browser coverage: Chromium (full suite), Firefox and WebKit builds of
  Playwright 1.64.1 (smoke tests). No real Safari or older Firefox, and no
  screen-reader pass with NVDA, JAWS or VoiceOver yet.
- The legal and trust texts are drafts awaiting the owner's review, and the
  About page shows no repository link until `SOURCE_REPOSITORY_URL` is set
  in `src/lib/site.ts`.
