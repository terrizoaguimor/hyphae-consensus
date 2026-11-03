# Run it locally

The [README](../README.md) has a summary of this page.

Prerequisites: Rust 1.99.1 via [rustup](https://rust-lang.org/tools/install), pinned in
`rust-toolchain.toml` (on Windows with the MSVC build tools, see the
[rustup book](https://rust-lang.github.io/rustup/installation/windows-msvc.html)), and
Docker for the web app. The commands below are for a POSIX shell (Git Bash on Windows),
from the repository root. None needs a credential, and none calls Meta or Hyphae.

```sh
cargo build --workspace --locked
cargo test --workspace --locked                           # deterministic; the 2 live tests are ignored
cargo run -p consensus-tests --example evaluate           # scripted protocol conformance check (10/10)
cargo run -p consensus-api --locked -- verify research/2026-10-27/live-battery-2026-10-27-final.jsonl   # read-only chain and policy check
```

**Scripted demo.** The `consensus-api` binary without a subcommand runs the reference
scenario to `AwaitingHumanApproval`; `--approve` or `--reject` also records a decision
(nothing is executed). It keeps its log in `scripted-demo/events.jsonl` under the current
directory and replays the run it finds there instead of re-running, so run it from a
scratch directory. It refuses to run inside `demo-run/` or `research/`, and any other
argument prints the usage.

```sh
REPO=$(pwd); cd "$(mktemp -d)"
"$REPO/target/debug/consensus-api"              # scripted run, stops at AwaitingHumanApproval
"$REPO/target/debug/consensus-api" --approve    # replays it and records an approval
```

**Docker.** The `Dockerfile` at the repository root builds the `consensus-api` binary with
the pinned toolchain (`--locked`) into a slim Debian image (about 140 MB) that runs as a non-root user
(uid 10001) and holds `research/` at `/opt/hyphae/research`. It is for two things: the scripted
demo, and re-verifying the archived runs with `verify`. It holds no credential, makes no
network call and has no web build.

```sh
docker build -t hyphae-consensus .
docker run --rm hyphae-consensus                    # scripted run, stops at AwaitingHumanApproval
docker run --rm hyphae-consensus --approve          # also records an approval (nothing is executed)
docker run --rm hyphae-consensus verify /opt/hyphae/research/2026-10-27/live-battery-2026-10-27-final.jsonl
docker run --rm hyphae-consensus --help             # the usage; any unknown argument prints it and exits 2
sh eval/verify-archive.sh                           # verify every archived log in the image (eval/README.md)
```

The demo's log lives in `/home/hyphae/scripted-demo/` inside the container and goes with it.
The image is **not** the portal: the public server binds only a unix socket or a loopback
TCP address, so a published container port could not reach it, and the web app is built only
through `apps/web/scripts/in-docker.sh`. To run the portal, follow "Web app and public
server" below on the host.

**Operator console** (loopback only; it must never be exposed):

```sh
CONSENSUS_RUN_STORE_DIR="$(mktemp -d)" cargo run -p consensus-api --locked -- serve-operator
# http://127.0.0.1:8787/ ; the bearer token is printed once at startup (or read from CONSENSUS_OPERATOR_TOKEN_FILE)
```

`model=fake` runs the five guided scenarios with simulated proofs and needs nothing else.
A non-loopback bind is refused, every `/api` route needs the bearer token, and approving a
`blocked` or `abstained` run needs a note. Live modes (`model=meta`, `memory=hyphae`) need
credentials from the environment (`.env.example`, `scripts/hyphae-container/README.md`),
spend money on the Meta key, and without them the API names the missing variable instead
of falling back. Secrets must come from environment variables, never files committed to
this repository.

**Web app and public server.** The Node toolchain runs only in throwaway containers, never
on the host (`apps/web/README.md`, "Build only through Docker"):

```sh
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm build        # static export to apps/web/out
sh apps/web/scripts/in-docker.sh pnpm check        # theme, contract types, typecheck, lint, Vitest, export
sh apps/web/scripts/playwright-docker.sh           # end-to-end and axe tests against out/, API mocked
CONSENSUS_PUBLIC_BIND=127.0.0.1:8788 CONSENSUS_COOKIE_SECURE=false \
  CONSENSUS_RUN_STORE_DIR="$(mktemp -d)" cargo run -p consensus-api --locked -- serve-public
# http://127.0.0.1:8788/ : guided runs and the research showcase; live mode stays off
```

**Known flake (end-to-end).** In the WebKit project, `e2e/api.spec.ts` "new -> run -> frames ->
decision -> receipt link @smoke" has failed once under load in a full run, on the CI run 41551913221
for `255f21c` and once on a developer machine (the final audit of `e540fd7`: a time-out waiting for
the skipped-pacing marker). Both failures showed the same two console errors, "Refused to apply a
stylesheet ... style-src" and "CSP violation: style-src-elem blocked inline", so a
Content-Security-Policy style violation may be shared between them. The isolated reruns (3 of 3)
and the green CI run for `e540fd7` did not report them.
Rerun it alone (`PW_WORKERS=1 sh apps/web/scripts/playwright-docker.sh --project=webkit e2e/api.spec.ts:109`, the line of that test when this was written)
before treating a failure of it as a regression. The cause is not established.

`CONSENSUS_PUBLIC_BIND` is for development only (loopback; in production the public server
listens on a unix socket behind cloudflared). The public server's variables are documented
in `crates/consensus-api/src/public_config.rs` and `crates/consensus-api/src/live/config.rs`.
Both servers keep one hash-chained log per run under `CONSENSUS_RUN_STORE_DIR` (default
`consensus-store/`), never a log of `demo-run/` or `research/`. Request bodies are typed:
unknown fields are rejected (`422`), `model` and `memory` are enums without defaults,
`model=fake` with a non-fake memory is refused, and the public surface accepts no free
text for any model (guided scenarios and live presets only; the optional decision note
after a run is the one visitor text, and no model sees it).

**Never delete `demo-run/`.** The git-ignored `demo-run/` holds the original live research
data behind `docs/evaluation.md`: the battery logs, the run records with their receipts,
the raw model drafts and the Hyphae recovery files. `research/` holds byte-identical copies
of its event logs and run records; the raw drafts and the recovery files exist only in
`demo-run/`. Never open a log in either place with a writing tool: `EventLog::open` opens a
log read-write and can repair a torn tail. Earlier builds kept the CLI demo's log in
`demo-run/events.jsonl`, which therefore holds the 2026-10-26 evening runs of
`docs/evaluation.md` §4; the demo no longer opens it. The servers open archived logs
read-only (`consensus-api verify`, the showcase).
