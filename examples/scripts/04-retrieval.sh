#!/usr/bin/env bash
set -euo pipefail

# Semantic retrieval over operational history
# Prerequisites: kith with events in the store (from daemon sync or local ops)

KITH="${KITH:-./target/release/kith}"

echo "=== Search operational history ==="
echo "find all nginx-related changes from this week" | $KITH

echo ""
echo "=== Fleet query ==="
echo "show me recent drift events across the fleet" | $KITH

echo ""
echo "=== Context-aware follow-up ==="
echo "which of those changes haven't been committed yet?" | $KITH
