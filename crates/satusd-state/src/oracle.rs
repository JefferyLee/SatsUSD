//! Oracle price aggregation (PRD §5.D7, §5.D6 ClaimClock freshness, §8.2 step 4).
//!
//! Aggregates a quorum of signed `OracleMessage`s into a single `price_e8`:
//! quorum ≥ 3-of-5, distinct in-set signers, correct oracle-set epoch, fresh
//! timestamps (relative to `chain_time = MTP + 3600`), then median + inlier
//! filtering. Passes §13.2 O-01..O-08.
//!
//! `verify_signatures` performs the real per-message **EdDSA-BabyJub** check
//! (O-07, circom-compatible via `satusd_crypto::eddsa`); `aggregate` does the
//! orthogonal quorum/freshness/outlier logic. Callers run both.
//!
//! SPEC NOTE: §8.2 step 5 says reject if global `(max-min)/median > 5%`, but
//! §13.2 O-05 requires *excluding* the outlier and accepting when ≥3 inliers
//! remain. These conflict; we implement the O-05 (acceptance-test) behaviour —
//! inlier filtering within 2% of the median, require ≥3 — and drop the global
//! hard reject. PRD §8.2 step 5 should be reworded to match O-05.

use satusd_crypto::eddsa::verify_eddsa_babyjub;
use satusd_crypto::poseidon::oracle_message_hash;
use satusd_types::derive::oracle_set_hash;
use satusd_types::types::OracleMessage;

/// Minimum valid signatures to aggregate (§18.3, MVP).
pub const ORACLE_THRESHOLD: usize = 3;
/// Maximum oracle set size (§18.3, MVP).
pub const ORACLE_SET_SIZE: usize = 5;
/// Inlier band around the median, in basis points (§18.3, 2%).
pub const ORACLE_OUTLIER_DEV_BPS: u128 = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OracleRejectReason {
    OracleSetHashMismatch,
    QuorumInsufficient,  // O-01
    WrongOracleSetEpoch, // O-02
    StaleOracle,         // O-03
    FutureOracle,        // O-04
    InsufficientInliers, // O-05
    SignatureInvalid,    // O-07
    SignerNotInSet,      // O-08
    DuplicateSigner,
    TooManyMessages,
}

/// Verification context (from `StateRoot` + `ClaimClock`).
pub struct OracleParams<'a> {
    pub expected_oracle_set_epoch: u64,
    pub expected_oracle_set_hash: [u8; 32],
    /// `chain_time = l1_anchor_mtp + MTP_LAG_OFFSET` (§5.D6), in seconds.
    pub chain_time_sec: u64,
    pub max_epoch_lag_sec: u32,
    pub oracle_future_tolerance_sec: u32,
    /// The active oracle signer set (committed by `expected_oracle_set_hash`).
    pub signer_set: &'a [[u8; 32]],
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

/// Verify every message's EdDSA-BabyJub signature over its `oracle_message_hash`
/// (§8.2 step 1, O-07). A real cryptographic check (circom-compatible); orthogonal
/// to price aggregation, so callers run this then [`aggregate`].
pub fn verify_signatures(messages: &[OracleMessage]) -> Result<(), OracleRejectReason> {
    for m in messages {
        let h = oracle_message_hash(
            m.oracle_set_epoch,
            m.price_epoch,
            m.timestamp_ms,
            m.price_e8,
        );
        if !verify_eddsa_babyjub(&m.signer_pubkey, &h, &m.signature) {
            return Err(OracleRejectReason::SignatureInvalid); // O-07
        }
    }
    Ok(())
}

