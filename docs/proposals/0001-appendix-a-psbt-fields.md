```
BIP: ?
Layer: Applications
Title: PSBT Fields for TA-Aware Taproot DLC Construction
Author: Jeffery Lee <jeffery@example.invalid>
Comments-Summary: No comments yet
Comments-URI: (to be assigned)
Status: Draft v2 — the anchor-template construction these fields support is now implemented and devnet-validated (see the main proposal §8); field assignments unchanged, still proprietary pending upstream namespaces
Type: Standards Track
Created: 2026-06-08
License: BSD-2-Clause
Requires: 174, 327, 341, 371, BIP-tap
```

# Appendix A — PSBT Fields for TA-Aware Taproot DLC Construction

This appendix is a companion to *Taproot Assets in Taproot+MuSig2 DLC
Funding Outputs* (`docs/proposals/0001-ta-in-dlc-funding-output.md`). It
defines the concrete PSBT field layout needed to construct, exchange, and
finalize the three transaction types described in §3 of the parent
proposal: the **Funding** transaction, the per-outcome **CET**, and the
timelocked **Refund**. Until appropriate upstream specs (dlcspecs Taproot
extension; BIP-tap PSBT extension as in bitcoin/bips#1489) define standard
key prefixes, all fields defined here are proprietary per BIP-174.

## A.1 Conventions

### A.1.1 Encoding primitives

| Symbol | Meaning |
|---|---|
| `<u8>` | 1-byte unsigned integer |
| `<u16-le>` | 2-byte little-endian unsigned integer |
| `<u32-le>` | 4-byte little-endian unsigned integer |
| `<u64-le>` | 8-byte little-endian unsigned integer |
| `<compact-size>` | BIP-141 compact size (1, 3, 5, or 9 bytes) |
| `<varbytes>` | `<compact-size>` length prefix followed by that many bytes |
| `<32-bytes>` | exactly 32 raw bytes |
| `<x-only-pubkey>` | exactly 32 raw bytes (BIP-340 x-only) |

### A.1.2 Proprietary key prefix

All fields defined below use BIP-174's `PSBT_*_PROPRIETARY` key type
(`0xFC`). The proprietary identifier is the 6-byte ASCII string `SatUSD`,
followed by a 1-byte version byte (currently `0x01`):

```
PROPRIETARY_PREFIX = 0xFC
                  || 0x07            -- length of "SatUSD" + version byte
                  || "SatUSD"        -- 0x53 0x61 0x74 0x55 0x53 0x44
                  || 0x01            -- version

[subtype-byte] [optional subkey-data] then follow.
```

When upstream `PSBT_*_DLC_*` and `PSBT_*_TAP_ASSET_*` namespaces are
assigned, implementations SHOULD migrate the corresponding subtypes into
those namespaces and treat the `SatUSD/v1/` form as deprecated-but-accepted
for one release cycle.

### A.1.3 Field-scope table

Each subtype is permitted only in specific PSBT scopes. Implementations
MUST reject PSBTs that place a subtype outside its declared scope.

| Scope letter | PSBT section |
|---|---|
| `G` | Global map |
| `I` | Per-input map |
| `O` | Per-output map |

## A.2 Subtype Registry

### A.2.1 Global subtypes

| Subtype | Name | Scope | Key data | Value data |
|---|---|---|---|---|
| `0x00` | `SATUSD_GLOBAL_PROTOCOL_VERSION` | G | (none) | `<u16-le>`: protocol revision (currently `0x0001`) |
| `0x01` | `SATUSD_GLOBAL_CONTRACT_ID` | G | (none) | `<32-bytes>`: contract identifier; SHA-256 of the canonical
encoding of (LP_pubkey ‖ User_pubkey ‖ asset_id ‖ amount ‖ oracle_pubkey ‖ event_id) |
| `0x02` | `SATUSD_GLOBAL_ORACLE_ANNOUNCEMENT` | G | (none) | `<varbytes>`: serialized oracle announcement per dlcspecs Oracle.md, including event_id, attestation public key, and committed nonces R₁…R_N |
| `0x03` | `SATUSD_GLOBAL_CET_COUNT` | G | (none) | `<u32-le>`: number `N` of pre-signed CETs |
| `0x04` | `SATUSD_GLOBAL_PRICE_DISCRETIZATION` | G | (none) | `<u8>` mode ‖ mode-specific data; mode `0x00` = uniform: `<u64-le>` p_min ‖ `<u64-le>` p_step ‖ `<u32-le>` bucket_count |
| `0x05` | `SATUSD_GLOBAL_WINDOW_SECONDS` | G | (none) | `<u32-le>`: validity window from contract activation to refund unlock |

### A.2.2 Per-input subtypes

| Subtype | Name | Scope | Key data | Value data |
|---|---|---|---|---|
| `0x10` | `SATUSD_IN_ROLE` | I | (none) | `<u8>`: input role; `0x00`=user-TA-input, `0x01`=LP-BTC-input, `0x02`=funding-input (for CET/refund PSBTs) |
| `0x11` | `SATUSD_IN_TA_PROOF` | I | (none) | `<varbytes>`: serialized TA asset transfer proof file per BIP-tap §"Proof File Format" |
| `0x12` | `SATUSD_IN_TA_ASSET_ID` | I | (none) | `<32-bytes>`: asset_id of the TA commitment being spent |
| `0x13` | `SATUSD_IN_TA_AMOUNT` | I | (none) | `<u64-le>`: TA amount (in asset's base unit) being spent |
| `0x14` | `SATUSD_IN_MUSIG2_PUB_NONCE` | I | `<x-only-pubkey>`: contributing party's public key | `<66-bytes>`: BIP-327 §`NonceGen` output (R₁ ‖ R₂ concatenation) |
| `0x15` | `SATUSD_IN_MUSIG2_PARTIAL_SIG` | I | `<x-only-pubkey>`: contributing party's public key | `<32-bytes>`: BIP-327 §`Sign` output |
| `0x16` | `SATUSD_IN_ADAPTOR_DECRYPT_SECRET` | I | (none, CET-PSBT only) | `<32-bytes>`: revealed oracle scalar = R_anticipated_nonce + H(outcome) · oracle_secret |
| `0x17` | `SATUSD_IN_ORACLE_ATTESTATION` | I | (none, CET-PSBT only) | `<varbytes>`: full oracle attestation envelope per dlcspecs (event_id ‖ outcome ‖ Schnorr_signature) |

### A.2.3 Per-output subtypes

| Subtype | Name | Scope | Key data | Value data |
|---|---|---|---|---|
| `0x20` | `SATUSD_OUT_ROLE` | O | (none) | `<u8>`: output role; `0x00`=funding-output, `0x01`=user-BTC, `0x02`=NUMS-sink-TA, `0x03`=LP-BTC-refund, `0x04`=user-TA-refund |
| `0x21` | `SATUSD_OUT_TA_COMMITMENT_LEAF` | O | (none) | `<varbytes>`: serialized tap leaf containing the BIP-tap asset commitment (leaf version, leaf script) — required iff role ∈ {funding-output, NUMS-sink-TA, user-TA-refund} |
| `0x22` | `SATUSD_OUT_TA_PROOF` | O | (none) | `<varbytes>`: outgoing TA asset transfer proof for this output (forward link to the receiving wallet) |
| `0x23` | `SATUSD_OUT_CET_ADAPTOR_SIG` | O | `<u32-le>`: CET index `i` ∈ [0, N) | `<162-bytes>`: adaptor signature per BIP-327 §`SignAdaptor`, binding to the canonical sighash of CET_i and to the oracle's R_i anticipated point |
| `0x24` | `SATUSD_OUT_CET_PRICE_BUCKET` | O | `<u32-le>`: CET index `i` ∈ [0, N) | `<u64-le>`: price bucket `p_i` represented by CET_i (sats per USD × 10⁸, or unit-system declared in `SATUSD_GLOBAL_PRICE_DISCRETIZATION`) |
| `0x25` | `SATUSD_OUT_REFUND_DESCRIPTOR` | O | (none) | `<varbytes>`: serialized output descriptor (BIP-380) for the refund_leaf script: typically `tr(NUMS,{ta_leaf, and_v(v:older(<csv>),multi_a(2,LP,User))})` |
| `0x26` | `SATUSD_OUT_REFUND_CSV_BLOCKS` | O | (none) | `<u32-le>`: BIP-112 CSV relative-timelock value applied to the refund_leaf |
| `0x27` | `SATUSD_OUT_NUMS_INTERNAL_KEY` | O | (none) | `<x-only-pubkey>`: NUMS internal key for the NUMS-sink-TA output, derived per BIP-tap §"Burn Sink Key Derivation" |

## A.3 Required field combinations

The presence of certain subtypes is REQUIRED depending on the PSBT's
purpose. The purpose is signaled by `SATUSD_GLOBAL_PROTOCOL_VERSION` and
the combination of per-input/per-output roles.

### A.3.1 Funding PSBT

Global: `0x00`, `0x01`, `0x02`, `0x03`, `0x04`, `0x05`
Inputs:
- One input with role `0x00` (user-TA-input) MUST carry `0x11`, `0x12`,
  `0x13`, plus `0x14` and `0x15` (one each per signing party).
- One input with role `0x01` (LP-BTC-input) MUST carry `0x14` and `0x15`.
Outputs:
- Exactly one output with role `0x00` (funding-output) MUST carry `0x21`
  (the TA commitment leaf), and SHOULD carry one `0x23` and one `0x24`
  for every CET index in `[0, N)`. The output also carries `0x25`, `0x26`,
  and the BIP-371 fields `PSBT_OUT_TAP_INTERNAL_KEY` and
  `PSBT_OUT_TAP_TREE`.

### A.3.2 CET PSBT (for outcome `i`)

Global: `0x00`, `0x01`
Inputs:
- The single input has role `0x02` (funding-input), and MUST carry `0x16`
  (the revealed adaptor decryption secret) and `0x17` (the oracle
  attestation). MuSig2 partial signatures (`0x15`) MAY be present if
  re-signing is required.
Outputs:
- Output with role `0x01` (user-BTC): no SatUSD-specific fields.
- Output with role `0x02` (NUMS-sink-TA): MUST carry `0x21`, `0x22`,
  `0x27`.
- Output with role `0x03` (LP-BTC-refund, here repurposed as LP residual
  return): no SatUSD-specific fields.

### A.3.3 Refund PSBT

Global: `0x00`, `0x01`, `0x05`
Inputs:
- The single input has role `0x02`. MUST carry `0x15` from both LP and
  User (script-path spend of `refund_leaf` requires 2-of-2 sigs).
Outputs:
- Output with role `0x04` (user-TA-refund): MUST carry `0x21`, `0x22`.
- Output with role `0x03` (LP-BTC-refund): no SatUSD-specific fields.

## A.4 Validation rules

A validating implementation MUST reject the PSBT if any of the following
fail:

1. `SATUSD_GLOBAL_PROTOCOL_VERSION` is absent or not in the implementation's
   supported set.
2. `SATUSD_GLOBAL_CET_COUNT` (`N`) does not equal the number of distinct
   `0x23` keys present on the funding output.
3. For each CET index `i ∈ [0, N)`, both `0x23/<i>` and `0x24/<i>` are
   present.
4. The bucket values declared by `0x24` are strictly monotonic in `i` and
   consistent with `SATUSD_GLOBAL_PRICE_DISCRETIZATION`.
5. The funding output's BIP-371 `PSBT_OUT_TAP_TREE` field MUST list at
   least two leaves: one whose script equals `0x21`, and one whose script
   matches the `refund_leaf` reconstructed from `0x25` and `0x26`.
6. For each adaptor signature `0x23/<i>`, the adaptor verification per
   BIP-327 succeeds against the canonical sighash of the CET reconstructed
   from globals + per-output `SATUSD_OUT_ROLE` + `0x24/<i>`.
7. (CET PSBT only) `0x16` decrypts the adaptor signature `0x23/<i>` for
   the index `i` indicated by the attestation in `0x17`.
8. (Funding PSBT only) the sum of TA inputs (`0x13`) equals the sum of TA
   outputs that will result from any CET (the TA-burn output amount).
9. All MuSig2 partial signatures present satisfy BIP-327 partial-signature
   verification against the contract's aggregate key.

## A.5 Coexistence with BIP-371

The funding-output Taproot structure is defined entirely by standard
BIP-371 fields:

- `PSBT_OUT_TAP_INTERNAL_KEY` carries the MuSig2-aggregated key
  (`P = MuSig2_KeyAgg(LP, User)`).
- `PSBT_OUT_TAP_TREE` carries the script tree containing the TA commit
  leaf and the refund leaf.

The SatUSD subtypes defined here are strictly additive: they convey
construction *intent* and adaptor-signature material, not consensus-level
Taproot structure. A Bitcoin-Core PSBT processor that does not recognize
the `SatUSD/v1/` proprietary prefix MUST be able to finalize the funding
transaction using BIP-371 fields alone (assuming the MuSig2 aggregation
has been completed by the parties before finalization).

## A.6 Worked sizes (informative)

For an N = 100 CET contract:

| Field | Count | Size each | Total |
|---|---|---|---|
| `0x23` adaptor sigs | 100 | ~163 bytes | ~16 KB |
| `0x24` price buckets | 100 | ~13 bytes | ~1.3 KB |
| `0x21` TA commit leaf | 1 | ~80 bytes | 80 B |
| `0x11` TA proof in | 1 | typically 2–20 KB | 2–20 KB |
| Other globals + inputs | — | — | ~1 KB |
| **Approximate total** | | | **20–40 KB** |

This is well within typical PSBT exchange limits (BIP-174 places no hard
cap; QR-encoding becomes awkward beyond ~10 KB, so transport over QR
SHOULD be discouraged for this PSBT type and HTTP/Nostr/email used
instead).

## A.7 Open questions

These map to the parent proposal's §6 open questions and require
community consensus before the field assignments above stabilize.

**Q1.** Should `SATUSD_OUT_TA_COMMITMENT_LEAF` (subtype `0x21`) and
`SATUSD_OUT_TA_PROOF` (subtype `0x22`) migrate to an upstream
`PSBT_OUT_TAP_ASSET_*` namespace defined by BIP-tap, removing the need
for the proprietary prefix on TA fields entirely?

**Q2.** Should `SATUSD_OUT_CET_ADAPTOR_SIG` and `SATUSD_OUT_CET_PRICE_BUCKET`
migrate to an upstream `PSBT_OUT_DLC_*` namespace, removing the
proprietary prefix on DLC fields?

**Q3.** Is the per-CET adaptor-sig encoding (`<162-bytes>` per BIP-327
adaptor signature, indexed by `<u32-le>`) the right serialization, or
should it follow a different convention adopted by an upstream
DLC-Taproot PSBT spec?

---

*This appendix is a draft for community review. Comments and corrections
welcome — open an issue on the SatUSD repository or reply on Delving
Bitcoin.*
