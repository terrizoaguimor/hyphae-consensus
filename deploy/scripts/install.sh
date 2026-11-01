#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Hyphae Consensus: server preparation and installation (runbook sections 2-5).
# Run as root on the Ubuntu 24.04 server, from a copy of the repository's
# deploy/ directory:
#
#   sudo deploy/scripts/install.sh --binary ./consensus-api --sha256 <digest> [--yes]
#   deploy/scripts/install.sh --binary ./consensus-api --sha256 <digest> --dry-run
#   deploy/scripts/install.sh --binary ./consensus-api --sha256 <digest> --no-systemd
#
# --dry-run checks the templates and the binary's digest, then prints every
# change it would make (users, groups, directories, files, systemd calls)
# and makes none. It needs no root, but run it with sudo all the same: only
# root sees inside /etc/consensus (0750), so without root an existing
# public.env cannot be seen. --no-systemd is for a host or container
# without a running systemd (the deploy-day rehearsal in ubuntu:24.04,
# docs/deployment/deploy-day.md): everything is installed, but
# /run/consensus is created by hand with the tmpfiles.d line's mode and
# owners instead of `systemd-tmpfiles --create`, and `systemctl
# daemon-reload` is skipped; the systemd package need not be installed
# (/etc/tmpfiles.d is created if missing). On the real server use neither.
#
# What it does (idempotent; nothing is started, no firewall rule is touched):
#   1. system users and groups: consensus, hyphae, cloudflared, and the socket
#      group consensus-sock (cloudflared's ONLY extra group; it is removed from
#      the group consensus if an earlier layout put it there)
#   2. directories and modes under /etc/consensus, /var/lib/consensus
#      (ledger/staging and ledger/public, snapshots 0700), /opt/hyphae,
#      /etc/cloudflared, and /run/consensus (2750 consensus:consensus-sock)
#      through /etc/tmpfiles.d/consensus.conf
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
DRY_RUN=0
NO_SYSTEMD=0

usage() {
  cat <<'EOF'
usage: install.sh --binary <path> --sha256 <hex digest> [--deploy-dir <dir>] [--yes] [--dry-run | --no-systemd]
  --binary      consensus-api built in the pinned ubuntu-24.04 container
  --sha256      sha256 of that binary as printed on the build host
  --deploy-dir  the repository's deploy/ directory (default: this script's parent)
  --yes         do not ask before replacing an existing binary or unit file
  --dry-run     print every change, make none (no root needed; run it with
                sudo to see inside /etc/consensus)
  --no-systemd  install without a running systemd (a rehearsal container;
                the systemd package is optional): /run/consensus by hand,
                no systemd-tmpfiles, no daemon-reload
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --binary) BINARY="$2"; shift 2 ;;
    --sha256) EXPECTED_SHA256="$2"; shift 2 ;;
    --deploy-dir) DEPLOY_DIR="$2"; shift 2 ;;
    --yes) ASSUME_YES=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --no-systemd) NO_SYSTEMD=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 64 ;;
  esac
done

if [ "$DRY_RUN" -eq 1 ] && [ "$NO_SYSTEMD" -eq 1 ]; then
  echo "--dry-run and --no-systemd exclude each other" >&2; exit 64
fi
if [ "$DRY_RUN" -eq 0 ]; then
  [ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }
fi
if [ -z "$BINARY" ] || [ -z "$EXPECTED_SHA256" ]; then usage; exit 64; fi
[ -f "$BINARY" ] || { echo "binary not found: $BINARY" >&2; exit 1; }
for f in systemd/consensus-public.service systemd/consensus-operator.service \
         systemd/cloudflared.service.d/10-consensus.conf tmpfiles.d/consensus.conf \
         cloudflared/config.yml.example \
         etc/consensus/public.env.example etc/consensus/operator.env.example; do
  [ -f "$DEPLOY_DIR/$f" ] || { echo "missing template: $DEPLOY_DIR/$f" >&2; exit 1; }
done

[ "$DRY_RUN" -eq 1 ] && echo "== DRY RUN: nothing below is changed; every change is printed as 'would run'"

say() {
  # say <text>: a status line about a change; in a dry run it reads
  # "would: <text>", because the change was only printed.
  if [ "$DRY_RUN" -eq 1 ]; then echo "   would: $*"; else echo "   $*"; fi
}

