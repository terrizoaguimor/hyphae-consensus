#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Gate G6, edge versus origin: fetches the same paths from the origin through
# the unix socket and from the public hostname through Cloudflare, and
# compares the bodies byte for byte. Also prints cf-cache-status for every
# path (/api and /research must never be HIT) and the security headers seen
# at the edge. Runbook section 8.4.
#
#   sudo deploy/scripts/verify-edge.sh --hostname <hostname> \
#        [--socket /run/consensus/public.sock] [--path /en/ ...] [--keep <dir>]
#
# Root (or a member of group consensus) is needed to open the socket. The
# origin refuses a request without CF-Connecting-IP (the socket serves
# cloudflared only), so the probe sends a documentation address and a
# CF-IPCountry the server's geo check accepts. Exit status: 0 when every body
# is identical, every status matches and no cf-cache-status is HIT on
# /api or /research; 1 otherwise.
set -euo pipefail

HOSTNAME_=""
SOCKET=/run/consensus/public.sock
KEEP=""
PATHS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --socket) SOCKET="$2"; shift 2 ;;
    --path) PATHS+=("$2"); shift 2 ;;
    --keep) KEEP="$2"; shift 2 ;;
    -h|--help) sed -n '3,19p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
[ -n "$HOSTNAME_" ] || { echo "--hostname is required" >&2; exit 64; }
[ -S "$SOCKET" ] || { echo "no socket at $SOCKET (is consensus-public running?)" >&2; exit 1; }
if [ ${#PATHS[@]} -eq 0 ]; then
  PATHS=(/ /en/ /es/ /pt-BR/ /fr/ /this-path-does-not-exist
         /api/v1/health /api/v1/capabilities /api/v1/scenarios
         /research/MANIFEST.json /research/SHA256SUMS)
fi

work="${KEEP:-$(mktemp -d)}"
mkdir -p "$work"
cleanup() { if [ -z "$KEEP" ]; then rm -rf "$work"; fi; }
trap cleanup EXIT

status=0
printf '%-32s %-6s %-6s %-7s %-12s %s\n' PATH ORIGIN EDGE BODIES CACHE-STATUS NOTE
for p in "${PATHS[@]}"; do
  safe="$(printf '%s' "$p" | tr '/' '_')"
  o_code="$(curl -sS --unix-socket "$SOCKET" -o "$work/origin$safe" -D "$work/origin$safe.h" \
            -w '%{http_code}' -H "Host: $HOSTNAME_" -H 'CF-Connecting-IP: 203.0.113.10' \
            -H 'CF-IPCountry: US' "http://localhost$p" || echo ERR)"
  e_code="$(curl -sS -o "$work/edge$safe" -D "$work/edge$safe.h" -w '%{http_code}' \
            "https://$HOSTNAME_$p" || echo ERR)"
  if cmp -s "$work/origin$safe" "$work/edge$safe"; then bodies=equal; else bodies=DIFFER; status=1; fi
  cache="$(grep -i '^cf-cache-status:' "$work/edge$safe.h" | tr -d '\r' | awk '{print $2}' | tail -n 1)"
  note=""
  case "$p" in
    /api/*|/research/*)
      if [ "$cache" = "HIT" ]; then note="HIT on an uncacheable route"; status=1; fi ;;
  esac
  if [ "$o_code" != "$e_code" ]; then note="${note:+$note; }status differs"; status=1; fi
  printf '%-32s %-6s %-6s %-7s %-12s %s\n' "$p" "$o_code" "$e_code" "$bodies" "${cache:-none}" "$note"
done

echo
echo "== security headers at the edge (first path):"
first="$(printf '%s' "${PATHS[0]}" | tr '/' '_')"
grep -i -E '^(content-security-policy|x-content-type-options|referrer-policy|x-frame-options|cache-control):' \
  "$work/edge$first.h" | tr -d '\r' | sed 's/^/   /' || true
echo
if [ "$status" -eq 0 ]; then
  echo "G6 edge-vs-origin: PASS"
else
  echo "G6 edge-vs-origin: FAIL (see DIFFER, HIT or 'status differs' above)"
fi
if [ -n "$KEEP" ]; then echo "responses kept under $work"; fi
exit "$status"