/// Aggregate oracle messages into a single `price_e8` (the median of the quorum):
/// quorum, distinct in-set signers, correct epoch, freshness, outlier filtering.
/// Signatures are verified separately by [`verify_signatures`].
pub fn aggregate(
    messages: &[OracleMessage],
    p: &OracleParams<'_>,
) -> Result<u64, OracleRejectReason> {
    use OracleRejectReason::*;

    ensure!(
        oracle_set_hash(p.expected_oracle_set_epoch, p.signer_set) == p.expected_oracle_set_hash,
        OracleSetHashMismatch
    );
    ensure!(messages.len() >= ORACLE_THRESHOLD, QuorumInsufficient); // O-01
    ensure!(messages.len() <= ORACLE_SET_SIZE, TooManyMessages);

    let mut seen: Vec<[u8; 32]> = Vec::with_capacity(messages.len());
    for m in messages {
        ensure!(p.signer_set.contains(&m.signer_pubkey), SignerNotInSet); // O-08
        ensure!(!seen.contains(&m.signer_pubkey), DuplicateSigner);
        seen.push(m.signer_pubkey);
        ensure!(
            m.oracle_set_epoch == p.expected_oracle_set_epoch,
            WrongOracleSetEpoch // O-02
        );
        let ts = m.timestamp_ms / 1000;
        // O-03: too old (later than max_epoch_lag behind chain_time).
        ensure!(
            !(p.chain_time_sec > ts && p.chain_time_sec - ts > p.max_epoch_lag_sec as u64),
            StaleOracle
        );
        // O-04: too far in the future.
        ensure!(
            !(ts > p.chain_time_sec
                && ts - p.chain_time_sec > p.oracle_future_tolerance_sec as u64),
            FutureOracle
        );
    }

    // Median of the submitted prices (§8.2 step 4).
    let mut prices: Vec<u64> = messages.iter().map(|m| m.price_e8).collect();
    prices.sort_unstable();
    let median = prices[prices.len() / 2];

    // Inliers: within ORACLE_OUTLIER_DEV_BPS of the median; need ≥ threshold.
    let inliers = prices
        .iter()
        .filter(|&&pr| {
            let dev = (pr as i128 - median as i128).unsigned_abs();
            dev * 10_000 <= median as u128 * ORACLE_OUTLIER_DEV_BPS
        })
        .count();
    ensure!(inliers >= ORACLE_THRESHOLD, InsufficientInliers); // O-05

    Ok(median)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPOCH: u64 = 7;
    const CHAIN_TIME: u64 = 1_700_000_000;

    fn signer(i: u8) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[0] = 0xc0 + i;
        k
    }

    fn signer_set() -> Vec<[u8; 32]> {
        (0..ORACLE_SET_SIZE as u8).map(signer).collect()
    }

    fn msg(i: u8, price_e8: u64, epoch: u64, ts_sec: u64) -> OracleMessage {
        OracleMessage {
            domain: [0x01; 32],
            oracle_id: [i; 32],
            oracle_set_epoch: epoch,
            price_epoch: 100,
            timestamp_ms: ts_sec * 1000,
            pair: *b"BTC/USD\0",
            price_e8,
            source_commitment: [0x02; 32],
            signer_pubkey: signer(i),
            signature: [0x03; 64],
        }
    }

    fn params(set: &[[u8; 32]]) -> OracleParams<'_> {
        OracleParams {
            expected_oracle_set_epoch: EPOCH,
            expected_oracle_set_hash: oracle_set_hash(EPOCH, set),
            chain_time_sec: CHAIN_TIME,
            max_epoch_lag_sec: 600,
            oracle_future_tolerance_sec: 600,
            signer_set: set,
        }
    }

    /// 5 tight prices around $50,000.
    fn five_tight() -> Vec<OracleMessage> {
        [49_998, 49_999, 50_000, 50_001, 50_002]
            .iter()
            .enumerate()
            .map(|(i, &p)| msg(i as u8, p * 100_000_000, EPOCH, CHAIN_TIME))
            .collect()
    }

    #[test]
    fn happy_path_returns_median() {
        let set = signer_set();
        let msgs = five_tight();
        let price = aggregate(&msgs, &params(&set)).unwrap();
        assert_eq!(price, 50_000 * 100_000_000); // median
    }

    #[test]
    fn o01_quorum_insufficient() {
        let set = signer_set();
        let msgs = &five_tight()[..2];
        assert_eq!(
            aggregate(msgs, &params(&set)),
            Err(OracleRejectReason::QuorumInsufficient)
        );
    }

    #[test]
    fn o02_wrong_set_epoch() {
        let set = signer_set();
        let mut msgs = five_tight();
        msgs[1].oracle_set_epoch = EPOCH + 1;
        assert_eq!(
            aggregate(&msgs, &params(&set)),
            Err(OracleRejectReason::WrongOracleSetEpoch)
        );
    }

    #[test]
    fn o03_stale_timestamp() {
        let set = signer_set();
        let mut msgs = five_tight();
        msgs[2].timestamp_ms = (CHAIN_TIME - 601) * 1000; // > 600s old
        assert_eq!(
            aggregate(&msgs, &params(&set)),
            Err(OracleRejectReason::StaleOracle)
        );
    }

    #[test]
    fn o04_future_timestamp() {
        let set = signer_set();
        let mut msgs = five_tight();
        msgs[3].timestamp_ms = (CHAIN_TIME + 601) * 1000; // > 600s ahead
        assert_eq!(
            aggregate(&msgs, &params(&set)),
            Err(OracleRejectReason::FutureOracle)
        );
    }

    #[test]
    fn o05_outlier_excluded_but_quorum_holds() {
        // One price 10% high; 4 inliers remain ⇒ accept, median unaffected.
        let set = signer_set();
        let mut msgs = five_tight();
        msgs[4].price_e8 = 55_000 * 100_000_000;
        let price = aggregate(&msgs, &params(&set)).unwrap();
        assert_eq!(price, 50_000 * 100_000_000);
    }

    #[test]
    fn o05_too_few_inliers_rejected() {
        // 3 wildly different prices ⇒ < 3 within 2% of median.
        let set = signer_set();
        let msgs = vec![
            msg(0, 40_000 * 100_000_000, EPOCH, CHAIN_TIME),
            msg(1, 50_000 * 100_000_000, EPOCH, CHAIN_TIME),
            msg(2, 60_000 * 100_000_000, EPOCH, CHAIN_TIME),
        ];
        assert_eq!(
            aggregate(&msgs, &params(&set)),
            Err(OracleRejectReason::InsufficientInliers)
        );
    }

    // O-07: real EdDSA-BabyJub verification. The valid message + signature come
    // from circomlibjs (privkey 0x11×32, fields below hash to the signed msg);
    // tampering the signature or any signed field is rejected.
    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().unwrap()
    }
    fn h64(s: &str) -> [u8; 64] {
        hex::decode(s).unwrap().try_into().unwrap()
    }
    fn signed_reference() -> OracleMessage {
        OracleMessage {
            domain: [0; 32],
            oracle_id: [0; 32],
            oracle_set_epoch: 7,
            price_epoch: 100,
            timestamp_ms: 1_700_000_000_000,
            pair: *b"BTC/USD\0",
            price_e8: 5_000_000_000_000,
            source_commitment: [0; 32],
            signer_pubkey: h32("323a1772ccd2bf78ca0f82e4de1d4d48ded87f6f26d92d6a99e5998ac88901a6"),
            signature: h64("e1c966e0d52d5f5b20161c5b653101c10c7935521980770d838d826fbc93c42e35351a47e0b0d02009c15179e144ba6780244d10d391fbda0d5411f7b8562a02"),
        }
    }

    #[test]
    fn o07_signatures_verified_and_forgery_rejected() {
        let m = signed_reference();
        verify_signatures(std::slice::from_ref(&m)).expect("valid circomlibjs sig");

        let mut bad_sig = m.clone();
        bad_sig.signature[0] ^= 1;
        assert_eq!(
            verify_signatures(&[bad_sig]),
            Err(OracleRejectReason::SignatureInvalid)
        );
        // Tampering a signed field also breaks the signature.
        let mut bad_field = m.clone();
        bad_field.price_e8 += 1;
        assert_eq!(
            verify_signatures(&[bad_field]),
            Err(OracleRejectReason::SignatureInvalid)
        );
    }

    #[test]
    fn o08_signer_not_in_set() {
        let set = signer_set();
        let mut msgs = five_tight();
        msgs[2].signer_pubkey = [0xff; 32]; // not in set
        assert_eq!(
            aggregate(&msgs, &params(&set)),
            Err(OracleRejectReason::SignerNotInSet)
        );
    }

    #[test]
    fn oracle_set_hash_mismatch_rejected() {
        let set = signer_set();
        let mut p = params(&set);
        p.expected_oracle_set_hash = [0x00; 32];
        assert_eq!(
            aggregate(&five_tight(), &p),
            Err(OracleRejectReason::OracleSetHashMismatch)
        );
    }
}
