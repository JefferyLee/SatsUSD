//! FR-6 (spec 07 §4.4, spec 01 §6): client-side verification that an
//! option position is fully backed by its **own locked `Q`** — pure
//! functions over the public position descriptor plus a Bitcoin view the
//! caller supplies. No server of ours (PRD NFR-2): the buyer recomputes
//! `Q` from the LP's disclosed key and their own, compares it to the
//! on-chain output, and checks over-collateralisation, the no-liquidation
//! payout split, and that the pre-signed maturity CET is armed for the
//! announced oracle event — all before paying.

use bitcoin::hashes::Hash;
use bitcoin::taproot::{LeafVersion, TapLeafHash};
use bitcoin::ScriptBuf;
use satusd_oracle::oracle::Announcement;
use satusd_rail1::cet::bucket_adaptor_point;
use satusd_rail1::funding::refund_leaf_script;
use satusd_vault::contract::{cr_bps, VaultTerms};
use satusd_vault::musig::{aggregate_internal_x_pub, verify_keyspend_adaptor};

const SATS_PER_BTC: u128 = 100_000_000;
const MICRO_PER_USD: u128 = 1_000_000;

/// Everything a buyer is handed about a position — all public. The buyer
/// supplies their own knowledge of `holder_pub` independently.
#[derive(Clone, Debug)]
pub struct OptionPosition {
    pub terms: VaultTerms,
    /// LP and holder full (33-byte) public keys, in the MuSig2 key-agg
    /// order [LP, holder] used at issuance.
    pub lp_pub: [u8; 33],
    pub holder_pub: [u8; 33],
    /// The single refund leaf's CSV delta and its two x-only keys, in the
    /// order `refund_leaf_script(csv, user_x, lp_x)` was built with.
    pub refund_csv: u16,
    pub refund_user_x: [u8; 32],
    pub refund_lp_x: [u8; 32],
}

