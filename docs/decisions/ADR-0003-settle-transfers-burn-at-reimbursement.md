# ADR-0003: SETTLE moves the SatUSD out of the user's control; the burn obligation attaches to reimbursement

- **Date**: 2026-06-10
- **Status**: accepted
- **Authority**: MISSION v2 via ADR-0001; amends spec 02 §3.1/§3.3/§4
- **Evidence**: the Rail-0 J3 devnet E2E
  (`crates/satusd-rail0/tests/devnet_swap.rs`, anchor tx
  `90d13a12…22825a5e` on regtest, 2026-06-10)
- **Author**: Jeffery (decision) + AI agent (drafting)

## Context

Spec 02 §3.3 (v0.1) required a conforming SETTLE transaction to
atomically burn the quoted SatUSD to the sink. Implementing Rail-0
exposed two facts:

1. **Mechanical**: the tapd-native burn key is derived from the
   burn's *first input PrevID* (validated live, see
   `satusd-rail0::burn_key`), which is only known after funding —
   a burn-in-swap requires mutating the funded vPSBT's output
   script key before signing. Possible, but invasive.
2. **Conceptual** (the deeper one): a settle whose TA leg lands on
   the **LP's script key** is simply a P2P trade. The user's
   guarantee — quoted BTC against surrendered SatUSD, atomically —
   is identical in both destinations. What the burn actually
   protects is not the user but the **supply/reserve invariant**,
   and that invariant is enforced at a different point: the reserve
   reimburses only against a burn artifact (spec 04 §1). Forcing
   the burn into the settle transaction conflated the user↔LP leg
   with the LP↔reserve leg.

## Decision

1. A conforming SETTLE moves the quoted SatUSD **out of the user's
   control**, atomically with the user's BTC payout. The TA leg's
   destination is the LP's choice:
   - **settle-to-LP**: a pure P2P trade. The LP may recirculate the
     SatUSD (market making). No reserve interaction, no capacity
     consumed.
   - **settle-to-burn**: the tapd-native burn key in the settle
     transaction itself; the LP claims reimbursement afterwards.
2. **The burn obligation attaches to reimbursement, not to
   settle.** Spec 04 §1's rule — reimbursement strictly requires a
   burn artifact — is now load-bearing for conservation: supply
   decreases exactly by burns, and every sat leaving the reserve is
   backed one-for-one by a burn.
3. S2 (conservation) is restated accordingly in spec 02 §4.
4. Both settle modes produce S3 artifacts and both feed the
   internal price history — a P2P trade is exactly as much a price
   attestation as a redemption.

## Consequences

- Redemption and market-making unify under one settle primitive;
  the Rail-0 implementation as built (settle-to-LP) is conforming.
- The settle-to-burn variant remains available as a UX optimization
  for direct redemption (its prerequisite, the DeriveBurnKey
  replication, is implemented and devnet-validated); building it is
  optional, not required for conformance.
- Capacity (spec 02 §6) semantics are unchanged: it caps
  reimbursements, which are the only reserve draw.
- PRD J3 wording updated to match; spec 02 bumped to v0.2-draft.
- A collapsed rail's `lock_template_commitment` is now defined as a
  commitment to the settle's full economic plan (spec 02 §3.1) —
  the user recomputes it and refuses to co-sign deviations.
