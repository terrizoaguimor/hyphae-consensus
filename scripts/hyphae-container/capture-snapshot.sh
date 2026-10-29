#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# capture-snapshot.sh: the one authorized capture session that produces the
# sealed public evidence snapshot (ADR-0008; docs/architecture/sealed-snapshot.md
# is the authoritative description, scripts/snapshot_manifest.py builds the
# manifest from this script's output on the host).
#
# Runs INSIDE the hyphae-consensus container like its sibling scripts: copy it
# to /hyphae-home and run it with docker exec. In order it
#   0. checks its preconditions and records the Hyphae version and help texts;
#   1. creates a NEW Agent Memory instance under its own HOME
#      ("hyphae agent setup --no-service"), never touching the demo store
#      (/hyphae-home) or the lab store (/hyphae-home/lab-injection); the
#      stdout of setup goes to the scratch directory that is removed at exit,
#      whatever happens, because whether it prints key material is unverified;
#   2. starts a daemon for that HOME only, socket OUTSIDE the data directory,
#      exactly as serve.sh does (the writer path used for seeding is the MCP
#      adapter, which is a client of the daemon, so the daemon must exist
#      before the seeding step);
#   3. seeds exactly the 7 synthetic demo-repo facts, the texts copied verbatim
#      from seed.sh and seed-demo-facts.sh and cross-checked against those
#      files; nothing is ever forgotten in this store;
#   4. captures a proven list (demo-repo, limit 64) and a proven empty recall
#      (other-repo) through "hyphae agent ui", keeping each exact response and
#      a verbatim copy of the proof and the witness it names (both must lie
#      under the new HOME), and records the sha256 of the response, the proof
#      copy and the witness copy the moment they are written: these digests
#      are what binds each file to its capture once the directory leaves the
#      container (captures.index, step 8);
#   5. verifies every capture with "hyphae proof verify" against the one
#      shared anchor, keeps the verifier report and records its sha256;
#   6. records "hyphae --version" and the sha256 of the hyphae binary;
#   7. stops the daemon, destroys the new instance's credentials and makes its
#      data directory read-only, printing exactly what was removed;
#   8. writes captures.index and session.json for the host-side manifest builder.
# Every command that changes anything is printed as "+ <command>" before it
# runs; the whole session, prints included, is logged with UTC timestamps to
# <out>/capture.log. The script never deletes a proof, a witness, a store or a
# backup, and it never prints or reads the content of a credential file.
#
# Usage (inside the container):
#   sh /hyphae-home/capture-snapshot.sh [--dry-run] [--ui-direct] <snapshot-id>
#   sh /hyphae-home/capture-snapshot.sh --inspect <json file>
#   sh capture-snapshot.sh --selftest        (host side, no Hyphae needed)
#   --inspect    no session: print what this script extracts from a saved
#                "agent ui" response or "proof verify" report (debugging aid)
#   --selftest   no session: check the parts of this script that a session
#                cannot rehearse (store response classification, the
#                tokenizer's refusal of raw control characters, the bounded
#                daemon stop and the signal traps, with sleep processes as
#                the daemon); run it under dash + mawk in a throwaway
#                debian:trixie-slim container and under dash + gawk on the
#                host before a session. Writes only under /tmp.
#   --dry-run    print every command, execute nothing, create nothing
#   --ui-direct  stop the daemon after seeding, BEFORE the "agent ui" captures.
#                Use only if the session shows that "agent ui" opens the data
#                directory itself and refuses one held by a daemon. The default
#                keeps the daemon up for the captures: "hyphae agent --help"
#                describes the agent surface as the adapter "over managed
#                Native HTTP v2", i.e. a daemon client, and "agent ui --help"
#                says nothing about either. Unverified until the session.
#   <snapshot-id> matches ^[a-z0-9][a-z0-9-]{0,63}$ and becomes the snapshot
#                directory name on the host (e.g. snap-2026-10-29-demo-repo).
# Paths (override with SNAPSHOT_HOME_DIR / SNAPSHOT_OUT_DIR / SNAPSHOT_BASE_DIR):
#   instance HOME  /hyphae-home/sealed/<snapshot-id>     (refused if it exists)
#   output         /hyphae-home/snapshots/<snapshot-id>  (refused if it exists)
#   siblings       /hyphae-home/{hyphae-env.sh,seed.sh,seed-demo-facts.sh}
# Exit codes: 0 ok; 2 usage; 3 precondition, setup, daemon or seeding failure;
# 4 a capture failed its checks; 5 a verification failed; 6 the credential
# destruction could not be confirmed; 130, 143, 129 when SIGINT, SIGTERM or
# SIGHUP interrupted the session (the daemon is stopped and the scratch
# directory removed first; the signal is then re-raised).
set -eu

# ---------------------------------------------------------------- arguments
usage() {
  echo "usage: sh capture-snapshot.sh [--dry-run] [--ui-direct] <snapshot-id>"
  echo "       sh capture-snapshot.sh --inspect <json file>"
  echo "       sh capture-snapshot.sh --selftest"
}
MODE=capture
DRY_RUN=no
UI_MODE=daemon
SNAPSHOT_ID=""
INSPECT_FILE=""
VICTIM_DIR=""
while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) DRY_RUN=yes ;;
    --ui-direct) UI_MODE=direct ;;
    --ui-via-daemon) UI_MODE=daemon ;;
    --inspect)
      if [ $# -lt 2 ]; then usage >&2; exit 2; fi
      MODE=inspect
      INSPECT_FILE=$2
      shift
      ;;
    --selftest) MODE=selftest ;;
    # Internal: the child process of the --selftest trap rehearsal.
    --selftest-victim)
      if [ $# -lt 2 ]; then usage >&2; exit 2; fi
      MODE=selftest-victim
      VICTIM_DIR=$2
      shift
      ;;
    -h|--help) usage; exit 0 ;;
    -*) usage >&2; exit 2 ;;
    *)
      if [ -n "$SNAPSHOT_ID" ]; then usage >&2; exit 2; fi
      SNAPSHOT_ID=$1
      ;;
  esac
  shift
done
case $MODE in
  inspect) SNAPSHOT_ID=inspect ;;
  selftest|selftest-victim) SNAPSHOT_ID=selftest ;;
  *) if [ -z "$SNAPSHOT_ID" ]; then usage >&2; exit 2; fi ;;
esac
case $SNAPSHOT_ID in
  -*|*[!a-z0-9-]*) echo "snapshot id must match ^[a-z0-9][a-z0-9-]{0,63}$" >&2; exit 2 ;;
