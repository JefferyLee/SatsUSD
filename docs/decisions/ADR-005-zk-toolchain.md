# ADR-005: zk toolchain — circom + circomlib + Groth16/BN254

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §8 (circuits), §8.4 (M4 split), §5.D18 (circuit boundary), §14 M4a
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

M4a must pick the circuit toolchain (the §8.4 "Halo2/arkworks 选型" risk; reserved
ADR-005). Hard constraints from the PRD:

- **Proving system: Groth16 on BN254** — gate-count targets, <5 min proving
  (batch 32), and BitVM3 compatibility all assume Groth16. This makes Halo2 /
  Plonky3 a *fallback* (PLONKish, not Groth16), used only if gate count explodes
  (§17 risk row).
- **Pinned primitives are circom-native**: oracle sigs are **EdDSA-BabyJub**
  ("iden3/circom convention", §References) and our Poseidon is
  **circomlib-compatible** (ADR-0013). `circomlib` *is* the reference
  implementation of exactly these gadgets.

The PRD framed the choice as "arkworks vs Halo2 (vs plonky3)" — Rust-native — but
that predates weighing the circomlib primitive commitments.

## Options Considered

1. **circom + circomlib (Groth16/BN254)** — chosen. circomlib provides the exact
   Poseidon + EdDSA-BabyJub gadgets we pinned, so the circuit's commitments match
   the wallet/state-node hashes with **zero parameter-matching risk** — decisive
   for M4c's 5× EdDSA-BabyJub (the hardest part). Mature Groth16 toolchain
   (snarkjs/rapidsnark). Cost: circom DSL (not Rust), an extra Node/circom
   toolchain, and a per-circuit trusted setup.
2. **arkworks (ark-groth16 + r1cs-std)** — Rust-native, reuses ark-bn254. But we
   would have to re-implement circomlib-byte-compatible Poseidon + EdDSA-BabyJub
   gadgets in R1CS (matching effort/risk), with a smaller gadget ecosystem.
3. **Halo2 / Plonky3** — PLONKish, not Groth16; kept as the gate-count-overrun
   fallback only.

## Decision

Use **circom 2 + circomlib + Groth16 over BN254** (snarkjs for setup/prove/verify;
rapidsnark later for speed). Circuits live in `circuits/`; the §8.1 software
verifiers (`satusd-state`) remain the spec from which circuit constraints are
derived, and the cross-language test vectors (Rust/TS, circomlib Poseidon)
already pin the in-circuit hash parameters.

## Consequences

- New toolchain: `circom` compiler (built from source) + `snarkjs` + `circomlib`
  (npm); build artifacts (`*.r1cs`, `*.zkey`, `*.ptau`, witnesses) are gitignored.
- M4a delivers the skeleton: Poseidon state-hash gadget + range-checked u128
  CR/mint math (DL-24) + tier bucketing, compiled and proven/verified end-to-end —
  no SMT, no signatures (those are M4b/M4c).
- Per-circuit Groth16 trusted setup is required; a real ceremony is a Pilot/M5+
  concern (MVP uses a local dev setup).

## References

- `circuits/`, PRD §8.4, §5.D18; circom/circomlib (iden3)
