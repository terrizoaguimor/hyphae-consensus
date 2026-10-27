# Reviews, agent briefs and run reports

This folder is the audit trail of Hyphae Consensus: independent reviews, the
briefs given to headless agent runs, and the reports those runs wrote. The files
are kept as written, including statements that later turned out wrong.
Corrections live in the errata below and in dated sections of the documents they
concern. Nothing in this folder is a current instruction.

| File | Date | Kind | What it is |
|---|---|---|---|
| `2026-10-26-claude-opus-review.md` | 2026-10-26 | Review, historical (in Spanish) | Read-only independent review of the M1–M3 code by Claude: 14 findings, a rubric and a top-5 list. Input to the first fix run |
| `2026-10-26-claude-fix-brief.md` | 2026-10-26 | Agent brief, historical | Instructions for the first headless fix run, written for that run only |
| `2026-10-26-claude-fix-report.md` | 2026-10-26/27 | Agent run report, historical | What the first fix run did: finding-by-finding status, two new defects, the console HTTP smoke test, the 2026-10-26 live battery, schema 1.1 |
| `2026-10-27-claude-fix-brief-2.md` | 2026-10-27 | Agent brief, historical | The owner's decisions on the open items of fix report 1, given as instructions to the second headless run |
| `2026-10-27-claude-fix-report-2.md` | 2026-10-27 | Agent run report, historical | What the second run did: live proofs, schema 1.2, the 2026-10-27 batteries, the hostile note with a valid proof and its deletion |
| `2026-10-27-independent-audit.md` | 2026-10-27 | Audit, current | Multi-agent audit of `72e422a` with adversarial verification, research sweeps, a completeness critic, the plan's work items and the owner's decisions for the public portal |

## Superseded instructions (audit finding DOC-11)

The two briefs were written for one run each. Do not follow them now. In
particular:

- Brief 1, "Live battery" step, starts Hyphae with `hyphae serve --data-dir
  /hyphae-home/.local/share/hyphae/agent-memory --native-api-key-auth` and points
  `HYPHAE_NATIVE_ENDPOINT` at a socket inside that data directory. That is the
  misconfiguration that made every proven recall fail (ADR-0003, "Why live proofs
  failed, and the fix"). Use `scripts/hyphae-container/serve.sh`, which serves
  with the endpoint outside the data directory.
- Brief 1 calls the proof failure a "KNOWN UPSTREAM BUG". It was not a Hyphae
  defect: Hyphae's generic `corruption` code reported the socket inside the data
  directory, and later the 4 MiB MCP response limit (ADR-0003).
- Both briefs say the repository license is undecided and not the agent's to
  choose. Superseded: the owner chose Apache-2.0 on 2026-10-27 (`LICENSE`,
  `NOTICE`, README).
- Brief 1 tells the run to execute the ignored live tests and a live battery, and
  brief 2 authorizes recovery commands and a fresh store inside the
  `hyphae-consensus` container. Those authorizations covered those runs only.
  Live model calls, live batteries and container changes each need the owner's
  explicit, current authorization.
- Still valid from both briefs: agents never push; the owner authorizes every
  push.

The briefs and reports also name operational details: where the Meta key is
stored (the Windows user environment), container credential paths, a container
process id, a socket digest and backup checksum prefixes. None of these is a
secret value. The session logs of the headless runs (`claude-fix-run*.log`) are
not in the repository; one of them held an operator token, void since its server
process stopped (fix report 1, "Incident").

## Errata

**`2026-10-26-claude-fix-report.md`**

- Row 11d ("Blocked upstream") and the "upstream corruption signal" wording: the
  cause was a deployment misconfiguration, the daemon's socket inside its data
  directory, not a Hyphae defect (ADR-0003; fix report 2, A.1).
- Row 6 ("Fixed: per-item challenge ids"): items got ids, but the no-progress exit
  still compares counts of unresolved items, and the ids are new every round
  (audit RT-7).
- Row 5b ("Fixed: UI label"): the label exists in the launch form and in every
  verification record and receipt, but the console's result views show a
  simulated proof with the same `valid` badge as a real one (audit UI-4, SEC-16).
- Risk 6: the mutex is held for a whole live run, which on 2026-10-27 meant 3.7 to
  6.8 minutes, not 1 to 2 (audit SEC-4). "There is no CSRF protection": axum's JSON
  content-type check and the missing CORS layer resist classic CSRF today, but
  nothing tests that (audit SEC-14).
- Row 13 and risk 10 ("a public OSI license may be required"): the Official Rules
  require no license (audit, section 6.1).
- Risk 3 (the hostile note remains in the store): the note was forgotten from every
  served store on 2026-10-27 (`docs/evaluation.md` §5.4).

**`2026-10-27-claude-fix-report-2.md`**

- Outcome and A.3 ("8/8 `contested`, every final-round claim corroborated"): true
  for the final battery F01–F06 only. H02's final round corroborated 1 of 4 claims,
  and H01's final round had 2 claims with contradicting evidence (audit DOC-3).
- Outcome ("Two defects in this round's own changes"): only the Evaluator wording
  was new. The field-name drift had existed since the first commit (`adbbb66`), as
  the report's own defect section says (audit DOC-4).
- Outcome ("Local commits only, nothing pushed, no license chosen"): true when
  written. `origin/main` was later pushed to `72e422a`, and the license is now
  Apache-2.0.
- A.3 and the R06 success criterion: R06's `supported` depended on protocol 1.2 not
  re-judging 7 round-1 objections (audit RT-1; `docs/evaluation.md`,
  "Corrections").
- Proof-size figures ("near 3.85 MB" in A.1, "about 3.9 MB" elsewhere, 3.8 MB in
  `scripts/hyphae-container/hyphae-env.sh`): approximations of one limit, the
  4 MiB MCP response that carries the whole witness (ADR-0003).
- B ("the owner may purge them"), open decision 5 ("may be pruned") and open
  decision 6 ("Keep or delete them"): archive first and never delete research
  evidence. `demo-run/` holds the only copy of the live research data, and the
  proof witnesses are needed to re-verify recorded proofs. The plan allows
  owner-run pruning of container artifacts only after archiving (audit DOC-2,
  HY-5).
- Open decision 2 ("the console shows them", about uncertainties): the console
  never shows uncertainties (audit UI-6).

**`2026-10-26-claude-opus-review.md`**

- "Fortalezas" lists dropping unknown ids ("descarta ids desconocidos") as a
  strength. The 2026-10-27 audit found that dropping them is a fail-open path
  toward `supported` (MA-1).
- Section 13 suggests the rules may require a public repository with an OSI
  license. The Official Rules require neither.
- Section 6: the `unresolved >= prev` rule it flagged still compares counts
  (RT-7).

## Where the dated corrections are

- `docs/evaluation.md`: "Corrections (2026-10-27, independent audit)".
- `docs/threat-model.md`: "Known gaps found by the 2026-10-27 audit", and the
  corrected wording of threat 5.
- `docs/adr/0001-m1-architecture.md`: "Amendment — 2026-10-27 independent audit".
- `docs/adr/0002-meta-model-adapter.md`: "Note — 2026-10-27 independent audit".
- `docs/demo-script.md`, `docs/mission-control.md`: superseded banners.
- `docs/pitch.md`, `docs/journal.md` and the README status: corrected statements,
  each with a dated note.
