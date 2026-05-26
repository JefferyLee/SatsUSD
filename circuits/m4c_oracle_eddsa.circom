pragma circom 2.1.9;

include "circomlib/circuits/poseidon.circom";
include "circomlib/circuits/eddsaposeidon.circom";

// M4c: oracle price-attestation verification (§5.D7, DL-25). The Poseidon-friendly
// part of the oracle quorum that DOES go in the MVP circuit: an oracle signs
// `oracle_message_hash = Poseidon([set_epoch, price_epoch, timestamp_ms,
// price_e8])` with EdDSA over BabyJubJub (circomlib convention, matching
// satusd_crypto::{poseidon::oracle_message_hash, eddsa}). The circuit recomputes
// the message hash from the fields and verifies one signer's signature over it.
//
// (SHA256/secp256k1 verification — SPV, lineage, TapTweak — stays OFF-circuit per
// DL-25, enforced by the software verifier + challenger.)
template OracleEddsa() {
    // The signed oracle message fields.
    signal input oracleSetEpoch;
    signal input priceEpoch;
    signal input timestampMs;
    signal input priceE8;
    // The signer's BabyJubJub pubkey + the signature (unpacked to coords).
    signal input Ax;
    signal input Ay;
    signal input S;
    signal input R8x;
    signal input R8y;

    signal output msg; // oracle_message_hash

    component h = Poseidon(4);
    h.inputs[0] <== oracleSetEpoch;
    h.inputs[1] <== priceEpoch;
    h.inputs[2] <== timestampMs;
    h.inputs[3] <== priceE8;
    msg <== h.out;

    component v = EdDSAPoseidonVerifier();
    v.enabled <== 1;
    v.Ax <== Ax;
    v.Ay <== Ay;
    v.S <== S;
    v.R8x <== R8x;
    v.R8y <== R8y;
    v.M <== msg;
}

component main = OracleEddsa();
