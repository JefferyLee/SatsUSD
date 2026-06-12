# SatUSD signet quickstart — redeem dollars for sats in ~30 minutes

You will run a Bitcoin signet node set, receive SatUSD (a Taproot
Asset), and redeem it for BTC through an LP — with your own machine
verifying every step. No account, no permission, no trust in any
endpoint.

## 0. Prerequisites

- macOS/Linux, ~10 GB disk, Rust toolchain, `bitcoind`/`bitcoin-cli`
  in PATH (v25+)
- clone this repo; everything below runs from its root

## 1. Bring up the node set

```sh
./devnet/fetch-binaries.sh    # once: lnd + tapd binaries
make signet-up                # bitcoind + lnd + tapd on public signet
make signet-status            # repeat until all three say synced
```

First sync takes a while (it's a real public chain). Everything
lives under `signet/data/`, removable any time.

## 2. Get signet coins (gas + anchors)

```sh
source signet/env.sh
slncli newaddress p2tr        # → faucet this at https://signetfaucet.com
```

## 3. Get SatUSD

Ask the founder (or any holder) to send SatUSD to your address —
the asset is `d0c0fb17…6f1543` (see `registry.md`, verify it
yourself):

```sh
stapcli addrs new --asset_id d0c0fb1781dc4eac0778998c57263da95e54da5ecd535eb2d368cb7d766f1543 --amt 1000000
# share the encoded address; 1,000,000 µUSD = $1.00 arrives on-chain
satusd balance                # SATUSD_NET=signet cargo run -p satusd-cli --bin satusd -- balance
```

## 4. Redeem: dollars in, sats out

```sh
export SATUSD_NET=signet
cargo run -p satusd-cli --bin satusd -- \
  redeem http://207.148.98.132:9595 0.80
```

The founder-run signet LP is at `http://207.148.98.132:9595` — small
amounts only (per-quote cap is in `GET /v0/manifest`). It is *a*
market participant, not *the* endpoint: anyone may run their own,
and clients should verify rails by manifest hash, not by URL.

Watch what the CLI does **before** it signs anything:

```
rail_id  : … (recomputed locally)        ← manifest rebuilt + hashed by YOUR machine
plan     : ✓ commitment matches          ← fee math recomputed by YOUR machine
lp_sig   : ✓ BIP-340 valid               ← the LP signed exactly this quote
price    : ✓ within N bps of oracle      ← quoted price checked against the public
                                            oracle, inside the rail's own bound
```

Refusal on any mismatch is the design, not an error. Then: your
tapd signs the asset leg, your lnd signs the anchor, the LP
co-signs over HTTP, your node broadcasts. One transaction carries
both legs — the SatUSD moving and the BTC paying you.

## 5. Verify independently (optional, encouraged)

- Oracle price feed: `curl http://207.148.98.132:9590/v0/latest`
  then `/v0/attestation/<ts>` — decode it with any dlcspecs client;
  pubkey is pinned in `registry.md`
- Every encoding this flow used is pinned cross-language:
  `make vectors && make verify` (4,700+ assertions)
- The asset's genesis carries the mission text's hash:
  `registry.md` shows how to recompute it

## Something broke?

- `satusd redeem` cleans stale leases automatically; a failed
  attempt costs nothing but a locked UTXO for ~10 min
- signet blocks come ~10 min apart; "unconfirmed" just means wait
- file an issue with the CLI output — the error messages name the
  exact verification that refused
