#!/usr/bin/env bash
# Real Groth16 setup → prove → verify for the monolithic m7_transition circuit.
#
# m7 has 710,584 total constraints (336,828 non-linear + 373,756 linear); Groth16's
# phase-2 domain needs 2^power >= 2 * constraints, i.e. **2^21** powers of tau (not
# 2^19 — the linear constraints + the 2n factor count). Generating a fresh 2^21 ptau
# in snarkjs (pure JS) needs multiple GB and ~1h, and tends to OOM in a small dev
# sandbox — hence `NODE_OPTIONS=--max-old-space-size`. For a machine without the
# budget, the evidence that the full Groth16 pipeline works is `prove.sh`
# (m4a_cr_tier, fresh 2^14, `snarkJS: OK!`), and the m7 witness-calc 3-way cross-check
# (circuits/check.sh) proves the m7 circuit is satisfiable (⇒ a valid proof exists).
# The single-contributor ceremony here is dev-only, NOT a production trusted setup.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build keys
export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=8192}"
SNARKJS="npx --no-install snarkjs"
POT=keys/pot21_final.ptau

echo "== compile m7_transition (r1cs + wasm) =="
circom m7_transition.circom --r1cs --wasm -l node_modules -o build

echo "== powers of tau 2^21 (local, dev — heavy, may need a big-memory host) =="
if [ ! -f "$POT" ]; then
  $SNARKJS powersoftau new bn128 21 keys/pot21_0.ptau -v
  $SNARKJS powersoftau contribute keys/pot21_0.ptau keys/pot21_1.ptau \
    --name="satusd dev" -v -e="satusd m7 dev entropy"
  $SNARKJS powersoftau prepare phase2 keys/pot21_1.ptau "$POT" -v
fi

echo "== groth16 setup (m7_transition) =="
$SNARKJS groth16 setup build/m7_transition.r1cs "$POT" keys/m7_0.zkey
$SNARKJS zkey contribute keys/m7_0.zkey keys/m7.zkey --name="satusd dev" -v -e="satusd m7 dev 2"
$SNARKJS zkey export verificationkey keys/m7.zkey keys/m7_vk.json

echo "== witness + prove + verify =="
node m7_input.mjs
node build/m7_transition_js/generate_witness.js \
  build/m7_transition_js/m7_transition.wasm build/m7_input.json build/m7_witness.wtns
$SNARKJS groth16 prove keys/m7.zkey build/m7_witness.wtns build/m7_proof.json build/m7_public.json
$SNARKJS groth16 verify keys/m7_vk.json build/m7_public.json build/m7_proof.json

echo "== m7 public outputs [prevCommit, newCommit] =="
cat build/m7_public.json
