@echo off
rem SPDX-License-Identifier: Apache-2.0
rem Forwards every Hyphae invocation to the Linux binary inside the
rem hyphae-consensus Docker container, preserving stdio (used as
rem CONSENSUS_HYPHAE_BIN). The Windows 4.0.0 Agent Memory surface does
rem not start on this host; the Linux build in the container works.
docker exec -i hyphae-consensus hyphae %*
