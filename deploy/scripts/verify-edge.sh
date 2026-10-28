#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Gate G6, edge versus origin: fetches the same paths from the origin (the
# public unix socket on the server) and from the public hostname (through
# Cloudflare), and compares the bodies byte for byte. Also prints
# cf-cache-status for every path (/api and /research must never be HIT) and
# the security headers seen at the edge. Runbook section 8.4.
#
# Runs on the OWNER MACHINE. The server's egress policy (runbook 2.4) reaches
# Cloudflare's tunnel edge, Meta and Siteverify only, not Cloudflare's HTTP
# edge, so the server cannot fetch its own hostname; the origin half is
# fetched over SSH instead:
#
#   deploy/scripts/verify-edge.sh --hostname <hostname> --ssh <server> \
#        [--no-sudo] [--socket /run/consensus/public.sock] [--path /en/ ...] [--keep <dir>]
#
# --ssh runs `sudo -n curl --unix-socket ...` on the server: sudo must not
# ask for a password there (sudo -n fails instead of prompting). Otherwise
# add your SSH user to the group consensus-sock for the duration of the
# checks (gpasswd -a <user> consensus-sock; -d afterwards) and pass --no-sudo.
# Without --ssh the origin is read from a local socket, which only works on a
# host that can also reach the edge (not this server).
# The origin refuses a request without CF-Connecting-IP (the socket serves
# cloudflared only), so the probe sends a documentation address and a
# CF-IPCountry the server's geo check accepts. Staging behind Cloudflare
# Access: export CF_ACCESS_CLIENT_ID and CF_ACCESS_CLIENT_SECRET (a service
# token with a Service Auth policy, runbook 7.9); the edge requests then
# carry the two Access headers. Exit status: 0 when every body is identical,
# every status matches and no cf-cache-status is HIT on /api or /research;
# 1 otherwise.
set -euo pipefail

HOSTNAME_=""
SSH_HOST=""
USE_SUDO=1
SOCKET=/run/consensus/public.sock
KEEP=""
PATHS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --hostname) HOSTNAME_="$2"; shift 2 ;;
    --ssh) SSH_HOST="$2"; shift 2 ;;
    --no-sudo) USE_SUDO=0; shift ;;
    --socket) SOCKET="$2"; shift 2 ;;
    --path) PATHS+=("$2"); shift 2 ;;
    --keep) KEEP="$2"; shift 2 ;;
    -h|--help) sed -n '3,31p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done
[ -n "$HOSTNAME_" ] || { echo "--hostname is required" >&2; exit 64; }
if [ -z "$SSH_HOST" ] && [ ! -S "$SOCKET" ]; then
  echo "no socket at $SOCKET; from the owner machine pass --ssh <server>" >&2; exit 1
