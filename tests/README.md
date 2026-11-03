# Tests

This directory is the entry point to the test suites of Hyphae Consensus. The tests themselves
live next to the code they check, in the places listed below; `tests/run-all.sh` runs them from
here.

```sh
sh tests/run-all.sh --help      # the options
sh tests/run-all.sh --quick     # a few minutes: the conformance example + chain verification of one archived log
sh tests/run-all.sh --dry-run   # print every command, run nothing
sh tests/run-all.sh             # everything: --rust, --web and --e2e
```

Everything runs in Docker (Rust in Ubuntu 24.04, Node and Playwright in pinned images): the
Node toolchain runs only in throwaway containers, never on the host (`apps/web/README.md`,
"Build only through Docker"). No suite needs a Meta key or a Hyphae daemon, and none calls
Meta, Hyphae or Cloudflare. The two tests that would (the live probes) are `#[ignore]`d.

## The numbers, and where they come from

These are the results of the CI run of commit `521d823`
([run 41652255882](https://github.com/terrizoaguimor/hyphae-consensus/actions/runs/41652255882),
both jobs green), read from its log. They are a record of that run, not a promise about a later
commit; the workflow is `.github/workflows/ci.yml`.

| Suite (CI step) | Result at `521d823` |
|---|---|
| Rust: `cargo test --workspace --locked` and the demo crate | 759 passed, 0 failed, 2 ignored (757 in the workspace, 2 in `demo-repo/payment-client`; the 2 ignored are the live tests that call Meta and need a key) |
| Vitest (`pnpm check`) | 43 files, 1004 passed, 16 skipped (cases that skip themselves when an input is absent, for example `tests/api-smoke.test.ts` without a capture of real-server responses) |
| Playwright | 636 passed across 4 projects (chromium, chromium-mobile, firefox, webkit) |
| axe (inside the Playwright run) | 340 scans, 0 violations (the run's own line: "violations by impact {}, serious or critical: 0") |
| Deploy rehearsal, mode A (`scripts/deploy-rehearsal.sh`) | 190 passed, 0 failed. Not a CI step: this figure is from the final audit of 2026-11-03, a local run ([docs/threat-model.md](../docs/threat-model.md) section 7) |

The scripted conformance example (10 of 10 cases, 0 unauthorized executions) is a CI step too;
its table is in [docs/evaluation.md](../docs/evaluation.md) section 1.

## Map of the suites

| Suite | Where | What it proves | Run it |
|---|---|---|---|
| Rust unit tests | `#[cfg(test)]` modules in `crates/*/src` (domain types, state machine, event log, policy, orchestrator, adapters, limits, the live gate and ledger) | Each module's rules in isolation: transitions, the hash chain, policy rules, request limits, config validation | `cargo test --workspace --locked` (through `sh tests/run-all.sh --rust`) |
| Rust integration tests, API | `crates/consensus-api/tests/*.rs` (for example `api_public.rs`, `api_operator.rs`, `api_verify.rs`, `meta_adapter.rs`, `hyphae_adapter.rs`, `live_gate.rs`, `live_budget.rs`, `snapshot_client.rs`, `baseline.rs`) | The public and operator HTTP surfaces end to end against fakes (a loopback fake of Meta's Chat Completions endpoint, a scripted Hyphae); sessions, limits, SSE, receipts, the live gate and the USD ledger; the `verify` subcommand (a changed byte fails); the credential canary (a fake key appears in no output) | same |
| Rust integration tests, scenarios and archive | `crates/consensus-tests/tests/*.rs` (`m1_scenarios.rs`, `protocol_1_3.rs`, `receipts_v2_scenarios.rs`, `run_store.rs`, `research_archive.rs`, `research_receipts.rs`, `research_baseline.rs`) | Deterministic and adversarial scenarios through the whole runtime; the research archive re-checked from its bytes (on temporary copies, never in place) against `research/MANIFEST.json` | same |
| Scripted conformance example | `crates/consensus-tests/examples/evaluate.rs` | Ten pinned cases, each with an outcome its author wrote down in advance (10 of 10 produced; 0 unauthorized executions). Deterministic doubles: it shows the runtime enforces its own rules, it is not an experiment (docs/evaluation.md section 1) | `cargo run -q -p consensus-tests --example evaluate` (also in `--quick`) |
| Contract drift | `crates/consensus-api/tests/api_contract.rs`, `contracts/api-v1/` | The committed JSON Schemas and fixtures equal what the server renders | part of `cargo test` |
| Demo crate | `demo-repo/payment-client/src` | The synthetic payment client that the scenarios reason about (2 tests) | `cargo test --manifest-path demo-repo/payment-client/Cargo.toml --locked` |
| Shell and Python self-tests | `deploy/scripts/verify-edge-selftest.sh`, `deploy/scripts/restart-when-idle-selftest.sh`, `scripts/hyphae-container/capture-snapshot.sh --selftest`, `python3 -I scripts/snapshot_manifest.py selftest`, `python3 -I scripts/baseline_battery.py selftest` | The deploy and tooling scripts behave as documented, offline, with stubs for `curl`, `ssh` and `systemctl` | gates of `sh scripts/linux-gates.sh` |
| Linux gates | `scripts/linux-gates.sh` | In an Ubuntu 24.04 container (the deployment target) with the checkout mounted read-only: `cargo fmt --check`, `clippy -D warnings`, the workspace tests, the demo crate, the conformance example and the five self-tests above; every gate runs even after one fails | `sh tests/run-all.sh --rust` |
| Deploy rehearsal | `scripts/deploy-rehearsal.sh` | The server half of the deployment in a throwaway container (install, layout, serve-public over a unix socket, the public routes and their headers, isolation, credentials), no network and no Meta call. Needs the release binary and the web export first; see the header of the script | `sh scripts/deploy-rehearsal.sh` (not part of `run-all.sh`) |
| Vitest | `apps/web/tests/*.test.ts` | The web app's pure logic and static properties: API client and schemas, the in-browser chain verifier over the real archived bytes, run views and explanations, i18n parity and wording in four languages, colour contrast, glass and radius scans, CSP manifest, SEO | `sh apps/web/scripts/in-docker.sh pnpm check` (through `--web`) |
| Playwright + axe | `apps/web/e2e/*.spec.ts` (`api.spec.ts`, `a11y.spec.ts`, `showcase.spec.ts`, `decision.spec.ts`, `navigation.spec.ts`, ...) | The static export in four browser projects against the contract fixtures as a mocked `/api/v1`: user flows, the four locales, both themes, phone width, and axe on the main screens. The container has no network | `sh apps/web/scripts/playwright-docker.sh` (through `--e2e`; needs the export that `--web` builds) |

## `run-all.sh`

| Option | Runs |
|---|---|
| `--rust` | `sh scripts/linux-gates.sh` (about 10 to 15 minutes; the first run also fills the cargo volumes) |
| `--web` | `sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile`, then `... pnpm check` (theme, API types, typecheck, lint, Vitest, static export) |
| `--e2e` | `sh apps/web/scripts/playwright-docker.sh` (needs `apps/web/out`, which `--web` builds; the script stops with exit 66 and says so if it is missing) |
| `--quick` | In one `rust:1.99.1-slim-bookworm` container with the checkout mounted read-only: the conformance example, then `consensus-api verify` on `research/2026-10-27/live-battery-2026-10-27-final.jsonl` |
| `--dry-run` | Prints the commands and runs nothing |
| none | `--rust`, `--web` and `--e2e`, in that order |

It stops at the first failing command, exiting with that command's status, and prints a
summary. It deletes nothing. `--quick` keeps its build output and the cargo registry in two
Docker volumes (`hc-tests-target`, `hc-tests-cargo`; the prefix is `HC_TESTS_VOLUME_PREFIX`) so
a second run is fast; `docker volume rm` them to reclaim the space. `--web` writes
`apps/web/node_modules`, `.next` and `out`, and `--e2e` writes `apps/web/test-results`, all of
them git-ignored. On Docker Desktop for Windows export `HC_WEB_NODE_MODULES_VOLUME=hc-web`
first, as [docs/running-locally.md](../docs/running-locally.md) says.

## Known flake

In the WebKit project, `e2e/api.spec.ts` "new -> run -> frames -> decision -> receipt link
@smoke" has failed under load, once on CI and once on a developer machine, and its isolated
reruns passed. The exact wording, the two failures and the command to rerun it alone are in
[docs/running-locally.md](../docs/running-locally.md), under "Known flake (end-to-end)". The
cause is not established. Rerun that test alone before treating its failure as a regression.

## What these suites do not show

They do not call the live models, so they say nothing about live model quality; that is what
the recorded runs are for, and they are described in [eval/README.md](../eval/README.md) and
[docs/evaluation.md](../docs/evaluation.md). A passing suite shows the code does what its
tests say, not that a proof or a policy outcome is right in the world.
