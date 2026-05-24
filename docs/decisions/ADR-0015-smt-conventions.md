# ADR-0015: Sparse Merkle tree conventions (height 256, Poseidon)

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §6.1, §5.D17, §8 (M4b)
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

§6.1 specifies "Sparse Merkle Tree of height 256, leaf hash = Poseidon(key,
value)" but leaves the rest open: path derivation, empty-node value, internal
node hash, and how a 32-byte key/value maps to field elements. These must be
fixed so the M4b circuit and the software verifier agree.

## Decision

- **Height 256.** A key is 32 bytes; its path is the 256 bits most-significant
  first (depth 0 = MSB of byte 0).
- **Empty leaf = field zero.** Empty subtree of height `h`: `E[0] = 0`,
  `E[h] = poseidon2(E[h-1], E[h-1])`.
- **Occupied leaf** = `poseidon2(hash_bytes(key), hash_bytes(value))` (ADR-0013
  `hash_bytes`, so 32-byte key/value are bound losslessly).
- **Internal node** = `poseidon2(left, right)`.
- **Membership proof** = 256 sibling node values indexed by depth (0 = top);
  verification folds the leaf up applying path bits (bit 0 → `poseidon2(cur,
  sib)`, bit 1 → `poseidon2(sib, cur)`).
- **Non-membership of a key** = an inclusion proof that its leaf is the empty
  (zero) leaf. This directly supports the D17 lock state machine: a nullifier /
  lock-consumed key proves absence before insertion and cannot prove absence
  after (tested).

## Consequences

- Positive: O(1)-state per key, simple non-membership = empty-leaf inclusion;
  matches the D17 `active → consumed/refunded` enforcement. Reference + tests in
  Rust (M0). Cross-language vectors deferred (TS reference is hash/encoding-only
  at M0; the circuit formalizes the SMT at M4b).
- Trade-off: this is the M0 reference convention; M4b must implement the exact
  same leaf/empty/fold rules in-circuit.

## References

- `crates/satusd-crypto/src/smt.rs`
