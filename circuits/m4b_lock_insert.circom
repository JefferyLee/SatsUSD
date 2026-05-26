pragma circom 2.1.9;

include "smt_fold.circom";

// M4b: the §5.D17 set-insert transition in-circuit (the shape used by
// REDEEM_FAST_LOCK's lock_record insert, and the lock_consumed / nullifier
// inserts at finalize). One sibling path proves BOTH:
//   oldRoot = fold(empty leaf 0)   — the key is absent before (non-membership)
//   newRoot = fold(leaf)           — the key maps to `leaf` after
// i.e. `newRoot == root_after_update(key, value, exclusion_proof)` with the
// non-membership precondition enforced by the same proof. A verifier checks the
// pre-state commits `oldRoot` and the post-state commits `newRoot`.
template LockInsert(HEIGHT) {
    signal input leaf;                 // poseidon2(hash_bytes(key), hash_bytes(value))
    signal input pathBits[HEIGHT];
    signal input siblings[HEIGHT];
    signal output oldRoot;
    signal output newRoot;

    component before = SmtFold(HEIGHT);
    before.leaf <== 0;
    before.pathBits <== pathBits;
    before.siblings <== siblings;
    oldRoot <== before.root;

    component after = SmtFold(HEIGHT);
    after.leaf <== leaf;
    after.pathBits <== pathBits;
    after.siblings <== siblings;
    newRoot <== after.root;
}

component main = LockInsert(256);
