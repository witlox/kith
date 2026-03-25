#!/usr/bin/env bash
set -euo pipefail

# Apply/commit/rollback workflow — change management with commit windows
# Prerequisites: kith-daemon running, kith connected

KITH="${KITH:-./target/release/kith}"

echo "=== Apply a change (enters pending state) ==="
echo "apply the nginx config update on prod-1, back up /etc/nginx/" | $KITH

echo ""
echo "=== Check what's pending ==="
echo "what changes are pending?" | $KITH

echo ""
echo "=== Commit (finalize the change) ==="
echo "commit the pending nginx change" | $KITH

echo ""
echo "=== Or rollback instead ==="
# echo "rollback the pending change" | $KITH
