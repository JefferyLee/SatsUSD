# Discovery: M1 — `tapd.BurnAsset` metadata channel

- Date: 2026-05-25
- Triggered by: M1 deliverable (PRD §9.2, §14.4); informs DL-1 / DL-2.
- Status: **resolved** — `BurnAsset` does **not** provide a trustworthy on-chain
  user-metadata channel; DL-1/DL-2 stand (lock-first redemption, no BurnAsset
  redemption entry). Verified against live tapd v0.7.2 on the devnet.

## Question

The PRD (DL-1/DL-2) assumed `tapd.BurnAsset` cannot carry trustworthy
user-defined metadata, so SatUSD redemption uses a custom-script **lock** (whose
`script_key` is derived from the `RedeemIntent` before the asset moves) rather than
a burn. M1 must confirm this against the shipped tapd, and decide whether a
metadata-bearing burn is usable (if so it would be an incremental optimization,
not on the MVP critical path).

## Findings (tapd v0.7.2)

1. **`BurnAssetRequest` gained a `note` field** (since the PRD was written):
   *"A note that may contain user defined metadata related to this burn."*
   We burned grouped SatUSD on the devnet (`assets burn --asset_id … --amount …`)
   and exported the burn proof.

2. **The `note` is a local DB annotation, not committed on-chain.** It is stored
   in `tapdb` (SQL `Note NULLABLE`) and is **absent from the exported
   `proof.File`** (the proof carries `asset`, `meta_reveal`, `tx_merkle_proof`,
   inclusion/exclusion proofs, `is_burn`, `genesis_reveal`, … — no note). So a
   verifier/challenger reading the universe proof cannot see or rely on it.

3. **The burn key is structural, not metadata-bearing.** tapd's
   `asset.DeriveBurnKey(firstPrevID)` = `TapTweak(NUMS, serialize(prevID))` where
   `prevID = outpoint ‖ asset_id ‖ script_key` (> 32 bytes, so no valid script
   path). It is provably unspendable and unique per burn, but it commits the
   **spent input**, not any user data. You cannot fix a `redeem_intent_hash` into
   it before burning.

## Conclusion

`tapd.BurnAsset` provides a **provable supply reduction** (unspendable burn key +
`is_burn` proof) but **no committed user-metadata channel**. It therefore cannot
serve as the redemption entry point: SatUSD must bind the `redeem_intent_hash`
into the output *before* the asset moves, which only the custom-script lock
(`lock_script_key = TapTweak(user_asset_refund_key, H(rih ‖ payment_hash))`)
achieves. **DL-1/DL-2 stand; no change to the lock-first design.**

This is orthogonal to SatUSD's **protocol burn sink** (D16, §5.D16): the sink is
SatUSD's *own* family-derived NUMS (`SHA256("SATUSD_BURN_SINK_V1" ‖
asset_family_id)`), to which finalize sends the redeemed asset, and which the
state node verifies via the §5.D16 burn-proof binding (`check_burn_proof_binding`,
#31). SatUSD does not use `tapd.BurnAsset` for that either — the sink is reached
by a normal transfer to the NUMS sink script key, so the redeem_intent / amount
bindings remain enforceable. The `note`-based burn could later annotate operator
bookkeeping, but carries no protocol weight.
