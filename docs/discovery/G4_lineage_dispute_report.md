# G4 report: TA lineage dispute path (lineage + lock-binding sub-circuits)

- Date: 2026-05-26
- Scope: PRD §5.D15 (lineage / DL-23), §5.D18 (dispute layering), §11.3/§11.4
  (optimistic / BitVM3 backends), §13.7 (BLC-04), §14.14 (G4 gate). Relates to
  ADR-007.
- Status: dispute **logic** implemented + tested (`satusd-dispute`); the on-chain
  signet Disprove drill (the gate's live acceptance) is the remaining deferred step.

## What the gate proves

G4 (hard gate before BitVM3 setup) requires the lineage-dispute sub-circuit
**designed** and a fake lineage **Disproven** on signet (BLC-04 + RB-04). The
sub-circuit's adjudication logic is the verifiable core; this report records it +
the design, and marks the signet tx-level drill as the live-pilot remainder.

## The dispute reduces to recompute-and-compare

A challenger disputes by recomputing the correct value from the data and showing the
operator's committed value differs (§5.D18). `satusd-dispute::resolve_dispute`:

| Dispute | Recompute (correct) | DisproveSucceeds iff |
|---|---|---|
| lineage (D15) | `lineage_proof_hash(real DA proof)`, after `verify_lineage` | operator's `asset_proof_hash` ≠ correct |
| lock-binding (D18) | `TapTweak(user_asset_refund_key, lock_tweak(rih, payment_hash))` | operator's `lock_script_key` ≠ correct |

A lineage whose proof does not itself verify yields `Malformed` (the challenger must
offer the valid real lineage to have something authoritative to disprove against).

## Results (against the real grouped-asset vector)

Using `integration/lineage_vectors/burn_to_sink.hex` (a real grouped SatUSD lineage)
and a canonical intent:

- lineage, honest hash → `DisproveFails` (no fraud).
- lineage, fake hash → `DisproveSucceeds { claimed, correct }` (BLC-04: fake lineage
  caught).
- lineage, tampered proof → `Malformed`.
- lock-binding, derived key → `DisproveFails`; mis-bound key → `DisproveSucceeds`.

The optimistic lifecycle (`PlaygroundReserve`, §11.3) ties it together: assert opens
a challenge window; an un-disproven elapsed window withdraws (pays); a successful
`submit_disprove` slashes the claim (reservation freed, never paid).

## Mapping to the BitVM3 sub-circuit

The garbled sub-circuit proves `claimed != correct` where `correct` is recomputed
from the DA. For lineage that is a SHA-256 chain over the proof bytes
(`lineage_proof_hash`); for lock-binding it is SHA-256 (the `lock_tweak` tagged hash)
plus a secp256k1 point tweak (`TapTweak`). Per DL-25 these stay out of the MVP
Groth16 surface and run only on the dispute path. `resolve_dispute` is the
specification the garbled circuit must match, and is the off-chain check the state
node + challenger both run (§5.D18 MVP stage).

## Deferred (the gate's live remainder + beyond)

- The on-chain signet Disprove drill (BLC-04 + RB-04 live) — needs the
  OptimisticPlayground / BitVM2 tx graph on signet.
- The garbled sub-circuits themselves (SHA-256 chain; secp256k1 tweak — needs
  circom-ecdsa-class tooling or BitVM3 garbling), the sharded reserve graph, and the
  setup ceremony (G6-gated, upstream-dependent — §14.16 M8).

## Assurance

`satusd-dispute`: 6 tests (4 dispute-resolution + 2 optimistic-lifecycle). The
lineage cases run against the same real vector the M3 lineage verifier and the M5
challenger use, so the dispute resolver, the state-node verifier, and the challenger
agree on what a valid lineage is.
