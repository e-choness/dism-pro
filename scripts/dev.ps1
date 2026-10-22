# Thin wrapper (ADR-0003): only calls docker. Usage: scripts\dev.ps1 <service> [-- args]
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
docker compose run --rm @args
exit $LASTEXITCODE
