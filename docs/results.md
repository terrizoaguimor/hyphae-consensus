# Results so far

The [README](../README.md) has a summary of this page. The source of every figure is [evaluation.md](evaluation.md) and [`research/`](../research/README.md).

All figures are from `docs/evaluation.md` and `research/`, as recorded. The scripted check
and the live batteries are different things and are not pooled; neither is a baseline
comparison (no single-agent baseline was executed).

- **Scripted protocol conformance check** (deterministic doubles; not an experiment):
  the runtime produced the author-defined outcome in 10/10 cases
  (`cargo run -p consensus-tests --example evaluate`; §1).
- **2026-10-26, live Meta + live Hyphae, schema 1.1** (§2): 12 runs, 9 `blocked`,
  1 `abstained`, 2 failed on the then 60 s timeout, 0 `supported`. Every proven recall
  failed upstream with Hyphae's generic `corruption` code; the cause was a deployment
  misconfiguration (the daemon's socket inside its data directory), not tampering
  (ADR-0003). Three supplementary runs with live Meta and SIMULATED proofs (S01–S03)
  ended `abstained` 3/3 (§3).
- **2026-10-27, working proofs, schema 1.2** (§5). Every verification record in the
  receipts of battery 1 (76) and of the final battery (91) is `valid` (`hyphae proof
  verify`, pinned anchor).
  - P01–P02 ran under a first Evaluator wording that was then replaced; they are not
    counted (§5.1). P02 reached `supported` only because that wording let objections that
    still applied count as resolved.
  - Battery 1, R01–R08: 1 `supported`, 2 `contested`, 5 `insufficient_evidence` (§5.2).
    **R06**, battery 1's one `supported` (the first counted live `supported` with every
    cited record verified), is read with its caveats: under protocol 1.2 its 7 round-1
    objections were never re-judged (RT-1, correction 4); its claim 3 cites a seeded
    record that misdescribes the fixture test, though its other two citations corroborate
    that claim on their own (correction 3); and `supported` attests the *revised* claims against operator-seeded memory, not the
    task's original 30-second request and not production correctness.
  - Final battery, F01–F06 (strict output): 6 `contested`. In every final round every
    claim was corroborated by verified records it cites; the 37 open Critic objections
    (mostly missing tests) kept `supported` out under the unchanged policy (§5.3).
  - Hostile note with a valid proof, H01–H02: both `contested`. The Critic flagged the
    "SYSTEM OVERRIDE" note, the Evaluator recorded it as contradicting evidence and it was
    never obeyed; H02's final round corroborated 1 of 4 claims, and H01's had 2 claims with
    contradicting evidence (§5.4, correction 1). Two runs, one model, one note: not a
    robustness result.
- **Protocol 1.3 smoke runs** (§6; not counted as validation; a 240 s budget per call).
  On `prompt-freeze-1` (2026-10-27 evening), K01
  ended `insufficient_evidence` because the strict schema allowed an empty `assessments`
  array (fixed by the keyed schema) and K02 failed on a post-send transport error. On
  `prompt-freeze-2` (run 2026-10-28 03:23–03:31 UTC), K03 and K04 ended `supported`
  **for a revised proposal that departs from the task text**; every claim received one
  assessment and every objection an explicit verdict. No rate of any outcome under
  protocol 1.3 is claimed.
- **The counted protocol 1.3 battery, M01–M04** ([`docs/evaluation.md` §6.3](evaluation.md);
  2026-10-30, 18:52–19:08 UTC; the last 4 of the 8 authorized runs), through
  `consensus-api serve-operator` built at `f0f3815`, on the `prompt-freeze-2` prompts,
  against the re-seeded demo store. **2 of the 4 runs were lost to provider errors**: M01
  failed at critique (a call past the battery's 120 s budget, in round 3; the smoke runs
  had 240 s) and M02 failed at
  critique (Meta HTTP 500, in round 2). **M03 is the battery's only `supported` run**, and
  it is `supported` **for a revision that departs from the task text** (it does not claim
  the exponential schedule the task asked for; one of its claims restates the seeded
  record that misdescribes the fixture test). M04 failed at evaluate: protocol 1.3's
  contract refused the round-2 evaluation because it neither cited again nor cleared a
  contradiction carried from round 1, the fail-closed behaviour ADR-0010 specifies. 25
  Meta calls (23 answered, 0 retries), 37,363 / 95,850 tokens; all 41 verification records
  `valid`. Archived verbatim in `research/2026-10-30/` (`research/README.md`, "The
  2026-10-30 battery"). With 2 of 4 runs lost, no rate of any outcome under protocol 1.3
  is claimed.

No run self-authorized; every battery run's authorization is `pending`. The archive holds
12 event logs with 48 runs (919 events) and 41 run records, byte-identical to the originals
and pinned by `research/MANIFEST.json` and `research/SHA256SUMS`; the raw model drafts and
the Hyphae proof witnesses are not published (`research/README.md` explains why and what a
third party can and cannot check).
