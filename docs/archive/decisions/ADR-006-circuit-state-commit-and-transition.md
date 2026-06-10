# ADR-006: In-circuit state commitment + the full transition circuit (M4)

- **Date**: 2026-05-25
- **Status**: accepted
- **PRD reference**: §6.1 (StateRoot / state_root_hash), §8 (proof engine), §14
  (M4b/M4c), DL-13, DL-25. Relates to ADR-0013 (Poseidon byte packing).
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

M4 needs the SatUSD state transition expressed for the Groth16 circuit:
SMT lock folds + the StateRoot commitment + the prev→new linkage. The SMT folds
(§5.D17) and the oracle EdDSA (§5.D7) are implemented and 3-way aligned
(`m4b_*`, `m4c_oracle_eddsa`). This ADR records how the **state commitment** and a
**monolithic transition circuit** fit.

`state_root_hash = hash_bytes(canonical_encode(StateRoot))` (§6.1, ADR-0013): the
536-byte canonical encoding is folded as 31-byte big-endian limbs via poseidon2.

## Decision

1. **StateCommit gadget (now).** `m4_state_commit.circom::StateCommit(18)`
   reproduces `state_root_hash` by folding the 18 encoding limbs — 3-way aligned
   (Rust `state_root_hash` = TS `foldLimbsHex` = circuit). This proves the
   canonical state commitment is reproducible in-circuit.

2. **MVP proving = per-gadget circuits + software verifier ties them.** The
   circuit proves the expensive Poseidon-friendly pieces individually (cr/tier,
   batch_root, SMT folds, oracle EdDSA, state commit). The **software verifier**
   (`satusd-state`) computes `state_root_hash` from the structured StateRoot and
   binds the SMT roots / oracle / SPV (re-computed by the challenger). This matches
   DL-25: the MVP does not put a monolithic transition in one Groth16 circuit.

3. **The monolithic in-circuit transition is M7-deferred, and should switch to a
   Poseidon-over-fields state commit.** A single circuit that re-derives
   `state_root_hash` from fields *and* binds individual roots (e.g. asserting the
   new encoding's `lock_record_root` equals the lock fold's output, and
   `new.prev_state_root == prevCommit`) must extract 32-byte root fields that
   **span 31-byte limb boundaries** — heavy, error-prone byte plumbing. The clean
   path is a **Poseidon-over-fields** commitment (each StateRoot field a field
   element, Poseidon-folded; 32-byte roots already are field elements), which lets
   the transition circuit bind fields directly. Adopting it is an M7 (BitVM3-stage)
   change that would amend §6.1 / ADR-0013; the byte-oriented `hash_bytes` commit
   stays for the off-circuit storage/wire layer until then.

## Consequences

- M4's in-circuit surface is complete and 3-way aligned at the gadget level
  (arithmetic, Poseidon commitments incl. state commit, EdDSA). Gate counts: see
  `docs/discovery/M4_gate_count_report.md`.
- The transition's structural linkage is software-verified + challenger-recomputed
  in the MVP (per DL-25); no soundness gap (the challenger re-runs the same
  checks), just not a single succinct proof yet.
- M7 introduces the monolithic transition circuit + a Poseidon-over-fields state
  commit; the SMT-fold / EdDSA / batch-root / state-commit gadgets here are its
  building blocks.

## Alternatives considered

- *Byte-plumb the field binding now.* Rejected for the MVP: ~hundreds of
  Num2Bits/offset constraints to extract limb-spanning roots, high bug risk, and
  superseded by the Poseidon-of-fields migration at M7 anyway.
- *Switch the canonical state_root_hash to Poseidon-of-fields immediately.*
  Rejected now: it ripples through every verifier + the frozen cross-language
  vectors; sequence it deliberately at M7 when the monolithic circuit needs it.
