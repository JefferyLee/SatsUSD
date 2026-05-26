pragma circom 2.1.9;

include "circomlib/circuits/poseidon.circom";

// M4: the StateRoot commitment in-circuit. state_root_hash =
// hash_bytes(canonical_encode(StateRoot)) (§6.1, ADR-0013) folds the canonical
// encoding as 31-byte big-endian limbs via poseidon2 (acc starts at 0). This
// gadget reproduces that fold; `satusd_crypto::state::state_root_hash` and the TS
// reference agree. The StateRoot encoding is 536 bytes ⇒ 18 limbs (last is 9B).
//
// NB this commits the *bytes* of the encoding (each limb is a 31-byte chunk), not
// the structured fields — see ADR-006 for why a full in-circuit transition that
// binds individual roots/fields is M7-deferred (and would use a Poseidon-of-fields
// commit instead).
template StateCommit(N) {
    signal input limbs[N];
    signal output commit;

    signal acc[N + 1];
    acc[0] <== 0;
    component h[N];
    for (var i = 0; i < N; i++) {
        h[i] = Poseidon(2);
        h[i].inputs[0] <== acc[i];
        h[i].inputs[1] <== limbs[i];
        acc[i + 1] <== h[i].out;
    }
    commit <== acc[N];
}

component main = StateCommit(18);
