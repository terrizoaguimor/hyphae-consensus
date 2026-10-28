#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Runs one command for apps/web inside a throwaway Node container. This is
# the only supported way to run the Node toolchain here (see README.md): the
# host environment holds the owner's Meta key, and dependency code runs with
# the caller's full environment during install, build, lint and test.
#
# What reaches the container: apps/web mounted at /app, nothing else. No -e
# variables, no host home directory, no credentials, no TTY (so pnpm runs
# non-interactively and never edits pnpm-workspace.yaml on its own). Inside
# the container, Next.js telemetry is switched off before the command runs.
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

set -eu

# node:24-bookworm-slim, pinned by digest (pulled 2026-10-27, Node 24.22.1).
IMAGE="node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205"

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <command> [args...]" >&2
  exit 64
fi

app_dir=$(cd "$(dirname "$0")/.." && pwd)

# Git Bash: give Docker the Windows form of the path, and stop MSYS from
# rewriting /app and the other container paths in the arguments.
if windows_dir=$(cd "$app_dir" && pwd -W 2>/dev/null); then
  app_dir=$windows_dir
  export MSYS_NO_PATHCONV=1
fi

exec docker run --rm \
  --volume "$app_dir:/app" \
  --volume "$app_dir/scripts:/app/scripts:ro" \
  --volume "$app_dir/pnpm-workspace.yaml:/app/pnpm-workspace.yaml:ro" \
  --workdir /app \
  "$IMAGE" \
  sh -c 'corepack enable pnpm && export COREPACK_ENABLE_DOWNLOAD_PROMPT=0 NEXT_TELEMETRY_DISABLED=1 && exec "$@"' \
  sh "$@"
