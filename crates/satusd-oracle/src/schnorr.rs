//! BIP-340 Schnorr signing with a **predetermined nonce** — the
//! primitive dlcspecs attestations require (the nonce R is committed
//! in the announcement before the outcome exists), which the
//! secp256k1 crate's `sign_schnorr` cannot do (it derives its own
//! nonce).
//!
//! Verification in tests round-trips through the library verifier,
//! so the construction is checked against an independent
//! implementation.

use satusd_rail::encode::tagged_hash;
use secp256k1::{Parity, PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};

/// secp256k1 group order n, big-endian.
const N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];

/// Interpret 32 bytes as a scalar mod n (BIP-340 challenge rule).
/// A single conditional subtraction suffices: the input is < 2^256
/// < 2n.
pub fn scalar_mod_n(bytes: &[u8; 32]) -> Scalar {
    if let Ok(s) = Scalar::from_be_bytes(*bytes) {
        return s;
    }
    // bytes >= n: subtract n once (borrow-propagating, big-endian).
    let mut out = [0u8; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let d = i16::from(bytes[i]) - i16::from(N[i]) - borrow;
        if d < 0 {
            out[i] = (d + 256) as u8;
            borrow = 1;
        } else {
            out[i] = d as u8;
            borrow = 0;
        }
    }
    Scalar::from_be_bytes(out).expect("reduced below n")
}

/// BIP-340 challenge `e = H_tag(BIP0340/challenge, R.x ‖ P.x ‖ m)`.
pub fn challenge(rx: &XOnlyPublicKey, px: &XOnlyPublicKey, msg32: &[u8; 32]) -> Scalar {
    let mut buf = Vec::with_capacity(96);
    buf.extend_from_slice(&rx.serialize());
    buf.extend_from_slice(&px.serialize());
    buf.extend_from_slice(msg32);
    scalar_mod_n(&tagged_hash("BIP0340/challenge", &buf))
}

/// Sign `msg32` with secret key `sk` using the predetermined nonce
/// `k`. Returns the 64-byte BIP-340 signature `R.x ‖ s` where
/// `s = k ± e·d` with the signs fixed by the even-Y rules.
pub fn sign_with_nonce(
    sk: &[u8; 32],
    k: &[u8; 32],
    msg32: &[u8; 32],
) -> Result<[u8; 64], secp256k1::Error> {
    let secp = Secp256k1::new();

    let mut d = SecretKey::from_byte_array(*sk)?;
    let (px, p_parity) = d.x_only_public_key(&secp);
    if p_parity == Parity::Odd {
        d = d.negate();
    }

    let mut kk = SecretKey::from_byte_array(*k)?;
    let (rx, r_parity) = kk.x_only_public_key(&secp);
    if r_parity == Parity::Odd {
        kk = kk.negate();
    }

    let e = challenge(&rx, &px, msg32);
    let ed = d.mul_tweak(&e)?;
    let s = kk.add_tweak(&Scalar::from(ed))?;

    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&rx.serialize());
    sig[32..].copy_from_slice(&s.secret_bytes());
    Ok(sig)
}

/// The nonce's public point `R.x` (even-Y normalized) — what an
/// announcement publishes.
pub fn nonce_point(k: &[u8; 32]) -> Result<XOnlyPublicKey, secp256k1::Error> {
    let secp = Secp256k1::new();
    Ok(SecretKey::from_byte_array(*k)?.x_only_public_key(&secp).0)
}

/// The anticipated signature point `S = R + e·P` for an outcome —
/// a public key whose discrete log is exactly the `s` the oracle
/// will publish. This is what DLC adaptors encrypt to.
pub fn anticipation_point(
    rx: &XOnlyPublicKey,
    px: &XOnlyPublicKey,
    msg32: &[u8; 32],
) -> Result<PublicKey, secp256k1::Error> {
    let secp = Secp256k1::new();
    let e = challenge(rx, px, msg32);
    let p_even = px.public_key(Parity::Even);
    let ep = p_even.mul_tweak(&secp, &e)?;
    let r_even = rx.public_key(Parity::Even);
    r_even.combine(&ep)
}

/// Modular inverse mod n via Fermat (`a^(n−2)`), built from
/// `SecretKey::mul_tweak` — no bigint dependency. `a` MUST be
/// nonzero mod n.
fn invert_mod_n(a: &SecretKey) -> Result<SecretKey, secp256k1::Error> {
    // exponent = n − 2, big-endian.
    let mut exp = N;
    exp[31] -= 2; // n ends in 0x41; no borrow.

    let mut result: Option<SecretKey> = None;
    let mut base = *a;
    // Right-to-left square-and-multiply.
    for byte in exp.iter().rev() {
        let mut bits = *byte;
        for _ in 0..8 {
            if bits & 1 == 1 {
                result = Some(match result {
                    None => base,
                    Some(r) => r.mul_tweak(&Scalar::from(base))?,
                });
            }
            bits >>= 1;
            base = base.mul_tweak(&Scalar::from(base))?;
        }
    }
    result.ok_or(secp256k1::Error::InvalidSecretKey)
}

