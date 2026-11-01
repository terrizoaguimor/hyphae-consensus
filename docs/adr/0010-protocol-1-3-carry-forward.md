# ADR-0010: Protocol 1.3 — fail-closed evaluation contract and cross-round carry-forward

Date: 2026-10-27 · Status: accepted, implemented (commits `1ee386a`, `92ccfeb`,
`08f6d33`, `c8749f5`; keyed Evaluator schema `3d1aeec`), **smoke-validated live on
`prompt-freeze-2`** (K03, K04: `docs/evaluation.md` §6.2), **counted battery run
2026-10-30** (M01–M04, `docs/evaluation.md` §6.3: the contract of decision 3 refused one
evaluation live, M04's, for a carried contradiction neither cited again nor cleared)
· Scope: schema 1.3

ADR-0005 (per-run logs and asynchronous execution) and ADR-0006 (public API and
sessions) are written after their lanes land; the numbering is not chronological.

## Context

The independent audit of 2026-10-27 (findings MA-1 and RT-1, `docs/threat-model.md` section 4)
found two paths by which a run could reach `supported` without anyone judging the
objections that stood against it:

- **MA-1.** The Meta adapter silently dropped Evaluator ids that were not byte-exact
  (`Item-5`, `item 5`, a claim id used as an item id, a duplicate assessment, an unknown
  contradicting id). `unresolved_items` is the only place open objections count, so an
  emptied list fell through to `supported`. In 7 of the 10 recorded `contested` live runs
  those ids were the only blocker. The live model had copied all 190 ids exactly, so the
  path was latent, not observed.
- **RT-1.** Every round rebuilt the model context from scratch, the adapter minted new
  item ids each round, and the policy read only the final round. Objections and recorded
  contradictions from round 1 were never shown or judged again. Live run R06 reached
  `supported` with 7 round-1 objections that no later evaluation judged.

The owner decided (2026-10-27) to fix both without changing the policy function: the
rules of schema 1.2 stay, and the inputs the rules see become complete.

## Decisions

1. **Schema 1.3 is additive.** Every new field reachable from `ConsensusEvent` is an
   `Option` or `Vec` with `#[serde(default, skip_serializing_if = …)]`, and no serialized
   type uses a hash map. A test asserts that every chained line of every archived log in
   `research/` (585 records, schemas 1.1 and 1.2) still round-trips byte for byte, so
   their hash chains keep verifying. New fields: `Evaluation.item_verdicts` and
   `cleared_contradictions`; `Recommendation.unresolved_uncertainties`;
   `HumanAuthorization.authority`; `Task.mode`; evidence provenance fields
   (`retrieval`, `snapshot_id`, `captured_at`, `proof_ref`, `proof_sha256`,
   `proof_request`, `backend_project`, `backend_scope`); `EvidenceVerification.round`,
   `proof_ref`, `report`; the `ModelInvocation` request-detail fields; `RunFailed.code`
   and `.invocation`; `EvidenceUnavailable.code`; and the event `RecallProven`.

2. **Policy rules are unchanged.** `PolicyVersion::for_schema("1.3")` maps to the 1.2
   rules. `policy::explain` is the single implementation (a `PolicyTrace` with the rule
   that fired and the ids behind it); `derive_recommendation_with` returns its kind, and a
   differential test holds it to a frozen copy of the pre-change function over 20,000
   pseudo-random cases per version. New runs are labelled **"protocol 1.3 · policy rules
   1.2"**: the function is the same, its inputs are not, so 1.2 and 1.3 outcomes are never
   pooled in one statistic.

3. **An evaluation must satisfy a contract before it is recorded, and again on replay.**
   `contract::validate_evaluation` enforces: exactly one assessment per current claim and
   none for an unknown claim; every cited or contradicting id was retrieved in this run;
   one explicit verdict (`resolved` with a non-empty reason, or `unresolved`) for **every**
   open item, no missing, unknown or duplicate id; `unresolved_items` equal to the
   unresolved verdicts; every contradiction carried from an earlier round either cited
   again as contradicting or cleared with a reason. The orchestrator refuses a violating
   evaluation before anything is persisted (`RunFailed{stage: "evaluate", code:
   "evaluation_contract"}`), and `RunAggregate::apply` runs the same check on every 1.3
   record, so a forged 1.3 log fails replay. The Meta adapter never drops an id any more:
   an unknown or duplicate decision-relevant id is `MalformedOutput` naming the field.

4. **Open items and contradictions are carried forward.** The state machine derives
   `open(k) = open(k-1) − resolved(k-1) + items(challenge k)`; item ids are unique per run;
   each round's context holds every record retrieved so far (still at most 20) plus the
   carried open items and contradictions; the Critic is told not to repeat carried
   objections, the Evaluator must judge each of them, the reviser sees them all. Silence
   never clears an item or a contradiction. Progress is set-based: a revision happens only
   while items remain open, the result is not `blocked`, at least one previously open item
   was resolved, and the round bound allows.

5. **No schema downgrade inside a run.** A record labelled with an older schema than an
   earlier record of the same run fails both the event log and replay
   (`SchemaDowngrade`), and a chained record labelled 1.0 is refused, so a forger cannot
   relabel the one record whose check would catch the forgery.

6. **Receipts show what older protocols dropped.** Receipts v2
   (`receipts::build_receipt_v2`) list `earlier_round_objections_never_rejudged`,
   computed from the records for every protocol: an earlier-round item counts as judged
   only if the final evaluation lists it as unresolved or a later evaluation resolves it
   with an explicit verdict. On the archive: R06 = 7, P02 = 8, F01–F06 = 13, 6, 5, 13, 6, 8.
   For contract-valid 1.3 runs the list is empty by construction.

## Consequences

- R06's recorded `supported` is now presented with its trace ("7 earlier-round objections
  never re-judged") and the correction about seeded record 5
  (`docs/evaluation.md`, corrections section). It is not re-labelled; the record is history.
- The Evaluator prompt changed (explicit verdicts, carried items, carried contradictions)
  and later rounds carry more evidence, so prompts, token use and outcomes must be
  **re-validated live** (authorized battery of at most 8 runs) before the prompt freeze.
  Until then no claim about live behavior under protocol 1.3 is made anywhere.
  **Note 2026-10-30:** the 8 runs are spent (K01–K04 smoke runs, M01–M04 counted battery,
  `docs/evaluation.md` §6). The claims made are the recorded runs only; with 2 of the 4
  counted runs lost to provider errors, no rate of any outcome is claimed.
- A hostile record that contradicts legitimate ones now stays visible in every round; it
  can still force `contested`, never `supported`.
- Relabelling an entire run to 1.1/1.2 and re-hashing it still passes as an honest older
  run; only an outside anchor (a previously exported receipt head, the run index of
  ADR-0005, or signed receipts after the hackathon) detects that.
