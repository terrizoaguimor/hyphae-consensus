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
#      (/hyphae-home) or the lab store (/hyphae-home/lab-injection);
#   2. starts a daemon for that HOME only, socket OUTSIDE the data directory,
#      exactly as serve.sh does (the writer path used for seeding is the MCP
#      adapter, which is a client of the daemon, so the daemon must exist
#      before the seeding step);
#   3. seeds exactly the 7 synthetic demo-repo facts, the texts copied verbatim
#      from seed.sh and seed-demo-facts.sh and cross-checked against those
#      files; nothing is ever forgotten in this store;
#   4. captures a proven list (demo-repo, limit 64) and a proven empty recall
#      (other-repo) through "hyphae agent ui", keeping each exact response and
#      a verbatim copy of the proof and the witness it names;
#   5. verifies every capture with "hyphae proof verify" against the one
#      shared anchor and keeps the verifier report;
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
#   --inspect    no session: print what this script extracts from a saved
#                "agent ui" response or "proof verify" report (debugging aid)
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
# destruction could not be confirmed.
set -eu

# ---------------------------------------------------------------- arguments
usage() {
  echo "usage: sh capture-snapshot.sh [--dry-run] [--ui-direct] <snapshot-id>"
  echo "       sh capture-snapshot.sh --inspect <json file>"
}
MODE=capture
DRY_RUN=no
UI_MODE=daemon
SNAPSHOT_ID=""
INSPECT_FILE=""
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
    -h|--help) usage; exit 0 ;;
    -*) usage >&2; exit 2 ;;
    *)
      if [ -n "$SNAPSHOT_ID" ]; then usage >&2; exit 2; fi
      SNAPSHOT_ID=$1
      ;;
  esac
  shift
done
if [ "$MODE" = inspect ]; then
  SNAPSHOT_ID=inspect
elif [ -z "$SNAPSHOT_ID" ]; then
  usage >&2; exit 2
fi
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

