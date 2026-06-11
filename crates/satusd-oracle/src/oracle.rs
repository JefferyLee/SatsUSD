//! The oracle itself: seed → keys; announce ahead; attest at
//! maturity. Pure (no clocks, no I/O) — the daemon binary supplies
//! time and price.

use satusd_rail::encode::tagged_hash;
use secp256k1::{Secp256k1, SecretKey};

use crate::event::{attestation_msg, digits, event_id, nonce, BASE, NB_DIGITS};
use crate::schnorr::{nonce_point, sign_with_nonce};
use crate::tlv;

pub const ANNOUNCEMENT_TAG: &str = "DLC/oracle/announcement/v0";
pub const SEED_TO_KEY_TAG: &str = "SatUSD/oracle/key/v1";

pub struct Oracle {
    sk: [u8; 32],
    seed: [u8; 32],
    pub pubkey: [u8; 32],
}

#[derive(Debug)]
pub struct Announcement {
    pub event_id: String,
    pub nonce_points: Vec<[u8; 32]>,
    pub tlv_bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct Attestation {
    pub event_id: String,
    pub price_usd: u32,
    /// One 64-byte signature per digit, MSB-first.
    pub signatures: Vec<[u8; 64]>,
    pub tlv_bytes: Vec<u8>,
}

impl Oracle {
    /// Derive signing key and nonce seed from one master seed.
    pub fn from_seed(master_seed: &[u8; 32]) -> Result<Self, secp256k1::Error> {
        let sk = tagged_hash(SEED_TO_KEY_TAG, master_seed);
        let secp = Secp256k1::new();
        let (px, _) = SecretKey::from_byte_array(sk)?.x_only_public_key(&secp);
        Ok(Self {
            sk,
            seed: *master_seed,
            pubkey: px.serialize(),
        })
    }

    /// Build the announcement for the tick at `unix_ts`.
    pub fn announce(&self, unix_ts: u64) -> Result<Announcement, secp256k1::Error> {
        let id = event_id(unix_ts);
        let mut points = Vec::with_capacity(NB_DIGITS as usize);
        for i in 0..NB_DIGITS {
            let k = nonce(&self.seed, &id, i);
            points.push(nonce_point(&k)?.serialize());
        }
        let descriptor = tlv::digit_descriptor(BASE, false, "USD/BTC", 0, NB_DIGITS);
        let event = tlv::oracle_event(&points, unix_ts as u32, &descriptor, &id);

        // Announcement signature: a regular BIP-340 signature (no
        // predetermined nonce needed) over the dlcspecs message.
        let msg = tagged_hash(ANNOUNCEMENT_TAG, &event);
        let ann_nonce = tagged_hash("SatUSD/oracle/announcement-nonce/v1", &{
            let mut b = self.seed.to_vec();
            b.extend_from_slice(id.as_bytes());
            b
        });
        let sig = sign_with_nonce(&self.sk, &ann_nonce, &msg)?;

        Ok(Announcement {
            event_id: id,
            nonce_points: points,
            tlv_bytes: tlv::oracle_announcement(&sig, &self.pubkey, &event),
        })
    }

    /// Attest the outcome for the tick at `unix_ts`.
    pub fn attest(&self, unix_ts: u64, price_usd: u32) -> Result<Attestation, secp256k1::Error> {
        let id = event_id(unix_ts);
        let ds = digits(price_usd);
        let mut sigs = Vec::with_capacity(ds.len());
        for (i, d) in ds.iter().enumerate() {
            let k = nonce(&self.seed, &id, i as u16);
            sigs.push(sign_with_nonce(&self.sk, &k, &attestation_msg(*d))?);
        }
        let outcomes: Vec<&str> = ds.iter().map(|d| if *d == 0 { "0" } else { "1" }).collect();
        let tlv_bytes = tlv::oracle_attestation(&id, &self.pubkey, &sigs, &outcomes);
        Ok(Attestation {
            event_id: id,
            price_usd,
            signatures: sigs,
            tlv_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schnorr::anticipation_point;
    use secp256k1::schnorr::Signature;
    use secp256k1::XOnlyPublicKey;

    fn oracle() -> Oracle {
        Oracle::from_seed(&[42u8; 32]).unwrap()
    }

    #[test]
    fn attestation_signatures_verify_and_match_announced_nonces() {
        let o = oracle();
        let ts = 1_700_000_000u64;
        let ann = o.announce(ts).unwrap();
        let att = o.attest(ts, 60_123).unwrap();

        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(o.pubkey).unwrap();
        let ds = digits(60_123);
        for (i, sig) in att.signatures.iter().enumerate() {
            // R matches the announcement's committed nonce point.
            assert_eq!(sig[..32], ann.nonce_points[i], "digit {i} nonce");
            // The signature verifies under an independent verifier.
            secp.verify_schnorr(
                &Signature::from_byte_array(*sig),
                &attestation_msg(ds[i]),
                &px,
            )
            .unwrap();
        }
    }

    #[test]
    fn anticipation_points_decrypt_with_published_scalars() {
        // The consumer-side contract: for each digit, the point
        // computed from the ANNOUNCEMENT alone equals s·G of the
        // ATTESTATION's scalar.
        let o = oracle();
        let ts = 1_700_000_001u64;
        let ann = o.announce(ts).unwrap();
        let att = o.attest(ts, 99_999).unwrap();

        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(o.pubkey).unwrap();
        let ds = digits(99_999);
        for (i, d) in ds.iter().enumerate() {
            let rx = XOnlyPublicKey::from_byte_array(ann.nonce_points[i]).unwrap();
            let point = anticipation_point(&rx, &px, &attestation_msg(*d)).unwrap();
            let s =
                SecretKey::from_byte_array(att.signatures[i][32..].try_into().unwrap()).unwrap();
            assert_eq!(point, s.public_key(&secp), "digit {i}");
        }
    }

    /// Pinned wire vectors: cross-language ground truth + regression
    /// guard on the dlcspecs TLV layout.
    #[test]
    fn pinned_tlv_vectors() {
        let o = oracle();
        let ann = o.announce(1_700_000_000).unwrap();
        let att = o.attest(1_700_000_000, 60_123).unwrap();
        assert_eq!(
            hex::encode(satusd_rail::encode::tagged_hash(
                "SatUSD/vector",
                &ann.tlv_bytes
            )),
            "97a579e4ff897b59f033891bd49ffec5abe42c23302067551450b80ffe64094d"
        );
        assert_eq!(
            hex::encode(satusd_rail::encode::tagged_hash(
                "SatUSD/vector",
                &att.tlv_bytes
            )),
            "a7a0ea3f74a21eb13187c40a098a83b3aec6a31336956bcc57b9bf159f01655b"
        );
    }
}
