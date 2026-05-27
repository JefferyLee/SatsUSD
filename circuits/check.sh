#!/usr/bin/env bash
# Compile the M4a/M4b circuits (witness calculators only) and cross-check their
# outputs against the shared integration vectors → Rust = TS = circuit.
# Requires: circom on PATH + `npm install` here. Fast (no trusted setup).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build
circom m4a_cr_tier.circom    --wasm -l node_modules -o build
circom m4a_batch_root.circom --wasm -l node_modules -o build
circom m4b_smt.circom         --wasm -l node_modules -o build
circom m4b_lock_insert.circom --wasm -l node_modules -o build
circom m4b_lock_finalize.circom --wasm -l node_modules -o build
circom m4c_oracle_eddsa.circom  --wasm -l node_modules -o build
circom m4_state_commit.circom   --wasm -l node_modules -o build
circom m7_transition.circom     --wasm -l node_modules -o build
node test_tier.mjs
node test_batch.mjs
node test_smt.mjs
node test_lock_insert.mjs
node test_lock_finalize.mjs
node test_oracle_eddsa.mjs
node test_state_commit.mjs
node test_m7_transition.mjs
