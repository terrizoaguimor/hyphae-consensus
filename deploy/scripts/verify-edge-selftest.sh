#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Self-test of verify-edge.sh (gate G6), run by scripts/linux-gates.sh as the
# gate edge-selftest. No network: `ssh` (the origin half) and `curl` (the
# edge half) are replaced on PATH by stubs that answer from fixture files,
# and verify-edge.sh runs unchanged against them.
#
# Cases:
#   - identical bodies: PASS
#   - /api/v1/capabilities differing only in live.budget.daily_available_now
#     (the budget refills by the second, so two reads never agree while live
#     mode is on): PASS, with the field named in the NOTE column
#   - /api/v1/capabilities differing in any other field: FAIL
#   - an HTML page differing in a daily_available_now-shaped string: FAIL
#     (only /api bodies are masked)
#   - an edge status different from the origin's: FAIL
#
# Usage: bash deploy/scripts/verify-edge-selftest.sh   (exit 0 when every case holds)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

# ssh <host> <command>: the command ends with 'http://localhost<path>'; print
# the origin fixture for that path, then the status (three bytes), as curl
# -o - -w '%{http_code}' would.
cat >"$work/bin/ssh" <<'STUB'
#!/usr/bin/env bash
set -eu
cmd="${*: -1}"
path="${cmd##*http://localhost}"; path="${path%\'}"
safe="$(printf '%s' "$path" | tr '/' '_')"
cat "$FIXTURES/origin$safe"
printf '%s' "$(cat "$FIXTURES/origin$safe.code" 2>/dev/null || echo 200)"
STUB

# curl -sS -o <body> -D <headers> -w '%{http_code}' https://<host><path>
cat >"$work/bin/curl" <<'STUB'
#!/usr/bin/env bash
set -eu
out=""; hdr=""; url=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -D) hdr="$2"; shift 2 ;;
    -w|-H) shift 2 ;;
    -*) shift ;;
    *) url="$1"; shift ;;
  esac
done
path="/${url#https://*/}"
safe="$(printf '%s' "$path" | tr '/' '_')"
cp "$FIXTURES/edge$safe" "$out"
printf 'HTTP/2 200\r\ncf-cache-status: DYNAMIC\r\ncontent-security-policy: default-src %s\r\n\r\n' "'none'" >"$hdr"
printf '%s' "$(cat "$FIXTURES/edge$safe.code" 2>/dev/null || echo 200)"
STUB
chmod +x "$work/bin/ssh" "$work/bin/curl"

caps() { # caps <daily_available_now> <live_enabled>
  printf '{"surface":"public","live_enabled":%s,"live":{"budget":{"daily_cap":"10.000000","daily_available_now":"%s","daily_remaining":"10.000000"}}}' "$2" "$1"
}

failures=0
# run_case <name> <expected exit> <expected text in the output, or ''>
run_case() {
  local name="$1" want="$2" text="$3" rc=0 out
  out="$(PATH="$work/bin:$PATH" FIXTURES="$work/fx" bash "$here/verify-edge.sh" \
           --hostname consensus.example --ssh stub-server --path / --path /api/v1/capabilities 2>&1)" || rc=$?
  if [ "$rc" -ne "$want" ]; then
    echo "selftest FAIL $name: exit $rc, expected $want"; printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1)); return 0
  fi
  if [ -n "$text" ] && ! printf '%s\n' "$out" | grep -qF -- "$text"; then
    echo "selftest FAIL $name: output lacks '$text'"; printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1)); return 0
  fi
  echo "selftest ok   $name (exit $rc)"
}

fixtures() { # fixtures <origin caps> <edge caps> <origin html> <edge html>
  rm -rf "$work/fx"; mkdir -p "$work/fx"
  printf '%s' "$1" >"$work/fx/origin_api_v1_capabilities"
  printf '%s' "$2" >"$work/fx/edge_api_v1_capabilities"
  printf '%s' "$3" >"$work/fx/origin_"
  printf '%s' "$4" >"$work/fx/edge_"
}

html='<!doctype html><title>Hyphae Consensus</title>'

fixtures "$(caps 9.087615 true)" "$(caps 9.087615 true)" "$html" "$html"
run_case "identical bodies" 0 "G6 edge-vs-origin: PASS"

fixtures "$(caps 9.087615 true)" "$(caps 9.087731 true)" "$html" "$html"
run_case "capabilities: only daily_available_now differs (refills by the second)" 0 "daily_available_now"

fixtures "$(caps 9.087615 true)" "$(caps 9.087731 false)" "$html" "$html"
run_case "capabilities: another field differs too" 1 "DIFFER"

fixtures "$(caps 9.087615 true)" "$(caps 9.087615 true)" \
  "$html\"daily_available_now\":\"1.0\"" "$html\"daily_available_now\":\"2.0\""
run_case "HTML page: not masked" 1 "DIFFER"

fixtures "$(caps 9.087615 true)" "$(caps 9.087615 true)" "$html" "$html"
printf '404' >"$work/fx/edge_.code"
run_case "status differs" 1 "status differs"

if [ "$failures" -ne 0 ]; then
  echo "selftest: $failures case(s) FAILED"
  exit 1
fi
echo "selftest: every case passed"
