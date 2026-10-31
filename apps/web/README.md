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
| New deliberation | `new/` | Guided examples from `GET /scenarios` (the count the server lists, title, description, expected outcome as a neutral chip, the SCRIPTED model badge (provenance.ts `modelLabel`, as on run pages) and the SIMULATED evidence badge, a Run button: `ensureSession`, `POST /runs` with a fresh `Idempotency-Key`, then the run page) and the live presets from `capabilities.live` (`LiveCapabilitiesV1`: each preset with the badges of its own recorded model and evidence axes, the public budget in US dollars, the 18+ and territory attestation linking to the Terms, an optional invite code under a disclosure, the Turnstile widget, `POST /live-runs`; see "Live form" below), or the fixed "Live mode is not enabled on this server" state linking to the showcase; every API error shows its code's sentence and the Retry-After wait |
| My runs | `runs/` | The session's runs from `GET /runs` (title, state, outcome with its evidence axis, the failure sentence of a failed run, provenance badges, never a LIVE label: a live-model run reads META MODEL with its recorded date, created and updated, "access ends" and "deleted after" from the capabilities limits), the empty state, no list and no count without a session |
| Run workspace | `run/?id=<run_id>` | Client-rendered from the query string: header (title, state, outcome with evidence axis and, for a revision, which proposal it is for, records, revision round, run id, the failure sentence of a failed run, provenance axes with the latest frame's delivery, flags, and "Go to your decision" on every tab while the run awaits a decision that can be recorded here), the error of a failed detail request with a retry, tabs Live (only while the run deliberates and its latest frame is LIVE; Playback otherwise), Decision, Explorer, Evidence, Proof inspector, Audit trail and Meta API (`src/lib/run/decision.ts`: Decision right after the first tab, first for a recorded run); the Decision tab opens with the decision card while a decision can be recorded or once one is (two full-size buttons of the same variant; on a blocked or abstained run "Approve as an override" and a note marked "Required for this run" before any click); `run/?id=<run_id>&source=showcase` opens a recorded run of the archive in RECORDED mode; see "Run workspace" below |
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
  gone from `src/lib/site.ts`. Copy whose truth depends on the connection
  (the Overview legend's intro, the tour's first steps, the Docs build note,
  the Modes article's "On this portal", the Settings "Technical"
  description) has connected and not-connected variants chosen from the same
  probe (`<ConnectionText>`, `useConnectionState()` in
  `src/components/ConnectionText.tsx`; the tour's choice is `tourBodyKey` in
  `src/lib/tour.ts`), and until the probe settles a pending variant that
  makes no claim either way; a server that did not answer is
  worded as such, never as "not connected in this build".
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
- **Decision** (`DecisionTab`, the tab right after the stage; first for a
  recorded run): while a decision can be recorded here, and once one is,
  the decision card comes first in the tab, before the long content
  (`src/lib/run/decision.ts`). It holds the recommendation line with its
  evidence axis (while awaiting), the fixed public_demo no-authority notice,
  then the public decision form, enabled only while the run awaits a
  decision and belongs to this session: a note and two full-size buttons
  that share one variant (`DECIDE_BUTTON_VARIANT`), so Approve is never
  promoted over Reject. On a blocked or abstained run Approve reads "Approve
  as an override", the note is labelled "Required for this run" and the
  override notice says, before any click, that approving needs the note (the
  server's rule: a rejection needs none); elsewhere the note is optional.
  Once decided, the card shows the recorded decision with the server's
  notice (the workspace holds the decision this session sent, so the
  header's "Go to your decision" goes away even if the detail cannot be read
  again). Otherwise the card stays after the objections. Then the
  recommendation kind and, right under it, the scope line when the
  recommendation is for a revision (which proposal it is for, which one it
  replaced, the task, both summaries side by side: K03/K04), rationale,
  protocol, unresolved assumptions and uncertainties, the open objections.
  The card also links the receipt, and the verbatim log only when the
  public surface publishes it (`ndjsonPublishable`, the server's rule: a
  live-declared run answers `404 ndjson_unavailable`, said in a sentence
  instead); cancel for a run still deliberating.
- **Evidence** (`EvidenceTab`, lane W2): every record the run retrieved
  (`src/lib/run/evidence.ts`, from `mergeEvents`): kind, project, retrieval
  mode, snapshot id and capture time when present, what its proof covers
  (from the proof's own request, never the claim's query: "Proof covers:
  complete record list of <project> at anchor <prefix>." for the sealed
  snapshot, the sealed request for a live recall, and, when that request's
  query is word for word the statement of one of the run's claims (the
  runtime recalls with the claim text, so in the archive it always is,
  compared from the run's proposals, never assumed), that the proof covers
  what recalling that text returned, not the claim; a sentence for a pre-1.3
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
  every record's SHA-256 recomputed with WebCrypto from the raw line with its
  top-level hash blanked textually, where `event_log.rs` hashes its
  `serde_json` re-serialization: the two agree on the canonical lines the
  runtime writes (the archive's and the server's `events.ndjson`), and on
  any other line the two recipes give different hashes, so the two checks
  can disagree in either direction (neither is a superset of the other);
  it reads only the chain fields
  (schema version, sequence, ids, link, hash), checks no content of a
  schema-1.0 record, and neither replays the history nor re-derives the
  recommendation, which the panel says and leaves to `consensus-api verify`;
  a UTF-8 byte order mark is kept and fails the line, as `serde_json` fails
  it; every link followed; the run's segment compared with the
  pinned digest, the receipt's head and count and the detail's chain; pass,
  or the first failing record, or, for a run whose records carry no hash
  (schema 1.0), a neutral "no chain to check" with the chain "not checked"
  and the head "not compared", introduced without any claim of hashing; a
  run that began before schema 1.1 (hashless first records, chained after)
  reads as a neutral "checked in part" naming its unverified records: the
  green "Chain checks out" only for a run whose every record was verified
  link by link, and its record count only over the records that carry a
  hash). The receipt the check compares is the one it reads itself when it
  starts, just before downloading the log, and the panel says so (if that
  read fails, the receipt already shown stays shown and the check says it
  compared no receipt); the workspace also reads the receipt again whenever
  the detail moves on (a decision, the end of the stream). The log is
  append-only, so a log that grew in between reads the receipt or the
  detail's chain as "matches an earlier state" (neutral, never a
  difference, and the headline then says the segment "matches or extends"
  what the views hold) only when the file verified link by link and the
  record at that copy's own last position hashes to its head; any other
  mismatch fails. The button stays focusable while the
  check runs (busy, `aria-disabled`, never natively disabled), and one
  always-mounted, visually hidden status region announces the progress and
  then the short verdict once (the manifest check on `/showcase` does the
  same with its outcome); the detailed results are not a live region.
  The guided Run, live Start, Approve and Reject buttons follow the same
  rule while their request is in flight (busy, never natively disabled); a
  decision that replaces the form moves the focus to the recorded decision,
  and a live request sent with Enter from a field moves it to Start. A
  refused live request spends the Turnstile token, so when the form comes
  back waiting for a fresh one (Start natively disabled again) the error
  notice takes the focus (`focusAfterRefusal` in `src/lib/live/form.ts`). A run
  whose detail's chain scope is `partly_chained` gets its own intro
  (`Run.chain.introPartlyChained`): its first records carry no hash, so
  they are not chain-verified or tamper-evident; every record of the file
  that carries a hash (this run's later records among them) is recomputed.
  Every redacted view (the links, the receipt
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
  A listing (`GET /showcase`) that cannot be read is shown in the header
  with its retry and with what it leaves out (the log, its digest, the CLI
  command, the chain check); the Audit and Decision tabs say so in place of
  them, and keep the loading line for a listing still being read. A manifest
  that cannot be read, parse or match says so in the header (with a retry
  of the manifest alone when its download failed), never dropping its note
  silently. Each log's listed SHA-256 is cross-checked against the checked
  manifest's own pin (`logPin` in `src/lib/showcase.ts`, `LogPinNote`): the
  showcase list and the Audit trail say whether it matches, differs (both
  digests named), is not in the manifest, is still being checked (a loading
  line), or could not be compared, and the in-browser chain check compares
  the file with the manifest's pin when there is one, saying when it fell
  back to the listed digest (and, once finished, keeping the wording of the
  digest it actually used). The digest labelled pinned by the manifest is
  the checked manifest's own (`pinDisplay`): when it differs, the listed one
  is shown under its own label, and a file the checked manifest does not
  name is labelled as the server lists it. On a blocked run the
  generation-failed sentence points to the header's manifest notice rather
  than repeating it.

Honesty rules the views follow (release gate G7; `src/lib/provenance.ts`,
each with a unit test): LIVE only when a frame's delivery is `live`, the
model axis is `live_meta` and no flag is raised (`provenance_inconsistent`,
`unchained` and `mode_undeclared` are shown and never render as LIVE; a
live-delivered record of a scripted run reads STREAMING); every label with a
LIVE word (the LIVE delivery badge, the LIVE META model badge, "Meta model ·
live", the Live tab) goes through `isLive`, and no evidence or source label
carries the LIVE word (the evidence of a live Hyphae recall reads HYPHAE
RECALL), so a replayed frame, a list entry, a preset, a recorded run or a
flagged run reads META MODEL (always in the info colour; a blocking flag is
its own warning badge) with "Meta model · recorded <date>" next to REPLAY, in every locale
(`tests/provenance.test.ts` checks each locale's LIVE word); a scripted
provider reads "scripted double, no model call"; simulated evidence is named
next to every outcome, as a second tag ("supported" + "Evidence: SIMULATED",
`OutcomeTags`: both tags wrap and are never truncated); a simulated
proof is the neutral SIMULATED token with its scripted outcome as text,
never a green "valid"; every count comes from the run data; nothing
pretends to load. Colours (round 4, HE #1, #2, #16): only a real verifier's
accepted proof (and the in-browser chain check) is green; a "supported"
outcome and a "corroborated" assessment are the info role (`INFO_VARIANT`),
never green, on any evidence; META MODEL is the info colour in every state;
LIVE and LIVE META share `LIVE_VARIANT`, used by nothing else. Round 5 has
no hue: "green" is now the verified encoding (a solid mid grey under an
inverted label, no frame), given only to a real verifier's accepted proof;
the in-browser chain check and the showcase manifest's digest keep the
success variant in the view but are drawn "checked" (hollow: the surface
and a 1px border/strong frame), because integrity is not truth. The same
rules hold on the monochrome encodings (globals.css "Status without hue";
checked on the rendered tags by `e2e/api.spec.ts` and
`e2e/showcase.spec.ts`). Every counter
of simulated proofs says so on the same line ("4 simulated proofs (not
checked)", "SIMULATED, scripted “valid”: 4 proofs (not checked)", the plain
explanation's "all SIMULATED … (scripted, not checked)"), and a server text or
JSON that says "valid" for a simulated proof gets a caption. In a mixed run
(some proofs scripted, some checked by a real verifier) the lane and the
receipt split the counts; the plain explanation's proofs line says "some of
them SIMULATED … (the simulated ones are scripted, not checked)" with no
number the trace does not carry, the explanation adds "Some proof statuses of
this run are SIMULATED", and the rationale and technical-count captions say
the server's counts include scripted statuses that were not checked. The
invalid_proof lead and reason say SIMULATED when every invalid status the
check names is the scripted verifier's.

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
  renders it); since round 5 it is the dashed SIMULATED tag, never the
  verified solid. Request ids
  (`meta_request_ids`, `ModelInvocationV1.request_id`) are evidence of a
  recorded Meta call, never of liveness.
- **Errors and failures**: `<ApiErrorNotice error context onRetry remaining>` (every
  `ErrorCode` has a sentence; `error.message` is never shown; the headline
  says the consequence first, by the code's class and the `context`:
  `create` for a run's creation, `decision`, or `read` for anything else
  (a cancel included), see
  `errorHeadlineKey` in `src/lib/api/errors.ts`; under a decision only a
  contract error body reads "not recorded", and no answer, a timeout or an
  unreadable answer reads "may or may not have been recorded" and re-reads
  the run; a run-creation sentence
  such as "admits no new run" is only for `create`; a 429 with a wait on a
  creation keeps its buttons disabled with a countdown, `src/lib/api/backoff.ts`,
  which the notice follows through `remaining` and says once, and links to a
  recorded run; a `rate_limited` wait holds for the whole page, the guided
  Run buttons and live Start alike); a failure
  code through `FailureSentence` / `useFailureText` / `useUnavailableText`
  (`src/components/run/FailureSentence.tsx`). A settled failure is never a
  loading line.
- **States the reader must not miss** (round 4, heuristic evaluation
  problems 5 and 7) are a full-width `<Callout tone>` (`src/components/Callout.tsx`:
  icon, title in words, body; failed, contested, success or info), never one
  more equal tag: a chain check that failed in this browser travels to the
  run header and the decision (`ChainBroken`, a page state kept by the
  workspace until a later check does not fail, its reason the verifier's own
  `Run.chain.problems` sentence); PROVENANCE INCONSISTENT keeps
  its tag and gets its note in a callout; labels the records contradict and a
  receipt that disagrees with the run come from `src/lib/run/consistency.ts`
  (the page's own check: it never adds, removes or changes a label); a
  manifest that does not match is a failed banner, one that matches but does
  not parse a contested banner (the success tag only for a confirmed
  manifest), and every showcase card
  then reads "Archive not confirmed". A recorded run never reads "awaiting
  your decision" (its receipt's state is the state when recorded).
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
  `src/theme/hyphae-mission.css` (`astryx-theme`) →
  `src/app/hc-mono-tokens.css`, `src/app/hc-mono-glass.css` and
  `src/app/globals.css` (`app`, in that order). App rules
  stay inside `@layer app`, because unlayered rules would beat every Astryx
  layer.
- **Monochrome** (owner direction 2026-10-31,
  `design/palette-mono.md` revision 2): every colour is a true grey (R = G =
  B) from one 14-step scale (`mono/0` `#FFFFFF` to `mono/13` `#0B0B0B`), in
  both themes; no hue anywhere. Token names are unchanged; their values
  follow the mapping of palette-mono.md section 5. `hc-mono-tokens.css` is
  the palette's generated token layer (the `--hc-*` functional, role, body,
  glass and button tokens as `light-dark()` tuples), copied from
  `design/palette-mono/` with one change (`color-scheme` is not forced on
  `:root`). The accent is ink: links are always underlined (the only link
  cue without hue; buttons drawn as `<a>` and navigation are not), the
  focus ring is ink, and `accent-color`, `caret-color`, `scrollbar-color`,
  `::selection` and autofill take grey tokens. The two blue glows became the
  grey body gradient (`body::before`), and the glass and its backdrop are
  greyed. Status tags are told apart by fill weight, label polarity, frame
  style, an edge mark or marker and an icon, not by colour (globals.css
  "Status without hue": verified solid mid grey with no frame, failed solid
  ink in a double frame, simulated dashed with a 4px pixel hatch strip
  (`public/brand/hatch-{light,dark}.png`), LIVE a 2px ink frame with an 8px
  square that pulses three times and stops (static under reduced motion),
  contested double, RECORDED its own mid tint, pending your decision a 2px
  frame on the strong tint with a 3px ink bar, info a tint and a hairline,
  checked hollow). Every tag goes through `src/components/Tag.tsx`: the
  variant a view picks from `src/lib/provenance.ts` is drawn as a role
  (`VARIANT_ROLES`; a view may name only a role `VARIANT_ALLOWED_ROLES`
  allows: success as "checked", neutral as "recorded", blue as "pending"),
  set as `data-tag-role` for the role hooks, with an icon of that role's
  family (`ROLE_ICONS`; no outline in two families, the shield-check the
  verified role's alone, none for LIVE). The icons are Lucide 1.61 shapes
  redrawn as inline SVG in `src/components/icons.tsx` (ISC, see
  THIRD_PARTY_NOTICES.md; no icon dependency), aria-hidden, 12 px in tags.
  Tags are rectangles of radius 2, never pills. Callouts likewise
  (palette-mono 2.3): failed a 2px ink frame with an inverted title band
  that holds its icon (unlink for a broken chain, file-x for a manifest
  that differs), contested a 4px double bar, success and info a 3px solid
  bar, each on its tint. `tests/tags.test.ts` covers the mapping, the icon
  families, the rendered Tag and Callout and the role hooks;
  `e2e/tag-encoding.spec.ts` (with `helpers.ts` `ROLE_ENCODING`) every
  role's rendered border style, fill, hatch, bar and icon in both themes.
  `tests/no-hue.test.ts` fails on any colour literal
  with a hue in the theme output or the app's stylesheets, and on any colour
  Astryx's base stylesheet sets with a hue that the theme does not
  redeclare; `e2e/no-hue.spec.ts` checks the built CSS and every colour the
  browser renders, in both themes.
- **Shapes (round 5, square first, design-direction 3.7)**: every corner
  is 0, 2, 4 or 6 px, tags included (the tags lane draws the Astryx Badge
  at radius 2, so the drawn-corner exceptions of `tests/radius-scan.ts`
  hold no tag). The theme control is a 3-segment System / Light / Dark
  radio group (`ThemeControl.tsx`: outer radius 4, inner joins 0, the
  selected segment in the ink material with a check; icons only in the top
  bar, icons and words in the drawer), the Overview step markers are 24 px
  squares at radius 4, the tour's progress is a segmented bar (24x4
  segments, radius 0), radio marks are square-cornered (radius 2, still
  native radios: globals.css switches off the invisible input's native
  drawing, which Chromium keeps round whatever its radius), and navigation
  rows are square. `tests/radius-scan.test.ts` evaluates every radius
  declaration of the stylesheets the build bundles (every sheet
  `styles.ts` imports, `hc-mono-glass.css` included, and `out/` when a
  build exists) against the theme's tokens: the app's own sheets allow no
  exception, and Astryx's sheet is off the scale only in the classes of the
  allow-list in `tests/radius-scan.ts`, each with the component that uses
  it and why it may ship (no entry may be stale). `e2e/shapes.spec.ts`
  checks the built CSS on every run and the corners the browser computes
  for every element, tags included.
- **Left for the later round 5 lanes** (known deviations of this
  foundation from palette-mono and design-direction):
  - Resolved since the foundation: RECORDED has its own role hook, so the
    other neutral labels take the informational tint, and every tag,
    including `[data-recommendation-kind] .astryx-badge`, is 2 px (tags
    lane); the end corners of `.hc-nav-link` are 0 (shapes lane); every
    tile `hc-mono-tokens.css` names is shipped (the brushed grain tiles
    with the glass, below; the hatch tiles, for `--hc-hatch` on the
    simulated tag, with the tags lane).
  - Tags that palette-mono gives an icon outside the shipped families keep
    their role's default icon: In progress (`loader`), Draft
    (`pencil-ruler`) and the chain row cells (`check`, `x`, `minus`,
    `rotate-ccw`). (The revision kinds follow design-direction 3.2 through
    `REVISION_TAGS` in `src/lib/provenance.ts`: unchanged, changed and
    added are informational, with `equal`, `pencil` and `plus`; only a
    dropped claim stays contested, with `circle-minus`.)
- **Brushed glass** (round 5, lane glass; palette-mono.md section 3):
  `src/app/hc-mono-glass.css` is the material part of
  `design/palette-mono/hc-mono-glass.css` (live glass, static glass, the
  clickable card, the dialog scrim, every fallback and the ink button), in
  `@layer app`, loaded after the tokens and before `globals.css`, so the
  component rules there (position, radius, the decision card's 2px border,
  the Settings 3:1 borders) win and set no fill, backdrop or shadow on an
  element that carries the glass. The design file's other parts are not
  copied: its browser defaults and body backdrop were already in
  `globals.css`, and its status tags, rows and callouts would collide with
  the app's `.hc-tag` and `.hc-callout` (tags lane). The grain tiles
  (`public/brand/brushed-{light,dark}.png`, 128 × 128 grey + alpha, and
  `@2x`, 256 × 256, byte-identical to `design/palette-mono/brand/`) are
  same-origin, so `img-src 'self'` covers them and the CSP is unchanged;
  `--hc-glass-grain` switches per theme, `--hc-btn-grain` is the light tile
  in both.
  - **Live** (`.hc-glass`, a 16 or 24 px greyed blur, sheen, grain, masked
    edge ring): the top bar (`--region`, level 1), the tour and the drawer
    (level 3; native modal dialogs in the top layer, so the drawer is not
    caught by the top bar's backdrop root; the drawer at the level 1 blur
    under 672 px), and the decision card (`hc-glass--2 hc-glass--float`):
    live level 2 under 672 px, static level 2 from 672 px, the one floating
    card of palette-mono 3.7.
  - **Static** (`.hc-glass--static`: opaque stops, grain, a border-box edge,
    no backdrop, no pseudo-element): the sidebar, the run header, the run and
    showcase cards and the docs cards (both `--interactive`: top highlight;
    hover a flat `bg/hover` with the edge at `border/strong`, and no border
    colour of their own in `globals.css` that would hide it), the scenario
    cards, the Overview steps, the docs tour panel
    and the five Settings panels. The live ring's `::before` is scoped to
    non-static glass, so the Overview step number (the step's own
    `::before`) is untouched.
  - **Solid**: tables (`.hc-timeline` on the surface colour, also inside a
    panel), tags, inputs, code and JSON, long text (docs, legal pages, the
    Decision tab's recommendation and explanation panels, which stay plain
    `.hc-panel`), message panels (errors, not-connected, empty states),
    stage and proof cards, and any panel nested in a card.
  - **Primary button** (`.astryx-button[data-variant="primary"]`): the ink
    gradient `btn/top` → `btn/bottom` under the button grain, a 1px
    highlight and lowlight; hover the hover gradient; pressed (`:active`)
    flat `btn/hover-bottom` with no shadow; forced colours the system
    button.
  - **Fallbacks**: no `backdrop-filter` (opaque stops), reduced transparency
    (the OS setting, or a `.hc-transparency-reduced` root class that no
    Settings control sets yet: opaque stops and colour, no blur, opaque
    scrim), more contrast (flat popover surface, strong ring), forced
    colours (Canvas, CanvasText border), print (surface, strong border, no
    body gradient). The glass's forced-colours and print border is the
    element's real boundary, so `globals.css` clears a glass element's
    border only `@media screen and (forced-colors: none)` (the dialogs, the
    static panels). In forced colours the tour has a full 1px frame, the
    drawer its inline-end edge, and the top bar only its bottom line (a
    decision: a frame round the full-width sticky bar would read as a box).
  - **Tertiary ink**: on live glass and static glass/3, `text/tertiary`
    fails, so both `--hc-text-tertiary` and the name Astryx actually reads,
    `--color-text-disabled`, are remapped to the secondary ink there.
  - Tests: `tests/glass.test.ts` (the sheet's rules and every fallback, glass
    never on a forbidden selector or element, live glass only on the four
    allowed elements, the tiles' sizes and alpha, the button, and
    palette-mono's worst-case contrast recomputed on the shipped tokens and
    tiles) and `e2e/glass.spec.ts`. palette-mono's own checker,
    `palette_mono_check.py`, passes every contrast pair and glass row on the
    shipped tokens (byte-equal values) and tiles; its only failures are the
    six tag lints (forced-colour inversion, the LIVE pulse, the hatch, the
    pending bar, tags in a selected row), which belong to the tags lane.
- **Theme**: `src/theme/hyphaeMissionTheme.ts` extends the neutral theme with
  `defineTheme`. `pnpm theme:build` writes the committed
  `hyphae-mission.{css,js,d.ts,css.d.ts,variants.d.ts}`; rebuild after any
  Astryx upgrade. Tokens: accent ink (`#0B0B0B` light, `#F4F4F4` dark; text
  and icons `#0B0B0B` / `#FFFFFF`); text on accent white in light mode and
  `#0B0B0B` in dark mode; surfaces white in light mode (body and muted
  `#F4F4F4`) and near-black in dark mode (body and muted `#0B0B0B`, surface
  `#131313`, card `#1B1B1B`, popover `#242424`); the status colours, the
  nine hue families, the syntax and data-visualisation palettes and the
  inset status shadows re-pointed at greys; `--hc-color-proof` is now the
  verified solid (`#6E6E6E` / `#9E9E9E`, the cyan is gone); square-first
  radii (design-direction 3.7): 2 px (inner), 4 px (element: buttons,
  inputs) and 6 px (container: cards, panels, dialogs).
  `tests/contrast.test.ts` reads the compiled theme and requires 4.5:1 for
  every text token on every surface token, in both modes, and 3:1 (WCAG
  1.4.11) for the local token `--hc-color-border-strong` (`#6E6E6E` /
  `#9E9E9E`, palette-mono `border/strong`), the boundary of controls and of
  panels that bound one: radio rings, checkbox boxes, the off Switch track,
  native selects, the Settings panels and the dashed not-connected panel
  (`globals.css` points Astryx's
  `--color-border-emphasized`, and the Switch's `--color-background-gray`,
  at it on those controls only). Fonts are set
  through the typography roles, not plain tokens: neutral's
  touch-and-narrow adaptation re-derives the font tokens from those roles.
- **Fonts**: Inter (variable, with optical size) and JetBrains Mono through
  `next/font/google`, downloaded at build time and served from
  `/_next/static/media`. theme-neutral's `fonts.css` is never imported (it
  loads Figtree from Google at runtime). `.hc-tabular-nums` and `.hc-code`
  are the utilities for metrics and for hashes and ids.
- **Components used**: `Button`, `RadioList`, `Badge`, `Heading`,
  `Text`, `CheckboxInput`, `TextInput` and `TextArea` (the header's
  `Switch` became the app's own theme control in round 5). Long-form content
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
every `light-dark()` into them). The header's theme control (System / Light
/ Dark, `src/components/ThemeControl.tsx`) shows "System" in the static HTML
and takes the stored choice on hydration.

Reduced motion comes from the OS (`prefers-reduced-motion`) or from Settings
(`data-motion="reduce"` on `<html>`), and is tiered: the drawer's slide and
the dialog's small shift go, short opacity fades stay, everything else is cut
to 0.01 ms (so `transitionend` still fires), and Motion gets
`reducedMotion="always"`. Live glass (a backdrop filter) is limited to the
top bar, the open dialog (tour or drawer) and, on narrow viewports, the
decision card ("Brushed glass" under Astryx). Under
`prefers-reduced-transparency: reduce` it takes the opaque stops and an
opaque background colour, with no blur and an opaque scrim; without
`backdrop-filter` support the opaque stops; with `prefers-contrast: more` a
flat surface with a strong ring; in forced-colors mode system colours and a
real border; in print the surface with a strong border.

## Logo (Quorum)

The Quorum mark (design/logo/quorum-final, v1.0; its README is the rule
book) follows one threshold: below 64 px the mark is always flat. The top
bar (56 px) carries the hand-set 32 px pixel cut before the wordmark, inline
in `currentColor` (`QuorumMark`, `src/components/Quorum.tsx`), so it takes
the wordmark's ink in each theme with no style attribute; it is decorative
and the home link is named by the wordmark. The brushed-glass mark appears
only at 64 px or more, as images of the small glass files (64 to 192 px):
96 px on the root page's language chooser and on the 404 page
(`QuorumGlass`). Both surfaces are in the page and `globals.css` shows the
one for `<html data-theme>` (the OS setting when the head script has not run;
light in print), so their gradient and filter ids never meet another mark's.

`src/lib/brand.ts` holds the paths and the metadata the three documents (the
`[locale]` layout, the root layout, the 404 page) share: the favicon
(`favicon.svg`, the 16 px cut, plus the 16 and 32 px PNGs), the 180 px
apple-touch icon, and `theme-color` in greys (the body background, #F4F4F4
light, #0B0B0B dark). `favicon.svg` is the one shipped SVG with a `<style>`
element (its dark-browser ink): it is linked, never inlined, and the CSP
applies to HTML responses only. The app icons (`icon-512.png`,
`icon-1024.png`, dark tile, and their `-light` versions) are served from the
export root; there is no web app manifest (the app has none, and the API
server's CSP sets no `manifest-src`). `SocialMeta` writes the Open Graph and
Twitter card tags itself, with `og:image` pointing at `/og-image.png`
(1200 x 630, greys, the glass mark and the wordmark on the dark stage):
Next's `openGraph` metadata would resolve it against
`http://localhost:3000` without a `metadataBase`. The public origin is known
only when the build is given one: put `NEXT_PUBLIC_SITE_ORIGIN=https://<hostname>`
in `apps/web/.env.production.local` (ignored by git; `in-docker.sh` passes no
variables into the container, Next reads the file) before `pnpm build`, and
`og:image` becomes `https://<hostname>/og-image.png`, the absolute address
the Open Graph protocol and the link-preview crawlers (X, LinkedIn, Slack)
need. Without it `og:image` stays the same-origin path, which many crawlers
do not resolve. The value must be a bare https origin (no path, query or
credentials; `parseSiteOrigin` in `src/lib/brand.ts`), or the build fails.
There is no `og:url`: the tags come from layouts shared by every page under
them, which cannot know the page's own address.

Known limits, accepted:

- `theme-color` follows the OS colour scheme (two media-gated metas), not
  the app's own theme toggle: with the app set to dark on a light OS, the
  browser chrome stays #F4F4F4 above the #0B0B0B page (and the reverse).
  Following the toggle would need a script rewriting the meta at runtime,
  which the hash-only CSP makes costly for a cosmetic gain.
- The top-bar mark's ink is the theme's text colour (`--color-text-primary`,
  #131313 light, #F4F4F4 dark), so mark and wordmark are one ink, not the
  Quorum README's #141414 / #EDEDED for the 32 px cut. The token file is
  generated and has no logo-ink token, and `globals.css` names no colours
  of its own; the exact ink needs a logo-ink token in the generated token
  source. The favicon PNGs and `favicon.svg` keep #141414 / #EDEDED.
- The lockup stays on one line (`white-space: nowrap`); at 360 px and below
  the top bar's edge padding and gap drop to `--spacing-2` so the mark, the
  wordmark and the Menu button fit at 320 px, the WCAG 1.4.10 reflow width.

Every file in `public/` other than `licenses/` is built by
`python -I scripts/web_brand_assets.py <design/logo/quorum-final>` (repository
root; standard library only). `tests/brand-assets.test.ts` checks the sizes,
that every PNG pixel is grey, and that the SVGs carry no style attribute,
script or external reference; `e2e/brand.spec.ts` checks the top bar, the
glass threshold, the served files, the links in every document, and axe.

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
  2 themes): **0 violations of any impact**. The gate is zero serious or
  critical violations (`blocking` in `e2e/axe-results.ts`, asserted by every
  spec that scans); the scan count and the other totals are only printed,
  and no code compares the scan count with 300. How to reproduce them: run the full
  suite (`sh apps/web/scripts/playwright-docker.sh --reporter=line` after
  `pnpm build`); `e2e/global-teardown.ts` adds up `test-results/axe/*.json`
  into `test-results/axe-summary.json` and prints the line
  `axe: <scans> scans, <rule passes> rule passes, <element checks> element
  checks passed, …`. For the same tree, the scan count and the violations
  reproduce exactly; the rule-pass and element-check counts do not: axe
  counts only what is in the page when a scan runs, and a rule whose result
  is incomplete counts as no pass, so they move a little from run to run.
  Three full runs of the tree as of 2026-10-28, over the same pages and
  specs, gave 13,250 / 141,919 (the 2026-10-28 run quoted here), 13,244 /
  141,811 and 13,232 / 141,601 rule passes / element checks, all with 300
  scans and no violation. Any change to the pages changes these totals: the
  W2 review's round-2 i18n lane (2026-10-29) adds elements, and its two full
  runs gave 13,232 / 142,283 and 13,242 / 142,495, each with 300 scans and
  no violation. `color-contrast` stays "incomplete" on every scan:
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
`style-src-attr 'unsafe-inline'`. Since round 5 the theme control is built
from native radios with static classes: no page has a `style` attribute
except Settings (7, from Astryx's radio lists), which still needs it. The 404 page (`/404.html`, also exported as
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
| `preferences.spec.ts` | The header theme control (System / Light / Dark) and persistence; OS following; Settings (each choice, its announcement, persistence, Docs details, reset); "Match system"; a language chosen in Settings confirmed in the new language; the locale switcher and the root page's redirect; no cookies at all |
| `onboarding.spec.ts` | Offered but never opened by itself; Skip, Escape and backdrop close it and are remembered; all six steps with Back and Finish; strict focus trap and inert page; reopening from Docs; translated in every locale |
| `motion.spec.ts` | OS reduced motion and the in-app setting remove movement and keep a short fade; "Match system" follows the OS live; the drawer; reduced transparency makes the glass solid (Chromium, through the DevTools protocol) |
| `glass.spec.ts` | The brushed glass, in both themes: a live backdrop filter only on the top bar and, at 390 px, the decision card (from 672 px the card is static), with the theme's grain tile; an open drawer or tour is the one modal dialog, live level 3 (the drawer at the 16 px blur on a phone), with its scrim; static glass (grain, opaque gradient, no backdrop) on the sidebar, the run header, the Overview steps (their number untouched), the docs cards and the Settings panels (their 3:1 border kept); no glass nested in glass; tables and tags solid (opaque fill, no grain, no backdrop) on the Decision, Audit trail and Meta API tabs at both widths; the primary button's ink gradient and grain, and its pressed state (flat, no shadow); more contrast, forced colours and print drop every backdrop filter; in forced colours the tour keeps a 1px frame, the drawer its inline-end edge and the top bar its bottom line only; a hovered docs card shows `border/strong` (no ink border over it); disabled-colour text inside the top bar and the decision card is the secondary ink; axe with no violation at all on the Decision tab at both widths |
| `a11y.spec.ts` | axe on every page in both themes and all locales, the overlays, the 404 page in both themes, and phone layouts |
| `api.spec.ts` | Over the mocked `/api/v1` (`e2e/api-mock.ts`, the contract fixtures; the live section in the contract's shape, validated with the client's zod mirror): New deliberation -> run -> frames -> decision -> receipt link (with the `Idempotency-Key` and the `expected` block checked); a replayed finished run never reads LIVE and is named Playback; no LIVE word on a replayed, flagged or listed live-model run in the four locales, and LIVE for a live-model run in progress; a `provenance_inconsistent` run never reads LIVE even on live-delivered frames; pacing only for scripted runs, and off with the in-app Reduced setting; the header's labels; the live section's disabled state, the closed form (no site key, tripped or unreadable ledger) and the open form's request (trimmed invite code, a live refusal's sentence); an error's sentence and Retry-After; a failed detail request with its retry; a failed stream with the stored records and a reconnect; a failed run explained from its code; the scope line of a revision; no verbatim-log link for a live-declared run; My runs with and without a session; the Explorer (filter, translated statuses, SIMULATED proofs, =0 count); the run page without an id; the runtime NotConnected state against the static server; the Overview's button (and a server on another contract); the four locales; axe on `/new`, `/runs` and the three tabs of `/run` in every locale and theme, and on the open live form, a failed run and a failed detail; lane W2: the Evidence tab (simulated and sealed-snapshot coverage, the English original note), the Proof inspector (simulated and real proofs, a failed receipt, a receipt with other limitations), the Audit trail (chain, receipt, log, check command; a live-declared run), a guided run's log checked in the browser (pass, then a changed record failing at its sequence), the Meta API panel (scripted doubles, recorded Meta calls and totals), the explanations (plain, technical, before protocol 1.3, three locales), the revision view, and axe on all seven tabs; the guided Run, live Start, Approve and Reject buttons keeping the keyboard focus while their request is in flight and after it (and through a 429 wait); round 4: the connected and not-connected variants of the legend intro, the tour, the Docs build note, Modes and Settings, the outage wording, and error headlines by class and context (500, 503, 429 distinct; a refused decision; a showcase read), the 429 countdown with disabled buttons and the recorded-run link, and the guided error inside its card; the pending variants while the health probe is held, the notice's countdown reaching "You can retry now", a `rate_limited` wait shared with live Start (and a busy wait that is not), a refused cancel headed by its class, a decision answered with an unreadable 200 headed as uncertain, and the guided Run button keeping the focus after a 500 |
| `layout-contrast.spec.ts` | Round 4, heuristic evaluation #9 and #10: every main screen (Overview, New deliberation and its open live form, Runs, a run's Playback and Decision with a revision, its Explorer, Evidence, Proof inspector, Audit trail and Meta API tabs on a guided run and on a recorded run, the showcase, a recorded run's Decision, Docs, Settings, the legal pages) no wider than a 390 px phone in English and Spanish, the Decision screens in all four locales (`document.scrollingElement.scrollWidth`); the claim comparison stacked one block per claim below 672 px (its column names visual only), a table above, claim ids on one line; the decision's outcome tag never cut, its evidence label visible (simulated, sealed snapshot and a recorded run, four locales); and the computed colors of radio rings, checkbox boxes, the native select, the Settings panels and the dashed not-connected panel at 3:1 or more against their fill and surroundings in both themes, and of the theme control (its frame against the bar and its own fill, its dividers, the selected segment's ink fill against the bar and against its own icon and check; top bar and drawer) in both themes, in place of the off Switch track it replaced |
| `shapes.spec.ts` | Round 5, square-first shapes (design-direction 3.7): the built CSS keeps every corner on the 0/2/4/6 px scale outside the reasoned Astryx allow-list of `tests/radius-scan.ts`; every rendered element and pseudo-element computes corners on the scale (every page, a run's tabs, the live form, the tour, the drawer; tags included); the theme control (outer 4, joins 0, ink and check on the selected segment, no Switch), the 24 px square Overview step markers (radius 4), the tour's segmented progress bar (24x4, radius 0, current 40 wide, upcoming an empty frame), radio marks at radius 2 that stay native radios, navigation rows at radius 0; the theme control as a keyboard radio group (one tab stop on the selected segment, arrows move, select and wrap, Space selects, System removes the stored key, reload keeps the choice, no cookie), its names in all four locales, its icons at a 1.5 px drawn stroke, the drawer's worded variant, the drawer's focus trap holding from every segment whichever is checked (Tab and Shift+Tab, also from an unchecked segment that holds focus), and axe with no violation of any impact on it in both themes |
| `showcase.spec.ts` | Over the mocked `/showcase` and `/research` (entries from the archive's manifest and logs, the server's F03 entry): the list (RECORDED, by battery, outcomes next to their evidence, the manifest checked, the archive's note), a recorded run in RECORDED mode (no stream, read-only Decision, archive receipt and log, recorded proofs, the check command), its archived log checked in the browser (pass against the pinned digest and the receipt's head; a flipped byte failing at its record), its receipt said to be the server's rebuild with the run record's own limitations quoted on request (and an unreadable record said), a run blocked by proof-generation failures, a schema-1.0 run (no run record), a run the listing does not name, a failed listing, a forged manifest, no LIVE in four locales, the Overview's figures recounted from the same entries, and axe on the figures, the list and every recorded tab in four locales and both themes; a recorded live-model run with no flag at all (no delivery badge, never LIVE, four locales); the Overview's figures read as visible text against numbers counted from two different archives; hostile markup, markdown links and an unclosed bidi override in record text, model output, the manifest's note, the receipt's and the run record's strings, titles and labels rendered inert in every recorded view (with a control showing the check sees each); axe on a failing and a partial chain check, a schema-1.0 run, a forged manifest (list and run), a failed listing, a failed recorded detail and a run the listing does not name (fr, both themes), and on a phone (390x844); the recorded tabs (arrows, Home, End) and the chain check driven by the keyboard; a partly chained run's chain check intro in four locales |

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
  views read it (2026-10-28, pending that sign-off), and its showcase
  section says which recorded runs can be re-checked: those whose log
  carries a hash chain (schema 1.1 on), with the manifest digest compared
  against a tagged commit as advice, while runs before schema 1.1 carry no
  chain, so only their file's SHA-256 can be compared with the manifest's
  digest, which shows the archived bytes and not that no record changed
  before archiving (2026-10-29, pending that sign-off).
- Lane W2: the public surface publishes no output bound, attempt count or
  schema and prompt digests of a model call (operator only), so the Meta API
  panel says they are not published; the contract publishes no prices, so no
  cost is shown; showcase entries carry no dates, so the Overview's figures
  name runs, batteries, schema versions and policy rules but no dates; and
  the in-browser chain check always fails a line with invalid UTF-8, where
  the runtime decodes it lossily (U+FFFD): the runtime accepts that in a
  pre-1.1 record, which no hash covers, and fails it in a chained record
  either at parsing or at the hash. On a non-canonical line (other
  whitespace, member order or escapes, unknown members) the browser, which
  hashes the raw line, and the runtime, which hashes its own
  re-serialization, compute different hashes: a record hashed the runtime's
  way fails in the browser and passes the runtime, and a record hashed over
  its raw line passes in the browser and fails the runtime; and the runtime
  ignores, and does not hash, top-level members it does not know (a crate
  matter, not changed here).
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
