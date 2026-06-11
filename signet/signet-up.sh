#!/usr/bin/env bash
# Bring up the SatUSD signet node set: bitcoind + lnd + tapd as host
# processes. Idempotent: skips any daemon already responding. Data
# persists under signet/data.
#
# Unlike the regtest devnet this does NOT block on chain sync —
# signet IBD takes a while on first run. Daemons settle as the sync
# completes; watch with `make signet-status`.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

if [ ! -x "$BIN/lnd" ] || [ ! -x "$BIN/tapd" ]; then
  echo "missing lnd/tapd binaries — run devnet/fetch-binaries.sh first" >&2
  exit 1
fi

mkdir -p "$SDATA/bitcoind" "$SDATA/lnd" "$SDATA/tapd" "$SLOG"

# Render configs (tapd needs absolute lnd macaroon/tls paths).
cp "$SCONF/bitcoin.conf" "$SDATA/bitcoind/bitcoin.conf"
cp "$SCONF/lnd.conf" "$SDATA/lnd/lnd.conf"
sed "s#__ROOT__#$SIGNET_ROOT#g" "$SCONF/tapd.conf.template" > "$SDATA/tapd/tapd.conf"

wait_for() { # <description> <max_tries> <cmd...>
  local desc="$1" tries="$2"; shift 2
  for _ in $(seq 1 "$tries"); do "$@" >/dev/null 2>&1 && return 0; sleep 1; done
  echo "timeout waiting for $desc" >&2; return 1
}

# --- 1. bitcoind ---
if ! scli getblockchaininfo >/dev/null 2>&1; then
  echo "starting bitcoind (signet)..."
  nohup bitcoind -datadir="$SDATA/bitcoind" -conf="$SDATA/bitcoind/bitcoin.conf" \
    > "$SLOG/bitcoind.log" 2>&1 &
  echo $! > "$SDATA/bitcoind.pid"
fi
wait_for bitcoind 30 scli getblockchaininfo

# On-chain wallet for faucet coins / funding (independent of lnd's).
scli -named createwallet wallet_name=signet >/dev/null 2>&1 \
  || scli loadwallet signet >/dev/null 2>&1 || true

# --- 2. lnd ---
# --noseedbackup: signet-only convenience (scaffold; signet coins
# carry no value). Replace with a real seed before anything that
# must survive a disk loss.
if ! slncli getinfo >/dev/null 2>&1; then
  echo "starting lnd (signet)..."
  nohup "$BIN/lnd" --lnddir="$SDATA/lnd" --noseedbackup > "$SLOG/lnd.log" 2>&1 &
  echo $! > "$SDATA/lnd.pid"
fi
wait_for "lnd rpc" 90 slncli getinfo

# --- 3. tapd ---
# tapd waits internally for lnd to be synced; safe to start now.
if ! stapcli getinfo >/dev/null 2>&1; then
  echo "starting tapd (signet)..."
  nohup "$BIN/tapd" --tapddir="$SDATA/tapd" --configfile="$SDATA/tapd/tapd.conf" \
    > "$SLOG/tapd.log" 2>&1 &
  echo $! > "$SDATA/tapd.pid"
fi

echo
"$SIG/signet-status.sh"
echo
echo "first run? IBD is in progress — watch with: make signet-status"
echo "faucet coins:  swcli getnewaddress  →  https://signetfaucet.com"
