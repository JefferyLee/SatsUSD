//! secp256k1 ECDSA verification for the issuer multisig (PRD §5.D11).
//! Issuer multisig pubkeys are 33-byte compressed; signatures are 64-byte compact.

use secp256k1::{ecdsa::Signature, Message, PublicKey, Secp256k1};

/// Verify a compact ECDSA signature over a 32-byte message digest. Returns false
/// for any malformed key/signature rather than erroring.
pub fn verify_ecdsa(msg32: &[u8; 32], pubkey33: &[u8; 33], sig64: &[u8; 64]) -> bool {
    let secp = Secp256k1::verification_only();
    let (Ok(pk), Ok(sig)) = (
        PublicKey::from_slice(pubkey33),
        Signature::from_compact(sig64),
    ) else {
        return false;
    };
    secp.verify_ecdsa(Message::from_digest(*msg32), &sig, &pk)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Secp256k1, SecretKey};

    #[test]
    fn valid_signature_verifies_wrong_key_fails() {
        let secp = Secp256k1::new();
        let sk = SecretKey::from_byte_array([0x11; 32]).unwrap();
        let pk = sk.public_key(&secp).serialize();
        let msg = [0x42u8; 32];
        let sig = secp
            .sign_ecdsa(Message::from_digest(msg), &sk)
            .serialize_compact();
        assert!(verify_ecdsa(&msg, &pk, &sig));

        // Wrong message and wrong key both fail.
        let mut bad_msg = msg;
        bad_msg[0] ^= 1;
        assert!(!verify_ecdsa(&bad_msg, &pk, &sig));
        let other = SecretKey::from_byte_array([0x22; 32])
            .unwrap()
            .public_key(&secp)
            .serialize();
        assert!(!verify_ecdsa(&msg, &other, &sig));
    }
}
