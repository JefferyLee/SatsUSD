# SatUSD signet node set

The M-B environment: bitcoind + lnd + tapd on **public signet**,
run as host processes, parallel to (and port-disjoint from) the
regtest devnet — both can run at once.

## Bring-up

```sh
./devnet/fetch-binaries.sh   # once; binaries are shared with the devnet
make signet-up               # starts all three; does NOT block on IBD
make signet-status           # watch sync progress
make signet-down
```

First run performs signet IBD; lnd and tapd settle on their own as
bitcoind catches up. Everything persists under `signet/data/`
(gitignored).

## Funding

```sh
source signet/env.sh
swcli getnewaddress          # then use a signet faucet, e.g.:
                             #   https://signetfaucet.com
slncli newaddress p2tr       # lnd's own wallet, for channel/anchor funding
```

## Ports

| daemon | RPC | other |
|---|---|---|
| bitcoind | 38332 | zmq 28342 / 28343 |
| lnd | 10010 | p2p 9736, REST 8081 |
| tapd | 10030 | REST 8090 |

## CLI wrappers

`source signet/env.sh` →
`scli` (bitcoin-cli), `swcli` (signet wallet), `slncli`, `stapcli`.

## Oracle daemon, public

```sh
cargo run -p satusd-oracle --bin oracled -- \
  <seed-hex> signet/data/oracle 100000 1 0.0.0.0:9590
```

serves `GET /v0/pubkey | /v0/announcement/<ts> | /v0/attestation/<ts>
| /v0/latest` (spec 03 §3.2 — transport non-normative, TLVs are the
standard).

### Live founder-run instance (signet scaffolding, PRD §8)

```
base url : http://207.148.98.132:9590
pubkey   : 943853cf7912f0f8515746e3c5db4aa97e9dc1a64648be925a647f10dcbd5019
cadence  : 1 s · price: live median of Coinbase/Kraken/Bitstamp
           (≥2 venues required; stale >120 s ⇒ tick skipped, never
           back-filled)
retention: attestation/announcement files pruned after 14 days
           (anyone may mirror; clients verify signatures, never
           endpoints)
```

Deployed 2026-06-12 (systemd `oracled.service`, Ubuntu 22.04).
Equivocation by this oracle is provable and key-extracting per
spec 05 §2 — watch it.

## Scaffolding notes

- lnd runs `--noseedbackup` (signet coins carry no value); replace
  with a real seed for anything that must survive disk loss.
- The founder-run signet instance of the oracle is a PRD §8 ledger
  row (single-signer, transitional by declaration — spec 03 §3.4).
