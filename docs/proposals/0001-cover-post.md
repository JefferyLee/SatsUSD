# Delving Bitcoin cover post — proposal 0001 v2

*Draft for the forum post. Replace the link placeholder before
posting. Venue: delvingbitcoin.org, category "Protocol Design".*

---

## Title

**Taproot Assets inside Taproot DLC funding outputs — implemented, with an oracle-gated settlement running on regtest**

## Body

Hi all,

A few days ago I drafted a construction for carrying a Taproot
Asset commitment inside a Taproot-style DLC funding output — one
UTXO that is simultaneously a TA-committed output and a DLC funding
output. The observation it rests on: a key-path-spent DLC funding
output's script tree is empty *by convention, not by protocol*, and
BIP-tap's asset commitment is an inclusion-proof leaf, never a
spending path. So they can share a tree.

Since then I've implemented it, and I'm posting now because the
claims are no longer paper claims:

- **Real Taproot Assets land inside the DLC-shaped output.** tapd
  (v0.7.2) supports the construction natively — `NewAddr` takes a
  foreign internal key plus a `tapscript_sibling` carrying my refund
  leaf. The on-chain output key matches my independent
  reconstruction `Q = P + TapTweak(P ‖ branch(TA leaf, refund
  leaf))·G` byte-for-byte, cross-checked against rust-bitcoin's
  TaprootBuilder.
- **A full oracle-gated DLC settlement executed end-to-end on
  regtest**: CETs made deterministic before signing (via
  `CommitVirtualPsbts` with `skip_funding`), adaptor pre-signatures
  created *before* the outcome existed, the oracle attested, only
  the winning price bucket's secret decrypted, and the resulting
  BIP-341 key-path spend broadcast and confirmed. The losing
  bucket's decryption fails by construction.
- Along the way: a Rust replication of tapd's `DeriveBurnKey`
  validated byte-exactly against a live `BurnAsset` call, and a
  field map of tapd's vPSBT anchor fields (including one endianness
  trap) that future implementers will want.

Full write-up (bilingual EN/中文), with the construction, PSBT
field appendix, evidence table, and reproduction commands
(`make devnet-up` + one `cargo test`):
https://github.com/JefferyLee/SatsUSD/blob/master/docs/proposals/0001-ta-in-dlc-funding-output.md

## Prior art (and why this isn't 10101)

This design space has bodies in it, so positioning honestly:

- **[10101](https://10101.finance/blog/10101-is-shutting-down/)** is
  the closest prior art: a synthetic USD as a 1x-short position
  inside a DLC channel, coordinator as counterparty. It shut down in
  2024 citing Lightning channel plumbing ("route not found" ate the
  engineering budget) and lack of traction — not a failure of DLC
  settlement itself. The channel problem doesn't carry over here:
  this is an on-chain L1 asset, no channels needed to hold or
  transfer it. Two structural problems DO carry over, and I'd rather
  name them than be told: someone must fund the over-collateralized
  long side, and the peg-holder's carry cost reappears at every
  expiry/roll. (My answers: collateral is posted by minters opening
  leveraged-long vault positions — the "who wants the long side"
  and "where does collateral come from" questions are the same
  question; and the roll cost is an explicit fee that accrues to the
  common reserve rather than to an exchange.)
- **Perp-hedge synthetics** ([Stablesats](https://stablesats.com/),
  [Hermetica](https://www.coingecko.com/en/coins/hermetica-usdh)):
  replace the exchange/coordinator counterparty and continuous
  margin with fixed-expiry CET settlement on L1.
- **Sidechain CDPs** ([Sovryn
  Zero](https://wiki.sovryn.com/en/sovryn-dapp/subprotocols/zero-zusd),
  Citrea's ctUSD): no bridge, no federation, no liquidation engine —
  gap risk is bounded by over-collateralization plus short tenors
  and is priced ex-ante instead of depending on liquidation bots
  that fail exactly when needed.
- **DLC custody/loans** ([Lava](https://stacker.news/items/1279809),
  iBTC/[BitSafe](https://media.bitsafe.finance/p/dlc-link-to-bitsafe),
  Firefish, Lendasat): these produce bilateral credit positions;
  the novel part here is making the stable claim itself a
  *transferable bearer asset* — the TA commitment living inside the
  DLC funding output. Worth noting soberly: Lava quietly moved off
  DLC custody in 2025, and iBTC pivoted to institutional custody.
  DLC ops are hard; "anyone-can-broadcast with deterministic CETs"
  is my bet on why settlement (not custody UX) is the right place
  for DLCs.
- **[Tether on Taproot
  Assets](https://tether.io/news/tether-brings-usdt-to-bitcoins-lightning-network-ushering-in-a-new-era-of-unstoppable-technology/)**
  is live and is the incumbent on these very rails. Same rails,
  opposite trust model: no issuer, no fiat reserve, no freeze key.
  I don't claim better liquidity or UX — I claim a different
  product, for holders an issuer can't or won't serve.

## "Your oracle is a single signer" — yes, and here is its fence

Named before you name it: the oracle is the system's weakest link,
registered as scaffolding with a removal criterion. Security rests
on the boundedness of its dishonesty, not on its honesty:
equivocation = key extraction by anyone (implemented, tested);
single-sided lying is capped by a per-settlement deviation bound ×
per-rail capacity; silence ends in a CSV refund. Above that floor,
oracles are a free-entry market — and the market's real product is
volume: every settlement is an implicit price attestation, and with
enough history the price signal needs no signer at all
(volume-gated activation, external anchor until then). The
competing oracles aren't aggregated by that end state; they're
metabolized by it.

The three open questions I'd most value input on:

1. Should BIP-tap explicitly bless "asset commitment in a
   key-path-spent output", or leave it as opt-in verifier behavior?
   It demonstrably works today; the question is whether the
   ecosystem wants it normative.
2. How should TA-side and DLC-side PSBT key ranges be arranged so
   hybrid PSBTs compose with standard ones?
3. MuSig2 nonce discipline for long-lived N-CET pre-signing — my v0
   uses a single funding key (rust-secp256k1 has no musig module
   yet) with per-CET deterministic nonces; the even-Y handling of
   the adaptor's combined nonce hides at least one real bug class
   (my tests caught one). What's the right standardized derivation
   for the 2-of-2 case?

What I most want is "this breaks because X". The construction
touching two protocols' assumptions at once is exactly the kind of
thing that fails in the seams, and I'd rather learn that here than
on signet.

**Disclosure:** built solo with heavy AI assistance (Claude Code) —
the construction, decisions, and mistakes are mine; the AI
accelerated the engineering and the literature survey. The repo's
test culture (cross-language vectors, devnet equivalence tests
against live tapd) exists precisely because a single human can't
review everything an AI writes — machine-verifiable correctness is
the compensating control. Please don't be gentle.

— Jeffery
