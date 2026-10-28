#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Hyphae Consensus: server preparation and installation (runbook sections 2-5).
# Run as root on the Ubuntu 24.04 server, from a copy of the repository's
# deploy/ directory:
#
#   sudo deploy/scripts/install.sh --binary ./consensus-api --sha256 <digest> [--yes]
#
# What it does (idempotent; nothing is started, no firewall rule is touched):
#   1. system users and groups: consensus, hyphae, cloudflared (in group consensus)
#   2. directories and modes under /etc/consensus, /var/lib/consensus, /opt/hyphae,
#      /etc/cloudflared
#   3. the consensus-api binary -> /usr/local/bin after a sha256 check against
#      the digest printed on the build host
#   4. the unit files and the cloudflared drop-in -> /etc/systemd/system
#   5. the env and cloudflared templates -> /etc/consensus and /etc/cloudflared
#      (an existing file is never overwritten)
#   6. systemctl daemon-reload
# Destructive steps (replacing an existing binary or unit file) ask first
# unless --yes is given. No secret is read or written here: the credential
# files are created by hand (runbook 4.2).
set -euo pipefail

DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY=""
EXPECTED_SHA256=""
ASSUME_YES=0

usage() {
  cat <<'EOF'
usage: install.sh --binary <path> --sha256 <hex digest> [--deploy-dir <dir>] [--yes]
  --binary      consensus-api built in the pinned ubuntu-24.04 container
  --sha256      sha256 of that binary as printed on the build host
  --deploy-dir  the repository's deploy/ directory (default: this script's parent)
  --yes         do not ask before replacing an existing binary or unit file
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --binary) BINARY="$2"; shift 2 ;;
    --sha256) EXPECTED_SHA256="$2"; shift 2 ;;
    --deploy-dir) DEPLOY_DIR="$2"; shift 2 ;;
    --yes) ASSUME_YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 64 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }
if [ -z "$BINARY" ] || [ -z "$EXPECTED_SHA256" ]; then usage; exit 64; fi
[ -f "$BINARY" ] || { echo "binary not found: $BINARY" >&2; exit 1; }
for f in systemd/consensus-public.service systemd/consensus-operator.service \
         systemd/cloudflared.service.d/10-consensus.conf cloudflared/config.yml.example \
         etc/consensus/public.env.example etc/consensus/operator.env.example; do
  [ -f "$DEPLOY_DIR/$f" ] || { echo "missing template: $DEPLOY_DIR/$f" >&2; exit 1; }
done

confirm() {
  # confirm <question>: true when --yes was given or the operator answers y.
  if [ "$ASSUME_YES" -eq 1 ]; then return 0; fi
  local answer
  read -r -p "$1 [y/N] " answer
  [ "$answer" = "y" ] || [ "$answer" = "Y" ]
}

ensure_group() { getent group "$1" >/dev/null || groupadd --system "$1"; }
ensure_user() {
  # ensure_user <name> <home> <group>
  if ! getent passwd "$1" >/dev/null; then
    useradd --system --gid "$3" --home-dir "$2" --no-create-home --shell /usr/sbin/nologin "$1"
  fi
}

echo "== users and groups"
ensure_group consensus;   ensure_user consensus   /var/lib/consensus consensus
ensure_group hyphae;      ensure_user hyphae      /opt/hyphae        hyphae
ensure_group cloudflared; ensure_user cloudflared /nonexistent       cloudflared
# cloudflared reaches the socket through the consensus group (drop-in
# SupplementaryGroups=consensus; the group membership below is the fallback
# for a cloudflared started by hand).
usermod -a -G consensus cloudflared
# Nobody here may be in the docker group (gate G9: no daemon, no docker).
for u in consensus hyphae cloudflared; do
  if id -nG "$u" | tr ' ' '\n' | grep -qx docker; then
    echo "refusing to continue: user $u is in the docker group" >&2
    exit 1
  fi
done

echo "== directories"
install -d -o root      -g consensus -m 0750 /etc/consensus
install -d -o root      -g root      -m 0700 /etc/consensus/credentials
install -d -o consensus -g consensus -m 0750 /var/lib/consensus
for d in runs ledger research snapshots web; do
  install -d -o consensus -g consensus -m 0750 "/var/lib/consensus/$d"
done
install -d -o hyphae -g hyphae -m 0755 /opt/hyphae /opt/hyphae/bin
install -d -o root   -g cloudflared -m 0750 /etc/cloudflared

echo "== binary"
actual="$(sha256sum "$BINARY" | cut -d' ' -f1)"
if [ "$actual" != "$EXPECTED_SHA256" ]; then
  echo "sha256 mismatch: expected $EXPECTED_SHA256, got $actual" >&2
  exit 1
fi
if [ -e /usr/local/bin/consensus-api ] && ! cmp -s "$BINARY" /usr/local/bin/consensus-api; then
  confirm "replace the existing /usr/local/bin/consensus-api?" || { echo "kept the existing binary"; exit 1; }
fi
install -o root -g root -m 0755 "$BINARY" /usr/local/bin/consensus-api
echo "   /usr/local/bin/consensus-api sha256 $actual (restart consensus-public if it is running)"

install_unit() {
  # install_unit <source> <destination>
  if [ -e "$2" ] && ! cmp -s "$1" "$2"; then
    confirm "overwrite $2?" || { echo "   kept $2"; return 0; }
  fi
  install -o root -g root -m 0644 "$1" "$2"
  echo "   $2"
}

echo "== systemd units"
install_unit "$DEPLOY_DIR/systemd/consensus-public.service"   /etc/systemd/system/consensus-public.service
install_unit "$DEPLOY_DIR/systemd/consensus-operator.service" /etc/systemd/system/consensus-operator.service
install -d -o root -g root -m 0755 /etc/systemd/system/cloudflared.service.d
install_unit "$DEPLOY_DIR/systemd/cloudflared.service.d/10-consensus.conf" \
             /etc/systemd/system/cloudflared.service.d/10-consensus.conf

echo "== configuration templates (an existing file is never overwritten)"
for f in public.env operator.env; do
  if [ -e "/etc/consensus/$f" ]; then
    echo "   /etc/consensus/$f exists; left as is (template: $DEPLOY_DIR/etc/consensus/$f.example)"
  else
    install -o root -g consensus -m 0640 "$DEPLOY_DIR/etc/consensus/$f.example" "/etc/consensus/$f"
    echo "   /etc/consensus/$f installed from the template: fill in every <placeholder>"
  fi
done
if [ -e /etc/cloudflared/config.yml ]; then
  echo "   /etc/cloudflared/config.yml exists; left as is (template: $DEPLOY_DIR/cloudflared/config.yml.example)"
else
  install -o root -g cloudflared -m 0640 "$DEPLOY_DIR/cloudflared/config.yml.example" /etc/cloudflared/config.yml
  echo "   /etc/cloudflared/config.yml installed from the template: fill in <TUNNEL-UUID> and <hostname>"
fi

systemctl daemon-reload
cat <<'EOF'
== done. Next steps (runbook sections 3 to 6):
   - copy web/, research/ and the snapshot directory into /var/lib/consensus
     and check their checksums (sha256sum -c)
   - install the Hyphae 4.0.0 binary under /opt/hyphae (owned by hyphae)
   - create the credential files in /etc/consensus/credentials (root 0600)
   - fill /etc/consensus/public.env, /etc/consensus/operator.env and
     /etc/cloudflared/config.yml; copy the tunnel credentials file to
     /etc/cloudflared/<TUNNEL-UUID>.json (root:cloudflared 0640)
   - systemctl enable --now consensus-public; systemctl restart cloudflared
EOF
