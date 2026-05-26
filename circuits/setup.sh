#!/usr/bin/env bash
# Groth16 trusted setup for the SatUSD circuits (BN254). DEFERRED TO SIGNET — the
# MVP cross-checks (check.sh) use witness calculators only and need NO setup. This
# script is the documented recipe for when proving/verifying keys are required
# (M6 signet pilot onward). The phase-2 contribution is per-circuit and MUST be a
# real multi-party ceremony for production; the single-contributor flow below is
# for local proving only.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build keys

PTAU_POWER="${PTAU_POWER:-18}" # 2^18 ≈ 260k constraints; lock_finalize needs ~315k → use 19 for it
PTAU="keys/pot${PTAU_POWER}_final.ptau"

# --- Phase 1: universal powers-of-tau (circuit-independent) ---
if [ ! -f "$PTAU" ]; then
  echo "phase 1: powers of tau (2^${PTAU_POWER})"
  npx snarkjs powersoftau new bn128 "$PTAU_POWER" keys/pot_0000.ptau -v
  npx snarkjs powersoftau contribute keys/pot_0000.ptau keys/pot_0001.ptau \
    --name="local dev contribution" -v -e="$(head -c 64 /dev/urandom | xxd -p -c 64)"
  npx snarkjs powersoftau prepare phase2 keys/pot_0001.ptau "$PTAU" -v
fi

# --- Phase 2: per-circuit (needs the .r1cs from `circom --r1cs`) ---
setup_circuit() {
  local name="$1"
  echo "phase 2: $name"
  circom "${name}.circom" --r1cs -l node_modules -o build
  npx snarkjs groth16 setup "build/${name}.r1cs" "$PTAU" "keys/${name}_0000.zkey"
  npx snarkjs zkey contribute "keys/${name}_0000.zkey" "keys/${name}_final.zkey" \
    --name="local dev" -v -e="$(head -c 64 /dev/urandom | xxd -p -c 64)"
  npx snarkjs zkey export verificationkey "keys/${name}_final.zkey" "keys/${name}_vk.json"
}

# The circuits that would be proven (extend as the monolithic transition lands):
setup_circuit m4a_cr_tier
setup_circuit m4c_oracle_eddsa
# setup_circuit m4b_lock_finalize   # needs PTAU_POWER=19 (≈315k constraints)

echo "setup complete — keys/ has the proving (.zkey) + verifying (vk.json) keys"
