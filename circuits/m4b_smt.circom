pragma circom 2.1.9;

include "circomlib/circuits/poseidon.circom";

// M4b: sparse-Merkle-tree membership fold (ADR-0015; matches
// satusd_crypto::smt::fold_to_root). Height 256, Poseidon(2) internal nodes.
//
// Folds `leaf` up through `siblings` applying the key's path bits (MSB-first,
// pathBits[d] = bit at depth d, 0 = top) to the public `root`:
//   bit == 0 → node = poseidon2(cur, sib)   (current is the left child)
//   bit == 1 → node = poseidon2(sib, cur)   (current is the right child)
//
// Inclusion: leaf = poseidon2(hash_bytes(key), hash_bytes(value)).
// Non-membership: leaf = 0 (the empty leaf). Both are the same fold; only the
// leaf differs — exactly the §5.D17 lock-state-machine checks (lock_record
// membership, lock_consumed / nullifier non-membership).
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
        // Absorb siblings from the bottom (depth HEIGHT-1) up to the root (depth 0).
        var d = HEIGHT - 1 - i;
        pathBits[d] * (pathBits[d] - 1) === 0; // boolean

        // bit==0 ⇒ (left,right)=(cur,sib); bit==1 ⇒ (sib,cur).
        left[i]  <== cur[i]       + pathBits[d] * (siblings[d] - cur[i]);
        right[i] <== siblings[d]  + pathBits[d] * (cur[i] - siblings[d]);

        h[i] = Poseidon(2);
        h[i].inputs[0] <== left[i];
        h[i].inputs[1] <== right[i];
        cur[i + 1] <== h[i].out;
    }

    root <== cur[HEIGHT];
}

component main = SmtFold(256);
