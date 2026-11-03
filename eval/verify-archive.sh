#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Re-verifies, offline, every file that research/MANIFEST.json lists:
#   - each event log (kind event_log) with `consensus-api verify`: schema
#     versions, gap-free sequences, unique ids, the hash chain, every run
#     replayed through the state machine and its recommendation re-derived
#     under the policy; the run and record counts are compared with the manifest;
#   - each single-agent baseline log (kind baseline_log) with
#     `consensus-api baseline verify`;
#   - every file, logs and run records alike, against the size and the SHA-256
#     the manifest pins.
# It prints one line per log (labels of its runs, path, runs, records, chain
# scopes, digest) and ends with "N logs verified, M failed". The exit status is 0
# only when nothing failed. Nothing is written to research/ or anywhere else:
# the commands are read-only, and the container has no network.
#
# Usage, from any directory (Git Bash on Windows, Linux or macOS):
#   docker build -t hyphae-consensus .      # once (the Dockerfile at the repository root)
#   sh eval/verify-archive.sh               # verifies the research/ copied into the image
#   sh eval/verify-archive.sh --research DIR  # the same over another research directory (mounted read-only),
#       # e.g. --research research for this checkout's, or a copy you altered, to see a failure
#   sh eval/verify-archive.sh --image NAME  # another image name (default hyphae-consensus, or $HC_IMAGE)
#   CONSENSUS_API=/path/to/consensus-api sh eval/verify-archive.sh
#       # no Docker: a locally built binary (cargo build --release -p consensus-api) over the checkout's research/
#       # (or over --research DIR)
#       # (needs sh, awk, sha256sum, wc, mktemp, grep and sed)
#   sh eval/verify-archive.sh --help
#
# The counts: the portal's "53 recorded runs, 1,103 records" are the 17 consensus
# event logs. The 8 single-agent baseline logs (8 runs, 168 records) are verified
# here too and counted apart; the 46 + 8 run records are JSON files, not event
# logs, so only their size and SHA-256 are checked. See eval/README.md.

set -eu

