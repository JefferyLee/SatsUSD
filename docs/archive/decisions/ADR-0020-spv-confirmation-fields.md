# ADR-0020: BtcPayoutConfirmation gains SPV-completeness fields

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §5.D14, §6.6, §13.1 (R-07/R-13/R-14)
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

The §6.6 `BtcPayoutConfirmation` witness (DL-22/D14) gave merkle proofs as bare
`Vec<[u8;32]>` and identified the inclusion blocks only by hash — which is
insufficient to *actually* verify a Bitcoin SPV proof:

- A Bitcoin merkle branch needs the **tx position** to know hashing direction at
  each level; a hash list alone is ambiguous.
- Verifying merkle inclusion needs each inclusion block's **merkle root**, i.e.
  its 80-byte header — only the block *hash* was provided.

## Decision

Add four fields to `BtcPayoutConfirmation` (appended; v5.2 struct revision):

```rust
htlc_tx_index:           u32,
claim_tx_index:          u32,
htlc_inclusion_header:   [u8; 80],
claim_inclusion_header:  [u8; 80],
```

The `*_inclusion_block_hash` fields must equal `dsha256` of the corresponding
header (the verifier checks this), and `confirmation_headers[0]` chains onto
`claim_inclusion_block_hash`.

`satusd-state::spv::verify_payout_confirmation` implements the off-chain
subset of §5.D14: preimage→payment_hash, merkle inclusion of both txs, claim ≥
htlc height, ≥K confirmation headers chaining onto the claim block with valid
PoW, and buried ≥K vs the tip. Satisfies R-07, R-13, R-14.

## Consequences

- `BtcPayoutConfirmation` canonical encoding changes → vectors regenerated
  (Rust/TS still byte-match); PRD §6.6/D14 struct updated.
- **Not yet covered** (further extensions, tracked separately): that the claim
  tx body actually spends the HTLC outpoint (R-15 — needs the claim tx, not just
  its txid) and that the HTLC output script commits the right
  payment_hash/pubkey/CSV (needs script parsing). Check 10 (best-chain tip) and
  11 (no competing spend) remain live-bitcoind concerns; the tip height is passed
  in as a verified input here.

## References

- `crates/satusd-state/src/spv.rs`, PRD §5.D14
