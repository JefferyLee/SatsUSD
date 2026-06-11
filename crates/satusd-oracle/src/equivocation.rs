//! Equivocation evidence (spec 05 §2): a pair of conflicting
//! attestations for the same event. Per spec 03 §3.3 the pair IS the
//! artifact — validation parses both TLVs, finds a digit signed two
//! ways under one committed nonce, checks both signatures, and
//! extracts the oracle's secret key (EOTS).

use secp256k1::schnorr::Signature;
use secp256k1::{Secp256k1, XOnlyPublicKey};

use crate::event::attestation_msg;
use crate::schnorr::extract_secret;
use crate::tlv::{parse_attestation, ParseError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquivocationProof {
    pub event_id: String,
    pub oracle_pubkey: [u8; 32],
    /// First digit index attested both ways.
    pub digit_index: u16,
    /// The extracted (even-Y-normalized) oracle secret key — the
    /// proof is self-certifying: anyone can check `d·G = pubkey`.
    pub extracted_secret: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquivocationError {
    Parse(ParseError),
    /// Not the same event, oracle, or digit count.
    NotComparable,
    /// Outcomes are identical — no equivocation.
    NoConflict,
    /// An outcome string is not a valid base-2 digit.
    BadOutcome,
    /// A signature fails BIP-340 verification or does not reuse the
    /// committed nonce point.
    BadSignature,
    /// Extraction did not reproduce the oracle pubkey.
    ExtractionFailed,
}

impl From<ParseError> for EquivocationError {
    fn from(e: ParseError) -> Self {
        EquivocationError::Parse(e)
    }
}

/// Validate an equivocation evidence pair. `Ok` means the oracle
/// provably signed two different outcomes for `event_id` — the
/// slash condition of spec 05 §2, with the key compromise as
/// built-in proof.
pub fn verify_equivocation(
    att_a_tlv: &[u8],
    att_b_tlv: &[u8],
) -> Result<EquivocationProof, EquivocationError> {
    let a = parse_attestation(att_a_tlv)?;
    let b = parse_attestation(att_b_tlv)?;
    if a.event_id != b.event_id
        || a.oracle_pubkey != b.oracle_pubkey
        || a.signatures.len() != b.signatures.len()
    {
        return Err(EquivocationError::NotComparable);
    }
    let px = XOnlyPublicKey::from_byte_array(a.oracle_pubkey)
        .map_err(|_| EquivocationError::BadSignature)?;

    let digit = |s: &str| match s {
        "0" => Ok(0u8),
        "1" => Ok(1u8),
        _ => Err(EquivocationError::BadOutcome),
    };

    for (i, (oa, ob)) in a.outcomes.iter().zip(&b.outcomes).enumerate() {
        let (da, db) = (digit(oa)?, digit(ob)?);
        if da == db {
            continue;
        }
        // Both signatures must verify for their claimed outcomes and
        // share the committed nonce point.
        let secp = Secp256k1::new();
        let (ma, mb) = (attestation_msg(da), attestation_msg(db));
        for (sig, msg) in [(&a.signatures[i], &ma), (&b.signatures[i], &mb)] {
            secp.verify_schnorr(&Signature::from_byte_array(*sig), msg, &px)
                .map_err(|_| EquivocationError::BadSignature)?;
        }
        if a.signatures[i][..32] != b.signatures[i][..32] {
            return Err(EquivocationError::BadSignature);
        }
        let extracted = extract_secret(&a.signatures[i], &b.signatures[i], &ma, &mb, &px)
            .ok_or(EquivocationError::ExtractionFailed)?;
        return Ok(EquivocationProof {
            event_id: a.event_id,
            oracle_pubkey: a.oracle_pubkey,
            digit_index: i as u16,
            extracted_secret: extracted,
        });
    }
    Err(EquivocationError::NoConflict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::Oracle;
    use secp256k1::{Secp256k1, SecretKey};

    #[test]
    fn double_attestation_yields_key_extraction() {
        let o = Oracle::from_seed(&[13u8; 32]).unwrap();
        let ts = 1_700_000_300u64;
        // Same event, two prices differing in low digits — the
        // equivocation a price-manipulating oracle would commit.
        let a = o.attest(ts, 60_123).unwrap();
        let b = o.attest(ts, 60_124).unwrap();

        let proof = verify_equivocation(&a.tlv_bytes, &b.tlv_bytes).unwrap();
        assert_eq!(proof.event_id, a.event_id);
        assert_eq!(proof.oracle_pubkey, o.pubkey);

        // The proof is self-certifying: d·G reproduces the pubkey.
        let secp = Secp256k1::new();
        let (px, _) = SecretKey::from_byte_array(proof.extracted_secret)
            .unwrap()
            .x_only_public_key(&secp);
        assert_eq!(px.serialize(), o.pubkey);

        // And the extracted key actually signs: forge an attestation
        // for an arbitrary digit under the oracle's identity.
        let forged = crate::schnorr::sign_with_nonce(
            &proof.extracted_secret,
            &[7u8; 32],
            &attestation_msg(1),
        )
        .unwrap();
        let opx = XOnlyPublicKey::from_byte_array(o.pubkey).unwrap();
        secp.verify_schnorr(
            &Signature::from_byte_array(forged),
            &attestation_msg(1),
            &opx,
        )
        .expect("extracted key signs as the oracle");
    }

    #[test]
    fn honest_pairs_and_tampered_evidence_rejected() {
        let o = Oracle::from_seed(&[13u8; 32]).unwrap();
        let ts = 1_700_000_301u64;
        let a = o.attest(ts, 60_123).unwrap();

        // Identical attestations: no conflict.
        assert_eq!(
            verify_equivocation(&a.tlv_bytes, &a.tlv_bytes).unwrap_err(),
            EquivocationError::NoConflict
        );

        // Different events: not comparable.
        let other = o.attest(ts + 1, 60_124).unwrap();
        assert_eq!(
            verify_equivocation(&a.tlv_bytes, &other.tlv_bytes).unwrap_err(),
            EquivocationError::NotComparable
        );

        // Tampered signature: flip a byte in the second attestation's
        // first differing-digit signature region.
        let b = o.attest(ts, 60_124).unwrap();
        let mut tampered = b.tlv_bytes.clone();
        let len = tampered.len();
        tampered[len - 200] ^= 0x01;
        assert!(verify_equivocation(&a.tlv_bytes, &tampered).is_err());

        // Garbage bytes: parse error.
        assert!(matches!(
            verify_equivocation(&a.tlv_bytes, b"junk").unwrap_err(),
            EquivocationError::Parse(_)
        ));
    }
}
