pragma circom 2.1.9;

include "circomlib/circuits/comparators.circom";
include "circomlib/circuits/bitify.circom";
include "circomlib/circuits/poseidon.circom";

// M4a skeleton (PRD §8.4, DL-24): collateral ratio + emergency tier + a Poseidon
// state commitment. No SMT, no signatures (those are M4b/M4c).
//
// cr_ppm = reserve_sats * price_e8 * 1e6 / (supply_atoms * 1e14), as an
// in-field Euclidean division proven by the (cr_q, cr_r) witness:
//   num = reserve*price*1e6,  den = supply*1e14,  num = den*cr_q + cr_r, 0<=cr_r<den.
// All intermediates stay < 2^148 << the BN254 scalar field, so field equality is
// integer equality. The native `satusd_types::tier` is the spec this derives from.
template CrTier() {
    signal input reserve_sats;   // private, u64
    signal input supply_atoms;   // private, u64, > 0
    signal input price_e8;       // private, u64
    signal input cr_q;           // private: floor(num/den) == cr_ppm
    signal input cr_r;           // private: num mod den

    signal output cr_ppm;
    signal output tier;          // 0=Healthy 1=PauseMint 2=Auction 3=Settlement
    signal output state_commit;  // Poseidon commitment skeleton

    // u64 range checks on the economic inputs.
    component rb = Num2Bits(64); rb.in <== reserve_sats;
    component sb = Num2Bits(64); sb.in <== supply_atoms;
    component pb = Num2Bits(64); pb.in <== price_e8;

    // supply_atoms != 0 (this skeleton handles the CR-defined case).
    signal inv;
    inv <-- 1 / supply_atoms;
    supply_atoms * inv === 1;

    // num = reserve * price * 1e6 ; den = supply * 1e14
    signal rp;
    rp <== reserve_sats * price_e8;            // < 2^128
    signal num;
    num <== rp * 1000000;                      // < 2^148
    signal den;
    den <== supply_atoms * 100000000000000;    // < 2^111

    // Euclidean division: num = den*cr_q + cr_r, 0 <= cr_r < den.
    component qb = Num2Bits(64);  qb.in <== cr_q;    // cr_ppm fits u64
    component rrb = Num2Bits(128); rrb.in <== cr_r;
    signal dq;
    dq <== den * cr_q;
    num === dq + cr_r;
    component rlt = LessThan(128);
    rlt.in[0] <== cr_r;
    rlt.in[1] <== den;
    rlt.out === 1;

    cr_ppm <== cr_q;

    // Tier bucketing: tier = 3 - [cr>=1.5M] - [cr>=1.3M] - [cr>=1.1M].
    component ge15 = GreaterEqThan(64); ge15.in[0] <== cr_ppm; ge15.in[1] <== 1500000;
    component ge13 = GreaterEqThan(64); ge13.in[0] <== cr_ppm; ge13.in[1] <== 1300000;
    component ge11 = GreaterEqThan(64); ge11.in[0] <== cr_ppm; ge11.in[1] <== 1100000;
    tier <== 3 - ge15.out - ge13.out - ge11.out;

    // Poseidon state-commitment skeleton (circomlib BN254 Poseidon, ADR-0013).
    component h = Poseidon(5);
    h.inputs[0] <== reserve_sats;
    h.inputs[1] <== supply_atoms;
    h.inputs[2] <== price_e8;
    h.inputs[3] <== cr_ppm;
    h.inputs[4] <== tier;
    state_commit <== h.out;
}

component main = CrTier();
