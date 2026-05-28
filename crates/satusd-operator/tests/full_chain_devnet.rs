//! Full single-run regtest E2E (M6, in-env half of the signet pilot):
//! real BTC deposit → state-node mint → redeem_lock (real captured lineage) → real
//! operator BTC HTLC + user claim → real SPV from bitcoind → submit_claim → committee
//! 3-of-5 approve → finalize_claim → real 3-of-5 P2WSH **reserve spend** pays the
//! operator. Every BTC operation is on the live devnet bitcoind; the asset-layer
//! lineage uses the captured tapd proofs (lock_anchor.hex, burn_to_sink.hex), so the
//! grouped SatUSD asset is not re-minted here.
//!
//! `#[ignore]` — opt-in (needs `make devnet-up`):
//!   cargo test --test full_chain_devnet -p satusd-operator -- --ignored
//!
//! On signet this is the same code with `Network::Signet` and a signet bitcoind.

use std::slice;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::key::Keypair;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::{
    absolute::LockTime, transaction::Version, Address, Amount, Network, OutPoint as BtcOutPoint,
    PublicKey, Sequence, TapLeafHash, Transaction, TxIn, TxOut, Witness, XOnlyPublicKey,
};
use bitcoincore_rpc::{Auth, Client, RpcApi};

use satusd_crypto::nums::{derive_nums_key, tap_tweak};
use satusd_operator::{
    build_btc_htlc, build_payout_confirmation,
    reserve_spend::{
        build_reserve_spend, reserve_address, reserve_witness_script, sign_reserve_spend,
    },
};
use satusd_reserve::Committee;
use satusd_state::node::StateNode;
use satusd_state::{claim, mint, redeem, registry};
use satusd_types::derive::{
    lineage_proof_hash, lock_record_hash, lock_tweak, oracle_set_hash, redeem_intent_hash,
    reserve_committee_hash,
};
use satusd_types::types::{
    ClaimClock, IssuerPosition, IssuerStatus, LockFinalizeRecord, LockRecord, OracleMessage,
    OutPoint, RedeemIntent,
};

// ---- Constants (must match the captured fixtures + the canonical intent) --------

const FAMILY: [u8; 32] = [
    0x0c, 0x58, 0x77, 0x1b, 0xaf, 0x09, 0x1f, 0xbc, 0xea, 0xdf, 0x1c, 0x22, 0x39, 0x4e, 0x9e, 0x72,
    0xad, 0x91, 0xc6, 0xa1, 0x35, 0xd7, 0xbf, 0x78, 0x30, 0x08, 0x62, 0xa0, 0x63, 0xc4, 0xbf, 0x9b,
];
const ISSUER_ID: [u8; 32] = [0xaa; 32];
const OPERATOR: [u8; 32] = [0x20; 32];
const META: [u8; 32] = [0xef; 32];
const PRICE_50K: u64 = 5_000_000_000_000;
const ORACLE_MTP: u64 = 1_699_996_400;
const REQUESTED_MINT_ATOMS: u64 = 100_000_000;

// ---- Issuer + committee + oracle helpers (deterministic, replicate node-test fixtures) ----

fn issuer_keys(secp: &Secp256k1<bitcoin::secp256k1::All>) -> [(SecretKey, [u8; 33]); 3] {
    [[0x11u8; 32], [0x22; 32], [0x33; 32]].map(|raw| {
        let sk = SecretKey::from_slice(&raw).unwrap();
        let pk = PublicKey::new(sk.public_key(secp)).inner.serialize();
        (sk, pk)
    })
}

/// 5 deterministic committee keys (governance approval + reserve spend share keys).
fn committee_keys(secp: &Secp256k1<bitcoin::secp256k1::All>) -> Vec<(SecretKey, PublicKey)> {
    (1..=5u8)
        .map(|i| {
            let mut s = sha256::Hash::hash(format!("satusd-reserve-committee-{i}").as_bytes())
                .to_byte_array();
            loop {
                if let Ok(sk) = SecretKey::from_slice(&s) {
                    return (sk, PublicKey::new(sk.public_key(secp)));
                }
                s = sha256::Hash::hash(&s).to_byte_array();
            }
        })
        .collect()
}

