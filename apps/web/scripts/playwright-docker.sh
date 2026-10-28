#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Runs the Playwright end-to-end suite (with axe) for apps/web inside a
# throwaway container of Microsoft's Playwright image, pinned by digest to the
# release that matches the pinned @playwright/test. Like in-docker.sh, the
# container sees apps/web mounted at /app and the API contract (the
# repository's contracts/ directory, whose fixtures the specs serve as the
# mocked /api/v1) read-only at /contracts and the research archive (research/,
# whose verbatim files the specs serve as the mocked /research) read-only at
# /research, and nothing else: no -e variables,
# no host home directory, no credentials, no TTY. It also gets no network at
# all (--network none): the tests serve the static export on the container's
# own loopback, and nothing they load may come from anywhere else.
#
# The container is further confined (red-team finding ABUSE-3: code running
# in it must not be able to rewrite the scripts the host runs):
#   - /app is mounted read-only; only test-results/ is writable, where
#     Playwright puts its report, traces and the axe results. Playwright's
#     own transform cache goes to the container's /tmp.
#   - it runs as the image's unprivileged user (pwuser, uid 1001), so the
#     files it writes to test-results/ are not root-owned on Linux and macOS.
#     test-results/ is created here, world-writable, and the container writes
#     with umask 0, so the host user can read and delete everything in it
#     whatever uid the container used.
#   - every capability is dropped and no process can gain privileges
#     (--cap-drop ALL --security-opt no-new-privileges). Playwright launches
#     Chromium with its sandbox off by default, so none is needed.
#
# A test-results/ left behind by the earlier version of this script (which
# ran as root) is not writable by pwuser: the container then stops with a
# message, and deleting apps/web/test-results once fixes it. On Docker
# Desktop for Windows the bind mount keeps such container-created ownership
# in NTFS metadata, so the same applies there.
#
# Build the export first (through in-docker.sh), then run the suite:
#   sh apps/web/scripts/in-docker.sh pnpm build
#   sh apps/web/scripts/playwright-docker.sh
#   sh apps/web/scripts/playwright-docker.sh --project chromium e2e/a11y.spec.ts
#
# Extra arguments go to `playwright test`. Results land in
# apps/web/test-results/ (git-ignored).

set -eu

# mcr.microsoft.com/playwright:v1.64.1-noble, pinned by digest (pulled
# 2026-10-27; Node 24.22.1, Chromium, Firefox and WebKit builds of
# Playwright 1.64.1). Keep it in step with @playwright/test in package.json.
IMAGE="mcr.microsoft.com/playwright@sha256:47b510dd9230bf0b9bfc9b20102241e577ea7a99334ab4c3ca19f19770e9b8ea"

app_dir=$(cd "$(dirname "$0")/.." && pwd)
contract_dir=$(cd "$app_dir/../../contracts" && pwd)
research_dir=$(cd "$app_dir/../../research" && pwd)

if [ ! -f "$app_dir/out/csp-manifest.json" ]; then
  echo "playwright-docker: no out/csp-manifest.json; run 'sh apps/web/scripts/in-docker.sh pnpm build' first" >&2
  exit 66
fi

# The one writable directory; created here, world-writable, so the bind mount
# never has to create it and pwuser can write to it (chmod is a no-op on a
# Windows file system, where host-created entries are world-writable anyway).
mkdir -p "$app_dir/test-results"
chmod a+rwx "$app_dir/test-results" 2>/dev/null || true

# Git Bash: give Docker the Windows form of the paths, and stop MSYS from
# rewriting /app and the other container paths in the arguments.
if windows_dir=$(cd "$app_dir" && pwd -W 2>/dev/null); then
  app_dir=$windows_dir
  contract_dir=$(cd "$contract_dir" && pwd -W)
  research_dir=$(cd "$research_dir" && pwd -W)
  export MSYS_NO_PATHCONV=1
fi

# --init reaps the browser processes; --shm-size gives Chromium the shared
# memory it needs without sharing the host's IPC namespace.
exec docker run --rm \
  --network none \
  --init \
  --shm-size 1g \
  --user pwuser \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --volume "$app_dir:/app:ro" \
  --volume "$app_dir/test-results:/app/test-results" \
  --volume "$contract_dir:/contracts:ro" \
  --volume "$research_dir:/research:ro" \
  --workdir /app \
  "$IMAGE" \
  sh -c '[ -w /app/test-results ] || { echo "playwright-docker: test-results/ is not writable by pwuser (left by an earlier run as root?); delete apps/web/test-results and retry" >&2; exit 73; }; umask 0 && export CI=1 && exec node node_modules/@playwright/test/cli.js test "$@"' \
  sh "$@"
