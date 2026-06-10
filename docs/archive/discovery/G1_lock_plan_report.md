# Discovery: G1 — TA Lock Plan A/B (lock construction feasibility)

- Date: 2026-05-24
- Triggered by: G1 hard gate (PRD §9.3, §5.D3, §18.6)
- Status: **PASSED** — lock realized through live tapd; on-chain finalize/refund spends verified. See ADR-001.
- Environment: native regtest devnet (ADR-0017) — bitcoind v31, lnd v0.20.1-beta, tapd v0.7.2

## Background

G1 must prove the SatUSD lock (§5.D3) is physically realizable:
- **Asset layer**: move SatUSD to a vPSBT output whose `script_key =
  TapTweak(user_asset_refund_key, H(intent_hash || payment_hash))`.
- **Bitcoin layer**: the anchor tx output is a P2TR with a **NUMS internal key**
  (no key-path spend) and a 2-leaf script tree `{finalize, refund}` (§18.6).
- Verify finalize (preimage + operator_sig + CSV) and refund (user_sig + CSV)
  spends succeed; tampering and key-path spend fail.

The PRD flags the core unknown as: *does tapd let us anchor to a custom P2TR with
our own internal key + script tree?* (Plan A). If not → Plan B (manually build the
anchor tx, then have tapd register the transfer).

## Findings (so far)

1. **tapcli is insufficient.** Its `assets` subcommands expose only high-level
   `send` (to a tap address), `mint`, `burn`, etc. The low-level vPSBT/anchor and
   key-derivation RPCs are **not** in tapcli — they live in the **AssetWallet
   gRPC service**. Plan A must drive gRPC directly.

2. **AssetWallet RPCs (tapd v0.7.2)**: `FundVirtualPsbt`, `SignVirtualPsbt`,
   `AnchorVirtualPsbts`, `CommitVirtualPsbts`, `PublishAndLogTransfer`,
   `NextInternalKey`, `NextScriptKey`, `DeclareScriptKey`, `QueryScriptKey`, …

3. **Custom anchors ARE supported — via CommitVirtualPsbts, not
   AnchorVirtualPsbts.** The proto is explicit (taprpc/assetwalletrpc):
   > AnchorVirtualPsbts … should be used if the BTC level anchor transaction …
   > [is] encumbered by a **normal key and don't require any special spending
   > conditions**. For any **custom spending conditions on the BTC level**, the two
   > RPCs **CommitVirtualPsbts and PublishAndLogTransfer** should be used instead
   > (which in combination do the same … but allow for more flexibility).

   Our lock is exactly a "custom BTC-level spending condition" (NUMS + script
   tree), so **Plan A is feasible** along this path:
   `FundVirtualPsbt(script_key=lock_script_key)` → `SignVirtualPsbt` →
   `CommitVirtualPsbts` (funds + prepares the anchor PSBT up to signing, allowing
   a custom anchor output) → sign the anchor → `PublishAndLogTransfer`.

### PRD impact

