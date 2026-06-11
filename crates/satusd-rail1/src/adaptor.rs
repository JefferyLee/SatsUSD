//! Schnorr adaptor signatures encrypted to a point `T` whose
//! discrete log `t` the oracle will publish (the anticipation
//! point, or a sum of per-digit anticipation points).
//!
//! Scheme (single signer; the BIP-340 x-only convention forces the
//! even-Y dance):
//!
//! ```text
//! presign:  choose k_i = H(base ‖ i) until R_i + T has even Y
//!           e   = H_BIP340(x(R+T) ‖ P.x ‖ m)
//!           s_a = k + e·d                    (pre-signature)
//! verify:   s_a·G == R + e·P
//! decrypt:  s = s_a + t  →  (x(R+T), s) is a valid BIP-340 sig
//! recover:  t = s − s_a   (from a broadcast signature)
//! ```
//!
//! `recover` is what lets an LP learn the oracle scalar from a
//! settlement someone else broadcast — and what makes adaptor reuse
//! across messages unsafe (one decryption reveals `t` to everyone).

use satusd_oracle::schnorr::{challenge, scalar_mod_n};
use satusd_rail::encode::tagged_hash;
use secp256k1::{Parity, PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptorSig {
    /// x-only of the *combined* nonce point `R + T` — the R the
    /// final BIP-340 signature will carry.
    pub combined_rx: [u8; 32],
    /// The signer's own nonce point, full compressed encoding —
    /// parity matters: R itself may be odd-Y even when R + T is
    /// even-Y, and the verifier must reconstruct the exact point.
    pub r_point: [u8; 33],
    /// Pre-signature scalar `k + e·d`.
    pub s_a: [u8; 32],
    /// How many nonce derivation attempts were skipped to land on an
    /// even-Y `R + T` (the verifier re-derives deterministically).
    pub nonce_counter: u32,
}

/// Derive the i-th candidate nonce for this presign context.
fn nonce_candidate(base: &[u8; 32], i: u32) -> [u8; 32] {
    let mut buf = Vec::with_capacity(36);
    buf.extend_from_slice(base);
    buf.extend_from_slice(&i.to_be_bytes());
    tagged_hash("SatUSD/rail1/presign-nonce/v1", &buf)
}

/// Pre-sign `msg32` under `sk`, encrypted to adaptor point `t_point`.
/// `nonce_base` deterministically seeds the nonce search (unique per
/// CET — never reuse across messages).
pub fn presign(
    sk: &[u8; 32],
    nonce_base: &[u8; 32],
    msg32: &[u8; 32],
    t_point: &PublicKey,
) -> Result<AdaptorSig, secp256k1::Error> {
    let secp = Secp256k1::new();

    let mut d = SecretKey::from_byte_array(*sk)?;
    let (px, p_parity) = d.x_only_public_key(&secp);
    if p_parity == Parity::Odd {
        d = d.negate();
    }

    for i in 0u32..256 {
        let k_bytes = nonce_candidate(nonce_base, i);
        let Ok(k) = SecretKey::from_byte_array(k_bytes) else {
            continue;
        };
        let r = k.public_key(&secp);
        let combined = r.combine(t_point)?;
        let (combined_rx, parity) = combined.x_only_public_key();
        if parity == Parity::Odd {
            continue;
        }

        let e = challenge(&combined_rx, &px, msg32);
        let ed = d.mul_tweak(&e)?;
        let s_a = k.add_tweak(&Scalar::from(ed))?;

        return Ok(AdaptorSig {
            combined_rx: combined_rx.serialize(),
            r_point: r.serialize(),
            s_a: s_a.secret_bytes(),
            nonce_counter: i,
        });
    }
    Err(secp256k1::Error::InvalidTweak)
}

/// Verify a pre-signature: `s_a·G == R + e·P`, with `e` computed
/// over the combined nonce. The user runs this on every CET before
/// locking anything (S1 depends on it).
pub fn verify_presig(
    presig: &AdaptorSig,
    pubkey_x: &[u8; 32],
    msg32: &[u8; 32],
    t_point: &PublicKey,
) -> Result<bool, secp256k1::Error> {
    let secp = Secp256k1::new();
    let px = XOnlyPublicKey::from_byte_array(*pubkey_x)?;
    let r = PublicKey::from_byte_array_compressed(presig.r_point)?;

    // Recompute the combined point and check it matches (and is the
    // even-Y one the challenge was computed over).
    let combined = r.combine(t_point)?;
    let (combined_rx, parity) = combined.x_only_public_key();
    if parity == Parity::Odd || combined_rx.serialize() != presig.combined_rx {
        return Ok(false);
    }

    let e = challenge(&combined_rx, &px, msg32);
    let p_even = px.public_key(Parity::Even);
    let ep = p_even.mul_tweak(&secp, &e)?;
    let expected = r.combine(&ep)?;

    let s_a = SecretKey::from_byte_array(presig.s_a)?;
    Ok(s_a.public_key(&secp) == expected)
}

/// Decrypt the pre-signature with the oracle scalar `t` into a full
/// BIP-340 signature. Anyone holding the attestation can do this —
/// L1 bought liveness in one function.
pub fn decrypt(presig: &AdaptorSig, t: &[u8; 32]) -> Result<[u8; 64], secp256k1::Error> {
    let s_a = SecretKey::from_byte_array(presig.s_a)?;
    let t_scalar = scalar_mod_n(t);
    let s = s_a.add_tweak(&t_scalar)?;
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&presig.combined_rx);
    sig[32..].copy_from_slice(&s.secret_bytes());
    Ok(sig)
}

