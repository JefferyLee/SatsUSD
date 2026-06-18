//! The CET bucket schedule: aligned binary digit-prefix wildcards —
//! the power-of-two special case of dlcspecs
//! NumericOutcomeCompression.
//!
//! With `m` prefix digits the outcome space [0, 2^20) splits into
//! 2^m buckets; a CET for bucket `b` aggregates only the first `m`
//! per-digit anticipation points, leaving the remaining 20−m digits
//! as wildcards. One adaptor signature per bucket — 16 buckets need
//! 16 presigns, not a million.

use satusd_oracle::event::{attestation_msg, digits, NB_DIGITS};
use satusd_oracle::oracle::{Announcement, Attestation};
use satusd_oracle::schnorr::anticipation_point;
use secp256k1::{PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CetBucket {
    /// Bucket index = the value of the first `m` digits.
    pub index: u32,
    /// Price range [lo, hi) covered by this bucket, whole USD.
    pub price_lo: u32,
    pub price_hi: u32,
    /// What the user's BTC output carries if this bucket settles —
    /// computed by the rail's plan layer at the bucket's midpoint.
    pub user_sats: u64,
}

/// Build the 2^m bucket ranges. Payouts are the caller's plan-layer
/// concern; this fills the ranges and leaves `user_sats` to the
/// supplied pricing function.
pub fn bucket_schedule(m: u8, user_sats_at: impl Fn(u32) -> u64) -> Vec<CetBucket> {
    assert!(m >= 1 && u16::from(m) <= NB_DIGITS);
    let width = 1u32 << (NB_DIGITS - u16::from(m));
    (0..(1u32 << m))
        .map(|index| {
            let lo = index * width;
            let hi = lo + width;
            CetBucket {
                index,
                price_lo: lo,
                price_hi: hi,
                user_sats: user_sats_at(lo + width / 2),
            }
        })
        .collect()
}

/// The bucket's adaptor point: the sum of the first `m` per-digit
/// anticipation points for the bucket's prefix digits. Its discrete
/// log is the sum of the oracle's first `m` signature scalars.
pub fn bucket_adaptor_point(
    announcement: &Announcement,
    oracle_pubkey: &[u8; 32],
    m: u8,
    bucket_index: u32,
) -> Result<PublicKey, secp256k1::Error> {
    let px = XOnlyPublicKey::from_byte_array(*oracle_pubkey)?;
    // A buyer runs this on an LP-supplied announcement (satusd-verify); a
    // truncated nonce-point vector must error, not panic.
    if announcement.nonce_points.len() < m as usize {
        return Err(secp256k1::Error::InvalidPublicKey);
    }
    let mut acc: Option<PublicKey> = None;
    for i in 0..m {
        // Digit i of the prefix, MSB first.
        let digit = ((bucket_index >> (m - 1 - i)) & 1) as u8;
        let rx = XOnlyPublicKey::from_byte_array(announcement.nonce_points[i as usize])?;
        let point = anticipation_point(&rx, &px, &attestation_msg(digit))?;
        acc = Some(match acc {
            None => point,
            Some(a) => a.combine(&point)?,
        });
    }
    Ok(acc.expect("m >= 1"))
}

/// Which bucket an attested price lands in.
pub fn bucket_of(price_usd: u32, m: u8) -> u32 {
    let clamped = price_usd.min((1 << NB_DIGITS) - 1);
    clamped >> (NB_DIGITS - u16::from(m))
}

/// The bucket's decryption scalar from an attestation: the sum of
/// the first `m` per-digit signature scalars. Errors if the
/// attested price is not in the bucket (the scalars wouldn't match
/// the adaptor point).
pub fn bucket_secret(
    attestation: &Attestation,
    m: u8,
    bucket_index: u32,
) -> Result<[u8; 32], secp256k1::Error> {
    if bucket_of(attestation.price_usd, m) != bucket_index {
        return Err(secp256k1::Error::InvalidTweak);
    }
    if attestation.signatures.len() < m as usize {
        return Err(secp256k1::Error::InvalidTweak);
    }
    let ds = digits(attestation.price_usd);
    let mut acc: Option<SecretKey> = None;
    for (i, d) in ds.iter().enumerate().take(m as usize) {
        // Defense in depth: the digit signed must match the prefix.
        debug_assert_eq!(u32::from(*d), (bucket_index >> (m as usize - 1 - i)) & 1);
        let s = SecretKey::from_byte_array(attestation.signatures[i][32..].try_into().unwrap())?;
        acc = Some(match acc {
            None => s,
            Some(a) => a.add_tweak(&Scalar::from(s))?,
        });
    }
    Ok(acc.expect("m >= 1").secret_bytes())
}

/// Cross-check used in tests and by the user before locking: the
/// bucket secret's public point equals the bucket adaptor point.
pub fn secret_matches_point(t: &[u8; 32], point: &PublicKey) -> bool {
    let secp = Secp256k1::new();
    SecretKey::from_byte_array(*t)
        .map(|s| s.public_key(&secp) == *point)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_oracle::oracle::Oracle;

    const M: u8 = 4;

    #[test]
    fn schedule_shape() {
        let buckets = bucket_schedule(M, |mid| u64::from(mid) * 10);
        assert_eq!(buckets.len(), 16);
        assert_eq!(buckets[0].price_lo, 0);
        assert_eq!(buckets[15].price_hi, 1 << 20);
        // Contiguous, non-overlapping.
        for w in buckets.windows(2) {
            assert_eq!(w[0].price_hi, w[1].price_lo);
        }
    }

    #[test]
    fn bucket_of_boundaries() {
        let width = 1u32 << (20 - M as u16);
        assert_eq!(bucket_of(0, M), 0);
        assert_eq!(bucket_of(width - 1, M), 0);
        assert_eq!(bucket_of(width, M), 1);
        assert_eq!(bucket_of(u32::MAX, M), 15, "overflow clamps to top bucket");
    }

    #[test]
    fn adaptor_point_equals_secret_point_for_attested_bucket() {
        // The heart of Rail-1: the point computable from the
        // ANNOUNCEMENT equals s·G of the secret computable from the
        // ATTESTATION — for the bucket the price actually fell in.
        let o = Oracle::from_seed(&[9u8; 32]).unwrap();
        let ts = 1_700_000_100u64;
        let ann = o.announce(ts).unwrap();

        let price = 60_123u32;
        let att = o.attest(ts, price).unwrap();
        let b = bucket_of(price, M);

        let point = bucket_adaptor_point(&ann, &o.pubkey, M, b).unwrap();
        let t = bucket_secret(&att, M, b).unwrap();
        assert!(secret_matches_point(&t, &point));

        // A different bucket's secret is refused.
        assert!(bucket_secret(&att, M, b + 1).is_err());

        // And a different bucket's point does NOT match this secret.
        let other = bucket_adaptor_point(&ann, &o.pubkey, M, b + 1).unwrap();
        assert!(!secret_matches_point(&t, &other));
    }

    #[test]
    fn truncated_announcement_errors_not_panics() {
        // The buyer-side path (satusd-verify) runs this on LP-supplied data;
        // a short nonce-point vector must Err, never panic.
        use satusd_oracle::oracle::Announcement;
        let ann = Announcement {
            event_id: "x".into(),
            nonce_points: vec![[1u8; 32]], // only 1 point, but m = 4
            tlv_bytes: vec![],
        };
        let o = Oracle::from_seed(&[3u8; 32]).unwrap();
        assert!(bucket_adaptor_point(&ann, &o.pubkey, M, 0).is_err());
    }

    #[test]
    fn frost_median_drives_the_bucket_as_one_key() {
        // FR-5 end-to-end: the decentralised stake-weighted median, attested
        // by a t-of-n FROST cohort, is consumed by THIS bucket code exactly
        // like a single oracle — the cohort presents as one key.
        use satusd_oracle::frost::Cohort;
        use satusd_oracle::median::{stake_weighted_median, Report};

        let reports = [
            Report { price_usd: 60_000, stake: 20 },
            Report { price_usd: 64_000, stake: 60 }, // > 50% stake → the median
            Report { price_usd: 70_000, stake: 20 },
        ];
        let price = stake_weighted_median(&reports).unwrap();
        assert_eq!(price, 64_000);

        let cohort = Cohort::keygen(5, 3, &[3u8; 32]).unwrap();
        let ts = 1_700_000_300u64;
        let quorum = [1u16, 2, 4];
        let ann = cohort.announce(ts, &quorum).unwrap();
        let att = cohort.attest(ts, price, &quorum).unwrap();

        // The bucket point from the ANNOUNCEMENT equals s·G of the secret
        // from the ATTESTATION — under the cohort's group key.
        let win = bucket_of(price, M);
        let point = bucket_adaptor_point(&ann, &cohort.group_pubkey, M, win).unwrap();
        let t = bucket_secret(&att, M, win).unwrap();
        assert!(secret_matches_point(&t, &point));
        // A non-attested bucket has no secret — same selective decryption.
        assert!(bucket_secret(&att, M, win ^ 1).is_err());
    }

    #[test]
    fn end_to_end_with_adaptor() {
        // Oracle → bucket point → presign → attest → bucket secret →
        // decrypt → BIP-340 verify. The full J4 crypto chain, pure.
        use crate::adaptor::{decrypt, presign, verify_presig};
        use satusd_rail::encode::tagged_hash;
        use secp256k1::schnorr::Signature;

        let secp = Secp256k1::new();
        let o = Oracle::from_seed(&[11u8; 32]).unwrap();
        let ts = 1_700_000_200u64;
        let ann = o.announce(ts).unwrap();

        let funding_sk = tagged_hash("test/funding", b"j4");
        let (px, _) = SecretKey::from_byte_array(funding_sk)
            .unwrap()
            .x_only_public_key(&secp);

        // Pre-sign every bucket's CET before the outcome exists.
        let mut presigs = Vec::new();
        for b in 0..(1u32 << M) {
            let point = bucket_adaptor_point(&ann, &o.pubkey, M, b).unwrap();
            let msg = tagged_hash("test/cet-sighash", &b.to_be_bytes());
            let nonce_base = tagged_hash("test/cet-nonce", &b.to_be_bytes());
            let presig = presign(&funding_sk, &nonce_base, &msg, &point).unwrap();
            assert!(verify_presig(&presig, &px.serialize(), &msg, &point).unwrap());
            presigs.push((b, msg, presig));
        }

        // The outcome arrives; exactly the matching bucket decrypts.
        let price = 98_765u32;
        let att = o.attest(ts, price).unwrap();
        let win = bucket_of(price, M);
        let t = bucket_secret(&att, M, win).unwrap();

        let (_, msg, presig) = &presigs[win as usize];
        let sig = decrypt(presig, &t).unwrap();
        secp.verify_schnorr(&Signature::from_byte_array(sig), msg, &px)
            .expect("winning CET broadcasts");
    }
}
