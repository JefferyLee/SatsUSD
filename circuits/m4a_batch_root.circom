pragma circom 2.1.9;

include "circomlib/circuits/poseidon.circom";

// M4a batch-root skeleton (§6.8 ReserveClaim batch roots): a 4-leaf binary
// Poseidon merkle, matching satusd_crypto::poseidon::batch_root for 4 leaves.
// Real batches (≤ MAX_BATCH_SIZE = 64) are the same construction with more levels.
template BatchRoot4() {
    signal input leaves[4];
    signal output root;

    component h01 = Poseidon(2);
    h01.inputs[0] <== leaves[0];
    h01.inputs[1] <== leaves[1];

    component h23 = Poseidon(2);
    h23.inputs[0] <== leaves[2];
    h23.inputs[1] <== leaves[3];

    component hr = Poseidon(2);
    hr.inputs[0] <== h01.out;
    hr.inputs[1] <== h23.out;

    root <== hr.out;
}

component main = BatchRoot4();