§5.D3's "MVP 实现路径" lists `FundVirtualPsbt + SignVirtualPsbt +
AnchorVirtualPsbts`. For a **custom** anchor (which the lock requires), the
correct path is `FundVirtualPsbt + SignVirtualPsbt + CommitVirtualPsbts +
PublishAndLogTransfer`. Recommend updating §5.D3 (ADR-001 will record this).

## Progress

- **Rust gRPC client built** (`crates/satusd-tapd-client`, tonic 0.14 + vendored
  v0.7.2 protos). Connectivity to the devnet tapd verified via `g1_probe`
  (TaprootAssets.GetInfo → version 0.7.2, regtest).
- **TLS gotcha (resolved)**: tapd (like lnd) serves a self-signed cert marked
  CA:TRUE; rustls rejects it (`CaUsedAsEndEntity`) and tonic's `ClientTlsConfig`
  has no escape hatch. Solution: a custom rustls connector that **pins** the
  served cert (see lib.rs `PinnedCertVerifier`). The reference SDK will need the
  same to talk to lnd. Requires `protoc` at build time (brew protobuf).

## Progress (cont.)

- **Bitcoin-layer lock anchor built** (`crates/satusd-lock`, rust-bitcoin 0.32):
  `build_lock_anchor` produces the P2TR with NUMS internal key + finalize/refund
  tapscript leaves (§5.D3). Deterministic output key, both leaves carry control
  blocks, scriptPubKey is P2TR, internal key == lock-anchor NUMS. Unit-tested.
  - **SPEC GAP**: §5.D3 says the anchor uses "a fixed NUMS key" but gives no
    derivation. We derive from `SATUSD_LOCK_ANCHOR_NUMS_V1` (no salt) via the
    §18.7 NUMS rule → **§18.2 must register this domain + a test vector** (ADR).
  - Note: bitcoin 0.32 brings secp256k1 0.29 alongside satusd-crypto's 0.31;
    crossed via `[u8;32]` bytes (no type clash). Unify later if desired.

## Progress (cont. 2)

- **Asset-layer `lock_script_key` implemented** (`satusd-lock::derive_lock_script_key`):
  `TapTweak(user_asset_refund_key, lock_tweak)` reusing satusd-crypto/-types.
  Deterministic, valid x-only, payment-hash-sensitive. Unit-tested.
- **How our script tree attaches (key discovery):** tapd embeds custom spending
  conditions as a **`tapscript_sibling`** of the Taproot Asset commitment — the
  anchor output's tap tree is `branch(asset_commitment_root, tapscript_sibling)`.
  The sibling preimage is supplied on the receiving side (e.g. `NewAddr`'s
  `tapscript_sibling`, taprootassets.proto field 8; also on transfer outputs).
  - **Refines §5.D3's model**: the finalize/refund leaves are the *sibling* of the
    asset commitment, not the whole tree. Our `satusd-lock` finalize/refund branch
    becomes the `tapscript_sibling` preimage.
  - **Still open**: setting the anchor **internal key to NUMS** (§5.D3) — address
    receive uses tapd's own internal key; NUMS likely needs `CommitVirtualPsbts`
    with a custom anchor internal key. To verify next.

## Bitcoin-layer spend paths — VERIFIED on regtest

`satusd-lock` bin `g1_lock_btc` builds the lock anchor, funds it from the regtest
wallet, and spends it (via bitcoind RPC, rust-bitcoin 0.32):

- **FINALIZE** path: witness `[operator_sig, preimage, finalize_script,
  control_block]`, `nSequence = finalize_csv` — confirmed on-chain.
- **REFUND** path: witness `[user_sig, refund_script, control_block]`,
  `nSequence = refund_csv` — confirmed on-chain.
- **TAMPER** (wrong preimage in finalize): rejected by bitcoind with
  `mempool-script-verify-flag-failed (Script failed an OP_EQUALVERIFY operation)`.
- **KEY-PATH**: structurally impossible — internal key is the lock-anchor NUMS.

This retires G1's core risk: the §5.D3 Bitcoin-layer lock and both spend paths
are physically realizable. Run: `make devnet-up && cargo run -p satusd-lock --bin g1_lock_btc`.

## Asset-layer anchoring — integration design resolved (byte-precise)

The receive path is `NewAddr` (taprootassets.proto), which takes all three pieces:
- `script_key` (field 3) = our `lock_script_key` (asset-level).
- `internal_key` (field 4, KeyDescriptor) = the lock-anchor NUMS (BTC-level).
- `tapscript_sibling` (field 5) = serialized preimage of our finalize/refund branch.

**`tapscript_sibling` byte format** (confirmed from tapd v0.7.2 `commitment/taproot.go`):
- Branch: `0x01 || leftLeafTapHash(32) || rightLeafTapHash(32)` — tapd recomputes the
  TapBranch hash with BIP341 sorting, so child order is irrelevant.
- Leaf:   `0x00 || leafVersion || compactSize(script) || script`.
Our sibling is the branch of {finalize, refund}.

**VALIDATED against live tapd** (`satusd-tapd-client` bin `g1_addr`): `NewAddr`
with our branch preimage (`satusd-lock::tapscript_sibling_preimage`) succeeds and
the daemon **echoes the identical `tapscript_sibling`** and returns a
`taproot_output_key` — confirming the reverse-engineered encoding is correct.

**Key consequence for spends (important):** tapd attaches our branch as the
*sibling of the Taproot Asset commitment*, so the real anchor tap tree is
`TapBranch(TA_commitment_root, branch(finalize, refund))` — one level deeper than
the bare Bitcoin-layer demo. Therefore an asset-layer finalize/refund spend needs
a control block whose merkle path includes **`TA_commitment_root`** (computed by
tapd at anchor time, recoverable from the transfer/proof). I.e. the asset-locked
spend = the on-chain demo's control block + one extra TA-commitment merkle step.

## Asset-layer lock + on-chain spend — VERIFIED on regtest

`satusd-tapd-client` bin `g1_lock_asset` locks a **real minted asset** through
live tapd into the §5.D3 structure and spends the anchor on-chain:

1. **`NewAddr` accepted a foreign script_key + foreign NUMS internal_key.**
   Byte-precise field encoding (the spec for the reference SDK):
   - `script_key.pub_key` = **32-byte x-only** `lock_script_key` (33-byte is
     rejected: "bad pubkey byte string size (want 32, have 33)").
   - `script_key.key_desc.raw_key_bytes` = **33-byte compressed** internal key
     behind the tweak (`user_asset_refund_key`); `script_key.tap_tweak` = 32-byte
     `lock_tweak`; `script_key.type = SCRIPT_KEY_SCRIPT_PATH_EXTERNAL`.
   - `internal_key.raw_key_bytes` = **33-byte compressed** lock-anchor NUMS.
   - `tapscript_sibling` = our finalize/refund branch preimage.
   tapd echoes all three and returns the expected `taproot_output_key`.
   - ⚠ tapd v0.7.2 **panics** (nil-pointer, `rpcserver.go:1825`) if an external
     `script_key` is sent **without** `key_desc` — report upstream.
2. **`SendAsset` (full balance) built + broadcast the custom anchor.** We read
   `taproot_asset_root` / `merkle_root` / `internal_key` straight off the transfer
   output (`TransferOutputAnchor`) — **no binary proof parsing needed**. (Also
   exposed via `ManagedUtxo`/`AnchorInfo`.) The anchor `internal_key` == our NUMS.
3. **Reconstruction matches tapd byte-for-byte.** `build_asset_lock_anchor(...,
   taproot_asset_root)` rebuilds `TapBranch(taproot_asset_root, branch(finalize,
   refund))`; its `merkle_root`, `taproot_output_key`, and P2TR `scriptPubKey` all
   equal tapd's.
4. **FINALIZE spend confirmed on-chain.** The control block is **97 bytes**
   (= 1 + 32 + 2×32), i.e. the bare 65-byte block plus the extra
   `taproot_asset_root` merkle step. REFUND is the analogous path (the extra step
   is identical); the bare-layer demo already confirms refund/tamper/key-path.

Run: `make devnet-up && cargo run -p satusd-tapd-client --bin g1_lock_asset`.

## Conclusion

**G1 PASSED.** The §5.D3 SatUSD lock is physically realizable: a real asset is
locked through tapd's standard receive flow under a NUMS internal key + our
finalize/refund tapscript sibling, and the anchor is spent on-chain via the
finalize path. Plan B (manual anchor + proof import) is not needed. Decisions and
the §5.D3 / §18.2 follow-ups are recorded in **ADR-001**.

## References

- taproot-assets v0.7.2 `taprpc/assetwalletrpc/assetwallet.proto`
- PRD §5.D3 (lock script), §18.6 (BTC HTLC/lock script template), §9.3 (G1)
- satusd-crypto `nums::tap_tweak`, `nums::derive_nums_key`
