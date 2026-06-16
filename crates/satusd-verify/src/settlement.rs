//! Rail-0 settlement verification: from (manifest, quote, payout
//! claim, anchor tx) to an S3 settlement point — refusing anything
//! that doesn't bind.
//!
//! What this checks is the **observer's** half of S2/S3 for the BTC
//! leg: the quote belongs to the rail, the quote's lock-template
//! commitment binds the recomputed plan, and the broadcast
//! transaction actually carries the planned payout. The TA leg
//! (lineage of the moved asset) composes with `satusd-ta-proof` in
//! the next increment.

use bitcoin::consensus::deserialize;
use bitcoin::{Amount, ScriptBuf, Transaction};
use satusd_rail::manifest::RailManifest;
use satusd_rail::quote::Quote;
use satusd_rail0::plan::{PlanError, SwapPlan};

use crate::marker::SettlementPoint;

#[derive(Debug, PartialEq, Eq)]
pub enum SettlementError {
    /// The quote does not belong to the claimed rail.
    RailIdMismatch,
    /// Plan recomputation failed (oversize, dust, wrong rail class).
    Plan(PlanError),
    /// The quote's lock_template_commitment does not bind the plan.
    CommitmentMismatch,
    /// The anchor transaction lacks the planned payout output.
    PayoutMissing,
    /// Transaction bytes do not parse.
    BadTransaction,
}

/// Verify one Rail-0 settlement and extract its settlement point.
///
/// `payout_script` is the user's claimed payout script; the tx must
/// contain an output of exactly `plan.user_sats` to it.
pub fn verify_rail0_settlement(
    manifest: &RailManifest,
    quote: &Quote,
    payout_script: &ScriptBuf,
    anchor_tx_bytes: &[u8],
) -> Result<SettlementPoint, SettlementError> {
    if quote.rail_id != manifest.rail_id() {
        return Err(SettlementError::RailIdMismatch);
    }
    let plan = SwapPlan::from_quote(manifest, quote).map_err(SettlementError::Plan)?;
    if quote.lock_template_commitment != plan.lock_template_commitment() {
        return Err(SettlementError::CommitmentMismatch);
    }

    let tx: Transaction =
        deserialize(anchor_tx_bytes).map_err(|_| SettlementError::BadTransaction)?;
    let paid = tx
        .output
        .iter()
        .any(|o| o.value == Amount::from_sat(plan.user_sats) && o.script_pubkey == *payout_script);
    if !paid {
        return Err(SettlementError::PayoutMissing);
    }

    Ok(SettlementPoint {
        price_msat_per_usd: quote.price_msat_per_usd,
        volume_micro_usd: quote.amount_micro_usd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::absolute::LockTime;
    use bitcoin::consensus::serialize;
    use bitcoin::transaction::Version;
    use bitcoin::TxOut;
    use satusd_rail::manifest::{FeeSchedule, LpStake};
    use satusd_rail0::rail::{rail0_manifest, Rail0Params};

    fn setup() -> (RailManifest, Quote, ScriptBuf, Vec<u8>) {
        let manifest = rail0_manifest(Rail0Params {
            asset_id: [0x11; 32],
            max_size_sats: 5_000_000,
            committed_term: 4032,
            fee_schedule: FeeSchedule {
                retain_bps: 10,
                service_bps: 0,
                fixed_sats: 200,
            },
            price_dev_bound_bps: 100,
            quote_validity_s: 30,
            lp_stake: LpStake {
                amount_sats: 10_000_000,
                slash_conditions: vec![],
            },
        })
        .unwrap();

        let mut quote = Quote {
            rail_id: manifest.rail_id(),
            price_msat_per_usd: 1_000_000,
            amount_micro_usd: 1_000_000_000,
            expiry_unix_s: u64::MAX,
            lock_template_commitment: [0; 32],
        };
        let plan = SwapPlan::from_quote(&manifest, &quote).unwrap();
        quote.lock_template_commitment = plan.lock_template_commitment();

        let payout: ScriptBuf =
            ScriptBuf::from_hex("0014deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![TxOut {
                value: Amount::from_sat(plan.user_sats),
                script_pubkey: payout.clone(),
            }],
        };
        (manifest, quote, payout, serialize(&tx))
    }

    #[test]
    fn valid_settlement_yields_point() {
        let (m, q, payout, tx) = setup();
        let point = verify_rail0_settlement(&m, &q, &payout, &tx).unwrap();
        assert_eq!(point.price_msat_per_usd, 1_000_000);
        assert_eq!(point.volume_micro_usd, 1_000_000_000);
    }

    #[test]
    fn unbound_commitment_refused() {
        let (m, mut q, payout, tx) = setup();
        q.lock_template_commitment = [9; 32];
        assert_eq!(
            verify_rail0_settlement(&m, &q, &payout, &tx).unwrap_err(),
            SettlementError::CommitmentMismatch
        );
    }

    #[test]
    fn wrong_payout_refused() {
        let (m, q, _payout, tx) = setup();
        let other = ScriptBuf::from_hex("0014ffffffffffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(
            verify_rail0_settlement(&m, &q, &other, &tx).unwrap_err(),
            SettlementError::PayoutMissing
        );
    }

    #[test]
    fn settlements_feed_the_marker() {
        // The whole pipeline: verified settlements → internal_twap.
        use crate::marker::internal_twap;
        let (m, q, payout, tx) = setup();
        let p1 = verify_rail0_settlement(&m, &q, &payout, &tx).unwrap();
        assert_eq!(internal_twap(&[p1, p1, p1], 0), Some(1_000_000));
    }
}
