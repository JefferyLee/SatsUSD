#!/usr/bin/env bash
# Real Groth16 setup → prove → verify for the monolithic m7_transition circuit
# (~336k constraints ⇒ 2^19 powers of tau, generated locally — no download).
# Heavy: phase-2 prep + setup take minutes and several GB. The single-contributor
# ceremony here is dev-only, NOT a production trusted setup.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build keys
SNARKJS="npx --no-install snarkjs"
POT=keys/pot19_final.ptau

echo "== compile m7_transition (r1cs + wasm) =="
circom m7_transition.circom --r1cs --wasm -l node_modules -o build

echo "== powers of tau 2^19 (local, dev) =="
if [ ! -f "$POT" ]; then
  $SNARKJS powersoftau new bn128 19 keys/pot19_0.ptau -v
  $SNARKJS powersoftau contribute keys/pot19_0.ptau keys/pot19_1.ptau \
    --name="satusd dev" -v -e="satusd m7 dev entropy"
  $SNARKJS powersoftau prepare phase2 keys/pot19_1.ptau "$POT" -v
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
