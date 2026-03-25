#!/usr/bin/env bash
set -euo pipefail

# Remote execution via kith-daemon
# Prerequisites: kith-daemon running on target machine, keypair exchanged

KITH="${KITH:-./target/release/kith}"

echo "=== Check remote machine status ==="
echo "what's the state of staging-1?" | $KITH

echo ""
echo "=== Direct remote command ==="
echo "run docker ps on staging-1" | $KITH

echo ""
echo "=== Fleet-wide query ==="
echo "which machines have high disk usage?" | $KITH
