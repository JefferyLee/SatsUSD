# ADR-004: Mint finalize CR re-check semantics (G3 / DL-27)

- **Date**: 2026-05-25
- **Status**: accepted (G3 hard gate **passed**)
- **PRD reference**: §5.D1, §5.D8, §5.D11 (two-phase mint), §14.8 (G3), DL-27.
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

Minting is two-phase (§5.D11): `MINT_COMMIT` reserves issuance against an
IssuerPosition, `MINT_FINALIZE` actually increases `sat_usd_supply_atoms`. Between
commit and finalize the price can move and other mints/redemptions can change
supply, so the collateral check done at commit can be stale by finalize. DL-27
requires the binding check at the moment supply actually grows.

## Decision

**The mint-capacity collateral check is evaluated on the _post-mint_ supply at
finalize, using the finalize-time oracle price**, and must clear the Healthy
threshold:

```
post_supply = prev.sat_usd_supply_atoms + requested_mint_atoms
cr = collateral_ratio_ppm(prev.reserve_btc_sats, post_supply, finalize_price_e8)
require cr ≥ TIER_HEALTHY_THRESHOLD_PPM   // 1_500_000 (150%), §5.D8
```

(`satusd-state::mint::apply_mint_finalize`, DL-27.) `MINT_COMMIT` also runs the
same check on post-mint supply as an early pre-check, but finalize is the
authoritative gate — minting may not push the system below 150%.

Separately, every transition recomputes the StateRoot's own
`collateral_ratio_ppm` + `emergency_tier` from `(reserve, supply, price)` via the
ADR-003 formula (so the committed tier always reflects current state).

## Consequences

- A mint that was healthy at commit but would breach 150% at finalize (price drop
  or intervening supply growth) is rejected at finalize (test I-04 / DL-27).
- The check uses `prev.reserve_btc_sats` (reserve is credited by the deposit at
  commit), `post_supply`, and the finalize witness price — all three must be the
  finalize-time values, not the commit-time snapshot.
- Mirrors the ADR-003 CR formula exactly, so finalize gating is consistent with
  the tier the StateRoot commits.

## Alternatives considered

- *Check only at commit.* Rejected (DL-27): stale by finalize; lets a mint breach
  collateralization after a price move.
- *Check on pre-mint supply.* Rejected: understates the post-mint dilution, the
  quantity that actually matters for solvency.
