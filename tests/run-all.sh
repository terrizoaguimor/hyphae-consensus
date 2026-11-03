#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# One entry point to the test suites of this repository (tests/README.md maps
# every suite). It needs only Docker: every command below runs the toolchain in
# a throwaway container (Rust gates in Ubuntu 24.04, Node and Playwright in
# their pinned images), never on the host. It calls no Meta or Hyphae service and
# needs no credential.
#
# Usage, from any directory (Git Bash on Windows, Linux or macOS):
#   sh tests/run-all.sh             # --rust, --web and --e2e, in that order
#   sh tests/run-all.sh --quick     # the fast subset (a few minutes, see below)
#   sh tests/run-all.sh --rust      # sh scripts/linux-gates.sh (about 5 minutes with warm cargo volumes, measured 314 s; the first run takes longer)
#   sh tests/run-all.sh --web       # pnpm install and pnpm check in apps/web (Docker)
#   sh tests/run-all.sh --e2e       # Playwright + axe against the static export (needs --web first)
#   sh tests/run-all.sh --dry-run   # print the commands that would run, run nothing
#   sh tests/run-all.sh --help
# Suite options can be combined (`--rust --web`); they always run in the order
# quick, rust, web, e2e. The first command that fails stops the run with a
# non-zero exit status; the run ends with a summary of what passed.
#
# --quick runs, in one rust:1.99.1 container with the repository mounted
# READ-ONLY: the scripted protocol conformance check
# (`cargo run -p consensus-tests --example evaluate`, 10 cases) and the
# read-only chain and policy check of one archived log (`consensus-api verify`).
# Build output and the cargo registry live in two named Docker volumes
# (<prefix>-target, <prefix>-cargo; prefix hc-tests, or HC_TESTS_VOLUME_PREFIX)
# so a second run is fast; remove them with `docker volume rm` when you no longer
# want them. Nothing else is created or deleted: this script has no destructive
# command. --web writes apps/web/node_modules, apps/web/.next and apps/web/out
# (git-ignored); --e2e writes apps/web/test-results (git-ignored).
#
# Docker Desktop for Windows: export HC_WEB_NODE_MODULES_VOLUME=hc-web before
# --web and --e2e (see apps/web/README.md); the variable is read by
# apps/web/scripts/in-docker.sh and playwright-docker.sh.

set -eu

IMAGE="rust:1.99.1-slim-bookworm"   # the toolchain of rust-toolchain.toml
QUICK_LOG="research/2026-10-27/live-battery-2026-10-27-final.jsonl"

usage() {
  sed -n '/^# Usage, from any directory/,/^# non-zero exit status/p' "$0" | sed 's/^# \{0,1\}//'
}

want_quick=no
want_rust=no
want_web=no
want_e2e=no
dry=no
for arg in "$@"; do
  case $arg in
    --quick) want_quick=yes ;;
    --rust) want_rust=yes ;;
    --web) want_web=yes ;;
    --e2e) want_e2e=yes ;;
    --dry-run) dry=yes ;;
    -h | --help) usage; exit 0 ;;
    *) echo "tests/run-all.sh: unknown option $arg" >&2; usage >&2; exit 64 ;;
  esac
done
if [ $want_quick = no ] && [ $want_rust = no ] && [ $want_web = no ] && [ $want_e2e = no ]; then
  want_rust=yes
  want_web=yes
  want_e2e=yes
fi

# Always work from the repository root, wherever the script is called from.
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"

# Git Bash: give Docker the Windows form of the path and stop MSYS from
# rewriting the container paths in the arguments (as scripts/linux-gates.sh does).
docker_repo=$repo
if windows_repo=$(pwd -W 2>/dev/null); then
  docker_repo=$windows_repo
  export MSYS_NO_PATHCONV=1
fi
prefix=${HC_TESTS_VOLUME_PREFIX:-hc-tests}
case $prefix in
  *[!A-Za-z0-9_.-]* | [!A-Za-z0-9]*) echo "tests/run-all.sh: HC_TESTS_VOLUME_PREFIX must be a plain Docker volume name" >&2; exit 64 ;;
esac

# The quick subset as one container command (the repository is mounted at /src, read-only).
quick_cmd="docker run --rm --volume \"$docker_repo:/src:ro\" --volume $prefix-cargo:/usr/local/cargo/registry --volume $prefix-target:/target --env CARGO_TARGET_DIR=/target --workdir /src $IMAGE sh -c 'cargo run -q --locked -p consensus-tests --example evaluate && cargo run -q --locked -p consensus-api -- verify $QUICK_LOG'"

passed=""
total_start=$(date +%s)

step() {
  # step <name> <command>: print it; run it unless --dry-run; stop at the first failure.
  name=$1
  cmd=$2
  echo "== $name"
  echo "\$ $cmd"
  if [ $dry = yes ]; then
    passed="$passed $name"
    return 0
  fi
  start=$(date +%s)
  if sh -c "$cmd"; then
    passed="$passed $name($(($(date +%s) - start))s)"
  else
    rc=$?
    echo "FAIL $name (exit $rc) after $(($(date +%s) - start))s" >&2
    echo "== summary: passed:${passed:- none}; failed: $name" >&2
    exit "$rc"
  fi
}

if [ $want_quick = yes ]; then
  step quick "$quick_cmd"
fi
if [ $want_rust = yes ]; then
  step rust "sh scripts/linux-gates.sh"
fi
if [ $want_web = yes ]; then
  step web-install "sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile"
  step web-check "sh apps/web/scripts/in-docker.sh pnpm check"
fi
if [ $want_e2e = yes ]; then
  step e2e "sh apps/web/scripts/playwright-docker.sh"
fi

if [ $dry = yes ]; then
  echo "== dry run: nothing was run; would run:${passed:- nothing}"
else
  echo "== summary: passed:${passed:- none} (total $(($(date +%s) - total_start))s)"
fi
