# Dot-source to configure a live Meta + Hyphae session in PowerShell:
#   . .\scripts\hyphae-container\live-env.ps1                    # demo store
#   . .\scripts\hyphae-container\live-env.ps1 /hyphae-home/lab-injection
# Sets paths only. The Meta key is read from the Windows User environment
# and never printed. The Hyphae paths are CONTAINER paths: set them from
# PowerShell, not Git Bash, which rewrites POSIX-looking env values.
param([string]$HyphaeHome = "/hyphae-home")
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$env:META_API_KEY = [Environment]::GetEnvironmentVariable("META_API_KEY", "User")
$env:CONSENSUS_HYPHAE_BIN = Join-Path $repo "scripts\hyphae-docker.cmd"
# Canonical Agent Memory socket (outside the data directory), as printed by
# `hyphae agent configure` for this instance.
$configured = docker exec -e "HOME=$HyphaeHome" hyphae-consensus hyphae agent configure opencode |
    Where-Object { $_ -match '"endpoint"' }
$env:HYPHAE_NATIVE_ENDPOINT = ($configured -replace '.*"endpoint":\s*"([^"]+)".*', '$1').Trim()
$env:HYPHAE_NATIVE_API_KEY_FILE = "$HyphaeHome/.config/hyphae/credentials/memory-reader.key"
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Write-Host "live env set for $HyphaeHome; endpoint $env:HYPHAE_NATIVE_ENDPOINT; META_API_KEY present: $([bool]$env:META_API_KEY)"