fn oracle_pubkeys() -> Vec<[u8; 32]> {
    vec![
        hex32("323a1772ccd2bf78ca0f82e4de1d4d48ded87f6f26d92d6a99e5998ac88901a6"),
        hex32("fff67c4b050994bb6d9e1a482edb649dab97b1bff12b73ae97af37487d828180"),
        hex32("b54e5bf89ce35b522a33efcb1d87a3ae81754b36a5aa1bcb87c5bedd8986bd14"),
    ]
}

/// EdDSA signatures from the satusd-state node-test fixture (pre-computed via
/// circomlibjs over `oracle_message_hash(7, 100, 1.7e12, 5e12)`).
fn oracle_messages() -> Vec<OracleMessage> {
    let sigs = [
        hex64("e1c966e0d52d5f5b20161c5b653101c10c7935521980770d838d826fbc93c42e35351a47e0b0d02009c15179e144ba6780244d10d391fbda0d5411f7b8562a02"),
        hex64("149b0d3019084f8537bce93c981e68c812c663df98c81ba36d1083af6353a694f299684962649aa9c7cc27ae06f0f96f295665f32c7ab817b0ce4e2341ecd104"),
        hex64("804b792bd825a16561f6f3b78f9a6db2949d5e5289d2230dd28a7b23c23e722b066761e4474e02c82d251a2c7ea0b8f34d1c79071bcb9b2b47094b9ceb572204"),
    ];
    oracle_pubkeys()
        .into_iter()
        .zip(sigs)
        .enumerate()
        .map(|(i, (pk, sig))| OracleMessage {
            domain: [0; 32],
            oracle_id: [i as u8; 32],
            oracle_set_epoch: 7,
            price_epoch: 100,
            timestamp_ms: 1_700_000_000_000,
            pair: *b"BTC/USD\0",
            price_e8: PRICE_50K,
            source_commitment: [0; 32],
            signer_pubkey: pk,
            signature: sig,
        })
        .collect()
}

fn submit_claim_clock() -> ClaimClock {
    ClaimClock {
        l1_anchor_height: 840_000,
        l1_anchor_hash: [0x0c; 32],
        l1_anchor_mtp: ORACLE_MTP,
        l1_anchor_chain_time: ORACLE_MTP + claim::MTP_LAG_OFFSET_SEC,
        recent_header_chain: [[0u8; 80]; 12],
        oracle_epoch: 7,
        selected_oracle_price_e8: PRICE_50K,
        max_epoch_lag_sec: 600,
        oracle_future_tolerance: 300,
    }
}

fn lock_vector_intent() -> RedeemIntent {
    RedeemIntent {
        version: 1,
        network: 0,
        redemption_id: [0x77; 32],
        satusd_asset_family_id: FAMILY,
        amount_satusd_atoms: 4_000,
        user_btc_refund_pubkey: [0x31; 32],
        user_btc_claim_pubkey: [0x32; 32],
        user_asset_refund_key: derive_nums_key("satusd-lock-vector-user", &[]),
        operator_id: Some(OPERATOR),
        mode: redeem::MODE_FAST_OPERATOR,
        payment_hash: satusd_types::sha256(&[&[0x55u8; 32]]),
        asset_lock_csv_delta: 288,
        btc_htlc_csv_delta: 144,
        max_operator_fee_bps: 50,
        l1_anchor_height: 840_000,
        l1_anchor_hash: [0x33; 32],
        expiry_height: 900_000,
        nonce: [0x34; 32],
    }
}

fn lock_anchor_vector() -> (Vec<u8>, [u8; 32], u32) {
    let bytes =
        hex::decode(include_str!("../../../integration/lineage_vectors/lock_anchor.hex").trim())
            .unwrap();
    let f = satusd_ta_proof::parse_proof_file(&bytes).unwrap();
    let head = satusd_ta_proof::verify_lineage(&f.parsed().unwrap()).unwrap();
    (bytes, head.txid, head.output_index)
}

