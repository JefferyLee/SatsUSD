# satusd-oracle-signer

Live oracle signer daemon for the SatUSD M6 pilot (PRD §5.D7). Signs
**BabyJubjub EdDSA** over `oracle_message_hash(set_epoch, price_epoch, ts_ms,
price_e8)` for a 3-of-5 quorum, in Node + `circomlibjs`. Rust
`babyjubjub-rs::sign` panics on arm64 (M2 #29) but its `verify` is fine, and the
iden3 packed format is identical — so JS signs, Rust verifies, byte-for-byte.

## Setup

```bash
cd tools/oracle-signer
npm install
node index.mjs
```

Listens on `:8090` by default. Configurable via env: `PORT`, `KEY_SEED_PREFIX`.

On startup the daemon prints its 5 derived pubkeys; use them to compute
`oracle_set_hash` at state-node genesis.

## Endpoints

| Method | Path | Returns |
|---|---|---|
| GET | `/v1/oracle/pubkeys` | `{ pubkeys: [hex; 5] }` — the 5 packed BabyJubjub pubkeys (32 bytes each) |
| GET | `/v1/oracle/quorum?price_e8=N[&set_epoch=N&price_epoch=N&ts_ms=N]` | `{ set_epoch, price_epoch, timestamp_ms, price_e8, messages: [OracleMessage; 5] }` |

`OracleMessage` JSON mirrors the Rust `satusd_types::types::OracleMessage`
shape exactly: `{ domain, oracle_id, oracle_set_epoch, price_epoch,
timestamp_ms, pair, price_e8, source_commitment, signer_pubkey, signature }`.
`pair` is the 8-byte ASCII `BTC/USD\0`. The submitter picks any **3+ of the 5**
messages to feed into `submit_claim`.

## Cross-language interop smoke (already proven in dev)

```bash
# 1. Boot the daemon (separate terminal):
node index.mjs

# 2. Fetch a signed quorum at $50k/BTC:
curl -s 'http://127.0.0.1:8090/v1/oracle/quorum?price_e8=5000000000000&set_epoch=7&price_epoch=100&ts_ms=1700000000000' > /tmp/quorum.json

# 3. Verify message #0 with the Rust babyjubjub-rs verifier:
PK=$(node -e "console.log(JSON.parse(require('fs').readFileSync('/tmp/quorum.json')).messages[0].signer_pubkey)")
SIG=$(node -e "console.log(JSON.parse(require('fs').readFileSync('/tmp/quorum.json')).messages[0].signature)")
cargo run -p satusd-operator --bin eddsa_verify -- 7 100 1700000000000 5000000000000 "$PK" "$SIG"
# → verify: OK  (exit 0)
```

This is the proof that the daemon's bytes match what the state-node's oracle
verifier (`satusd-state::oracle::verify_signatures`) accepts on a real claim.

## Going to production (pilot signet onward)

The dev daemon hosts all 5 keys for convenience. A real M6 deploys **5
independent operators**, each:

- Holding **ONE** key (HSM / hardware wallet / Yubikey-backed signer), never
  shared.
- Running their own instance exposing a `/v1/oracle/sign` (single signer).
- Subscribing to a real price feed (CoinGecko, Pyth, Binance, …) and committing
  to `source_commitment` (a hash of the source transcript); the transcripts go
  to OracleDA so the challenger can recompute `price_e8` (PRD §5.D7 step 3,
  social-layer detection of O-06 single-signer drift).

The submitter (operator) hits all 5 in parallel and includes the first 3+
fresh signatures in the claim. The state-node verifies against the committed
`oracle_set_hash`; outliers get filtered by the median ±2% inlier rule
(§5.D7, O-05).

Key custody: **never** ship a real key derived from `KEY_SEED_PREFIX` to
production — load from env / vault / HSM. The dev derivation
(`SHA256("{prefix}-{i}")`) is for reproducibility in regtest only.
