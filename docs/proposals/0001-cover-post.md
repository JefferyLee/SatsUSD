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
[LINK to docs/proposals/0001-ta-in-dlc-funding-output.md]

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
