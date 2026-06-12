# SatUSD asset registry

Authoritative identifiers for every SatUSD issuance. Wallets and
verifiers pin these; anything not listed here claiming the name is
not SatUSD. (Note: an unrelated EVM project uses the ticker
"satUSD" — this registry is the Bitcoin L1 asset only.)

## signet (genesis: 2026-06-12)

| field | value |
|---|---|
| network | Bitcoin signet |
| asset_id | `d0c0fb1781dc4eac0778998c57263da95e54da5ecd535eb2d368cb7d766f1543` |
| group_key | `02259ce987bff0e6e7601bb07a31c10fe53e2106c1b740250b17a0745260077454` |
| genesis_point | `ba89660d5259eeb37b6953b4d57be74d747e170f42a40e194d27d782003011e7:2` |
| anchor_tx | `7705fb5a926293d709b1ea1b0314c1a1c1f20e3b9d3bafd6a4d369a3ba3c9642` (block 308523) |
| genesis_supply | 1,000,000,000 µUSD ($1,000.00) |
| decimals | 6 (µUSD base unit, spec 01 §2.1) |
| meta_type | opaque, canonical `AssetMeta` (spec 01 §2.2) |
| meta_bytes | `00010653617455534406fcd56d398c8b8f6189bb17f2b4f3a0a1e54d4b63f577081b6cc36290e82053e6` |
| meta_hash (tapd) | `11e2e0ac456511b3b88a367d8fd26d1e15a9bc27608d09078b4c9d9d43e2fbac` |
| mission_commitment | `fcd56d398c8b8f6189bb17f2b4f3a0a1e54d4b63f577081b6cc36290e82053e6` — SHA-256 of `docs/MISSION.md` at commit `10189aa` |
| issuer (scaffold) | founder-held group key — PRD §8 ledger row; removal: FROST k-of-n or covenant issuance (spec 01 §3) |
| oracle (signet) | `http://207.148.98.132:9590`, pubkey `943853cf7912f0f8515746e3c5db4aa97e9dc1a64648be925a647f10dcbd5019`, 1 s ticks, 3-venue median |
| tapd | v0.7.2 |

Verification: fetch the genesis proof from any tapd universe served
by a holder, check the meta bytes against the `AssetMeta` vectors
(`integration/vectors/rail-vectors.json`), and recompute
`mission_commitment` from the repository history.

## mainnet

Not yet. M-C gates apply (PRD §7): conservative caps, the
scaffolding ledger shipping with the asset, and the security-review
decision — before any mainnet genesis.