TMP=""
LOG_READY=""
DAEMON_PID=""
ANCHOR=""
CAPTURE_IDS=""
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
# outside [A-Za-z0-9_-] and any trailing data is a hard failure (exit 3), so
# an unexpected shape stops the session instead of producing a partial index.
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
function parse_string(   s, rest, c, esc) {
  if (substr(buf, pos, 1) != "\"") fail("expected a string")
  pos++
  s = ""
  while (1) {
    if (pos > n) fail("unterminated string")
    rest = substr(buf, pos)
    if (match(rest, /["\\]/) == 0) fail("unterminated string")
    s = s substr(rest, 1, RSTART - 1)
    pos += RSTART - 1
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
stop_daemon() {
  if [ -z "$DAEMON_PID" ]; then return 0; fi
  log "+ kill -INT $DAEMON_PID   (the daemon of $NEW_HOME; SIGINT makes it remove its socket)"
  kill -INT "$DAEMON_PID" 2>/dev/null || log "daemon $DAEMON_PID was already gone"
  _i=0
  while [ -S "$HYPHAE_SOCK" ]; do
    _i=$((_i + 1))
    if [ "$_i" -gt 60 ]; then
      log "WARNING: the socket is still present after 30 s; sending SIGTERM"
      kill -TERM "$DAEMON_PID" 2>/dev/null || true
      break
    fi
    sleep 0.5
  done
  wait "$DAEMON_PID" 2>/dev/null || true
  DAEMON_PID=""
  log "daemon stopped"
}
cleanup() {
  _status=$?
  if [ -n "$DAEMON_PID" ]; then
    log "cleanup: stopping the daemon this session started"
    stop_daemon
  fi
  if [ -n "$TMP" ] && [ "$DRY_RUN" = no ]; then rm -rf -- "$TMP"; fi
  if [ "$_status" -ne 0 ] && [ "$DRY_RUN" = no ] && [ "$MODE" = capture ]; then
    log "session FAILED (exit $_status). Nothing was deleted. The instance HOME $NEW_HOME may still hold credentials: review it, destroy them by hand (docs/architecture/sealed-snapshot.md, section Failure handling), or rerun with a new snapshot id. Output so far: $OUT"
  fi
}
trap cleanup EXIT

# ---------------------------------------------------------------- seeding
# One memory through the writer credential, with hyphae_store from
# hyphae-env.sh (the MCP writer path seed.sh and seed-demo-facts.sh use;
# identity is derived from project + layer + text, so identical text gives the
# same memory id as the recorded research runs). The text must appear
# verbatim in seed.sh or seed-demo-facts.sh next to this script.
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
  case $_resp in
    *'"isError":true'*|*'"isError": true'*|*tool-error*) die 3 "hyphae_store reported an error: $_resp" ;;
  esac
  log "  store response: $_resp"
  SEEDED=$((SEEDED + 1))
}

# ---------------------------------------------------------------- captures
capture() { # capture <id> <operation> <arguments json> <expected record count>
  _cid=$1; _cop=$2; _cargs=$3; _cexp=$4
  _creq="{\"schema\":\"$CONTROL_SCHEMA\",\"id\":\"capture-$_cid\",\"operation\":\"$_cop\",\"arguments\":$_cargs}"
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
  if [ ! -s "$_ppath" ]; then die 4 "capture $_cid: the proof file named by the response is missing or empty: $_ppath"; fi
  if [ ! -s "$_wpath" ]; then die 4 "capture $_cid: the witness file named by the response is missing or empty: $_wpath"; fi
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
  elif [ "$_canchor" != "$ANCHOR" ]; then
    die 4 "capture $_cid: anchor_hex $_canchor differs from the anchor of the first capture $ANCHOR (the store changed between captures)"
  fi
  run cp -- "$_ppath" "$CAP/$_cid.proof"
  run cp -- "$_wpath" "$CAP/$_cid.witness"
  cmp -s "$_ppath" "$CAP/$_cid.proof" || die 4 "capture $_cid: the proof copy differs from $_ppath"
  cmp -s "$_wpath" "$CAP/$_cid.witness" || die 4 "capture $_cid: the witness copy differs from $_wpath"
  log "capture $_cid: $_count record(s); proof $(wc -c < "$CAP/$_cid.proof" | tr -d ' ') bytes (blake3 $_pblake); witness $(wc -c < "$CAP/$_cid.witness" | tr -d ' ') bytes"
  for _k in expired_filtered retrieval_mode semantic_status snapshot.visible_csn snapshot.root_digest snapshot.logical_time_micros; do
    if _v=$(leaf_any "$_leaves" "$_root.$_k"); then log "capture $_cid: $_k = $_v"; fi
  done
  while IFS= read -r _id; do log "capture $_cid: memory id $_id"; done < "$_ids"
  printf '%s|%s|%s|%s|%s|%s\n' "$_cid" "$_cop" "$_cargs" "$_pblake" "$_canchor" "$_cstart" > "$TMP/$_cid.meta"
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
  printf '%s\n' "$_vstart" > "$TMP/$_vid.verified_at"
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
# The setup output stays INSIDE the instance HOME (outside its data directory),
# never in the output directory that leaves the container: it is not inspected
# by this script, and whether it prints credential material is unverified.
run_to "$NEW_HOME/setup.out" env HOME="$NEW_HOME" hyphae agent setup --no-service || die 3 "hyphae agent setup --no-service failed for $NEW_HOME"
if [ "$DRY_RUN" = no ]; then
  log "setup output kept in $NEW_HOME/setup.out ($(stat -c %s "$NEW_HOME/setup.out") bytes; not copied out)"
  if [ ! -d "$DATA_DIR" ]; then die 3 "setup did not create the data directory where hyphae-env.sh expects it: $DATA_DIR"; fi
  if [ ! -d "$CRED_DIR" ]; then die 3 "setup did not create the credentials directory where hyphae-env.sh expects it: $CRED_DIR"; fi
  log "credential files created by setup (names only, never read):"
  find "$CRED_DIR" -type f | sort | while IFS= read -r _f; do log "  $_f ($(stat -c '%s bytes, mode %a' "$_f"))"; done
  # hyphae-env.sh for the NEW HOME: paths, the canonical socket (printed by
  # "hyphae agent configure", outside the data directory) and hyphae_store.
  HYPHAE_HOME_DIR=$NEW_HOME
  export HYPHAE_HOME_DIR
  . "$BASE/hyphae-env.sh"
  if [ "$HYPHAE_DATA" != "$DATA_DIR" ]; then die 3 "hyphae-env.sh derives the data directory as $HYPHAE_DATA, this script expected $DATA_DIR"; fi
  if [ "$HYPHAE_READER_KEY" != "$CRED_DIR/memory-reader.key" ] || [ "$HYPHAE_WRITER_KEY" != "$CRED_DIR/memory-writer.key" ]; then
    die 3 "hyphae-env.sh derives credential paths outside $CRED_DIR"
  fi
  if [ -z "$HYPHAE_SOCK" ]; then die 3 "no canonical endpoint for $NEW_HOME (hyphae agent configure printed none)"; fi
  case $HYPHAE_SOCK in
    "$HYPHAE_DATA"/*) die 3 "the canonical endpoint $HYPHAE_SOCK lies inside the data directory; proofs would fail (see hyphae-env.sh)" ;;
    /*) ;;
    *) die 3 "the canonical endpoint is not an absolute path: $HYPHAE_SOCK" ;;
  esac
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
capture "$LIST_ID" list "$LIST_ARGS" "$EXPECTED_RECORDS"
capture "$ISOLATION_ID" recall "$ISOLATION_ARGS" 0

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
  log "kept in the container, private: the original proof and witness files under $NEW_HOME/.local/state/hyphae/proofs/, $NEW_HOME/setup.out, $NEW_HOME/serve.log"
fi

# ================================================================ step 8
log "step 8: write $OUT/captures.index (one JSON object per line) and $OUT/session.json"
if [ "$DRY_RUN" = yes ]; then
  log "(dry-run) would write one line per capture: {id, operation, arguments, files{response,proof,witness,verify}, proof_blake3, anchor_hex, memory_ids, captured_at, verified_at}"
  log "(dry-run) would write session.json: {schema, snapshot_id, project, captured_at, finished_at, anchor_hex, record_count, hyphae_version, binary_path, binary_sha256, control_schema, ui_mode, instance_home}"
else
  : > "$OUT/captures.index"
  for _cid in $CAPTURE_IDS; do
    IFS='|' read -r _mid _mop _margs _mblake _manchor _mstart < "$TMP/$_cid.meta"
    _mverified=$(cat "$TMP/$_cid.verified_at")
    _mids=$(awk 'BEGIN { ORS = "" } NR > 1 { printf "," } { printf "\"%s\"", $0 }' "$TMP/$_cid.ids")
    printf '{"id":"%s","operation":"%s","arguments":%s,"files":{"response":"captures/%s.response.json","proof":"captures/%s.proof","witness":"captures/%s.witness","verify":"captures/%s.verify.json"},"proof_blake3":"%s","anchor_hex":"%s","memory_ids":[%s],"captured_at":"%s","verified_at":"%s"}\n' \
      "$_mid" "$_mop" "$_margs" "$_mid" "$_mid" "$_mid" "$_mid" "$_mblake" "$_manchor" "$_mids" "$_mstart" "$_mverified" >> "$OUT/captures.index"
  done
  printf '{"schema":"hyphae-consensus-capture-session-v1","snapshot_id":"%s","project":"%s","captured_at":"%s","finished_at":"%s","anchor_hex":"%s","record_count":%s,"hyphae_version":"%s","binary_path":"%s","binary_sha256":"%s","control_schema":"%s","ui_mode":"%s","instance_home":"%s"}\n' \
    "$SNAPSHOT_ID" "$PROJECT" "$SESSION_START" "$(utc)" "$ANCHOR" "$EXPECTED_RECORDS" "$HYPHAE_VERSION" "$HYPHAE_BIN" "$BINARY_SHA256" "$CONTROL_SCHEMA" "$UI_MODE" "$NEW_HOME" > "$OUT/session.json"
  log "captures.index:"
  while IFS= read -r _l; do log "  $_l"; done < "$OUT/captures.index"
  log "output files:"
  find "$OUT" -type f | sort | while IFS= read -r _f; do log "  $_f ($(stat -c %s "$_f") bytes)"; done
fi
log "DONE. Next, on the host: docker cp hyphae-consensus:$OUT snapshots/ ; python -I scripts/snapshot_manifest.py build snapshots/$SNAPSHOT_ID ; python -I scripts/snapshot_manifest.py check snapshots/$SNAPSHOT_ID"
log "anchor_hex: ${ANCHOR:-<not captured in dry-run>}; pin it as CONSENSUS_HYPHAE_EXPECTED_ANCHOR"