usage() {
  sed -n '/^# Usage, from any directory/,/^#   sh eval\/verify-archive.sh --help/p' "$0" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------- inner part
# Runs where the binary and research/ are (inside the container, or on the host
# with CONSENSUS_API set). Not meant to be called by hand: VA_BIN and VA_RESEARCH
# name the binary and the archive.
if [ "${1:-}" = "--inner" ]; then
  bin=${VA_BIN:?}
  research=${VA_RESEARCH:?}
  manifest=$research/MANIFEST.json
  [ -f "$manifest" ] || { echo "no $manifest" >&2; exit 66; }
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT

  # The manifest is pretty-printed JSON, one key per line. Emit one line per
  # file: F|path|kind|sha256|bytes|runs|events|labels, where labels summarize
  # the run labels that the manifest's "runs" and "baseline_runs" sections
  # attach to that file.
  awk '
    function val(l,   v) { v = l; sub(/^[^:]*: */, "", v); sub(/,$/, "", v); gsub(/"/, "", v); return v }
    /^  "files": \[/ { sec = "files"; next }
    /^  "runs": \[/ { sec = "runs"; next }
    /^  "baseline_runs": \[/ { sec = "runs"; next }
    /^  \]/ { sec = ""; next }
    sec == "files" && /^      "path":/ { n++; path[n] = val($0) }
    sec == "files" && /^      "kind":/ { kind[n] = val($0) }
    sec == "files" && /^      "sha256":/ { sha[n] = val($0) }
    sec == "files" && /^      "bytes":/ { bytes[n] = val($0) }
    sec == "files" && /^      "runs":/ { runs[n] = val($0) }
    sec == "files" && /^      "events":/ { events[n] = val($0) }
    sec == "runs" && /^      "label":/ { lab = val($0) }
    sec == "runs" && /^      "file":/ { f = val($0); c[f]++; if (c[f] == 1) first[f] = lab; last[f] = lab }
    END {
      for (i = 1; i <= n; i++) {
        l = "-"
        if (path[i] in c) l = (c[path[i]] == 1) ? first[path[i]] : first[path[i]] ".." last[path[i]]
        printf "F|%s|%s|%s|%s|%s|%s|%s\n", path[i], kind[i], sha[i], bytes[i], (runs[i] == "" ? 0 : runs[i]), (events[i] == "" ? 0 : events[i]), l
      }
    }' "$manifest" > "$tmp/files"

  total=$(wc -l < "$tmp/files" | tr -d ' ')
  [ "$total" -gt 0 ] || { echo "the manifest lists no file" >&2; exit 65; }

  logs_ok=0; logs_fail=0; rec_ok=0; rec_fail=0; runs_sum=0; events_sum=0
  brun_sum=0; bev_sum=0; blogs_ok=0
  while IFS='|' read -r _ path kind sha bytes mruns mevents labels; do
    file=$research/$path
    problems=""
    nruns=0; nrec=0; scopes=""; detail=""
    if [ ! -f "$file" ]; then
      problems="missing file"
      digest="-"
    else
      digest=$(sha256sum "$file" | awk '{print $1}')
      size=$(wc -c < "$file" | tr -d ' ')
      [ "$digest" = "$sha" ] || problems="$problems; sha256 differs from the manifest"
      [ "$size" = "$bytes" ] || problems="$problems; size $size, manifest $bytes"
    fi
    case $kind in
      event_log)
        if [ -f "$file" ]; then
          "$bin" verify "$file" > "$tmp/out" 2>&1 && rc=0 || rc=$?
          nruns=$(grep -c '^ok  ' "$tmp/out" || true)
          nrec=$(grep '^ok  ' "$tmp/out" | sed -n 's/.* events=\([0-9][0-9]*\) .*/\1/p' | awk '{s += $1} END {print s + 0}')
          scopes=$(grep '^ok  ' "$tmp/out" | sed -n 's/.* chain=\([A-Za-z]*\) .*/\1/p' | sort -u | tr '\n' ',' | sed 's/,$//')
          [ "$rc" -eq 0 ] || problems="$problems; verify exit $rc: $(grep -m1 'FAIL' "$tmp/out" | cut -c1-160)"
          [ "$nruns" = "$mruns" ] || problems="$problems; $nruns runs, manifest $mruns"
          [ "$nrec" = "$mevents" ] || problems="$problems; $nrec records, manifest $mevents"
          [ "$scopes" != "Unchained" ] || scopes="Unchained (schema 1.0 predates the hash chain)"
          if [ -z "$problems" ]; then
            runs_sum=$((runs_sum + nruns)); events_sum=$((events_sum + nrec))
          fi
        fi
        detail="runs=$nruns records=$nrec chain=${scopes:--}"
        ;;
      baseline_log)
        if [ -f "$file" ]; then
          "$bin" baseline verify "$file" > "$tmp/out" 2> "$tmp/err" && rc=0 || rc=$?
          nrec=$(sed -n 's/^  "event_count": \([0-9][0-9]*\),*$/\1/p' "$tmp/out")
          nrec=${nrec:-0}
          [ "$rc" -eq 0 ] || problems="$problems; baseline verify exit $rc: $(head -n1 "$tmp/err" | cut -c1-160)"
          [ "$nrec" = "$mevents" ] || problems="$problems; $nrec records, manifest $mevents"
          if [ -z "$problems" ]; then
            brun_sum=$((brun_sum + 1)); bev_sum=$((bev_sum + nrec)); blogs_ok=$((blogs_ok + 1))
          fi
        fi
        detail="runs=1 records=$nrec chain=baseline"
        ;;
      run_record | baseline_record)
        if [ -z "$problems" ]; then rec_ok=$((rec_ok + 1)); else rec_fail=$((rec_fail + 1)); fi
        continue
        ;;
      *)
        problems="$problems; unknown kind $kind"
        detail=""
        ;;
    esac
    problems=${problems#; }
    if [ -z "$problems" ]; then
      logs_ok=$((logs_ok + 1))
      echo "ok    $labels  $path  $detail  sha256=match($(echo "$digest" | cut -c1-12))"
    else
      logs_fail=$((logs_fail + 1))
      echo "FAIL  $labels  $path  $detail  $problems"
    fi
  done < "$tmp/files"

  failed=$((logs_fail + rec_fail))
  echo "-- consensus logs verified: runs=$runs_sum records=$events_sum (consensus event logs)"
  echo "-- baseline logs verified: $blogs_ok runs=$brun_sum records=$bev_sum"
  echo "-- run records (JSON, not event logs): $rec_ok with size and sha256 matching the manifest, $rec_fail not"
  echo "$logs_ok logs verified, $failed failed"
  [ "$failed" -eq 0 ]
  exit $?
fi

# ---------------------------------------------------------------- outer part
image=${HC_IMAGE:-hyphae-consensus}
research_arg=""
while [ "$#" -gt 0 ]; do
  case $1 in
    --research) [ "$#" -ge 2 ] || { echo "verify-archive: --research needs a directory" >&2; exit 64; }; research_arg=$2; shift ;;
    --image) [ "$#" -ge 2 ] || { echo "verify-archive: --image needs a name" >&2; exit 64; }; image=$2; shift ;;
    -h | --help) usage; exit 0 ;;
    *) echo "verify-archive: unknown option $1" >&2; usage >&2; exit 64 ;;
  esac
  shift
done

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)

# A locally built binary: no Docker.
if [ -n "${CONSENSUS_API:-}" ]; then
  [ -x "$CONSENSUS_API" ] || { echo "verify-archive: CONSENSUS_API=$CONSENSUS_API is not an executable file" >&2; exit 66; }
  if [ -n "$research_arg" ]; then local_research=$(cd "$research_arg" && pwd); else local_research=$repo/research; fi
  VA_BIN=$CONSENSUS_API VA_RESEARCH=$local_research exec sh "$here/verify-archive.sh" --inner
fi

command -v docker > /dev/null 2>&1 || { echo "verify-archive: Docker is needed (or set CONSENSUS_API to a built binary)" >&2; exit 69; }
if ! docker image inspect "$image" > /dev/null 2>&1; then
  echo "verify-archive: no image named $image; build it first: docker build -t $image ." >&2
  exit 66
fi

# Git Bash: the Windows form of the paths for Docker, and no rewriting of container paths.
docker_here=$here
if win_here=$(cd "$here" && pwd -W 2>/dev/null); then
  docker_here=$win_here
  export MSYS_NO_PATHCONV=1
fi

set -- --rm --network none --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges   --volume "$docker_here/verify-archive.sh:/verify-archive.sh:ro" --env VA_BIN=consensus-api
if [ -n "$research_arg" ]; then
  [ -f "$research_arg/MANIFEST.json" ] || { echo "verify-archive: $research_arg has no MANIFEST.json" >&2; exit 66; }
  research_abs=$(cd "$research_arg" && pwd)
  if win_research=$(cd "$research_arg" && pwd -W 2>/dev/null); then research_abs=$win_research; fi
  set -- "$@" --volume "$research_abs:/research:ro" --env VA_RESEARCH=/research
else
  set -- "$@" --env VA_RESEARCH=/opt/hyphae/research
fi
exec docker run "$@" --entrypoint sh "$image" /verify-archive.sh --inner
