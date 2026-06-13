# Signet ops — founder-run infrastructure runbook

Operational record for the PRD §8 scaffolding infrastructure.
No secrets live in this file: the VPS password is rotated and held
by Jeff; the oracle seed exists only on the server.

## VPS

| | |
|---|---|
| Host | 207.148.98.132 (Vultr) |
| OS | Ubuntu 22.04.5 LTS · 1 vCPU · 1 GB RAM (+1 GB swapfile) · 30 GB disk |
| Access | `ssh -i ~/.ssh/satusd_vps root@207.148.98.132` (ed25519 key on Jeff's machine; password auth remains enabled by choice, Vultr console as fallback) |
| Firewall | ufw: OpenSSH + 9590/tcp (oracle) + 9595/tcp (LP) |

## oracled (live since 2026-06-12)

| | |
|---|---|
| Service | systemd `oracled.service` — `Restart=always` |
| Binary | `/root/satusd-src/target/release/oracled` (built on the server from rsynced source) |
| Seed | `/root/satusd-oracle/seed` (hex, mode 600) — **the oracle's identity; never leaves the server, never entered a chat or repo.** Losing it = a new oracle pubkey (acceptable on signet; mainnet needs real key management) |
| Data | `/root/satusd-oracle/data` — `ann-<ts>.hex`, `att-<ts>.hex`, `latest.txt` |
| Retention | `/etc/cron.daily/oracled-prune` deletes `*.hex` older than 14 days (~3.5 GB steady state) |
| Public surface | `http://207.148.98.132:9590` — pubkey `943853cf7912f0f8515746e3c5db4aa97e9dc1a64648be925a647f10dcbd5019`, 1 s cadence, live 3-venue median price (since 2026-06-12; ticks with no fresh price are skipped, never back-filled) |

## Routine commands

```sh
SSH="ssh -i ~/.ssh/satusd_vps root@207.148.98.132"

$SSH systemctl status oracled            # health
$SSH journalctl -u oracled -n 50 --no-pager   # logs
$SSH systemctl restart oracled           # restart
curl -s http://207.148.98.132:9590/v0/latest  # external liveness
```

## Redeploy (after code changes)

```sh
rsync -az -e "ssh -i ~/.ssh/satusd_vps" --delete --exclude target --exclude .git \
  Cargo.toml Cargo.lock crates root@207.148.98.132:/root/satusd-src/
ssh -i ~/.ssh/satusd_vps root@207.148.98.132 \
  'source ~/.cargo/env && cd /root/satusd-src && \
   cargo build --release -p satusd-oracle --bin oracled && \
   systemctl restart oracled && sleep 2 && systemctl is-active oracled'
```

## satusd-lpd — founder Rail-0 LP daemon (live since 2026-06-13)

| | |
|---|---|
| Service | systemd `satusd-lpd.service` — depends on `tapd.service` → `lnd.service` → `bitcoind.service` |
| Binary | `/root/satusd-src/target/release/satusd-lpd` (built on the server from rsynced source) |
| Public surface | `http://207.148.98.132:9595` — `GET /v0/manifest`, `POST /v0/quote`, `POST /v0/settle` |
| rail_id | `d05ecafd22811b1b1c676dd9a090a79da3bb6cd9b2bee573c23e9c3e3762af07` (recompute locally from the manifest; never trust the endpoint) |
| LP pubkey | `93d034fec1ea8e13cc731d9b1ae63dee53141746f0f9a96253f4e6221a13c074` (BIP-340 quote-signing key) |
| Cap | `max_size_sats=100_000` per quote (~$50) — Jeff's hot-wallet constraint |
| Seed | `/root/satusd-node/lp.seed` (mode 600, server-only) — LP quote-signing key derives from it |
| Env | `/root/satusd-node/lpd.env` (LP_SEED + ASSET_ID) loaded by the unit |
| Node set | `/root/satusd-node/signet/data/{bitcoind,lnd,tapd}` — pruned signet (prune=2000, dbcache=384, no txindex); lnd listens 127.0.0.1 only (wallet/key backend for tapd, not LN connectivity); tapd at 127.0.0.1:10030 |

### Bootstrap notes (one-time, 2026-06-13)

- bitcoind IBD on this 1 vCPU / 1 GB box took most of a day; `dbcache` was raised 128→384 mid-sync. lnd/tapd must not be started until bitcoind is past IBD — tapd's lnd-connection has a ~20 s timeout and will crash-loop against a still-syncing lnd. Order on a cold boot: bitcoind → wait for `initialblockdownload:false` → lnd (auto-unlocks via `wallet.pw`) → wait `synced_to_chain:true` → tapd → satusd-lpd.
- lnd wallet was created via REST `/v1/genseed` + `/v1/initwallet`; the 24-word cipher seed is in `/root/satusd-node/lnd-seed-mnemonic.json` (mode 600).
- The SatUSD asset genesis was synced into the VPS tapd via a one-time universe sync (`tapcli universe sync --universe_host=<host> --asset_id=d0c0fb17…`) from a tapd that already held it. Without it, `tapd NewAddr` for SatUSD fails and the LP can't quote. To re-seed after a tapd data wipe, repeat the sync from any SatUSD-holding universe.

### Funding posture (hot wallet, small amounts)

Only the BTC wallet matters: the LP receives SatUSD into freshly-derived TA addresses (no SatUSD held at rest). Keep the BTC balance roughly enough for *a few* concurrent quotes at the configured cap. Top up from the founder's signet wallet or a public faucet; never park more than is needed.

### Routine commands

```sh
SSH="ssh -i ~/.ssh/satusd_vps root@207.148.98.132"

$SSH 'systemctl status bitcoind lnd tapd satusd-lpd | head -40'
$SSH 'journalctl -u satusd-lpd -n 50 --no-pager'
curl -s http://207.148.98.132:9595/v0/manifest | jq .   # external liveness
```

### Redeploy LP code

```sh
rsync -az -e "ssh -i ~/.ssh/satusd_vps" --delete --exclude target --exclude .git \
  Cargo.toml Cargo.lock crates root@207.148.98.132:/root/satusd-src/
ssh -i ~/.ssh/satusd_vps root@207.148.98.132 \
  'source ~/.cargo/env && cd /root/satusd-src && \
   cargo build --release -p satusd-lp --bin satusd-lpd && \
   systemctl restart satusd-lpd && sleep 2 && systemctl is-active satusd-lpd'
```

## Notes

- Changing the oracle seed or any parameter that alters announced
  events is a **new oracle** — announce it, don't blend histories.
- The oracle and LP instances are scaffolding (PRD §8 "founder-run
  single oracle" / "founder LP daemon"); removal criterion: ≥ 1
  independent oracle class with market share and ≥ 1 external LP
  quoting. Anyone may mirror the oracle data dir; clients verify
  signatures, never endpoints.
- The LP daemon is permissionless to query: anyone can `GET
  /v0/manifest` and `POST /v0/quote`; the cost of an unconsumed
  quote to the founder is a UTXO locked for `quote_validity_s` (10
  min by default).
