# SatUSD circuits (M4)

circom + circomlib + Groth16/BN254 (ADR-005). The pure-Rust `satusd-state`
software verifiers are the spec; circuit constraints are derived from them.

## M4a skeleton — `m4a_cr_tier.circom`

Proves the DL-24 collateral-ratio math + tier bucketing + a Poseidon state
commitment (no SMT, no signatures — those are M4b/M4c):

- `cr_ppm = reserve_sats·price_e8·1e6 / (supply_atoms·1e14)` as an in-field
  Euclidean division proven by the `(cr_q, cr_r)` witness, with u64 range checks
  and `0 ≤ cr_r < den`.
- `tier = 3 − [cr≥1.5M] − [cr≥1.3M] − [cr≥1.1M]` (Healthy/PauseMint/Auction/Settlement).
- `state_commit = Poseidon(reserve, supply, price, cr_ppm, tier)` (circomlib BN254).

~1570 constraints (1035 non-linear + 535 linear). Public outputs:
`[cr_ppm, tier, state_commit]`.

## M4a batch root — `m4a_batch_root.circom`

A 4-leaf binary Poseidon merkle matching `satusd_crypto::poseidon::batch_root`
(the §6.8 ReserveClaim batch-root convention: poseidon2 compression, pad to the
next power of two with the field zero). Real batches (≤ MAX_BATCH_SIZE = 64) are
the same construction with more levels.

## Cross-check (Rust = TS = circuit)

`check.sh` compiles both circuits (witness calculators only — fast, no trusted
setup) and runs the circuit's outputs against the shared integration vectors that
the Rust `satusd-state`/`satusd-crypto` and TS reference already agree on:

```sh
cd circuits && npm install && bash check.sh
# circuit ↔ vectors: 7 tier vectors match (cr_ppm + tier)
# circuit ↔ vectors: 80 batch_root vectors match
```

So the circuit's `cr_ppm`/`tier`/`batch_root` are pinned to the same vectors as
the software verifier — a 3-way consistency check.

## Run (full Groth16 prove/verify)

```sh
cd circuits
npm install                 # snarkjs + circomlib
bash prove.sh               # compile → dev Groth16 setup → prove input.json → verify
```

Requires `circom` 2.x on PATH (build from source: `git clone
https://github.com/iden3/circom && cd circom && cargo build --release`, copy the
binary onto PATH).

`prove.sh`'s powers-of-tau / zkey contributions are **dev-only**, not a real
trusted-setup ceremony (that is a Pilot/M5+ concern). Build artifacts
(`build/`, `*.ptau`, `*.zkey`, …) and `node_modules/` are gitignored.

`input.json` is the 150%-collateral case → `cr_ppm=1500000, tier=0`.
