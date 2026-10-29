#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Runs one command for apps/web inside a throwaway Node container. This is
# the only supported way to run the Node toolchain here (see README.md): the
# host environment holds the owner's Meta key, and dependency code runs with
# the caller's full environment during install, build, lint and test.
#
# What reaches the container: apps/web mounted at /app, the API contract
# (the repository's contracts/ directory: JSON Schemas and fixtures the Rust
# server exports) mounted read-only at /contracts, so the type generator and
# the tests can read it, and the research archive (research/: the verbatim
# event logs and run records the showcase serves, public data) mounted
# read-only at /research, so the chain verifier's tests run over the real
# archived bytes. Nothing else. No -e variables, no host home
# directory, no credentials, no TTY (so pnpm runs non-interactively and never
# edits pnpm-workspace.yaml on its own). Inside the container, Next.js
# telemetry is switched off before the command runs.
#
# Two parts of the mount are read-only, so code running in the container can
# never rewrite what the host runs or trusts (red-team finding ABUSE-3):
#   - scripts/: this file and playwright-docker.sh run on the host, where the
#     Meta key is set; serve-static.mjs and csp-manifest.mjs run inside
#     containers and are read by the unit tests.
#   - pnpm-workspace.yaml: the supply-chain policy (release cooldown, denied
#     build scripts), which pnpm reads and must not loosen.
# The rest of /app stays writable because the toolchain needs it: pnpm
# install writes node_modules/ (with its store in node_modules/.pnpm-store),
# next typegen writes .next/types and next-env.d.ts, tsc writes
# tsconfig.tsbuildinfo, next build writes .next/ and out/, and csp-manifest.mjs
# writes out/csp-manifest.json. Nothing else in apps/web is written by a
# command of this project.
#
# Usage, from any directory (Git Bash, Linux or macOS):
#   sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
#   sh apps/web/scripts/in-docker.sh pnpm build
#   sh apps/web/scripts/in-docker.sh sh -c 'pnpm typecheck && pnpm lint'

#
# Optional: with HC_WEB_NODE_MODULES_VOLUME=<name> set on the host, a named
# Docker volume of that name is mounted at /app/node_modules instead of the
# bind-mounted directory (pnpm's store then lives in the volume too). On
# hosts where bind-mount writes are slow (Docker Desktop on a nested-virtual
# Windows VM: an install there took hours) this makes install, build and
# test fast. Use one volume per checkout and pass the same name to
# playwright-docker.sh; nothing else changes (the variable is read on the
# host and never passed into the container).

set -eu

# node:24-bookworm-slim, pinned by digest (pulled 2026-10-27, Node 24.22.1).
IMAGE="node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205"

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <command> [args...]" >&2
  exit 64
fi

app_dir=$(cd "$(dirname "$0")/.." && pwd)
contract_dir=$(cd "$app_dir/../../contracts" && pwd)
research_dir=$(cd "$app_dir/../../research" && pwd)

# Git Bash: give Docker the Windows form of the paths, and stop MSYS from
# rewriting /app and the other container paths in the arguments.
if windows_dir=$(cd "$app_dir" && pwd -W 2>/dev/null); then
  app_dir=$windows_dir
  contract_dir=$(cd "$contract_dir" && pwd -W)
  research_dir=$(cd "$research_dir" && pwd -W)
  export MSYS_NO_PATHCONV=1
fi

modules_volume=${HC_WEB_NODE_MODULES_VOLUME:-}
if [ -n "$modules_volume" ]; then
  case $modules_volume in
    *[!A-Za-z0-9_.-]* | [!A-Za-z0-9]*) echo "in-docker: HC_WEB_NODE_MODULES_VOLUME must be a plain Docker volume name" >&2; exit 64 ;;
  esac
  modules_mount="--volume=$modules_volume:/app/node_modules"
else
  modules_mount=""
fi

exec docker run --rm \
  --volume "$app_dir:/app" ${modules_mount:+"$modules_mount"} \
  --volume "$app_dir/scripts:/app/scripts:ro" \
  --volume "$app_dir/pnpm-workspace.yaml:/app/pnpm-workspace.yaml:ro" \
  --volume "$contract_dir:/contracts:ro" \
  --volume "$research_dir:/research:ro" \
  --workdir /app \
  "$IMAGE" \
  sh -c 'corepack enable pnpm && export COREPACK_ENABLE_DOWNLOAD_PROMPT=0 NEXT_TELEMETRY_DISABLED=1 && exec "$@"' \
  sh "$@"
