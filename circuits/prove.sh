#!/usr/bin/env bash
# Compile the M4a circuit, run a dev Groth16 setup, prove input.json, verify.
# Requires: circom on PATH + `npm install` here (snarkjs + circomlib).
# NOTE: the powers-of-tau / zkey "contributions" are dev-only, NOT a real
# trusted-setup ceremony (that is a Pilot/M5+ concern).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build
SNARKJS="npx --no-install snarkjs"

echo "== compile =="
circom m4a_cr_tier.circom --r1cs --wasm --sym -l node_modules -o build

echo "== powers of tau (dev) =="
if [ ! -f build/pot.ptau ]; then
  $SNARKJS powersoftau new bn128 14 build/pot_0.ptau -v
  $SNARKJS powersoftau contribute build/pot_0.ptau build/pot_1.ptau --name="dev" -v -e="satusd dev entropy"
  $SNARKJS powersoftau prepare phase2 build/pot_1.ptau build/pot.ptau -v
fi

echo "== groth16 setup =="
$SNARKJS groth16 setup build/m4a_cr_tier.r1cs build/pot.ptau build/circuit_0.zkey
$SNARKJS zkey contribute build/circuit_0.zkey build/circuit.zkey --name="dev" -v -e="satusd dev entropy 2"
$SNARKJS zkey export verificationkey build/circuit.zkey build/vkey.json

echo "== witness + prove + verify =="
node build/m4a_cr_tier_js/generate_witness.js \
  build/m4a_cr_tier_js/m4a_cr_tier.wasm input.json build/witness.wtns
$SNARKJS groth16 prove build/circuit.zkey build/witness.wtns build/proof.json build/public.json
$SNARKJS groth16 verify build/vkey.json build/public.json build/proof.json

echo "== public outputs [cr_ppm, tier, state_commit] =="
cat build/public.json
