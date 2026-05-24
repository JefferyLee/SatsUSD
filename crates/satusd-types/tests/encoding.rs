use satusd_types::derive;
use satusd_types::domain;
use satusd_types::encoding::canonical_encode;
use satusd_types::types::*;

fn sample_intent() -> RedeemIntent {
    RedeemIntent {
        version: 1,
        network: 0,
        redemption_id: [1; 32],
        satusd_asset_family_id: [2; 32],
        amount_satusd_atoms: 10_000,
        user_btc_refund_pubkey: [3; 32],
        user_btc_claim_pubkey: [4; 32],
        user_asset_refund_key: [5; 32],
        operator_id: Some([6; 32]),
        mode: 0,
        payment_hash: [7; 32],
        asset_lock_csv_delta: 288,
        btc_htlc_csv_delta: 144,
        max_operator_fee_bps: 50,
        l1_anchor_height: 900_000,
        l1_anchor_hash: [8; 32],
        expiry_height: 900_144,
        nonce: [9; 32],
    }
}

#[test]
fn encoding_is_deterministic() {
    let intent = sample_intent();
    assert_eq!(canonical_encode(&intent), canonical_encode(&intent));
}

#[test]
fn optional_presence_flag_changes_length() {
    let mut intent = sample_intent();
    let with_some = canonical_encode(&intent).len();
    intent.operator_id = None;
    let with_none = canonical_encode(&intent).len();
    // Some adds a 1-byte flag + 32-byte value; None adds only the 1-byte flag.
    assert_eq!(with_some, with_none + 32);
}

#[test]
fn intent_hash_is_field_sensitive() {
    let intent = sample_intent();
    let base = derive::redeem_intent_hash(&intent);
    let mut tweaked = intent.clone();
    tweaked.amount_satusd_atoms += 1;
    assert_ne!(base, derive::redeem_intent_hash(&tweaked));
}

#[test]
fn domain_tag_is_raw_ascii() {
    assert_eq!(
        domain::domain_tag(domain::REDEEM_INTENT),
        b"SATUSD_REDEEM_INTENT_V1"
    );
}

#[test]
fn all_domain_separators_unique() {
    let mut seen = std::collections::HashSet::new();
    for name in domain::ALL {
        assert!(seen.insert(*name), "duplicate domain separator {name}");
    }
}

#[test]
fn claim_id_ignores_claim_id_and_signature_fields() {
    let mut claim = ReserveClaim {
        claim_id: [0; 32],
        transition_type: 0x11,
        operator_id: [1; 32],
        prev_state_root: [2; 32],
        new_state_root: [3; 32],
        redemption_batch_root: [4; 32],
        oracle_batch_root: [5; 32],
        lock_batch_root: [6; 32],
        payout_batch_root: [7; 32],
        confirmation_batch_root: [8; 32],
        finalize_batch_root: [9; 32],
        burn_proof_batch_root: [10; 32],
        lineage_proof_batch_root: [11; 32],
        live_da_root: [12; 32],
        archival_da_root: [13; 32],
        l1_anchor: ClaimClock {
            l1_anchor_height: 1,
            l1_anchor_hash: [14; 32],
            l1_anchor_mtp: 2,
            l1_anchor_chain_time: 3,
            recent_header_chain: [[15; 80]; 12],
            oracle_epoch: 4,
            selected_oracle_price_e8: 5,
            max_epoch_lag_sec: 6,
            oracle_future_tolerance: 7,
        },
        reserve_shard_id: 0,
        reimbursement_sats: 100,
        proof_commitment: [16; 32],
        claim_expiry_height: 900_144,
        operator_signature: [17; 64],
    };
    let base = derive::claim_id(&claim);

    claim.claim_id = [0xaa; 32];
    claim.operator_signature = [0xbb; 64];
    assert_eq!(
        base,
        derive::claim_id(&claim),
        "claim_id must ignore claim_id/signature"
    );

    claim.reimbursement_sats += 1;
    assert_ne!(
        base,
        derive::claim_id(&claim),
        "claim_id must cover body fields"
    );
}