/// The caller-supplied on-chain view of the collateral output `Q`.
#[derive(Clone, Debug)]
pub struct QView {
    pub value_sats: u64,
    pub script_pubkey: ScriptBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PositionError {
    /// The LP/holder public keys do not parse or aggregate.
    BadKeys,
    /// The recomputed `Q` is not the on-chain output (wrong key / wrong
    /// counterparties / wrong refund leaf).
    QMismatch,
    /// `Q` does not hold exactly `terms.collateral_sats`.
    CollateralMismatch,
    /// The collateral ratio at the reference price is below the opening CR.
    UnderCollateralised,
}

/// A verified backing summary.
#[derive(Debug, PartialEq, Eq)]
pub struct Backing {
    pub collateral_sats: u64,
    pub cr_bps: u32,
}

/// Recompute `Q = taptweak(KeyAgg(LP, holder), refund_root)` and confirm
/// the supplied on-chain output is that 2-of-2 key, holds exactly
/// `terms.collateral_sats`, and is over-collateralised at `ref_price_usd`.
/// This is "the position is fully backed by its own locked `Q`".
pub fn verify_backing(
    pos: &OptionPosition,
    q: &QView,
    ref_price_usd: u32,
) -> Result<Backing, PositionError> {
    let internal_x =
        aggregate_internal_x_pub(&pos.lp_pub, &pos.holder_pub).ok_or(PositionError::BadKeys)?;
    let refund = refund_leaf_script(pos.refund_csv, &pos.refund_user_x, &pos.refund_lp_x);
    let merkle_root = TapLeafHash::from_script(&refund, LeafVersion::TapScript).to_byte_array();
    let q_x = satusd_crypto::nums::tap_tweak(&internal_x, &merkle_root);

    let mut expected = Vec::with_capacity(34);
    expected.push(0x51); // OP_1
    expected.push(0x20); // push 32
    expected.extend_from_slice(&q_x);
    if q.script_pubkey != ScriptBuf::from_bytes(expected) {
        return Err(PositionError::QMismatch);
    }
    if q.value_sats != pos.terms.collateral_sats {
        return Err(PositionError::CollateralMismatch);
    }
    let cr = cr_bps(pos.terms.collateral_sats, pos.terms.mint_micro_usd, ref_price_usd);
    if cr < pos.terms.opening_cr_bps {
        return Err(PositionError::UnderCollateralised);
    }
    Ok(Backing { collateral_sats: pos.terms.collateral_sats, cr_bps: cr })
}

/// Recompute `Q`'s BIP-340 x-only key from the position descriptor (the
/// key the maturity CET is signed under). `None` on malformed keys.
pub fn position_q_x(pos: &OptionPosition) -> Option<[u8; 32]> {
    let internal_x = aggregate_internal_x_pub(&pos.lp_pub, &pos.holder_pub)?;
    let refund = refund_leaf_script(pos.refund_csv, &pos.refund_user_x, &pos.refund_lp_x);
    let merkle_root = TapLeafHash::from_script(&refund, LeafVersion::TapScript).to_byte_array();
    Some(satusd_crypto::nums::tap_tweak(&internal_x, &merkle_root))
}

/// The option payout split at a settlement price: P (the holder's P leg) =
/// `min(collateral, face)`, N (the LP's N leg) = `collateral − P`. P + N ≡
/// `collateral` at every price — the no-liquidation invariant (spec 07 §3):
/// the legs always partition `Q`, so there is nothing to liquidate.
pub fn payout_at(terms: &VaultTerms, price_usd: u32) -> (u64, u64) {
    let face_sats = (u128::from(terms.mint_micro_usd) * SATS_PER_BTC
        / (u128::from(price_usd) * MICRO_PER_USD)) as u64;
    let p = face_sats.min(terms.collateral_sats);
    let n = terms.collateral_sats - p;
    (p, n)
}

/// Verify the pre-signed maturity CET is sound, WITHOUT the oracle secret:
/// the 2-of-2 MuSig2 adaptor `adaptor_sig` verifies under `q_x` for the
/// settlement `keyspend_sighash`, encrypted to the anticipation point of
/// `bucket` in the announced maturity event. This ties the LP's issuance-
/// time commitment to the right oracle event + bucket — so at maturity the
/// public attestation, and nothing else, completes it.
#[allow(clippy::too_many_arguments)]
pub fn verify_cet_adaptor(
    q_x: &[u8; 32],
    keyspend_sighash: &[u8; 32],
    announcement: &Announcement,
    oracle_pubkey: &[u8; 32],
    m: u8,
    bucket: u32,
    adaptor_sig: &[u8; 65],
) -> bool {
    let Ok(point) = bucket_adaptor_point(announcement, oracle_pubkey, m, bucket) else {
        return false;
    };
    verify_keyspend_adaptor(adaptor_sig, q_x, keyspend_sighash, &point.serialize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_oracle::oracle::Oracle;
    use satusd_rail::encode::tagged_hash as th;
    use satusd_rail1::cet::bucket_of;
    use satusd_vault::musig::cosign_keyspend_adaptor;
    use secp256k1::{Secp256k1, SecretKey};

    const M: u8 = 4;

    fn terms() -> VaultTerms {
        VaultTerms {
            collateral_sats: 2_000_000, // ~$2000 of BTC backing a $1000 face
            mint_micro_usd: 1_000_000_000,
            opening_cr_bps: 15_000,
            liq_cr_bps: 11_000,
            checkpoint_interval: 144,
            maturity_height: 1_000_000,
            m: M,
            penalty_bps: 500,
            oracle_event_series: [7u8; 32],
        }
    }

    /// Build a position + its true on-chain Q from known keys.
    fn position() -> (OptionPosition, QView, [u8; 32], [u8; 32]) {
        let secp = Secp256k1::new();
        let lp_sk = th("verify/opt-lp", b"v1");
        let holder_sk = th("verify/opt-holder", b"v1");
        let lp_pub = SecretKey::from_byte_array(lp_sk).unwrap().public_key(&secp).serialize();
        let holder_pub = SecretKey::from_byte_array(holder_sk).unwrap().public_key(&secp).serialize();
        let lp_x = SecretKey::from_byte_array(lp_sk).unwrap().x_only_public_key(&secp).0.serialize();
        let holder_x = SecretKey::from_byte_array(holder_sk).unwrap().x_only_public_key(&secp).0.serialize();

        let pos = OptionPosition {
            terms: terms(),
            lp_pub,
            holder_pub,
            refund_csv: 144,
            refund_user_x: holder_x,
            refund_lp_x: lp_x,
        };
        let q_x = position_q_x(&pos).unwrap();
        let mut spk = vec![0x51, 0x20];
        spk.extend_from_slice(&q_x);
        let q = QView { value_sats: terms().collateral_sats, script_pubkey: ScriptBuf::from_bytes(spk) };
        (pos, q, lp_sk, holder_sk)
    }

    #[test]
    fn backing_verifies_against_its_own_q() {
        let (pos, q, _, _) = position();
        let b = verify_backing(&pos, &q, 100_000).unwrap();
        assert_eq!(b.collateral_sats, 2_000_000);
        assert!(b.cr_bps >= pos.terms.opening_cr_bps);
    }

    #[test]
    fn wrong_q_collateral_and_cr_are_refused() {
        let (pos, q, _, _) = position();
        // A different on-chain key is not this position's Q.
        let mut bad_spk = vec![0x51, 0x20];
        bad_spk.extend_from_slice(&[0xab; 32]);
        let bad_q = QView { value_sats: 2_000_000, script_pubkey: ScriptBuf::from_bytes(bad_spk) };
        assert_eq!(verify_backing(&pos, &bad_q, 100_000), Err(PositionError::QMismatch));

        // Right Q, wrong amount.
        let under = QView { value_sats: 1_000_000, ..q.clone() };
        assert_eq!(verify_backing(&pos, &under, 100_000), Err(PositionError::CollateralMismatch));

        // Right Q + amount, but the reference price puts CR below opening.
        assert_eq!(verify_backing(&pos, &q, 40_000), Err(PositionError::UnderCollateralised));
    }

    #[test]
    fn payout_conserves_collateral_in_both_regimes() {
        let t = terms();
        // x >= S: P = face (synthetic dollar), N = the cushion.
        let (p_hi, n_hi) = payout_at(&t, 100_000);
        assert_eq!(p_hi, 1_000_000); // $1000 @ $100k
        assert_eq!(p_hi + n_hi, t.collateral_sats, "P + N = Q");
        // x < S: P = all of Q (shortfall), N = 0.
        let (p_lo, n_lo) = payout_at(&t, 40_000);
        assert_eq!(p_lo, t.collateral_sats); // face ($2500k) capped at Q
        assert_eq!(n_lo, 0);
        assert_eq!(p_lo + n_lo, t.collateral_sats, "P + N = Q");
    }

    #[test]
    fn cet_adaptor_is_tied_to_the_announced_bucket() {
        let (pos, _q, lp_sk, holder_sk) = position();
        let q_x = position_q_x(&pos).unwrap();
        let refund = refund_leaf_script(pos.refund_csv, &pos.refund_user_x, &pos.refund_lp_x);
        let merkle_root = TapLeafHash::from_script(&refund, LeafVersion::TapScript).to_byte_array();

        let oracle = Oracle::from_seed(&th("verify/opt-oracle", b"seed")).unwrap();
        let ann = oracle.announce(1_700_000_000).unwrap();
        let bucket = bucket_of(100_000, M);
        let point = bucket_adaptor_point(&ann, &oracle.pubkey, M, bucket).unwrap();

        // Issuance: LP + holder co-sign the CET as a MuSig2 adaptor.
        let sighash = th("verify/cet-sighash", b"settle");
        let adaptor = cosign_keyspend_adaptor(&lp_sk, &holder_sk, &merkle_root, &sighash, &point.serialize());

        // The buyer's check: the adaptor is armed for the announced bucket.
        assert!(verify_cet_adaptor(&q_x, &sighash, &ann, &oracle.pubkey, M, bucket, &adaptor));
        // A different bucket's anticipation point does not match.
        assert!(!verify_cet_adaptor(&q_x, &sighash, &ann, &oracle.pubkey, M, bucket + 1, &adaptor));
    }
}
