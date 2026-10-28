#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Judge-lane invite codes (ADR-0007; crates/consensus-api/src/live/judge.rs).
# Generates N codes of 140 random bits each from /dev/urandom: 28 symbols of
# the 32-symbol alphabet 0-9 A-Z without I, L, O and U, in seven groups of
# four joined by hyphens. PRINTS THEM ONCE on stdout, for the owner to put in
# the submission's testing instructions, and writes only their SHA-256
# digests to the digests file the server reads
# (CONSENSUS_LIVE_JUDGE_CODES_FILE): a comment line, then one 64-character
# lowercase hex digest per line, owned root:consensus, mode 0640. A code is
# never written to disk, never passed as an argument to another program and
# never logged by this script.
#
# A judge enters a code as printed, hyphens included (lowercase is accepted
# too): the server hashes what it receives (trimmed), then its capitals, and
# compares them with these digests.
#
#   sudo deploy/scripts/judge-codes.sh [--count N] [--out FILE] [--force]
#
#   --count N   codes to generate, 1 to 50 (default 3)
#   --out FILE  the digests file (default /etc/consensus/judge-codes.sha256)
#   --force     replace an existing digests file, which revokes every code it
#               held; without it an existing file is never overwritten
#
# Revocation is editing the digests file (delete the code's line) and
# restarting consensus-public; the kill switch stops every lane at once.
# Runbook sections 1 and 4.3.
set -euo pipefail

COUNT=3
OUT=/etc/consensus/judge-codes.sha256
FORCE=0
FILE_GROUP=consensus
ALPHABET=0123456789ABCDEFGHJKMNPQRSTVWXYZ
SYMBOLS=28
GROUP_LEN=4

usage() {
  echo "usage: judge-codes.sh [--count N] [--out FILE] [--force]" >&2
  exit 64
}

while [ $# -gt 0 ]; do
  case "$1" in
    --count) [ $# -ge 2 ] || usage; COUNT="$2"; shift 2 ;;
    --out) [ $# -ge 2 ] || usage; OUT="$2"; shift 2 ;;
    --force) FORCE=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done

case "$COUNT" in
  ''|*[!0-9]*) echo "--count must be a whole number from 1 to 50" >&2; exit 64 ;;
esac
if [ "$COUNT" -lt 1 ] || [ "$COUNT" -gt 50 ]; then
  echo "--count must be a whole number from 1 to 50" >&2
  exit 64
fi
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo): the digests file is installed root:$FILE_GROUP 0640" >&2; exit 1; }
getent group "$FILE_GROUP" >/dev/null || { echo "group $FILE_GROUP not found (deploy/scripts/install.sh creates it)" >&2; exit 1; }
[ -r /dev/urandom ] || { echo "/dev/urandom is not readable" >&2; exit 1; }
for tool in od sha256sum mktemp; do
  command -v "$tool" >/dev/null || { echo "$tool not found" >&2; exit 1; }
done

refuse_existing() {
  if [ -e "$OUT" ] && [ "$FORCE" -ne 1 ]; then
    echo "$OUT exists: refusing to overwrite it (every code it holds would be revoked); pass --force to replace it" >&2
    exit 1
  fi
}
refuse_existing
dir="$(dirname -- "$OUT")"
[ -d "$dir" ] || { echo "no directory $dir" >&2; exit 1; }

# The digests are assembled next to their destination, readable by root
# only until they are complete, then moved into place in one rename.
umask 077
tmp="$(mktemp "$dir/.judge-codes.XXXXXX")"
trap 'rm -f -- "$tmp"' EXIT

# One code: SYMBOLS bytes from /dev/urandom, each mapped to one of the 32
# symbols (byte % 32; 256 is a multiple of 32, so every symbol is equally
# likely: 5 bits per symbol, 140 bits per code), a hyphen every GROUP_LEN.
new_code() {
  local bytes byte code="" n=0
  bytes="$(od -An -N"$SYMBOLS" -tu1 /dev/urandom)"
  for byte in $bytes; do
    if [ "$n" -gt 0 ] && [ $((n % GROUP_LEN)) -eq 0 ]; then
      code="$code-"
    fi
    code="$code${ALPHABET:$((byte % 32)):1}"
    n=$((n + 1))
  done
  if [ "$n" -ne "$SYMBOLS" ]; then
    echo "short read from /dev/urandom ($n of $SYMBOLS bytes)" >&2
    exit 1
  fi
  printf '%s' "$code"
}

codes=()
{
  echo "# Judge-lane invite codes: SHA-256 of each code, one per line (judge-codes.sh, $(date -u +%Y-%m-%dT%H:%M:%SZ), $COUNT codes)"
  for _ in $(seq "$COUNT"); do
    code="$(new_code)"
    codes+=("$code")
    # printf is a builtin: the code reaches sha256sum on stdin, never argv.
    printf '%s' "$code" | sha256sum | cut -d' ' -f1
  done
} >"$tmp"

digests="$(grep -c -E '^[0-9a-f]{64}$' "$tmp" || true)"
if [ "$digests" != "$COUNT" ]; then
  echo "expected $COUNT digests in $tmp, found $digests: nothing installed" >&2
  exit 1
fi
chown "root:$FILE_GROUP" "$tmp"
chmod 0640 "$tmp"
refuse_existing
mv -f -- "$tmp" "$OUT"
trap - EXIT

echo "Judge-lane invite codes ($COUNT). They are shown ONCE and stored nowhere else:"
echo "copy them into the submission's testing instructions now. A judge enters a code"
echo "exactly as shown, hyphens included."
echo
for code in "${codes[@]}"; do
  echo "  $code"
done
echo
echo "Digests written to $OUT ($(stat -c '%U:%G %a' "$OUT"), $COUNT lines)."
echo "Set CONSENSUS_LIVE_JUDGE_CODES_FILE=$OUT in /etc/consensus/public.env if it is"
echo "not set, then restart consensus-public: codes are read at startup only."
