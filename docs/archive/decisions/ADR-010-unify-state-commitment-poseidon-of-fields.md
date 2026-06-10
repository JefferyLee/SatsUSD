# ADR-010: Unify the state commitment on Poseidon-over-fields (retire byte form)

- **Date**: 2026-05-26
- **Status**: accepted
- **PRD reference**: §6.1 (StateRoot / state_root_hash), §18.1 (canonical encoding),
  §8 (proof engine). Supersedes the parallel-commitment decision in **ADR-009 #2**;
  relates to ADR-006 (M7 direction), ADR-0013 (Poseidon byte packing).
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

The protocol had **two** commitments over the same `StateRoot`:

- `state_root_hash` = `hash_bytes(canonical_encode(StateRoot))` — byte-oriented
  (31-byte limbs of the canonical encoding; ADR-0013), used everywhere on-chain /
  wire (`claim.{prev,new}_state_root`, the StateRoot linkage, `claim_id`).
- `state_commit_fields` = Poseidon-over-fields (each scalar directly, each 32-byte
  value as hi/lo 128-bit limbs; ADR-009), used **only** by the m7 transition circuit.

ADR-009 added the field form in parallel and explicitly did *not* replace the byte
form, to avoid re-freezing the cross-language vectors. The cost was real tech debt:
two hashes of the same state (divergence risk) and — more importantly — the m7
circuit's `prev/new` commitments were **not** the on-chain `state_root_hash`, so a
Groth16 m7 proof did not bind to the on-chain commitment.

## Decision

1. **`state_root_hash` IS the Poseidon-over-fields commitment.** The byte-oriented
   `hash_bytes(canonical_encode)` form is **retired**. `state_root_hash(s)` now folds
   `state_field_elements(s)` (the 42 field elements, declaration order) with
   `poseidon2` (`acc_0 = 0`). One canonical commitment. The separate
   `state_commit_fields` function is removed (it *was* this).

2. **Everything downstream auto-migrates.** The ~22 `satusd-state` call sites call
   `state_root_hash()` and recompute (linkage checks, `claim_id` inputs) — unchanged.
   Cross-language: TS `stateCommitFieldsHex` derives the 42 fields from the StateRoot
   struct and folds (byte-matches Rust; `make verify` green). The byte `state_commit`
   vector + the `m4_state_commit.circom` gadget are retired; the canonical fold's
   in-circuit form is `m7_transition`'s `Fold(42)` (its `prevCommit`/`newCommit` now
   equal the on-chain `state_root_hash`).

3. **`hash_bytes` stays** — it is the SMT leaf primitive
   (`poseidon2(hash_bytes(key), hash_bytes(value))`), independent of this change.

## Consequences

- **The commitment-binding gap closes**: m7's public `prev/new` commitments now equal
  `claim.{prev,new}_state_root`. (This is *necessary* for a consensus-authoritative
  succinct proof, not *sufficient* — m7 still proves only a subset of the transition
  rules; the full monolithic transition circuit remains future work.)
- **New constraint (accepted cost)**: adding/removing a `StateRoot` field now requires
  (a) updating `state_field_elements` (Rust) + `stateCommitFieldsHex` (TS), (b) bumping
  `m7_transition.circom`'s `NF` (=42) **and its field-index map** (EPOCH/SUPPLY/root
  positions), and (c) re-freezing the cross-language vectors. The byte form needed
  only a re-encode. This brittleness is the price of a circuit-bindable commitment;
  keep the index map in `m7_transition.circom` in sync with the struct.
- One-time re-freeze: `vectors.json` regenerated (every `state_root_hash` / `claim_id`
  / pending-claim value changed). `make check` + `circuits/check.sh` green.

## Alternatives considered

- *Keep both (ADR-009 status quo).* Rejected now: the divergence debt + unbound proof
  outweigh the re-freeze churn (the migration is a single function + one TS deriver +
  a vector regen, because expectations are computed, not literal).
- *Keep byte canonical + bridge to fields in-circuit.* Rejected (ADR-006): proving the
  42 field limbs equal the 31-byte encoding limbs is exactly the cross-limb-boundary
  byte plumbing the field form exists to avoid.
