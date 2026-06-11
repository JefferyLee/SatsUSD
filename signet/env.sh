#!/usr/bin/env bash
# Shared paths and CLI wrappers for the SatUSD signet node set.
# Source this for interactive use (works under bash or zsh):
#   source signet/env.sh
#
# Binaries are shared with the devnet (devnet/bin — run
# devnet/fetch-binaries.sh once). Ports are distinct from the
# regtest devnet so both can run side by side.

if [ -n "${BASH_SOURCE:-}" ]; then
  SIG="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
elif [ -d "./signet" ]; then
  SIG="$(cd ./signet && pwd)"
elif [ -d "../signet" ]; then
  SIG="$(cd ../signet && pwd)"
else
  SIG="$PWD"
fi
SIGNET_ROOT="$(dirname "$SIG")"
BIN="$SIGNET_ROOT/devnet/bin"
SDATA="$SIG/data"
SLOG="$SIG/logs"
SCONF="$SIG/conf"
export SIGNET_ROOT SIG BIN SDATA SLOG SCONF

# bitcoin-cli against the signet node.
scli() {
  bitcoin-cli -signet -datadir="$SDATA/bitcoind" \
    -rpcuser=satusd -rpcpassword=satusd -rpcport=38332 "$@"
}

# bitcoin-cli bound to the "signet" wallet (faucet coins land here).
swcli() { scli -rpcwallet=signet "$@"; }

# lnd / tapd CLIs (shared binaries, signet network, signet data dirs).
slncli() { "$BIN/lncli" --network=signet --lnddir="$SDATA/lnd" --rpcserver=127.0.0.1:10010 "$@"; }
stapcli() { "$BIN/tapcli" --network=signet --rpcserver=127.0.0.1:10030 --tapddir="$SDATA/tapd" "$@"; }
