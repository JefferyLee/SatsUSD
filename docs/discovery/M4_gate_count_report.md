# M4 circuit gate-count report (M4a/M4b/M4c)

- Date: 2026-05-25
- Scope: PRD §8 (proof engine), §14 (M4a/b/c), DL-13 (M4 split), DL-25 (circuit
  verification layering). Groth16 / BN254, circom 2.x + circomlib.
- Status: M4a + M4b + the M4c oracle circuit implemented; all cross-checked
  3-way (Rust = TypeScript = circuit) against the shared vectors.

## Constraint counts (non-linear / R1CS)

| Circuit | Purpose | Non-linear constraints |
|---|---|---:|
| `m4a_cr_tier` | CR (Euclidean div) + tier + Poseidon state commit (§5.D1/D8) | 1,035 |
| `m4a_batch_root` | 4-leaf Poseidon batch root (§6.8) | 729 |
| `m4b_smt` | one SMT membership/non-membership fold, height 256 (ADR-0015) | 62,976 |
| `m4b_lock_insert` | §5.D17 set-insert (2 folds: old + new root) | 125,952 |
| `m4b_lock_finalize` | REDEEM_FAST_FINALIZE transition (5 folds) | 314,880 |
| `m4c_oracle_eddsa` | EdDSA-BabyJub verify + Poseidon msg hash (§5.D7) | 7,679 |
| `m4_state_commit` | state_root_hash = fold 18 encoding limbs (§6.1; ADR-006) | 4,374 |
| `m7_transition` | monolithic REDEEM_FAST_FINALIZE: 6 SMT folds + Poseidon-of-fields prev/new commit + root binding (§5.D17, ADR-009) | 336,828 |

## Reading the numbers

- **The SMT fold dominates.** ~62,976 constraints per height-256 fold (~246 per
  level, almost entirely the Poseidon(2) at each level). This is the cost DL-13
  anticipated when splitting M4 — the lock state machine, not the arithmetic, is
  the circuit's weight. `m4b_lock_finalize` = 5 folds (lock_record membership +
  lock_consumed insert ×2 + nullifier insert ×2) ≈ 315k.
- **EdDSA is cheap** (~7.7k) — circomlib's `EdDSAPoseidonVerifier` over a Poseidon
  challenge is far lighter than the SMT folds. The Poseidon-friendly oracle
  signature is comfortably in-circuit.
- **Arithmetic is negligible** (cr_tier ~1k, batch_root ~0.7k).

## Optimization levers (for M4c+ / a full transition circuit)

1. **Shared sibling paths.** In `lock_finalize`, the lock_record membership fold
   and the lock_consumed insert use the *same key* (`lock_record_hash`) but
   different trees (different siblings), so they can't share folds directly. A
   combined SMT that proves membership-in-A + insert-in-B with one path is only
   possible if the trees share structure (they don't here).
2. **Tree height.** Height 256 is keyed by full 32-byte hashes. A protocol-chosen
   shorter key space (e.g. 160-bit) would cut folds ~38%. Out of scope for the
   frozen ADR-0015 conventions, noted as a future lever.
3. **Batching.** A batch of N redemptions multiplies the per-redemption fold cost
   (≈315k × N). The §6.8 batch root + a recursion/aggregation layer (M7+) is the
   intended path for batches, not a single monolithic circuit.

## Scope boundary (DL-25)

The MVP Groth16 circuit verifies **only Poseidon-friendly commitments**: CR/tier,
Poseidon SMT folds, the §6.8 batch root, and the EdDSA-Poseidon oracle signature.
**SHA256 and secp256k1 verification stay off-circuit** — BTC SPV (§5.D14),
Taproot Asset lineage (DL-23), and TapTweak (§5.D16/D3) are enforced by the
software verifier (`satusd-state::spv`, `satusd-ta-proof`) and re-computed by the
challenger; the BitVM3 stage (M8+) adds the lineage/lock-binding dispute
sub-circuits. So the gate counts above are the complete MVP in-circuit surface.

## Cross-language assurance

Every circuit's output is checked against the shared `integration/vectors`
(`circuits/check.sh`): cr_tier (326), batch_root (80), smt (5), smt_insert (2),
lock_finalize (1), oracle_eddsa (1). Rust generates, TypeScript re-derives, and
the circuit re-computes — a 3-way match. The EdDSA signature is additionally
cross-verified by two independent implementations (Rust `babyjubjub-rs` + circom
`EdDSAPoseidonVerifier`), with circomlibjs as the canonical unpacking reference.
circomlib's Poseidon/EdDSA are the same primitives the Rust reference was pinned
to (ADR-0013/0014), so there is zero parameter-alignment risk.

## Remaining M4

- A full state-transition circuit composing the lock folds with the StateRoot
  Poseidon commitment + linkage (prev → new), per transition type.
- Trusted setup (Groth16 powers-of-tau + circuit-specific) for proving/verifying
  keys — deferred to the signet stage; the witness-calculator cross-checks here
  need no setup.
