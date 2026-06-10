# Discovery: G2 — BTC SPV-Based Confirmed Payout Demo

- Date: 2026-05-25
- Triggered by: G2 hard gate (PRD §14.7, §5.D14, DL-22)
- Status: **PASSED** — confirmed-payout SPV verifier independently re-checks the
  HTLC claim burial; R-07, R-13, R-14, R-15 all reject. See ADR-002.
- Environment: pure-Rust verifier (`satusd-state::spv`), cross-checked against real
  regtest block headers (grind-PoW), in the §5.D12 batch claim pipeline.

## Background

G2 must prove the resource-safety property behind the v5.1 redemption redesign
(DL-22, fixing v5.0 P0-#2/#3): **a reserve reimbursement can only finalize against
a Bitcoin payout that is confirmed-and-buried, never a mempool-only preimage.**

The state node (and any challenger) must independently re-verify, from a
`BtcPayoutConfirmation` (§6.6) witness, that the user's BTC HTLC claim spend is
included in a block and buried under ≥ `BTC_CLAIM_CONFIRMATION_DEPTH` (= 6)
confirmations — replacing the v5.0 `confirmation_depth` / `revealed_preimage`
"trust the operator's word" facts with re-computable SPV evidence (G7 three-piece:
SPV + lineage + burn).

## Findings

The SPV verifier (`crates/satusd-state/src/spv.rs`, `verify_payout_confirmation`)
performs the off-chain-verifiable subset of §5.D14, in pure Rust (SHA-256 only):

1. **Preimage binding** — `sha256(revealed_preimage) == payment_hash`.
2. **Header consistency + PoW** — each inclusion header's double-SHA256 equals the
   stated block hash, and its hash ≤ target expanded from the compact `nBits`
   (regtest `0x207fffff`).
3. **Merkle inclusion (5,6)** — both the HTLC tx and the claim tx are Merkle-
   included in their stated block headers, folding the leaf with the tx index for
   direction (ADR-0020 added the tx index + inclusion headers needed to do this).
4. **Ordering (7)** — the claim block is no earlier than the HTLC block.
5. **Confirmation chain (8,9)** — ≥ K headers chain onto the claim block by
   `prev_block_hash`, each with valid PoW.
6. **Burial (10)** — `tip_height ≥ claim_height + K`.
7. **Claim spends the HTLC outpoint (R-15)** — the `claim_tx_legacy` body's
   double-SHA256 equals the Merkle-committed `claim_spend_txid`, and its input at
   `claim_spend_input_index` spends exactly `(btc_htlc_txid, btc_htlc_vout)`.

**Why R-15 needed a witness extension.** The original `BtcPayoutConfirmation`
committed the claim *txid* (via the Merkle proof) but not the claim tx *body*, so
nothing tied the confirmed tx's inputs to the HTLC outpoint — a confirmed but
unrelated preimage-revealing tx could masquerade as the claim. We added a
`claim_tx_legacy` field (the no-witness serialization; its dSHA256 is the txid) and
parse its inputs to enforce the spend. See ADR-002 (amends ADR-0020).

## Acceptance — `R-07, R-13, R-14, R-15` (+ drill)

| Test | Scenario | Result | Where |
|---|---|---|---|
| **R-07** | mempool-only preimage (confirmations < 6) → cannot finalize | `InsufficientConfirmations` | `spv::r07_r13_insufficient_confirmations`, `claim::shallow_burial_rejected` |
| **R-13** | `confirmation_headers.len() < 6` | `InsufficientConfirmations` | `spv::r07_r13_insufficient_confirmations` |
| **R-14** | broken header chain / bad PoW / bad Merkle proof | `HeaderChainBroken` / `HeaderPowInvalid` / `ClaimMerkleInvalid` | `spv::r14_broken_chain`, `spv::r14_bad_merkle_proof` |
| **R-15** | claim tx does not spend the HTLC outpoint (or body ≠ txid) | `ClaimDoesNotSpendHtlc` / `MalformedClaimTx` | `spv::r15_claim_must_spend_htlc_outpoint`, `spv::r15_claim_tx_body_must_hash_to_txid` |

The headline adversarial drill — **"operator submits a claim with only a mempool
preimage"** (R-07, PRD line 729/2526) — is rejected because the verifier requires
6 chained, PoW-valid confirmation headers burying the claim block; a mempool tx
has none. All SPV tests use **real regtest block headers** (nonce ground to satisfy
PoW), so the PoW/Merkle/chain checks run against genuine Bitcoin header bytes.

## Scope boundary (tracked, not a silent gap)

`verify_payout_confirmation` covers the *resource-safety* subset of §5.D14. It does
**not** parse the HTLC *output script* to confirm it commits the right
`payment_hash` / pubkeys / CSV — i.e. that the HTLC was well-formed in the first
place. That is the wallet/operator's construction-time concern and is enforced by
the intent↔lock binding + the R-01..R-06 HTLC-shape adversarial cases at the
operator layer (M3 operator backend), not by the on-chain SPV proof. Recorded here
so the gate's exact coverage is unambiguous.

## Conclusion

G2 **passes**: the confirmed-payout SPV verifier is real, re-computable, and
rejects R-07/R-13/R-14/R-15. The "mempool preimage cannot finalize" property
(DL-22) holds. Reserve reimbursement now requires confirmed Bitcoin SPV evidence,
one of the three independent proofs G7 demands.
