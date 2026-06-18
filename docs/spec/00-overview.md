# SatUSD System Overview & Conventions

- **Spec**: 00
- **Version**: 0.1-draft
- **Authority**: `docs/MISSION.md` v2 via ADR-0001

This document is the entry point to the specification set: the
system map, the shared terminology, and the encoding conventions
every other spec builds on. The key words MUST/SHOULD/MAY follow
RFC 2119.

## 1. System map

```
                 ┌─────────────────────────────────────────┐
                 │            Bitcoin L1                    │
                 │  TA asset (01) · settlements (02) ·      │
                 │  reserve UTXOs + epoch tranches (04)     │
                 └───────────────┬─────────────────────────┘
                                 │ everything below only
                                 │ *reads* or *broadcasts*
   ┌──────────┐   quotes    ┌────┴─────┐  attestations ┌─────────┐
   │  Holder/ │◄───────────►│  Rails   │◄──────────────│ Oracles  │
   │ AI agent │  settle     │ (02, LPs)│               │  (03)    │
   └────┬─────┘             └────┬─────┘               └─────────┘
        │ verify everything      │ reimbursement (04)
   ┌────┴─────┐             ┌────┴─────┐   evidence    ┌─────────┐
   │ Verifier │             │ Reserve  │◄──────────────│Challeng-│
   │ lib (01/ │             │ + epoch  │    slash      │ers (05) │
   │ 02 S3)   │             │ allot(04)│               └─────────┘
   └──────────┘             └──────────┘
        ▲
   ┌────┴─────┐
   │ Curation │  signed rail lists (02 §6.4)
   │  lists   │
   └──────────┘
```

Spec index:

| Spec | Owns |
|---|---|
| 00 (this) | map, terminology, encoding conventions |
| 01 | the TA asset: issuance, burn, supply commitments |
| 02 | the rail standard: state machine, manifest, properties |
| 03 | oracle classes + the reference marker |
| 04 | reserve, reimbursement, epoch allotment |
| 05 | dispute hooks, evidence formats, slashing |
| 06 | vaults: a DLC collateral primitive (LP-issuer sources `Q` / CDP self-mint); checkpoint-CET liquidation |
| 07 | the BTC/USD option: P/N legs (no liquidation), 2-of-2 MuSig2 `Q`, unilateral maturity settlement, offline floor (ADR-0006) |
| 08 | Lightning: the BTC speed layer; redeem-to-pay, on-chain DLC backstop |

## 2. Terminology

| Term | Definition |
|---|---|
| **rail** | a complete SatUSD↔BTC conversion channel defined by a RailManifest (02 §2) |
| **rail_id** | `tagged_hash("SatUSD/rail-manifest/v1", canonical_encode(manifest))` — self-certifying identity |
| **epoch** | 2016 Bitcoin blocks; the cadence of capacity recomputation, reserve allotment (04), and supply commitments (01) |
| **tranche** | a reserve UTXO pre-allocated to one rail for one epoch, sized by the capacity formula (02 §6.1) |
| **S3 artifacts** | the txids, TA transfer proofs, and oracle attestations from which any observer reconstructs a settlement (02 §4) |
| **reference marker** | the protocol-level BTC/USD price used for `price_dev_bound` checks, reimbursement pricing, and the option's **maturity-settlement price** (07 §4/§8, attested by the 03 §5.7 FROST aggregate); defined in 03 §5 |
| **retain fee** | the fee component accruing to the common reserve via reimbursement haircut; feeds the capacity formula (ADR-0002) |
| **service fee** | the fee component paying named per-settlement service providers |
| **burn** | destruction of SatUSD units via the TA-native unspendable script key (01 §4) |
| **attestation** | an oracle's signed statement of an outcome, per its class (03) |
| **CET** | contract execution transaction (dlcspecs); one per discretized outcome in DLC rails |
| **scaffolding** | a transitional founder-dependent role, enumerated in PRD §8 with a removal criterion |

## 3. Encoding conventions

All specs share these rules; cross-language test vectors
(Rust = TypeScript) MUST pin every encoding before mainnet use.

1. **Integers** are unsigned big-endian (network order, matching
   dlcspecs/BOLT practice) unless a containing format (Bitcoin tx,
   PSBT) dictates otherwise.
