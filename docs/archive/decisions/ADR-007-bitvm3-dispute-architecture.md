# ADR-007: BitVM3 dispute architecture (lineage + lock-binding sub-circuits, M8)

- **Date**: 2026-05-26
- **Status**: accepted
- **PRD reference**: §5.D15 (lineage / DL-23), §5.D18 (dispute layering / DL-25),
  §11.4 (BitVM3Reserve), §11.5 (BitVM2 fallback / DL-33), §14.14 (G4), §14.16 (M8).
  Relates to ADR-008/009 (DA / circuits), ADR-0022/0023 (claim / committee).
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

In the optimistic reserve (M7+), a reserve claim is **not** verified on-chain on the
happy path; the state node and challenger each verify it off-chain, and a challenger
disputes a fraudulent claim by broadcasting a Disprove that a BitVM3 garbled
sub-circuit adjudicates (§5.D18). Two dispute kinds carry SHA-256 / secp256k1, which
DL-25 keeps **out** of the MVP Groth16 circuit; they live in BitVM3 dispute
sub-circuits triggered only on dispute:

- **lineage-dispute (D15)**: the operator's committed `asset_proof_hash` vs the hash
  of the real TA lineage carried in the DA.
- **lock-binding-dispute (D18)**: the operator's `lock_script_key` vs
  `TapTweak(user_asset_refund_key, lock_tweak(rih, payment_hash))`.

## Decision

1. **The dispute reduces to a recompute-and-compare, modelled off-circuit
   (`satusd-dispute`).** `resolve_dispute(DisputeWitness) -> DisputeOutcome`:
   - lineage: parse + `verify_lineage` the DA proof (else `Malformed`), recompute
     `lineage_proof_hash`; `DisproveSucceeds` iff the operator's claimed hash differs.
   - lock-binding: recompute the tweaked key; `DisproveSucceeds` iff it differs.
   This is exactly what the BitVM3 garbled sub-circuit enforces (SHA-256 chain /
   SHA-256 + secp256k1 tweak); the Rust verifier is the specification + the
   challenger/state-node off-chain check (§5.D18 "both run the lineage verifier"),
   and the source of the on-chain `Disprove { claim_id, claimed, correct }`.

2. **The reserve-backend split stays (DL-7).** `OptimisticEnforcementBackend`
   (`satusd-reserve`) extends `ReserveBackend` with `submit_assert` /
   `submit_disprove` / `finalize_withdraw` / `observe_challenge_window`.
   `PlaygroundReserve` (`satusd-dispute`) is the in-memory model: assert opens a
   challenge window; a successful disprove slashes the claim (the reservation is
   freed, never paid); an un-disproven elapsed window withdraws. BitVM2Reserve and
   BitVM3Reserve implement the **same** trait on-chain (DL-33: the BitVM2 fallback is
   interface-identical, so state node / challenger / wallet are unaffected).

3. **The on-chain BitVM3 stays deferred (G6-gated).** The garbled-circuit gadgets
   (SHA-256 chain; secp256k1 point tweak — infeasible in the MVP Groth16/circom
   surface), the sharded Assert/Disprove/Withdraw/Reclaim tx graph, and the public
   setup ceremony are upstream-dependent signet work (§14.16, 6–18+ months). If
   BitVM3 upstream is not ready by M7 end (advisory gate G6), the BitVM2 fallback
   carries the same trait.

## Consequences

- The dispute logic is a tested verifier now (real grouped-asset burn vector: an
  honest hash → no disprove, a fake hash → disprove, a tampered proof → malformed;
  a mis-bound lock key → disprove). G4 (`docs/discovery/G4_lineage_dispute_report.md`)
  documents how it maps to the sub-circuit; the signet Disprove drill is the gate's
  remaining acceptance.
- `submit_disprove` consumes the `Disprove` the resolver produces — the on-chain
  enforcement layer only needs to prove `claimed != correct`, which the garbled
  sub-circuit does without re-deriving inside Groth16.
- No soundness gap vs. the MVP: the state node already rejects fake lineage / mis-
  bound locks at `REDEEM_FAST_LOCK` (DL-23) and the challenger recomputes (M5); the
  dispute path adds the *optimistic* on-chain enforcement, not a new check.

## Alternatives considered

- *Put lineage / lock-binding verification in the MVP Groth16 circuit.* Rejected by
  DL-25: SHA-256 + secp256k1 blow the gate budget; they belong in the
  dispute-only BitVM3 sub-circuit.
- *Make MultisigReserve pretend to run the optimistic challenge.* Rejected by DL-7:
  it would force a challenger rewrite at the BitVM3 stage; the two-layer trait keeps
  the optimistic path isolated.
