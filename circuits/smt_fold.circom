pragma circom 2.1.9;

include "circomlib/circuits/poseidon.circom";

// Sparse-Merkle-tree membership fold (ADR-0015; matches
// satusd_crypto::smt::fold_to_root). Height-parameterised, Poseidon(2) nodes.
// Folds `leaf` up through `siblings` (indexed by depth, 0 = top) applying the
// key's path bits (MSB-first):
//   bit == 0 → node = poseidon2(cur, sib)   (current is the left child)
//   bit == 1 → node = poseidon2(sib, cur)   (current is the right child)
template SmtFold(HEIGHT) {
    signal input leaf;
    signal input pathBits[HEIGHT];
    signal input siblings[HEIGHT];
    signal output root;

    signal cur[HEIGHT + 1];
    cur[0] <== leaf;

    component h[HEIGHT];
    signal left[HEIGHT];
    signal right[HEIGHT];

    for (var i = 0; i < HEIGHT; i++) {
        var d = HEIGHT - 1 - i; // absorb from the leaf level up to the root
        pathBits[d] * (pathBits[d] - 1) === 0; // boolean

        left[i]  <== cur[i]      + pathBits[d] * (siblings[d] - cur[i]);
        right[i] <== siblings[d] + pathBits[d] * (cur[i] - siblings[d]);

        h[i] = Poseidon(2);
        h[i].inputs[0] <== left[i];
        h[i].inputs[1] <== right[i];
        cur[i + 1] <== h[i].out;
    }

    root <== cur[HEIGHT];
}
