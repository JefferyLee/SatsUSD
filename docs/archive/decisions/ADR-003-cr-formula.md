# ADR-003: Collateral-ratio formula final form (G3)

- **Date**: 2026-05-25
- **Status**: accepted (G3 hard gate **passed**)
- **PRD reference**: §5.D1 (CR), §5.D8 (emergency tiers), §14.8 (G3), DL-24.
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

G3 freezes the collateralization math before M4 circuit work. The collateral ratio
mixes three units — reserve (sats), supply (cent-atoms, decimals=2), and price
(USD/BTC × 10^8) — so the dimensional reconciliation and rounding must be pinned
exactly, identically in Rust, TypeScript, and the circuit.

## Decision

The collateral ratio, in parts-per-million, is:

```
collateral_ratio_ppm(reserve_sats, supply_atoms, price_e8):
    if supply_atoms == 0: undefined (None)
    num    = reserve_sats · price_e8 · 1_000_000     // widen to u128 first
    den    = supply_atoms · 10^14
    cr_ppm = floor(num / den)                          // integer division
    assert cr_ppm ≤ u64::MAX
```

- **Dimensional derivation.** Reserve value in USD-e8 = `reserve_sats · price_e8 /
  10^8`. Supply value in USD-e8 = `supply_atoms / 10^2 · 10^8 = supply_atoms ·
  10^6`. Ratio × 10^6 (ppm) = `(reserve·price/10^8) / (supply·10^6) · 10^6 =
  reserve·price·10^6 / (supply·10^14)`. Hence the `10^14` denominator term.
- **Arithmetic safety.** All multiplications widen to `u128` before dividing
  (§ arithmetic-safety rule); `cr_ppm` is asserted to fit `u64`. With the §18.3
  ranges (reserve ≤ 21M BTC, price ≤ 10^14) and `supply ≥ ~120`, `num ≤ 2^118` and
  `cr_ppm` stays within `u64`.
- **Zero supply.** Undefined CR (no outstanding SatUSD to back); the tier mapping
  treats it as `Healthy`.

This is the **DL-24 corrected** form. The §5.D8 table's "tiny supply" row originally
printed `5×10^14`, but `reserve=1e8, price=5e12, supply=100` yields `5×10^10`
(reserve $50k / supply $1 = 50000× = 5×10^10 ppm) — a print typo, **already
corrected in the PRD** (§5.D8, v5.2). The formula above is authoritative.

## Consequences

- Rust (`satusd_types::tier::collateral_ratio_ppm`), TypeScript, and the M4a
  circuit (`m4a_cr_tier.circom`, Euclidean `cr_q`/`cr_r` division) all implement
  this exact form, cross-verified on 326 fixtures (see G3 report).
- The circuit assumes `supply > 0`; the `supply = 0 → Healthy` case is enforced by
  the software verifier / state-transition layer, not in-circuit.
- Tier thresholds are fixed in ADR-003's companion table (§5.D8): 150% Healthy,
  130% PauseMint, 110% Auction, below Settlement.

## Alternatives considered

- *Float / rational CR.* Rejected: non-determinism + circuit-incompatibility.
- *Rounding up.* Rejected: floor is conservative (never overstates collateral) and
  matches the in-circuit Euclidean-division gadget.
