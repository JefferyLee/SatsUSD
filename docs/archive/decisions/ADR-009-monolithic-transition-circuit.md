# ADR-009: Poseidon-over-fields state commit + monolithic transition circuit (M7)

- **Date**: 2026-05-26
- **Status**: accepted
- **PRD reference**: §6.1 (StateRoot), §8 (proof engine), §5.D17 (lock state
  machine), §14.15 (M7), DL-25. Realizes the direction set in **ADR-006** (which
  deferred this to M7); relates to ADR-0013 (byte-oriented `state_root_hash`),
  ADR-0015 (SMT).
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

ADR-006 deferred the monolithic in-circuit transition because binding individual
StateRoot roots against the byte-oriented `state_root_hash` (a Poseidon fold over
31-byte limbs of the canonical encoding) means extracting 32-byte root fields that
span 31-byte limb boundaries — heavy, error-prone byte plumbing. It set the
direction: switch the in-circuit commitment to **Poseidon-over-fields**.

## Decision

1. **Poseidon-over-fields state commitment (`state_commit_fields`).** The StateRoot
   is committed as an ordered list of BN254 field elements
   (`satusd_crypto::state::state_field_elements`): each scalar field directly, each
   32-byte value as two 128-bit big-endian limbs (hi, lo), in struct declaration
   order — **42 elements** — Poseidon2-folded (`acc_0 = 0`,
   `acc_i = Poseidon2(acc_{i-1}, field_i)`). Splitting every 32-byte value into two
   ≤128-bit limbs guarantees each is < Fr regardless of derivation (Poseidon or
   SHA256), and a Poseidon root (which is < Fr) reconstructs **exactly** as
   `hi·2^128 + lo`. 3-way pinned: Rust = TS (`liveDaRootHex` fold) = circuit
   (`m7_transition` `Fold(42)` outputs).

2. **`state_commit_fields` is parallel to, not a replacement for,
   `state_root_hash`.** The byte-oriented hash (ADR-0013) remains the canonical
   wire/storage commitment; the field commitment is the circuit-facing one. This
   avoids rippling the frozen cross-language vectors / every verifier, as ADR-006
   required. A future unification (single canonical commitment) is out of scope.

3. **Monolithic transition circuit (`m7_transition.circom`).** A single circuit for
   REDEEM_FAST_FINALIZE (one redemption) composes the §5.D17 lock state machine
   (3 SMT folds: lock_record membership + lock_consumed insert + nullifier insert)
   with the prev/new field commitments, and **binds the lock roots embedded in the
   committed state**: it reconstructs each root from its (hi, lo) limbs — each limb
   range-checked to 128 bits (`Num2Bits`) so the reconstruction is unique — and
   asserts it equals the corresponding fold output. It also binds the supply
   decrement (`prev.supply == new.supply + amount`) and epoch advance. Outputs the
   prev/new commitments. ~336,828 non-linear constraints (the 6 height-256 SMT
   folds dominate).

## Consequences

- The transition's structural linkage (roots ↔ folds, supply, epoch) is now a
  single in-circuit relation, no longer only software-verified + challenger-
  recomputed. Witness-calculator 3-way cross-check is green (`check.sh`:
  `m7_transition` — commits match Rust/TS, all bindings satisfiable).
- The full Groth16 setup→prove→verify loop runs end-to-end with a fresh local ptau
  (`prove.sh`, m4a_cr_tier, `snarkJS: OK!`) — no downloaded ptau. `m7_transition`
  needs `PTAU_POWER=19` (≈390k constraints); its key generation is the documented
  `setup.sh` recipe (run on demand; a real ceremony for production).
- Still single-redemption and Poseidon-friendly only (DL-25): oracle EdDSA / cr-tier
  are separate gadgets composable on top; SHA256/secp (SPV, lineage, TapTweak) stay
  off-circuit (software verifier + challenger). Batching N redemptions and the
  recursion/aggregation layer remain future work.

## Alternatives considered

- *Bind roots against the byte-oriented `state_root_hash` in-circuit.* Rejected per
  ADR-006: limb-spanning extraction is heavy and bug-prone.
- *Migrate the canonical commitment to Poseidon-of-fields now.* Rejected: ripples
  the frozen vectors + every verifier; the parallel commitment is sufficient for the
  circuit and keeps the wire format stable.
- *Skip the 128-bit range checks (witness-calc only).* Rejected: without them a
  prover could pick (hi, lo) that wrap mod Fr to forge a root match; the checks are
  ~1.5k constraints, negligible against the folds.
