#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Self-test of restart-when-idle.sh (runbook 9.4), run by scripts/linux-gates.sh
# as the gate restart-selftest. Offline: `curl`, `systemctl`, `id` and
# `journalctl` are replaced on PATH by stubs, and the ledger command by
# HC_LEDGER_SH; restart-when-idle.sh runs unchanged against them.
#
# The curl stub answers /ready from a script of bodies, one per call (the
# last one repeats); the systemctl stub records each call and the curl call
# it came after, so a case can check that the restart came only after the
# idle answer.
#
# Cases:
#   - active_runs 1, then 0: restarts once, after the second poll; exit 0
#   - queued_runs 1, then idle: restarts only after the idle answer
#   - /ready unreadable, then idle: unreadable counts as busy
#   - always busy within --max-wait: never restarts; exit 2
#   - idle, restarted, the ledger shows an open reservation: exit 3, its id printed
#   - idle, restarted, /ready never ready again: exit 4
#
# Usage: bash deploy/scripts/restart-when-idle-selftest.sh   (exit 0 when every case holds)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

cat >"$work/bin/curl" <<'STUB'
#!/usr/bin/env bash
set -eu
n=$(( $(cat "$STATE/curl.count" 2>/dev/null || echo 0) + 1 ))
echo "$n" >"$STATE/curl.count"
total=$(wc -l <"$STATE/bodies")
line=$n; [ "$line" -le "$total" ] || line=$total
body="$(sed -n "${line}p" "$STATE/bodies")"
if [ "$body" = "UNREADABLE" ]; then
  echo "curl: (7) Couldn't connect to server" >&2
  exit 7
fi
printf '%s' "$body"
STUB

cat >"$work/bin/systemctl" <<'STUB'
#!/usr/bin/env bash
set -eu
echo "$* after-curl=$(cat "$STATE/curl.count" 2>/dev/null || echo 0)" >>"$STATE/systemctl.log"
STUB

cat >"$work/bin/id" <<'STUB'
#!/usr/bin/env bash
echo 0
STUB

cat >"$work/bin/journalctl" <<'STUB'
#!/usr/bin/env bash
echo "[startup] limits: model timeout 240 s, run deadline 1200 s"
STUB

cat >"$work/ledger" <<'STUB'
#!/usr/bin/env bash
cat "$STATE/ledger.json"
STUB
chmod +x "$work/bin/"* "$work/ledger"

busy_active='{"ready":true,"active_runs":1,"queued_runs":0,"shutting_down":false}'
busy_queued='{"ready":true,"active_runs":0,"queued_runs":1,"shutting_down":false}'
idle='{"ready":true,"active_runs":0,"queued_runs":0,"shutting_down":false}'
not_ready='{"ready":false,"active_runs":0,"queued_runs":0,"shutting_down":false}'
no_open='{
  "ledger_dir": "/var/lib/consensus/ledger/public",
  "open_reservations": []
}'
one_open='{
  "ledger_dir": "/var/lib/consensus/ledger/public",
  "open_reservations": ["5f0c8a52-1d3e-4c43-9a59-0d6f3b1c2e7a"]
}'

failures=0
# run_case <name> <expected exit> <expected systemctl log, or 'none'> <ledger json> <max-wait> <bodies...>
run_case() {
  local name="$1" want="$2" restarts="$3" ledger="$4" max_wait="$5" rc=0 out got
  shift 5
  rm -rf "$work/state"; mkdir -p "$work/state"
  printf '%s\n' "$@" >"$work/state/bodies"
  printf '%s\n' "$ledger" >"$work/state/ledger.json"
  out="$(PATH="$work/bin:$PATH" STATE="$work/state" HC_LEDGER_SH="$work/ledger" \
           bash "$here/restart-when-idle.sh" --host consensus.example --interval 0 \
           --max-wait "$max_wait" --ready-wait 2 2>&1)" || rc=$?
  got="$(cat "$work/state/systemctl.log" 2>/dev/null || echo none)"
  if [ "$rc" -ne "$want" ]; then
    echo "selftest FAIL $name: exit $rc, expected $want"; printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1)); return 0
  fi
  if [ "$got" != "$restarts" ]; then
    echo "selftest FAIL $name: systemctl calls '$got', expected '$restarts'"; printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1)); return 0
  fi
  if [ "$want" -eq 3 ] && ! printf '%s\n' "$out" | grep -qF "5f0c8a52-1d3e-4c43-9a59-0d6f3b1c2e7a"; then
    echo "selftest FAIL $name: the open reservation id is not printed"; printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1)); return 0
  fi
  echo "selftest ok   $name (exit $rc)"
}

run_case "active 1, then 0: restart after the idle answer" 0 \
  "restart consensus-public after-curl=2" "$no_open" 60 "$busy_active" "$idle"
run_case "queued 1, then idle: restart only after it" 0 \
  "restart consensus-public after-curl=3" "$no_open" 60 "$busy_queued" "$busy_queued" "$idle"
run_case "unreadable /ready counts as busy" 0 \
  "restart consensus-public after-curl=2" "$no_open" 60 "UNREADABLE" "$idle"
run_case "never idle within --max-wait: not restarted" 2 \
  "none" "$no_open" 0 "$busy_active"
run_case "an open reservation after the restart" 3 \
  "restart consensus-public after-curl=1" "$one_open" 60 "$idle"
run_case "not ready again after the restart" 4 \
  "restart consensus-public after-curl=1" "$no_open" 60 "$idle" "$not_ready"

if [ "$failures" -ne 0 ]; then
  echo "selftest: $failures case(s) FAILED"
  exit 1
fi
echo "selftest: every case passed"
