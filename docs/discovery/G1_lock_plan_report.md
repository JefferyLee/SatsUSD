# Discovery: G1 — TA Lock Plan A/B (lock construction feasibility)

- Date: 2026-05-24
- Triggered by: G1 hard gate (PRD §9.3, §5.D3, §18.6)
- Status: **IN PROGRESS** (API feasibility confirmed; on-chain lock + spend paths not yet exercised)
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

## Open / next steps

- [ ] Derive `lock_script_key` (reuse satusd-crypto `tap_tweak` + lock_tweak).
- [ ] `CommitVirtualPsbts` with a custom anchor P2TR (NUMS internal + finalize/refund
      leaves per §18.6); confirm tapd accepts the externally-shaped anchor output.
- [ ] Confirm anchor on regtest; verify finalize + refund spends; verify tamper
      and key-path-spend failures.
- [ ] If CommitVirtualPsbts cannot place assets under a fully custom external
      anchor output → fall back to Plan B (manual anchor + RegisterTransfer/proof
      import) and document.
- [ ] Finalize this report + ADR-001 ("Lock construction: Plan A / B / Hybrid").

## References

- taproot-assets v0.7.2 `taprpc/assetwalletrpc/assetwallet.proto`
- PRD §5.D3 (lock script), §18.6 (BTC HTLC/lock script template), §9.3 (G1)
- satusd-crypto `nums::tap_tweak`, `nums::derive_nums_key`