fn burn_vector() -> (Vec<u8>, [u8; 32], u32) {
    let bytes =
        hex::decode(include_str!("../../../integration/lineage_vectors/burn_to_sink.hex").trim())
            .unwrap();
    let f = satusd_ta_proof::parse_proof_file(&bytes).unwrap();
    let head = satusd_ta_proof::verify_lineage(&f.parsed().unwrap()).unwrap();
    (bytes, head.txid, head.output_index)
}

fn commit_witness(
    secp: &Secp256k1<bitcoin::secp256k1::All>,
    deposit_txid: [u8; 32],
    deposit_confirmation: satusd_types::types::BtcDepositConfirmation,
    reserve_committee_pubkeys: Vec<[u8; 33]>,
    reserve_committee_threshold: u8,
) -> mint::MintCommitWitness {
    let deposit_sats = 4_000_000_000u64;
    let sighash = satusd_types::derive::mint_request_sighash(
        &ISSUER_ID,
        REQUESTED_MINT_ATOMS,
        deposit_sats,
        &deposit_txid,
        &META,
    );
    let signatures = issuer_keys(secp)[..2]
        .iter()
        .map(|(sk, pk)| mint::MultisigSig {
            signer_pubkey: *pk,
            signature: secp
                .sign_ecdsa(&Message::from_digest(sighash), sk)
                .serialize_compact(),
        })
        .collect();
    mint::MintCommitWitness {
        issuer_id: ISSUER_ID,
        requested_mint_atoms: REQUESTED_MINT_ATOMS,
        deposit_txid,
        deposit_sats,
        asset_metadata_commitment: META,
        signatures,
        oracle_price_e8: PRICE_50K,
        deposit_confirmation,
        reserve_committee_pubkeys,
        reserve_committee_threshold,
    }
}

fn issuer() -> IssuerPosition {
    let secp = Secp256k1::new();
    IssuerPosition {
        issuer_id: ISSUER_ID,
        status: IssuerStatus::Active,
        multisig_pubkeys: issuer_keys(&secp).iter().map(|(_, pk)| *pk).collect(),
        multisig_threshold: 2,
        reserve_deposits_sats: 0,
        minted_satusd_atoms: 0,
        pending_mint_atoms: 0,
        collateral_ratio_ppm: 0,
        last_deposit_txid: None,
        freeze_reason: None,
        registered_at_height: 100,
        pending_mint_commitment: None,
    }
}

fn hex32(s: &str) -> [u8; 32] {
    hex::decode(s).unwrap().try_into().unwrap()
}
fn hex64(s: &str) -> [u8; 64] {
    hex::decode(s).unwrap().try_into().unwrap()
}

// ---- The full chain ------------------------------------------------------------

