#!/usr/bin/env bash
set -euo pipefail

# Basic kith usage — shows pass-through vs intent classification
# Prerequisites: kith binary built, config at ~/.config/kith/config.toml

KITH="${KITH:-./target/release/kith}"

echo "=== Pass-through commands (zero latency, no LLM) ==="
echo 'echo "hello from kith"' | $KITH
echo 'ls -la' | $KITH
echo 'git status' | $KITH

echo ""
echo "=== Escape hatch (forced pass-through) ==="
echo 'run: docker ps' | $KITH

echo ""
echo "=== Intent (routed to LLM) ==="
echo "what's using the most disk space?" | $KITH

echo ""
echo "=== Single command mode ==="
$KITH "summarize the git log for this week"
