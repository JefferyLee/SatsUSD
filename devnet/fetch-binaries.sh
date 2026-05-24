#!/usr/bin/env bash
# Download the pinned lnd + tapd binaries into devnet/bin (darwin-arm64).
# Binaries are not committed; run this once on a fresh checkout.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

LND_VER="v0.20.1-beta"
TAPD_VER="v0.7.2"
PLATFORM="darwin-arm64"

mkdir -p "$BIN" "$DEV/_dl"
cd "$DEV/_dl"

echo "downloading lnd $LND_VER ($PLATFORM)..."
curl -fsSL -o lnd.tar.gz \
  "https://github.com/lightningnetwork/lnd/releases/download/$LND_VER/lnd-$PLATFORM-$LND_VER.tar.gz"
echo "downloading tapd $TAPD_VER ($PLATFORM)..."
curl -fsSL -o tapd.tar.gz \
  "https://github.com/lightninglabs/taproot-assets/releases/download/$TAPD_VER/taproot-assets-$PLATFORM-$TAPD_VER.tar.gz"

tar xzf lnd.tar.gz
tar xzf tapd.tar.gz
cp "lnd-$PLATFORM-$LND_VER/lnd" "lnd-$PLATFORM-$LND_VER/lncli" "$BIN/"
cp "taproot-assets-$PLATFORM-$TAPD_VER/tapd" "taproot-assets-$PLATFORM-$TAPD_VER/tapcli" "$BIN/"
chmod +x "$BIN"/*
xattr -dr com.apple.quarantine "$BIN"/* 2>/dev/null || true

cd "$DEV"
rm -rf _dl
echo "installed lnd $LND_VER + tapd $TAPD_VER into $BIN"
