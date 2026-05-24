#!/usr/bin/env bash
# Shared paths and CLI wrappers for the SatUSD regtest devnet.
# Source this for interactive use (works under bash or zsh):
#   source devnet/env.sh

# Resolve the devnet/ directory. Under bash use BASH_SOURCE; otherwise (zsh, or
# sourced without it) locate ./devnet or ../devnet relative to the cwd.
if [ -n "${BASH_SOURCE:-}" ]; then
  DEV="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
elif [ -d "./devnet" ]; then
  DEV="$(cd ./devnet && pwd)"
elif [ -d "../devnet" ]; then
  DEV="$(cd ../devnet && pwd)"
else
  DEV="$PWD"
fi
DEVNET_ROOT="$(dirname "$DEV")"
BIN="$DEV/bin"
DATA="$DEV/data"
LOG="$DEV/logs"
CONF="$DEV/conf"
export DEVNET_ROOT DEV BIN DATA LOG CONF

# bitcoin-cli against the devnet regtest node.
bcli() {
  bitcoin-cli -regtest -datadir="$DATA/bitcoind" \
    -rpcuser=satusd -rpcpassword=satusd -rpcport=18443 "$@"
}

# bitcoin-cli bound to the funded "regtest" wallet.
bwcli() { bcli -rpcwallet=regtest "$@"; }

# lnd / tapd CLIs (devnet binaries, regtest, devnet data dirs).
dlncli() { "$BIN/lncli" --network=regtest --lnddir="$DATA/lnd" --rpcserver=127.0.0.1:10009 "$@"; }
dtapcli() { "$BIN/tapcli" --network=regtest --rpcserver=127.0.0.1:10029 --tapddir="$DATA/tapd" "$@"; }