run() {
  # run <command...>: runs it, or with --dry-run prints it instead.
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '   would run:'; printf ' %q' "$@"; printf '\n'
  else
    "$@"
  fi
}

confirm() {
  # confirm <question>: true when --yes was given or the operator answers y.
  # A dry run asks nothing: it reports the question and carries on.
  if [ "$ASSUME_YES" -eq 1 ]; then return 0; fi
  if [ "$DRY_RUN" -eq 1 ]; then echo "   would ask: $1"; return 0; fi
  local answer
  read -r -p "$1 [y/N] " answer
  [ "$answer" = "y" ] || [ "$answer" = "Y" ]
}

ensure_group() { getent group "$1" >/dev/null || run groupadd --system "$1"; }
ensure_user() {
  # ensure_user <name> <home> <group>
  if ! getent passwd "$1" >/dev/null; then
    run useradd --system --gid "$3" --home-dir "$2" --no-create-home --shell /usr/sbin/nologin "$1"
  fi
}
in_group() { id -nG "$1" 2>/dev/null | tr ' ' '\n' | grep -qx "$2"; }

echo "== users and groups"
ensure_group consensus;      ensure_user consensus   /var/lib/consensus consensus
ensure_group hyphae;         ensure_user hyphae      /opt/hyphae        hyphae
ensure_group cloudflared;    ensure_user cloudflared /nonexistent       cloudflared
ensure_group consensus-sock
# cloudflared reaches the socket through consensus-sock, the group that owns
# nothing but /run/consensus and public.sock (the drop-in sets
# SupplementaryGroups=consensus-sock; the membership below is the fallback
# for a cloudflared started by hand). It must NOT be in the group consensus,
# which reads /etc/consensus and, via the service user, the witnesses.
in_group cloudflared consensus-sock || run usermod -a -G consensus-sock cloudflared
if in_group cloudflared consensus; then
  run gpasswd -d cloudflared consensus
  say "removed cloudflared from the group consensus (restart cloudflared to apply)"
fi
in_group consensus consensus-sock && echo "   note: consensus is in consensus-sock; not needed (the socket's owner is consensus)"
# Nobody here may be in the docker group (gate G9: no daemon, no docker).
for u in consensus hyphae cloudflared; do
  if in_group "$u" docker; then
    echo "refusing to continue: user $u is in the docker group" >&2
    exit 1
  fi
done

echo "== directories"
run install -d -o root      -g consensus -m 0750 /etc/consensus
run install -d -o root      -g root      -m 0700 /etc/consensus/credentials
run install -d -o consensus -g consensus -m 0750 /var/lib/consensus
for d in runs ledger research web; do
  run install -d -o consensus -g consensus -m 0750 "/var/lib/consensus/$d"
done
# One ledger directory per phase (runbook 4.5): staging spend is accounted
# apart and the kill-switch rehearsal trips the staging ledger; go-live
# switches CONSENSUS_LIVE_LEDGER_DIR to the empty production ledger.
for d in ledger/staging ledger/public; do
  run install -d -o consensus -g consensus -m 0750 "/var/lib/consensus/$d"
done
run install -d -o root -g root -m 0700 /var/lib/consensus/ledger-archive
# The snapshot tree holds private witnesses: the service user and nobody else.
run install -d -o consensus -g consensus -m 0700 /var/lib/consensus/snapshots
run install -d -o hyphae -g hyphae -m 0755 /opt/hyphae /opt/hyphae/bin
run install -d -o root   -g cloudflared -m 0750 /etc/cloudflared
# /run/consensus: 2750 consensus:consensus-sock, recreated at every boot by
# systemd-tmpfiles-setup.service from the tmpfiles.d entry; applied now too.
run install -d -o root -g root -m 0755 /etc/tmpfiles.d
run install -o root -g root -m 0644 "$DEPLOY_DIR/tmpfiles.d/consensus.conf" /etc/tmpfiles.d/consensus.conf
if [ "$NO_SYSTEMD" -eq 1 ]; then
  # No systemd-tmpfiles here: the same directory, mode and owners as the
  # tmpfiles.d line (d /run/consensus 2750 consensus consensus-sock).
  install -d -o consensus -g consensus-sock -m 0750 /run/consensus
  chmod 2750 /run/consensus
  echo "   --no-systemd: /run/consensus created by hand (at boot on a real server: systemd-tmpfiles)"
