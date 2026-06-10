# ADR-002: BTC payout confirmation requirements (G2)

- **Date**: 2026-05-25
- **Status**: accepted (G2 hard gate **passed**)
- **PRD reference**: §5.D14 (BTC payment confirmation witness), §14.7 (G2), §6.6
  (`BtcPayoutConfirmation`), DL-22. Amends ADR-0020.
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

DL-22 (fixing v5.0 P0-#2/#3) requires that a reserve reimbursement only finalize
against a **confirmed-and-buried** Bitcoin HTLC claim spend — never a mempool-only
preimage. The state node and challengers must re-verify this independently from a
`BtcPayoutConfirmation` witness (§6.6), with no trust in the operator's report.

ADR-0020 added the fields needed to verify Merkle inclusion + header chaining (tx
indices + the two inclusion headers). G2 surfaced one remaining hole: the witness
committed the claim **txid** (via its Merkle proof) but not the claim tx **body**,
so nothing bound the confirmed tx's *inputs* to the HTLC outpoint (R-15).

## Decision

**The confirmed-payout SPV check (`satusd-state::spv::verify_payout_confirmation`)
is the authoritative resource-safety gate for finalize**, performed in pure Rust
(SHA-256 only) so it is re-computable by any challenger and portable to a circuit.
It enforces, against `BTC_CLAIM_CONFIRMATION_DEPTH = 6`:

1. `sha256(revealed_preimage) == payment_hash`.
2. Inclusion-header ↔ block-hash consistency + valid PoW (`nBits` → target).
3. Merkle inclusion of the HTLC tx and the claim tx in their block headers.
4. Claim block ≥ HTLC block; ≥ 6 confirmation headers chaining onto the claim
   block (each PoW-valid); claim buried `tip_height ≥ claim_height + 6`.
5. **(new, R-15)** the claim tx body (`claim_tx_legacy`, the no-witness
   serialization) double-SHA256s to the Merkle-committed `claim_spend_txid`, and
   its input at `claim_spend_input_index` spends exactly
   `(btc_htlc_txid, btc_htlc_vout)`.

**Witness change (amends ADR-0020):** add `claim_tx_legacy: Vec<u8>` to
`BtcPayoutConfirmation`, encoded as `var_bytes` immediately after
`claim_spend_witness` in the canonical encoding (§18.1); cross-language fixtures +
TS mirror updated accordingly.

## Scope boundary

The SPV proof verifies *resource safety* (the payout is real and buried). It does
**not** parse the HTLC output script to confirm it commits the right
`payment_hash`/pubkeys/CSV — that is the operator/wallet construction concern
covered by the R-01..R-06 HTLC-shape cases at the operator layer, not by the
on-chain proof.

## Consequences

- Reserve reimbursement now requires confirmed Bitcoin SPV evidence (G7 piece 1 of
  3, alongside lineage DL-23 and burn D16).
- The canonical encoding of `BtcPayoutConfirmation` changed; any stored witnesses
  must be re-serialized. Caught by the cross-language vector regeneration.
- A circuit port of the SPV check (M4c) must include the R-15 input-binding step.

## Alternatives considered

- *Commit only the claim txid (status quo).* Rejected: a confirmed but unrelated
  preimage-revealing tx could stand in for the claim — the R-15 hole.
- *Carry the full segwit tx.* Rejected: the txid is over the no-witness
  serialization, so the legacy body is the minimal sufficient witness; parsing the
  segwit marker/witness is unnecessary work.
