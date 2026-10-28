#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# SSE through the tunnel: follows one run's event stream with curl -N for a
# fixed time, timestamps every line, counts the heartbeat comments, measures
# the longest silence and remembers the last event id (input of the
# cloudflared-restart resume test). Runs on the OWNER MACHINE (the server
# cannot reach its own hostname, runbook 2.4). Runbook section 8.5.
#
#   deploy/scripts/sse-heartbeat-test.sh --hostname <hostname> --run <run-id> \
#       --cookie-jar <file> [--seconds 480] [--resume <last-event-id>] [--log <file>]
#
# The cookie jar must hold the session cookie of the session that owns the
# run (runbook 8.2 shows how to mint one with curl). A stream stays open only
# while its run deliberates and ends with `stream_end` once the run awaits a
# decision or is terminal, so the 8-minute horizon needs a live run on
# staging; guided runs finish in seconds. Staging behind Cloudflare Access:
# export CF_ACCESS_CLIENT_ID and CF_ACCESS_CLIENT_SECRET (runbook 7.9).
# PASS: no silence longer than 2 x the heartbeat interval + 5 s (15 s -> 35 s)
# and the stream either stayed open for --seconds or ended with stream_end.
set -euo pipefail

HOSTNAME_=""; RUN=""; JAR=""; DURATION=480; RESUME=""; LOG=""; HEARTBEAT=15
while [ $# -gt 0 ]; do
  case "$1" in
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --run) RUN="$2"; shift 2 ;;
    --cookie-jar) JAR="$2"; shift 2 ;;
    --seconds) DURATION="$2"; shift 2 ;;
    --resume) RESUME="$2"; shift 2 ;;
    --heartbeat) HEARTBEAT="$2"; shift 2 ;;
    --log) LOG="$2"; shift 2 ;;
    -h|--help) sed -n '3,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
if [ -z "$HOSTNAME_" ] || [ -z "$RUN" ] || [ -z "$JAR" ]; then
  echo "--hostname, --run and --cookie-jar are required" >&2; exit 64
fi
[ -f "$JAR" ] || { echo "cookie jar not found: $JAR" >&2; exit 1; }
LOG="${LOG:-sse-$(date -u +%Y%m%dT%H%M%SZ).log}"
STATE="$LOG.state"
max_gap_allowed=$((HEARTBEAT * 2 + 5))

extra_headers=()
if [ -n "$RESUME" ]; then extra_headers+=(-H "Last-Event-ID: $RESUME"); fi
if [ -n "${CF_ACCESS_CLIENT_ID:-}" ] && [ -n "${CF_ACCESS_CLIENT_SECRET:-}" ]; then
  extra_headers+=(-H "CF-Access-Client-Id: $CF_ACCESS_CLIENT_ID" -H "CF-Access-Client-Secret: $CF_ACCESS_CLIENT_SECRET")
fi

start=$(date +%s)
echo "streaming https://$HOSTNAME_/api/v1/runs/$RUN/events for up to $DURATION s (log: $LOG)"
: >"$LOG"
printf '0 0 0 0 -\n' >"$STATE"

# curl -N: no output buffering, so each SSE line arrives as the origin sends
# it; --max-time bounds the whole test (curl exit 28 is the expected end when
# the run outlives the window). The counters are kept in a state file because
# the while loop runs in a subshell.
set +e
curl -sS -N --max-time "$DURATION" -b "$JAR" "${extra_headers[@]}" \
     -H 'Accept: text/event-stream' "https://$HOSTNAME_/api/v1/runs/$RUN/events" 2>>"$LOG" |
{
  last=$start; heartbeats=0; events=0; max_gap=0; ended=0; last_id="-"
  while IFS= read -r line; do
    now=$(date +%s)
    gap=$((now - last)); last=$now
    if [ "$gap" -gt "$max_gap" ]; then max_gap=$gap; fi
    printf '%s %s\n' "$(date -u +%FT%TZ)" "$line" >>"$LOG"
    case "$line" in
      ": heartbeat") heartbeats=$((heartbeats + 1)) ;;
      "id: "*) last_id="${line#id: }" ;;
      "event: consensus"|"event: resync") events=$((events + 1)) ;;
      "event: stream_end") ended=1 ;;
    esac
    printf '%s %s %s %s %s\n' "$heartbeats" "$events" "$max_gap" "$ended" "$last_id" >"$STATE"
  done
}
curl_status=${PIPESTATUS[0]}
set -e

elapsed=$(( $(date +%s) - start ))
read -r heartbeats events max_gap ended last_id <"$STATE"
rm -f "$STATE"
echo "elapsed ${elapsed}s, heartbeats $heartbeats, event frames $events, longest silence ${max_gap}s," \
     "stream_end=$ended, last id $last_id, curl exit $curl_status"

verdict=PASS
if [ "$heartbeats" -eq 0 ] && [ "$events" -eq 0 ]; then
  verdict=FAIL; echo "no SSE frame at all: wrong run id or session cookie, the stream already ended, or an Access/WAF answer (see $LOG)"
fi
if [ "$max_gap" -gt "$max_gap_allowed" ]; then
  verdict=FAIL; echo "a silence of ${max_gap}s exceeds the allowed ${max_gap_allowed}s"
fi
if [ "$ended" -eq 0 ] && [ "$elapsed" -lt "$DURATION" ]; then
  verdict=FAIL; echo "the stream closed after ${elapsed}s without stream_end (curl exit $curl_status)"
fi
echo "SSE heartbeat test: $verdict"
if [ "$last_id" != "-" ]; then
  echo "resume test: restart cloudflared on the server (ssh <server> sudo systemctl restart cloudflared) while a stream is open," \
       "then rerun with --resume $last_id and expect the replay to continue from sequence $((last_id + 1)) (delivery: replay, then live)"
fi
[ "$verdict" = PASS ]
