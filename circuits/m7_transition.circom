pragma circom 2.1.9;

include "circomlib/circuits/poseidon.circom";
include "circomlib/circuits/bitify.circom";
include "smt_fold.circom";

// M7 (ADR-006): the monolithic REDEEM_FAST_FINALIZE transition (single
// redemption). It composes the §5.D17 lock state machine (3 SMT folds) with the
// Poseidon-over-fields state commitment, and binds the lock roots embedded in the
// committed prev/new state — the field-level binding that the byte-oriented
// `state_root_hash` made "heavy byte plumbing", which is why this was deferred.
//
// State is committed as NF=42 field elements (`satusd_crypto::state::
// state_field_elements`: StateRoot fields in declaration order, each 32-byte value
// as hi||lo 128-bit limbs). The lock roots live at fixed indices; the circuit
// reconstructs root = hi·2^128 + lo (with hi,lo range-checked < 2^128 so the
// reconstruction is unique) and asserts it equals the matching SMT fold output.

// Poseidon-over-fields fold: acc_0 = 0, acc_i = Poseidon2(acc_{i-1}, fields[i]).
template Fold(N) {
    signal input fields[N];
    signal output out;
    signal acc[N + 1];
    acc[0] <== 0;
    component h[N];
    for (var i = 0; i < N; i++) {
        h[i] = Poseidon(2);
        h[i].inputs[0] <== acc[i];
        h[i].inputs[1] <== fields[i];
        acc[i + 1] <== h[i].out;
    }
    out <== acc[N];
}

// Reconstruct a 32-byte root committed as (hi, lo) 128-bit limbs. Range-checking
// each limb to 128 bits makes `hi·2^128 + lo` the unique integer < 2^256, so a
// Poseidon root (< Fr) binds to exactly one (hi, lo) pair.
template Recon() {
    signal input hi;
    signal input lo;
    signal output root;
    component rh = Num2Bits(128);
    rh.in <== hi;
    component rl = Num2Bits(128);
    rl.in <== lo;
    root <== hi * (2 ** 128) + lo;
}

template Transition(HEIGHT, NF) {
    signal input prevFields[NF];
    signal input newFields[NF];
    // lock state-machine witness (single redemption).
    signal input lockLeaf;                       // leaf_hash(lock_record_hash, SET_MEMBER)
    signal input lockPathBits[HEIGHT];
    signal input lockMembershipSiblings[HEIGHT]; // path in lock_record_root
    signal input consumedSiblings[HEIGHT];       // path in lock_consumed_root
    signal input nfLeaf;                         // leaf_hash(nullifier, SET_MEMBER)
    signal input nfPathBits[HEIGHT];
    signal input nfSiblings[HEIGHT];
    signal input amount;                         // SatUSD atoms burned this transition

    signal output prevCommit;
    signal output newCommit;

    // field-element indices (declaration order; 32-byte field = hi, lo).
    // ADR-010: `state_root_hash` IS this Poseidon-over-fields fold. If the StateRoot
    // struct changes, bump NF and update these indices (+ Rust state_field_elements,
    // TS stateCommitFieldsHex) and re-freeze the vectors — they must stay in sync.
    var EPOCH = 1;
    var SUPPLY = 7;
    var LR_HI = 23; var LR_LO = 24; // lock_record_root
    var LC_HI = 25; var LC_LO = 26; // lock_consumed_root
    var NF_HI = 29; var NF_LO = 30; // redemption_nullifier_root

    // --- state commitments ---
    component pc = Fold(NF);
    pc.fields <== prevFields;
    prevCommit <== pc.out;
    component nc = Fold(NF);
    nc.fields <== newFields;
    newCommit <== nc.out;

    // --- 1. lock_record membership (unchanged across the transition) ---
    component mem = SmtFold(HEIGHT);
    mem.leaf <== lockLeaf;
    mem.pathBits <== lockPathBits;
    mem.siblings <== lockMembershipSiblings;
    component lrP = Recon();
    lrP.hi <== prevFields[LR_HI]; lrP.lo <== prevFields[LR_LO];
    lrP.root === mem.root;
    component lrN = Recon();
    lrN.hi <== newFields[LR_HI]; lrN.lo <== newFields[LR_LO];
    lrN.root === mem.root;

    // --- 2. lock_consumed: absent in prev → inserted in new (R-09) ---
    component cOld = SmtFold(HEIGHT);
    cOld.leaf <== 0;
    cOld.pathBits <== lockPathBits;
    cOld.siblings <== consumedSiblings;
    component lcP = Recon();
    lcP.hi <== prevFields[LC_HI]; lcP.lo <== prevFields[LC_LO];
    lcP.root === cOld.root;
    component cNew = SmtFold(HEIGHT);
    cNew.leaf <== lockLeaf;
    cNew.pathBits <== lockPathBits;
    cNew.siblings <== consumedSiblings;
    component lcN = Recon();
    lcN.hi <== newFields[LC_HI]; lcN.lo <== newFields[LC_LO];
    lcN.root === cNew.root;

    // --- 3. nullifier: unused in prev → inserted in new (R-10) ---
    component nOld = SmtFold(HEIGHT);
    nOld.leaf <== 0;
    nOld.pathBits <== nfPathBits;
    nOld.siblings <== nfSiblings;
    component nfP = Recon();
    nfP.hi <== prevFields[NF_HI]; nfP.lo <== prevFields[NF_LO];
    nfP.root === nOld.root;
    component nNew = SmtFold(HEIGHT);
    nNew.leaf <== nfLeaf;
    nNew.pathBits <== nfPathBits;
    nNew.siblings <== nfSiblings;
    component nfN = Recon();
    nfN.hi <== newFields[NF_HI]; nfN.lo <== newFields[NF_LO];
    nfN.root === nNew.root;

    // --- 4. accounting linkage: supply burns by `amount`, epoch advances by 1 ---
    prevFields[SUPPLY] === newFields[SUPPLY] + amount;
    newFields[EPOCH] === prevFields[EPOCH] + 1;
}

component main = Transition(256, 42);