2. **Tagged hashes** follow BIP-340:
   `tagged_hash(tag, m) = SHA-256(SHA-256(tag) ‖ SHA-256(tag) ‖ m)`.
   All SatUSD tags use the namespace `"SatUSD/<purpose>/v<N>"`.
3. **canonical_encode(struct)**: fields in declaration order; fixed-
   width fields raw; variable-length fields prefixed with a
   BigSize length (per dlcspecs Messaging.md); enums as `u8`
   discriminants in declaration order starting at 0; maps as sorted
   `(key, value)` arrays; no padding.
4. **Signatures** are BIP-340 Schnorr over secp256k1 unless a spec
   states otherwise. x-only public keys are 32 bytes.
5. **Asset amounts** are `u64` in the asset's base unit (01 §2).
6. **Prices** use the canonical type
   `price_msat_per_usd: u64` — milli-satoshis per USD base dollar.
   Conversion from a quoted USD/BTC price `P` (whole dollars):
   `price_msat_per_usd = round_half_up(10^11 / P)`.
   At P = $100,000: `1_000_000` msat/USD. Granularity stays below
   0.001 % up to P = $10M.
7. **Rounding** in settlements always favors the asset-conservation
   invariant: BTC paid to the redeeming user is floored
   (`⌊·⌋ sats`); SatUSD issued to a minting user is floored;
   residuals accrue to the LP leg. Fee computations round half up
   in the protocol's favor (i.e., toward the reserve).

## 4. The four properties (normative summary)

Defined in 02 §4 and binding on every component:

- **S1 unilateral exit** — no reachable state requires a specific
  counterparty's cooperation to avoid loss.
- **S2 conservation** — the user's surrendered amount equals the
  quote; supply decreases exactly by burns; every reimbursement is
  burn-backed (ADR-0003); nothing mints outside spec-04 constraints.
- **S3 observer verifiability** — every terminal state reproducible
  from chain data + published artifacts alone.
- **L1 bought liveness** — every fund-protecting transition is
  executable by anyone or falls to a consensus timeout.

The mission criterion applies to every design decision: *does it
move us closer to, or further from, the state where the external
dependency can be removed?*

## 5. Component inventory (the build plan)

| Component | Crate | Implements | Status |
|---|---|---|---|
| Canonical types + encodings | `satusd-types` | 00 §3 | kept frame, fields re-audited |
| Crypto primitives | `satusd-crypto` | 00 §3.2/3.4 | **kept** as-is |
| TA proof verification | `satusd-ta-proof` | 01 §5 | **kept** |
| Rail state machine + manifest | `satusd-rail` | 02 | new |
| Rail-0 (RFQ swap) | `satusd-rail0` | 02 §7 | new |
| Rail-1 (DLC) | `satusd-rail1` | 02 §7, proposal 0001 | new |
| Vault: collateral primitive + option settle (MuSig2 `Q`, MuSig2-adaptor CET) | `satusd-vault` | 06, 07 | new |
| Oracle: tick attest + decentralised-median FROST cohort (`cohortd`) + tlock | `satusd-oracle` | 03 | new |
| Epoch allotment script | `satusd-allot` | 04 §2–3 | new |
| Client verifier library (incl. option position backing) | `satusd-verify` | 01 §6, 02 §4 (S3), 07 §4.4 | new |
| Cross-language reference | `ts/` + `satusd-vectors` | all encodings | **kept** culture |
| Old-world crates | `satusd-state`, `-reserve`, `-operator`, `-challenger`, `-state-node`, `-dispute`, `-lock`, `-tapd-client` | archived architecture | keep building until replaced (ADR-0001) |

The new crates exist and are tested; the Phase-1 (M-1) trustless core is
validated end-to-end on devnet — option-pair mint + two-input MuSig2
maturity settlement, the offline maturity floor, the decentralised FROST
oracle, and client-side position verification (PRD M-1).

## 6. Networks

| Network | Purpose | Asset ids |
|---|---|---|
| regtest | development, M-A | ephemeral; pinned per-devnet in `integration/` |
| signet | public alpha, M-B | recorded in `docs/spec/registry.md` when minted |
| mainnet | M-C onward | recorded in `docs/spec/registry.md` when minted |

A `registry.md` file (created at first signet mint) records asset
ids, group keys, NUMS sink keys, and genesis txids per network —
the only file whose content is environment-bound rather than
normative.
