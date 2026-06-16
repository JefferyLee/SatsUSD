# SatUSD Asset

- **Spec**: 01
- **Version**: 0.1-draft
- **Authority**: `docs/MISSION.md` v2 via ADR-0001
- **Depends on**: 00 (conventions); tapd ≥ 0.7 (supply commitments)

Defines the SatUSD Taproot Asset itself: issuance, units, burning,
supply commitments, and what a verifier must check. Conversion
mechanics live in 02; reserve constraints on issuance live in 04.

## 1. Asset definition

SatUSD is a **grouped, fungible Taproot Asset** on Bitcoin L1:

| Parameter | Value |
|---|---|
| Protocol | Taproot Assets (BIP-tap), tapd ≥ 0.7 wire compatibility |
| Asset type | Normal (fungible) |
| Name | `SatUSD` |
| Grouping | grouped asset (group key enables ongoing issuance) |
| Supply cap | none fixed — issuance is bounded by reserve rules (04), not a constant |
| Genesis metadata | `canonical_encode(AssetMeta)` per §2.2 |

Per-network asset ids, group keys, and genesis txids are recorded
in `registry.md` (00 §6), not here.

## 2. Units and metadata

### 2.1 Base unit

```
1 SatUSD  = $1.00
base unit = 1 µUSD = $0.000001     (6 decimals)
amounts   = u64 in µUSD
display   = 2 decimals by default
```

Rationale: bps-level fees on sub-dollar amounts stay exactly
representable; u64 ceiling (~$18.4 trillion) is not a practical
constraint. All specs and code MUST carry amounts in µUSD; display
rounding is a wallet concern.

### 2.2 Genesis metadata

```
AssetMeta {
    spec_version:        u16     — 01 spec version at genesis
    name:                "SatUSD" (UTF-8, varbytes)
    decimals:            u8 = 6
    mission_commitment:  32 bytes — SHA-256 of docs/MISSION.md at
                                    genesis (a historical anchor,
                                    not a living reference)
}
```

## 3. Issuance

New supply is created by a TA issuance transaction signed by the
**group key**. In v0, issuance is **driven by an LP-issuer selling a
redemption-bearing note** (spec 07 §2): the LP locks over-
collateralised BTC `Q` (spec 06 §2) and the group key signs the
issuance against that collateral. The common-reserve constraints /
CR rules of spec 04 are the **deferred** reserve-era backing
(ADR-0005); the group key holder signs only issuances accompanied by
the collateral evidence chain.

**Scaffolding**: group-key custody is founder-held at launch.
Ledger row in PRD §8; removal criterion: threshold group key
(FROST k-of-n among independent parties) or covenant-gated
issuance, whichever lands first. Until then, every issuance is
publicly auditable against spec-04 evidence even though the
signature is centralized — a verifier that finds an issuance
without matching evidence has found a violation (§6.4).

## 4. Burning

Burning destroys SatUSD via the **TA-native burn**: a transfer
whose output asset script key is the protocol-defined provably
unspendable key (tapd `BurnAsset` semantics; the derivation is
replicated in `satusd-rail0::burn_key` and devnet-validated against
a live `BurnAsset` call). Per ADR-0005 (superseding ADR-0003), the only conforming disposition
of a note is **burn-on-redeem**: redemption is one tx with the note
as a *required input* sent to the burn key (spec 07 §3), in the same
tx that pays the holder BTC. There is no settle-to-LP / recirculation
path (retired with transferability), and the reserve-reimbursement
burn timing (spec 04 §1) is part of the deferred reserve era.
Wherever a burn occurs, the TA-native mechanism is REQUIRED — no
custom sink constructions — so that:

1. burns appear in the supply commitment's burn subtree (§5)
   automatically, and
2. any tapd-compatible verifier recognizes them without SatUSD-
   specific code.

A conforming burn output inside a settlement transaction (02 §3.3)
MUST be constructible inside pre-signed transactions (Rail-1 CETs):
implementations pre-compute the burn-key-bearing TA commitment for
the exact burn amount of each CET bucket.

The S3 burn artifact is the TA transfer proof showing
`script_key = burn_key` and the burned amount; `satusd-verify`
MUST validate it without consulting any server (proof files travel
out-of-band per proposal 0001 §5 strategy 1).

## 5. Supply commitments

The issuer MUST publish a TA **supply commitment** (tapd v0.7
mechanism) **once per epoch**, on the same cadence as reserve
allotment (04 §2). Each commitment binds the cumulative mint, burn,
and outstanding-supply state at a block height within that epoch.

Verifiers reconstruct circulating supply as:

```
supply(epoch) = Σ issuances − Σ burns      (from commitment subtrees)
```

and MUST check `supply(epoch)` against the reserve disclosure of
spec 04. A missing or late supply commitment (> 1 epoch gap) is a
red-flag condition: conforming wallets SHOULD warn and curation
lists SHOULD note it.

## 6. Verifier requirements (`satusd-verify`)

A conforming verifier, given only a Bitcoin node (or headers +
SPV) and published artifacts, MUST be able to:

1. **Validate lineage** of any SatUSD UTXO from genesis via TA
   transfer proofs (`satusd-ta-proof`), including assets held in
   key-path-spent outputs (DLC funding outputs, proposal 0001 §5
   strategy 1 — do not rely on tapd's script-path-only UTXO
   recognition).
2. **Validate burns** per §4 and attribute them to settlements
   (S3 chains).
3. **Reconstruct supply** per §5 and cross-check reserve
   disclosures (04).
4. **Detect violations**: issuance without spec-04 evidence; burn
   amount ≠ quoted conversion (S2); supply commitment gaps.
   Violation artifacts are dispute evidence (05).

The kill-our-servers test (PRD NFR-2) applies: none of the above
may require infrastructure run by the project.

## 7. Experimental assets

Assets experimenting with different **asset rules** (reserve model,
CR formula, burn semantics — ADR-0002 §8) are separate TA groups.
They MUST set `name = "SatUSD-X<n>"` in genesis metadata and MUST
NOT use the unsuffixed name. Wallets display them with suffix and
asset_id (02 §6.5 naming discipline).

## 8. Open items

1. Exact `AssetMeta` byte vectors (Rust = TS) — before signet mint.
2. Whether supply commitments should additionally commit the
   epoch's allotment-plan hash (04) — links supply and reserve
   states in one artifact; leaning yes, decide with spec 04.
3. Minimum TA output dust policy for µUSD-scale transfers.