#[test]
#[ignore]
fn full_chain_regtest_e2e() {
    let secp = Secp256k1::new();
    let btc = Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        Auth::UserPass("satusd".into(), "satusd".into()),
    )
    .expect("devnet bitcoind reachable");
    let miner = btc.get_new_address(None, None).unwrap().assume_checked();

    // ---- Committee (3-of-5) + P2WSH reserve address ----
    let comm = committee_keys(&secp);
    let comm_pks_btc: Vec<PublicKey> = comm.iter().map(|(_, pk)| *pk).collect();
    let comm_pks_state: Vec<[u8; 33]> =
        comm_pks_btc.iter().map(|pk| pk.inner.serialize()).collect();
    let ws = reserve_witness_script(&comm_pks_btc, 3);
    let reserve = reserve_address(&ws, Network::Regtest);
    let reserve_spk = reserve.script_pubkey();
    println!("[1] reserve 3-of-5 P2WSH: {reserve}");

    // ---- Real BTC deposit into the reserve ----
    let deposit_sats = 4_000_000_000u64;
    let deposit_txid_btc = btc
        .send_to_address(
            &reserve,
            Amount::from_sat(deposit_sats),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    // Bury the deposit ≥ 6 blocks so the in-state SPV verifier (post-MVP
    // tightening of the I-01 witness-fact check) accepts it.
    btc.generate_to_address(6, &miner).unwrap();
    let deposit_tx = btc.get_raw_transaction(&deposit_txid_btc, None).unwrap();
    let reserve_vout = deposit_tx
        .output
        .iter()
        .position(|o| o.script_pubkey == reserve_spk)
        .unwrap() as u32;
    let deposit_txid: [u8; 32] = deposit_txid_btc.to_byte_array();
    println!("[2] BTC deposit → reserve: {deposit_txid_btc}:{reserve_vout} ({deposit_sats} sat)");

    // ---- State node with MultisigReserve backend ----
    let mut node = StateNode::genesis(FAMILY, oracle_set_hash(7, &oracle_pubkeys()), 7, PRICE_50K);
    node.set_l1_anchor(840_000, [0x0c; 32], ORACLE_MTP);
    node.use_multisig_reserve(Committee {
        threshold: 3,
        pubkeys: comm_pks_state.clone(),
    });
    assert_eq!(
        node.state().reserve_committee_hash,
        reserve_committee_hash(3, &comm_pks_state)
    );

    // ---- issuer_register + mint with the REAL deposit txid ----
    node.issuer_register(issuer()).unwrap();
    // Build the deposit SPV proof from bitcoind (the real-bitcoind dual of the
    // synthetic `crate::spv::build_deposit_confirmation` test helper).
    let deposit_conf = satusd_operator::build_deposit_confirmation(&btc, deposit_txid, 6)
        .expect("build_deposit_confirmation");
    node.mint_commit(
        ISSUER_ID,
        &commit_witness(&secp, deposit_txid, deposit_conf, comm_pks_state.clone(), 3),
    )
    .unwrap();
    node.mint_finalize(
        ISSUER_ID,
        &mint::MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: REQUESTED_MINT_ATOMS,
            deposit_txid,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        },
    )
    .unwrap();
    let reserve_before = node.state().reserve_btc_sats;
    println!(
        "[3] mint: supply={} reserve_btc={}",
        node.state().sat_usd_supply_atoms,
        reserve_before
    );

    // ---- redeem_lock against the real captured lineage ----
    let it = lock_vector_intent();
    let rih = redeem_intent_hash(&it);
    let (lock_bytes, lock_txid, lock_vout) = lock_anchor_vector();
    let lock = LockRecord {
        lock_record_version: 1,
        redeem_intent_hash: rih,
        lock_anchor_outpoint: OutPoint {
            txid: lock_txid,
            vout: lock_vout,
        },
        lock_anchor_txid: lock_txid,
        lock_script_key: tap_tweak(
            &it.user_asset_refund_key,
            &lock_tweak(&rih, &it.payment_hash),
        ),
        lock_amount_atoms: it.amount_satusd_atoms,
        asset_family_id: FAMILY,
        asset_lock_csv_delta: it.asset_lock_csv_delta,
        payment_hash: it.payment_hash,
        lineage_proof_hash: lineage_proof_hash(&lock_bytes),
        lineage_verified_by: vec![],
        anchor_inclusion_height: 1,
    };
    node.redeem_lock(redeem::RedeemLockWitness {
        redeem_intent: it.clone(),
        lock_record: lock.clone(),
        lock_exclusion_proof: vec![],
        lineage_proof: lock_bytes,
    })
    .expect("redeem_lock w/ real lineage");
    println!("[4] redeem_lock: real lineage verified (§5.D15)");

    // ---- Real BTC HTLC: operator funds, user claims with preimage ----
    let preimage = [0x55u8; 32];
    let payment_hash: [u8; 32] = sha256::Hash::hash(&preimage).to_byte_array();
    assert_eq!(payment_hash, it.payment_hash);
    let user = det_keypair(&secp, "fcd-user");
    let oper = det_keypair(&secp, "fcd-operator");
    let user_xonly: XOnlyPublicKey = user.x_only_public_key().0;
    let oper_xonly: XOnlyPublicKey = oper.x_only_public_key().0;
    let h = build_btc_htlc(
        &payment_hash,
        user_xonly,
        oper_xonly,
        it.btc_htlc_csv_delta as i64,
    );

    let htlc_spk = h.script_pubkey();
    let htlc_addr = Address::from_script(&htlc_spk, Network::Regtest).unwrap();
    let htlc_value = Amount::from_sat(100_000); // operator covers reimbursement+fee
    let fund_txid = btc
        .send_to_address(&htlc_addr, htlc_value, None, None, None, None, None, None)
        .unwrap();
    btc.generate_to_address(1, &miner).unwrap();
    let fund_tx = btc.get_raw_transaction(&fund_txid, None).unwrap();
    let htlc_vout = fund_tx
        .output
        .iter()
        .position(|o| o.script_pubkey == htlc_spk)
        .unwrap() as u32;
    let htlc_outpoint = BtcOutPoint {
        txid: fund_txid,
        vout: htlc_vout,
    };

    // user claim tx (witness: <sig> <preimage> <claim_script> <control_block>)
    let dest = btc.get_new_address(None, None).unwrap().assume_checked();
    let mut claim_tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: htlc_outpoint,
            script_sig: Default::default(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: htlc_value - Amount::from_sat(500),
            script_pubkey: dest.script_pubkey(),
        }],
    };
    let prevout = TxOut {
        value: htlc_value,
        script_pubkey: htlc_spk.clone(),
    };
    let leaf = TapLeafHash::from_script(&h.claim_script, LeafVersion::TapScript);
    let sighash = SighashCache::new(&claim_tx)
        .taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(slice::from_ref(&prevout)),
            leaf,
            TapSighashType::Default,
        )
        .unwrap();
    let sig = secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &user);
    let control = h
        .spend_info
        .control_block(&(h.claim_script.clone(), LeafVersion::TapScript))
        .unwrap();
    let mut w = Witness::new();
    w.push(sig.serialize());
    w.push(preimage);
    w.push(h.claim_script.as_bytes());
    w.push(control.serialize());
    claim_tx.input[0].witness = w;
    let claim_txid = btc.send_raw_transaction(&claim_tx).unwrap();
    btc.generate_to_address(1, &miner).unwrap();
    println!("[5] HTLC funded {fund_txid}; user CLAIMED with preimage: {claim_txid}");

    // ---- Observer: build the real BtcPayoutConfirmation from bitcoind ----
    const MIN_DEPTH: usize = 6;
    btc.generate_to_address(MIN_DEPTH as u64, &miner).unwrap();
    let htlc_outpoint_state = OutPoint {
        txid: htlc_outpoint.txid.to_byte_array(),
        vout: htlc_outpoint.vout,
    };
    let confirmation = build_payout_confirmation(
        &btc,
        &payment_hash,
        htlc_outpoint_state,
        claim_txid,
        MIN_DEPTH,
    )
    .unwrap();
    let tip = btc.get_block_count().unwrap() as u32;
    satusd_state::spv::verify_payout_confirmation(&confirmation, &payment_hash, MIN_DEPTH, tip)
        .expect("real SPV confirmation verifies");
    println!("[6] observer built BtcPayoutConfirmation; SPV ACCEPTED (R-07/13/14/15)");

    // ---- submit_claim with the real SPV + captured burn proof ----
    let (burn_bytes, burn_txid, burn_vout) = burn_vector();
    let redemption = claim::BatchRedemption {
        redeem_intent: it.clone(),
        lock_record: lock.clone(),
        lock_finalize: LockFinalizeRecord {
            lock_record_hash: lock_record_hash(&lock),
            payment_preimage: preimage,
            finalize_anchor_txid: burn_txid,
            finalize_anchor_outpoint: OutPoint {
                txid: burn_txid,
                vout: burn_vout,
            },
            protocol_sink_script_key: satusd_crypto::nums::protocol_sink_script_key(&FAMILY),
            protocol_burn_internal_key: satusd_crypto::nums::protocol_burn_internal_key(&FAMILY),
            finalized_amount_atoms: lock.lock_amount_atoms,
            operator_id: OPERATOR,
            finalize_height: 840_002,
            universe_burn_proof_hash: lineage_proof_hash(&burn_bytes),
        },
        payout_confirmation: confirmation,
        lock_membership_proof: vec![],
        consumed_exclusion_proof: vec![],
        refund_exclusion_proof: vec![],
        nullifier_exclusion_proof: vec![],
        burn_proof: burn_bytes,
    };
    let claim_id = node
        .submit_claim(
            vec![redemption],
            oracle_messages(),
            oracle_pubkeys(),
            submit_claim_clock(),
            0,
            840_100,
            tip,
        )
        .expect("submit_claim accepted");
    let reimbursement = 80_000u64;
    assert_eq!(node.state().reserved_pending_claim_sats, reimbursement);
    println!(
        "[7] submit_claim ACCEPTED: claim_id={}, reserved={reimbursement} sat",
        hex::encode(claim_id)
    );

    // ---- Committee approves (3-of-5 ECDSA over claim_id, same 5 keys) ----
    let approvals: Vec<mint::MultisigSig> = comm[..3]
        .iter()
        .map(|(sk, pk)| mint::MultisigSig {
            signer_pubkey: pk.inner.serialize(),
            signature: secp
                .sign_ecdsa(&Message::from_digest(claim_id), sk)
                .serialize_compact(),
        })
        .collect();
    let reserve_committee = registry::ReserveCommittee {
        threshold: 3,
        pubkeys: comm_pks_state.clone(),
    };
    node.finalize_claim(claim_id, &reserve_committee, &approvals, 840_050)
        .expect("finalize_claim (consensus + backend)");
    assert_eq!(
        node.state().reserve_btc_sats,
        reserve_before - reimbursement
    );
    println!("[8] finalize_claim: state debited {reimbursement} sat (consensus + backend OK)");

    // ---- Realize the ReserveSpend as a real on-chain 3-of-5 P2WSH spend ----
    let operator_payout = btc.get_new_address(None, None).unwrap().assume_checked();
    let fee = 1_000u64;
    let change = deposit_sats - reimbursement - fee;
    let reserve_utxo = BtcOutPoint {
        txid: deposit_txid_btc,
        vout: reserve_vout,
    };
    let mut spend =
        build_reserve_spend(reserve_utxo, operator_payout.script_pubkey(), reimbursement);
    spend.output.push(TxOut {
        value: Amount::from_sat(change),
        script_pubkey: reserve_spk.clone(),
    });
    let signers: Vec<(usize, SecretKey)> = comm[..3]
        .iter()
        .enumerate()
        .map(|(i, (sk, _))| (i, *sk))
        .collect();
    sign_reserve_spend(&mut spend, &ws, deposit_sats, signers);
    let spend_txid = btc.send_raw_transaction(&spend).unwrap();
    btc.generate_to_address(1, &miner).unwrap();
    let got = btc
        .get_received_by_address(&operator_payout, Some(1))
        .unwrap();
    assert_eq!(got.to_sat(), reimbursement);
    println!(
        "[9] real 3-of-5 reserve spend {spend_txid}: operator paid {} sat ✓",
        got.to_sat()
    );

    println!(
        "\n★ full chain regtest E2E closed: deposit → mint → redeem → finalize → reserve-spend."
    );
}

fn det_keypair(secp: &Secp256k1<bitcoin::secp256k1::All>, label: &str) -> Keypair {
    let mut s = sha256::Hash::hash(label.as_bytes()).to_byte_array();
    loop {
        if let Ok(sk) = SecretKey::from_slice(&s) {
            return Keypair::from_secret_key(secp, &sk);
        }
        s = sha256::Hash::hash(&s).to_byte_array();
    }
}
