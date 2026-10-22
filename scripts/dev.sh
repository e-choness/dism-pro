#!/usr/bin/env sh
# Thin wrapper (ADR-0003): only calls docker. Usage: scripts/dev.sh <service> [-- args]
set -eu
cd "$(dirname "$0")/.."
exec docker compose run --rm "$@"
