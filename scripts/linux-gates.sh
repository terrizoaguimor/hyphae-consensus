#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Runs every Rust gate inside an Ubuntu 24.04 container, the deployment
# target (glibc 2.39), with the repository mounted READ-ONLY. Nothing from
# the host environment is passed in: no Meta key, no Hyphae paths. The
# toolchain, registry and build outputs live in named Docker volumes so a
# second run is fast.
#
# Usage, from the repository root (Git Bash, Linux or macOS):
#   sh scripts/linux-gates.sh                 # fmt, clippy, tests, evaluate
#
# First verified 2026-10-27 on feat/public-portal (269 tests passed,
# 2 ignored live tests not run, evaluate 10/10).

set -eu

IMAGE="ubuntu:24.04"
TOOLCHAIN="1.99.1"   # must match rust-toolchain.toml

repo=$(cd "$(dirname "$0")/.." && pwd)
# Git Bash: give Docker the Windows form of the path and stop MSYS from
# rewriting the container paths in the arguments.
if windows_repo=$(cd "$repo" && pwd -W 2>/dev/null); then
  repo=$windows_repo
  export MSYS_NO_PATHCONV=1
fi

exec docker run --rm \
  --volume "$repo:/src:ro" \
  --volume hc-linux-cargo:/cargo \
  --volume hc-linux-rustup:/rustup \
  --volume hc-linux-target:/target \
  --env CARGO_HOME=/cargo \
  --env RUSTUP_HOME=/rustup \
  --env CARGO_TARGET_DIR=/target \
  --env DEBIAN_FRONTEND=noninteractive \
  --env PATH=/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  --workdir /src \
  "$IMAGE" \
  bash -c "
    set -e
    . /etc/os-release
    echo \"== \$PRETTY_NAME, \$(ldd --version | head -1)\"
    apt-get update -qq >/dev/null
    apt-get install -y -qq --no-install-recommends curl ca-certificates build-essential pkg-config >/dev/null
    if [ ! -x /cargo/bin/cargo ]; then
      curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain $TOOLCHAIN --profile default >/dev/null
    fi
    cargo --version
    echo '== fmt';     cargo fmt --all -- --check && echo FMT_OK
    echo '== clippy';  cargo clippy --workspace --all-targets --locked -- -D warnings 2>&1 | tail -1
    echo '== test';    cargo test --workspace --locked 2>&1 | grep -E '^test result|FAILED|panicked at'
    echo '== evaluate'; cargo run -q -p consensus-tests --example evaluate 2>&1 | tail -2
    echo '== DONE'
  "
