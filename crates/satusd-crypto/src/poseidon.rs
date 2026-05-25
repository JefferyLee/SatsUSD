//! Poseidon over the BN254 scalar field, circomlib/iden3-compatible (ADR-0013).
//!
//! Chosen for consistency with the EdDSA-BabyJubjub oracle scheme (PRD §D7,
//! iden3/circom convention) and the future M4 circuit. `light-poseidon`'s
//! `new_circom` matches circomlibjs constants, so the TypeScript reference can
//! byte-match via circomlibjs.

use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use light_poseidon::{Poseidon, PoseidonHasher};

/// Field element as 32-byte big-endian (BN254 Fr is < 2^254, so it fits in 32B).
pub fn fr_to_be_bytes(x: &Fr) -> [u8; 32] {
    let v = x.into_bigint().to_bytes_be();
    let mut out = [0u8; 32];
    // to_bytes_be yields 32 bytes for BN254 Fr; copy right-aligned defensively.
    out[32 - v.len()..].copy_from_slice(&v);
    out
}

/// Interpret big-endian bytes as a field element, reduced modulo the BN254 order.
/// Callers pass <= 31-byte chunks so no reduction actually occurs.
pub fn fr_from_be_bytes(b: &[u8]) -> Fr {
    Fr::from_be_bytes_mod_order(b)
}

/// circomlib-compatible Poseidon over `n` field-element inputs (1..=16).
pub fn poseidon(inputs: &[Fr]) -> Fr {
    let mut h = Poseidon::<Fr>::new_circom(inputs.len()).expect("circom poseidon params");
    h.hash(inputs).expect("poseidon hash")
}

/// Two-input Poseidon — the compression function for the fold and the SMT.
pub fn poseidon2(a: Fr, b: Fr) -> Fr {
    poseidon(&[a, b])
}

/// Byte-oriented `poseidon2`: inputs/output as big-endian bytes (each input < Fr).
pub fn poseidon2_be(a: &[u8], b: &[u8]) -> [u8; 32] {
    fr_to_be_bytes(&poseidon2(fr_from_be_bytes(a), fr_from_be_bytes(b)))
}

/// Hash an arbitrary byte string into one field element (ADR-0013).
///
/// Pack big-endian into 31-byte limbs (each < Fr, lossless) and fold with the
/// 2-input compression: `acc_0 = 0`, `acc_i = poseidon2(acc_{i-1}, limb_i)`.
/// Empty input hashes to the field zero.
pub fn hash_bytes(input: &[u8]) -> Fr {
    let mut acc = Fr::from(0u64);
    for chunk in input.chunks(31) {
        acc = poseidon2(acc, fr_from_be_bytes(chunk));
    }
    acc
}

/// `hash_bytes` returned as 32-byte big-endian.
pub fn hash_bytes_be(input: &[u8]) -> [u8; 32] {
    fr_to_be_bytes(&hash_bytes(input))
}

/// Binary Poseidon merkle root over field-element leaves (poseidon2 compression),
/// padded to the next power of two with the field zero. This is the batch-root
/// convention for §6.8 ReserveClaim batches (redemption/confirmation/burn/lineage
/// roots), defined here so the M4a circuit's `batch_root` gadget can match it.
/// Empty input → field zero.
pub fn batch_root(leaves: &[Fr]) -> Fr {
    if leaves.is_empty() {
        return Fr::from(0u64);
    }
    let mut n = 1usize;
    while n < leaves.len() {
        n <<= 1;
    }
    let mut level = leaves.to_vec();
    level.resize(n, Fr::from(0u64));
    while level.len() > 1 {
        level = level.chunks(2).map(|p| poseidon2(p[0], p[1])).collect();
    }
    level[0]
}

/// `batch_root` over big-endian leaf bytes, returned big-endian.
pub fn batch_root_be(leaves_be: &[[u8; 32]]) -> [u8; 32] {
    let frs: Vec<Fr> = leaves_be.iter().map(|b| fr_from_be_bytes(b)).collect();
    fr_to_be_bytes(&batch_root(&frs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poseidon_is_deterministic() {
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        assert_eq!(poseidon2(a, b), poseidon2(a, b));
    }

    #[test]
    fn fr_roundtrip_be_bytes() {
        let x = poseidon2(Fr::from(7u64), Fr::from(9u64));
        let bytes = fr_to_be_bytes(&x);
        assert_eq!(fr_from_be_bytes(&bytes), x);
    }

    #[test]
    fn batch_root_4_is_binary_merkle() {
        let l = [[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]]; // each < Fr
        let a = poseidon2_be(&l[0], &l[1]);
        let b = poseidon2_be(&l[2], &l[3]);
        assert_eq!(batch_root_be(&l), poseidon2_be(&a, &b));
        // 1-leaf root is the leaf itself (next pow2 of 1 is 1); empty → zero.
        assert_eq!(batch_root(&[Fr::from(5u64)]), Fr::from(5u64));
        assert_eq!(batch_root(&[]), Fr::from(0u64));
        // 3 leaves pad to 4 with a zero leaf.
        let three = [Fr::from(7u64), Fr::from(8u64), Fr::from(9u64)];
        let exp3 = poseidon2(
            poseidon2(Fr::from(7u64), Fr::from(8u64)),
            poseidon2(Fr::from(9u64), Fr::from(0u64)),
        );
        assert_eq!(batch_root(&three), exp3);
    }

    // Printed for cross-checking against circomlibjs poseidon([1,2]).
    #[test]
    fn print_poseidon_1_2() {
        let h = poseidon2(Fr::from(1u64), Fr::from(2u64));
        println!("poseidon([1,2]) = 0x{}", hex::encode(fr_to_be_bytes(&h)));
    }
}
