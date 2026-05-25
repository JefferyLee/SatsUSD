//! EdDSA over BabyJubJub with a Poseidon challenge (circomlib/iden3 convention),
//! the oracle signature scheme (PRD §5.D7, DL-5). circom-compatible via
//! `babyjubjub-rs`, so the same signatures verify in the M4c circuit.

use num_bigint::{BigInt, Sign};

/// Verify a circomlib EdDSA-Poseidon signature. `pubkey` = compressed BabyJubJub
/// point (32B), `msg_be` = the signed field element (32B big-endian, e.g.
/// `oracle_message_hash`), `sig` = compressed R8‖S (64B). Malformed → false.
pub fn verify_eddsa_babyjub(pubkey: &[u8; 32], msg_be: &[u8; 32], sig: &[u8; 64]) -> bool {
    let Ok(point) = babyjubjub_rs::decompress_point(*pubkey) else {
        return false;
    };
    let Ok(signature) = babyjubjub_rs::decompress_signature(sig) else {
        return false;
    };
    let msg = BigInt::from_bytes_be(Sign::Plus, msg_be);
    babyjubjub_rs::verify(point, signature, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon::oracle_message_hash;

    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().unwrap()
    }
    fn h64(s: &str) -> [u8; 64] {
        hex::decode(s).unwrap().try_into().unwrap()
    }

    /// Reference vector from circomlibjs `eddsa.signPoseidon` (privkey = 0x11×32,
    /// msg = poseidon([7,100,1.7e12,5e12])). Proves babyjubjub-rs verify is
    /// circom-compatible AND our `oracle_message_hash` == circomlib Poseidon.
    #[test]
    fn verifies_circomlibjs_reference() {
        let msg_be = oracle_message_hash(7, 100, 1_700_000_000_000, 5_000_000_000_000);
        assert_eq!(
            hex::encode(msg_be),
            "289b339e55e4a8e64369b9ec988f7534e2a2478236dab9f5f99e12d4fe2d0687",
            "oracle_message_hash must match circomlib poseidon"
        );
        let pubkey = h32("323a1772ccd2bf78ca0f82e4de1d4d48ded87f6f26d92d6a99e5998ac88901a6");
        let sig = h64("e1c966e0d52d5f5b20161c5b653101c10c7935521980770d838d826fbc93c42e35351a47e0b0d02009c15179e144ba6780244d10d391fbda0d5411f7b8562a02");

        assert!(verify_eddsa_babyjub(&pubkey, &msg_be, &sig));

        // Tampered message and tampered signature both fail.
        let mut bad_msg = msg_be;
        bad_msg[0] ^= 1;
        assert!(!verify_eddsa_babyjub(&pubkey, &bad_msg, &sig));
        let mut bad_sig = sig;
        bad_sig[0] ^= 1;
        assert!(!verify_eddsa_babyjub(&pubkey, &msg_be, &bad_sig));
    }
}
