#!/usr/bin/env bash
# Stop the devnet (tapd, lnd, bitcoind). Data under devnet/data is preserved.
# Pass --wipe to also delete chain/wallet/tapd data for a clean restart.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

stop() { # <name>
  local name="$1"
  local pidfile="$DATA/$name.pid"
  if [ -f "$pidfile" ]; then
    local pid; pid="$(cat "$pidfile")"
    if kill -0 "$pid" 2>/dev/null; then
      echo "stopping $name (pid $pid)..."
      kill "$pid" 2>/dev/null || true
    fi
    rm -f "$pidfile"
  fi
}

stop tapd
stop lnd
stop bitcoind

if [ "${1:-}" = "--wipe" ]; then
  echo "wiping devnet data..."
  rm -rf "$DATA/bitcoind" "$DATA/lnd" "$DATA/tapd"
fi
echo "devnet down"