/// Recover the oracle scalar from a broadcast signature:
/// `t = s − s_a`.
pub fn recover_t(presig: &AdaptorSig, sig: &[u8; 64]) -> Result<[u8; 32], secp256k1::Error> {
    let s = SecretKey::from_byte_array(sig[32..].try_into().unwrap())?;
    let s_a = SecretKey::from_byte_array(presig.s_a)?;
    let neg_sa = s_a.negate();
    let t = s.add_tweak(&Scalar::from(neg_sa))?;
    Ok(t.secret_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::schnorr::Signature;

    fn setup() -> ([u8; 32], [u8; 32], [u8; 32], SecretKey, PublicKey) {
        let secp = Secp256k1::new();
        let sk = tagged_hash("test/funding-sk", b"rail1");
        let base = tagged_hash("test/nonce-base", b"cet-7");
        let msg = tagged_hash("test/cet-sighash", b"bucket-7");
        // The "oracle secret" t and its point T.
        let t = SecretKey::from_byte_array(tagged_hash("test/t", b"oracle-s")).unwrap();
        let t_point = t.public_key(&secp);
        (sk, base, msg, t, t_point)
    }

    #[test]
    fn full_adaptor_lifecycle() {
        let secp = Secp256k1::new();
        let (sk, base, msg, t, t_point) = setup();

        let presig = presign(&sk, &base, &msg, &t_point).unwrap();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);

        // User-side check before locking funds.
        assert!(verify_presig(&presig, &px.serialize(), &msg, &t_point).unwrap());

        // Oracle publishes t → anyone decrypts → library verifies.
        let sig = decrypt(&presig, &t.secret_bytes()).unwrap();
        let combined_rx = XOnlyPublicKey::from_byte_array(presig.combined_rx).unwrap();
        assert_eq!(sig[..32], combined_rx.serialize());
        // The full signature must verify under the COMBINED nonce as
        // a standard BIP-340 signature on the funding key.
        secp.verify_schnorr(&Signature::from_byte_array(sig), &msg, &px)
            .expect("decrypted signature is a valid BIP-340 sig");

        // LP recovers t from the broadcast signature.
        assert_eq!(recover_t(&presig, &sig).unwrap(), t.secret_bytes());
    }

    #[test]
    fn presig_rejects_wrong_t_point_and_tampering() {
        let secp = Secp256k1::new();
        let (sk, base, msg, _t, t_point) = setup();
        let presig = presign(&sk, &base, &msg, &t_point).unwrap();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);

        let wrong_t = SecretKey::from_byte_array(tagged_hash("test/t", b"other"))
            .unwrap()
            .public_key(&secp);
        assert!(!verify_presig(&presig, &px.serialize(), &msg, &wrong_t).unwrap());

        let mut tampered = presig.clone();
        tampered.s_a[0] ^= 1;
        assert!(!verify_presig(&tampered, &px.serialize(), &msg, &t_point).unwrap());
    }

    #[test]
    fn decrypt_with_wrong_t_fails_library_verification() {
        let secp = Secp256k1::new();
        let (sk, base, msg, _t, t_point) = setup();
        let presig = presign(&sk, &base, &msg, &t_point).unwrap();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);

        let wrong = tagged_hash("test/t", b"not-the-oracle");
        let sig = decrypt(&presig, &wrong).unwrap();
        assert!(secp
            .verify_schnorr(&Signature::from_byte_array(sig), &msg, &px)
            .is_err());
    }
}
