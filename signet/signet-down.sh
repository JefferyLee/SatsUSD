#!/usr/bin/env bash
# Stop the signet node set (reverse order). Data persists.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

stop_pid() { # <name> <pidfile>
  local name="$1" pidfile="$2"
  if [ -f "$pidfile" ]; then
    local pid; pid="$(cat "$pidfile")"
    if kill -0 "$pid" 2>/dev/null; then
      echo "stopping $name (pid $pid)..."
      kill "$pid"
      for _ in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    fi
    rm -f "$pidfile"
  fi
}

stop_pid tapd "$SDATA/tapd.pid"
stop_pid lnd "$SDATA/lnd.pid"

# bitcoind gets a clean RPC shutdown when possible.
if scli getblockchaininfo >/dev/null 2>&1; then
  echo "stopping bitcoind..."
  scli stop >/dev/null
  for _ in $(seq 1 60); do scli getblockchaininfo >/dev/null 2>&1 || break; sleep 1; done
fi
rm -f "$SDATA/bitcoind.pid"

echo "signet node set stopped."
