# syntax=docker/dockerfile:1
#
# SPDX-License-Identifier: Apache-2.0
#
# Hyphae Consensus (Meta Global AI Developer Hackathon 2026): the Rust CLI.
#
# WHAT THIS IMAGE IS FOR. Two things, both offline: running the scripted
# reference scenario (the deterministic doubles, SIMULATED proofs) to
# AwaitingHumanApproval, and re-verifying the archived runs of research/
# (`verify`: hash chain, policy, receipt re-derivation; read only).
# WHAT IT IS NOT. It is not the full portal. The public server binds only a
# unix socket or a loopback TCP address, so a container cannot publish it, and
# the web app builds only through apps/web/scripts/in-docker.sh. The image holds
# no web export and calls neither Meta nor Hyphae. For the portal, see
# docs/running-locally.md ("Web app and public server").
#
# The binary is built with the toolchain pinned in rust-toolchain.toml (1.99.1)
# and --locked. The image holds no credential, no .env file and no git history,
# and runs as the non-root user hyphae (uid 10001). research/ is copied to
# /opt/hyphae/research, so `verify` and eval/verify-archive.sh work in it.
#
#   docker build -t hyphae-consensus .
#   docker run --rm hyphae-consensus                          # scripted run, stops at AwaitingHumanApproval
#   docker run --rm hyphae-consensus --approve                # also records a human approval (nothing is executed)
#   docker run --rm hyphae-consensus verify /opt/hyphae/research/2026-10-27/live-battery-2026-10-27-final.jsonl
#   docker run --rm hyphae-consensus --help                   # the usage
#
# The scripted demo keeps its log in ./scripted-demo/events.jsonl under the
# working directory (/home/hyphae), which is discarded with the container.

FROM rust:1.99.1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p consensus-api

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 hyphae
COPY --from=build /src/target/release/consensus-api /usr/local/bin/consensus-api
COPY research /opt/hyphae/research
USER hyphae
WORKDIR /home/hyphae
ENTRYPOINT ["consensus-api"]
