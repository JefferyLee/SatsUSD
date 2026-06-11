#!/usr/bin/env bash
# Report signet node-set state: bitcoind sync progress, lnd chain
# sync, tapd readiness.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

echo "signet status:"
if info="$(scli getblockchaininfo 2>/dev/null)"; then
  blocks="$(echo "$info" | grep -o '"blocks": *[0-9]*' | grep -o '[0-9]*')"
  headers="$(echo "$info" | grep -o '"headers": *[0-9]*' | grep -o '[0-9]*')"
  progress="$(echo "$info" | grep -o '"verificationprogress": *[0-9.]*' | grep -o '[0-9.]*')"
  echo "  bitcoind : blocks $blocks / headers $headers (progress $progress)"
else
  echo "  bitcoind : DOWN"
fi

if lninfo="$(slncli getinfo 2>/dev/null)"; then
  synced="$(echo "$lninfo" | grep -o '"synced_to_chain": *[a-z]*' | grep -o '[a-z]*$')"
  height="$(echo "$lninfo" | grep -o '"block_height": *[0-9]*' | grep -o '[0-9]*')"
  echo "  lnd      : height $height, synced_to_chain=$synced"
else
  echo "  lnd      : DOWN (or still waiting on bitcoind sync)"
fi

if tapinfo="$(stapcli getinfo 2>/dev/null)"; then
  tsync="$(echo "$tapinfo" | grep -o '"sync_to_chain": *[a-z]*' | grep -o '[a-z]*$')"
  echo "  tapd     : up (sync_to_chain=$tsync)"
else
  echo "  tapd     : DOWN (or still waiting on lnd)"
fi
