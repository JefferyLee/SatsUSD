# ADR-001: SatUSD lock plan (G1) — asset-layer anchoring path

- **Date**: 2026-05-24
- **Status**: accepted (G1 hard gate **passed**)
- **PRD reference**: §5.D3 (lock script), §9.3 (G1), §18.6 (lock template), §18.7 (NUMS)
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

G1 (PRD §9.3) is a hard gate: no M2/M3 until the §5.D3 SatUSD lock is proven
physically realizable on Taproot Assets. The lock must (a) move a SatUSD asset to
an output bound to `lock_script_key = TapTweak(user_asset_refund_key,
H(intent_hash || payment_hash))`, and (b) anchor it under a Bitcoin-layer P2TR
with a NUMS internal key (no key-path spend) and a `{finalize, refund}` script
tree, with both spend paths working and tampering/key-path rejected.

The PRD flagged two unknowns: does tapd let us anchor a custom internal key +
script tree (Plan A), and is the §5.D3 "MVP path" (`FundVirtualPsbt +
SignVirtualPsbt + AnchorVirtualPsbts`) correct?

## Decision / Findings

**Plan A is confirmed; the lock is realizable through tapd's standard receive
flow (`NewAddr` + `SendAsset`), verified on regtest against live tapd v0.7.2.**

1. **Tree shape (refines §5.D3).** tapd attaches our `{finalize, refund}` branch
   as the **`tapscript_sibling` of the Taproot Asset commitment**, not as the whole
   tap tree. The real anchor tree is one level deeper:
   `root = TapBranch(taproot_asset_root, TapBranch(finalize, refund))`.
   Consequently an asset-layer finalize/refund spend uses a control block with one
   **extra merkle step** (`taproot_asset_root`) vs the bare Bitcoin-layer demo
   (97-byte vs 65-byte control block, verified).

2. **`taproot_asset_root` is exposed directly** in `ManagedUtxo` (ListUtxos),
   `TransferOutputAnchor` (ListTransfers), and `AnchorInfo` — no binary proof
   parsing is needed. `merkle_root` and `internal_key` are exposed alongside it.
   Our Rust reconstruction (`satusd_lock::build_asset_lock_anchor`) reproduces
   tapd's `taproot_output_key`, `merkle_root`, and P2TR `scriptPubKey` byte-for-byte.

3. **`NewAddr` field encoding** (the realizable lock address):
   - `script_key.pub_key` = **32-byte x-only** `lock_script_key` (33-byte
     compressed is rejected: "bad pubkey byte string size (want 32, have 33)").
   - `script_key.key_desc.raw_key_bytes` = **33-byte compressed** internal key
     behind the tweak (`user_asset_refund_key`); `script_key.tap_tweak` = the
     32-byte `lock_tweak`; `script_key.type = SCRIPT_KEY_SCRIPT_PATH_EXTERNAL`.
   - `internal_key.raw_key_bytes` = **33-byte compressed** lock-anchor NUMS.
   - `tapscript_sibling` = our finalize/refund branch preimage
     (`0x01 || finalizeLeafHash || refundLeafHash`).
   tapd **accepts a foreign script_key + foreign NUMS internal key** and echoes all
   three, returning the expected `taproot_output_key`.

4. **Path correction to §5.D3.** For a *custom* anchor, the MVP path
   `…AnchorVirtualPsbts` is only for normal-key anchors. The realizable lock does
   not even need the low-level vPSBT path: tapd's high-level `NewAddr` + `SendAsset`
   build and broadcast the custom anchor directly. (`CommitVirtualPsbts +
   PublishAndLogTransfer` remains the lower-level alternative if finer control is
   later required.) **Update §5.D3's MVP implementation path accordingly.**

5. **SPEC GAP — anchor NUMS derivation.** §5.D3 says the anchor uses "a fixed NUMS
   key" but gives no derivation. We derive it from a dedicated domain
   `SATUSD_LOCK_ANCHOR_NUMS_V1` (no salt) via the §18.7 NUMS rule. **§18.2 must
   register this domain and pin a test vector.**

## Verification

- `satusd-lock` bin `g1_lock_btc`: bare Bitcoin-layer lock — FINALIZE
  (preimage+operator_sig+CSV) and REFUND (user_sig+CSV) confirmed on-chain; wrong
  preimage rejected (`OP_EQUALVERIFY`); key-path impossible (NUMS).
- `satusd-tapd-client` bin `g1_lock_asset`: locks a **real minted asset** through
  tapd into the §5.D3 structure, recovers `taproot_asset_root`, rebuilds the deeper
  tree (matches tapd exactly), and spends the anchor via the FINALIZE path on-chain.
- `satusd-lock` unit test `asset_anchor_adds_one_merkle_step_over_bare`.

## Consequences

- G1 passes; M2/M3 unblocked.
- Plan B (manual anchor + proof import) is **not needed**.
- Follow-ups: register `SATUSD_LOCK_ANCHOR_NUMS_V1` in §18.2 (+ vector); reword
  §5.D3 MVP path and tree model. Upstream note: tapd v0.7.2 `NewAddr` **panics
  (nil-pointer, rpcserver.go:1825)** when a `script_key` of type
  `SCRIPT_KEY_SCRIPT_PATH_EXTERNAL` is sent without a `key_desc` — worth reporting.

## References

- `crates/satusd-lock` (`build_lock_anchor`, `build_asset_lock_anchor`,
  `tapscript_sibling_preimage`), `crates/satusd-tapd-client/src/bin/g1_lock_asset.rs`
- `docs/discovery/G1_lock_plan_report.md`
- taproot-assets v0.7.2 `taprpc/{taprootassets,assetwalletrpc}.proto`