fi
if [ ${#PATHS[@]} -eq 0 ]; then
  PATHS=(/ /en/ /es/ /pt-BR/ /fr/ /this-path-does-not-exist
         /api/v1/health /api/v1/capabilities /api/v1/scenarios
         /research/MANIFEST.json /research/SHA256SUMS)
fi
access=()
if [ -n "${CF_ACCESS_CLIENT_ID:-}" ] && [ -n "${CF_ACCESS_CLIENT_SECRET:-}" ]; then
  access=(-H "CF-Access-Client-Id: $CF_ACCESS_CLIENT_ID" -H "CF-Access-Client-Secret: $CF_ACCESS_CLIENT_SECRET")
  echo "edge requests carry the Cloudflare Access service-token headers"
fi

work="${KEEP:-$(mktemp -d)}"
mkdir -p "$work"
cleanup() { if [ -z "$KEEP" ]; then rm -rf "$work"; fi; }
trap cleanup EXIT

sudo_prefix=""
if [ "$USE_SUDO" -eq 1 ]; then sudo_prefix="sudo -n "; fi

# origin_fetch <path> <body file>: prints the HTTP status (or ERR). One
# request: the body, then curl's %{http_code} (exactly three bytes), split
# with head -c -3 / tail -c 3 so the body stays byte-exact (GNU head: "with
# the leading '-', print all but the last NUM bytes").
origin_fetch() {
  local raw="$2.raw"
  if [ -n "$SSH_HOST" ]; then
    ssh "$SSH_HOST" "${sudo_prefix}curl -sS --unix-socket '$SOCKET' -o - -w '%{http_code}' \
        -H 'Host: $HOSTNAME_' -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' 'http://localhost$1'" \
        >"$raw" 2>"$raw.err" || true
  else
    curl -sS --unix-socket "$SOCKET" -o - -w '%{http_code}' -H "Host: $HOSTNAME_" \
        -H 'CF-Connecting-IP: 203.0.113.10' -H 'CF-IPCountry: US' "http://localhost$1" \
        >"$raw" 2>"$raw.err" || true
  fi
  if [ "$(wc -c <"$raw")" -lt 3 ]; then
    : >"$2"; echo ERR; return 0
  fi
  head -c -3 "$raw" >"$2"
  tail -c 3 "$raw"
}

status=0
printf '%-32s %-6s %-6s %-7s %-12s %s\n' PATH ORIGIN EDGE BODIES CACHE-STATUS NOTE
for p in "${PATHS[@]}"; do
  safe="$(printf '%s' "$p" | tr '/' '_')"
  o_code="$(origin_fetch "$p" "$work/origin$safe")"
  e_code="$(curl -sS -o "$work/edge$safe" -D "$work/edge$safe.h" -w '%{http_code}' \
            "${access[@]}" "https://$HOSTNAME_$p" 2>"$work/edge$safe.err")" || e_code=ERR
  note=""
  if [ "$o_code" = ERR ]; then
    note="origin: $(head -n 1 "$work/origin$safe.raw.err" 2>/dev/null || true)"; status=1
  fi
  if [ "$e_code" = ERR ]; then
    note="${note:+$note; }edge: $(head -n 1 "$work/edge$safe.err" 2>/dev/null || true)"; status=1
  fi
  if cmp -s "$work/origin$safe" "$work/edge$safe"; then bodies=equal; else bodies=DIFFER; status=1; fi
  cache="$(grep -i '^cf-cache-status:' "$work/edge$safe.h" 2>/dev/null | tr -d '\r' | awk '{print $2}' | tail -n 1 || true)"
  case "$p" in
    /api/*|/research/*)
      if [ "$cache" = "HIT" ]; then note="${note:+$note; }HIT on an uncacheable route"; status=1; fi ;;
  esac
  if [ "$o_code" != "$e_code" ]; then note="${note:+$note; }status differs"; status=1; fi
  if grep -qi -E '^location: .*cloudflareaccess\.com' "$work/edge$safe.h" 2>/dev/null; then
    note="${note:+$note; }Cloudflare Access login redirect: set CF_ACCESS_CLIENT_ID/CF_ACCESS_CLIENT_SECRET"
  elif { [ "$e_code" = 401 ] || [ "$e_code" = 403 ]; } && [ "$o_code" != "$e_code" ]; then
    note="${note:+$note; }edge refused (Access service token missing or wrong, or a WAF rule)"
  fi
  printf '%-32s %-6s %-6s %-7s %-12s %s\n' "$p" "$o_code" "$e_code" "$bodies" "${cache:-none}" "$note"
done

echo
echo "== security headers at the edge (first path):"
first="$(printf '%s' "${PATHS[0]}" | tr '/' '_')"
grep -i -E '^(content-security-policy|x-content-type-options|referrer-policy|x-frame-options|cache-control):' \
  "$work/edge$first.h" 2>/dev/null | tr -d '\r' | sed 's/^/   /' || true
echo
if [ "$status" -eq 0 ]; then
  echo "G6 edge-vs-origin: PASS"
else
  echo "G6 edge-vs-origin: FAIL (see DIFFER, HIT, ERR or 'status differs' above)"
fi
if [ -n "$KEEP" ]; then echo "responses kept under $work"; fi
exit "$status"