esac
if [ ${#SNAPSHOT_ID} -gt 64 ]; then echo "snapshot id longer than 64 characters" >&2; exit 2; fi

# ---------------------------------------------------------------- constants
BASE=${SNAPSHOT_BASE_DIR:-/hyphae-home}
NEW_HOME=${SNAPSHOT_HOME_DIR:-$BASE/sealed/$SNAPSHOT_ID}
OUT=${SNAPSHOT_OUT_DIR:-$BASE/snapshots/$SNAPSHOT_ID}
CAP=$OUT/captures
LOG=$OUT/capture.log
PROJECT=demo-repo
EXPECTED_RECORDS=7
CONTROL_SCHEMA=hyphae-omarchy-control-v1
PROOF_SCOPE=memory-snapshot-v1
# The two REQUIRED captures (contract with scripts/snapshot_manifest.py and the
# runtime SnapshotMemoryClient). The arguments objects are sent byte for byte
# and written byte for byte into captures.index.
LIST_ID=list-demo-repo
LIST_ARGS='{"project":"demo-repo","query":"","limit":64,"prove":true}'
ISOLATION_ID=recall-other-repo-isolation
ISOLATION_ARGS='{"project":"other-repo","query":"Fix payment client timeouts","limit":8,"prove":true}'
# Layout of an Agent Memory HOME, as hyphae-env.sh derives it; asserted equal
# to what hyphae-env.sh computes once it is sourced for the new HOME.
DATA_DIR=$NEW_HOME/.local/share/hyphae/agent-memory
CRED_DIR=$NEW_HOME/.config/hyphae/credentials
# The anchor of the owner's demo store (/hyphae-home) on 2026-10-27. The
# sealed instance is a different store, so its anchor is expected to differ;
# equality is reported as a warning (it would mean the pinned public anchor no
# longer distinguishes the snapshot from the demo store), not as a failure.
DEMO_STORE_ANCHOR=d91ac10d17ce58a641f89a5337f3d0266c3b5eb0acb87db3ff3202c79f19f168
# How long stop_daemon waits for the daemon process to exit after each stop
# signal (SIGINT, then SIGTERM) before escalating; the last step is SIGKILL.
# The wait for the socket to vanish after SIGINT (30 s) comes before this.
DAEMON_STOP_GRACE=10

TMP=""
LOG_READY=""
DAEMON_PID=""
ANCHOR=""
CAPTURE_IDS=""
PROOF_DIRS=""
SEEDED=0
SESSION_START=""
HYPHAE_SOCK=""
HYPHAE_DATA=""
HYPHAE_READER_KEY=""
HYPHAE_WRITER_KEY=""

# ---------------------------------------------------------------- helpers
utc() { date -u +%Y-%m-%dT%H:%M:%SZ; }
log() {
  _line="$(utc) $*"
  printf '%s\n' "$_line"
  if [ "$DRY_RUN" = no ] && [ -n "$LOG_READY" ]; then printf '%s\n' "$_line" >> "$LOG"; fi
}
die() { # die <exit code> <message>
  _code=$1; shift
  log "FAILED: $*"
  exit "$_code"
}
# Print a command, then run it (unless --dry-run).
run() {
  log "+ $*"
  if [ "$DRY_RUN" = yes ]; then return 0; fi
  "$@"
}
# Print a command, then run it with stdout to a file; stderr is kept aside and
# logged (bounded) only when the command fails or when asked (LOG_STDERR=yes).
run_to() { # run_to <file> <command...>
  _file=$1; shift
  log "+ $* > $_file"
  if [ "$DRY_RUN" = yes ]; then return 0; fi
  _rc=0
  "$@" > "$_file" 2> "$TMP/last.stderr" || _rc=$?
  if [ "$_rc" -ne 0 ] || [ "${LOG_STDERR:-no}" = yes ]; then log_stderr; fi
  return "$_rc"
}
# Print a command, then run it with a one-line request on stdin and stdout to a file.
run_in() { # run_in <request> <file> <command...>
  _req=$1; _file=$2; shift 2
  log "+ printf '%s\\n' <request> | $* > $_file"
  log "  request: $_req"
  if [ "$DRY_RUN" = yes ]; then return 0; fi
  _rc=0
  printf '%s\n' "$_req" | "$@" > "$_file" 2> "$TMP/last.stderr" || _rc=$?
  if [ "$_rc" -ne 0 ]; then log_stderr; fi
  return "$_rc"
}
log_stderr() {
  if [ -s "$TMP/last.stderr" ]; then
    head -n 40 "$TMP/last.stderr" | while IFS= read -r _l; do log "  stderr: $_l"; done
  fi
}
is_hex64() {
  case $1 in ''|*[!0-9a-f]*) return 1 ;; esac
  [ ${#1} -eq 64 ]
}
lower() { printf '%s' "$1" | tr 'A-F' 'a-f'; }
# The sha256 of one file, 64 lowercase hex digits or failure.
sha256_of() { # sha256_of <file>
  _d=$(sha256sum -- "$1" | cut -c1-64) || return 1
  is_hex64 "$_d" || return 1
  printf '%s' "$_d"
}
# Regular files under the instance HOME outside its data directory, with
# sizes: what setup and configure created, as evidence in the log (names and
# sizes only, never contents).
list_home_files() { # list_home_files <label>
  log "$1 (files under $NEW_HOME outside the data directory; names and sizes only):"
  find "$NEW_HOME" -path "$DATA_DIR" -prune -o -type f -print | sort | while IFS= read -r _f; do
    log "  $_f ($(stat -c '%s bytes, mode %a' "$_f"))"
  done
}
# Values this script copies into paths and index lines must be plain: no
# JSON escapes, quotes, control characters or separators it does not expect.
plain() {
  for _v in "$@"; do
    case $_v in
      ''|*\\*|*\"*|*\|*|*[[:cntrl:]]*) return 1 ;;
    esac
  done
  return 0
}

# ------------------------------------------------- JSON without jq or python
# The container has neither jq nor python3, so JSON is read with this small awk
# program (mawk compatible). It parses one JSON document completely and prints
# one line per scalar leaf and per empty container:
#     <path>TAB<type>TAB<raw value>
# path: "" is the root, ".key" an object member, "[N]" an array element
# (e.g. .memories[3].id). type: string, number, true, false, null,
# empty_object, empty_array. String values keep their JSON escapes raw; the
# fields this script uses (paths, hex digests, decimal ids) are rejected if
# they contain any escape. Any syntax it does not understand, any object key
# outside [A-Za-z0-9_-], any raw control character (bytes 0x00-0x1F, which
# JSON forbids inside a string; a raw newline would otherwise split one leaf
# into two lines) and any trailing data is a hard failure (exit 3), so an
# unexpected shape stops the session instead of producing a partial index.
# It handles compact one-line output as well as pretty-printed output: the
# "agent ui" help text does not say which one it prints (unverified), while
# the live "proof verify" report seen on 2026-10-27 was pretty-printed.
JSON_LEAVES='
BEGIN { buf = "" }
{ buf = buf $0 "\n" }
END {
  n = length(buf)
  pos = 1
  skip_ws()
  if (pos > n) fail("empty input")
  parse_value("")
  skip_ws()
  if (pos <= n) fail("trailing data after the JSON value")
}
function fail(msg) {
  printf("json-leaves: %s (byte %d)\n", msg, pos) > "/dev/stderr"
  exit 3
}
function skip_ws(   c) {
  while (pos <= n) {
    c = substr(buf, pos, 1)
    if (c == " " || c == "\t" || c == "\n" || c == "\r") pos++
    else break
  }
}
function emit(path, type, value) {
  printf("%s\t%s\t%s\n", path, type, value)
}
function parse_string(   s, rest, c, esc, frag) {
  if (substr(buf, pos, 1) != "\"") fail("expected a string")
  pos++
  s = ""
  while (1) {
    if (pos > n) fail("unterminated string")
    rest = substr(buf, pos)
    if (match(rest, /["\\]/) == 0) fail("unterminated string")
    frag = substr(rest, 1, RSTART - 1)
    pos += RSTART - 1
    # JSON forbids raw control characters inside a string (they must be
    # escaped); the class is octal 001-037 = bytes 0x01-0x1F, which both
    # mawk and gawk accept. A NUL byte is refused before awk runs (json_leaves).
    if (match(frag, /[\001-\037]/) > 0) {
      pos = pos - length(frag) + RSTART - 1
      fail("raw control character inside a string")
    }
    s = s frag
    c = substr(buf, pos, 1)
    if (c == "\"") { pos++; return s }
    esc = substr(buf, pos + 1, 1)
    if (esc == "") fail("unterminated escape")
    if (esc == "u") {
      if (substr(buf, pos + 2, 4) !~ /^[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]$/) fail("bad unicode escape")
      s = s "\\u" substr(buf, pos + 2, 4)
      pos += 6
    } else if (esc ~ /^["\\\/bfnrt]$/) {
      s = s "\\" esc
      pos += 2
    } else fail("bad escape")
  }
}
function parse_number(   rest) {
  rest = substr(buf, pos)
  if (match(rest, /^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)?/) == 0) fail("bad number")
  pos += RLENGTH
  return substr(rest, 1, RLENGTH)
}
function parse_value(path,   c, s, key, i) {
  skip_ws()
  if (pos > n) fail("unexpected end of input")
  c = substr(buf, pos, 1)
  if (c == "{") {
    pos++
    skip_ws()
    if (substr(buf, pos, 1) == "}") { pos++; emit(path, "empty_object", ""); return }
    while (1) {
      skip_ws()
      key = parse_string()
      if (key !~ /^[A-Za-z0-9_-]+$/) fail("unsupported object key: " key)
      skip_ws()
      if (substr(buf, pos, 1) != ":") fail("expected a colon")
      pos++
      parse_value(path "." key)
      skip_ws()
      c = substr(buf, pos, 1)
      if (c == ",") { pos++; continue }
      if (c == "}") { pos++; return }
      fail("expected a comma or a closing brace")
    }
  }
  if (c == "[") {
    pos++
    skip_ws()
    if (substr(buf, pos, 1) == "]") { pos++; emit(path, "empty_array", ""); return }
    i = 0
    while (1) {
      parse_value(path "[" i "]")
      i++
      skip_ws()
      c = substr(buf, pos, 1)
      if (c == ",") { pos++; continue }
      if (c == "]") { pos++; return }
      fail("expected a comma or a closing bracket")
    }
  }
  if (c == "\"") { s = parse_string(); emit(path, "string", s); return }
  if (substr(buf, pos, 4) == "true") { pos += 4; emit(path, "true", "true"); return }
  if (substr(buf, pos, 5) == "false") { pos += 5; emit(path, "false", "false"); return }
  if (substr(buf, pos, 4) == "null") { pos += 4; emit(path, "null", "null"); return }
  if (c ~ /^[-0-9]$/) { s = parse_number(); emit(path, "number", s); return }
  fail("unexpected character")
}
'
json_leaves() { # json_leaves <json file> <leaves file>
  # A NUL byte is never valid JSON (not even as whitespace) and an awk regular
  # expression cannot name it, so it is refused here, before the tokenizer.
  if [ "$(tr -cd '\000' < "$1" | wc -c | tr -d ' ')" -ne 0 ]; then
    echo "json-leaves: NUL byte in the input" >&2
    return 3
  fi
  awk "$JSON_LEAVES" < "$1" > "$2"
}
# The value of exactly one leaf with this path and type; failure otherwise
# (absent, duplicated, or another type).
leaf() { # leaf <leaves file> <path> <type>
  awk -F '\t' -v p="$2" -v t="$3" '
    $1 == p && $2 == t { v = $3; k++ }
    END { if (k != 1) exit 1; print v }' "$1"
}
# The value of exactly one leaf with this path, whatever its type.
leaf_any() { # leaf_any <leaves file> <path>
  awk -F '\t' -v p="$2" '
    $1 == p { v = $3; k++ }
    END { if (k != 1) exit 1; print v }' "$1"
}
has_leaf() { # has_leaf <leaves file> <path> <type>
  awk -F '\t' -v p="$2" -v t="$3" '
    $1 == p && $2 == t { f = 1 }
    END { exit f ? 0 : 1 }' "$1"
}
# Where the result object lives: "" when the response is the result object
# itself, ".result" when the response wraps it (the help text does not say
# which; both are accepted and the response file keeps the exact bytes).
detect_root() { # detect_root <leaves file>
  if awk -F '\t' '
      $1 == ".memories" || index($1, ".memories[") == 1 { f = 1 }
      END { exit f ? 0 : 1 }' "$1"; then
    printf ''
    return 0
  fi
  if awk -F '\t' '
      $1 == ".result.memories" || index($1, ".result.memories[") == 1 { f = 1 }
      END { exit f ? 0 : 1 }' "$1"; then
    printf '.result'
    return 0
  fi
  return 1
}
# One field of every memories[N] in response order.
memory_field() { # memory_field <leaves file> <root> <field>
  awk -F '\t' -v r="$2" -v f="$3" '
    BEGIN { pre = r ".memories[" }
    index($1, pre) == 1 {
      rest = substr($1, length(pre) + 1)
      if (rest ~ /^[0-9]+\]\.[A-Za-z0-9_-]+$/) {
        sub(/^[0-9]+\]\./, "", rest)
        if (rest == f && $2 == "string") print $3
      }
    }' "$1"
}

# ---------------------------------------------------------------- daemon
# Same two commands as serve.sh (agent recover, then serve with the socket
# outside the data directory), for the NEW HOME only; the pid is kept so the
# daemon can be stopped by this script and reaped (PID 1 is sleep infinity).
start_daemon() {
  log "+ env HOME=$NEW_HOME hyphae agent recover"
  if [ "$DRY_RUN" = no ]; then
    env HOME="$NEW_HOME" hyphae agent recover || die 3 "hyphae agent recover failed for $NEW_HOME"
  fi
  log "+ echo '--- capture-snapshot serve start <utc> endpoint $HYPHAE_SOCK' >> $NEW_HOME/serve.log"
  log "+ nohup env HOME=$NEW_HOME hyphae serve --data-dir $HYPHAE_DATA --endpoint $HYPHAE_SOCK --native-api-key-auth >> $NEW_HOME/serve.log 2>&1 &"
  if [ "$DRY_RUN" = yes ]; then return 0; fi
  echo "--- capture-snapshot serve start $(utc) endpoint $HYPHAE_SOCK" >> "$NEW_HOME/serve.log"
  nohup env HOME="$NEW_HOME" hyphae serve --data-dir "$HYPHAE_DATA" --endpoint "$HYPHAE_SOCK" --native-api-key-auth >> "$NEW_HOME/serve.log" 2>&1 &
  DAEMON_PID=$!
  log "daemon pid $DAEMON_PID; waiting for the socket $HYPHAE_SOCK"
  _i=0
  while [ ! -S "$HYPHAE_SOCK" ]; do
    _i=$((_i + 1))
    if [ "$_i" -gt 60 ]; then die 3 "the daemon socket did not appear within 30 s (see $NEW_HOME/serve.log)"; fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then die 3 "the daemon exited early (see $NEW_HOME/serve.log)"; fi
    sleep 0.5
  done
  sleep 1
  log "daemon ready on $HYPHAE_SOCK"
}
# Polls (every 0.5 s, at most <seconds> seconds) for the daemon process to be
# gone: 0 when it is, 1 when it is still running at the end.
daemon_gone_within() { # daemon_gone_within <seconds>
  _n=$(( $1 * 2 ))
  while kill -0 "$DAEMON_PID" 2>/dev/null; do
    if [ "$_n" -le 0 ]; then return 1; fi
    _n=$((_n - 1))
    sleep 0.5
  done
  return 0
}
# Stops the daemon this script started and reaps it, in bounded time: SIGINT
# (the daemon removes its socket, then exits), SIGTERM after DAEMON_STOP_GRACE
# seconds if it is still running, SIGKILL after another DAEMON_STOP_GRACE
# seconds. The bound matters: step 7 (credential destruction) and the cleanup
# trap come after this, and an unbounded wait on a daemon that ignores its
# signals would leave the credentials in place.
stop_daemon() {
  if [ -z "$DAEMON_PID" ]; then return 0; fi
  log "+ kill -INT $DAEMON_PID   (the daemon of $NEW_HOME; SIGINT makes it remove its socket)"
  kill -INT "$DAEMON_PID" 2>/dev/null || log "daemon $DAEMON_PID was already gone"
  _i=0
  while [ -S "$HYPHAE_SOCK" ] && kill -0 "$DAEMON_PID" 2>/dev/null; do
    _i=$((_i + 1))
    if [ "$_i" -gt 60 ]; then
      log "WARNING: the socket is still present after 30 s"
      break
    fi
    sleep 0.5
  done
  if ! daemon_gone_within "$DAEMON_STOP_GRACE"; then
    log "WARNING: daemon $DAEMON_PID is still running $DAEMON_STOP_GRACE s after SIGINT; sending SIGTERM"
    kill -TERM "$DAEMON_PID" 2>/dev/null || true
    if ! daemon_gone_within "$DAEMON_STOP_GRACE"; then
      log "WARNING: daemon $DAEMON_PID is still running $DAEMON_STOP_GRACE s after SIGTERM; sending SIGKILL (its socket $HYPHAE_SOCK may be left behind)"
      kill -KILL "$DAEMON_PID" 2>/dev/null || true
    fi
  fi
  wait "$DAEMON_PID" 2>/dev/null || true
  DAEMON_PID=""
  log "daemon stopped"
}
cleanup() { # cleanup <exit status>
  _status=$1
  if [ -n "$DAEMON_PID" ]; then
    log "cleanup: stopping the daemon this session started"
    stop_daemon
  fi
  if [ -n "$TMP" ] && [ "$DRY_RUN" = no ]; then rm -rf -- "$TMP"; fi
  if [ "$_status" -ne 0 ] && [ "$DRY_RUN" = no ] && [ "$MODE" = capture ]; then
    log "session FAILED (exit $_status). Nothing was deleted. The instance HOME $NEW_HOME may still hold credentials: review it, destroy them by hand (docs/architecture/sealed-snapshot.md, section Failure handling), or rerun with a new snapshot id. Output so far: $OUT"
  fi
}
# dash runs the EXIT trap on exit only, not when a signal kills the shell, so
# SIGINT (terminal), SIGTERM and SIGHUP (a docker exec session torn down) get
# the same cleanup and are then re-raised with their default action, so the
# exit status says which signal it was. POSIX defers a trap while a foreground
# child runs (hyphae agent ui, say): the cleanup follows when it returns.
# SIGKILL cannot be caught; docs/architecture/sealed-snapshot.md (section 3,
# failure handling) says how to find a daemon left behind without ps.
on_signal() { # on_signal <signal name> <exit status>
  trap - INT TERM HUP EXIT
  log "received SIG$1: stopping the daemon and cleaning up before exiting"
  cleanup "$2"
  kill "-$1" $$
  exit "$2"
}
trap 'cleanup $?' EXIT
trap 'on_signal INT 130' INT
trap 'on_signal TERM 143' TERM
trap 'on_signal HUP 129' HUP

# ---------------------------------------------------------------- seeding
# One memory through the writer credential, with hyphae_store from
# hyphae-env.sh (the MCP writer path seed.sh and seed-demo-facts.sh use;
# identity is derived from project + layer + text, so identical text gives the
# same memory id as the recorded research runs). The text must appear
# verbatim in seed.sh or seed-demo-facts.sh next to this script.
# The one response line hyphae_store prints (the JSON-RPC message with
# "id":2) counts as a stored record only when it is a tools/call RESULT object
# without an error marker. "hyphae mcp" answers a refused request (reader key
# where the writer is needed, write not allowed, bad arguments) with a
# JSON-RPC ERROR object ({"jsonrpc":"2.0","id":2,"error":{...}}), which
# carries neither isError nor a tool-error envelope: without the positive
# marker it would pass as a success and the session would fail later, at the
# capture, with a misleading message. 0 = stored, 1 = anything else.
store_response_ok() { # store_response_ok <response line>
  case $1 in
    '') return 1 ;;
    *'"isError":true'*|*'"isError": true'*|*tool-error*|*'"error":'*|*'"error" :'*) return 1 ;;
  esac
  case $1 in
    *'"result":{'*|*'"result": {'*) return 0 ;;
  esac
  return 1
}
seed_one() { # seed_one <kind> <text>
  _kind=$1; _text=$2
  case $_text in
    *\"*|*\\*) die 3 "seed text contains a double quote or a backslash, which hyphae_store cannot send" ;;
  esac
  if ! grep -F -q -- "$_text" "$BASE/seed.sh" "$BASE/seed-demo-facts.sh"; then
    die 3 "seed text is not found verbatim in $BASE/seed.sh or $BASE/seed-demo-facts.sh: $_text"
  fi
  log "+ hyphae_store $PROJECT $_kind \"$_text\""
  if [ "$DRY_RUN" = yes ]; then return 0; fi
  _resp=$(hyphae_store "$PROJECT" "$_kind" "$_text") || die 3 "hyphae_store failed"
  if [ -z "$_resp" ]; then die 3 "hyphae_store returned no response line (daemon not reachable?)"; fi
  if ! store_response_ok "$_resp"; then
    die 3 "hyphae_store did not return a tools/call result object (a JSON-RPC error, a tool error or an unexpected shape); raw response: $_resp"
  fi
  log "  store response: $_resp"
  SEEDED=$((SEEDED + 1))
}

# ---------------------------------------------------------------- captures
capture() { # capture <id> <operation> <arguments json> <expected record count> <request number>
  _cid=$1; _cop=$2; _cargs=$3; _cexp=$4; _cnum=$5
  # agent ui accepts only an integer request id (a string id is refused as
  # invalid_request; observed on the 2026-10-29 scratch session).
  _creq="{\"schema\":\"$CONTROL_SCHEMA\",\"id\":$_cnum,\"operation\":\"$_cop\",\"arguments\":$_cargs}"
  _cresp=$CAP/$_cid.response.json
  case $_cargs in
    *\|*|*\\*|*[[:cntrl:]]*) die 4 "capture $_cid: arguments contain a pipe, a backslash or a control character, which this script does not index" ;;
  esac
  log "capture $_cid: operation $_cop, arguments $_cargs, expecting $_cexp record(s)"
  _cstart=$(utc)
  _rc=0
  run_in "$_creq" "$_cresp" env HOME="$NEW_HOME" timeout 120 hyphae agent ui || _rc=$?
  if [ "$_rc" -ne 0 ]; then die 4 "capture $_cid: hyphae agent ui exited $_rc (its stdout is kept in $_cresp)"; fi
  if [ "$DRY_RUN" = yes ]; then
    log "(dry-run) would validate $_cresp, require proof != null and $_cexp record(s), copy proof and witness to $CAP/$_cid.proof and $CAP/$_cid.witness"
    return 0
  fi
  if [ ! -s "$_cresp" ]; then die 4 "capture $_cid: empty response"; fi
  log "capture $_cid: response $(wc -c < "$_cresp" | tr -d ' ') bytes, $(wc -l < "$_cresp" | tr -d ' ') newline(s)"
  _leaves=$TMP/$_cid.leaves
  json_leaves "$_cresp" "$_leaves" || die 4 "capture $_cid: the response is not JSON this script understands (see above)"
  _root=$(detect_root "$_leaves") || {
    log "capture $_cid: the response has no memories array; error fields, if any:"
    grep -E '^(\.result)?\.(code|message|error|status)' "$_leaves" | while IFS= read -r _l; do log "  $_l"; done
    die 4 "capture $_cid: not a list/recall result"
  }
  if has_leaf "$_leaves" "$_root.proof" null; then die 4 "capture $_cid: proof is null (proof generation failed or prove was not honoured)"; fi
  _ppath=$(leaf "$_leaves" "$_root.proof.proof_path" string) || die 4 "capture $_cid: proof.proof_path missing or not unique"
  _wpath=$(leaf "$_leaves" "$_root.proof.witness_path" string) || die 4 "capture $_cid: proof.witness_path missing or not unique"
  _pblake=$(leaf "$_leaves" "$_root.proof.proof_blake3" string) || die 4 "capture $_cid: proof.proof_blake3 missing or not unique"
  _canchor=$(leaf "$_leaves" "$_root.proof.anchor_hex" string) || die 4 "capture $_cid: proof.anchor_hex missing or not unique"
  _pscope=$(leaf "$_leaves" "$_root.proof.scope" string) || die 4 "capture $_cid: proof.scope missing or not unique"
  plain "$_ppath" "$_wpath" "$_pblake" "$_canchor" || die 4 "capture $_cid: a proof field contains an escape or an unexpected character"
  _canchor=$(lower "$_canchor")
  _pblake=$(lower "$_pblake")
  is_hex64 "$_canchor" || die 4 "capture $_cid: anchor_hex is not 64 hex digits"
  is_hex64 "$_pblake" || die 4 "capture $_cid: proof_blake3 is not 64 hex digits"
  if [ "$_pscope" != "$PROOF_SCOPE" ]; then die 4 "capture $_cid: proof.scope is $_pscope, expected $PROOF_SCOPE"; fi
  # The proof must come from the NEW instance: Hyphae writes proofs under the
  # HOME it runs with. A path outside it means agent ui answered from another
  # store (the demo store, for example), which must never be captured.
  case $_ppath in "$NEW_HOME"/*) ;; *) die 4 "capture $_cid: the proof was written outside the instance HOME ($_ppath): agent ui did not answer from the new instance" ;; esac
  case $_wpath in "$NEW_HOME"/*) ;; *) die 4 "capture $_cid: the witness was written outside the instance HOME ($_wpath): agent ui did not answer from the new instance" ;; esac
  case $_ppath in "$DATA_DIR"/*) die 4 "capture $_cid: the proof was written inside the data directory ($_ppath)" ;; esac
  if [ ! -s "$_ppath" ]; then die 4 "capture $_cid: the proof file named by the response is missing or empty: $_ppath"; fi
  if [ ! -s "$_wpath" ]; then die 4 "capture $_cid: the witness file named by the response is missing or empty: $_wpath"; fi
  _pdir=$(dirname -- "$_ppath")
  case " $PROOF_DIRS " in *" $_pdir "*) ;; *) PROOF_DIRS="$PROOF_DIRS $_pdir" ;; esac
  _ids=$TMP/$_cid.ids
  memory_field "$_leaves" "$_root" id > "$_ids"
  _count=$(wc -l < "$_ids" | tr -d ' ')
  if [ "$_count" -eq 0 ] && ! has_leaf "$_leaves" "$_root.memories" empty_array; then
    die 4 "capture $_cid: memories is neither an array of records with ids nor an empty array"
  fi
  while IFS= read -r _id; do
    case $_id in ''|*[!0-9]*) die 4 "capture $_cid: a memory id is not a decimal string: $_id" ;; esac
  done < "$_ids"
  if [ "$_count" -ne "$_cexp" ]; then die 4 "capture $_cid: $_count record(s) returned, expected exactly $_cexp"; fi
  if [ "$_count" -gt 0 ]; then
    _projects=$(memory_field "$_leaves" "$_root" project | sort -u | tr '\n' ' ')
    if [ "$_projects" != "$PROJECT " ]; then die 4 "capture $_cid: records attributed to unexpected project(s): $_projects"; fi
  fi
  if [ -z "$ANCHOR" ]; then
    ANCHOR=$_canchor
    log "anchor_hex (from $_cid, shared by every capture): $ANCHOR"
    if [ "$ANCHOR" = "$DEMO_STORE_ANCHOR" ]; then
      log "WARNING: this anchor equals the demo store's anchor of 2026-10-27 ($DEMO_STORE_ANCHOR); pinning it would not distinguish the snapshot from the demo store. The proof paths above show the store that answered; review before using the snapshot."
    fi
  elif [ "$_canchor" != "$ANCHOR" ]; then
    die 4 "capture $_cid: anchor_hex $_canchor differs from the anchor of the first capture $ANCHOR (the store changed between captures)"
  fi
  run cp -- "$_ppath" "$CAP/$_cid.proof"
  run cp -- "$_wpath" "$CAP/$_cid.witness"
  cmp -s "$_ppath" "$CAP/$_cid.proof" || die 4 "capture $_cid: the proof copy differs from $_ppath"
  cmp -s "$_wpath" "$CAP/$_cid.witness" || die 4 "capture $_cid: the witness copy differs from $_wpath"
  # The capture-time digests: computed here, right after the copies were
  # compared with the files the response names, and carried to the host in
  # captures.index. The manifest builder refuses a file whose digest differs
  # (a swapped or altered proof, witness or response is caught even though
  # every proof of this session shares the anchor).
  _rsha=$(sha256_of "$_cresp") || die 4 "capture $_cid: sha256sum of the response failed"
  _psha=$(sha256_of "$CAP/$_cid.proof") || die 4 "capture $_cid: sha256sum of the proof copy failed"
  _wsha=$(sha256_of "$CAP/$_cid.witness") || die 4 "capture $_cid: sha256sum of the witness copy failed"
  log "capture $_cid: $_count record(s); proof $(wc -c < "$CAP/$_cid.proof" | tr -d ' ') bytes (blake3 $_pblake); witness $(wc -c < "$CAP/$_cid.witness" | tr -d ' ') bytes"
  log "capture $_cid: sha256 response $_rsha"
  log "capture $_cid: sha256 proof    $_psha"
  log "capture $_cid: sha256 witness  $_wsha"
  for _k in expired_filtered retrieval_mode semantic_status snapshot.visible_csn snapshot.root_digest snapshot.logical_time_micros; do
    if _v=$(leaf_any "$_leaves" "$_root.$_k"); then log "capture $_cid: $_k = $_v"; fi
  done
  while IFS= read -r _id; do log "capture $_cid: memory id $_id"; done < "$_ids"
  printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\n' "$_cid" "$_cop" "$_cargs" "$_pblake" "$_canchor" "$_cstart" "$_rsha" "$_psha" "$_wsha" > "$TMP/$_cid.meta"
  CAPTURE_IDS="$CAPTURE_IDS $_cid"
}

verify_capture() { # verify_capture <id>
  _vid=$1
  _vout=$CAP/$_vid.verify.json
  _vstart=$(utc)
  _rc=0
  run_to "$_vout" env HOME="$NEW_HOME" timeout 600 hyphae proof verify --proof "$CAP/$_vid.proof" --witness "$CAP/$_vid.witness" --anchor "${ANCHOR:-<anchor_hex of the list response>}" || _rc=$?
  if [ "$_rc" -ne 0 ]; then die 5 "verify $_vid: hyphae proof verify exited $_rc (its stdout is kept in $_vout)"; fi
  if [ "$DRY_RUN" = yes ]; then
    log "(dry-run) would require status=verified kind=memory scope=semantic_reexecution semantic_reexecution_performed=true anchor_digest=<anchor> in $_vout"
    return 0
  fi
  _vleaves=$TMP/$_vid.vleaves
  json_leaves "$_vout" "$_vleaves" || die 5 "verify $_vid: the verifier report is not JSON this script understands"
  _vstatus=$(leaf "$_vleaves" .status string) || die 5 "verify $_vid: report has no unique string status"
  _vkind=$(leaf "$_vleaves" .kind string) || die 5 "verify $_vid: report has no unique string kind"
  _vscope=$(leaf "$_vleaves" .scope string) || die 5 "verify $_vid: report has no unique string scope"
  _vre=$(leaf_any "$_vleaves" .semantic_reexecution_performed) || die 5 "verify $_vid: report has no semantic_reexecution_performed"
  _vanchor=$(leaf "$_vleaves" .anchor_digest string) || die 5 "verify $_vid: report has no unique string anchor_digest"
  if [ "$_vstatus" != verified ]; then die 5 "verify $_vid: status is $_vstatus, not verified"; fi
  if [ "$_vkind" != memory ]; then die 5 "verify $_vid: kind is $_vkind, not memory"; fi
  if [ "$_vscope" != semantic_reexecution ]; then die 5 "verify $_vid: scope is $_vscope, not semantic_reexecution"; fi
  if [ "$_vre" != true ]; then die 5 "verify $_vid: semantic_reexecution_performed is $_vre, not true"; fi
  if [ "$(lower "$_vanchor")" != "$ANCHOR" ]; then die 5 "verify $_vid: anchor_digest $_vanchor is not the anchor given ($ANCHOR)"; fi
  log "verify $_vid: status=$_vstatus kind=$_vkind scope=$_vscope semantic_reexecution_performed=$_vre anchor_digest=$_vanchor"
  # The report's own digests (proof, witness, request, result), when present:
  # they are deterministic functions of the proof and the witness, so a later
  # run of the same verifier on the same files must reproduce them (what the
  # runtime does at boot, and what snapshot_manifest.py --reverify compares).
  for _k in proof_digest witness_digest request_digest result_digest evidence_digest file_count directory_count total_file_bytes; do
    if _v=$(leaf_any "$_vleaves" ".$_k"); then log "verify $_vid: $_k = $_v"; fi
  done
  _vsha=$(sha256_of "$_vout") || die 5 "verify $_vid: sha256sum of the verifier report failed"
  log "verify $_vid: sha256 report   $_vsha"
  printf '%s\n' "$_vstart" > "$TMP/$_vid.verified_at"
  printf '%s\n' "$_vsha" > "$TMP/$_vid.vsha"
}

# ---------------------------------------------------------------- inspect
# --inspect <file>: the extraction above, applied to a saved response or
# verifier report, printed for a human. Reads one file, writes nothing but a
# scratch directory under /tmp that it removes.
if [ "$MODE" = inspect ]; then
  if [ ! -f "$INSPECT_FILE" ]; then echo "no such file: $INSPECT_FILE" >&2; exit 2; fi
  TMP=$(mktemp -d /tmp/capture-snapshot.XXXXXX)
  echo "leaves of $INSPECT_FILE (path, type, raw value):"
  json_leaves "$INSPECT_FILE" "$TMP/leaves" || { echo "NOT JSON this script understands (see the message above)"; exit 4; }
  cat "$TMP/leaves"
  echo "---"
  if _root=$(detect_root "$TMP/leaves"); then
    echo "list/recall result found at root: ${_root:-<top level>}"
    if has_leaf "$TMP/leaves" "$_root.proof" null; then echo "proof: null"; fi
    for _k in proof.proof_path proof.witness_path proof.proof_blake3 proof.anchor_hex proof.scope expired_filtered retrieval_mode semantic_status snapshot.visible_csn snapshot.root_digest snapshot.logical_time_micros; do
      if _v=$(leaf_any "$TMP/leaves" "$_root.$_k"); then echo "$_k = $_v"; else echo "$_k: absent or not unique"; fi
    done
    echo "memory ids, in response order:"
    memory_field "$TMP/leaves" "$_root" id
    echo "memory count: $(memory_field "$TMP/leaves" "$_root" id | wc -l | tr -d ' ')"
    echo "projects: $(memory_field "$TMP/leaves" "$_root" project | sort -u | tr '\n' ' ')"
  else
    echo "no list/recall result (no memories array)"
  fi
  for _k in status kind scope semantic_reexecution_performed anchor_digest code message; do
    if _v=$(leaf_any "$TMP/leaves" ".$_k"); then echo "$_k = $_v"; fi
  done
  exit 0
fi

# ---------------------------------------------------------------- selftest
# --selftest: host-side checks of the parts of this script a session cannot
# rehearse, with no Hyphae involved: store_response_ok on the response shapes
# "hyphae mcp" can print, the tokenizer's refusal of raw control characters
# (and --inspect on such a file), the bounded daemon stop (stop_daemon) and
# the signal traps (a child of this script, --selftest-victim, with a sleep
# process as its daemon, is sent SIGTERM; SELFTEST_SH names the shell that
# runs the child, default sh). Writes only a scratch directory under /tmp,
# removed at exit. Exit 0 when every check passed, 1 otherwise.
if [ "$MODE" = selftest-victim ]; then
  # The rehearsal child: a fake daemon that ignores SIGINT and SIGTERM stands
  # in for hyphae serve; the traps installed above must stop it (SIGKILL after
  # the bounded waits) when this process receives SIGTERM, then re-raise it.
  DAEMON_STOP_GRACE=1
  NEW_HOME=$VICTIM_DIR
  HYPHAE_SOCK=$VICTIM_DIR/no-such-socket
  sh -c 'trap "" INT TERM; while :; do sleep 1; done' &
  DAEMON_PID=$!
  printf '%s\n' "$DAEMON_PID" > "$VICTIM_DIR/daemon.pid"
  : > "$VICTIM_DIR/ready"
  while :; do sleep 0.2; done
fi
if [ "$MODE" = selftest ]; then
  TMP=$(mktemp -d /tmp/capture-snapshot.XXXXXX)
  PASSED=0
  FAILED=0
  check() { # check <label> <command...>: passes when the command exits 0
    _label=$1; shift
    if "$@"; then PASSED=$((PASSED + 1)); echo "ok   $_label"
    else FAILED=$((FAILED + 1)); echo "FAIL $_label"; fi
  }
  not() { ! "$@"; }
  # json_leaves must fail and name the reason on stderr.
  refused() { # refused <json file> <message fragment>
    if json_leaves "$1" "$TMP/leaves" 2> "$TMP/err"; then return 1; fi
    grep -q -F -- "$2" "$TMP/err"
  }
  accepted() { json_leaves "$1" "$TMP/leaves" 2> "$TMP/err"; }
  leaf_is() { [ "$(leaf "$1" "$2" "$3")" = "$4" ]; }
  inspect_refuses() { # --inspect exits 4 on a file the tokenizer refuses
    _rc=0
    "${SELFTEST_SH:-sh}" "$0" --inspect "$1" > "$TMP/inspect.out" 2>&1 || _rc=$?
    [ "$_rc" -eq 4 ] && grep -q "NOT JSON" "$TMP/inspect.out"
  }
  pid_gone() { [ -n "$1" ] && ! kill -0 "$1" 2>/dev/null; }
  # stop_daemon on the current DAEMON_PID: returns within <seconds>, the
  # process is gone afterwards, and its log (to <file>) says so.
  stopped_within() { # stopped_within <seconds> <log file>
    _pid=$DAEMON_PID
    _t0=$(date +%s)
    stop_daemon > "$2"
    _dt=$(( $(date +%s) - _t0 ))
    if ! pid_gone "$_pid"; then echo "  daemon $_pid still exists after stop_daemon"; return 1; fi
    if [ "$_dt" -gt "$1" ]; then echo "  stop_daemon took $_dt s, bound $1 s"; return 1; fi
    grep -q "daemon stopped" "$2"
  }

  echo "selftest: store response classification (seed_one)"
  _ok='{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"id\":\"186670067796945807561772405393844769735\"}"}],"structuredContent":{"id":"186670067796945807561772405393844769735","project":"demo-repo","kind":"fact"},"isError":false}}'
  _iserr='{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"write not allowed"}],"isError":true}}'
  _iserr_sp='{"jsonrpc": "2.0", "id": 2, "result": {"content": [], "isError": true}}'
  _toolerr='{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"schema":"hyphae-native-mcp-tool-error-v2","error":{"category":"permission","message":"write not allowed"}},"isError":false}}'
  _rpcerr='{"jsonrpc":"2.0","id":2,"error":{"code":-32001,"message":"authentication refused"}}'
  _rpcerr_sp='{"jsonrpc": "2.0", "id": 2, "error": {"code": -32602, "message": "invalid params"}}'
  check "a tools/call result object counts as a stored record" store_response_ok "$_ok"
  check "an empty line does not" not store_response_ok ""
  check "isError:true does not" not store_response_ok "$_iserr"
  check "isError: true (spaced) does not" not store_response_ok "$_iserr_sp"
  check "a tool-error envelope does not" not store_response_ok "$_toolerr"
  check "a JSON-RPC error object does not" not store_response_ok "$_rpcerr"
  check "a spaced JSON-RPC error object does not" not store_response_ok "$_rpcerr_sp"
  check "a message without a result object does not" not store_response_ok '{"jsonrpc":"2.0","id":2}'

  echo "selftest: tokenizer (json_leaves) and raw control characters"
  printf '{"project":"demo\nrepo","id":"1"}\n' > "$TMP/newline.json"
  printf '{"project":"demo\trepo"}\n' > "$TMP/tab.json"
  printf '{"project":"demo\000repo"}\n' > "$TMP/nul.json"
  printf '%s\n' '{"text":"line\nbreak\tand \"quote\"","n":1}' > "$TMP/escaped.json"
  printf '{\n  "memories": [\n    {"id": "1", "project": "demo-repo"}\n  ],\n  "proof": null\n}\n' > "$TMP/pretty.json"
  check "a raw newline inside a string is refused" refused "$TMP/newline.json" "raw control character"
  check "a raw tab inside a string is refused" refused "$TMP/tab.json" "raw control character"
  check "a NUL byte is refused" refused "$TMP/nul.json" "NUL byte"
  check "escaped control characters are accepted" accepted "$TMP/escaped.json"
  check "the escaped leaf keeps its escapes raw" leaf_is "$TMP/leaves" .text string 'line\nbreak\tand \"quote\"'
  check "pretty-printed JSON (newlines between tokens) is accepted" accepted "$TMP/pretty.json"
  check "the pretty-printed sample yields its leaves" leaf_is "$TMP/leaves" '.memories[0].project' string demo-repo
  check "--inspect refuses the raw newline sample with exit 4" inspect_refuses "$TMP/newline.json"

  echo "selftest: bounded daemon stop (stop_daemon, grace 1 s)"
  DAEMON_STOP_GRACE=1
  HYPHAE_SOCK=$TMP/no-such-socket
  sh -c 'exit 0' &
  DAEMON_PID=$!
  check "a daemon that already exited is reaped at once" stopped_within 4 "$TMP/stop-exited.log"
  # A background child of a non-interactive shell inherits SIGINT ignored, so
  # a plain sleep stands for a daemon that answers SIGTERM only.
  sleep 300 &
  DAEMON_PID=$!
  check "a daemon ignoring SIGINT is stopped by SIGTERM" stopped_within 5 "$TMP/stop-term.log"
  check "  and the escalation to SIGTERM is logged" grep -q "sending SIGTERM" "$TMP/stop-term.log"
  check "  without a SIGKILL" not grep -q "sending SIGKILL" "$TMP/stop-term.log"
  sh -c 'trap "" INT TERM; while :; do sleep 1; done' &
  DAEMON_PID=$!
  check "a daemon ignoring SIGINT and SIGTERM is killed within the bound" stopped_within 7 "$TMP/stop-kill.log"
  check "  and the SIGKILL is logged" grep -q "sending SIGKILL" "$TMP/stop-kill.log"

  echo "selftest: signal traps (a child of this script with a fake daemon is sent SIGTERM)"
  _vdir=$TMP/victim
  mkdir -p "$_vdir"
  "${SELFTEST_SH:-sh}" "$0" --selftest-victim "$_vdir" > "$_vdir/out.log" 2>&1 &
  _victim=$!
  _i=0
  while [ ! -f "$_vdir/ready" ]; do
    _i=$((_i + 1))
    if [ "$_i" -gt 50 ]; then break; fi
    sleep 0.2
  done
  check "the rehearsal child started its fake daemon" test -f "$_vdir/daemon.pid"
  _dpid=$(cat "$_vdir/daemon.pid" 2>/dev/null || true)
  _t0=$(date +%s)
  kill -TERM "$_victim" 2>/dev/null || true
  _vrc=0
  wait "$_victim" || _vrc=$?
  _dt=$(( $(date +%s) - _t0 ))
  check "SIGTERM ran the cleanup trap (logged)" grep -q "received SIGTERM" "$_vdir/out.log"
  check "the trap stopped the daemon (SIGKILL after the bounded waits)" grep -q "sending SIGKILL" "$_vdir/out.log"
  check "  and reaped it (daemon stopped logged)" grep -q "daemon stopped" "$_vdir/out.log"
  check "the fake daemon is gone" pid_gone "$_dpid"
  check "the child exited with the re-raised signal status 143 (got $_vrc)" test "$_vrc" -eq 143
  check "the cleanup on SIGTERM stayed within the bound (took $_dt s)" test "$_dt" -le 8

  echo "selftest: $PASSED passed, $FAILED failed"
  if [ "$FAILED" -ne 0 ]; then exit 1; fi
  exit 0
fi

# ================================================================ step 0
echo "capture-snapshot: snapshot id $SNAPSHOT_ID, instance HOME $NEW_HOME, output $OUT, dry-run $DRY_RUN, ui mode $UI_MODE"
if [ "$NEW_HOME" = "$BASE" ] || [ "$NEW_HOME" = "$BASE/lab-injection" ]; then
  die 3 "refusing to use an existing store HOME as the instance HOME: $NEW_HOME"
fi
case $NEW_HOME in /*) ;; *) die 3 "instance HOME must be an absolute path: $NEW_HOME" ;; esac
case $OUT in /*) ;; *) die 3 "output directory must be an absolute path: $OUT" ;; esac
if [ -e "$NEW_HOME" ]; then die 3 "instance HOME already exists, refusing to touch it: $NEW_HOME (choose another snapshot id)"; fi
if [ -e "$OUT" ]; then die 3 "output directory already exists, refusing to touch it: $OUT (choose another snapshot id)"; fi
for _f in hyphae-env.sh seed.sh seed-demo-facts.sh; do
  if [ ! -f "$BASE/$_f" ]; then die 3 "missing $BASE/$_f (copy every scripts/hyphae-container/*.sh into the container first)"; fi
done
if ! grep -q 'hyphae_store()' "$BASE/hyphae-env.sh"; then die 3 "$BASE/hyphae-env.sh does not define hyphae_store"; fi
for _t in hyphae awk sed grep sort tr wc cmp cp chmod find stat sha256sum timeout nohup mktemp readlink date sleep; do
  if ! command -v "$_t" > /dev/null 2>&1; then
    if [ "$DRY_RUN" = yes ]; then echo "(dry-run) note: $_t is not on PATH here; the container must have it"
    else die 3 "required tool not found on PATH: $_t"; fi
  fi
done
if [ "$DRY_RUN" = no ]; then
  TMP=$(mktemp -d /tmp/capture-snapshot.XXXXXX)
  run mkdir -p "$CAP"
  : > "$LOG"
  LOG_READY=yes
  log "created before this log existed: $CAP (mkdir -p) and this file; scratch directory $TMP (removed at exit)"
else
  TMP=/tmp/capture-snapshot.dry-run
  run mkdir -p "$CAP"
fi
SESSION_START=$(utc)
log "step 0: session start; snapshot id $SNAPSHOT_ID; project $PROJECT; expected records $EXPECTED_RECORDS; ui mode $UI_MODE"
log "step 0: instance HOME $NEW_HOME (data $DATA_DIR, credentials $CRED_DIR); output $OUT"
for _h in "--version" "agent ui --help" "agent setup --help" "proof verify --help"; do
  # shellcheck disable=SC2086
  run_to "$TMP/help.txt" hyphae $_h || die 3 "hyphae $_h failed"
  if [ "$DRY_RUN" = no ]; then
    log "hyphae $_h:"
    while IFS= read -r _l; do log "  | $_l"; done < "$TMP/help.txt"
  fi
done

# ================================================================ step 1
log "step 1: create the instance HOME and set up Agent Memory there (no service unit)"
run mkdir -p "$NEW_HOME"
# The setup output is not inspected by this script and whether it prints
# credential material is unverified, so it goes to the scratch directory,
# which cleanup removes at exit on every path (success or failure): nothing of
# it stays in the container or reaches the output directory. On failure the
# bounded stderr is logged by run_to; the stdout never is.
run_to "$TMP/setup.out" env HOME="$NEW_HOME" hyphae agent setup --no-service || die 3 "hyphae agent setup --no-service failed for $NEW_HOME"
if [ "$DRY_RUN" = no ]; then
  log "setup stdout: $(stat -c %s "$TMP/setup.out") bytes, discarded with the scratch directory at exit (never logged, never copied out)"
  if [ ! -d "$DATA_DIR" ]; then die 3 "setup did not create the data directory where hyphae-env.sh expects it: $DATA_DIR"; fi
  if [ ! -d "$CRED_DIR" ]; then die 3 "setup did not create the credentials directory where hyphae-env.sh expects it: $CRED_DIR"; fi
  log "credential files created by setup (names only, never read):"
  find "$CRED_DIR" -type f | sort | while IFS= read -r _f; do log "  $_f ($(stat -c '%s bytes, mode %a' "$_f"))"; done
  list_home_files "after setup"
  _size_before_configure=$(du -b -s "$DATA_DIR" | cut -f1)
  # hyphae-env.sh for the NEW HOME: paths, the canonical socket (printed by
  # "hyphae agent configure opencode", outside the data directory) and
  # hyphae_store. "agent configure" may write an MCP host configuration under
  # the new HOME; whatever it created is listed below, and the data directory
  # must not have changed.
  HYPHAE_HOME_DIR=$NEW_HOME
  export HYPHAE_HOME_DIR
  log "+ . $BASE/hyphae-env.sh   (runs: env HOME=$NEW_HOME hyphae agent configure opencode)"
  . "$BASE/hyphae-env.sh"
  list_home_files "after hyphae agent configure"
  if [ "$(du -b -s "$DATA_DIR" | cut -f1)" != "$_size_before_configure" ]; then
    die 3 "hyphae agent configure changed the data directory size ($_size_before_configure -> $(du -b -s "$DATA_DIR" | cut -f1) bytes); it must not touch the store"
  fi
  if [ "$HYPHAE_DATA" != "$DATA_DIR" ]; then die 3 "hyphae-env.sh derives the data directory as $HYPHAE_DATA, this script expected $DATA_DIR"; fi
  if [ "$HYPHAE_READER_KEY" != "$CRED_DIR/memory-reader.key" ] || [ "$HYPHAE_WRITER_KEY" != "$CRED_DIR/memory-writer.key" ]; then
    die 3 "hyphae-env.sh derives credential paths outside $CRED_DIR"
  fi
  if [ -z "$HYPHAE_SOCK" ]; then die 3 "no canonical endpoint for $NEW_HOME (hyphae agent configure printed none)"; fi
  plain "$HYPHAE_SOCK" || die 3 "the canonical endpoint is not one plain path (hyphae agent configure printed several endpoint lines or unexpected characters)"
  case $HYPHAE_SOCK in
    "$HYPHAE_DATA"/*) die 3 "the canonical endpoint $HYPHAE_SOCK lies inside the data directory; proofs would fail (see hyphae-env.sh)" ;;
    "$NEW_HOME"/*) ;;
    /*) log "note: the canonical endpoint $HYPHAE_SOCK lies outside the instance HOME (it is removed by the daemon on SIGINT)" ;;
    *) die 3 "the canonical endpoint is not an absolute path: $HYPHAE_SOCK" ;;
  esac
  if [ -S "$HYPHAE_SOCK" ]; then die 3 "a socket already exists at the canonical endpoint $HYPHAE_SOCK: another daemon would answer the captures"; fi
  if [ ! -f "$HYPHAE_WRITER_KEY" ] || [ ! -f "$HYPHAE_READER_KEY" ]; then die 3 "credential files are missing under $CRED_DIR"; fi
  if ! command -v hyphae_store > /dev/null 2>&1; then die 3 "hyphae_store is not defined after sourcing hyphae-env.sh"; fi
  log "instance paths: data $HYPHAE_DATA; endpoint $HYPHAE_SOCK; writer key $HYPHAE_WRITER_KEY; reader key $HYPHAE_READER_KEY"
  log "data directory size after setup: $(du -b -s "$HYPHAE_DATA" | cut -f1) bytes"
else
  HYPHAE_DATA=$DATA_DIR
  HYPHAE_READER_KEY=$CRED_DIR/memory-reader.key
  HYPHAE_WRITER_KEY=$CRED_DIR/memory-writer.key
  HYPHAE_SOCK="<endpoint printed by: env HOME=$NEW_HOME hyphae agent configure opencode>"
fi

# ================================================================ step 2
log "step 2: start the daemon for $NEW_HOME only (serve.sh procedure; socket outside the data directory)"
start_daemon

# ================================================================ step 3
log "step 3: seed the $EXPECTED_RECORDS synthetic $PROJECT facts (writer credential, hyphae_store from hyphae-env.sh); nothing is ever forgotten here"
# Texts copied verbatim from scripts/hyphae-container/seed.sh (records 1-2) and
# scripts/hyphae-container/seed-demo-facts.sh (records 3-7), in that order,
# with the same kinds. seed_one refuses a text that is not found verbatim in
# the copies of those scripts next to this one.
seed_one constraint "Constraint: outbound retries must be capped at 3 attempts with jitter; unbounded retries caused the 2026-08 outage."
seed_one fact "Fact: payment client timeouts correlate with retry bursts above 5 attempts."
seed_one fact "Fact: the payment-client default RetryPolicy is max_attempts = 1 with jitter_ms = 0, so a single transient upstream timeout surfaces to users as a payment timeout."
seed_one fact "Fact: send_with_retries in payment-client retries only SendError::Timeout, up to RetryPolicy.max_attempts; upstream errors are returned at once without a retry."
seed_one fact "Fact: the payment-client test constraint_cap_is_three_attempts fails for any RetryPolicy with max_attempts above 3 or with jitter_ms equal to 0."
seed_one decision "Decision (2026-09 payments review): fix payment timeouts by setting the payment-client RetryPolicy to max_attempts = 3 with jitter_ms of at least 50; raising the attempt cap above 3 is not allowed."
seed_one fact "Fact: in the 2026-08 outage, retry bursts from payment clients amplified an upstream slowdown into a full outage."
if [ "$DRY_RUN" = no ]; then
  if [ "$SEEDED" -ne "$EXPECTED_RECORDS" ]; then die 3 "seeded $SEEDED records, expected $EXPECTED_RECORDS"; fi
  log "seeded $SEEDED records; data directory size now $(du -b -s "$HYPHAE_DATA" | cut -f1) bytes"
fi
if [ "$UI_MODE" = direct ]; then
  log "ui mode direct: stopping the daemon before the agent ui captures"
  stop_daemon
fi

# ================================================================ step 4
log "step 4: proven captures through hyphae agent ui ($CONTROL_SCHEMA)"
capture "$LIST_ID" list "$LIST_ARGS" "$EXPECTED_RECORDS" 1
capture "$ISOLATION_ID" recall "$ISOLATION_ARGS" 0 2
if [ "$DRY_RUN" = no ]; then
  # Two different requests seal two different proofs; identical bytes would
  # mean one capture was answered with the other's proof.
  if cmp -s "$CAP/$LIST_ID.proof" "$CAP/$ISOLATION_ID.proof"; then die 4 "the proofs of $LIST_ID and $ISOLATION_ID are byte-identical"; fi
fi

# ================================================================ step 5
log "step 5: offline verification of every capture against the shared anchor"
verify_capture "$LIST_ID"
verify_capture "$ISOLATION_ID"

# ================================================================ step 6
log "step 6: record the Hyphae version and the sha256 of the binary that produced the captures"
run_to "$OUT/hyphae-version.txt" hyphae --version || die 3 "hyphae --version failed"
if [ "$DRY_RUN" = no ]; then
  HYPHAE_BIN=$(readlink -f "$(command -v hyphae)")
  run_to "$OUT/hyphae-binary.sha256" sha256sum -- "$HYPHAE_BIN" || die 3 "sha256sum of $HYPHAE_BIN failed"
  HYPHAE_VERSION=$(tr -d '\r\n' < "$OUT/hyphae-version.txt")
  BINARY_SHA256=$(cut -c1-64 < "$OUT/hyphae-binary.sha256")
  is_hex64 "$BINARY_SHA256" || die 3 "unexpected sha256sum output in $OUT/hyphae-binary.sha256"
  plain "$HYPHAE_VERSION" || die 3 "unexpected hyphae --version output"
  log "hyphae version: $HYPHAE_VERSION; binary $HYPHAE_BIN sha256 $BINARY_SHA256"
else
  HYPHAE_BIN="<readlink -f of the hyphae on PATH>"
  log "+ sha256sum -- $HYPHAE_BIN > $OUT/hyphae-binary.sha256"
  HYPHAE_VERSION="<hyphae --version>"
  BINARY_SHA256="<sha256>"
fi

# ================================================================ step 7
log "step 7: stop the daemon, destroy the credentials of $NEW_HOME, make its data directory read-only"
stop_daemon
if [ "$DRY_RUN" = yes ]; then
  log "+ find $CRED_DIR -type f   (list what will be removed; names and sizes only)"
  log "+ rm -rf -- $CRED_DIR"
  log "+ chmod -R a-w -- $DATA_DIR"
else
  if [ ! -d "$CRED_DIR" ]; then die 6 "credentials directory not found at $CRED_DIR (layout changed; destroy by hand)"; fi
  log "removing every credential file of the sealed instance (names and sizes only, never read):"
  find "$CRED_DIR" -type f | sort | while IFS= read -r _f; do log "  removing $_f ($(stat -c '%s bytes, mode %a' "$_f"))"; done
  run rm -rf -- "$CRED_DIR"
  if [ -e "$CRED_DIR" ]; then die 6 "credentials directory still present after removal: $CRED_DIR"; fi
  log "removed: $CRED_DIR (the directory and every file listed above)"
  if [ -S "$HYPHAE_SOCK" ]; then
    log "WARNING: the socket $HYPHAE_SOCK still exists although the daemon was stopped; left in place (not a credential)"
  fi
  run chmod -R a-w -- "$HYPHAE_DATA"
  log "data directory made read-only: $HYPHAE_DATA ($(du -b -s "$HYPHAE_DATA" | cut -f1) bytes)"
  log "kept in the container, private: the original proof and witness files under${PROOF_DIRS:- <no proof directory recorded>}, the daemon log $NEW_HOME/serve.log; the setup stdout is removed with the scratch directory at exit"
  list_home_files "after credential destruction"
fi

# ================================================================ step 8
log "step 8: write $OUT/captures.index (one JSON object per line) and $OUT/session.json"
if [ "$DRY_RUN" = yes ]; then
  log "(dry-run) would write one line per capture: {id, operation, arguments, files{response,proof,witness,verify}, sha256{response,proof,witness,verify}, proof_blake3, anchor_hex, memory_ids, captured_at, verified_at}"
  log "(dry-run) would write session.json: {schema, snapshot_id, project, captured_at, finished_at, anchor_hex, record_count, hyphae_version, binary_path, binary_sha256, control_schema, ui_mode, instance_home}"
else
  : > "$OUT/captures.index"
  for _cid in $CAPTURE_IDS; do
    IFS='|' read -r _mid _mop _margs _mblake _manchor _mstart _mrsha _mpsha _mwsha < "$TMP/$_cid.meta"
    _mverified=$(cat "$TMP/$_cid.verified_at")
    _mvsha=$(cat "$TMP/$_cid.vsha")
    is_hex64 "$_mrsha" && is_hex64 "$_mpsha" && is_hex64 "$_mwsha" && is_hex64 "$_mvsha" || die 4 "capture $_cid: a recorded sha256 is missing (internal error)"
    _mids=$(awk 'BEGIN { ORS = "" } NR > 1 { printf "," } { printf "\"%s\"", $0 }' "$TMP/$_cid.ids")
    printf '{"id":"%s","operation":"%s","arguments":%s,"files":{"response":"captures/%s.response.json","proof":"captures/%s.proof","witness":"captures/%s.witness","verify":"captures/%s.verify.json"},"sha256":{"response":"%s","proof":"%s","witness":"%s","verify":"%s"},"proof_blake3":"%s","anchor_hex":"%s","memory_ids":[%s],"captured_at":"%s","verified_at":"%s"}\n' \
      "$_mid" "$_mop" "$_margs" "$_mid" "$_mid" "$_mid" "$_mid" "$_mrsha" "$_mpsha" "$_mwsha" "$_mvsha" "$_mblake" "$_manchor" "$_mids" "$_mstart" "$_mverified" >> "$OUT/captures.index"
  done
  printf '{"schema":"hyphae-consensus-capture-session-v1","snapshot_id":"%s","project":"%s","captured_at":"%s","finished_at":"%s","anchor_hex":"%s","record_count":%s,"hyphae_version":"%s","binary_path":"%s","binary_sha256":"%s","control_schema":"%s","ui_mode":"%s","instance_home":"%s"}\n' \
    "$SNAPSHOT_ID" "$PROJECT" "$SESSION_START" "$(utc)" "$ANCHOR" "$EXPECTED_RECORDS" "$HYPHAE_VERSION" "$HYPHAE_BIN" "$BINARY_SHA256" "$CONTROL_SCHEMA" "$UI_MODE" "$NEW_HOME" > "$OUT/session.json"
  log "captures.index:"
  while IFS= read -r _l; do log "  $_l"; done < "$OUT/captures.index"
  log "output files:"
  find "$OUT" -type f | sort | while IFS= read -r _f; do log "  $_f ($(stat -c %s "$_f") bytes)"; done
fi
log "DONE. Next, on the host (PowerShell): docker cp hyphae-consensus:$OUT snapshots/ ; python -I scripts/snapshot_manifest.py build snapshots/$SNAPSHOT_ID ; python -I scripts/snapshot_manifest.py check snapshots/$SNAPSHOT_ID --reverify scripts/hyphae-docker.cmd --verifier-dir $OUT"
log "anchor_hex: ${ANCHOR:-<not captured in dry-run>}; pin it as CONSENSUS_HYPHAE_EXPECTED_ANCHOR"
