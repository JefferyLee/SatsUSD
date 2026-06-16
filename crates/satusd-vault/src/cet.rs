//! The crash-bucket checkpoint CET schedule (spec 06 §4).
//!
//! A vault pre-signs CETs for **crash buckets only** — the rail-1
//! bucket ranges (spec 02 §7) restricted to prices at or below the
//! vault's crash ceiling (own CR ≤ liq_CR). A healthy price falls in
//! no pre-signed bucket: nothing is broadcastable and the vault glides.
//! A crash price makes exactly its bucket's CET decryptable from the
//! oracle attestation, and anyone may broadcast it.
//!
//! Bucket ranges + adaptor points + decryption secrets are the rail-1
//! primitives, reused verbatim (`satusd_rail1::cet`); this module adds
//! the crash-region filter and binds each bucket to a vault payout.

use crate::contract::{crash_price_ceiling, VaultTerms};
use crate::settle::{crash_payout, CrashPayout, PayoutParams};
use satusd_oracle::oracle::Announcement;
use satusd_rail1::cet::{bucket_schedule, CetBucket};
use secp256k1::PublicKey;

pub use satusd_rail1::cet::{bucket_of, bucket_secret, secret_matches_point};

/// One pre-signed crash checkpoint CET: the bucket it covers, the
/// price it settles at (the bucket midpoint), and the resulting payout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashCet {
    pub bucket_index: u32,
    pub price_lo: u32,
    pub price_hi: u32,
    /// Settlement price = bucket midpoint (spec 04 §4 convention).
    pub settle_price: u32,
    pub payout: CrashPayout,
}

/// The crash-bucket schedule for one checkpoint: every rail-1 bucket
/// whose range reaches into the crash region (`price_lo < ceiling`),
/// with its midpoint payout. These — and only these — are pre-signed.
pub fn crash_schedule(terms: &VaultTerms, params: PayoutParams) -> Vec<CrashCet> {
    let ceiling = crash_price_ceiling(terms);
    bucket_schedule(terms.m, |_| 0)
        .into_iter()
        .filter(|b: &CetBucket| b.price_lo < ceiling)
        .map(|b| {
            let settle_price = b.price_lo + (b.price_hi - b.price_lo) / 2;
            CrashCet {
                bucket_index: b.index,
                price_lo: b.price_lo,
                price_hi: b.price_hi,
                settle_price,
                payout: crash_payout(terms.collateral_sats, terms.mint_micro_usd, settle_price, params),
            }
        })
        .collect()
}

/// The bucket's adaptor point (oracle anticipation) — rail-1 verbatim
/// at the vault's `m`. Its discrete log is the decryption secret the
/// attestation reveals.
pub fn crash_adaptor_point(
    announcement: &Announcement,
    oracle_pubkey: &[u8; 32],
    terms: &VaultTerms,
    bucket_index: u32,
) -> Result<PublicKey, secp256k1::Error> {
    satusd_rail1::cet::bucket_adaptor_point(announcement, oracle_pubkey, terms.m, bucket_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::funding::{keyspend_secret, refund_leaf_script, vault_funding_output};
    use satusd_oracle::oracle::Oracle;
    use satusd_rail::encode::tagged_hash;
    use satusd_rail1::adaptor::{decrypt, presign, verify_presig};
    use secp256k1::schnorr::Signature;
    use secp256k1::{Secp256k1, SecretKey, XOnlyPublicKey};

    fn terms() -> VaultTerms {
        VaultTerms {
            collateral_sats: 2_343_750,
            mint_micro_usd: 1_000_000_000, // $1000
            opening_cr_bps: 15_000,
            liq_cr_bps: 11_000,
            checkpoint_interval: 144,
            maturity_height: 900_000,
            m: 6,
            penalty_bps: 500,
            oracle_event_series: [7u8; 32],
        }
    }
    fn params() -> PayoutParams {
        PayoutParams { penalty_bps: 500, bounty_bps_of_penalty: 1_000, bounty_cap_sats: 5_000, fee_budget_sats: 300 }
    }

    #[test]
    fn schedule_is_crash_region_only() {
        let t = terms();
        let ceiling = crash_price_ceiling(&t);
        let sched = crash_schedule(&t, params());
        assert!(!sched.is_empty());
        // Every pre-signed bucket reaches into the crash region; none is
        // a wholly-healthy bucket.
        for c in &sched {
            assert!(c.price_lo < ceiling, "bucket {} starts above the crash ceiling", c.bucket_index);
            assert_eq!(c.payout.total(), t.collateral_sats - 300);
        }
        // Buckets are the low ones, contiguous from 0.
        assert_eq!(sched[0].bucket_index, 0);
        for w in sched.windows(2) {
            assert_eq!(w[0].bucket_index + 1, w[1].bucket_index);
        }
    }

    #[test]
    fn crash_cet_decrypts_and_verifies_under_vault_key() {
        // Full chain: a vault funding output; pre-sign each crash
        // bucket's CET keyspend; the oracle attests a crash price; only
        // the matching bucket decrypts; the signature verifies under Q.
        let secp = Secp256k1::new();
        let t = terms();

        // Vault funding output + its keyspend secret.
        let internal_sk = tagged_hash("test/vault-internal", b"sched");
        let (px, _) = SecretKey::from_byte_array(internal_sk).unwrap().x_only_public_key(&secp);
        let refund = refund_leaf_script(
            4032,
            &tagged_hash("test/minter", b"m"),
            &tagged_hash("test/reserve", b"r"),
        );
        let f = vault_funding_output(&px.serialize(), &refund);
        let tweaked = keyspend_secret(&internal_sk, &f.merkle_root).unwrap();

        // Oracle announces the checkpoint event.
        let o = Oracle::from_seed(&[13u8; 32]).unwrap();
        let ts = 1_700_000_300u64;
        let ann = o.announce(ts).unwrap();

        // Pre-sign every crash bucket's CET keyspend before the outcome.
        let sched = crash_schedule(&t, params());
        let mut presigs = Vec::new();
        for c in &sched {
            let point = crash_adaptor_point(&ann, &o.pubkey, &t, c.bucket_index).unwrap();
            let msg = tagged_hash("test/vault-cet-sighash", &c.bucket_index.to_be_bytes());
            let base = tagged_hash("test/vault-cet-nonce", &c.bucket_index.to_be_bytes());
            let presig = presign(&tweaked, &base, &msg, &point).unwrap();
            assert!(verify_presig(&presig, &f.output_x, &msg, &point).unwrap());
            presigs.push((c.bucket_index, msg, presig));
        }

        // A crash price arrives (bucket 2 region for m=6: [32768,49152)).
        let crash_price = 40_960u32;
        let win = bucket_of(crash_price, t.m);
        assert!(sched.iter().any(|c| c.bucket_index == win), "crash price is in a pre-signed bucket");
        let att = o.attest(ts, crash_price).unwrap();
        let secret = bucket_secret(&att, t.m, win).unwrap();

        let (_, msg, presig) = presigs.iter().find(|(b, _, _)| *b == win).unwrap();
        let sig = decrypt(presig, &secret).unwrap();
        let q = XOnlyPublicKey::from_byte_array(f.output_x).unwrap();
        secp.verify_schnorr(&Signature::from_byte_array(sig), msg, &q)
            .expect("crash CET keyspend verifies under the vault output key");

        // A HEALTHY price decrypts nothing pre-signed.
        let healthy = 64_000u32;
        let hb = bucket_of(healthy, t.m);
        assert!(!sched.iter().any(|c| c.bucket_index == hb), "healthy price is in no pre-signed bucket — the vault glides");
    }
}
