#!/usr/bin/env bash
# Bring up the SatUSD regtest devnet: bitcoind + lnd + tapd as host processes.
# Idempotent: skips any daemon already responding. Data persists under devnet/data.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

mkdir -p "$DATA/bitcoind" "$DATA/lnd" "$DATA/tapd" "$LOG"

# Render configs (tapd needs absolute lnd macaroon/tls paths).
cp "$CONF/bitcoin.conf" "$DATA/bitcoind/bitcoin.conf"
cp "$CONF/lnd.conf" "$DATA/lnd/lnd.conf"
sed "s#__ROOT__#$DEVNET_ROOT#g" "$CONF/tapd.conf.template" > "$DATA/tapd/tapd.conf"

wait_for() { # <description> <max_tries> <cmd...>
  local desc="$1" tries="$2"; shift 2
  for _ in $(seq 1 "$tries"); do "$@" >/dev/null 2>&1 && return 0; sleep 1; done
  echo "timeout waiting for $desc" >&2; return 1
}

# --- 1. bitcoind ---
if ! bcli getblockchaininfo >/dev/null 2>&1; then
  echo "starting bitcoind..."
  nohup bitcoind -datadir="$DATA/bitcoind" -conf="$DATA/bitcoind/bitcoin.conf" \
    > "$LOG/bitcoind.log" 2>&1 &
  echo $! > "$DATA/bitcoind.pid"
fi
wait_for bitcoind 30 bcli getblockchaininfo

# Funded wallet + mature coinbase.
bcli -named createwallet wallet_name=regtest >/dev/null 2>&1 \
  || bcli loadwallet regtest >/dev/null 2>&1 || true
ADDR="$(bwcli getnewaddress)"
if [ "$(bcli getblockcount)" -lt 101 ]; then
  echo "mining 101 blocks for coinbase maturity..."
  bwcli generatetoaddress 101 "$ADDR" >/dev/null
fi

# --- 2. lnd ---
if ! dlncli getinfo >/dev/null 2>&1; then
  echo "starting lnd..."
  nohup "$BIN/lnd" --lnddir="$DATA/lnd" --noseedbackup > "$LOG/lnd.log" 2>&1 &
  echo $! > "$DATA/lnd.pid"
fi
wait_for lnd 90 dlncli getinfo
echo "waiting for lnd to sync to chain..."
wait_for "lnd chain sync" 90 bash -c "source '$DEV/env.sh'; dlncli getinfo | grep -qE '\"synced_to_chain\": *true'"

# --- 3. tapd ---
if ! dtapcli getinfo >/dev/null 2>&1; then
  echo "starting tapd..."
  nohup "$BIN/tapd" --tapddir="$DATA/tapd" --configfile="$DATA/tapd/tapd.conf" \
    > "$LOG/tapd.log" 2>&1 &
  echo $! > "$DATA/tapd.pid"
fi
wait_for tapd 90 dtapcli getinfo

echo
echo "devnet up:"
echo "  bitcoind height : $(bcli getblockcount)"
echo "  lnd pubkey      : $(dlncli getinfo | grep -o '\"identity_pubkey\": *\"[^\"]*\"' | head -1)"
echo "  tapd version    : $(dtapcli getinfo | grep -o '\"version\": *\"[^\"]*\"' | head -1)"
echo "logs: $LOG/{bitcoind,lnd,tapd}.log"