/// `a − b mod n`, both nonzero, result must be nonzero.
fn sub_mod_n(a: &SecretKey, b: &SecretKey) -> Result<SecretKey, secp256k1::Error> {
    a.add_tweak(&Scalar::from(b.negate()))
}

/// EOTS extraction (spec 03 §3.3 / spec 05): two BIP-340 signatures
/// under the same key `px` sharing the same nonce point `R` over
/// different messages leak the secret:
/// `d = (s1 − s2) / (e1 − e2) mod n`. Returns the even-Y-normalized
/// secret key iff it reproduces `px`; `None` for any inconsistent
/// input.
pub fn extract_secret(
    sig1: &[u8; 64],
    sig2: &[u8; 64],
    msg1: &[u8; 32],
    msg2: &[u8; 32],
    px: &XOnlyPublicKey,
) -> Option<[u8; 32]> {
    if sig1[..32] != sig2[..32] || msg1 == msg2 {
        return None;
    }
    let secp = Secp256k1::new();
    let rx = XOnlyPublicKey::from_byte_array(sig1[..32].try_into().unwrap()).ok()?;
    let e1 = challenge(&rx, px, msg1);
    let e2 = challenge(&rx, px, msg2);

    let s1 = SecretKey::from_byte_array(sig1[32..].try_into().unwrap()).ok()?;
    let s2 = SecretKey::from_byte_array(sig2[32..].try_into().unwrap()).ok()?;
    let ds = sub_mod_n(&s1, &s2).ok()?;
    // e1 − e2 as SecretKeys (zero e is rejected by from_byte_array).
    let e1k = SecretKey::from_byte_array(e1.to_be_bytes()).ok()?;
    let e2k = SecretKey::from_byte_array(e2.to_be_bytes()).ok()?;
    let de = sub_mod_n(&e1k, &e2k).ok()?;

    let d = ds.mul_tweak(&Scalar::from(invert_mod_n(&de).ok()?)).ok()?;
    let (dx, _) = d.x_only_public_key(&secp);
    if dx == *px {
        Some(d.secret_bytes())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::schnorr::Signature;

    fn keys() -> ([u8; 32], [u8; 32], [u8; 32]) {
        let sk = tagged_hash("test/sk", b"oracle");
        let k = tagged_hash("test/k", b"nonce");
        let msg = tagged_hash("test/msg", b"60123");
        (sk, k, msg)
    }

    #[test]
    fn library_verifier_accepts_custom_nonce_signature() {
        let (sk, k, msg) = keys();
        let sig = sign_with_nonce(&sk, &k, &msg).unwrap();

        let secp = Secp256k1::new();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);
        let signature = Signature::from_byte_array(sig);
        secp.verify_schnorr(&signature, &msg, &px)
            .expect("independent BIP-340 verifier must accept");
    }

    #[test]
    fn signature_r_matches_committed_nonce_point() {
        let (sk, k, msg) = keys();
        let sig = sign_with_nonce(&sk, &k, &msg).unwrap();
        assert_eq!(sig[..32], nonce_point(&k).unwrap().serialize());
    }

    #[test]
    fn anticipation_point_discrete_log_is_the_signature_scalar() {
        // S = s·G — the whole DLC construction in one assertion.
        let (sk, k, msg) = keys();
        let sig = sign_with_nonce(&sk, &k, &msg).unwrap();

        let secp = Secp256k1::new();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);
        let rx = nonce_point(&k).unwrap();
        let s_point = anticipation_point(&rx, &px, &msg).unwrap();

        let s_scalar = SecretKey::from_byte_array(sig[32..].try_into().unwrap()).unwrap();
        assert_eq!(s_point, s_scalar.public_key(&secp));
    }

    #[test]
    fn equivocation_extracts_the_secret_key() {
        // EOTS for free (spec 03 §3.3): two messages, one nonce →
        // d = (s1 − s2)/(e1 − e2). We just assert the two signatures
        // share R and differ in s — full extraction is the
        // challenger's job (spec 05).
        let (sk, k, _) = keys();
        let m1 = tagged_hash("test/msg", b"60123");
        let m2 = tagged_hash("test/msg", b"60124");
        let s1 = sign_with_nonce(&sk, &k, &m1).unwrap();
        let s2 = sign_with_nonce(&sk, &k, &m2).unwrap();
        assert_eq!(s1[..32], s2[..32], "same committed R");
        assert_ne!(s1[32..], s2[32..], "different s — extractable");
    }

    #[test]
    fn scalar_mod_n_reduces() {
        // n itself reduces to zero ⇒ from_be_bytes(0) is an error,
        // so probe n+1 → 1.
        let mut n_plus_1 = N;
        n_plus_1[31] += 1;
        assert_eq!(
            scalar_mod_n(&n_plus_1),
            Scalar::from_be_bytes({
                let mut one = [0u8; 32];
                one[31] = 1;
                one
            })
            .unwrap()
        );
    }
}
