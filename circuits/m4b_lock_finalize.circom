pragma circom 2.1.9;

include "smt_fold.circom";

// M4b: the §5.D17 active→consumed lock transition (REDEEM_FAST_FINALIZE) in
// circuit form, composing the SMT primitives. It proves, with one sibling path
// per tree:
//   1. lock_record membership — the lock exists (key ∈ lock_record_root).
//   2. lock_consumed insert    — the lock was NOT consumed (non-membership) and
//      is now inserted (R-09 double-finalize guard).
//   3. nullifier insert        — the redemption nullifier was unused and is now
//      inserted (R-10 replay guard).
// The lock_record and lock_consumed keys are the same `lock_record_hash` (so they
// share path bits); the nullifier is a distinct key. A verifier binds the five
// output roots to prev/new StateRoot fields. `leaf` for both sets is
// leaf_hash(key, SET_MEMBER); the empty leaf is 0.
template LockFinalize(HEIGHT) {
    // lock_record_hash key (membership + consumed insert share these path bits).
    signal input lockLeaf;                         // leaf_hash(lock_record_hash, SET_MEMBER)
    signal input lockPathBits[HEIGHT];
    signal input lockMembershipSiblings[HEIGHT];   // path in lock_record_root
    signal input consumedSiblings[HEIGHT];         // path in lock_consumed_root
    // nullifier key.
    signal input nfLeaf;                           // leaf_hash(nullifier, SET_MEMBER)
    signal input nfPathBits[HEIGHT];
    signal input nfSiblings[HEIGHT];

    signal output lockRecordRoot; // == prev.lock_record_root (membership)
    signal output consumedOld;    // == prev.lock_consumed_root
    signal output consumedNew;    // == new.lock_consumed_root
    signal output nfOld;          // == prev.redemption_nullifier_root
    signal output nfNew;          // == new.redemption_nullifier_root

    // 1. lock_record membership.
    component mem = SmtFold(HEIGHT);
    mem.leaf <== lockLeaf;
    mem.pathBits <== lockPathBits;
    mem.siblings <== lockMembershipSiblings;
    lockRecordRoot <== mem.root;

    // 2. lock_consumed: absent → inserted (same lock key).
    component cOld = SmtFold(HEIGHT);
    cOld.leaf <== 0;
    cOld.pathBits <== lockPathBits;
    cOld.siblings <== consumedSiblings;
    consumedOld <== cOld.root;
    component cNew = SmtFold(HEIGHT);
    cNew.leaf <== lockLeaf;
    cNew.pathBits <== lockPathBits;
    cNew.siblings <== consumedSiblings;
    consumedNew <== cNew.root;

    // 3. nullifier: unused → inserted.
    component nOld = SmtFold(HEIGHT);
    nOld.leaf <== 0;
    nOld.pathBits <== nfPathBits;
    nOld.siblings <== nfSiblings;
    nfOld <== nOld.root;
    component nNew = SmtFold(HEIGHT);
    nNew.leaf <== nfLeaf;
    nNew.pathBits <== nfPathBits;
    nNew.siblings <== nfSiblings;
    nfNew <== nNew.root;
}

component main = LockFinalize(256);
