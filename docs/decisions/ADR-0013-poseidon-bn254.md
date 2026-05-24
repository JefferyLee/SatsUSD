# ADR-0013: Poseidon parameterization and byte→field packing

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §6.1, §5.D7, §8, §21.1
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

The protocol hashes state with Poseidon over BN254 (`state_root_hash =
Poseidon(canonical_encode(StateRoot))`, SMT leaves `Poseidon(key, value)`). Two
things are under-specified: (a) which Poseidon parameter set, and (b) how byte
strings become field elements (Poseidon consumes field elements, not bytes;
BN254 Fr is < 2^254, so a 32-byte value can exceed it).

## Options Considered

1. **circomlib/iden3 Poseidon over BN254 Fr** + 31-byte big-endian limb folding.
   - Pros: forced-by-consistency with the EdDSA-BabyJubjub oracle scheme already
     pinned to "iden3/circom convention" (§5.D7); compatible with circom/arkworks
     M4 circuit; `light-poseidon` (Rust) and `circomlibjs` (TS) both implement it
     and byte-match (`poseidon([1,2]) = 0x115cc0f5…189a`).
   - Cons: tied to the iden3 constants.
2. Poseidon2 / arkworks-native params.
   - Cons: contradicts the EdDSA-BabyJubjub commitment; no toolchain alignment.

## Decision

- **Poseidon = circomlib-compatible Poseidon over BN254 Fr.** `light-poseidon`
  `new_circom(n)` in Rust; `circomlibjs` in TS. Pinned: `ark-bn254`/`ark-ff` 0.5
  (to match `light-poseidon` 0.4).
- **Field element serialization:** 32-byte big-endian.
- **`hash_bytes` (arbitrary bytes → 1 field element):** split big-endian into
  31-byte limbs (each < Fr, lossless), fold with the 2-input compression
  `acc_0 = 0`, `acc_i = poseidon2(acc_{i-1}, limb_i)`. Empty input → field zero.
- **`state_root_hash` per-field mapping is DEFERRED** to M2/M4b (it is coupled to
  the in-circuit witness layout). M0 freezes only the Poseidon primitive,
  `poseidon2`, and `hash_bytes`. Open question tracked here until then.

## Consequences

- Positive: Rust/TS byte-match on poseidon2 and hash_bytes vectors; consistent
  with the oracle signature stack and future circuit.
- Trade-off: `ark` 0.5 pin is dictated by `light-poseidon`; revisit when the M4a
  toolchain ADR-005 lands (may switch proving stack but must keep these outputs).
- Follow-up: define `state_root_hash` field mapping in M2; add its vectors then.

## References

- `crates/satusd-crypto/src/poseidon.rs`
- `integration/vectors/vectors.json` (`op: poseidon2`, `op: hash_bytes`)
- Cross-check: circomlibjs `buildPoseidon`, `poseidon([1,2])`.