else
  run systemd-tmpfiles --create /etc/tmpfiles.d/consensus.conf
fi
if [ "$DRY_RUN" -eq 0 ]; then
  echo "   /run/consensus: $(stat -c '%A %U:%G' /run/consensus) (expected drwxr-s--- consensus:consensus-sock)"
fi

echo "== binary"
actual="$(sha256sum "$BINARY" | cut -d' ' -f1)"
if [ "$actual" != "$EXPECTED_SHA256" ]; then
  echo "sha256 mismatch: expected $EXPECTED_SHA256, got $actual" >&2
  exit 1
fi
if [ -e /usr/local/bin/consensus-api ] && ! cmp -s "$BINARY" /usr/local/bin/consensus-api; then
  confirm "replace the existing /usr/local/bin/consensus-api?" || { echo "kept the existing binary"; exit 1; }
fi
run install -o root -g root -m 0755 "$BINARY" /usr/local/bin/consensus-api
say "/usr/local/bin/consensus-api sha256 $actual (restart consensus-public if it is running)"

install_unit() {
  # install_unit <source> <destination>
  if [ -e "$2" ] && ! cmp -s "$1" "$2"; then
    confirm "overwrite $2?" || { echo "   kept $2"; return 0; }
  fi
  run install -o root -g root -m 0644 "$1" "$2"
  say "$2"
}

echo "== systemd units"
run install -d -o root -g root -m 0755 /etc/systemd/system
install_unit "$DEPLOY_DIR/systemd/consensus-public.service"   /etc/systemd/system/consensus-public.service
install_unit "$DEPLOY_DIR/systemd/consensus-operator.service" /etc/systemd/system/consensus-operator.service
run install -d -o root -g root -m 0755 /etc/systemd/system/cloudflared.service.d
install_unit "$DEPLOY_DIR/systemd/cloudflared.service.d/10-consensus.conf" \
             /etc/systemd/system/cloudflared.service.d/10-consensus.conf

echo "== configuration templates (an existing file is never overwritten)"
if [ -d /etc/consensus ] && ! [ -x /etc/consensus ]; then
  echo "   cannot inspect /etc/consensus without root: whether public.env and operator.env exist is unknown (run the dry run with sudo)"
fi
for f in public.env operator.env; do
  if [ -e "/etc/consensus/$f" ]; then
    echo "   /etc/consensus/$f exists; left as is (template: $DEPLOY_DIR/etc/consensus/$f.example)"
  else
    run install -o root -g consensus -m 0640 "$DEPLOY_DIR/etc/consensus/$f.example" "/etc/consensus/$f"
    say "/etc/consensus/$f installed from the template: fill in every <placeholder>"
  fi
done
if [ -e /etc/cloudflared/config.yml ]; then
  echo "   /etc/cloudflared/config.yml exists; left as is (template: $DEPLOY_DIR/cloudflared/config.yml.example)"
else
  run install -o root -g cloudflared -m 0640 "$DEPLOY_DIR/cloudflared/config.yml.example" /etc/cloudflared/config.yml
  say "/etc/cloudflared/config.yml installed from the template: fill in <TUNNEL-UUID> and <hostname>"
fi

if [ "$NO_SYSTEMD" -eq 1 ]; then
  echo "   --no-systemd: systemctl daemon-reload skipped"
else
  run systemctl daemon-reload
fi
if [ "$DRY_RUN" -eq 1 ]; then
  echo "== dry run complete: nothing was changed"
  exit 0
fi
cat <<'EOF'
== done. Next steps (runbook sections 3 to 6):
   - copy web/, research/ and the snapshot directory into /var/lib/consensus
     and check their checksums (sha256sum -c); the snapshot directory stays 0700
   - install the Hyphae 4.0.0 binary under /opt/hyphae (owned by hyphae)
   - create the credential files in /etc/consensus/credentials (root 0600):
     meta.key, turnstile.secret, operator.token
   - fill /etc/consensus/public.env, /etc/consensus/operator.env and
     /etc/cloudflared/config.yml; copy the tunnel credentials file to
     /etc/cloudflared/<TUNNEL-UUID>.json (root:cloudflared 0640)
   - systemctl enable --now consensus-public; systemctl restart cloudflared
   - least privilege check (runbook 5.1): sudo -u cloudflared cat
     /etc/consensus/public.env must be refused; the socket must be reachable
EOF
