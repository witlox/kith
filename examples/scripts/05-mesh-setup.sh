#!/usr/bin/env bash
set -euo pipefail

# Mesh setup — initialize and connect machines
# Run on each machine that should join the mesh

KITH="${KITH:-./target/release/kith}"
DAEMON="${DAEMON:-./target/release/kith-daemon}"

echo "=== Step 1: Initialize (generates keypair + config) ==="
$KITH --init

echo ""
echo "=== Step 2: Show your public key (share with other machines) ==="
echo "Your public key is in ~/.config/kith/keypair.pub"
cat ~/.config/kith/keypair.pub 2>/dev/null || echo "(run kith --init first)"

echo ""
echo "=== Step 3: Start the daemon ==="
echo "Run on each machine:"
echo "  RUST_LOG=info $DAEMON"

echo ""
echo "=== Step 4: Connect from kith shell ==="
echo "Once daemons are running and Nostr signaling is configured,"
echo "machines discover each other automatically."
echo "  $KITH"
echo "  kith> what machines are in the mesh?"
