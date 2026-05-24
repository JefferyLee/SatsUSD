# SatUSD regtest devnet (native binaries)

A local Bitcoin **regtest** stack — `bitcoind` + `lnd` + `tapd` — run as host
processes. This is the interim devnet that unblocks **G1** (TA lock plan A/B) and
M1, pending the PRD's docker-compose multi-node devnet (see ADR-0017).

Pinned versions: **lnd v0.20.1-beta**, **tapd v0.7.2** (≥ PRD floor of lnd 0.17 /
tapd 0.4), **bitcoind** from your PATH (tested v31). Platform: darwin-arm64.

## Prerequisites

- `bitcoind` / `bitcoin-cli` on `PATH` (Homebrew: `brew install bitcoin`).
- `lnd`/`tapd` binaries: run `./devnet/fetch-binaries.sh` once (downloads into
  `devnet/bin/`, which is gitignored).

## Usage

```sh
./devnet/fetch-binaries.sh     # once: fetch lnd + tapd
make devnet-up                 # start bitcoind + lnd + tapd (idempotent)
make devnet-down               # stop (data preserved); add --wipe to reset
```

Interactive CLI wrappers (work under bash or zsh):

```sh
source devnet/env.sh
bcli getblockchaininfo         # bitcoind (regtest node)
bwcli -generate 6              # mine 6 blocks to the funded wallet
dlncli getinfo                 # lnd
dtapcli assets list            # tapd
```

## Layout

- `conf/` — committed config templates (`bitcoin.conf`, `lnd.conf`,
  `tapd.conf.template`; the tapd template's `__ROOT__` is substituted at start).
- `bin/`, `data/`, `logs/` — runtime only, **gitignored**.
- `data/{bitcoind,lnd,tapd}.pid` — daemon pidfiles used by `devnet-down.sh`.

## Ports

| service | RPC | other |
|---|---|---|
| bitcoind | 18443 | zmq 28332/28333 |
| lnd | 10009 (gRPC) | 9735 p2p, 8080 REST |
| tapd | 10029 (gRPC) | 8089 REST |

lnd runs with `--noseedbackup` (auto-created, auto-unlocked wallet) — regtest dev
only, never for real funds.

## Fund lnd + mint a test asset

`tapd` funds asset anchor txs from lnd's on-chain wallet, so fund lnd first:

```sh
source devnet/env.sh
ADDR=$(dlncli newaddress p2tr | grep -o '"address":  *"[^"]*"' | sed 's/.*"\([a-z0-9]*\)"$/\1/')
bwcli sendtoaddress "$ADDR" 10 && bwcli -generate 6
dtapcli assets mint --type normal --name my-asset --supply 1000000 --decimal_display 2
dtapcli assets mint finalize
bwcli -generate 6
dtapcli assets list
```
