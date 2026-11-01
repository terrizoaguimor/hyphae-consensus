#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Runs the Rust gates inside an Ubuntu 24.04 container, the deployment
# target (glibc 2.39), with the repository mounted READ-ONLY, and exits
# non-zero when any gate fails: fmt, clippy -D warnings, the workspace
# tests, the demo crate's tests, the conformance example (evaluate) and
# the sealed-snapshot tooling self-tests. Every gate runs even after one
# has failed; each prints its summary lines (a failing one also the last
# 60 lines of its log), and the run ends with the list of failed gates.
# cargo-deny is not run here (it needs the advisory database): CI runs it
# (.github/workflows/ci.yml), and so does `cargo deny check` on the host.
#
# Nothing from the host environment is passed in: no Meta key, no Hyphae
# paths. The image is pinned by digest; the toolchain (TOOLCHAIN, as in
# rust-toolchain.toml) is installed by rustup, which checks the published
# checksums of what it downloads, into named Docker volumes next to the
# registry, so a second run is fast. The build-output volume is per
# checkout path, because every checkout is mounted at /src and cargo's
# freshness check is mtime-based, so two worktrees sharing one target
# volume reuse each other's stale artifacts (seen 2026-10-28: a lane's
# older consensus-runtime rlib was reused for the main checkout and the
# api crate failed to compile).
#
# Usage, from the repository root (Git Bash, Linux or macOS):
#   sh scripts/linux-gates.sh             # the gates
#   sh scripts/linux-gates.sh --release   # the gates, then, only when all of
#       them pass and the checkout has no uncommitted change,
#       `cargo build --release --locked -p consensus-api` in the same
#       container and volumes: dist/consensus-api, dist/consensus-api.sha256
#       (checked after the copy) and dist/consensus-api.commit (runbook 3.1)
#
# First verified 2026-10-27 on feat/public-portal (269 tests passed,
# 2 ignored live tests not run, evaluate 10/10).

set -eu

IMAGE="ubuntu:24.04@sha256:24737cef17f4e22b1977df7d9a05e77c4e28b7da5b8911f8d8ccda136bf3573c"
TOOLCHAIN="1.99.1"   # must match rust-toolchain.toml

RELEASE=no
case "${1:-}" in
  "") ;;
  --release) RELEASE=yes ;;
  *) echo "usage: sh scripts/linux-gates.sh [--release]" >&2; exit 64 ;;
esac

repo=$(cd "$(dirname "$0")/.." && pwd)
host_repo=$repo
# Git Bash: give Docker the Windows form of the path and stop MSYS from
# rewriting the container paths in the arguments.
if windows_repo=$(cd "$repo" && pwd -W 2>/dev/null); then
  repo=$windows_repo
  export MSYS_NO_PATHCONV=1
fi

# One build-output volume per checkout (see the header).
target_key=$(printf %s "$repo" | sha256sum | cut -c1-12)

# The release build's output directory, mounted only with --release.
set --
if [ "$RELEASE" = yes ]; then
  if [ -n "$(cd "$host_repo" && git status --porcelain --untracked-files=no)" ]; then
    echo "--release builds a clean checkout only: commit the changes first" >&2
    exit 65
  fi
  mkdir -p "$host_repo/dist"
  rm -f "$host_repo/dist/consensus-api" "$host_repo/dist/consensus-api.sha256" "$host_repo/dist/consensus-api.commit"
  set -- --volume "$repo/dist:/out"
fi

status=0
docker run --rm "$@" \
  --volume "$repo:/src:ro" \
  --volume hc-linux-cargo:/cargo \
  --volume hc-linux-rustup:/rustup \
  --volume "hc-linux-target-$target_key:/target" \
  --env CARGO_HOME=/cargo \
  --env RUSTUP_HOME=/rustup \
  --env CARGO_TARGET_DIR=/target \
  --env DEBIAN_FRONTEND=noninteractive \
  --env PATH=/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  --workdir /src \
  "$IMAGE" \
  bash -c "
    set -u -o pipefail
    . /etc/os-release
    echo \"== \$PRETTY_NAME, \$(ldd --version | head -1)\"
    apt-get update -qq >/dev/null || { echo 'FAIL apt-get update'; exit 1; }
    apt-get install -y -qq --no-install-recommends curl ca-certificates build-essential pkg-config python3 >/dev/null \
      || { echo 'FAIL apt-get install'; exit 1; }
    if [ ! -x /cargo/bin/cargo ]; then
      curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain $TOOLCHAIN --profile default >/dev/null \
        || { echo 'FAIL rustup'; exit 1; }
    fi
    cargo --version
    failed=''
    gate() {
      # gate <name> <summary regex> <command...>: one gate, its output in
      # /tmp/<name>.log; prints the summary lines, on failure also the
      # log's last 60 lines; records the failure and lets the next gate run.
      name=\$1; summary=\$2; shift 2
      echo \"== \$name\"
      \"\$@\" >\"/tmp/\$name.log\" 2>&1
      rc=\$?
      grep -E \"\$summary\" \"/tmp/\$name.log\" || true
      if [ \$rc -eq 0 ]; then
        echo \"PASS \$name\"
      else
        tail -n 60 \"/tmp/\$name.log\"
        echo \"FAIL \$name (exit \$rc)\"
        failed=\"\$failed \$name\"
      fi
    }
    gate fmt 'Diff in' cargo fmt --all -- --check
    gate clippy '^(warning|error)|Finished' cargo clippy --workspace --all-targets --locked -- -D warnings
    gate test '^test result:|FAILED|panicked at' cargo test --workspace --locked
    gate demo-crate '^test result:|FAILED|panicked at' cargo test --manifest-path demo-repo/payment-client/Cargo.toml --locked
    gate evaluate 'author-defined outcome|Unauthorized executions' cargo run -q -p consensus-tests --example evaluate
    gate manifest-selftest '^selftest' python3 -I scripts/snapshot_manifest.py selftest
    gate capture-selftest '^selftest' sh scripts/hyphae-container/capture-snapshot.sh --selftest
    gate edge-selftest '^selftest' bash deploy/scripts/verify-edge-selftest.sh
    if [ -n \"\$failed\" ]; then
      echo \"== FAILED:\$failed\"
      exit 1
    fi
    echo '== DONE: every gate passed'
    if [ $RELEASE = yes ]; then
      echo '== release build'
      cargo build --release --locked -p consensus-api >/tmp/release.log 2>&1 \
        || { tail -n 60 /tmp/release.log; echo 'FAIL release build'; exit 1; }
      cp /target/release/consensus-api /out/consensus-api
      cd /out && sha256sum consensus-api > consensus-api.sha256 && cat consensus-api.sha256
    fi
  " || status=$?

if [ "$RELEASE" = yes ] && [ "$status" -eq 0 ]; then
  (cd "$host_repo/dist" && sha256sum -c consensus-api.sha256) || { echo "the copied binary does not match the digest computed in the container" >&2; exit 1; }
  (cd "$host_repo" && git rev-parse HEAD) > "$host_repo/dist/consensus-api.commit"
  echo "== release: dist/consensus-api built from commit $(cat "$host_repo/dist/consensus-api.commit")"
fi
exit "$status"
